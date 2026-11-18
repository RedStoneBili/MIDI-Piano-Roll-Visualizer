use serde_json::{json, Value};
const FG_PALETTE: [&str; 15] = [
    "\x1b[91m","\x1b[94m","\x1b[92m","\x1b[93m","\x1b[95m","\x1b[96m",
    "\x1b[31m","\x1b[34m","\x1b[32m","\x1b[33m","\x1b[35m","\x1b[36m",
    "\x1b[37m","\x1b[97m","\x1b[90m",
];
const BG_PALETTE: [&str; 15] = [
    "\x1b[101m","\x1b[104m","\x1b[102m","\x1b[103m","\x1b[105m","\x1b[106m",
    "\x1b[41m","\x1b[44m","\x1b[42m","\x1b[43m","\x1b[45m","\x1b[46m",
    "\x1b[47m","\x1b[107m","\x1b[100m",
];

const MIDNIGHT_FG: [&str; 15] = ["\x1b[94m","\x1b[34m","\x1b[96m","\x1b[36m","\x1b[95m","\x1b[35m","\x1b[97m","\x1b[37m","\x1b[90m","\x1b[91m","\x1b[92m","\x1b[93m","\x1b[34m","\x1b[36m","\x1b[35m"];
const MIDNIGHT_BG: [&str; 15] = ["\x1b[44m","\x1b[104m","\x1b[46m","\x1b[106m","\x1b[45m","\x1b[105m","\x1b[107m","\x1b[47m","\x1b[100m","\x1b[41m","\x1b[42m","\x1b[43m","\x1b[44m","\x1b[46m","\x1b[45m"];
const CANDY_FG: [&str; 15] = ["\x1b[95m","\x1b[93m","\x1b[92m","\x1b[96m","\x1b[91m","\x1b[94m","\x1b[97m","\x1b[37m","\x1b[35m","\x1b[33m","\x1b[32m","\x1b[36m","\x1b[31m","\x1b[34m","\x1b[90m"];
const CANDY_BG: [&str; 15] = ["\x1b[105m","\x1b[103m","\x1b[102m","\x1b[106m","\x1b[101m","\x1b[104m","\x1b[107m","\x1b[47m","\x1b[45m","\x1b[43m","\x1b[42m","\x1b[46m","\x1b[41m","\x1b[44m","\x1b[100m"];
const CYBER_FG: [&str; 15] = ["\x1b[96m","\x1b[95m","\x1b[93m","\x1b[92m","\x1b[94m","\x1b[91m","\x1b[97m","\x1b[36m","\x1b[35m","\x1b[33m","\x1b[32m","\x1b[34m","\x1b[31m","\x1b[37m","\x1b[90m"];
const CYBER_BG: [&str; 15] = ["\x1b[46m","\x1b[105m","\x1b[43m","\x1b[42m","\x1b[44m","\x1b[41m","\x1b[47m","\x1b[106m","\x1b[45m","\x1b[103m","\x1b[102m","\x1b[104m","\x1b[101m","\x1b[107m","\x1b[100m"];


pub fn theme_palette(name: &str) -> (&'static [&'static str; 15], &'static [&'static str; 15]) {
    match name {
        "midnight" => (&MIDNIGHT_FG, &MIDNIGHT_BG),
        "candy" => (&CANDY_FG, &CANDY_BG),
        "cyberpunk" => (&CYBER_FG, &CYBER_BG),
        _ => (&FG_PALETTE, &BG_PALETTE),
    }
}

pub fn defaults() -> Value {
    json!({"enabled": true, "name": "neon"})
}
