#![allow(unused_imports)]
use crate::Cell;
use crate::effects::{pbool, pchar, pf64, pi32, pstr, Effect, FrameCtx};
use serde_json::{json, Value};
const ESC: &str = "\x1b";

struct PercussionMark { glyph: char, color: String }
impl Effect for PercussionMark {
    fn style_head(&mut self, cell: &mut Cell, _note: u8, _v: u8, channel: u8, _ctx: &FrameCtx) {
        if channel & 0x0F == 9 {
            cell.1 = self.glyph;
            cell.0 = self.color.clone();
        }
    }
}

pub fn defaults() -> Value {
    json!({"enabled": true, "glyph": "◆", "color": format!("{}[93m", ESC)})
}

pub fn build(v: &Value) -> Box<dyn Effect> { Box::new(PercussionMark { glyph: pchar(v, "glyph", '◆'), color: pstr(v, "color", &format!("{}[93m", ESC)) }) }
