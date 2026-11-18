#![allow(unused_imports)]
use crate::Cell;
use crate::effects::{pbool, pchar, pf64, pi32, pstr, Effect, FrameCtx};
use serde_json::{json, Value};
const ESC: &str = "\x1b";

struct ModePalette {}
impl ModePalette {
    fn build() -> ModePalette { ModePalette {} }
}
impl Effect for ModePalette {
    fn style_head(&mut self, cell: &mut Cell, note: u8, _v: u8, _ch: u8, ctx: &FrameCtx) {
        let deg = (note as i32 - ctx.key_root).rem_euclid(12);
        let c = (196 - deg * 6).clamp(16, 231);
        cell.0 = if cell.1 == ' ' {
            format!("{}[48;5;{}m", ESC, c)
        } else {
            format!("{}[38;5;{}m", ESC, c)
        };
    }
}

pub fn defaults() -> Value { json!({"enabled": true}) }

pub fn build(_v: &Value) -> Box<dyn Effect> { Box::new(ModePalette::build()) }
