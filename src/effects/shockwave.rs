#![allow(unused_imports)]
use crate::Cell;
use crate::effects::{pf64, pi32, Effect, FrameCtx};
use serde_json::{json, Value};
const ESC: &str = "\x1b";

pub struct Shockwave {
    waves: Vec<(f64, f64, f64)>,
    beats_per_bar: f64,
    last_bar: i64,
    thickness: f64,
    radius: f64,   // 0 = 自动铺满屏幕
    duration: f64, // 一次冲击波持续秒数
    use_256: bool, // 是否使用 256 色
}

impl Effect for Shockwave {
    fn begin_frame(&mut self, ctx: &FrameCtx) {
        self.waves.retain(|(t, _, _)| ctx.now - *t < self.duration);
        if self.beats_per_bar > 0.0 {
            let bar = (ctx.beat / self.beats_per_bar).floor() as i64;
            if bar != self.last_bar {
                self.last_bar = bar;
                if bar > 0 {
                    self.waves.push((ctx.now, ctx.width as f64 / 2.0, ctx.height as f64 - 1.0));
                }
            }
        }
        if self.waves.len() > 8 {
            let n = self.waves.len() - 8;
            self.waves.drain(0..n);
        }
    }
    fn on_note_on(&mut self, _note: u8, velocity: u8, col: i32, ctx: &FrameCtx) {
        if velocity >= 110 {
            self.waves.push((ctx.now, col as f64, ctx.height as f64 - 1.0));
        }
    }
    fn paint_bg(&mut self, cell: &mut Cell, row: i32, col: i32, ctx: &FrameCtx) {
        if !cell.0.is_empty() || cell.1 != ' ' { return; }
        let max_r = if self.radius > 0.0 {
            self.radius
        } else {
            ctx.width.max(ctx.height) as f64 * 0.9
        };
        for &(t, cx, cy) in &self.waves {
            let age = ctx.now - t;
            let r = age / self.duration * max_r;
            let d = ((col as f64 - cx).powi(2) + (row as f64 - cy).powi(2)).sqrt();
            if (d - r).abs() <= self.thickness.max(0.6) {
                let k = (1.0 - age / self.duration).clamp(0.0, 1.0);
                cell.0 = if self.use_256 {
                    let idx = (196.0 * k) as i32;
                    format!("{}[38;5;{}m", ESC, idx.clamp(16, 231))
                } else if k > 0.66 {
                    format!("{}[97m", ESC)
                } else if k > 0.33 {
                    format!("{}[93m", ESC)
                } else {
                    format!("{}[91m", ESC)
                };
                cell.1 = if age < self.duration * 0.3 { '█' } else { '▒' };
                return;
            }
        }
    }
    fn needs_full_grid(&self) -> bool { true }
}

pub fn defaults() -> Value {
    json!({"enabled": false, "beats_per_bar": 4, "thickness": 1.0, "radius": 0.0, "duration": 2.2, "use_256": true})
}

pub fn build(v: &Value) -> Box<dyn Effect> {
    Box::new(Shockwave {
        waves: Vec::new(),
        beats_per_bar: pi32(v, "beats_per_bar", 4).max(1) as f64,
        last_bar: -1,
        thickness: pf64(v, "thickness", 1.0).clamp(0.5, 3.0),
        radius: pf64(v, "radius", 0.0).max(0.0),
        duration: pf64(v, "duration", 2.2).clamp(0.3, 10.0),
        use_256: v.get("use_256").and_then(|x| x.as_bool()).unwrap_or(true),
    })
}
