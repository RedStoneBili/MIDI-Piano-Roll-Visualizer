#![allow(unused_imports)]
use crate::Cell;
use crate::effects::{pchar, pi32, pstr, Effect, FrameCtx};
use serde_json::{json, Value};
const ESC: &str = "\x1b";

pub struct BeatGrid {
    beats_per_bar: f64,
    beat_unit: i32,
    beat_char: char,
    beat_full_width: bool,
    bar_char: char,
    color: String,
    lines: Vec<(i32, char, bool)>,
}

impl Effect for BeatGrid {
    fn begin_frame(&mut self, ctx: &FrameCtx) {
        self.lines.clear();
        let step = (32 / self.beat_unit.max(1)).max(1) as i64;
        for &(row, k) in &ctx.beat_marks {
            if k % step != 0 { continue; }
            let idx = k / step;
            let is_bar = self.beats_per_bar > 0.0 && (idx as f64 % self.beats_per_bar).abs() < 1e-6;
            let ch = if is_bar { self.bar_char } else { self.beat_char };
            if ch == ' ' { continue; }
            self.lines.push((row, ch, is_bar));
        }
    }
    fn paint_bg(&mut self, cell: &mut Cell, row: i32, col: i32, _ctx: &FrameCtx) {
        if !cell.0.is_empty() || cell.1 != ' ' { return; }
        for &(r, ch, is_bar) in &self.lines {
            if r != row { continue; }
            if !is_bar {
                if self.beat_full_width {
                    if col % 2 != 0 { return; }
                } else if col != 0 {
                    return;
                }
            }
            cell.0 = self.color.clone();
            cell.1 = ch;
            return;
        }
    }
    fn needs_full_grid(&self) -> bool { false }
    fn bg_rows(&self, _ctx: &FrameCtx) -> Vec<i32> {
        self.lines.iter().map(|x| x.0).collect()
    }
}

pub fn defaults() -> Value {
    json!({"enabled": true, "beat_char": "·", "bar_char": "─", "beat_unit": 4, "beats_per_bar": 4, "beat_full_width": false, "color": format!("{}[90m", ESC)})
}

pub fn build(v: &Value) -> Box<dyn Effect> {
    let bpb = v.get("beats_per_bar").and_then(|x| x.as_i64())
        .or_else(|| v.get("bar_every").and_then(|x| x.as_i64()))
        .unwrap_or(4).max(1) as f64;
    let mut unit = pi32(v, "beat_unit", 4);
    if ![2, 4, 8, 16, 32].contains(&unit) { unit = 4; }
    Box::new(BeatGrid {
        beats_per_bar: bpb,
        beat_unit: unit,
        beat_char: pchar(v, "beat_char", '·'),
        beat_full_width: v.get("beat_full_width").and_then(|x| x.as_bool()).unwrap_or(false),
        bar_char: pchar(v, "bar_char", '─'),
        color: pstr(v, "color", &format!("{}[90m", ESC)),
        lines: Vec::new(),
    })
}
