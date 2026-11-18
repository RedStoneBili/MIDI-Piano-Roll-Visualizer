#![allow(unused_imports)]
use crate::Cell;
use crate::effects::{pbool, pchar, pf64, pi32, pstr, Effect, FrameCtx};
use serde_json::{json, Value};
const ESC: &str = "\x1b";

struct KeyRipple { radius: i32, frames: f64, ripples: Vec<(u8, f64)> }
impl Effect for KeyRipple {
    fn begin_frame(&mut self, ctx: &FrameCtx) {
        self.ripples.retain(|(_, t)| ctx.now - *t < self.frames * ctx.frame_dt.max(0.001));
    }
    fn on_note_on(&mut self, note: u8, _v: u8, _col: i32, ctx: &FrameCtx) {
        self.ripples.push((note, ctx.now));
    }
    fn style_key(&mut self, cell: &mut Cell, note: u8, _active: bool, ctx: &FrameCtx) {
        let mut best = 0.0f64;
        for (n, t) in &self.ripples {
            let d = (note as i32 - *n as i32).abs();
            if d > self.radius {
                continue;
            }
            let age = ctx.now - *t;
            let life = self.frames * ctx.frame_dt.max(0.001);
            if age < life {
                let k = 1.0 - age / life;
                let k = k * (1.0 - d as f64 / (self.radius as f64 + 1.0));
                if k > best { best = k; }
            }
        }
        if best > 0.05 {
            cell.0 = format!("{}[48;5;{}m{}", ESC, (52 + (best * 20.0) as i32).min(231), cell.0);
        }
    }
}

pub fn defaults() -> Value {
    json!({"enabled": false, "radius": 3, "frames": 6})
}

pub fn build(v: &Value) -> Box<dyn Effect> { Box::new(KeyRipple { radius: pi32(v, "radius", 3), frames: pf64(v, "frames", 6.0), ripples: Vec::new() }) }
