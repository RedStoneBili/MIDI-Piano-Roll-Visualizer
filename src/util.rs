use serde_json::Value;
use std::io::{self, Write};

pub const ESC: &str = "\x1b";

// ---------- JSON 辅助 ----------
pub fn jstr(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(|s| s.to_string())
}
pub fn jf64(v: &Value, k: &str, d: f64) -> f64 {
    v.get(k).and_then(|x| x.as_f64()).unwrap_or(d)
}
pub fn jbool(v: &Value, k: &str, d: bool) -> bool {
    v.get(k).and_then(|x| x.as_bool()).unwrap_or(d)
}
pub fn ji32(v: &Value, k: &str, d: i32) -> i32 {
    v.get(k).and_then(|x| x.as_i64()).map(|n| n as i32).unwrap_or(d)
}
pub fn ju32(v: &Value, k: &str, d: u32) -> u32 {
    v.get(k).and_then(|x| x.as_u64()).map(|n| n as u32).unwrap_or(d)
}

pub fn char_width(c: char) -> usize {
    match unicode_width::UnicodeWidthChar::width(c) {
        Some(w) if w > 0 => w,
        _ => 1,
    }
}

pub fn visible_width(s: &str) -> usize {
    let ch: Vec<char> = s.chars().collect();
    let n = ch.len();
    let mut i = 0usize;
    let mut w = 0usize;
    while i < n {
        if ch[i] == '\x1b' {
            i += 1;
            if i < n && ch[i] == '[' {
                i += 1;
                while i < n {
                    let d = ch[i] as u32;
                    i += 1;
                    if (0x40..=0x7e).contains(&d) { break; }
                }
            }
        } else {
            w += char_width(ch[i]);
            i += 1;
        }
    }
    w
}

pub fn truncate_ansi(s: &str, max_width: usize, suffix: &str) -> String {
    if visible_width(s) <= max_width {
        return s.to_string();
    }
    let suffix_w: usize = suffix.chars().map(char_width).sum();
    let target = max_width.saturating_sub(suffix_w);
    let ch: Vec<char> = s.chars().collect();
    let n = ch.len();
    let mut result = String::new();
    let mut w = 0usize;
    let mut i = 0usize;
    while i < n {
        let c = ch[i];
        if c == '\x1b' {
            result.push(c);
            i += 1;
            if i < n && ch[i] == '[' {
                result.push(ch[i]);
                i += 1;
                while i < n {
                    let d = ch[i];
                    result.push(d);
                    i += 1;
                    if (0x40..=0x7e).contains(&(d as u32)) { break; }
                }
            }
        } else {
            let cw = char_width(c);
            if w + cw > target {
                break;
            }
            result.push(c);
            w += cw;
            i += 1;
        }
    }
    result.push_str(suffix);
    result.push_str(ESC);
    result.push_str("[0m");
    result
}

pub fn write_synced(s: &str, sync: bool) {
    if s.is_empty() {
        return;
    }
    let out = io::stdout();
    let mut lock = out.lock();
    if sync {
        let mut buf = Vec::with_capacity(s.len() + 20);
        buf.extend_from_slice(b"\x1b[?2026h");
        buf.extend_from_slice(s.as_bytes());
        buf.extend_from_slice(b"\x1b[?2026l");
        let _ = lock.write_all(&buf);
    } else {
        let _ = lock.write_all(s.as_bytes());
    }
    let _ = lock.flush();
}

