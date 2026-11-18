use crate::climax::ClimaxCfg;
use serde_json::{json, Map, Value};

const ESC: &str = "\x1b";

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

#[derive(Clone)]
pub struct PlaylistConfig {
    pub mode: String, // order | loop | random
    pub recursive: bool,
}

#[derive(Clone)]
pub struct FxConfig {
    pub enabled: bool,
    pub level: u8,
    pub effects: Map<String, Value>,
}

#[derive(Clone)]
pub struct Config {
    pub language: String,
    pub note_names: Vec<String>,
    pub reset: String,
    pub highlight_middle_c: bool,
    pub middle_c_bg: String,
    pub color_by: String,
    pub beats_for_drop: f64,
    pub drop_follows_tempo: bool,
    pub min_drop_floor: f64,
    pub refresh_rate: f64,
    pub block_style: String,
    pub trail_length: i32,
    pub trail_by_note: bool,
    pub dual_line_interp: bool,
    pub half_cell_notes: bool,
    pub max_nps: i64,
    pub sync_output: Option<bool>,
    pub debug_info: bool,
    pub default_tempo: u32,
    pub min_note_opt: Option<i32>,
    pub max_note_opt: Option<i32>,
    pub midi_queue_size: usize,
    pub progress_bar: bool,
    pub keyboard_highlight: String,
    pub fx: FxConfig,
    pub playlist: PlaylistConfig,
    pub climax: ClimaxCfg,
}

fn merge_effect(m: &mut Map<String, Value>, name: &str, val: &Value, defaults: &Value) {
    // 未注册的特效名直接忽略（视为多余配置）
    if !m.contains_key(name) { return; }
    let mut obj = m.get(name).and_then(|x| x.as_object()).cloned().unwrap_or_default();
    let allowed = |k: &str| k == "enabled" || defaults.get(k).is_some();
    match val {
        Value::Bool(b) => { obj.insert("enabled".into(), Value::Bool(*b)); }
        Value::Object(o) => {
            for (k, v) in o {
                if allowed(k) { obj.insert(k.clone(), v.clone()); }
            }
        }
        _ => return,
    }
    m.insert(name.into(), Value::Object(obj));
}

pub const DEFAULT_NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// 音名表必须正好覆盖 12 个音级，否则渲染时 note%12 会越界 panic。
/// 这里把用户配置补齐/截断成 12 项，空字符串也回退成默认音名。
pub fn normalize_note_names(v: Option<&Value>) -> Vec<String> {
    let mut names: Vec<String> = v
        .and_then(|x| x.as_array())
        .map(|a| a.iter().take(12).filter_map(|x| x.as_str()).map(|s| s.to_string()).collect())
        .unwrap_or_default();
    while names.len() < 12 {
        names.push(DEFAULT_NOTE_NAMES[names.len()].to_string());
    }
    for (i, n) in names.iter_mut().enumerate() {
        if n.trim().is_empty() {
            *n = DEFAULT_NOTE_NAMES[i].to_string();
        }
    }
    names
}

impl Config {
    pub fn from_value(v: &Value) -> Config {
        let nn = normalize_note_names(v.get("note_names"));

        let mut effects = crate::effects::default_effects();
        let defaults_map = effects.clone();
        let mut fx_enabled = jbool(v, "fx_enabled", true);
        let mut fx_level = 1u8;
        if let Some(fx) = v.get("fx").and_then(|x| x.as_object()) {
            if let Some(b) = fx.get("enabled").and_then(|x| x.as_bool()) { fx_enabled = b; }
            if let Some(l) = fx.get("level").and_then(|x| x.as_u64()) { fx_level = l.min(2) as u8; }
            for (k, val) in fx {
                if k == "enabled" || k == "level" || k == "effects" { continue; }
                if let Some(dv) = defaults_map.get(k) { merge_effect(&mut effects, k, val, dv); }
            }
            if let Some(eff) = fx.get("effects").and_then(|x| x.as_object()) {
                for (k, val) in eff {
                    if let Some(dv) = defaults_map.get(k) { merge_effect(&mut effects, k, val, dv); }
                }
            }
        }

        let climax = crate::climax::from_value(v.get("climax").unwrap_or(&Value::Null));

        let mut playlist = PlaylistConfig { mode: "order".into(), recursive: false };
        if let Some(p) = v.get("playlist").and_then(|x| x.as_object()) {
            if let Some(m) = p.get("mode").and_then(|x| x.as_str()) {
                let m = m.to_lowercase();
                if m == "order" || m == "loop" || m == "random" { playlist.mode = m; }
            }
            if let Some(b) = p.get("recursive").and_then(|x| x.as_bool()) { playlist.recursive = b; }
        }

        let mut block_style = jstr(v, "block_style").unwrap_or_else(|| "bg".into()).to_lowercase();
        if block_style != "fg" && block_style != "bg" { block_style = "bg".into(); }

        Config {
            language: jstr(v, "language").unwrap_or_else(|| "lang/zh-CN.json".into()),
            note_names: nn,
            reset: jstr(v, "color_reset").unwrap_or_else(|| format!("{}[0m", ESC)),
            highlight_middle_c: jbool(v, "highlight_middle_c", true),
            middle_c_bg: jstr(v, "middle_c_bg").unwrap_or_else(|| format!("{}[107m", ESC)),
            color_by: jstr(v, "color_by").unwrap_or_else(|| "auto".into()),
            beats_for_drop: jf64(v, "beats_for_drop", 2.0).clamp(0.25, 64.0),
            drop_follows_tempo: jbool(v, "drop_follows_tempo", true),
            min_drop_floor: jf64(v, "min_drop_time_seconds", 0.15).clamp(0.02, 5.0),
            refresh_rate: jf64(v, "refresh_rate", 60.0).clamp(30.0, 120.0),
            block_style,
            trail_length: ji32(v, "trail_length", 0).max(-1),
            trail_by_note: jbool(v, "trail_by_note", false),
            dual_line_interp: jbool(v, "dual_line_interp", false),
            half_cell_notes: jbool(v, "half_cell_notes", true),
            max_nps: v.get("max_notes_per_second").and_then(|x| x.as_i64()).unwrap_or(0).max(-1),
            sync_output: v.get("sync_output").and_then(|x| x.as_bool()),
            debug_info: jbool(v, "debug_info", false),
            // tempo 为 0 会让 ticks→秒 的换算除零，回退到默认值
            default_tempo: match ju32(v, "default_tempo", 500000) { 0 => 500000, t => t },
            min_note_opt: v.get("min_note").and_then(|x| x.as_i64()).map(|n| (n as i32).clamp(0, 127)),
            max_note_opt: v.get("max_note").and_then(|x| x.as_i64()).map(|n| (n as i32).clamp(0, 127)),
            midi_queue_size: v.get("midi_queue_size").and_then(|x| x.as_i64()).unwrap_or(0).max(0) as usize,
            progress_bar: jbool(v, "progress_bar", true),
            keyboard_highlight: {
                let s = jstr(v, "keyboard_highlight").unwrap_or_else(|| "bg".into()).to_lowercase();
                if s == "bg" { "bg".into() } else if s == "off" { "off".into() } else { "fg".into() }
            },
            fx: FxConfig { enabled: fx_enabled, level: fx_level, effects },
            playlist,
            climax,
        }
    }

    pub fn to_value(&self) -> Value {
        json!({
            "language": self.language,
            "note_names": self.note_names,
            "color_reset": self.reset,
            "color_clear": format!("{}[H", ESC),
            "color_bg_white": format!("{}[47m", ESC),
            "default_tempo": self.default_tempo,
            "ticks_per_beat": 480,
            "highlight_middle_c": self.highlight_middle_c,
            "middle_c_bg": self.middle_c_bg,
            "color_by": self.color_by,
            "beats_for_drop": self.beats_for_drop,
            "drop_follows_tempo": self.drop_follows_tempo,
            "min_drop_time_seconds": self.min_drop_floor,
            "refresh_rate": self.refresh_rate,
            "block_style": self.block_style,
            "trail_length": self.trail_length,
            "trail_by_note": self.trail_by_note,
            "dual_line_interp": self.dual_line_interp,
            "half_cell_notes": self.half_cell_notes,
            "max_notes_per_second": self.max_nps,
            "midi_queue_size": self.midi_queue_size,
            "min_note": self.min_note_opt,
            "max_note": self.max_note_opt,
            "progress_bar": self.progress_bar,
            "keyboard_highlight": self.keyboard_highlight,
            "sync_output": self.sync_output,
            "debug_info": self.debug_info,
            "fx": {
                "enabled": self.fx.enabled,
                "level": self.fx.level,
                "effects": self.fx.effects
            },
            "playlist": {
                "mode": self.playlist.mode,
                "recursive": self.playlist.recursive
            },
            "climax": crate::climax::to_value(&self.climax)

        })
    }

    /// 读取并自修正/补全 config.json；返回 (配置, 是否写回)
    pub fn load_and_heal(path: &str) -> (Config, bool) {
        let old_text = std::fs::read_to_string(path).unwrap_or_default();
        let raw: Value = serde_json::from_str(&old_text).unwrap_or_else(|_| Value::Object(Map::new()));
        let cfg = Config::from_value(&raw);
        let canon = cfg.to_value();
        let new_text = serde_json::to_string_pretty(&canon).unwrap_or_default();
        let changed = old_text.trim() != new_text.trim();
        if changed {
            // 若目标文件不是本程序的配置（例如同目录下 Python 版留下的 config.json），
            // 先备份一份，避免把它直接覆盖掉。
            let looks_rust = ["\"fx\"", "\"climax\"", "\"progress_bar\"", "\"midi_queue_size\""]
                .iter().any(|k| old_text.contains(k));
            if !old_text.trim().is_empty() && !looks_rust {
                let _ = std::fs::copy(path, format!("{}.bak", path));
            }
            let _ = std::fs::write(path, &new_text);
        }
        (cfg, changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn note_names_are_always_twelve() {
        let n = normalize_note_names(None);
        assert_eq!(n.len(), 12);
        assert_eq!(n[1], "C#");

        let short = json!(["A", "B"]);
        let n = normalize_note_names(Some(&short));
        assert_eq!(n.len(), 12);
        assert_eq!(&n[0..2], ["A", "B"]);
        assert_eq!(n[2], "D"); // 缺的项用默认音名补齐

        let blank = json!(["", "B"]);
        let n = normalize_note_names(Some(&blank));
        assert_eq!(n[0], "C"); // 空字符串回退默认

        let long = json!(["1","2","3","4","5","6","7","8","9","10","11","12","13","14"]);
        assert_eq!(normalize_note_names(Some(&long)).len(), 12);
    }

    #[test]
    fn bad_values_are_clamped() {
        let c = Config::from_value(&json!({
            "beats_for_drop": 0.0,
            "refresh_rate": 1.0,
            "default_tempo": 0,
            "min_drop_time_seconds": -1.0,
            "min_note": 300,
            "max_note": -5
        }));
        assert_eq!(c.beats_for_drop, 0.25);
        assert_eq!(c.refresh_rate, 30.0);
        assert_eq!(c.default_tempo, 500000);
        assert_eq!(c.min_drop_floor, 0.02);
        assert_eq!(c.min_note_opt, Some(127));
        assert_eq!(c.max_note_opt, Some(0));
    }

    #[test]
    fn enum_like_strings_fall_back() {
        let c = Config::from_value(&json!({
            "keyboard_highlight": "weird",
            "block_style": "xx",
            "playlist": { "mode": "nope" }
        }));
        assert_eq!(c.keyboard_highlight, "fg");
        assert_eq!(c.block_style, "bg");
        assert_eq!(c.playlist.mode, "order");
    }

    #[test]
    fn value_roundtrip_is_stable() {
        let c = Config::from_value(&json!({ "language": "lang/en-US.json" }));
        let v1 = serde_json::to_string(&c.to_value()).unwrap();
        let c2 = Config::from_value(&c.to_value());
        let v2 = serde_json::to_string(&c2.to_value()).unwrap();
        assert_eq!(v1, v2);
        assert_eq!(c2.language, "lang/en-US.json");
    }
}
