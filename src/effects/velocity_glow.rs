#![allow(unused_imports)]
use crate::Cell;
use crate::effects::{pbool, pchar, pf64, pi32, pstr, Effect, FrameCtx};
use serde_json::{json, Value};
const ESC: &str = "\x1b";

struct VelocityGlow { threshold: f64 }
impl Effect for VelocityGlow {
    fn style_head(&mut self, cell: &mut Cell, _note: u8, velocity: u8, _ch: u8, _ctx: &FrameCtx) {
        if velocity as f64 >= self.threshold {
            cell.0 = format!("{}[1m{}", ESC, cell.0);
        }
    }
}

pub fn defaults() -> Value {
    json!({"enabled": true, "threshold": 100})
}

pub fn build(v: &Value) -> Box<dyn Effect> { Box::new(VelocityGlow { threshold: pf64(v, "threshold", 100.0) }) }
