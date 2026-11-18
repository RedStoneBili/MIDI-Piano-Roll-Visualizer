use std::path::{Path, PathBuf};

pub struct Playlist {
    pub items: Vec<PathBuf>,
    mode: String,
    idx: usize,
    order: Vec<usize>,
    seed: u64,
    last_played: Option<usize>,
}

fn is_midi(p: &Path) -> bool {
    match p.extension().and_then(|e| e.to_str()) {
        Some(e) => {
            let e = e.to_lowercase();
            e == "mid" || e == "midi"
        }
        None => false,
    }
}

fn collect_dir(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_dir() {
            if recursive {
                collect_dir(&p, recursive, out);
            }
        } else if is_midi(&p) {
            out.push(p);
        }
    }
}

impl Playlist {
    pub fn collect(inputs: &[String], recursive: bool) -> Playlist {
        let mut items = Vec::new();
        for inp in inputs {
            let s = inp.trim();
            if s.is_empty() {
                continue;
            }
            let p = Path::new(s);
            if p.is_dir() {
                collect_dir(p, recursive, &mut items);
            } else if p.is_file() {
                items.push(p.to_path_buf());
            }
        }
        items.sort();
        items.dedup();
        let n = items.len();
        Playlist {
            items,
            mode: "order".into(),
            idx: 0,
            order: (0..n).collect(),
            last_played: None,
            seed: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(12345),
        }
    }

    pub fn set_mode(&mut self, m: &str) {
        let m = m.to_lowercase();
        if m == "order" || m == "loop" || m == "random" {
            self.mode = m;
            self.idx = 0;
            self.shuffle();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn rnd(&mut self) -> u64 {
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.seed >> 33
    }

    fn shuffle(&mut self) {
        let n = self.items.len();
        self.order = (0..n).collect();
        if n > 1 {
            for i in (1..n).rev() {
                let j = (self.rnd() as usize) % (i + 1);
                self.order.swap(i, j);
            }
        }
    }

    pub fn next(&mut self) -> Option<PathBuf> {
        let n = self.items.len();
        if n == 0 {
            return None;
        }
        match self.mode.as_str() {
            "loop" => {
                let p = self.items[self.idx % n].clone();
                self.idx += 1;
                Some(p)
            }
            "random" => {
                if self.order.is_empty() || self.idx >= self.order.len() {
                    self.shuffle();
                    self.idx = 0;
                    // 新一轮的第一首不要和上一轮最后一首重复
                    if n > 1 {
                        if let Some(last) = self.last_played {
                            if self.order[0] == last {
                                self.order.swap(0, 1);
                            }
                        }
                    }
                }
                let k = self.order[self.idx];
                self.idx += 1;
                self.last_played = Some(k);
                self.items.get(k).cloned()
            }
            _ => {
                if self.idx >= n {
                    return None;
                }
                let p = self.items[self.idx].clone();
                self.idx += 1;
                Some(p)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sandbox(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dsh_playlist_{}_{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn names(p: &PathBuf) -> String {
        p.file_name().unwrap().to_string_lossy().to_string()
    }

    #[test]
    fn collects_only_midi_and_sorts() {
        let d = sandbox("collect");
        for n in ["b.mid", "a.MIDI", "c.txt"] {
            fs::write(d.join(n), b"x").unwrap();
        }
        let pl = Playlist::collect(&[d.to_string_lossy().to_string()], false);
        let got: Vec<String> = pl.items.iter().map(names).collect();
        assert_eq!(got, vec!["a.MIDI", "b.mid"]);
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn recursive_flag_is_respected() {
        let d = sandbox("recursive");
        let sub = d.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(d.join("top.mid"), b"x").unwrap();
        fs::write(sub.join("deep.mid"), b"x").unwrap();
        let flat = Playlist::collect(&[d.to_string_lossy().to_string()], false);
        assert_eq!(flat.items.len(), 1);
        let deep = Playlist::collect(&[d.to_string_lossy().to_string()], true);
        assert_eq!(deep.items.len(), 2);
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn order_stops_after_one_pass() {
        let d = sandbox("order");
        for n in ["1.mid", "2.mid"] {
            fs::write(d.join(n), b"x").unwrap();
        }
        let mut pl = Playlist::collect(&[d.to_string_lossy().to_string()], false);
        assert!(pl.next().is_some());
        assert!(pl.next().is_some());
        assert!(pl.next().is_none());
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn loop_wraps_forever() {
        let d = sandbox("loop");
        fs::write(d.join("1.mid"), b"x").unwrap();
        let mut pl = Playlist::collect(&[d.to_string_lossy().to_string()], false);
        pl.set_mode("loop");
        for _ in 0..3 {
            assert!(pl.next().is_some());
        }
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn random_covers_all_and_avoids_adjacent_repeat() {
        let d = sandbox("random");
        for n in ["1.mid", "2.mid", "3.mid", "4.mid"] {
            fs::write(d.join(n), b"x").unwrap();
        }
        let mut pl = Playlist::collect(&[d.to_string_lossy().to_string()], false);
        pl.set_mode("random");
        let seq: Vec<PathBuf> = (0..40).filter_map(|_| pl.next()).collect();
        assert_eq!(seq.len(), 40);
        // 每 4 首一轮，轮内不重复
        for round in seq.chunks(4) {
            let mut v: Vec<String> = round.iter().map(names).collect();
            v.sort();
            v.dedup();
            assert_eq!(v.len(), 4);
        }
        // 相邻两首不重复（含轮次交界）
        for w in seq.windows(2) {
            assert_ne!(w[0], w[1]);
        }
        fs::remove_dir_all(&d).ok();
    }
}
