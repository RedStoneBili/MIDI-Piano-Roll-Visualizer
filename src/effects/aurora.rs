#![allow(unused_imports)]
use crate::Cell;
use crate::effects::{pf64, Effect, FrameCtx};
use serde_json::{json, Value};
const ESC: &str = "\x1b";

pub struct Aurora { speed: f64 }

impl Effect for Aurora {
    fn paint_bg(&mut self, cell: &mut Cell, row: i32, col: i32, ctx: &FrameCtx) {
        if !cell.0.is_empty() || cell.1 != ' ' { return; }
        let t = (ctx.now * 4.0).floor() / 4.0;
        let x = col as f64 * 0.12 + t * 0.7 * self.speed;
        let y = row as f64 * 0.23 - t * 1.3 * self.speed;
        let v = (x.sin() + (y * 0.8).sin() + ((x * 0.5) + y * 0.6).sin()) / 3.0;
        if v > 0.30 {
            let shade = ((v - 0.30) / 0.70).clamp(0.0, 1.0);
            let idx = (17.0 + shade * 165.0) as i32;
            cell.0 = format!("{}[48;5;{}m", ESC, idx.clamp(16, 231));
            cell.1 = ' ';
        }
    }
    fn needs_full_grid(&self) -> bool { true }
}

pub fn defaults() -> Value {
    json!({"enabled": false, "speed": 1.0})
}

pub fn build(v: &Value) -> Box<dyn Effect> {
    Box::new(Aurora { speed: pf64(v, "speed", 1.0).clamp(0.2, 4.0) })
}
