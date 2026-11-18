#![allow(unused_imports)]
use crate::Cell;
use crate::effects::{pbool, pchar, pf64, pi32, pstr, Effect, FrameCtx};
use serde_json::{json, Value};
const ESC: &str = "\x1b";

struct ChordDegrees;
impl Effect for ChordDegrees {
    fn hud_extra(&mut self, ctx: &FrameCtx) -> Option<String> {
        if ctx.chord_name.is_empty() { return None; }
        let majors = [[0,2,4,5,7,9,11],[1,3,5,6,8,10,0],[2,4,6,7,9,11,1],[3,5,7,8,10,0,2],
                      [4,6,8,9,11,1,3],[5,7,9,10,0,2,4],[6,8,10,11,1,3,5],[7,9,11,0,2,4,6],
                      [8,10,0,1,3,5,7],[9,11,1,2,4,6,8],[10,0,2,3,5,7,9],[11,1,3,4,6,8,10]];
        let minors = [[0,2,3,5,7,8,10],[1,3,4,6,8,9,11],[2,4,5,7,9,10,0],[3,5,6,8,10,11,1],
                      [4,6,7,9,11,0,2],[5,7,8,10,0,1,3],[6,8,9,11,1,2,4],[7,9,10,0,2,3,5],
                      [8,10,11,1,3,4,6],[9,11,0,2,4,5,7],[10,0,1,3,5,6,8],[11,1,2,4,6,7,9]];
        let scale = if ctx.is_minor { minors[ctx.key_root as usize] } else { majors[ctx.key_root as usize] };
        let deg = (ctx.chord_root - ctx.key_root).rem_euclid(12);
        let numerals = ["I","II","III","IV","V","VI","VII"];
        let mut out = String::new();
        if let Some(i) = scale.iter().position(|&x| x == deg) {
            out.push_str(numerals[i]);
        } else {
            out.push('?');
        }
        if ctx.chord_name.to_lowercase().contains('m') && !ctx.chord_name.starts_with('M') {
            out = out.to_lowercase();
        }
        Some(format!("级数:{}", out))
    }
}

pub fn defaults() -> Value { json!({"enabled": true}) }

pub fn build(_v: &Value) -> Box<dyn Effect> { Box::new(ChordDegrees) }
