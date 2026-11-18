#![allow(unused_imports)]
use crate::Cell;
use crate::effects::{pbool, pf64, Effect, FrameCtx};
use serde_json::{json, Value};
const ESC: &str = "\x1b";

pub struct ClimaxShake {
    duration: f64,
    intensity: f64,
    flash: bool,
    offset: (i32, i32),
    active: bool,
    seed: u64,
}

impl Effect for ClimaxShake {
    fn begin_frame(&mut self, ctx: &FrameCtx) {
        self.offset = (0, 0);
        self.active = false;
        let mut best = 0.0f64;
        for &(t, s) in &ctx.climax {
            let age = ctx.now - t;
            if age >= 0.0 && age < self.duration {
                let k = 1.0 - age / self.duration;
                best = best.max(k * s.clamp(0.0, 1.5));
            }
        }
        if best > 0.02 {
            self.active = true;
            self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let r1 = ((self.seed >> 33) & 0xffff) as f64 / 65535.0;
            let r2 = ((self.seed >> 17) & 0xffff) as f64 / 65535.0;
            let amp = self.intensity * best.clamp(0.0, 1.0);
            let dx = ((r1 * 2.0 - 1.0) * amp).round() as i32;
            let mut dy = ((r2 * 2.0 - 1.0) * amp).round() as i32;
            if dx == 0 && dy == 0 {
                dy = if (self.seed >> 40) & 1 == 0 { 1 } else { -1 };
            }
            self.offset = (dx, dy);
        }
    }
    fn screen_offset(&mut self, _ctx: &FrameCtx) -> (i32, i32) { self.offset }
    fn force_full_redraw(&self) -> bool { self.active }
    fn style_head(&mut self, cell: &mut Cell, _n: u8, _v: u8, _c: u8, _ctx: &FrameCtx) {
        if self.flash && self.active { cell.0 = format!("{}[1m", ESC); }
    }
    fn style_key(&mut self, cell: &mut Cell, _n: u8, _a: bool, _ctx: &FrameCtx) {
        if self.flash && self.active { cell.0 = format!("{}[1m", ESC); }
    }
}

pub fn defaults() -> Value {
    json!({"enabled": true, "duration": 0.5, "intensity": 2.0, "flash": true})
}

pub fn build(v: &Value) -> Box<dyn Effect> {
    Box::new(ClimaxShake {
        duration: pf64(v, "duration", 0.5).clamp(0.1, 3.0),
        intensity: pf64(v, "intensity", 2.0).clamp(0.0, 4.0),
        flash: pbool(v, "flash", true),
        offset: (0, 0),
        active: false,
        seed: 0x2545F4914F6CDD1D,
    })
}
