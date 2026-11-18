#![allow(unused_imports)]
use crate::Cell;
use crate::effects::{pbool, Effect, FrameCtx};
use serde_json::{json, Value};
const ESC: &str = "\x1b";

struct P { x: f64, y: f64, vx: f64, vy: f64, born: f64, color: i32 }

pub struct Fireworks { parts: Vec<P>, seed: u64, last: f64, min_vel: u8 }

impl Fireworks {
    fn rnd(&mut self) -> f64 {
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.seed >> 33) as f64) / ((1u64 << 31) as f64)
    }
}

impl Effect for Fireworks {
    fn begin_frame(&mut self, ctx: &FrameCtx) {
        let dt = (ctx.now - self.last).clamp(0.0, 0.1);
        self.last = ctx.now;
        for p in &mut self.parts {
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            p.vy += 9.0 * dt;
        }
        self.parts.retain(|p| ctx.now - p.born < 1.8 && p.y < ctx.height as f64 + 2.0);
    }
    fn on_note_on(&mut self, _note: u8, velocity: u8, col: i32, ctx: &FrameCtx) {
        if velocity < self.min_vel { return; }
        for _ in 0..20 {
            let a = self.rnd() * std::f64::consts::TAU;
            let sp = 2.0 + self.rnd() * 6.0;
            let color = 196 + (self.rnd() * 36.0) as i32;
            self.parts.push(P {
                x: col as f64,
                y: ctx.height as f64 - 1.0,
                vx: a.cos() * sp,
                vy: a.sin() * sp - 3.0,
                born: ctx.now,
                color,
            });
        }
        if self.parts.len() > 600 {
            let n = self.parts.len() - 600;
            self.parts.drain(0..n);
        }
    }
    fn paint_bg(&mut self, cell: &mut Cell, row: i32, col: i32, _ctx: &FrameCtx) {
        if !cell.0.is_empty() || cell.1 != ' ' { return; }
        for p in &self.parts {
            if p.x.round() as i32 == col && p.y.round() as i32 == row {
                cell.0 = format!("{}[38;5;{}m", ESC, p.color.clamp(16, 231));
                cell.1 = '✦';
                return;
            }
        }
    }
    fn needs_full_grid(&self) -> bool { true }
}

pub fn defaults() -> Value {
    json!({"enabled": false, "min_velocity": 100})
}

pub fn build(v: &Value) -> Box<dyn Effect> {
    Box::new(Fireworks {
        parts: Vec::new(),
        seed: 0x9E3779B97F4A7C15,
        last: 0.0,
        min_vel: v.get("min_velocity").and_then(|x| x.as_u64()).unwrap_or(100) as u8,
    })
}
