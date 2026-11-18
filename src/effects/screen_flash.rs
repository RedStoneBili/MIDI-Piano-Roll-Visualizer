#![allow(unused_imports)]
use crate::Cell;
use crate::effects::{pbool, pchar, pf64, pi32, pstr, Effect, FrameCtx};
use serde_json::{json, Value};
const ESC: &str = "\x1b";

struct ScreenFlash { on_bar: bool, active: bool }
impl Effect for ScreenFlash {
    fn begin_frame(&mut self, ctx: &FrameCtx) {
        self.active = self.on_bar && ctx.beat_phase < 0.07;
    }
    fn style_head(&mut self, cell: &mut Cell, _n: u8, _v: u8, _c: u8, _ctx: &FrameCtx) {
        if self.active { cell.0 = format!("{}[1m", ESC); }
    }
    fn style_key(&mut self, cell: &mut Cell, _n: u8, _a: bool, _ctx: &FrameCtx) {
        if self.active { cell.0 = format!("{}[1m", ESC); }
    }
}

pub fn defaults() -> Value {
    json!({"enabled": false, "on_bar": true})
}

pub fn build(v: &Value) -> Box<dyn Effect> { Box::new(ScreenFlash { on_bar: pbool(v, "on_bar", true), active: false }) }
