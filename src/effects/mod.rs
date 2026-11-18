use crate::Cell;
use serde_json::Value;

const ESC: &str = "\x1b";

mod velocity_glow;
mod trail_fade;
mod attack_flash;
mod beat_grid;
mod mode_palette;
mod key_ripple;
mod percussion_mark;
mod screen_flash;
mod particles;
mod chord_degrees;
mod climax_shake;
mod aurora;
mod shockwave;
mod fireworks;
pub mod theme;

pub struct FrameCtx {
    pub now: f64,
    pub frame_dt: f64,
    pub beat: f64,
    pub beat_phase: f64,
    pub bar: u32,
    pub width: i32,
    pub height: i32,
    pub beats_for_drop: f64,
    pub beat_marks: Vec<(i32, i64)>,
    pub climax: Vec<(f64, f64)>,
    pub key_root: i32,
    pub is_minor: bool,
    pub progress: f64,
    pub bpm: f64,
    pub chord_name: String,
    pub chord_root: i32,
}

pub trait Effect {
    fn begin_frame(&mut self, _ctx: &FrameCtx) {}
    fn paint_bg(&mut self, _cell: &mut Cell, _row: i32, _col: i32, _ctx: &FrameCtx) {}
    fn style_head(&mut self, _cell: &mut Cell, _note: u8, _velocity: u8, _channel: u8, _ctx: &FrameCtx) {}
    fn style_tail(&mut self, _cell: &mut Cell, _note: u8, _dist: f64, _ctx: &FrameCtx) {}
    fn style_key(&mut self, _cell: &mut Cell, _note: u8, _active: bool, _ctx: &FrameCtx) {}
    fn on_note_on(&mut self, _note: u8, _velocity: u8, _col: i32, _ctx: &FrameCtx) {}
    fn hud_extra(&mut self, _ctx: &FrameCtx) -> Option<String> { None }
    fn needs_full_grid(&self) -> bool { false }
    fn force_full_redraw(&self) -> bool { false }
    fn bg_rows(&self, _ctx: &FrameCtx) -> Vec<i32> { Vec::new() }
    fn screen_offset(&mut self, _ctx: &FrameCtx) -> (i32, i32) { (0, 0) }
}

pub(crate) fn pbool(v: &Value, k: &str, d: bool) -> bool { v.get(k).and_then(|x| x.as_bool()).unwrap_or(d) }
pub(crate) fn pf64(v: &Value, k: &str, d: f64) -> f64 { v.get(k).and_then(|x| x.as_f64()).unwrap_or(d) }
pub(crate) fn pi32(v: &Value, k: &str, d: i32) -> i32 { v.get(k).and_then(|x| x.as_i64()).map(|n| n as i32).unwrap_or(d) }
pub(crate) fn pstr(v: &Value, k: &str, d: &str) -> String { v.get(k).and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or_else(|| d.to_string()) }
pub(crate) fn pchar(v: &Value, k: &str, d: char) -> char {
    v.get(k).and_then(|x| x.as_str()).and_then(|s| s.chars().next()).unwrap_or(d)
}


use crate::config::FxConfig;
use serde_json::Map;

pub struct EffectDef {
    pub name: &'static str,
    pub defaults: fn() -> Value,
    pub build: fn(&Value) -> Box<dyn Effect>,
}

pub fn registry() -> Vec<EffectDef> {
    vec![
        EffectDef { name: "mode_palette", defaults: mode_palette::defaults, build: mode_palette::build },
        EffectDef { name: "percussion_mark", defaults: percussion_mark::defaults, build: percussion_mark::build },
        EffectDef { name: "velocity_glow", defaults: velocity_glow::defaults, build: velocity_glow::build },
        EffectDef { name: "screen_flash", defaults: screen_flash::defaults, build: screen_flash::build },
        EffectDef { name: "trail_fade", defaults: trail_fade::defaults, build: trail_fade::build },
        EffectDef { name: "attack_flash", defaults: attack_flash::defaults, build: attack_flash::build },
        EffectDef { name: "key_ripple", defaults: key_ripple::defaults, build: key_ripple::build },
        EffectDef { name: "beat_grid", defaults: beat_grid::defaults, build: beat_grid::build },
        EffectDef { name: "particles", defaults: particles::defaults, build: particles::build },
        EffectDef { name: "chord_degrees", defaults: chord_degrees::defaults, build: chord_degrees::build },
        EffectDef { name: "aurora", defaults: aurora::defaults, build: aurora::build },
        EffectDef { name: "shockwave", defaults: shockwave::defaults, build: shockwave::build },
        EffectDef { name: "fireworks", defaults: fireworks::defaults, build: fireworks::build },
        EffectDef { name: "climax_shake", defaults: climax_shake::defaults, build: climax_shake::build },
    ]
}

/// 所有特效库声明的默认配置，供 config 自动补全/修正使用
pub fn default_effects() -> Map<String, Value> {
    let mut m = Map::new();
    for d in registry() {
        m.insert(d.name.to_string(), (d.defaults)());
    }
    m.insert("theme".to_string(), theme::defaults());
    m
}

pub struct FxManager {
    pub effects: Vec<Box<dyn Effect>>,
    pub level: u8,
}

impl FxManager {
    pub fn build(cfg: &FxConfig) -> FxManager {
        let mut effects: Vec<Box<dyn Effect>> = Vec::new();
        if !cfg.enabled { return FxManager { effects, level: cfg.level }; }
        for def in registry() {
            let mut v = cfg.effects.get(def.name).cloned().unwrap_or_else(|| (def.defaults)());
            if let (Some(obj), Some(defaults)) = (v.as_object_mut(), (def.defaults)().as_object()) {
                for (k, dv) in defaults {
                    obj.entry(k.clone()).or_insert_with(|| dv.clone());
                }
            }
            let enabled = v.get("enabled").and_then(|x| x.as_bool()).unwrap_or(false);
            if enabled {
                effects.push((def.build)(&v));
            }
        }
        FxManager { effects, level: cfg.level }
    }

    pub fn needs_full_grid(&self) -> bool { self.effects.iter().any(|e| e.needs_full_grid()) }

    pub fn begin_frame(&mut self, ctx: &FrameCtx) {
        for e in &mut self.effects { e.begin_frame(ctx); }
    }
    pub fn paint_bg(&mut self, cell: &mut Cell, row: i32, col: i32, ctx: &FrameCtx) {
        for e in &mut self.effects { e.paint_bg(cell, row, col, ctx); }
    }
    pub fn style_head(&mut self, cell: &mut Cell, note: u8, velocity: u8, channel: u8, ctx: &FrameCtx) {
        for e in &mut self.effects { e.style_head(cell, note, velocity, channel, ctx); }
    }
    pub fn style_tail(&mut self, cell: &mut Cell, note: u8, dist: f64, ctx: &FrameCtx) {
        for e in &mut self.effects { e.style_tail(cell, note, dist, ctx); }
    }
    pub fn style_key(&mut self, cell: &mut Cell, note: u8, active: bool, ctx: &FrameCtx) {
        for e in &mut self.effects { e.style_key(cell, note, active, ctx); }
    }
    pub fn on_note_on(&mut self, note: u8, velocity: u8, col: i32, ctx: &FrameCtx) {
        for e in &mut self.effects { e.on_note_on(note, velocity, col, ctx); }
    }
    pub fn screen_offset(&mut self, ctx: &FrameCtx) -> (i32, i32) {
        let mut best: (i32, i32) = (0, 0);
        for e in &mut self.effects {
            let o = e.screen_offset(ctx);
            if o.0.abs() > best.0.abs() { best.0 = o.0; }
            if o.1.abs() > best.1.abs() { best.1 = o.1; }
        }
        best
    }
    pub fn force_full_redraw(&self) -> bool { self.effects.iter().any(|e| e.force_full_redraw()) }

    pub fn bg_rows(&self, ctx: &FrameCtx) -> Vec<i32> {
        self.effects.iter().flat_map(|e| e.bg_rows(ctx)).collect()
    }

    pub fn hud_extra(&mut self, ctx: &FrameCtx) -> Vec<String> {
        let mut v = Vec::new();
        for e in &mut self.effects {
            if let Some(s) = e.hud_extra(ctx) { v.push(s); }
        }
        v
    }
}
