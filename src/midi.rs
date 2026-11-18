use memmap2::Mmap;
use crate::climax::ClimaxCfg;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::fs::File;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvKind {
    NoteOn,
    NoteOff,
    ProgramChange,
    ControlChange,
    Pitchwheel,
    Aftertouch,
    Polytouch,
    SetTempo,
}

#[derive(Clone, Debug)]
pub struct Event {
    pub abs_ticks: u64,
    pub kind: EvKind,
    pub track: usize,
    pub channel: u8,
    pub note: u8,
    pub velocity: u8,
    pub control: u8,
    pub value: u8,
    pub program: u8,
    pub pitch: i32,
    pub tempo: u32,
}

impl Event {
    fn base(abs_ticks: u64, kind: EvKind, track: usize) -> Self {
        Event {
            abs_ticks,
            kind,
            track,
            channel: 0,
            note: 0,
            velocity: 0,
            control: 0,
            value: 0,
            program: 0,
            pitch: 0,
            tempo: 0,
        }
    }
}

pub struct MidiMeta {
    pub tempo_changes: Vec<(u64, u32)>,
    pub note_count: u64,
    pub pitch_hist: [u64; 12],
    pub programs: Vec<u8>,
    pub channels_used: Vec<u8>,
    pub tracks_used: Vec<usize>,
    pub min_note: u8,
    pub max_note: u8,
    pub chord_count_est: u64,
    pub first_note_abs_ticks: u64,
    pub weighted_hist: [f64; 12],
    pub bass_pc: Option<i32>,
    pub first_pc: Option<i32>,
    pub key_sigs: Vec<(u64, i32, u8)>,
    pub climax: Vec<(f64, f64)>,
}


fn compute_climax(notes: &[(u64, u8, u8)], tempo: &[(u64, u32)], tpb: f64, cfg: &ClimaxCfg) -> Vec<(f64, f64)> {
    if !cfg.enabled || notes.is_empty() || tpb <= 0.0 { return Vec::new(); }
    let mut tm: Vec<(u64, u32)> = Vec::new();
    for &(t, v) in tempo {
        if let Some(last) = tm.last_mut() {
            if last.0 == t { *last = (t, v.max(1)); continue; }
        }
        tm.push((t, v.max(1)));
    }
    if tm.is_empty() { tm.push((0, 500000)); }
    let mut seg: Vec<f64> = vec![0.0];
    for i in 1..tm.len() {
        seg.push(seg[i - 1] + (tm[i].0 - tm[i - 1].0) as f64 * tm[i - 1].1 as f64 / tpb / 1_000_000.0);
    }
    // 二分定位速度段：几百万音符的大文件上，线性扫描会成为瓶颈
    let sec_at = |tick: u64| -> f64 {
        let i = tm.partition_point(|&(t, _)| t <= tick).saturating_sub(1).min(tm.len() - 1);
        seg[i] + (tick.saturating_sub(tm[i].0)) as f64 * tm[i].1.max(1) as f64 / tpb / 1_000_000.0
    };
    let bpm_at_sec = |s: f64| -> f64 {
        let i = seg.partition_point(|&x| x <= s).saturating_sub(1).min(tm.len() - 1);
        60_000_000.0 / tm[i].1.max(1) as f64
    };
    let bucket = cfg.bucket.clamp(0.1, 5.0);
    let mut max_sec = 0.0f64;
    for &(tick, _, _) in notes {
        let s = sec_at(tick);
        if s > max_sec { max_sec = s; }
    }
    let n = ((max_sec / bucket).floor() as usize) + 1;
    if n < 3 { return Vec::new(); }
    let mut count = vec![0u32; n];
    let mut vel = vec![0.0f64; n];
    let mut psum = vec![0.0f64; n];
    let mut pmin = vec![255u8; n];
    let mut pmax = vec![0u8; n];
    let mut chroma = vec![[false; 12]; n];
    for &(tick, v, note) in notes {
        let idx = ((sec_at(tick) / bucket) as usize).min(n - 1);
        count[idx] += 1;
        vel[idx] += v as f64;
        psum[idx] += note as f64;
        if note < pmin[idx] { pmin[idx] = note; }
        if note > pmax[idx] { pmax[idx] = note; }
        chroma[idx][(note % 12) as usize] = true;
    }
    let norm = |v: &[f64]| -> Vec<f64> {
        let mut mn = f64::INFINITY;
        let mut mx = f64::NEG_INFINITY;
        for &x in v {
            if x < mn { mn = x; }
            if x > mx { mx = x; }
        }
        if mx <= mn { return vec![0.0; v.len()]; }
        v.iter().map(|&x| (x - mn) / (mx - mn)).collect()
    };
    let dens: Vec<f64> = count.iter().map(|&c| c as f64).collect();
    let velavg: Vec<f64> = (0..n).map(|i| if count[i] > 0 { vel[i] / count[i] as f64 } else { 0.0 }).collect();
    let pmean: Vec<f64> = (0..n).map(|i| if count[i] > 0 { psum[i] / count[i] as f64 } else { 0.0 }).collect();
    let range: Vec<f64> = (0..n).map(|i| if count[i] > 0 { pmax[i] as f64 - pmin[i] as f64 } else { 0.0 }).collect();
    let harm: Vec<f64> = chroma.iter().map(|c| c.iter().filter(|&&b| b).count() as f64).collect();
    let bpm: Vec<f64> = (0..n).map(|i| bpm_at_sec((i as f64 + 0.5) * bucket)).collect();
    let dn = norm(&dens); let vn = norm(&velavg); let pn = norm(&pmean);
    let rn = norm(&range); let hn = norm(&harm); let bn = norm(&bpm);
    let wsum = cfg.w_density + cfg.w_velocity + cfg.w_pitch + cfg.w_range + cfg.w_harmony + cfg.w_tempo;
    let wsum = if wsum.abs() < 1e-9 { 1.0 } else { wsum };
    let raw: Vec<f64> = (0..n).map(|i| {
        (cfg.w_density * dn[i] + cfg.w_velocity * vn[i] + cfg.w_pitch * pn[i]
            + cfg.w_range * rn[i] + cfg.w_harmony * hn[i] + cfg.w_tempo * bn[i]) / wsum
    }).collect();
    let m = ((cfg.smooth / bucket).round() as usize).max(1);
    let mut prefix = vec![0.0f64; n + 1];
    for i in 0..n { prefix[i + 1] = prefix[i] + raw[i]; }
    let mut smooth = vec![0.0f64; n];
    for (i, slot) in smooth.iter_mut().enumerate() {
        let a = i.saturating_sub(m);
        let b = (i + m + 1).min(n);
        *slot = (prefix[b] - prefix[a]) / (b - a) as f64;
    }
    let mut sorted = smooth.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = sorted[n / 2];
    let mean = smooth.iter().sum::<f64>() / n as f64;
    let var = smooth.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
    let std = var.sqrt();
    let thresh = median + cfg.min_std * std;
    let mut peaks: Vec<(usize, f64)> = Vec::new();
    for i in 1..n {
        let is_peak = if i + 1 < n {
            smooth[i] >= smooth[i - 1] && smooth[i] > smooth[i + 1]
        } else {
            smooth[i] >= smooth[i - 1]
        };
        if is_peak {
            let t = (i as f64 + 0.5) * bucket;
            if t < cfg.exclude_edges || t > max_sec - cfg.exclude_edges { continue; }
            if smooth[i] < thresh { continue; }
            if median > 0.0 && smooth[i] < cfg.min_median_ratio * median { continue; }
            peaks.push((i, smooth[i]));
        }
    }
    peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut picked: Vec<(f64, f64)> = Vec::new();
    for (i, sc) in peaks {
        let t = (i as f64 + 0.5) * bucket;
        if picked.iter().any(|&(pt, _)| (pt - t).abs() < cfg.min_gap) { continue; }
        picked.push((t, sc));
        if picked.len() >= cfg.top_n.max(1) { break; }
    }
    picked.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    picked
}

pub struct FastMidiReader {
    mmap: Mmap,
    pub track_ranges: Vec<(usize, usize)>,
    pub ticks_per_beat: u16,
}

impl FastMidiReader {
    pub fn open(path: &str) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| format!("打开文件失败: {}", e))?;
        let mmap = unsafe { Mmap::map(&file) }.map_err(|e| format!("映射文件失败: {}", e))?;
        let mut r = FastMidiReader {
            mmap,
            track_ranges: Vec::new(),
            ticks_per_beat: 480,
        };
        r.parse_header()?;
        Ok(r)
    }

    fn view(&self) -> &[u8] {
        &self.mmap
    }

    fn parse_header(&mut self) -> Result<(), String> {
        let v = &self.mmap;
        let size = v.len();
        if size < 14 || &v[0..4] != b"MThd" {
            return Err("不是 MIDI 文件（缺少 MThd）".to_string());
        }
        let header_len = u32::from_be_bytes([v[4], v[5], v[6], v[7]]) as usize;
        let n_tracks = u16::from_be_bytes([v[10], v[11]]) as usize;
        let tpb = u16::from_be_bytes([v[12], v[13]]);
        self.ticks_per_beat = if tpb == 0 { 480 } else { tpb };

        let mut pos = 8 + header_len;
        let mut ranges = Vec::with_capacity(n_tracks);
        while pos + 8 <= size {
            if &v[pos..pos + 4] != b"MTrk" {
                match find_sub(&v[pos..], b"MTrk") {
                    Some(nxt) => {
                        pos += nxt;
                        continue;
                    }
                    None => break,
                }
            }
            let trk_len = u32::from_be_bytes([v[pos + 4], v[pos + 5], v[pos + 6], v[pos + 7]]) as usize;
            let start = pos + 8;
            let end = (start + trk_len).min(size);
            ranges.push((start, end));
            pos = end;
        }
        self.track_ranges = ranges;
        Ok(())
    }

    pub fn scan_meta(&self, mut progress: impl FnMut(usize, usize), climax_cfg: &ClimaxCfg) -> MidiMeta {
        let v = self.view();
        let n = self.track_ranges.len();
        let mut tempo_changes: Vec<(u64, u32)> = Vec::new();
        let mut programs: HashSet<u8> = HashSet::new();
        let mut channels_used: HashSet<u8> = HashSet::new();
        let mut tracks_used: HashSet<usize> = HashSet::new();
        let mut pitch_hist = [0u64; 12];
        let mut note_count: u64 = 0;
        let mut min_note_seen: i32 = 128;
        let mut max_note_seen: i32 = -1;
        let mut chord_count_est: u64 = 0;
        let mut first_note_abs_ticks: Option<u64> = None;
        let mut weighted_hist = [0.0f64; 12];
        let mut bass_pitch: i32 = 128;
        let mut first_note_pc: Option<i32> = None;
        let mut key_sigs: Vec<(u64, i32, u8)> = Vec::new();
        let mut note_events: Vec<(u64, u8, u8)> = Vec::new();
        let tpb = self.ticks_per_beat as f64;

        for (ti, &(s, e)) in self.track_ranges.iter().enumerate() {
            let mut pos = s;
            let mut abs_ticks: u64 = 0;
            let mut running_status: u8 = 0;
            let mut last_chord_abs: Option<u64> = None;
            let mut track_chord_notes: HashSet<u8> = HashSet::new();
            let mut track_has_notes = false;
            let mut stacks: HashMap<(u8, u8), VecDeque<(u64, u8)>> = HashMap::new();

            while pos < e {
                let mut delta: u64 = 0;
                while pos < e {
                    let b = v[pos];
                    pos += 1;
                    delta = (delta << 7) | ((b & 0x7F) as u64);
                    if b & 0x80 == 0 {
                        break;
                    }
                }
                abs_ticks += delta;
                if pos >= e {
                    break;
                }

                let b = v[pos];
                let status;
                if b < 0x80 {
                    status = running_status;
                } else {
                    status = b;
                    pos += 1;
                    if status < 0xF0 {
                        running_status = status;
                    }
                }
                let hi = status & 0xF0;

                if status == 0xFF {
                    if pos >= e {
                        break;
                    }
                    let meta_type = v[pos];
                    pos += 1;
                    let mut length: usize = 0;
                    while pos < e {
                        let b2 = v[pos];
                        pos += 1;
                        length = (length << 7) | ((b2 & 0x7F) as usize);
                        if b2 & 0x80 == 0 {
                            break;
                        }
                    }
                    if meta_type == 0x51 && length == 3 && pos + 3 <= e {
                        let tempo = ((v[pos] as u32) << 16) | ((v[pos + 1] as u32) << 8) | (v[pos + 2] as u32);
                        tempo_changes.push((abs_ticks, tempo.max(1)));
                    } else if meta_type == 0x59 && length == 2 && pos + 2 <= e {
                        let raw = v[pos] as i32;
                        let sf = if raw >= 128 { raw - 256 } else { raw };
                        key_sigs.push((abs_ticks, sf, v[pos + 1]));
                    } else if meta_type == 0x2F {
                        break;
                    }
                    pos += length;
                    continue;
                }

                if status == 0xF0 || status == 0xF7 {
                    let mut length: usize = 0;
                    while pos < e {
                        let b2 = v[pos];
                        pos += 1;
                        length = (length << 7) | ((b2 & 0x7F) as usize);
                        if b2 & 0x80 == 0 {
                            break;
                        }
                    }
                    pos += length;
                    continue;
                }

                if hi == 0x90 && pos + 1 < e {
                    let note = v[pos] & 0x7F;
                    let velocity = v[pos + 1] & 0x7F;
                    pos += 2;
                    let ch = status & 0x0F;
                    if velocity > 0 {
                        track_has_notes = true;
                        channels_used.insert(ch);
                        note_count += 1;
                        note_events.push((abs_ticks, velocity, note));
                        if (note as i32) < min_note_seen {
                            min_note_seen = note as i32;
                        }
                        if (note as i32) > max_note_seen {
                            max_note_seen = note as i32;
                        }
                        match first_note_abs_ticks {
                            None => first_note_abs_ticks = Some(abs_ticks),
                            Some(f) if abs_ticks < f => first_note_abs_ticks = Some(abs_ticks),
                            _ => {}
                        }
                        let new_chord = match last_chord_abs {
                            None => true,
                            Some(l) => abs_ticks.saturating_sub(l) > 0,
                        };
                        if new_chord {
                            if !track_chord_notes.is_empty() {
                                chord_count_est += 1;
                            }
                            track_chord_notes.clear();
                            last_chord_abs = Some(abs_ticks);
                        }
                        track_chord_notes.insert(note);
                        if ch != 9 {
                            pitch_hist[(note % 12) as usize] += 1;
                            stacks.entry((ch, note)).or_default().push_back((abs_ticks, velocity));
                            if first_note_pc.is_none() {
                                first_note_pc = Some((note % 12) as i32);
                            }
                            if (note as i32) < bass_pitch {
                                bass_pitch = note as i32;
                            }
                        }
                    } else if let Some(st) = stacks.get_mut(&(ch, note)) {
                        if let Some((on_tick, vel)) = st.pop_front() {
                            let dur = abs_ticks.saturating_sub(on_tick).max(1) as f64;
                            let weight = (dur / tpb) * (0.5 + 0.5 * vel as f64 / 127.0);
                            weighted_hist[(note % 12) as usize] += weight;
                        }
                    }
                } else if hi == 0x80 && pos + 1 < e {
                    let note = v[pos] & 0x7F;
                    let ch = status & 0x0F;
                    pos += 2;
                    if let Some(st) = stacks.get_mut(&(ch, note)) {
                        if let Some((on_tick, vel)) = st.pop_front() {
                            let dur = abs_ticks.saturating_sub(on_tick).max(1) as f64;
                            let weight = (dur / tpb) * (0.5 + 0.5 * vel as f64 / 127.0);
                            weighted_hist[(note % 12) as usize] += weight;
                        }
                    }
                } else if hi == 0xA0 || hi == 0xB0 || hi == 0xE0 {
                    pos += 2;
                } else if hi == 0xC0 {
                    if pos < e {
                        programs.insert(v[pos] & 0x7F);
                    }
                    pos += 1;
                } else if hi == 0xD0 {
                    pos += 1;
                } else {
                    break;
                }
            }

            if track_has_notes {
                tracks_used.insert(ti);
            }
            if !track_chord_notes.is_empty() {
                chord_count_est += 1;
            }
            progress(ti + 1, n);
        }

        tempo_changes.sort_by_key(|x| x.0);
        let climax = compute_climax(&note_events, &tempo_changes, tpb, climax_cfg);

        key_sigs.sort_by_key(|x| x.0);
        let mut deduped: Vec<(u64, i32, u8)> = Vec::new();
        for k in key_sigs.drain(..) {
            if let Some(last) = deduped.last_mut() {
                if last.0 == k.0 {
                    *last = k;
                    continue;
                }
            }
            deduped.push(k);
        }
        let bass_pc = if bass_pitch < 128 { Some(bass_pitch % 12) } else { None };

        let mut programs_vec: Vec<u8> = programs.into_iter().collect();
        programs_vec.sort_unstable();
        let mut channels_vec: Vec<u8> = channels_used.into_iter().collect();
        channels_vec.sort_unstable();
        let mut tracks_vec: Vec<usize> = tracks_used.into_iter().collect();
        tracks_vec.sort_unstable();

        MidiMeta {
            tempo_changes,
            note_count,
            pitch_hist,
            programs: programs_vec,
            channels_used: channels_vec,
            tracks_used: tracks_vec,
            min_note: if max_note_seen >= 0 { min_note_seen as u8 } else { 48 },
            max_note: if max_note_seen >= 0 { max_note_seen as u8 } else { 84 },
            chord_count_est,
            first_note_abs_ticks: first_note_abs_ticks.unwrap_or(0),
            weighted_hist,
            bass_pc,
            first_pc: first_note_pc,
            key_sigs: deduped,
            climax,
        }
    }

    pub fn iter_events(&self) -> EventIter<'_> {
        let mut cursors: Vec<TrackCursor<'_>> = self
            .track_ranges
            .iter()
            .enumerate()
            .map(|(i, &(s, e))| TrackCursor::new(self.view(), s, e, i))
            .collect();
        for c in cursors.iter_mut() {
            c.advance();
        }
        let mut heap: BinaryHeap<Reverse<(u64, usize)>> = BinaryHeap::new();
        for (i, c) in cursors.iter().enumerate() {
            if let Some(ev) = &c.current {
                heap.push(Reverse((ev.abs_ticks, i)));
            }
        }
        EventIter { cursors, heap }
    }
}

pub struct EventIter<'a> {
    cursors: Vec<TrackCursor<'a>>,
    heap: BinaryHeap<Reverse<(u64, usize)>>,
}

impl<'a> Iterator for EventIter<'a> {
    type Item = Event;
    fn next(&mut self) -> Option<Event> {
        let Reverse((_, i)) = self.heap.pop()?;
        let ev = self.cursors[i].current.take().unwrap();
        self.cursors[i].advance();
        if let Some(cur) = &self.cursors[i].current {
            self.heap.push(Reverse((cur.abs_ticks, i)));
        }
        Some(ev)
    }
}

struct TrackCursor<'a> {
    view: &'a [u8],
    pos: usize,
    end: usize,
    abs_ticks: u64,
    running_status: u8,
    current: Option<Event>,
    track: usize,
}

impl<'a> TrackCursor<'a> {
    fn new(view: &'a [u8], start: usize, end: usize, track: usize) -> Self {
        TrackCursor {
            view,
            pos: start,
            end,
            abs_ticks: 0,
            running_status: 0,
            current: None,
            track,
        }
    }

    fn advance(&mut self) {
        self.current = None;
        let v = self.view;
        let end = self.end;
        while self.pos < end {
            let mut delta: u64 = 0;
            while self.pos < end {
                let b = v[self.pos];
                self.pos += 1;
                delta = (delta << 7) | ((b & 0x7F) as u64);
                if b & 0x80 == 0 {
                    break;
                }
            }
            self.abs_ticks += delta;
            if self.pos >= end {
                return;
            }

            let b = v[self.pos];
            let status;
            if b < 0x80 {
                status = self.running_status;
            } else {
                status = b;
                self.pos += 1;
                if status < 0xF0 {
                    self.running_status = status;
                }
            }
            let hi = status & 0xF0;

            if status == 0xFF {
                if self.pos >= end {
                    return;
                }
                let meta_type = v[self.pos];
                self.pos += 1;
                let mut length: usize = 0;
                while self.pos < end {
                    let b2 = v[self.pos];
                    self.pos += 1;
                    length = (length << 7) | ((b2 & 0x7F) as usize);
                    if b2 & 0x80 == 0 {
                        break;
                    }
                }
                if meta_type == 0x2F {
                    self.pos = end;
                    return;
                }
                if meta_type == 0x51 && length == 3 && self.pos + 3 <= end {
                    let tempo = ((v[self.pos] as u32) << 16)
                        | ((v[self.pos + 1] as u32) << 8)
                        | (v[self.pos + 2] as u32);
                    self.pos += 3;
                    let mut ev = Event::base(self.abs_ticks, EvKind::SetTempo, self.track);
                    ev.tempo = tempo.max(1);
                    self.current = Some(ev);
                    return;
                }
                self.pos += length;
                continue;
            }

            if status == 0xF0 || status == 0xF7 {
                let mut length: usize = 0;
                while self.pos < end {
                    let b2 = v[self.pos];
                    self.pos += 1;
                    length = (length << 7) | ((b2 & 0x7F) as usize);
                    if b2 & 0x80 == 0 {
                        break;
                    }
                }
                self.pos += length;
                continue;
            }

            let ch = status & 0x0F;
            if hi == 0x90 && self.pos + 1 < end {
                let note = v[self.pos] & 0x7F;
                let vel = v[self.pos + 1] & 0x7F;
                self.pos += 2;
                let mut ev = Event::base(
                    self.abs_ticks,
                    if vel > 0 { EvKind::NoteOn } else { EvKind::NoteOff },
                    self.track,
                );
                ev.channel = ch;
                ev.note = note;
                ev.velocity = vel;
                self.current = Some(ev);
                return;
            }
            if hi == 0x80 && self.pos + 1 < end {
                let note = v[self.pos] & 0x7F;
                let vel = v[self.pos + 1] & 0x7F;
                self.pos += 2;
                let mut ev = Event::base(self.abs_ticks, EvKind::NoteOff, self.track);
                ev.channel = ch;
                ev.note = note;
                ev.velocity = vel;
                self.current = Some(ev);
                return;
            }
            if hi == 0xB0 && self.pos + 1 < end {
                let cc = v[self.pos] & 0x7F;
                let val = v[self.pos + 1] & 0x7F;
                self.pos += 2;
                let mut ev = Event::base(self.abs_ticks, EvKind::ControlChange, self.track);
                ev.channel = ch;
                ev.control = cc;
                ev.value = val;
                self.current = Some(ev);
                return;
            }
            if hi == 0xC0 && self.pos < end {
                let prog = v[self.pos] & 0x7F;
                self.pos += 1;
                let mut ev = Event::base(self.abs_ticks, EvKind::ProgramChange, self.track);
                ev.channel = ch;
                ev.program = prog;
                self.current = Some(ev);
                return;
            }
            if hi == 0xE0 && self.pos + 1 < end {
                let lo = (v[self.pos] & 0x7F) as i32;
                let hb = (v[self.pos + 1] & 0x7F) as i32;
                self.pos += 2;
                let pitch = ((hb << 7) | lo) - 0x2000;
                let mut ev = Event::base(self.abs_ticks, EvKind::Pitchwheel, self.track);
                ev.channel = ch;
                ev.pitch = pitch;
                self.current = Some(ev);
                return;
            }
            if hi == 0xA0 || hi == 0xD0 {
                self.pos += if hi == 0xA0 { 2 } else { 1 };
                continue;
            }
            self.pos = end;
            return;
        }
    }
}

fn find_sub(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vlq(mut v: u32, out: &mut Vec<u8>) {
        let mut buf = Vec::new();
        loop {
            buf.push((v & 0x7F) as u8);
            v >>= 7;
            if v == 0 { break; }
        }
        buf.reverse();
        let n = buf.len();
        for (i, b) in buf.into_iter().enumerate() {
            out.push(if i + 1 < n { b | 0x80 } else { b });
        }
    }

    fn track(events: &[(u32, &[u8])]) -> Vec<u8> {
        let mut data = Vec::new();
        for &(dt, bytes) in events {
            vlq(dt, &mut data);
            data.extend_from_slice(bytes);
        }
        let mut out = b"MTrk".to_vec();
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(&data);
        out
    }

    fn midi_file(tempo: [u8; 3]) -> Vec<u8> {
        let mut out = b"MThd".to_vec();
        out.extend_from_slice(&6u32.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&2u16.to_be_bytes());
        out.extend_from_slice(&480u16.to_be_bytes());
        out.extend_from_slice(&track(&[
            (0, &[0xFF, 0x51, 0x03, tempo[0], tempo[1], tempo[2]]),
            (0, &[0xFF, 0x59, 0x02, 0xFE, 0x00]),
            (0, &[0xFF, 0x2F, 0x00]),
        ]));
        out.extend_from_slice(&track(&[
            (0, &[0x90, 0x3C, 0x64]),
            (480, &[0x80, 0x3C, 0x40]),
            (0, &[0x90, 0x40, 0x64]),
            (480, &[0x80, 0x40, 0x40]),
            (0, &[0xFF, 0x2F, 0x00]),
        ]));
        out
    }

    fn write_tmp(name: &str, bytes: &[u8]) -> String {
        let p = std::env::temp_dir().join(name);
        std::fs::write(&p, bytes).unwrap();
        p.to_string_lossy().to_string()
    }

    fn quiet() -> ClimaxCfg {
        ClimaxCfg { enabled: false, ..Default::default() }
    }

    #[test]
    fn parses_notes_tempo_and_key_signature() {
        let path = write_tmp("dsh_midi_ok.mid", &midi_file([0x07, 0xA1, 0x20]));
        let r = FastMidiReader::open(&path).expect("应能打开");
        assert_eq!(r.ticks_per_beat, 480);
        assert_eq!(r.track_ranges.len(), 2);

        let meta = r.scan_meta(|_, _| {}, &quiet());
        assert_eq!(meta.note_count, 2);
        assert_eq!(meta.min_note, 60);
        assert_eq!(meta.max_note, 64);
        assert_eq!(meta.tempo_changes, vec![(0u64, 500_000u32)]);
        assert_eq!(meta.key_sigs, vec![(0u64, -2i32, 0u8)]);
        assert_eq!(meta.first_note_abs_ticks, 0);
        assert_eq!(meta.channels_used, vec![0u8]);

        let evs: Vec<Event> = r.iter_events().collect();
        assert_eq!(evs.iter().filter(|e| e.kind == EvKind::NoteOn).count(), 2);
        assert_eq!(evs.iter().filter(|e| e.kind == EvKind::NoteOff).count(), 2);
        assert_eq!(evs.iter().filter(|e| e.kind == EvKind::SetTempo).count(), 1);
        let offs: Vec<u64> = evs.iter().filter(|e| e.kind == EvKind::NoteOff).map(|e| e.abs_ticks).collect();
        assert_eq!(offs, vec![480, 960]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_non_midi_input() {
        let path = write_tmp("dsh_midi_bad.mid", b"this is definitely not a midi file");
        assert!(FastMidiReader::open(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn zero_tempo_is_clamped() {
        let path = write_tmp("dsh_midi_tempo0.mid", &midi_file([0x00, 0x00, 0x00]));
        let r = FastMidiReader::open(&path).expect("应能打开");
        let meta = r.scan_meta(|_, _| {}, &quiet());
        assert_eq!(meta.tempo_changes[0].1, 1, "0 速度要抬到 1，避免除零");
        let ticks: Vec<u32> = r.iter_events().filter(|e| e.kind == EvKind::SetTempo).map(|e| e.tempo).collect();
        assert_eq!(ticks, vec![1]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn truncated_track_does_not_hang() {
        let mut bytes = midi_file([0x07, 0xA1, 0x20]);
        bytes.truncate(bytes.len() - 6); // 砍掉结尾，制造半截事件
        let path = write_tmp("dsh_midi_trunc.mid", &bytes);
        let r = FastMidiReader::open(&path).expect("应能打开");
        let _ = r.scan_meta(|_, _| {}, &quiet());
        let _ = r.iter_events().count();
        let _ = std::fs::remove_file(&path);
    }
}
