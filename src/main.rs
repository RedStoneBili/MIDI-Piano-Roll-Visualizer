#![allow(dead_code)]

mod midi;

use crossbeam_channel::Sender;
use midi::{EvKind, FastMidiReader};
use serde_json::Value;
use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, Instant};

const ESC: &str = "\x1b";

// ---------- JSON 辅助 ----------
fn jstr(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(|s| s.to_string())
}
fn jf64(v: &Value, k: &str, d: f64) -> f64 {
    v.get(k).and_then(|x| x.as_f64()).unwrap_or(d)
}
fn jbool(v: &Value, k: &str, d: bool) -> bool {
    v.get(k).and_then(|x| x.as_bool()).unwrap_or(d)
}
fn ji32(v: &Value, k: &str, d: i32) -> i32 {
    v.get(k).and_then(|x| x.as_i64()).map(|n| n as i32).unwrap_or(d)
}
fn ju32(v: &Value, k: &str, d: u32) -> u32 {
    v.get(k).and_then(|x| x.as_u64()).map(|n| n as u32).unwrap_or(d)
}

struct Lang(Value);
impl Lang {
    fn s(&self, k: &str, d: &str) -> String {
        self.0.get(k).and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or_else(|| d.to_string())
    }
    fn arr(&self, k: &str) -> Option<Vec<String>> {
        self.0.get(k).and_then(|x| x.as_array()).map(|a| {
            a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
        })
    }
}

struct Config {
    note_names: Vec<String>,
    reset: String,
    highlight_middle_c: bool,
    middle_c_bg: String,
    color_by: String,
    beats_for_drop: f64,
    drop_follows_tempo: bool,
    min_drop_floor: f64,
    refresh_rate: f64,
    block_style: String,
    trail_length: i32,
    trail_by_note: bool,
    dual_line_interp: bool,
    half_cell_notes: bool,
    max_nps: i64,
    sync_output: Option<bool>,
    debug_info: bool,
    default_tempo: u32,
    min_note_opt: Option<i32>,
    max_note_opt: Option<i32>,
}

impl Config {
    fn load(v: &Value) -> Config {
        let nn = v.get("note_names").and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_else(|| vec!["C","C#","D","D#","E","F","F#","G","G#","A","A#","B"].iter().map(|s| s.to_string()).collect());
        Config {
            note_names: nn,
            reset: jstr(v, "color_reset").unwrap_or_else(|| format!("{}[0m", ESC)),
            highlight_middle_c: jbool(v, "highlight_middle_c", true),
            middle_c_bg: jstr(v, "middle_c_bg").unwrap_or_else(|| format!("{}[107m", ESC)),
            color_by: jstr(v, "color_by").unwrap_or_else(|| "auto".into()),
            beats_for_drop: jf64(v, "beats_for_drop", 2.0),
            drop_follows_tempo: jbool(v, "drop_follows_tempo", true),
            min_drop_floor: jf64(v, "min_drop_time_seconds", 0.15),
            refresh_rate: jf64(v, "refresh_rate", 60.0),
            block_style: jstr(v, "block_style").unwrap_or_else(|| "bg".into()),
            trail_length: ji32(v, "trail_length", 0),
            trail_by_note: jbool(v, "trail_by_note", false),
            dual_line_interp: jbool(v, "dual_line_interp", false),
            half_cell_notes: jbool(v, "half_cell_notes", true),
            max_nps: v.get("max_notes_per_second").and_then(|x| x.as_i64()).unwrap_or(0),
            sync_output: v.get("sync_output").and_then(|x| x.as_bool()),
            debug_info: jbool(v, "debug_info", false),
            default_tempo: ju32(v, "default_tempo", 500000),
            min_note_opt: v.get("min_note").and_then(|x| x.as_i64()).map(|n| n as i32),
            max_note_opt: v.get("max_note").and_then(|x| x.as_i64()).map(|n| n as i32),
        }
    }
}

const FG_PALETTE: [&str; 15] = [
    "\x1b[91m","\x1b[94m","\x1b[92m","\x1b[93m","\x1b[95m","\x1b[96m",
    "\x1b[31m","\x1b[34m","\x1b[32m","\x1b[33m","\x1b[35m","\x1b[36m",
    "\x1b[37m","\x1b[97m","\x1b[90m",
];
const BG_PALETTE: [&str; 15] = [
    "\x1b[101m","\x1b[104m","\x1b[102m","\x1b[103m","\x1b[105m","\x1b[106m",
    "\x1b[41m","\x1b[44m","\x1b[42m","\x1b[43m","\x1b[45m","\x1b[46m",
    "\x1b[47m","\x1b[107m","\x1b[100m",
];

fn make_palette_pair(n: usize) -> (Vec<String>, Vec<String>) {
    let mut fg = Vec::with_capacity(n);
    let mut bg = Vec::with_capacity(n);
    for i in 0..n {
        let idx = i % FG_PALETTE.len();
        let mut f = FG_PALETTE[idx].to_string();
        if i >= FG_PALETTE.len() {
            f = format!("\x1b[1m{}", f);
        }
        fg.push(f);
        bg.push(BG_PALETTE[idx].to_string());
    }
    (fg, bg)
}

fn char_width(c: char) -> usize {
    match unicode_width::UnicodeWidthChar::width(c) {
        Some(w) if w > 0 => w,
        _ => 1,
    }
}

fn visible_width(s: &str) -> usize {
    let ch: Vec<char> = s.chars().collect();
    let n = ch.len();
    let mut i = 0usize;
    let mut w = 0usize;
    while i < n {
        if ch[i] == '\x1b' {
            i += 1;
        } else {
            w += char_width(ch[i]);
            i += 1;
        }
    }
    w
}

fn truncate_ansi(s: &str, max_width: usize, suffix: &str) -> String {
    if visible_width(s) <= max_width {
        return s.to_string();
    }
    let suffix_w: usize = suffix.chars().map(char_width).sum();
    let target = max_width.saturating_sub(suffix_w);
    let ch: Vec<char> = s.chars().collect();
    let n = ch.len();
    let mut result = String::new();
    let mut w = 0usize;
    let mut i = 0usize;
    while i < n {
        let c = ch[i];
        if c == '\x1b' {
            result.push(c);
            i += 1;
        } else {
            let cw = char_width(c);
            if w + cw > target {
                break;
            }
            result.push(c);
            w += cw;
            i += 1;
        }
    }
    result.push_str(suffix);
    result.push_str(ESC);
    result.push_str("[0m");
    result
}

fn write_synced(s: &str, sync: bool) {
    if s.is_empty() {
        return;
    }
    let out = io::stdout();
    let mut lock = out.lock();
    if sync {
        let mut buf = Vec::with_capacity(s.len() + 20);
        buf.extend_from_slice(b"\x1b[?2026h");
        buf.extend_from_slice(s.as_bytes());
        buf.extend_from_slice(b"\x1b[?2026l");
        let _ = lock.write_all(&buf);
    } else {
        let _ = lock.write_all(s.as_bytes());
    }
    let _ = lock.flush();
}

fn locate(tm: &[(u64, u32)], ticks: u64) -> usize {
    let mut lo = 0usize;
    let mut hi = tm.len() - 1;
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
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
struct HeaderInfo {
    filename: String,
    n_tracks: usize,
    ticks_per_beat: u16,
    tempo_changes: Vec<(u64, u32)>,
    programs: Vec<u8>,
    channels_used: Vec<u8>,
    tracks_used: Vec<usize>,
    pitch_hist: [u64; 12],
    note_count: u64,
    chord_count_est: u64,
    min_note: i32,
    max_note: i32,
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
    });

    let (min_note, max_note) = match (pcfg.min_note_opt, pcfg.max_note_opt) {
        (Some(a), Some(b)) => (a, b),
        _ => ((meta.min_note as i32 - 3).max(0), (meta.max_note as i32 + 3).min(127)),
    };

    let filename = Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());

    let header = HeaderInfo {
        filename,
        n_tracks: reader.track_ranges.len(),
        ticks_per_beat: reader.ticks_per_beat,
        tempo_changes: meta.tempo_changes.clone(),
        programs: meta.programs.clone(),
        channels_used: meta.channels_used.clone(),
        tracks_used: meta.tracks_used.clone(),
        pitch_hist: meta.pitch_hist,
        note_count: meta.note_count,
        chord_count_est: meta.chord_count_est,
        min_note,
        max_note,
    };
    let _ = tx.send(Msg::Header(header));

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
    let _ = tx.send(Msg::Meta(MetaInfo { first_abs, n_notes: meta.note_count }));

    let mut stacks: HashMap<(i32, u8, u8), VecDeque<(f64, u8, u8, u32)>> = HashMap::new();
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
type Cell = (String, char);

#[derive(Clone, Copy, PartialEq)]
enum Half { Full, Top, Bottom }

#[derive(Clone)]
struct Active { end_time: f64, track: i32, channel: u8, key: i32 }

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

const MAJOR_SCALES: [[i32; 7]; 12] = [
    [0,2,4,5,7,9,11],[1,3,5,6,8,10,0],[2,4,6,7,9,11,1],[3,5,7,8,10,0,2],
    [4,6,8,9,11,1,3],[5,7,9,10,0,2,4],[6,8,10,11,1,3,5],[7,9,11,0,2,4,6],
    [8,10,0,1,3,5,7],[9,11,1,2,4,6,8],[10,0,2,3,5,7,9],[11,1,3,4,6,8,10],
];
const MINOR_SCALES: [[i32; 7]; 12] = [
    [0,2,3,5,7,8,10],[1,3,4,6,8,9,11],[2,4,5,7,9,10,0],[3,5,6,8,10,11,1],
    [4,6,7,9,11,0,2],[5,7,8,10,0,1,3],[6,8,9,11,1,2,4],[7,9,10,0,2,3,5],
    [8,10,11,1,3,4,6],[9,11,0,2,4,5,7],[10,0,1,3,5,6,8],[11,1,2,4,6,7,9],
];

fn chord_defs() -> Vec<(String, Vec<i32>)> {
    let mut d: Vec<(&str, Vec<i32>)> = vec![
        ("", vec![0,4,7]), ("maj", vec![0,4,7]), ("m", vec![0,3,7]),
        ("min", vec![0,3,7]), ("dim", vec![0,3,6]), ("aug", vec![0,4,8]),
        ("sus2", vec![0,2,7]), ("sus4", vec![0,5,7]), ("7", vec![0,4,7,10]),
        ("maj7", vec![0,4,7,11]), ("m7", vec![0,3,7,10]),
        ("m7b5", vec![0,3,6,10]), ("dim7", vec![0,3,6,9]),
        ("mMaj7", vec![0,3,7,11]), ("7b5", vec![0,4,6,10]),
        ("7#5", vec![0,4,8,10]), ("aug7", vec![0,4,8,10]),
        ("6", vec![0,4,7,9]), ("m6", vec![0,3,7,9]),
        ("6/9", vec![0,4,7,9,14]), ("9", vec![0,4,7,10,14]),
        ("maj9", vec![0,4,7,11,14]), ("m9", vec![0,3,7,10,14]),
        ("add9", vec![0,4,7,14]), ("madd9", vec![0,3,7,14]),
        ("11", vec![0,4,7,10,14,17]), ("maj11", vec![0,4,7,11,14,17]),
        ("m11", vec![0,3,7,10,14,17]), ("7#11", vec![0,4,7,10,14,18]),
        ("13", vec![0,4,7,10,14,17,21]), ("maj13", vec![0,4,7,11,14,17,21]),
        ("m13", vec![0,3,7,10,14,17,21]), ("7b9", vec![0,4,7,10,13]),
        ("7#9", vec![0,4,7,10,15]), ("7b13", vec![0,4,7,10,14,20]),
        ("alt7", vec![0,4,6,10,13,18]), ("7sus2", vec![0,2,7,10]),
        ("7sus4", vec![0,5,7,10]), ("9sus4", vec![0,5,7,10,14]),
        ("m7b9", vec![0,3,7,10,13]), ("m7#5", vec![0,3,8,10]),
        ("m9b5", vec![0,3,6,10,14]), ("7b9#9", vec![0,4,7,10,13,15]),
        ("7b9#11", vec![0,4,7,10,13,18]), ("7/6", vec![0,4,7,9,10]),
        ("9/13", vec![0,4,7,10,14,21]),
    ];
    d.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    d.into_iter().map(|(n, p)| (n.to_string(), p)).collect()
}

fn detect_sync(forced: Option<bool>) -> bool {
    if let Some(f) = forced { return f; }
    if std::env::var("WT_SESSION").is_ok() { return true; }
    let tp = std::env::var("TERM_PROGRAM").unwrap_or_default().to_lowercase();
    if ["wezterm","iterm.app","ghostty","vscode","warpterminal","alacritty","hyper","vscode-insiders"].contains(&tp.as_str()) { return true; }
    let term = std::env::var("TERM").unwrap_or_default().to_lowercase();
    if term.contains("kitty") || term.contains("alacritty") || term.contains("wezterm") { return true; }
    true
}

struct Player {
    cfg: Config,
    lang: Lang,
    min_note: i32,
    max_note: i32,
    term_width: i32,
    display_width: i32,
    display_height: i32,
    note_to_index: HashMap<i32, usize>,
    cell_starts: Vec<i32>,
    cell_widths: Vec<i32>,
    note_pos_map: HashMap<i32, i32>,
    keyboard_positions: Vec<(i32, String)>,
    middle_c_cell: Option<usize>,
    middle_c_pos: Option<i32>,
    kb_cells: Vec<(i32, i32, i32, String, bool)>,
    static_rows: Vec<Vec<Cell>>,
    color_key: String,
    fg_map: HashMap<i32, String>,
    bg_map: HashMap<i32, String>,
    header_filename: String,
    header_n_tracks: usize,
    mode_str: String,
    bpm_str: String,
    total_chord_count: u64,
    total_note_count: u64,
    instruments: Vec<(String, u32)>,
    instruments_list: Vec<String>,
    cc_state: HashMap<u8, HashMap<u8, u8>>,
    prog_state: HashMap<u8, u8>,
    pitch_state: HashMap<u8, i32>,
    sync_support: bool,
    chord_defs: Vec<(String, Vec<i32>)>,
}

impl Player {
    fn new(cfg: Config, lang: Lang) -> Player {
        let inst_list = lang.arr("instruments").unwrap_or_default();
        let sync = detect_sync(cfg.sync_output);
        let mut p = Player {
            cfg, lang,
            min_note: 48, max_note: 84,
            term_width: 120, display_width: 0, display_height: 40,
            note_to_index: HashMap::new(), cell_starts: Vec::new(), cell_widths: Vec::new(),
            note_pos_map: HashMap::new(), keyboard_positions: Vec::new(),
            middle_c_cell: None, middle_c_pos: None, kb_cells: Vec::new(),
            static_rows: Vec::new(),
            color_key: "track".into(), fg_map: HashMap::new(), bg_map: HashMap::new(),
            header_filename: String::new(), header_n_tracks: 0,
            mode_str: String::new(), bpm_str: String::new(),
            total_chord_count: 0, total_note_count: 0,
            instruments: Vec::new(), instruments_list: inst_list,
            cc_state: HashMap::new(), prog_state: HashMap::new(), pitch_state: HashMap::new(),
            sync_support: sync, chord_defs: chord_defs(),
        };
        if let (Some(a), Some(b)) = (p.cfg.min_note_opt, p.cfg.max_note_opt) { p.min_note = a; p.max_note = b; }
        p.init_layout();
        p
    }

    fn init_layout(&mut self) {
        let n = (self.max_note - self.min_note + 1).max(1);
        let w = self.term_width.max(n);
        let base = (w / n).max(1);
        self.display_width = base * n;
        self.note_to_index.clear(); self.cell_starts.clear(); self.cell_widths.clear();
        self.note_pos_map.clear(); self.keyboard_positions.clear();
        self.middle_c_cell = None; self.middle_c_pos = None;
        let mut pos = 0i32;
        for i in 0..n {
            let note = self.min_note + i;
            let center = pos + (base - 1) / 2;
            let name = self.cfg.note_names[(note % 12) as usize].clone();
            self.note_to_index.insert(note, i as usize);
            self.cell_starts.push(pos);
            self.cell_widths.push(base);
            self.note_pos_map.insert(note, center);
            self.keyboard_positions.push((center, name));
            if note == 60 { self.middle_c_cell = Some(i as usize); self.middle_c_pos = Some(center); }
            pos += base;
        }
        self.build_static_rows();
        self.kb_cells.clear();
        for i in 0..n {
            let note = self.min_note + i;
            let name = self.cfg.note_names[(note % 12) as usize].clone();
            let is_sharp = name.contains('#');
            self.kb_cells.push((self.cell_starts[i as usize], self.cell_widths[i as usize], note, name, is_sharp));
        }
    }

    fn build_static_rows(&mut self) {
        self.static_rows.clear();
        for _ in 0..self.display_height {
            let mut row: Vec<Cell> = Vec::with_capacity(self.display_width as usize);
            for _ in 0..self.display_width { row.push((String::new(), ' ')); }
            self.static_rows.push(row);
        }
    }

    fn assign_colors(&mut self, channels: &[u8], tracks: &[usize]) {
        let mut by = self.cfg.color_by.to_lowercase();
        if by == "auto" { by = if channels.len() > 1 { "channel".into() } else { "track".into() }; }
        if by != "channel" && by != "track" { by = "track".into(); }
        let mut keys: Vec<i32> = if by == "channel" {
            channels.iter().map(|&c| c as i32).collect()
        } else {
            tracks.iter().map(|&t| t as i32).collect()
        };
        if keys.is_empty() { keys.push(0); }
        keys.sort_unstable(); keys.dedup();
        let (fg, bg) = make_palette_pair(keys.len());
        self.color_key = by;
        self.fg_map.clear(); self.bg_map.clear();
        for (i, &k) in keys.iter().enumerate() {
            self.fg_map.insert(k, fg[i].clone());
            self.bg_map.insert(k, bg[i].clone());
        }
    }

    fn instrument_name(&self, prog: u8) -> String {
        self.instruments_list.get(prog as usize).cloned().unwrap_or_else(|| format!("乐器{}", prog as i32 + 1))
    }

    fn build_header_line1(&self) -> String {
        let inst = if self.instruments.len() > 3 {
            let head: Vec<String> = self.instruments.iter().take(3).map(|(n, c)| format!("{}{}", c, n)).collect();
            format!("{}等{}种", head.join("、"), self.instruments.len())
        } else if !self.instruments.is_empty() {
            self.instruments.iter().map(|(n, c)| format!("{}{}", c, n)).collect::<Vec<_>>().join("、")
        } else {
            "-".to_string()
        };
        format!(
            "{}[1m{}{}{} {}[1m{}{}{} {}[1m{}{}{} {}[1m{}{}{} {}[1m{}{}",
            ESC, self.lang.s("filename", "文件名:"), self.cfg.reset, self.header_filename,
            ESC, self.lang.s("tracks", "音轨数:"), self.cfg.reset, self.header_n_tracks,
            ESC, self.lang.s("instrument", "乐器:"), self.cfg.reset, inst,
            ESC, self.lang.s("tempo", "速度:"), self.cfg.reset, self.bpm_str,
            ESC, self.mode_str, self.cfg.reset
        )
    }

    fn build_header_line2(&self, played: u64, chord: u64) -> String {
        let mut parts: Vec<String> = vec![
            format!("{}[1m{}{}{}/{}", ESC, self.lang.s("notes", "音符数:"), self.cfg.reset, played, self.total_note_count),
            format!("{}[1m{}{}{}", ESC, self.lang.s("chords", "和弦数:"), self.cfg.reset, chord),
        ];
        let mut chs: Vec<u8> = self.cc_state.keys().cloned().collect();
        chs.sort_unstable();
        for &ch in chs.iter().take(3) {
            let mut items: Vec<String> = Vec::new();
            if let Some(cc) = self.cc_state.get(&ch) {
                if let Some(v) = cc.get(&7) { items.push(format!("{}={}", self.lang.s("cc_vol", "音量"), v)); }
                if let Some(v) = cc.get(&10) { items.push(format!("{}={}", self.lang.s("cc_pan", "声像"), v)); }
                if let Some(v) = cc.get(&91) { items.push(format!("{}={}", self.lang.s("cc_rev", "混响"), v)); }
                if let Some(v) = cc.get(&64) {
                    let s = if *v >= 64 { self.lang.s("cc_sus_on", "开") } else { self.lang.s("cc_sus_off", "关") };
                    items.push(format!("{}={}", self.lang.s("cc_sus", "延音"), s));
                }
            }
            if let Some(&prog) = self.prog_state.get(&ch) {
                items.push(format!("{}={}", self.lang.s("cc_prog", "音色"), prog));
            }
            if !items.is_empty() {
                parts.push(format!("ch{}[{}]", ch, items.join(" ")));
            }
        }
        parts.join(" | ")
    }

    fn display_header(&mut self) {
        let line1 = self.build_header_line1();
        let line2 = self.build_header_line2(0, self.total_chord_count);
        let w = self.term_width as usize;
        let mut parts = String::new();
        parts.push_str(&format!("{}[2J{}[H", ESC, ESC));
        parts.push_str(&truncate_ansi(&line1, w, "…"));
        parts.push_str(&format!("{}[K\n", ESC));
        parts.push_str(&truncate_ansi(&line2, w, "…"));
        parts.push_str(&format!("{}[K\n", ESC));
        write_synced(&parts, self.sync_support);
    }

    fn apply_header_info(&mut self, h: HeaderInfo) {
        self.header_filename = h.filename;
        self.header_n_tracks = h.n_tracks;
        self.instruments.clear();
        let mut counts: HashMap<String, u32> = HashMap::new();
        for &prog in &h.programs { *counts.entry(self.instrument_name(prog)).or_insert(0) += 1; }
        self.instruments = counts.into_iter().collect();

        let mut bpms: std::collections::BTreeSet<i32> = std::collections::BTreeSet::new();
        for &(_, t) in &h.tempo_changes {
            if t > 0 { bpms.insert((60_000_000.0 / t as f64).round() as i32); }
        }
        if let (Some(&lo), Some(&hi)) = (bpms.iter().next(), bpms.iter().last()) {
            if lo == hi { self.bpm_str = format!("{}BPM", lo); }
            else { self.bpm_str = format!("{}~{}BPM ({})", lo, hi, self.lang.s("tempo_variable", "变速")); }
        } else { self.bpm_str = "120BPM".to_string(); }

        let mut major = [0u64; 12];
        let mut minor = [0u64; 12];
        for i in 0..12 {
            major[i] = MAJOR_SCALES[i].iter().map(|&n| h.pitch_hist[n as usize]).sum();
            minor[i] = MINOR_SCALES[i].iter().map(|&n| h.pitch_hist[n as usize]).sum();
        }
        let best_major = (0..12).max_by_key(|&i| major[i]).unwrap();
        let best_minor = (0..12).min_by_key(|&i| std::cmp::Reverse(minor[i])).unwrap();
        if major[best_major] >= minor[best_minor] {
            self.mode_str = format!("{}{}{} ({}{})", self.lang.s("mode", "调式:"), self.cfg.note_names[best_major], self.lang.s("major", "大调"), self.lang.s("match", "匹配度:"), major[best_major]);
        } else {
            self.mode_str = format!("{}{}{} ({}{})", self.lang.s("mode", "调式:"), self.cfg.note_names[best_minor], self.lang.s("minor", "小调"), self.lang.s("match", "匹配度:"), minor[best_minor]);
        }

        self.total_chord_count = h.chord_count_est;
        self.total_note_count = h.note_count;

        if let Some((w, ht)) = terminal_size::terminal_size() {
            self.term_width = w.0 as i32;
            let lines = ht.0 as i32;
            self.display_height = (lines - 5).max(10);
        }
        if h.min_note != self.min_note || h.max_note != self.max_note {
            self.min_note = h.min_note;
            self.max_note = h.max_note;
        }
        self.init_layout();
        self.assign_colors(&h.channels_used, &h.tracks_used);
    }

    fn get_block_visual(&self, key: i32) -> (String, char, String) {
        if self.cfg.block_style == "bg" {
            (self.bg_map.get(&key).cloned().unwrap_or_else(|| format!("{}[41m", ESC)), ' ', self.fg_map.get(&key).cloned().unwrap_or_else(|| format!("{}[91m", ESC)))
        } else {
            (self.fg_map.get(&key).cloned().unwrap_or_else(|| format!("{}[91m", ESC)), '█', self.fg_map.get(&key).cloned().unwrap_or_else(|| format!("{}[91m", ESC)))
        }
    }

    fn render_row_cells(&self, line: usize, heads: &[(u8, i32, Half)], tails: &[(u8, String)]) -> Vec<Cell> {
        let mut cells = self.static_rows[line].clone();
        for (note, color) in tails {
            if let Some(&center) = self.note_pos_map.get(&(*note as i32)) {
                if center >= 0 && center < self.display_width {
                    cells[center as usize] = (color.clone(), '|');
                }
            }
        }
        let mut full: HashMap<i32, i32> = HashMap::new();
        let mut top: HashMap<i32, i32> = HashMap::new();
        let mut bot: HashMap<i32, i32> = HashMap::new();
        for &(note, key, half) in heads {
            let i = match self.note_to_index.get(&(note as i32)) { Some(&i) => i, None => continue };
            let start = self.cell_starts[i];
            let w = self.cell_widths[i];
            let end = (start + w).min(self.display_width);
            let mut k = start;
            while k < end {
                match half {
                    Half::Full => { full.insert(k, key); }
                    Half::Top => { top.insert(k, key); }
                    Half::Bottom => { bot.insert(k, key); }
                }
                k += 1;
            }
        }
        let mut keyset: std::collections::HashSet<i32> = std::collections::HashSet::new();
        keyset.extend(full.keys()); keyset.extend(top.keys()); keyset.extend(bot.keys());
        for &k in keyset.iter() {
            if let Some(&key) = full.get(&k) {
                let f = self.fg_map.get(&key).cloned().unwrap_or_else(|| format!("{}[91m", ESC));
                let b = self.bg_map.get(&key).cloned().unwrap_or_else(|| format!("{}[41m", ESC));
                if self.cfg.block_style == "bg" {
                    cells[k as usize] = (b, ' ');
                } else {
                    cells[k as usize] = (f, '█');
                }
            } else if top.contains_key(&k) && bot.contains_key(&k) {
                let f = self.fg_map.get(&top[&k]).cloned().unwrap_or_else(|| format!("{}[91m", ESC));
                let b = self.bg_map.get(&bot[&k]).cloned().unwrap_or_else(|| format!("{}[41m", ESC));
                cells[k as usize] = (format!("{}{}", f, b), '▀');
            } else if let Some(&key) = top.get(&k) {
                let f = self.fg_map.get(&key).cloned().unwrap_or_else(|| format!("{}[91m", ESC));
                cells[k as usize] = (f, '▀');
            } else if let Some(&key) = bot.get(&k) {
                let f = self.fg_map.get(&key).cloned().unwrap_or_else(|| format!("{}[91m", ESC));
                cells[k as usize] = (f, '▄');
            }
        }
        cells
    }

    fn diff_row_cells(&self, row: i32, old: &[Cell], new: &[Cell]) -> String {
        let n = new.len();
        let mut parts = String::new();
        let mut j = 0usize;
        while j < n {
            if old[j] == new[j] { j += 1; continue; }
            let start = j;
            let mut buf = format!("{}[0m", ESC);
            let mut last_color = String::new();
            while j < n && old[j] != new[j] {
                let (c, ch) = &new[j];
                if *c != last_color {
                    if !last_color.is_empty() { buf.push_str(ESC); buf.push_str("[0m"); }
                    if !c.is_empty() { buf.push_str(c); }
                    last_color = c.clone();
                }
                buf.push(*ch);
                j += 1;
            }
            if !last_color.is_empty() { buf.push_str(ESC); buf.push_str("[0m"); }
            parts.push_str(&format!("{}[{};{}H{}", ESC, 3 + row, start + 1, buf));
        }
        parts
    }

    fn render_keyboard_line(&self, active: &HashMap<u8, Vec<Active>>) -> String {
        let w = self.display_width;
        let mut cols: Vec<(char, String)> = vec![(' ', String::new()); w as usize];
        for &(start, width, note, ref name, is_sharp) in &self.kb_cells {
            let mut bg = String::new();
            if let Some(list) = active.get(&(note as u8)) {
                if let Some(last) = list.last() {
                    bg = self.bg_map.get(&last.key).cloned().unwrap_or_else(|| format!("{}[41m", ESC));
                }
            }
            if bg.is_empty() && self.cfg.highlight_middle_c && note == 60 {
                bg = self.cfg.middle_c_bg.clone();
            }
            let full_code = if bg.is_empty() { String::new() } else { format!("{}{}[1m{}[30m", bg, ESC, ESC) };
            for k in 0..width {
                let col = start + k;
                if col >= 0 && col < w { cols[col as usize] = (' ', full_code.clone()); }
            }
            let nchars = name.chars().count() as i32;
            let text = if width >= nchars {
                name.clone()
            } else if is_sharp {
                "#".to_string()
            } else {
                name.chars().next().map(|c| c.to_string()).unwrap_or_default()
            };
            let tlen = text.chars().count() as i32;
            let pad = ((width - tlen) / 2).max(0);
            for (ti, chc) in text.chars().enumerate() {
                let col = start + pad + ti as i32;
                if col >= 0 && col < w { cols[col as usize] = (chc, full_code.clone()); }
            }
        }
        let mut parts = String::new();
        let mut last = String::new();
        for (ch, code) in cols {
            if code != last {
                if !last.is_empty() { parts.push_str(ESC); parts.push_str("[0m"); }
                if !code.is_empty() { parts.push_str(&code); }
                last = code;
            }
            parts.push(ch);
        }
        if !last.is_empty() { parts.push_str(ESC); parts.push_str("[0m"); }
        parts
    }

    fn detect_chord(&self, notes: &[u8]) -> (String, Vec<String>, Option<u8>) {
        if notes.is_empty() { return (String::new(), Vec::new(), None); }
        let mut ns: Vec<i32> = notes.iter().map(|&n| n as i32).collect();
        ns.sort_unstable(); ns.dedup();
        let root = ns[0] % 12;
        let intervals: std::collections::HashSet<i32> = ns.iter().map(|&n| (n - ns[0]) % 12).collect();
        for (name, pattern) in &self.chord_defs {
            let pset: std::collections::HashSet<i32> = pattern.iter().cloned().collect();
            if pset.is_subset(&intervals) && intervals.len() >= pattern.len() {
                let chord_name = format!("{}{}", self.cfg.note_names[root as usize], name);
                let chord_notes: Vec<String> = pattern.iter().map(|&i| self.cfg.note_names[((root + i) % 12) as usize].clone()).collect();
                return (chord_name, chord_notes, Some(root as u8));
            }
        }
        (String::new(), Vec::new(), None)
    }

    fn build_chord_line(&self, name: &str, notes: &[String], duration: f64) -> String {
        if !name.is_empty() {
            format!(
                "{}[1m{}{} {}  {}[1m{}{} {:.2}{}  {}[1m{}{} {}{}[K",
                ESC, self.lang.s("label", "和弦:"), self.cfg.reset, name,
                ESC, self.lang.s("duration", "持续:"), self.cfg.reset, duration,
                self.lang.s("seconds", "s"),
                ESC, self.lang.s("components", "构成:"), self.cfg.reset, notes.join(" "), ESC
            )
        } else {
            format!("{}[1m{}{} {}{}[K", ESC, self.lang.s("label", "和弦:"), self.cfg.reset, self.lang.s("none", "无"), ESC)
        }
    }

    fn update_effect_from_bytes(&mut self, data: &[u8]) {
        if data.is_empty() { return; }
        let st = data[0] & 0xF0;
        let ch = data[0] & 0x0F;
        match st {
            0xB0 if data.len() >= 3 => { self.cc_state.entry(ch).or_default().insert(data[1], data[2]); }
            0xC0 if data.len() >= 2 => { self.prog_state.insert(ch, data[1]); }
            0xE0 if data.len() >= 3 => { self.pitch_state.insert(ch, (((data[2] as i32) << 7) | (data[1] as i32)) - 0x2000); }
            _ => {}
        }
    }
}


// ---------- 输入/路径 ----------
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

fn play_loop(mut player: Player, mut out: midir::MidiOutputConnection, midi_path: String) {
    let (tx, rx) = crossbeam_channel::bounded::<Msg>(5000);
    let pcfg = ProducerCfg {
        beats_for_drop: player.cfg.beats_for_drop,
        drop_follows_tempo: player.cfg.drop_follows_tempo,
        min_drop_floor: player.cfg.min_drop_floor,
        min_note_opt: player.cfg.min_note_opt,
        max_note_opt: player.cfg.max_note_opt,
        default_tempo: player.cfg.default_tempo,
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
    let mut prev_notes_key: Option<Vec<u8>> = None;
    let mut prev_chord_name = String::new();
    let mut prev_chord_notes: Vec<String> = Vec::new();
    let mut prev_chord_display: Option<String> = None;
    let mut prev_chord_time: f64 = 0.0;
    let mut prev_chord_line: Option<String> = None;
    let mut prev_header_snap: Option<(u64, Vec<(u8, Vec<(u8, u8)>)>)> = None;

    let refresh = player.cfg.refresh_rate.clamp(30.0, 120.0);
    let frame_interval = 1.0 / refresh;
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
                Ok(Msg::Meta(m)) => { player.total_note_count = m.n_notes; meta_received = true; }
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
                    cand.sort_by(|a, b| b.0.cmp(&a.0));
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
                let _ = out.send(&bytes);
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
                    let _ = out.send(&[0x90 | (blk.channel & 0x0F), blk.note, blk.velocity]);
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
                        let _ = out.send(&[0x80 | (blk.channel & 0x0F), blk.note, 0]);
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
                let _ = out.send(&[0x80 | (ch & 0x0F), nt, 0]);
            } else { break; }
        }

        // 清理活动音符
        let akeys: Vec<u8> = active.keys().cloned().collect();
        for k in akeys {
            if let Some(list) = active.get_mut(&k) { list.retain(|a| a.end_time > now); }
            if active.get(&k).map(|v| v.is_empty()).unwrap_or(false) { active.remove(&k); }
        }

        // 组装行
        let mut row_heads: HashMap<i32, Vec<(u8, i32, Half)>> = HashMap::new();
        let mut row_tails: HashMap<i32, Vec<(u8, String)>> = HashMap::new();
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
                    row_heads.entry(steps).or_default().push((note, key, half));
                } else if dual_line && frac >= 0.5 && (steps as f64) < hf - 1.0 {
                    row_heads.entry(steps).or_default().push((note, key, Half::Bottom));
                    row_heads.entry(steps + 1).or_default().push((note, key, Half::Top));
                } else {
                    row_heads.entry(steps).or_default().push((note, key, Half::Full));
                }
            }

            let trail_rows = blk.trail_rows;
            if trail_rows == 0.0 { continue; }
            let tail_color = player.fg_map.get(&key).cloned().unwrap_or_else(|| format!("{}[91m", ESC));
            let trail_top: i32 = if trail_rows == -1.0 { 0 } else { ((pos - trail_rows) as i32).max(0) };
            let mut trail_bot: i32 = if pos < hf { pos as i32 - 1 } else { h - 1 };
            if trail_bot > h - 1 { trail_bot = h - 1; }
            if trail_top <= trail_bot {
                for r in trail_top..=trail_bot {
                    row_tails.entry(r).or_default().push((note, tail_color.clone()));
                }
            }
        }

        let mut all_rows: std::collections::HashSet<i32> = std::collections::HashSet::new();
        all_rows.extend(row_heads.keys()); all_rows.extend(row_tails.keys()); all_rows.extend(prev_rows.keys());
        let mut new_rows: HashMap<i32, Vec<Cell>> = HashMap::new();
        for &row in &all_rows {
            let hl: &[(u8, i32, Half)] = row_heads.get(&row).map(|v| v.as_slice()).unwrap_or(&[]);
            let tl: &[(u8, String)] = row_tails.get(&row).map(|v| v.as_slice()).unwrap_or(&[]);
            if hl.is_empty() && tl.is_empty() {
                new_rows.insert(row, player.static_rows[row as usize].clone());
            } else {
                new_rows.insert(row, player.render_row_cells(row as usize, hl, tl));
            }
        }

        let kb_line = player.render_keyboard_line(&active);

        let mut frame_parts = String::new();

        let mut snap: Vec<(u8, Vec<(u8, u8)>)> = player.cc_state.iter().map(|(&ch, cc)| {
            let mut v: Vec<(u8, u8)> = cc.iter().map(|(&k, &val)| (k, val)).collect();
            v.sort_unstable();
            (ch, v)
        }).collect();
        snap.sort_unstable_by_key(|x| x.0);
        let header_snap = (played, snap);
        if prev_header_snap.as_ref() != Some(&header_snap) {
            let line2 = player.build_header_line2(played, player.total_chord_count);
            let w = player.term_width as usize;
            frame_parts.push_str(&format!("{}[2;1H{}{}[K", ESC, truncate_ansi(&line2, w, "…"), ESC));
            prev_header_snap = Some(header_snap);
        }

        for &row in &all_rows {
            let old = prev_rows.get(&row).cloned().unwrap_or_else(|| player.static_rows[row as usize].clone());
            let new = &new_rows[&row];
            if &old != new {
                frame_parts.push_str(&player.diff_row_cells(row, &old, new));
            }
        }

        if prev_kb.as_deref() != Some(kb_line.as_str()) {
            frame_parts.push_str(&format!("{}[{};1H{}", ESC, 3 + h, kb_line));
        }

        let notes_key: Vec<u8> = { let mut v: Vec<u8> = active.keys().cloned().collect(); v.sort_unstable(); v };
        if prev_notes_key.as_ref() != Some(&notes_key) {
            let (name, cn, _root) = player.detect_chord(&notes_key);
            prev_chord_name = name;
            prev_chord_notes = cn;
            prev_notes_key = Some(notes_key);
        }
        if prev_chord_display.as_deref() != Some(prev_chord_name.as_str()) {
            prev_chord_display = Some(prev_chord_name.clone());
            prev_chord_time = now;
        }
        let duration = if !prev_chord_name.is_empty() { now - prev_chord_time } else { 0.0 };
        let chord_line = player.build_chord_line(&prev_chord_name, &prev_chord_notes, duration);
        if prev_chord_line.as_deref() != Some(chord_line.as_str()) {
            let chord_row = 3 + h + 1;
            frame_parts.push_str(&format!("{}[{};1H{}", ESC, chord_row, chord_line));
            prev_chord_line = Some(chord_line);
        }

        if !frame_parts.is_empty() {
            write_synced(&frame_parts, player.sync_support);
        }
        prev_rows = new_rows;
        prev_kb = Some(kb_line);

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

    let _ = handle.join();
}

fn main() {
    #[cfg(windows)]
    unsafe { let _ = windows_sys::Win32::Media::timeBeginPeriod(1); }

    let cfg_v = match read_json("config.json") {
        Some(v) => v,
        None => { println!("找不到 config.json"); return; }
    };
    let lang_rel = jstr(&cfg_v, "language").unwrap_or_else(|| "lang/zh-CN.json".to_string());
    let lang_v = read_json(&lang_rel).unwrap_or_else(|| serde_json::json!({}));
    let cfg = Config::load(&cfg_v);
    let lang = Lang(lang_v);

    let _ = io::stdout().flush();
    print!("{}", format!("{}[2J{}[H", ESC, ESC));
    let _ = io::stdout().flush();
    let path = {
        print!("{}", lang.s("enter_midi_path", "请输入MIDI文件路径:"));
        let _ = io::stdout().flush();
        read_line()
    };
    let out = match select_output(&lang) {
        Some(o) => o,
        None => { return; }
    };
    let player = Player::new(cfg, lang);
    play_loop(player, out, path);

    #[cfg(windows)]
    unsafe { let _ = windows_sys::Win32::Media::timeEndPeriod(1); }
}
