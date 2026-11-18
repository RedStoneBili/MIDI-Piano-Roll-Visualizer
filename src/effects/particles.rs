#![allow(unused_imports)]
use crate::Cell;
use crate::effects::{pbool, pchar, pf64, pi32, pstr, Effect, FrameCtx};
use serde_json::{json, Value};
const ESC: &str = "\x1b";

struct Particles { density: u32 }
fn hash3(a: i32, b: i32, c: i32) -> u32 {
    let mut h = (a as u32).wrapping_mul(374761393)
        .wrapping_add((b as u32).wrapping_mul(668265263))
        .wrapping_add((c as u32).wrapping_mul(2246822519));
    h ^= h >> 13;
    h = h.wrapping_mul(1274126177);
    h ^ (h >> 16)
}
impl Effect for Particles {
    fn paint_bg(&mut self, cell: &mut Cell, row: i32, col: i32, ctx: &FrameCtx) {
        if !cell.0.is_empty() || cell.1 != ' ' { return; }
        let t = (ctx.now * 2.0) as i32;
        if hash3(row, col, t) % 1000 < self.density {
            cell.0 = format!("{}[90m", ESC);
            cell.1 = '.';
        }
    }
    fn needs_full_grid(&self) -> bool { true }
}

pub fn defaults() -> Value {
    json!({"enabled": false, "density": 20})
}

pub fn build(v: &Value) -> Box<dyn Effect> { Box::new(Particles { density: pi32(v, "density", 20).max(0) as u32 }) }
