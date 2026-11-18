#![allow(dead_code)]
#![recursion_limit = "512"]

mod climax;
mod config;
mod effects;
mod midi;
mod player;
mod playlist;
mod util;

use climax::ClimaxCfg;
use config::Config;
use crossbeam_channel::Sender;
use effects::{FrameCtx, FxManager};
use midi::{EvKind, FastMidiReader};
use player::{Active, Cell, Half, Head, Player, Tail};
use playlist::Playlist;
use serde_json::Value;
use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, Instant};
use util::{truncate_ansi, write_synced, ESC};

#[derive(Clone)]
pub struct Lang(Value);
impl Lang {
    /// 空语言表（缺失的文案都会走默认中文），测试和找不到语言文件时用
    pub fn empty() -> Lang {
        Lang(Value::Object(serde_json::Map::new()))
    }
    pub fn s(&self, k: &str, d: &str) -> String {
        self.0.get(k).and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or_else(|| d.to_string())
    }
    pub fn arr(&self, k: &str) -> Option<Vec<String>> {
        self.0.get(k).and_then(|x| x.as_array()).map(|a| {
            a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
        })
    }
}

// 键盘按键高亮在音符结束后继续点亮的时间（秒）：既让短音符不至于一帧亮一帧灭，
// 也保证按住期间颜色稳定（同一音高的多个音轨不会让背景色来回跳）。
const KB_HOLD_SECS: f64 = 0.40;

// 音符配对栈：key 为 (音轨, 通道, 音高)
type StackMap = HashMap<(i32, u8, u8), VecDeque<(f64, u8, u8, u32)>>;
// 第 2 行 header 的快照（已播放音符数 + 各通道控制器状态）
type HeaderSnap = (u64, Vec<(u8, Vec<(u8, u8)>)>);

fn locate(tm: &[(u64, u32)], ticks: u64) -> usize {
    let mut lo = 0usize;
    let mut hi = tm.len() - 1;
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if tm[mid].0 <= ticks {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}
fn ticks_to_sec(tm: &[(u64, u32)], seg: &[f64], tpb: f64, ticks: u64) -> f64 {
    let l = locate(tm, ticks);
    let (st, t) = tm[l];
    seg[l] + (ticks - st) as f64 * t as f64 / tpb / 1_000_000.0
}
fn tempo_at(tm: &[(u64, u32)], ticks: u64) -> u32 {
    tm[locate(tm, ticks)].1
}

// ---------- 消息 ----------
#[derive(Clone)]
pub struct HeaderInfo {
    pub filename: String,
    pub n_tracks: usize,
    pub ticks_per_beat: u16,
    pub tempo_changes: Vec<(u64, u32)>,
    pub programs: Vec<u8>,
    pub channels_used: Vec<u8>,
    pub tracks_used: Vec<usize>,
    pub pitch_hist: [u64; 12],
    pub weighted_hist: [f64; 12],
    pub bass_pc: Option<i32>,
    pub first_pc: Option<i32>,
    pub key_events: Vec<(f64, i32, u8)>,
    pub climax: Vec<(f64, f64)>,
    pub note_count: u64,
    pub chord_count_est: u64,
    pub min_note: i32,
    pub max_note: i32,
}

#[derive(Clone, Copy)]
struct MetaInfo { first_abs: f64, n_notes: u64 }

#[derive(Clone)]
struct NoteBlock {
    rel_spawn: f64,
    note: u8,
    velocity: u8,
    track: i32,
    channel: u8,
    duration: f64,
    drop_time: f64,
    trail_rows: f64,
    use_trail_for_off: bool,
    pos: f64,
    steps: i32,
    frac: f64,
    triggered: bool,
    id: u64,
}

#[derive(Clone)]
struct CtrlBlock { rel_time: f64, bytes: Vec<u8> }

#[allow(clippy::large_enum_variant)] // Notes 一个批次就是几十 KB，装箱收益不大
enum Msg {
    Header(HeaderInfo),
    Meta(MetaInfo),
    Stage { name: String, cur: u64, total: u64 },
    Notes(Vec<NoteBlock>),
    Ctrls(Vec<CtrlBlock>),
    Eof,
    Error(String),
}

struct ProducerCfg {
    climax: ClimaxCfg,
    beats_for_drop: f64,
    drop_follows_tempo: bool,
    min_drop_floor: f64,
    min_note_opt: Option<i32>,
    max_note_opt: Option<i32>,
    default_tempo: u32,
}

fn producer(path: String, tx: Sender<Msg>, pcfg: ProducerCfg) {
    if let Err(e) = run_producer(&path, &tx, &pcfg) {
        let _ = tx.send(Msg::Error(e));
    }
}

fn run_producer(path: &str, tx: &Sender<Msg>, pcfg: &ProducerCfg) -> Result<(), String> {
    let _ = tx.send(Msg::Stage { name: "loading".into(), cur: 0, total: 1 });
    let reader = FastMidiReader::open(path)?;
    let tpb = reader.ticks_per_beat as f64;
    let _ = tx.send(Msg::Stage { name: "loading".into(), cur: 1, total: 1 });
    let meta = reader.scan_meta(|cur, total| {
        let _ = tx.send(Msg::Stage { name: "scanning".into(), cur: cur as u64, total: total as u64 });
    }, &pcfg.climax);

    let (min_note, max_note) = match (pcfg.min_note_opt, pcfg.max_note_opt) {
        (Some(a), Some(b)) => (a, b),
        _ => ((meta.min_note as i32 - 3).max(0), (meta.max_note as i32 + 3).min(127)),
    };

    let filename = Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());

    let mut tempo_map: Vec<(u64, u32)> = Vec::new();
    for &(tick, tempo) in &meta.tempo_changes {
        if let Some(last) = tempo_map.last_mut() {
            if last.0 == tick {
                *last = (tick, tempo);
                continue;
            }
        }
        tempo_map.push((tick, tempo));
    }
    if tempo_map.is_empty() {
        tempo_map.push((0, pcfg.default_tempo));
    }
    let mut seg: Vec<f64> = vec![0.0];
    for i in 1..tempo_map.len() {
        let (pt, ptm) = tempo_map[i - 1];
        let ct = tempo_map[i].0;
        seg.push(seg[i - 1] + (ct - pt) as f64 * ptm as f64 / tpb / 1_000_000.0);
    }

    let first_tempo = tempo_map[0].1;
    let fixed_drop = (pcfg.beats_for_drop * first_tempo as f64 / 1_000_000.0).max(0.1);
    let calc_drop = |tempo: u32| -> f64 {
        let raw = if pcfg.drop_follows_tempo {
            pcfg.beats_for_drop * tempo as f64 / 1_000_000.0
        } else {
            fixed_drop
        };
        raw.max(pcfg.min_drop_floor).max(0.05)
    };

    let first_note_ticks = meta.first_note_abs_ticks;
    let first_note_sec = ticks_to_sec(&tempo_map, &seg, tpb, first_note_ticks);
    let first_note_tempo = tempo_at(&tempo_map, first_note_ticks);
    let first_abs = (first_note_sec - calc_drop(first_note_tempo)).max(0.0);

    let mut key_events: Vec<(f64, i32, u8)> = Vec::new();
    for &(tick, sf, mi) in &meta.key_sigs {
        let t = (ticks_to_sec(&tempo_map, &seg, tpb, tick) - first_abs).max(0.0);
        key_events.push((t, sf, mi));
    }
    key_events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let climax: Vec<(f64, f64)> = meta.climax.iter()
        .map(|&(t, s)| ((t - first_abs).max(0.0), s))
        .collect();

    let header = HeaderInfo {
        filename,
        n_tracks: reader.track_ranges.len(),
        ticks_per_beat: reader.ticks_per_beat,
        tempo_changes: meta.tempo_changes.clone(),
        programs: meta.programs.clone(),
        channels_used: meta.channels_used.clone(),
        tracks_used: meta.tracks_used.clone(),
        pitch_hist: meta.pitch_hist,
        weighted_hist: meta.weighted_hist,
        bass_pc: meta.bass_pc,
        first_pc: meta.first_pc,
        key_events,
        climax,
        note_count: meta.note_count,
        chord_count_est: meta.chord_count_est,
        min_note,
        max_note,
    };
    let _ = tx.send(Msg::Header(header));
    let _ = tx.send(Msg::Meta(MetaInfo { first_abs, n_notes: meta.note_count }));

    let mut stacks: StackMap = HashMap::new();
    let mut note_batch: Vec<NoteBlock> = Vec::new();
    let mut ctrl_batch: Vec<CtrlBlock> = Vec::new();

    for ev in reader.iter_events() {
        let abs = ev.abs_ticks;
        let abs_sec = ticks_to_sec(&tempo_map, &seg, tpb, abs);
        match ev.kind {
            EvKind::NoteOn => {
                let tempo = tempo_at(&tempo_map, abs);
                stacks.entry((ev.track as i32, ev.channel, ev.note))
                    .or_default()
                    .push_back((abs_sec, ev.velocity, ev.channel, tempo));
            }
            EvKind::NoteOff => {
                let key = (ev.track as i32, ev.channel, ev.note);
                if let Some(st) = stacks.get_mut(&key) {
                    if let Some((on_sec, vel, ch, tempo)) = st.pop_front() {
                        let duration = (abs_sec - on_sec).max(0.001);
                        let n = ev.note as i32;
                        if min_note <= n && n <= max_note {
                            let drop_time = calc_drop(tempo);
                            let rel_spawn = (on_sec - drop_time) - first_abs;
                            note_batch.push(NoteBlock {
                                rel_spawn,
                                note: ev.note,
                                velocity: vel,
                                track: ev.track as i32,
                                channel: ch,
                                duration,
                                drop_time,
                                trail_rows: 0.0,
                                use_trail_for_off: false,
                                pos: 0.0,
                                steps: 0,
                                frac: 0.0,
                                triggered: false,
                                id: 0,
                            });
                            if note_batch.len() >= 100 {
                                let b = std::mem::take(&mut note_batch);
                                let _ = tx.send(Msg::Notes(b));
                            }
                        }
                    }
                }
            }
            EvKind::ProgramChange => {
                ctrl_batch.push(CtrlBlock {
                    rel_time: abs_sec - first_abs,
                    bytes: vec![0xC0 | (ev.channel & 0x0F), ev.program & 0x7F],
                });
            }
            EvKind::ControlChange => {
                ctrl_batch.push(CtrlBlock {
                    rel_time: abs_sec - first_abs,
                    bytes: vec![0xB0 | (ev.channel & 0x0F), ev.control & 0x7F, ev.value & 0x7F],
                });
            }
            EvKind::Pitchwheel => {
                let p = ev.pitch + 0x2000;
                let lo = (p & 0x7F) as u8;
                let hi = ((p >> 7) & 0x7F) as u8;
                ctrl_batch.push(CtrlBlock {
                    rel_time: abs_sec - first_abs,
                    bytes: vec![0xE0 | (ev.channel & 0x0F), lo, hi],
                });
            }
            _ => {}
        }
        if ctrl_batch.len() >= 100 {
            let b = std::mem::take(&mut ctrl_batch);
            let _ = tx.send(Msg::Ctrls(b));
        }
    }

    if !ctrl_batch.is_empty() {
        let _ = tx.send(Msg::Ctrls(ctrl_batch));
    }
    if !note_batch.is_empty() {
        let _ = tx.send(Msg::Notes(note_batch));
    }
    let _ = tx.send(Msg::Eof);
    Ok(())
}


// ---------- 渲染 ----------
#[derive(Clone, Copy, PartialEq)]
struct OF(f64);
impl Eq for OF {}
impl PartialOrd for OF { fn partial_cmp(&self, o: &Self) -> Option<Ordering> { Some(self.cmp(o)) } }
impl Ord for OF { fn cmp(&self, o: &Self) -> Ordering { self.0.partial_cmp(&o.0).unwrap_or(Ordering::Equal) } }

struct HeapNote { rel: f64, seq: u64, blk: NoteBlock }
impl PartialEq for HeapNote { fn eq(&self, o: &Self) -> bool { self.rel == o.rel && self.seq == o.seq } }
impl Eq for HeapNote {}
impl PartialOrd for HeapNote { fn partial_cmp(&self, o: &Self) -> Option<Ordering> { Some(self.cmp(o)) } }
impl Ord for HeapNote {
    fn cmp(&self, o: &Self) -> Ordering {
        // 反序比较，让 BinaryHeap 变成按 rel 升序的小顶堆
        o.rel.partial_cmp(&self.rel).unwrap_or(Ordering::Equal).then_with(|| o.seq.cmp(&self.seq))
    }
}

fn find_file(rel: &str) -> Option<String> {
    if Path::new(rel).exists() { return Some(rel.to_string()); }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join(rel);
            if p.exists() { return Some(p.to_string_lossy().to_string()); }
        }
    }
    None
}

/// 以中文文案为底，叠加目标语言里已翻译的键。
/// 这样翻译文件只写了部分条目、甚至是空文件时，界面也不会出现空白文案。
fn merge_lang(base: &Value, over: &Value) -> Value {
    let mut m = base.as_object().cloned().unwrap_or_default();
    if let Some(o) = over.as_object() {
        for (k, v) in o {
            let usable = match v {
                Value::String(s) => !s.trim().is_empty(),
                Value::Array(a) => !a.is_empty(),
                _ => false,
            };
            if usable {
                m.insert(k.clone(), v.clone());
            }
        }
    }
    Value::Object(m)
}

fn read_json(rel: &str) -> Option<Value> {
    let p = find_file(rel)?;
    let s = std::fs::read_to_string(&p).ok()?;
    serde_json::from_str(&s).ok()
}

fn read_line() -> String {
    let mut s = String::new();
    let _ = io::stdin().read_line(&mut s);
    s.trim().to_string()
}

fn select_output(lang: &Lang) -> Option<midir::MidiOutputConnection> {
    let midi_out = match midir::MidiOutput::new("midi-piano-roll") {
        Ok(o) => o,
        Err(e) => { println!("{} {}", lang.s("output_failed", "打开MIDI输出失败: "), e); return None; }
    };
    let ports = midi_out.ports();
    if ports.is_empty() {
        println!("{}", lang.s("no_midi_output", "没有找到可用的MIDI输出设备"));
        return None;
    }
    println!("{}", lang.s("available_ports", "可用的MIDI输出设备:"));
    for (i, p) in ports.iter().enumerate() {
        println!("{}: {}", i, midi_out.port_name(p).unwrap_or_default());
    }
    let choice = read_line();
    let idx: usize = if choice.is_empty() { 0 } else { choice.parse().unwrap_or(0) };
    let idx = if idx < ports.len() { idx } else { 0 };
    match midi_out.connect(&ports[idx], "midi-piano-roll") {
        Ok(c) => Some(c),
        Err(e) => { println!("{} {}", lang.s("output_failed", "打开MIDI输出失败: "), e); None }
    }
}

fn progress_line(name: &str, cur: u64, total: u64, lang: &Lang, elapsed: f64) -> String {
    let bar_w = 20usize;
    let frac = if total > 0 { cur as f64 / total as f64 } else { 0.0 };
    let filled = ((bar_w as f64) * frac) as usize;
    let filled = filled.min(bar_w);
    let mut bar = String::new();
    for _ in 0..filled { bar.push('█'); }
    for _ in filled..bar_w { bar.push('░'); }
    let pct = frac * 100.0;
    let label = match name {
        "loading" => lang.s("stage_loading", "加载"),
        "scanning" => lang.s("stage_scanning", "扫描"),
        "building" => lang.s("stage_building", "分析"),
        "pushing" => lang.s("stage_pushing", "收集"),
        _ => name.to_string(),
    };
    if name == "loading" && cur == 0 {
        format!("{} [{}] 已耗时 {:.1}s", label, bar, elapsed)
    } else {
        format!("{} [{}] {}/{} ({:.1}%)", label, bar, cur, total, pct)
    }
}

// MIDI 发送线程：渲染循环只入队，真正的发送在独立线程里做。
// 队列默认无界；config 里 midi_queue_size > 0 时改为有界，满了丢新消息。
struct MidiSink {
    tx: Option<crossbeam_channel::Sender<Vec<u8>>>,
    done: Option<crossbeam_channel::Receiver<()>>,
    handle: Option<std::thread::JoinHandle<()>>,
    bounded: bool,
}

impl MidiSink {
    fn send(&self, bytes: &[u8]) {
        if let Some(tx) = &self.tx {
            let msg = bytes.to_vec();
            if self.bounded {
                let _ = tx.try_send(msg);
            } else {
                let _ = tx.send(msg);
            }
        }
    }

    fn close(&mut self) {
        self.tx.take();
        if let Some(done) = self.done.take() {
            let _ = done.recv_timeout(std::time::Duration::from_millis(1000));
        }
        self.handle.take();
    }
}

impl Drop for MidiSink {
    fn drop(&mut self) {
        self.tx.take();
        self.done.take();
        self.handle.take();
    }
}

fn make_midi_sink(mut out: midir::MidiOutputConnection, queue_size: usize) -> MidiSink {
    let bounded = queue_size > 0;
    let (tx, rx) = if bounded {
        let (t, r) = crossbeam_channel::bounded::<Vec<u8>>(queue_size);
        (t, r)
    } else {
        let (t, r) = crossbeam_channel::unbounded::<Vec<u8>>();
        (t, r)
    };
    let (done_tx, done_rx) = crossbeam_channel::bounded::<()>(1);
    let handle = std::thread::spawn(move || {
        while let Ok(msg) = rx.recv() {
            let _ = out.send(&msg);
        }
        let _ = done_tx.send(());
    });
    MidiSink { tx: Some(tx), done: Some(done_rx), handle: Some(handle), bounded }
}


fn cells_line(cells: &[Cell], dx: i32) -> String {
    let mut s = String::new();
    let len = cells.len();
    let skip = if dx < 0 { ((-dx) as usize).min(len) } else { 0 };
    let end = if dx > 0 { len.saturating_sub(dx as usize) } else { len };
    for _ in 0..dx.max(0) { s.push(' '); }
    let mut last = String::new();
    for &(ref code, ch) in cells.iter().take(end).skip(skip) {
        if *code != last {
            if !last.is_empty() { s.push_str(ESC); s.push_str("[0m"); }
            if !code.is_empty() { s.push_str(code); }
            last = code.clone();
        }
        s.push(ch);
    }
    if !last.is_empty() { s.push_str(ESC); s.push_str("[0m"); }
    s
}

fn play_loop(mut player: Player, sink: &mut MidiSink, midi_path: String) {
    let (tx, rx) = crossbeam_channel::bounded::<Msg>(5000);
    let pcfg = ProducerCfg {
        beats_for_drop: player.cfg.beats_for_drop,
        drop_follows_tempo: player.cfg.drop_follows_tempo,
        min_drop_floor: player.cfg.min_drop_floor,
        min_note_opt: player.cfg.min_note_opt,
        max_note_opt: player.cfg.max_note_opt,
        default_tempo: player.cfg.default_tempo,
        climax: player.cfg.climax.clone(),
    };
    let path = midi_path.clone();
    let handle = std::thread::spawn(move || { producer(path, tx, pcfg); });

    let mut header_displayed = false;
    let mut meta_received = false;
    let mut meta_at: Option<Instant> = None;
    let mut start: Option<Instant> = None;
    let mut producer_finished = false;
    let mut bucket_idx: i64 = -1;
    let mut allowed: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut frame_idx: i64 = 0;
    let mut stage: Option<(String, u64, u64)> = None;
    let mut ctrl_events: Vec<CtrlBlock> = Vec::new();
    let mut ctrl_idx = 0usize;
    let mut heap: BinaryHeap<HeapNote> = BinaryHeap::new();
    let mut seq: u64 = 0;
    let mut spawned: Vec<NoteBlock> = Vec::new();
    let mut active: HashMap<u8, Vec<Active>> = HashMap::new();
    let mut played: u64 = 0;
    let mut pending: BinaryHeap<Reverse<(OF, u8, u8)>> = BinaryHeap::new();
    let mut prev_rows: HashMap<i32, Vec<Cell>> = HashMap::new();
    let mut prev_kb: Option<String> = None;
    let mut kb_hold: HashMap<u8, (f64, i32)> = HashMap::new();
    let mut prev_bg_rows: std::collections::HashSet<i32> = std::collections::HashSet::new();
    let mut prev_shake_rows: std::collections::HashSet<i32> = std::collections::HashSet::new();
    let mut prev_notes_key: Option<Vec<u8>> = None;
    let mut prev_chord_name = String::new();
    let mut prev_chord_notes: Vec<String> = Vec::new();
    let mut prev_chord_display: Option<String> = None;
    let mut prev_chord_time: f64 = 0.0;
    let mut prev_chord_line: Option<String> = None;
    let mut prev_chord_root: i32 = -1;
    let mut prev_progress: Option<String> = None;
    let mut prev_header_snap: Option<HeaderSnap> = None;
    let mut was_shaking = false;

    let refresh = player.cfg.refresh_rate.clamp(30.0, 120.0);
    let frame_interval = 1.0 / refresh;
    let mut fx = FxManager::build(&player.cfg.fx);
    let loop_start = Instant::now();

    write_synced(&format!("{}[2J{}[H", ESC, ESC), player.sync_support);
    let _ = io::stdout().flush();

    loop {
        // 收消息
        let mut drained = 0;
        while drained < 1000 {
            match rx.try_recv() {
                Ok(Msg::Ctrls(b)) => { ctrl_events.extend(b); }
                Ok(Msg::Notes(b)) => {
                    for mut blk in b { blk.id = seq; heap.push(HeapNote { rel: blk.rel_spawn, seq, blk }); seq += 1; }
                }
                Ok(Msg::Eof) => { producer_finished = true; }
                Ok(Msg::Stage { name, cur, total }) => { stage = Some((name, cur, total)); }
                Ok(Msg::Header(h)) => {
                    player.apply_header_info(h);
                    header_displayed = true;
                    player.display_header();
                }
                Ok(Msg::Meta(m)) => { player.total_note_count = m.n_notes; player.first_abs = m.first_abs; meta_received = true; }
                Ok(Msg::Error(e)) => {
                    println!("\n{} {}", player.lang.s("load_failed", "加载失败: "), e);
                    return;
                }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(_) => break,
            }
            drained += 1;
        }

        if !header_displayed {
            if handle.is_finished() && !meta_received {
                println!("\n[错误] 解析进程已退出");
                return;
            }
            let elapsed = loop_start.elapsed().as_secs_f64();
            let line2 = if let Some((name, cur, total)) = &stage {
                progress_line(name, *cur, *total, &player.lang, elapsed)
            } else {
                format!("{}... {:.1}s", player.lang.s("loading", "加载 MIDI 文件"), elapsed)
            };
            let w = player.term_width as usize;
            write_synced(&format!("{}[2;1H{}{}[K", ESC, truncate_ansi(&line2, w, "…"), ESC), player.sync_support);
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }

        if start.is_none() {
            if meta_received {
                let now_t = Instant::now();
                if meta_at.is_none() { meta_at = Some(now_t); }
                let dt = meta_at.unwrap().elapsed().as_secs_f64();
                if dt < 0.3 {
                    let line2 = format!("{} [{}] {} 音符", player.lang.s("loading_done", "准备就绪"), "█".repeat(20), player.total_note_count);
                    let w = player.term_width as usize;
                    write_synced(&format!("{}[2;1H{}{}[K", ESC, truncate_ansi(&line2, w, "…"), ESC), player.sync_support);
                    std::thread::sleep(Duration::from_millis(30));
                    continue;
                }
                start = Some(Instant::now());
                frame_idx = 0;
                let h = player.display_height;
                let _ = h;
                write_synced(&format!("\r{}[K", ESC), player.sync_support);
                let line2 = player.build_header_line2(0, player.total_chord_count);
                let w = player.term_width as usize;
                write_synced(&format!("{}[2;1H{}{}[K", ESC, truncate_ansi(&line2, w, "…"), ESC), player.sync_support);
                prev_header_snap = None;
            } else {
                let line2 = if let Some((name, cur, total)) = &stage {
                    progress_line(name, *cur, *total, &player.lang, 0.0)
                } else {
                    format!("{}... ", player.lang.s("analyzing", "分析 MIDI 文件"))
                };
                let w = player.term_width as usize;
                write_synced(&format!("{}[2;1H{}{}[K", ESC, truncate_ansi(&line2, w, "…"), ESC), player.sync_support);
                std::thread::sleep(Duration::from_millis(30));
            }
            continue;
        }

        let s = start.unwrap();
        let now = s.elapsed().as_secs_f64();

        if !player.key_events.is_empty() {
            let mut new_idx = player.key_idx;
            while new_idx + 1 < player.key_events.len() && player.key_events[new_idx + 1].0 <= now {
                new_idx += 1;
            }
            if new_idx != player.key_idx {
                player.key_idx = new_idx;
                let (_, sf, mi) = player.key_events[new_idx];
                let im = mi == 1;
                let root = if im { ((7 * sf + 9) % 12 + 12) % 12 } else { ((7 * sf) % 12 + 12) % 12 };
                player.set_key_names(root, im, sf, None);
                player.init_layout();
                let line1 = player.build_header_line1();
                let w = player.term_width as usize;
                write_synced(&format!("{}[1;1H{}{}[K", ESC, truncate_ansi(&line1, w, "…"), ESC), player.sync_support);
            }
        }

        // 窗口尺寸变化（用户拖动终端边框）：重建布局 + 整屏重绘
        if frame_idx % 15 == 0 {
            if let Some((w, ht)) = terminal_size::terminal_size() {
                let nw = w.0 as i32;
                let nh = (ht.0 as i32 - 5).max(10);
                if nw != player.term_width || nh != player.display_height {
                    player.term_width = nw;
                    player.display_height = nh;
                    player.init_layout();
                    prev_rows.clear();
                    prev_bg_rows.clear();
                    prev_shake_rows.clear();
                    prev_kb = None;
                    prev_chord_line = None;
                    prev_progress = None;
                    prev_header_snap = None;
                    write_synced(&format!("{}[2J{}[H", ESC, ESC), player.sync_support);
                    player.display_header();
                }
            }
        }

        let abs_sec = player.first_abs + now;
        let beat_marks = player.beat_marks(abs_sec);
        let (beat, bpm) = player.beat_at(abs_sec);
        let beat_phase = beat.fract();
        let bar = (beat / 4.0) as u32;
        let progress = if player.total_note_count > 0 { played as f64 / player.total_note_count as f64 } else { 0.0 };
        let ctx = FrameCtx {
            now,
            frame_dt: frame_interval,
            beat,
            beat_phase,
            bar,
            width: player.display_width,
            height: player.display_height,
            beats_for_drop: player.cfg.beats_for_drop,
            beat_marks,
            climax: player.climax.clone(),
            key_root: player.key_root,
            is_minor: player.is_minor,
            progress,
            bpm,
            chord_name: prev_chord_name.clone(),
            chord_root: prev_chord_root,
        };
        fx.begin_frame(&ctx);

        if player.cfg.max_nps != 0 {
            let bucket = now.floor() as i64;
            if bucket != bucket_idx {
                bucket_idx = bucket;
                if player.cfg.max_nps > 0 {
                    let mut cand: Vec<(u8, u64)> = Vec::new();
                    for blk in &spawned {
                        if blk.triggered { continue; }
                        let tt = blk.rel_spawn + blk.drop_time;
                        if tt < (bucket + 1) as f64 { cand.push((blk.velocity, blk.id)); }
                    }
                    for hn in heap.iter() {
                        let blk = &hn.blk;
                        let tt = blk.rel_spawn + blk.drop_time;
                        if tt < (bucket + 1) as f64 { cand.push((blk.velocity, blk.id)); }
                    }
                    cand.sort_by_key(|x| std::cmp::Reverse(x.0));
                    allowed.clear();
                    for &(_, id) in cand.iter().take(player.cfg.max_nps as usize) { allowed.insert(id); }
                } else {
                    allowed.clear();
                }
            }
        }

        let h = player.display_height;
        let hf = h as f64;
        let dual_line = player.cfg.dual_line_interp;
        let half_cell = dual_line && player.cfg.half_cell_notes;

        // 控制事件
        while ctrl_idx < ctrl_events.len() {
            let rel = ctrl_events[ctrl_idx].rel_time;
            if now >= rel {
                let bytes = ctrl_events[ctrl_idx].bytes.clone();
                sink.send(&bytes);
                player.update_effect_from_bytes(&bytes);
                ctrl_idx += 1;
            } else { break; }
        }

        // 生成音符
        while let Some(top) = heap.peek() {
            if top.rel <= now {
                let hn = heap.pop().unwrap();
                let mut blk = hn.blk;
                if player.cfg.trail_by_note {
                    blk.trail_rows = (blk.duration * hf / blk.drop_time).max(0.0);
                    blk.use_trail_for_off = true;
                } else {
                    blk.trail_rows = player.cfg.trail_length as f64;
                    blk.use_trail_for_off = false;
                }
                blk.pos = 0.0;
                spawned.push(blk);
            } else { break; }
        }

        // 更新下落
        let mut new_spawned: Vec<NoteBlock> = Vec::with_capacity(spawned.len());
        for mut blk in spawned.drain(..) {
            let dt = blk.drop_time;
            let pos = (now - blk.rel_spawn) / dt * hf;
            let key = if player.color_key == "channel" { blk.channel as i32 } else { blk.track };

            if pos >= hf && !blk.triggered {
                blk.triggered = true;
                let play = if player.cfg.max_nps == 0 { true }
                    else if player.cfg.max_nps > 0 { allowed.contains(&blk.id) }
                    else { false };
                if play {
                    sink.send(&[0x90 | (blk.channel & 0x0F), blk.note, blk.velocity]);
                    let col = player.note_pos_map.get(&(blk.note as i32)).copied().unwrap_or(0);
                    fx.on_note_on(blk.note, blk.velocity, col, &ctx);
                    if !blk.use_trail_for_off {
                        pending.push(Reverse((OF(now + blk.duration), blk.channel, blk.note)));
                    }
                }
                active.entry(blk.note).or_default().push(Active { end_time: now + blk.duration, track: blk.track, channel: blk.channel, key });
                played += 1;
            }

            if blk.use_trail_for_off {
                let tr = blk.trail_rows.max(0.0);
                if pos >= hf + tr {
                    if blk.triggered {
                        sink.send(&[0x80 | (blk.channel & 0x0F), blk.note, 0]);
                        if let Some(list) = active.get_mut(&blk.note) {
                            list.retain(|a| !(a.track == blk.track && a.channel == blk.channel));
                        }
                        if active.get(&blk.note).map(|v| v.is_empty()).unwrap_or(false) {
                            active.remove(&blk.note);
                        }
                    }
                    continue;
                }
            } else if pos >= hf {
                continue;
            }

            blk.pos = pos;
            if pos < hf {
                blk.steps = pos as i32;
                blk.frac = pos - (pos as i32) as f64;
            } else {
                blk.steps = h;
                blk.frac = 0.0;
            }
            new_spawned.push(blk);
        }
        spawned = new_spawned;

        // 关音
        while let Some(&Reverse((OF(t), ch, nt))) = pending.peek() {
            if t <= now {
                pending.pop();
                sink.send(&[0x80 | (ch & 0x0F), nt, 0]);
            } else { break; }
        }

        // 清理活动音符
        let akeys: Vec<u8> = active.keys().cloned().collect();
        for k in akeys {
            if let Some(list) = active.get_mut(&k) { list.retain(|a| a.end_time > now); }
            if active.get(&k).map(|v| v.is_empty()).unwrap_or(false) { active.remove(&k); }
        }

        // 组装行
        let mut row_heads: HashMap<i32, Vec<Head>> = HashMap::new();
        let mut row_tails: HashMap<i32, Vec<Tail>> = HashMap::new();
        for blk in &spawned {
            let note = blk.note;
            if !player.note_to_index.contains_key(&(note as i32)) { continue; }
            let key = if player.color_key == "channel" { blk.channel as i32 } else { blk.track };
            let pos = blk.pos;
            let steps = blk.steps;
            let frac = blk.frac;

            if pos < hf {
                if half_cell {
                    let half = if frac < 0.5 { Half::Top } else { Half::Bottom };
                    row_heads.entry(steps).or_default().push(Head { note, key, half, velocity: blk.velocity, channel: blk.channel });
                } else if dual_line && frac >= 0.5 && (steps as f64) < hf - 1.0 {
                    row_heads.entry(steps).or_default().push(Head { note, key, half: Half::Bottom, velocity: blk.velocity, channel: blk.channel });
                    row_heads.entry(steps + 1).or_default().push(Head { note, key, half: Half::Top, velocity: blk.velocity, channel: blk.channel });
                } else {
                    row_heads.entry(steps).or_default().push(Head { note, key, half: Half::Full, velocity: blk.velocity, channel: blk.channel });
                }
            }

            let trail_rows = blk.trail_rows;
            if trail_rows == 0.0 { continue; }
            let tail_color = player.fg_map.get(&key).cloned().unwrap_or_else(|| format!("{}[91m", ESC));
            let trail_top: i32 = if trail_rows == -1.0 { 0 } else { ((pos - trail_rows) as i32).max(0) };
            let mut trail_bot: i32 = if pos < hf { pos as i32 - 1 } else { h - 1 };
            if trail_bot > h - 1 { trail_bot = h - 1; }
            if trail_top <= trail_bot {
                let denom = if trail_rows > 0.0 { trail_rows } else { pos.max(1.0) };
                for r in trail_top..=trail_bot {
                    let dist = ((pos - r as f64) / denom).clamp(0.0, 1.0);
                    row_tails.entry(r).or_default().push(Tail { note, color: tail_color.clone(), dist });
                }
            }
        }

        let (sdx, sdy) = fx.screen_offset(&ctx);
        let shaking = sdx != 0 || sdy != 0;
        let force_full = fx.force_full_redraw() || shaking || was_shaking;

        let bg_rows = fx.bg_rows(&ctx);
        let mut all_rows: std::collections::HashSet<i32> = std::collections::HashSet::new();
        all_rows.extend(row_heads.keys()); all_rows.extend(row_tails.keys()); all_rows.extend(prev_rows.keys());
        all_rows.extend(bg_rows.iter().cloned()); all_rows.extend(prev_bg_rows.iter().cloned());
        if fx.needs_full_grid() || force_full { for r in 0..h { all_rows.insert(r); } }
        let mut new_rows: HashMap<i32, Vec<Cell>> = HashMap::new();
        for &row in &all_rows {
            let hl: &[Head] = row_heads.get(&row).map(|v| v.as_slice()).unwrap_or(&[]);
            let tl: &[Tail] = row_tails.get(&row).map(|v| v.as_slice()).unwrap_or(&[]);
            if hl.is_empty() && tl.is_empty() && !fx.needs_full_grid() {
                new_rows.insert(row, player.static_rows.get(row as usize).cloned().unwrap_or_else(|| player.blank_row()));
            } else {
                new_rows.insert(row, player.render_row_cells(row as usize, hl, tl, &mut fx, &ctx));
            }
        }

        // 键盘高亮：同一个音高常被多个音轨/通道同时或轮流触发，
        // 若每帧都取 active 里的“最后一个”，按键期间的颜色会不停换色（看起来就是背景在闪）。
        // 规则：按住期间颜色保持不变，只在由暗转亮的那一刻取新颜色。
        let mut next_hold: HashMap<u8, (f64, i32)> = HashMap::new();
        for (note, list) in active.iter() {
            if let Some(a) = list.last() {
                let key = kb_hold.get(note).map(|&(_, k)| k).unwrap_or(a.key);
                next_hold.insert(*note, (now + KB_HOLD_SECS, key));
            }
        }
        for (&note, &(t, key)) in kb_hold.iter() {
            if next_hold.contains_key(&note) { continue; }
            if t > now { next_hold.insert(note, (t, key)); }
        }
        kb_hold = next_hold;
        let kb_view: HashMap<u8, i32> = kb_hold.iter().map(|(n, (_, k))| (*n, *k)).collect();
        let kb_cells = player.render_keyboard_cells(&kb_view, &mut fx, &ctx);
        let kb_line = cells_line(&kb_cells, 0);

        let notes_key: Vec<u8> = { let mut v: Vec<u8> = active.keys().cloned().collect(); v.sort_unstable(); v };
        if prev_notes_key.as_ref() != Some(&notes_key) {
            let (name, cn, root) = player.detect_chord(&notes_key);
            prev_chord_name = name;
            prev_chord_notes = cn;
            prev_chord_root = root.map(|r| r as i32).unwrap_or(-1);
            prev_notes_key = Some(notes_key);
        }
        if prev_chord_display.as_deref() != Some(prev_chord_name.as_str()) {
            prev_chord_display = Some(prev_chord_name.clone());
            prev_chord_time = now;
        }
        let duration = if !prev_chord_name.is_empty() { now - prev_chord_time } else { 0.0 };
        let hud = fx.hud_extra(&ctx);
        let chord_line = truncate_ansi(&player.build_chord_line(&prev_chord_name, &prev_chord_notes, duration, &hud), player.term_width as usize, "");
        let pline = if player.cfg.progress_bar {
            Some(truncate_ansi(&player.build_progress_line(played, player.total_note_count, now), player.term_width as usize, ""))
        } else { None };

        let w = player.term_width as usize;
        let max_row = h + 5;

        let slow_tick = frame_idx % 12 == 0;
        let mut frame_parts = String::new();
        if shaking {
            let dy = sdy;
            let mut used: std::collections::HashSet<i32> = std::collections::HashSet::new();
            if !was_shaking {
                for r in 1..=max_row {
                    frame_parts.push_str(&format!("{}[{};1H{}[K", ESC, r, ESC));
                }
            }
            let l1 = truncate_ansi(&player.build_header_line1(), w, "…");
            let l2 = truncate_ansi(&player.build_header_line2(played, player.total_chord_count), w, "…");
            {
                let mut add = |row: i32, line: &str| {
                    let rr = row.clamp(1, max_row);
                    used.insert(rr);
                    frame_parts.push_str(&format!("{}[{};1H{}{}[K", ESC, rr, line, ESC));
                };
                add(1 + dy, &l1);
                add(2 + dy, &l2);
                for r in 0..h {
                    let line = match new_rows.get(&r) { Some(c) => cells_line(c, sdx), None => String::new() };
                    add(3 + r + dy, &line);
                }
                add(3 + h + dy, &kb_line);
                add(3 + h + 1 + dy, &chord_line);
                if let Some(pl) = &pline {
                    add(3 + h + 2 + dy, pl);
                }
            }
            for r in prev_shake_rows.difference(&used) {
                frame_parts.push_str(&format!("{}[{};1H{}[K", ESC, r, ESC));
            }
            prev_shake_rows = used;
        } else {
            let mut snap: Vec<(u8, Vec<(u8, u8)>)> = player.cc_state.iter().map(|(&ch, cc)| {
                let mut v: Vec<(u8, u8)> = cc.iter().map(|(&k, &val)| (k, val)).collect();
                v.sort_unstable();
                (ch, v)
            }).collect();
            snap.sort_unstable_by_key(|x| x.0);
            let header_snap = (played, snap);
            if prev_header_snap.as_ref() != Some(&header_snap) {
                let line2 = player.build_header_line2(played, player.total_chord_count);
                frame_parts.push_str(&format!("{}[2;1H{}{}[K", ESC, truncate_ansi(&line2, w, "…"), ESC));
                prev_header_snap = Some(header_snap);
            }

            for &row in &all_rows {
                let old = prev_rows.get(&row).cloned().unwrap_or_else(|| player.blank_row());
                let new = &new_rows[&row];
                if &old != new {
                    frame_parts.push_str(&player.diff_row_cells(row, &old, new));
                }
            }

            if prev_kb.as_deref() != Some(kb_line.as_str()) {
                frame_parts.push_str(&format!("{}[{};1H{}", ESC, 3 + h, kb_line));
            }
            if (slow_tick || prev_chord_line.is_none()) && prev_chord_line.as_deref() != Some(chord_line.as_str()) {
                let chord_row = 3 + h + 1;
                frame_parts.push_str(&format!("{}[{};1H{}", ESC, chord_row, chord_line));
                prev_chord_line = Some(chord_line.clone());
            }
            if let Some(pl) = &pline {
                if slow_tick && prev_progress.as_deref() != Some(pl.as_str()) {
                    let prow = 3 + h + 2;
                    frame_parts.push_str(&format!("{}[{};1H{}", ESC, prow, pl));
                    prev_progress = Some(pl.clone());
                }
            }
        }

        if !frame_parts.is_empty() {
            write_synced(&frame_parts, player.sync_support);
        }
        if shaking {
            prev_rows.clear();
            prev_bg_rows.clear();
            prev_kb = None;
            prev_chord_line = None;
            prev_progress = None;
            prev_header_snap = None;
        } else {
            prev_rows = new_rows;
            prev_bg_rows = bg_rows.iter().cloned().collect();
            prev_kb = Some(kb_line.clone());
        }
        was_shaking = shaking;

        if producer_finished && spawned.is_empty() && active.is_empty() && pending.is_empty() && heap.is_empty()
            && ctrl_idx >= ctrl_events.len() {
            break;
        }

        frame_idx += 1;
        let target = s + Duration::from_secs_f64(frame_idx as f64 * frame_interval);
        let now_i = Instant::now();
        if now_i < target {
            std::thread::sleep(target.duration_since(now_i));
            while Instant::now() < target {}
        } else if now_i.duration_since(target).as_secs_f64() > frame_interval {
            frame_idx = (((now_i - s).as_secs_f64()) / frame_interval) as i64 + 1;
        }
    }

    for ch in 0..16u8 {
        sink.send(&[0xB0 | ch, 123, 0]);
    }
    let _ = handle.join();
}

fn looks_rust_config(v: &Value) -> bool {
    v.get("fx").is_some() || v.get("climax").is_some()
        || v.get("progress_bar").is_some() || v.get("midi_queue_size").is_some()
}

fn resolve_config_path() -> String {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("config.json"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let mut cur = Some(dir.to_path_buf());
            for _ in 0..4 {
                if let Some(d) = cur {
                    candidates.push(d.join("config.json"));
                    cur = d.parent().map(|p| p.to_path_buf());
                } else { break; }
            }
        }
    }
    for c in &candidates {
        if let Ok(t) = std::fs::read_to_string(c) {
            if let Ok(v) = serde_json::from_str::<Value>(&t) {
                if looks_rust_config(&v) {
                    return c.to_string_lossy().to_string();
                }
            }
        }
    }
    // 没有现成的 Rust 配置：优先在当前目录创建，其次 exe 目录
    if !std::path::Path::new("config.json").exists() {
        return "config.json".to_string();
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("config.json");
            if !p.exists() { return p.to_string_lossy().to_string(); }
        }
    }
    "config.json".to_string()
}

/// 退出时恢复终端：重新打开自动换行、显示光标、切回主屏幕缓冲区。
/// Ctrl+C 和 panic(abort) 都不会执行 Drop，所以额外装了控制台事件处理器和 panic hook。
const TERM_RESTORE: &[u8] = b"\x1b[?7h\x1b[?25h\x1b[?1049l";

#[cfg(windows)]
mod term_restore {
    use std::io::Write;
    use std::os::windows::io::{FromRawHandle, RawHandle};
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::Console::{GetStdHandle, SetConsoleCtrlHandler, STD_OUTPUT_HANDLE};

    /// 直接写控制台句柄，绕开 stdout 的锁（处理器里抢锁可能死锁）。
    /// 用 ManuallyDrop 包住，避免 File 析构时把标准输出句柄关掉。
    pub fn write_raw(bytes: &[u8]) {
        unsafe {
            let h: HANDLE = GetStdHandle(STD_OUTPUT_HANDLE);
            if h.is_null() {
                return;
            }
            let mut f = std::mem::ManuallyDrop::new(std::fs::File::from_raw_handle(h as RawHandle));
            let _ = f.write_all(bytes);
            let _ = f.flush();
        }
    }

    unsafe extern "system" fn on_event(_kind: u32) -> i32 {
        write_raw(super::TERM_RESTORE);
        0 // FALSE：不接管，交回默认处理器终止进程
    }

    pub fn install() {
        unsafe {
            SetConsoleCtrlHandler(Some(on_event), 1);
        }
    }
}

#[cfg(not(windows))]
mod term_restore {
    pub fn write_raw(bytes: &[u8]) {
        use std::io::Write;
        let _ = std::io::stdout().write_all(bytes);
        let _ = std::io::stdout().flush();
    }
    pub fn install() {}
}

fn restore_terminal() {
    term_restore::write_raw(TERM_RESTORE);
}

struct CursorGuard;

impl Drop for CursorGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn main() {
    #[cfg(windows)]
    unsafe { let _ = windows_sys::Win32::Media::timeBeginPeriod(1); }

    let cfg_path = resolve_config_path();
    let (cfg, healed) = Config::load_and_heal(&cfg_path);
    println!("配置: {}", cfg_path);
    if healed { println!("已自动补全/修正配置"); }
    std::thread::sleep(std::time::Duration::from_millis(900));
    let lang_rel = cfg.language.clone();
    let lang_base = read_json("lang/zh-CN.json").unwrap_or_else(|| serde_json::json!({}));
    let lang_over = read_json(&lang_rel).unwrap_or_else(|| serde_json::json!({}));
    let lang = Lang(merge_lang(&lang_base, &lang_over));

    let _ = io::stdout().flush();
    print!("{}[?1049h{}[2J{}[H{}[?25l{}[?7l", ESC, ESC, ESC, ESC, ESC);
    let _ = io::stdout().flush();
    let _cursor_guard = CursorGuard;
    term_restore::install();
    {
        // panic 时也先把终端还原，否则窗口会卡在备用屏幕、光标隐藏的状态
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore_terminal();
            default_hook(info);
        }));
    }

    let raw = {
        print!("{}", lang.s("enter_midi_paths", "请输入 MIDI 文件/文件夹（多个用 ; 分隔）:"));
        let _ = io::stdout().flush();
        read_line()
    };
    let inputs: Vec<String> = raw
        .split([';', ',', '|'])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let mut playlist = Playlist::collect(&inputs, cfg.playlist.recursive);
    if playlist.is_empty() {
        println!("{}", lang.s("no_midi", "没有找到可播放的 MIDI 文件"));
        return;
    }

    let mode = {
        print!("{}", lang.s("select_mode", "播放模式 [1]顺序 [2]循环 [3]随机（默认 1）: "));
        let _ = io::stdout().flush();
        match read_line().trim().to_lowercase().as_str() {
            "2" | "loop" => "loop",
            "3" | "random" => "random",
            _ => "order",
        }
    };
    playlist.set_mode(mode);

    let out = match select_output(&lang) {
        Some(o) => o,
        None => { return; }
    };
    let mut sink = make_midi_sink(out, cfg.midi_queue_size);

    while let Some(path) = playlist.next() {
        let pstr = path.to_string_lossy().to_string();
        let player = Player::new(cfg.clone(), lang.clone());
        play_loop(player, &mut sink, pstr);
    }
    sink.close();

    #[cfg(windows)]
    unsafe { let _ = windows_sys::Win32::Media::timeEndPeriod(1); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lang_merge_keeps_chinese_fallback() {
        let base = json!({"a": "甲", "b": "乙", "instruments": ["钢琴"]});
        let over = json!({"a": "A", "b": "", "instruments": []});
        let m = merge_lang(&base, &over);
        assert_eq!(m["a"], "A");
        assert_eq!(m["b"], "乙");
        assert_eq!(m["instruments"][0], "钢琴");
    }

    #[test]
    fn lang_merge_handles_broken_input() {
        let m = merge_lang(&Value::Null, &Value::Null);
        assert!(m.as_object().map(|o| o.is_empty()).unwrap_or(false));
    }

    #[test]
    fn config_path_looks_like_rust_config() {
        assert!(looks_rust_config(&json!({"fx": {}})));
        assert!(looks_rust_config(&json!({"progress_bar": true})));
        assert!(!looks_rust_config(&json!({"beats_for_drop": 4.0})));
    }
}
