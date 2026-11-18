#![allow(unused_imports)]
use crate::Cell;
use crate::effects::{pbool, pchar, pf64, pi32, pstr, Effect, FrameCtx};
use serde_json::{json, Value};
const ESC: &str = "\x1b";

struct TrailFade { chars: Vec<char> }
impl Effect for TrailFade {
    fn style_tail(&mut self, cell: &mut Cell, _note: u8, dist: f64, _ctx: &FrameCtx) {
        let n = self.chars.len();
        if n == 0 { return; }
        let idx = ((dist.clamp(0.0, 1.0)) * (n as f64)) as usize;
        let idx = idx.min(n - 1);
        cell.1 = self.chars[idx];
        if dist > 0.66 {
            cell.0 = format!("{}[2m{}", ESC, cell.0);
        }
    }
}

pub fn defaults() -> Value {
    json!({"enabled": false, "chars": "█▓▒░"})
}

pub fn build(v: &Value) -> Box<dyn Effect> { let chars: Vec<char> = pstr(v, "chars", "█▓▒░").chars().collect(); Box::new(TrailFade { chars }) }
