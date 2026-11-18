#![allow(unused_imports)]
use crate::Cell;
use crate::effects::{pbool, pchar, pf64, pi32, pstr, Effect, FrameCtx};
use serde_json::{json, Value};
const ESC: &str = "\x1b";

struct AttackFlash { frames: f64, keyboard: bool, until: std::collections::HashMap<u8, f64> }
impl Effect for AttackFlash {
    fn begin_frame(&mut self, ctx: &FrameCtx) {
        self.until.retain(|_, t| *t > ctx.now);
    }
    fn on_note_on(&mut self, note: u8, _v: u8, _col: i32, ctx: &FrameCtx) {
        self.until.insert(note, ctx.now + self.frames * ctx.frame_dt.max(0.001));
    }
    fn style_key(&mut self, cell: &mut Cell, note: u8, _active: bool, ctx: &FrameCtx) {
        if !self.keyboard { return; }
        if let Some(t) = self.until.get(&note) {
            if *t > ctx.now {
                cell.0 = format!("{}[7m", ESC);
            }
        }
    }
}

pub fn defaults() -> Value {
    json!({"enabled": true, "frames": 3, "keyboard": false})
}

pub fn build(v: &Value) -> Box<dyn Effect> { Box::new(AttackFlash { frames: pf64(v, "frames", 3.0), keyboard: v.get("keyboard").and_then(|x| x.as_bool()).unwrap_or(false), until: std::collections::HashMap::new() }) }
