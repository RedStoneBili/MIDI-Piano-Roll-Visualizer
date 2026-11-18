use serde_json::{json, Value};

#[derive(Clone)]
pub struct ClimaxCfg {
    pub enabled: bool,
    pub bucket: f64,
    pub smooth: f64,
    pub top_n: usize,
    pub min_gap: f64,
    pub exclude_edges: f64,
    pub min_std: f64,
    pub min_median_ratio: f64,
    pub w_density: f64,
    pub w_velocity: f64,
    pub w_pitch: f64,
    pub w_range: f64,
    pub w_harmony: f64,
    pub w_tempo: f64,
}

impl Default for ClimaxCfg {
    fn default() -> Self {
        ClimaxCfg {
            enabled: true,
            bucket: 0.5,
            smooth: 2.0,
            top_n: 3,
            min_gap: 8.0,
            exclude_edges: 3.0,
            min_std: 1.0,
            min_median_ratio: 1.3,
            w_density: 0.35,
            w_velocity: 0.25,
            w_pitch: 0.15,
            w_range: 0.10,
            w_harmony: 0.10,
            w_tempo: 0.05,
        }
    }
}

pub fn defaults() -> Value {
    json!({
        "enabled": true,
        "bucket": 0.5,
        "smooth": 2.0,
        "top_n": 3,
        "min_gap": 8.0,
        "exclude_edges": 3.0,
        "min_std": 1.0,
        "min_median_ratio": 1.3,
        "weights": {
            "density": 0.35,
            "velocity": 0.25,
            "pitch": 0.15,
            "range": 0.10,
            "harmony": 0.10,
            "tempo": 0.05
        }
    })
}

pub fn from_value(v: &Value) -> ClimaxCfg {
    let mut c = ClimaxCfg::default();
    let obj = match v.as_object() { Some(o) => o, None => return c };
    c.enabled = obj.get("enabled").and_then(|x| x.as_bool()).unwrap_or(c.enabled);
    c.bucket = obj.get("bucket").and_then(|x| x.as_f64()).unwrap_or(c.bucket);
    c.smooth = obj.get("smooth").and_then(|x| x.as_f64()).unwrap_or(c.smooth);
    c.top_n = obj.get("top_n").and_then(|x| x.as_u64()).unwrap_or(c.top_n as u64) as usize;
    c.min_gap = obj.get("min_gap").and_then(|x| x.as_f64()).unwrap_or(c.min_gap);
    c.exclude_edges = obj.get("exclude_edges").and_then(|x| x.as_f64()).unwrap_or(c.exclude_edges);
    c.min_std = obj.get("min_std").and_then(|x| x.as_f64()).unwrap_or(c.min_std);
    c.min_median_ratio = obj.get("min_median_ratio").and_then(|x| x.as_f64()).unwrap_or(c.min_median_ratio);
    if let Some(w) = obj.get("weights").and_then(|x| x.as_object()) {
        c.w_density = w.get("density").and_then(|x| x.as_f64()).unwrap_or(c.w_density);
        c.w_velocity = w.get("velocity").and_then(|x| x.as_f64()).unwrap_or(c.w_velocity);
        c.w_pitch = w.get("pitch").and_then(|x| x.as_f64()).unwrap_or(c.w_pitch);
        c.w_range = w.get("range").and_then(|x| x.as_f64()).unwrap_or(c.w_range);
        c.w_harmony = w.get("harmony").and_then(|x| x.as_f64()).unwrap_or(c.w_harmony);
        c.w_tempo = w.get("tempo").and_then(|x| x.as_f64()).unwrap_or(c.w_tempo);
    }
    c
}

pub fn to_value(c: &ClimaxCfg) -> Value {
    json!({
        "enabled": c.enabled,
        "bucket": c.bucket,
        "smooth": c.smooth,
        "top_n": c.top_n,
        "min_gap": c.min_gap,
        "exclude_edges": c.exclude_edges,
        "min_std": c.min_std,
        "min_median_ratio": c.min_median_ratio,
        "weights": {
            "density": c.w_density,
            "velocity": c.w_velocity,
            "pitch": c.w_pitch,
            "range": c.w_range,
            "harmony": c.w_harmony,
            "tempo": c.w_tempo
        }
    })
}
