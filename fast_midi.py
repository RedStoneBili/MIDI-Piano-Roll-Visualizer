import mmap
import struct
import heapq
from typing import Dict, List, Iterator, Optional


class FastMidiReader:
    """按轨流式读取 MIDI 事件。"""

    def __init__(self, path: str):
        self._fd = open(path, 'rb')
        self._mm = mmap.mmap(self._fd.fileno(), 0, access=mmap.ACCESS_READ)
        self._view = memoryview(self._mm)
        self._size = len(self._mm)
        self._parse_header()

    def _parse_header(self):
        view = self._view
        if self._size < 14 or bytes(view[0:4]) != b'MThd':
            raise ValueError("Not a MIDI file (missing MThd)")
        header_len = struct.unpack_from('>I', view, 4)[0]
        self.format = struct.unpack_from('>H', view, 8)[0]
        self.n_tracks = struct.unpack_from('>H', view, 10)[0]
        self.ticks_per_beat = struct.unpack_from('>H', view, 12)[0]
        if self.ticks_per_beat == 0:
            self.ticks_per_beat = 480

        self.track_ranges: List[tuple] = []
        pos = 8 + header_len
        while pos + 8 <= self._size:
            if bytes(view[pos:pos + 4]) != b'MTrk':
                nxt = bytes(view[pos:]).find(b'MTrk')
                if nxt < 0:
                    break
                pos += nxt
                continue
            trk_len = struct.unpack_from('>I', view, pos + 4)[0]
            start = pos + 8
            end = min(start + trk_len, self._size)
            self.track_ranges.append((start, end))
            pos = end

    def close(self):
        try:
            self._view.release()
        except Exception:
            pass
        try:
            self._mm.close()
        except Exception:
            pass
        try:
            self._fd.close()
        except Exception:
            pass

    # 第一遍：统计
    def scan_meta(self, progress_cb=None) -> Dict:
        """扫描所有轨道，收集 tempo、音符、音色等统计。"""
        view = self._view
        n = len(self.track_ranges)
        tempo_changes = []
        programs = set()
        channels_used = set()
        tracks_used = set()
        pitch_hist = [0] * 12
        note_count = 0
        min_note_seen = 128
        max_note_seen = -1
        chord_count_est = 0
        first_note_abs_ticks = None

        for ti, (s, e) in enumerate(self.track_ranges):
            pos = s
            abs_ticks = 0
            running_status = 0
            last_chord_abs = None
            track_chord_notes = set()
            track_has_notes = False

            while pos < e:
                # VLQ delta-time
                delta = 0
                while pos < e:
                    b = view[pos]; pos += 1
                    delta = (delta << 7) | (b & 0x7F)
                    if not (b & 0x80):
                        break
                abs_ticks += delta
                if pos >= e:
                    break

                b = view[pos]
                if b < 0x80:
                    status = running_status
                else:
                    status = b; pos += 1
                    if status < 0xF0:
                        running_status = status

                hi = status & 0xF0

                # meta
                if status == 0xFF:
                    if pos >= e: break
                    meta_type = view[pos]; pos += 1
                    length = 0
                    while pos < e:
                        b2 = view[pos]; pos += 1
                        length = (length << 7) | (b2 & 0x7F)
                        if not (b2 & 0x80):
                            break
                    if meta_type == 0x51 and length == 3 and pos + 3 <= e:
                        tempo = (view[pos] << 16) | (view[pos + 1] << 8) | view[pos + 2]
                        tempo_changes.append((abs_ticks, tempo))
                    elif meta_type == 0x2F:
                        break
                    pos += length
                    continue

                # sysex
                if status == 0xF0 or status == 0xF7:
                    length = 0
                    while pos < e:
                        b2 = view[pos]; pos += 1
                        length = (length << 7) | (b2 & 0x7F)
                        if not (b2 & 0x80):
                            break
                    pos += length
                    continue

                # channel 消息
                if hi == 0x90 and pos + 1 < e:
                    note = view[pos] & 0x7F
                    velocity = view[pos + 1] & 0x7F
                    pos += 2
                    if velocity > 0:
                        track_has_notes = True
                        channels_used.add(status & 0x0F)
                        note_count += 1
                        pitch_hist[note % 12] += 1
                        if note < min_note_seen: min_note_seen = note
                        if note > max_note_seen: max_note_seen = note
                        if first_note_abs_ticks is None or abs_ticks < first_note_abs_ticks:
                            first_note_abs_ticks = abs_ticks
                        if last_chord_abs is None or abs_ticks - last_chord_abs > 0.5:
                            if track_chord_notes:
                                chord_count_est += 1
                            track_chord_notes = set()
                            last_chord_abs = abs_ticks
                        track_chord_notes.add(note)
                elif hi in (0x80, 0xA0, 0xB0, 0xE0):
                    pos += 2
                elif hi == 0xC0:
                    if pos < e:
                        programs.add(view[pos] & 0x7F)
                    pos += 1
                elif hi == 0xD0:
                    pos += 1
                else:
                    break

            if track_has_notes:
                tracks_used.add(ti)
            if track_chord_notes:
                chord_count_est += 1
            if progress_cb:
                progress_cb(ti + 1, n)

        tempo_changes.sort()
        return {
            'tempo_changes': tempo_changes,
            'note_count': note_count,
            'pitch_hist': pitch_hist,
            'programs': sorted(programs),
            'channels_used': sorted(channels_used),
            'tracks_used': sorted(tracks_used),
            'min_note': min_note_seen if max_note_seen >= 0 else 48,
            'max_note': max_note_seen if max_note_seen >= 0 else 84,
            'chord_count_est': chord_count_est,
            'first_note_abs_ticks': first_note_abs_ticks or 0,
        }

    # 第二遍：流式产出
    def iter_events(self) -> Iterator[Dict]:
        """按 tick 归并各轨道，流式产出事件。"""
        cursors = [_TrackCursor(self._view, s, e, i)
                   for i, (s, e) in enumerate(self.track_ranges)]
        for c in cursors:
            c.advance()

        heap = []
        for i, c in enumerate(cursors):
            if c.current is not None:
                heapq.heappush(heap, (c.abs_ticks, i))

        while heap:
            _, i = heapq.heappop(heap)
            c = cursors[i]
            ev = c.current
            c.advance()
            if c.current is not None:
                heapq.heappush(heap, (c.abs_ticks, i))
            yield ev


class _TrackCursor:
    __slots__ = ('view', 'pos', 'end', 'abs_ticks', 'running_status',
                 'current', 'track_id')

    def __init__(self, view, start, end, track_id):
        self.view = view
        self.pos = start
        self.end = end
        self.abs_ticks = 0
        self.running_status = 0
        self.current = None
        self.track_id = track_id

    def advance(self):
        self.current = None
        view = self.view
        end = self.end
        while self.pos < end:
            # VLQ delta-time
            delta = 0
            while self.pos < end:
                b = view[self.pos]; self.pos += 1
                delta = (delta << 7) | (b & 0x7F)
                if not (b & 0x80):
                    break
            self.abs_ticks += delta
            if self.pos >= end:
                return

            b = view[self.pos]
            if b < 0x80:
                status = self.running_status
            else:
                status = b; self.pos += 1
                if status < 0xF0:
                    self.running_status = status
            hi = status & 0xF0

            if status == 0xFF:
                if self.pos >= end: return
                meta_type = view[self.pos]; self.pos += 1
                length = 0
                while self.pos < end:
                    b2 = view[self.pos]; self.pos += 1
                    length = (length << 7) | (b2 & 0x7F)
                    if not (b2 & 0x80):
                        break
                if meta_type == 0x2F:
                    self.pos = end
                    return
                if meta_type == 0x51 and length == 3 and self.pos + 3 <= end:
                    tempo = (view[self.pos] << 16) | (view[self.pos+1] << 8) | view[self.pos+2]
                    self.pos += 3
                    self.current = {
                        'abs_ticks': self.abs_ticks,
                        'type': 'set_tempo',
                        'tempo': tempo,
                        'track': self.track_id,
                    }
                    return
                self.pos += length
                continue

            if status == 0xF0 or status == 0xF7:
                length = 0
                while self.pos < end:
                    b2 = view[self.pos]; self.pos += 1
                    length = (length << 7) | (b2 & 0x7F)
                    if not (b2 & 0x80):
                        break
                self.pos += length
                continue

            ch = status & 0x0F
            if hi == 0x90 and self.pos + 1 < end:
                note = view[self.pos] & 0x7F
                vel = view[self.pos + 1] & 0x7F
                self.pos += 2
                self.current = {
                    'abs_ticks': self.abs_ticks,
                    'type': 'note_on' if vel > 0 else 'note_off',
                    'channel': ch, 'note': note, 'velocity': vel,
                    'track': self.track_id,
                }
                return
            if hi == 0x80 and self.pos + 1 < end:
                note = view[self.pos] & 0x7F
                vel = view[self.pos + 1] & 0x7F
                self.pos += 2
                self.current = {
                    'abs_ticks': self.abs_ticks,
                    'type': 'note_off',
                    'channel': ch, 'note': note, 'velocity': vel,
                    'track': self.track_id,
                }
                return
            if hi == 0xB0 and self.pos + 1 < end:
                cc = view[self.pos] & 0x7F
                val = view[self.pos + 1] & 0x7F
                self.pos += 2
                self.current = {
                    'abs_ticks': self.abs_ticks,
                    'type': 'control_change',
                    'channel': ch, 'control': cc, 'value': val,
                    'track': self.track_id,
                }
                return
            if hi == 0xC0 and self.pos < end:
                prog = view[self.pos] & 0x7F
                self.pos += 1
                self.current = {
                    'abs_ticks': self.abs_ticks,
                    'type': 'program_change',
                    'channel': ch, 'program': prog,
                    'track': self.track_id,
                }
                return
            if hi == 0xE0 and self.pos + 1 < end:
                lo = view[self.pos] & 0x7F
                hb = view[self.pos + 1] & 0x7F
                self.pos += 2
                pitch = ((hb << 7) | lo) - 0x2000
                self.current = {
                    'abs_ticks': self.abs_ticks,
                    'type': 'pitchwheel',
                    'channel': ch, 'pitch': pitch,
                    'track': self.track_id,
                }
                return
            if hi in (0xA0, 0xD0):
                self.pos += 2 if hi == 0xA0 else 1
                continue
            # 未知 status，终止该轨
            self.pos = end
            return