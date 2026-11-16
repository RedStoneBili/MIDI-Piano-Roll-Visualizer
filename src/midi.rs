use memmap2::Mmap;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};
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

    pub fn scan_meta(&self, mut progress: impl FnMut(usize, usize)) -> MidiMeta {
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

        for (ti, &(s, e)) in self.track_ranges.iter().enumerate() {
            let mut pos = s;
            let mut abs_ticks: u64 = 0;
            let mut running_status: u8 = 0;
            let mut last_chord_abs: Option<u64> = None;
            let mut track_chord_notes: HashSet<u8> = HashSet::new();
            let mut track_has_notes = false;

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
                        tempo_changes.push((abs_ticks, tempo));
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
                    if velocity > 0 {
                        track_has_notes = true;
                        channels_used.insert(status & 0x0F);
                        note_count += 1;
                        pitch_hist[(note % 12) as usize] += 1;
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
                    }
                } else if hi == 0x80 || hi == 0xA0 || hi == 0xB0 || hi == 0xE0 {
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
                    ev.tempo = tempo;
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
