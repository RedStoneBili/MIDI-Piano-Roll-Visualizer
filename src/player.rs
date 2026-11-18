use crate::config::Config;
use crate::effects::theme::theme_palette;
use crate::effects::{FrameCtx, FxManager};
use crate::util::{truncate_ansi, write_synced, ESC};
use crate::HeaderInfo;
use crate::Lang;
use std::collections::HashMap;

fn make_palette_pair(n: usize, theme: &str) -> (Vec<String>, Vec<String>) {
    let (fgp, bgp) = theme_palette(theme);
    let mut fg = Vec::with_capacity(n);
    let mut bg = Vec::with_capacity(n);
    for i in 0..n {
        let idx = i % fgp.len();
        let mut f = fgp[idx].to_string();
        if i >= fgp.len() {
            f = format!("{}[1m{}", ESC, f);
        }
        fg.push(f);
        bg.push(bgp[idx].to_string());
    }
    (fg, bg)
}

pub type Cell = (String, char);

pub struct Head { pub note: u8, pub key: i32, pub half: Half, pub velocity: u8, pub channel: u8 }
pub struct Tail { pub note: u8, pub color: String, pub dist: f64 }

#[derive(Clone, Copy, PartialEq)]
pub enum Half { Full, Top, Bottom }

#[derive(Clone)]
pub struct Active { pub end_time: f64, pub track: i32, pub channel: u8, pub key: i32 }

const MAJOR_PROFILE: [f64; 12] = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const MINOR_PROFILE: [f64; 12] = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];
const SHARP_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const FLAT_NAMES: [&str; 12] = ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B"];
const NATURAL_PC: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];

fn letter_index(c: char) -> usize {
    match c {
        'C' => 0, 'D' => 1, 'E' => 2, 'F' => 3, 'G' => 4, 'A' => 5, 'B' => 6,
        _ => 0,
    }
}

fn letter_char(i: usize) -> char {
    ['C', 'D', 'E', 'F', 'G', 'A', 'B'][i]
}

fn major_tonic(sf: i32) -> String {
    const T: [&str; 15] = ["Cb", "Gb", "Db", "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#"];
    let idx = (sf + 7).clamp(0, 14) as usize;
    T[idx].to_string()
}

// 按调号给出 12 个音级的习惯拼写：调内音用正确的字母+升降号，
// 调外音优先用自然音名，其余按升/降方向补足。
fn spell_names(root: i32, is_minor: bool, sf: i32) -> Vec<String> {
    let tonic = major_tonic(sf);
    let mut mi = letter_index(tonic.chars().next().unwrap());
    if is_minor {
        mi = (mi + 5) % 7;
    }
    let scale: &[i32; 7] = if is_minor {
        &MINOR_SCALES[root as usize]
    } else {
        &MAJOR_SCALES[root as usize]
    };
    let mut names: Vec<String> = vec![String::new(); 12];
    let mut used = [false; 12];
    for (i, &target) in scale.iter().enumerate() {
        let li = (mi + i) % 7;
        let mut a = (target - NATURAL_PC[li]).rem_euclid(12);
        if a > 6 {
            a -= 12;
        }
        let mut s = String::new();
        s.push(letter_char(li));
        if a > 0 {
            for _ in 0..a {
                s.push('#');
            }
        } else if a < 0 {
            for _ in 0..(-a) {
                s.push('b');
            }
        }
        names[target as usize] = s;
        used[target as usize] = true;
    }
    for pc in 0..12usize {
        if !used[pc] {
            names[pc] = if sf < 0 { FLAT_NAMES[pc].to_string() } else { SHARP_NAMES[pc].to_string() };
        }
    }
    names
}

fn key_sf(root: i32, is_minor: bool) -> i32 {
    let maj = if is_minor { (root + 3) % 12 } else { root };
    let mut sf = (maj * 7) % 12;
    if sf > 6 {
        sf -= 12;
    }
    sf
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
    d.sort_by_key(|x| std::cmp::Reverse(x.1.len()));
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

pub struct Player {
    pub cfg: Config,
    pub lang: Lang,
    pub min_note: i32,
    pub max_note: i32,
    pub term_width: i32,
    pub display_width: i32,
    pub display_height: i32,
    pub note_to_index: HashMap<i32, usize>,
    pub cell_starts: Vec<i32>,
    pub cell_widths: Vec<i32>,
    pub note_pos_map: HashMap<i32, i32>,
    pub keyboard_positions: Vec<(i32, String)>,
    pub middle_c_cell: Option<usize>,
    pub middle_c_pos: Option<i32>,
    pub kb_cells: Vec<(i32, i32, i32, String, bool)>,
    pub static_rows: Vec<Vec<Cell>>,
    pub color_key: String,
    pub fg_map: HashMap<i32, String>,
    pub bg_map: HashMap<i32, String>,
    pub header_filename: String,
    pub header_n_tracks: usize,
    pub mode_str: String,
    pub bpm_str: String,
    pub total_chord_count: u64,
    pub total_note_count: u64,
    pub key_events: Vec<(f64, i32, u8)>,
    pub climax: Vec<(f64, f64)>,
    pub key_idx: usize,
    pub first_abs: f64,
    pub tpb: f64,
    pub tempo_map: Vec<(u64, u32)>,
    pub seg: Vec<f64>,
    pub key_root: i32,
    pub is_minor: bool,
    pub instruments: Vec<(String, u32)>,
    pub instruments_list: Vec<String>,
    pub cc_state: HashMap<u8, HashMap<u8, u8>>,
    pub prog_state: HashMap<u8, u8>,
    pub pitch_state: HashMap<u8, i32>,
    pub sync_support: bool,
    pub chord_defs: Vec<(String, Vec<i32>)>,
}

impl Player {
    pub fn new(cfg: Config, lang: Lang) -> Player {
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
            key_events: Vec::new(), key_idx: 0,
            climax: Vec::new(),
            first_abs: 0.0, tpb: 480.0, tempo_map: vec![(0, 500000)], seg: vec![0.0],
            key_root: 0, is_minor: false,
            instruments: Vec::new(), instruments_list: inst_list,
            cc_state: HashMap::new(), prog_state: HashMap::new(), pitch_state: HashMap::new(),
            sync_support: sync, chord_defs: chord_defs(),
        };
        if let (Some(a), Some(b)) = (p.cfg.min_note_opt, p.cfg.max_note_opt) {
            // 配置写反了也能正常跑：小的当下限、大的当上限
            p.min_note = a.min(b);
            p.max_note = a.max(b);
        }
        p.init_layout();
        p
    }

    pub fn init_layout(&mut self) {
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
            let name = self.note_name(note);
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
            let name = self.note_name(note);
            let is_sharp = name.contains('#');
            self.kb_cells.push((self.cell_starts[i as usize], self.cell_widths[i as usize], note, name, is_sharp));
        }
    }

    /// 取音名；音名表异常短也不会 panic（配置加载时已补齐，这里再兜一层）
    pub fn note_name(&self, note: i32) -> String {
        let idx = note.rem_euclid(12) as usize;
        self.cfg.note_names.get(idx).cloned().unwrap_or_else(|| format!("{}", note))
    }

    /// 一行全空白，用于行号越界时的兜底（避免 static_rows 下标 panic）
    pub fn blank_row(&self) -> Vec<Cell> {
        vec![(String::new(), ' '); self.display_width.max(0) as usize]
    }

    pub fn build_static_rows(&mut self) {
        self.static_rows.clear();
        for _ in 0..self.display_height {
            let mut row: Vec<Cell> = Vec::with_capacity(self.display_width as usize);
            for _ in 0..self.display_width { row.push((String::new(), ' ')); }
            self.static_rows.push(row);
        }
    }

    pub fn assign_colors(&mut self, channels: &[u8], tracks: &[usize]) {
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
        let theme = self.cfg.fx.effects.get("theme")
            .and_then(|v| v.get("name"))
            .and_then(|x| x.as_str())
            .unwrap_or("neon")
            .to_string();
        let (fg, bg) = make_palette_pair(keys.len(), &theme);
        self.color_key = by;
        self.fg_map.clear(); self.bg_map.clear();
        for (i, &k) in keys.iter().enumerate() {
            self.fg_map.insert(k, fg[i].clone());
            self.bg_map.insert(k, bg[i].clone());
        }
    }

    pub fn instrument_name(&self, prog: u8) -> String {
        self.instruments_list.get(prog as usize).cloned().unwrap_or_else(|| format!("乐器{}", prog as i32 + 1))
    }

    pub fn set_key_names(&mut self, root: i32, is_minor: bool, ks: i32, match_pct: Option<f64>) {
        self.key_root = root;
        self.is_minor = is_minor;
        let names = spell_names(root, is_minor, ks);
        self.cfg.note_names = names.clone();
        let mode_name = if is_minor { self.lang.s("minor", "小调") } else { self.lang.s("major", "大调") };
        let tonic = names[root as usize].clone();
        self.mode_str = match match_pct {
            Some(p) => format!("{}{}{} ({}{:.1}%)",
                self.lang.s("mode", "调式:"), tonic, mode_name,
                self.lang.s("match", "匹配度:"), p),
            None => format!("{}{}{}",
                self.lang.s("mode", "调式:"), tonic, mode_name),
        };
    }

    pub fn beat_marks(&self, now_abs: f64) -> Vec<(i32, i64)> {
        // 以 32 分音符为最小刻度：k 为从曲首起的 32 分音符序号
        let mut out = Vec::new();
        if self.tempo_map.is_empty() { return out; }
        let hf = self.display_height as f64;
        let bpd = self.cfg.beats_for_drop.max(0.5);
        let fixed_spb_q = self.tempo_map[0].1.max(1) as f64 / 1_000_000.0;
        let (cur_beat, _) = self.beat_at(now_abs);
        let sub = 8.0f64;
        let first = (cur_beat * sub).floor() as i64 + 1;
        let last = first + (bpd * sub).ceil() as i64 + 2;
        for k in first..=last {
            let qb = k as f64 / sub;
            // 二分定位当前所在的速度段（原来逐个扫描，变速多的曲子每帧开销很大）
            let i = self
                .tempo_map
                .partition_point(|&(tick, _)| tick as f64 / self.tpb <= qb)
                .saturating_sub(1)
                .min(self.tempo_map.len() - 1);
            let sb = self.tempo_map[i].0 as f64 / self.tpb;
            let spb_q = self.tempo_map[i].1.max(1) as f64 / 1_000_000.0;
            let sec = self.seg[i] + (qb - sb) * spb_q;
            let drop = if self.cfg.drop_follows_tempo {
                (bpd * spb_q).max(self.cfg.min_drop_floor).max(0.05)
            } else {
                (bpd * fixed_spb_q).max(0.1).max(self.cfg.min_drop_floor).max(0.05)
            };
            let pos = (1.0 - (sec - now_abs) / drop) * hf;
            if pos >= 0.0 && pos < hf {
                out.push((pos as i32, k));
            }
        }
        out
    }

    pub fn beat_at(&self, abs_sec: f64) -> (f64, f64) {
        if self.tempo_map.is_empty() { return (0.0, 120.0); }
        // 二分找当前速度段，避免每帧线性扫描整张 tempo map
        let i = self
            .seg
            .partition_point(|&s| s <= abs_sec)
            .saturating_sub(1)
            .min(self.tempo_map.len() - 1);
        let (tick_i, tempo) = self.tempo_map[i];
        let tempo = tempo.max(1);
        let ticks = tick_i as f64 + (abs_sec - self.seg[i]) * 1_000_000.0 / tempo as f64 * self.tpb;
        (ticks / self.tpb, 60_000_000.0 / tempo as f64)
    }

    pub fn build_header_line1(&self) -> String {
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

    pub fn build_header_line2(&self, played: u64, chord: u64) -> String {
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

    pub fn display_header(&mut self) {
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

    pub fn apply_header_info(&mut self, h: HeaderInfo) {
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

        self.key_events = h.key_events.clone();
        self.climax = h.climax.clone();
        self.key_idx = 0;
        self.tpb = h.ticks_per_beat as f64;
        let mut tm: Vec<(u64, u32)> = Vec::new();
        for &(tick, tempo) in &h.tempo_changes {
            if let Some(last) = tm.last_mut() {
                if last.0 == tick { *last = (tick, tempo); continue; }
            }
            tm.push((tick, tempo));
        }
        if tm.is_empty() { tm.push((0, self.cfg.default_tempo)); }
        let mut sg = vec![0.0f64];
        for i in 1..tm.len() {
            sg.push(sg[i - 1] + (tm[i].0 - tm[i - 1].0) as f64 * tm[i - 1].1 as f64 / self.tpb / 1_000_000.0);
        }
        self.tempo_map = tm;
        self.seg = sg;

        if let Some(&(_, sf0, mi0)) = self.key_events.first() {
            let im = mi0 == 1;
            let root = if im { ((7 * sf0 + 9) % 12 + 12) % 12 } else { ((7 * sf0) % 12 + 12) % 12 };
            self.set_key_names(root, im, sf0, None);
        } else {
            let pitch_hist = h.pitch_hist;
            let wsum: f64 = h.weighted_hist.iter().sum();
            let weighted: Vec<f64> = if wsum <= 0.0 {
                pitch_hist.iter().map(|&x| x as f64).collect()
            } else {
                h.weighted_hist.to_vec()
            };
            let total_w: f64 = weighted.iter().sum::<f64>().max(1.0);
            let mut best: Option<(f64, i32, bool)> = None;
            for (im, profile) in [(false, &MAJOR_PROFILE), (true, &MINOR_PROFILE)] {
                for r in 0..12i32 {
                    let mut score = 0.0f64;
                    for (pc, &w) in weighted.iter().enumerate() {
                        let idx = ((pc as i32 - r) % 12 + 12) % 12;
                        score += w * profile[idx as usize];
                    }
                    if h.bass_pc == Some(r) { score += 0.15 * total_w; }
                    if h.first_pc == Some(r) { score += 0.05 * total_w; }
                    if best.map(|b| score > b.0).unwrap_or(true) {
                        best = Some((score, r, im));
                    }
                }
            }
            let (_, root, im) = best.unwrap();
            let ks = key_sf(root, im);
            let scale: &[i32; 7] = if im { &MINOR_SCALES[root as usize] } else { &MAJOR_SCALES[root as usize] };
            let hit: u64 = scale.iter().map(|&n| pitch_hist[n as usize]).sum();
            let tot: u64 = pitch_hist.iter().sum::<u64>().max(1);
            self.set_key_names(root, im, ks, Some(hit as f64 * 100.0 / tot as f64));
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

    pub fn get_block_visual(&self, key: i32) -> (String, char, String) {
        if self.cfg.block_style == "bg" {
            (self.bg_map.get(&key).cloned().unwrap_or_else(|| format!("{}[41m", ESC)), ' ', self.fg_map.get(&key).cloned().unwrap_or_else(|| format!("{}[91m", ESC)))
        } else {
            (self.fg_map.get(&key).cloned().unwrap_or_else(|| format!("{}[91m", ESC)), '█', self.fg_map.get(&key).cloned().unwrap_or_else(|| format!("{}[91m", ESC)))
        }
    }

    pub fn render_row_cells(&self, line: usize, heads: &[Head], tails: &[Tail], fx: &mut FxManager, ctx: &FrameCtx) -> Vec<Cell> {
        let mut cells = match self.static_rows.get(line) {
            Some(r) => r.clone(),
            None => vec![(String::new(), ' '); self.display_width.max(0) as usize],
        };
        for t in tails {
            if let Some(&center) = self.note_pos_map.get(&(t.note as i32)) {
                if center >= 0 && center < self.display_width {
                    let mut cell: Cell = (t.color.clone(), '|');
                    fx.style_tail(&mut cell, t.note, t.dist, ctx);
                    cells[center as usize] = cell;
                }
            }
        }
        let mut full: HashMap<i32, (u8, i32, u8, u8)> = HashMap::new();
        let mut top: HashMap<i32, (u8, i32, u8, u8)> = HashMap::new();
        let mut bot: HashMap<i32, (u8, i32, u8, u8)> = HashMap::new();
        for h in heads {
            let i = match self.note_to_index.get(&(h.note as i32)) { Some(&i) => i, None => continue };
            let start = self.cell_starts[i];
            let w = self.cell_widths[i];
            let end = (start + w).min(self.display_width);
            let mut k = start;
            while k < end {
                match h.half {
                    Half::Full => { full.insert(k, (h.note, h.key, h.velocity, h.channel)); }
                    Half::Top => { top.insert(k, (h.note, h.key, h.velocity, h.channel)); }
                    Half::Bottom => { bot.insert(k, (h.note, h.key, h.velocity, h.channel)); }
                }
                k += 1;
            }
        }
        let mut keyset: std::collections::HashSet<i32> = std::collections::HashSet::new();
        keyset.extend(full.keys()); keyset.extend(top.keys()); keyset.extend(bot.keys());
        for &k in keyset.iter() {
            if let Some(&(note, key, vel, ch)) = full.get(&k) {
                let f = self.fg_map.get(&key).cloned().unwrap_or_else(|| format!("{}[91m", ESC));
                let b = self.bg_map.get(&key).cloned().unwrap_or_else(|| format!("{}[41m", ESC));
                let mut cell: Cell = if self.cfg.block_style == "bg" { (b, ' ') } else { (f, '█') };
                fx.style_head(&mut cell, note, vel, ch, ctx);
                cells[k as usize] = cell;
            } else if let (Some(&tn), Some(&bn)) = (top.get(&k), bot.get(&k)) {
                let f = self.fg_map.get(&tn.1).cloned().unwrap_or_else(|| format!("{}[91m", ESC));
                let b = self.bg_map.get(&bn.1).cloned().unwrap_or_else(|| format!("{}[41m", ESC));
                cells[k as usize] = (format!("{}{}", f, b), '▀');
            } else if let Some(&(note, key, vel, ch)) = top.get(&k) {
                let f = self.fg_map.get(&key).cloned().unwrap_or_else(|| format!("{}[91m", ESC));
                let mut cell: Cell = (f, '▀');
                fx.style_head(&mut cell, note, vel, ch, ctx);
                cells[k as usize] = cell;
            } else if let Some(&(note, key, vel, ch)) = bot.get(&k) {
                let f = self.fg_map.get(&key).cloned().unwrap_or_else(|| format!("{}[91m", ESC));
                let mut cell: Cell = (f, '▄');
                fx.style_head(&mut cell, note, vel, ch, ctx);
                cells[k as usize] = cell;
            }
        }
        for k in 0..self.display_width {
            fx.paint_bg(&mut cells[k as usize], line as i32, k, ctx);
        }
        cells
    }

    pub fn diff_row_cells(&self, row: i32, old: &[Cell], new: &[Cell]) -> String {
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

    pub fn render_keyboard_cells(&self, active: &HashMap<u8, i32>, fx: &mut FxManager, ctx: &FrameCtx) -> Vec<Cell> {
        let w = self.display_width;
        let mut cols: Vec<Cell> = vec![(String::new(), ' '); w as usize];
        for &(start, width, note, ref name, is_sharp) in &self.kb_cells {
            let is_active = active.contains_key(&(note as u8));
            let mut full_code = String::new();
            if let Some(&key) = active.get(&(note as u8)) {
                if self.cfg.keyboard_highlight == "off" {
                    // 不显示按键高亮
                } else if self.cfg.keyboard_highlight == "bg" {
                    let bg = self.bg_map.get(&key).cloned().unwrap_or_else(|| format!("{}[41m", ESC));
                    full_code = format!("{}{}[1m{}[30m", bg, ESC, ESC);
                } else {
                    let fg = self.fg_map.get(&key).cloned().unwrap_or_else(|| format!("{}[97m", ESC));
                    full_code = format!("{}{}[1m", fg, ESC);
                }
            } else if self.cfg.highlight_middle_c && note == 60 {
                full_code = format!("{}{}[1m{}[30m", self.cfg.middle_c_bg, ESC, ESC);
            }
            for k in 0..width {
                let col = start + k;
                if col >= 0 && col < w {
                    let mut cell: Cell = (full_code.clone(), ' ');
                    fx.style_key(&mut cell, note as u8, is_active, ctx);
                    cols[col as usize] = cell;
                }
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
                if col >= 0 && col < w {
                    let mut cell: Cell = (full_code.clone(), chc);
                    fx.style_key(&mut cell, note as u8, is_active, ctx);
                    cols[col as usize] = cell;
                }
            }
        }
        cols
    }

    pub fn detect_chord(&self, notes: &[u8]) -> (String, Vec<String>, Option<u8>) {
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

    pub fn build_progress_line(&self, played: u64, total: u64, now: f64) -> String {
        let w = (self.term_width as usize).saturating_sub(30).max(8);
        let frac = if total > 0 { (played as f64 / total as f64).clamp(0.0, 1.0) } else { 0.0 };
        let filled = (frac * w as f64).round() as usize;
        let bar: String = "█".repeat(filled) + &"░".repeat(w.saturating_sub(filled));
        format!("{}[1m{}{} {} [{:>5.1}%] {:>5.0}s{}[K",
            ESC, self.lang.s("progress", "进度:"), self.cfg.reset, bar, frac * 100.0, now, ESC)
    }

    pub fn build_chord_line(&self, name: &str, notes: &[String], duration: f64, extra: &[String]) -> String {
        let mut core = if !name.is_empty() {
            format!(
                "{}[1m{}{} {}  {}[1m{}{} {:.2}{}  {}[1m{}{} {}",
                ESC, self.lang.s("label", "和弦:"), self.cfg.reset, name,
                ESC, self.lang.s("duration", "持续:"), self.cfg.reset, duration,
                self.lang.s("seconds", "s"),
                ESC, self.lang.s("components", "构成:"), self.cfg.reset, notes.join(" ")
            )
        } else {
            format!("{}[1m{}{} {}", ESC, self.lang.s("label", "和弦:"), self.cfg.reset, self.lang.s("none", "无"))
        };
        for e in extra {
            core.push_str("  ");
            core.push_str(e);
        }
        core.push_str(ESC);
        core.push_str("[K");
        core
    }

    pub fn update_effect_from_bytes(&mut self, data: &[u8]) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::Lang;
    use serde_json::json;

    fn player() -> Player {
        Player::new(Config::from_value(&json!({})), Lang::empty())
    }

    #[test]
    fn key_signature_follows_circle_of_fifths() {
        assert_eq!(key_sf(0, false), 0);   // C
        assert_eq!(key_sf(7, false), 1);   // G
        assert_eq!(key_sf(5, false), -1);  // F
        assert_eq!(key_sf(3, false), -3);  // Eb
        assert_eq!(key_sf(9, false), 3);   // A
        assert_eq!(key_sf(9, true), 0);    // A 小调 = C 大调调号
    }

    #[test]
    fn spelling_follows_theory() {
        let f = spell_names(5, false, -1); // F 大调：B 应写作 Bb
        assert!(f.iter().any(|n| n == "Bb"));
        assert!(!f.iter().any(|n| n == "A#"));
        let cs = spell_names(1, false, 7); // C# 大调：需要 B# 与 E#
        assert!(cs.iter().any(|n| n == "B#"));
        assert!(cs.iter().any(|n| n == "E#"));
        let cb = spell_names(11, false, -7); // Cb 大调：需要 Cb 与 Fb
        assert!(cb.iter().any(|n| n == "Cb"));
        assert!(cb.iter().any(|n| n == "Fb"));
        // 12 个音级都要有名字，且互不重复
        for names in [f, cs, cb] {
            let mut v = names.clone();
            v.sort();
            v.dedup();
            assert_eq!(v.len(), 12);
        }
    }

    #[test]
    fn chord_detection_basics() {
        let p = player();
        let (name, notes, root) = p.detect_chord(&[60, 64, 67]);
        assert_eq!(name, "C");
        assert_eq!(root, Some(0));
        assert_eq!(notes.len(), 3);
        let (minor, _, _) = p.detect_chord(&[60, 63, 67]);
        assert_eq!(minor, "Cm");
        let (none, _, r) = p.detect_chord(&[]);
        assert!(none.is_empty());
        assert_eq!(r, None);
    }

    #[test]
    fn beat_and_tempo_basics() {
        let mut p = player();
        p.tpb = 480.0;
        p.tempo_map = vec![(0, 500_000)];
        p.seg = vec![0.0];
        let (beat, bpm) = p.beat_at(1.0);
        assert!((bpm - 120.0).abs() < 1e-9);
        assert!((beat - 2.0).abs() < 1e-9);
        let marks = p.beat_marks(0.0);
        assert!(!marks.is_empty());
        assert!(marks.iter().all(|&(row, _)| row >= 0 && row < p.display_height));
    }

    #[test]
    fn beat_at_handles_tempo_changes() {
        let mut p = player();
        p.tpb = 480.0;
        // 0 拍 120BPM，2 拍后变 60BPM
        p.tempo_map = vec![(0, 500_000), (960, 1_000_000)];
        p.seg = vec![0.0, 1.0];
        let (beat, bpm) = p.beat_at(0.5);
        assert!((bpm - 120.0).abs() < 1e-9);
        assert!((beat - 1.0).abs() < 1e-9);
        let (beat2, bpm2) = p.beat_at(2.0);
        assert!((bpm2 - 60.0).abs() < 1e-9);
        assert!((beat2 - 3.0).abs() < 1e-9);
    }

    #[test]
    fn layout_survives_short_note_names() {
        let mut cfg = Config::from_value(&json!({}));
        cfg.note_names = vec!["X".into()]; // 故意写坏
        let p = Player::new(cfg, Lang::empty());
        assert!(p.display_width > 0);
        assert!(p.kb_cells.len() > 0);
        // 越界行号也只给空行，不 panic
        let blank = p.blank_row();
        assert_eq!(blank.len() as i32, p.display_width);
        let _ = p.render_row_cells(9999, &[], &[], &mut FxManager::build(&Config::from_value(&json!({})).fx), &FrameCtx { now: 0.0, frame_dt: 0.016, beat: 0.0, beat_phase: 0.0, bar: 0, width: p.display_width, height: p.display_height, beats_for_drop: 4.0, beat_marks: vec![], climax: vec![], key_root: 0, is_minor: false, progress: 0.0, bpm: 120.0, chord_name: String::new(), chord_root: -1 });
    }
}


// ---------- 输入/路径 ----------
