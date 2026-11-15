# 修复了部分问题 by CHCAT1320
# 添加英文支持 by CHCAT1320
# 修复：轨道独立颜色、屏幕高度自适应、第一音符瞬移、无声、非标准 MIDI
# 修复：下落速度改为由 MIDI tempo (BPM) 决定
# 新增：中央C（无音符按下时）显示白底
# 新增：header 音符数显示 已播放/总数，实时更新
# 新增：按原始时间精确发送 control_change / program_change / pitchwheel /
#       aftertouch / polytouch；note_on / note_off 带原始 channel
# 修复：双 header 残留 → 两行分开精确刷新
# 新增：实时效果信息（每通道 vol/pan/rev/sus/prog）
# 新增：CC 摘要中文化；变速 MIDI 每个音符按自身 tempo 计算下落时间
# 新增：缓解波浪 → 同步输出(DEC 2026) + 行级 diff + 颜色合并 + 定宽渲染
# 性能优化：detect_chord 缓存 / 增量渲染 / 静态行预计算 / 字节级写
# 新增：流式处理 → 生产者子进程 FastMidiReader 流式解析，主进程流式消费
# 修复：卡住不动 / 画面高度 / 进度条 / 分轨颜色丢失
# 修复：stage_msg 未初始化 NameError；Queue.get_nowait Windows 偶发阻塞
# 新增：自适应宽度 + block_style(bg/fg) + 底部键盘整格填满 + 完整名称
# 修复：所有列宽度必须一致 → 统一 base 宽度，右侧留白
import mido
import time
import os
import heapq
import sys
import io
import shutil
import multiprocessing
import queue as queue_mod
import gc

from fast_midi import FastMidiReader

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')

from collections import defaultdict, deque
from typing import List, Dict, Tuple, Optional
import json

CONFIG_PATH = os.path.join(os.path.dirname(__file__), 'config.json')
with open(CONFIG_PATH, 'r', encoding='utf-8') as f:
    CONFIG = json.load(f)

COLOR_RESET = CONFIG.get('color_reset', '\033[0m')
COLOR_CLEAR = CONFIG.get('color_clear', '\033[H')
NOTE_NAMES = CONFIG.get('note_names', ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'])

with open(CONFIG.get('language', 'lang/zh-CN.json'), 'r', encoding='utf-8') as f:
    LANG = json.load(f)

instruments = LANG.get("instruments", [
    "钢琴", "明亮钢琴", "电钢琴", "酒吧钢琴", "罗德钢琴", "合唱钢琴", "羽管键琴", "clavinet",
    "钢片琴", "钟琴", "音乐盒", "颤音琴", "马林巴", "木琴", "管钟", "大扬琴",
    "电风琴", "拉杆风琴", "摇滚风琴", "教堂风琴", "簧风琴", "手风琴", "口琴", "探戈手风琴",
    "尼龙吉他", "钢弦吉他", "爵士吉他", "清音吉他", "闷音吉他", "过载吉他", "失真吉他", "和声吉他",
    "原声贝斯", "指弹贝斯", "拨片贝斯", "无品贝斯", "slap贝斯1", "slap贝斯2", "合成贝斯1", "合成贝斯2",
    "小提琴", "中提琴", "大提琴", "低音提琴", "颤音弦乐", "拨奏弦乐", "竖琴", "定音鼓",
    "弦乐合奏1", "弦乐合奏2", "合成弦乐1", "合成弦乐2", "合唱人声", "人声哼唱", "合成人声", "管弦乐打击乐",
    "小号", "长号", "大号", "弱音小号", "法国号", "铜管组", "合成铜管1", "合成铜管2",
    "高音萨克斯", "中音萨克斯", "次中音萨克斯", "上低音萨克斯", "双簧管", "英国管", "巴松管", "单簧管",
    "短笛", "长笛", "竖笛", "排箫", "吹瓶", "尺八", "口哨", "奥卡里那",
    "合成主音1", "合成主音2", "合成主音3", "合成主音4", "合成主音5", "合成主音6", "合成主音7", "合成主音8",
    "合成背景1", "合成背景2", "合成背景3", "合成背景4", "合成背景5", "合成背景6", "合成背景7", "合成背景8",
    "合成效果1", "合成效果2", "合成效果3", "合成效果4", "合成效果5", "合成效果6", "合成效果7", "合成效果8",
    "民族乐器1", "民族乐器2", "民族乐器3", "民族乐器4", "民族乐器5", "民族乐器6", "民族乐器7", "民族乐器8",
    "打击乐1", "打击乐2", "合成鼓", "声音效果", "反转音效", "吉他品噪声", "呼吸噪声", "海浪",
    "鸟鸣", "电话铃", "直升机", "掌声", "枪声"
])

MAJOR_SCALES = [
    [0, 2, 4, 5, 7, 9, 11], [1, 3, 5, 6, 8, 10, 0],
    [2, 4, 6, 7, 9, 11, 1], [3, 5, 7, 8, 10, 0, 2],
    [4, 6, 8, 9, 11, 1, 3], [5, 7, 9, 10, 0, 2, 4],
    [6, 8, 10, 11, 1, 3, 5], [7, 9, 11, 0, 2, 4, 6],
    [8, 10, 0, 1, 3, 5, 7], [9, 11, 1, 2, 4, 6, 8],
    [10, 0, 2, 3, 5, 7, 9], [11, 1, 3, 4, 6, 8, 10],
]
MINOR_SCALES = [
    [0, 2, 3, 5, 7, 8, 10], [1, 3, 4, 6, 8, 9, 11],
    [2, 4, 5, 7, 9, 10, 0], [3, 5, 6, 8, 10, 11, 1],
    [4, 6, 7, 9, 11, 0, 2], [5, 7, 8, 10, 0, 1, 3],
    [6, 8, 9, 11, 1, 2, 4], [7, 9, 10, 0, 2, 3, 5],
    [8, 10, 11, 1, 3, 4, 6], [9, 11, 0, 2, 4, 5, 7],
    [10, 0, 1, 3, 5, 6, 8], [11, 1, 2, 4, 6, 7, 9],
]

Q_ERRS = (queue_mod.Empty, EOFError, OSError, ValueError)


def _safe_load_midi(path):
    try:
        return mido.MidiFile(path)
    except Exception as e:
        msg = str(e).lower()
        if ('data byte' in msg or 'range' in msg or 'status' in msg
                or 'invalid' in msg or 'unexpected' in msg):
            try:
                return mido.MidiFile(path, clip=True)
            except TypeError:
                raise e
            except Exception:
                raise
        raise


def _get_terminal_size_safe():
    try:
        s = shutil.get_terminal_size()
        return s.columns, s.lines
    except Exception:
        return 120, CONFIG.get('display_height', 40)


def _detect_sync_support():
    forced = CONFIG.get('sync_output')
    if forced is not None:
        return bool(forced)
    if os.environ.get('WT_SESSION'):
        return True
    term_program = os.environ.get('TERM_PROGRAM', '').lower()
    if term_program in ('wezterm', 'iterm.app', 'ghostty', 'vscode',
                        'warpterminal', 'alacritty', 'hyper', 'vscode-insiders'):
        return True
    term = os.environ.get('TERM', '').lower()
    if 'kitty' in term or 'alacritty' in term or 'wezterm' in term:
        return True
    return True


DEFAULT_FG_PALETTE = [
    "\033[91m", "\033[94m", "\033[92m", "\033[93m", "\033[95m", "\033[96m",
    "\033[31m", "\033[34m", "\033[32m", "\033[33m", "\033[35m", "\033[36m",
    "\033[37m", "\033[97m", "\033[90m",
]
DEFAULT_BG_PALETTE = [
    "\033[101m", "\033[104m", "\033[102m", "\033[103m", "\033[105m", "\033[106m",
    "\033[41m", "\033[44m", "\033[42m", "\033[43m", "\033[45m", "\033[46m",
    "\033[47m", "\033[107m", "\033[100m",
]


def _make_palette_pair(n):
    if n <= 0:
        return [], []
    if n <= len(DEFAULT_FG_PALETTE):
        return (list(DEFAULT_FG_PALETTE[:n]), list(DEFAULT_BG_PALETTE[:n]))
    fg, bg = [], []
    for i in range(n):
        idx = i % len(DEFAULT_FG_PALETTE)
        f = DEFAULT_FG_PALETTE[idx]
        b = DEFAULT_BG_PALETTE[idx]
        if i >= len(DEFAULT_FG_PALETTE):
            f = "\033[1m" + f
        fg.append(f)
        bg.append(b)
    return fg, bg


def _producer_proc(midi_path, header_q, event_q, cfg):
    def _push_stage(name, cur, total, delay=0.0):
        try:
            header_q.put({'type': 'stage', 'name': name,
                          'cur': int(cur), 'total': max(int(total), 1)})
        except Exception:
            pass
        if delay > 0:
            time.sleep(delay)

    _push_stage('loading', 0, 1, delay=0.05)
    try:
        reader = FastMidiReader(midi_path)
    except Exception as e:
        header_q.put({'type': 'error', 'message': f'加载失败: {e}'})
        return
    _push_stage('loading', 1, 1, delay=0.08)

    try:
        n_tracks = len(reader.track_ranges)
        ticks_per_beat = reader.ticks_per_beat

        def _scan_progress(cur, total):
            _push_stage('scanning', cur, total, delay=0.02)

        meta = reader.scan_meta(progress_cb=_scan_progress)
        _push_stage('scanning', n_tracks, n_tracks, delay=0.1)

        if cfg.get('use_config_range') and 'min_note' in cfg and 'max_note' in cfg:
            min_note = cfg['min_note']
            max_note = cfg['max_note']
        else:
            min_note = max(0, meta['min_note'] - 3)
            max_note = min(127, meta['max_note'] + 3)

        try:
            header_q.put({
                'type': 'header',
                'filename': os.path.basename(midi_path),
                'n_tracks': n_tracks,
                'ticks_per_beat': ticks_per_beat,
                'tempo_changes': meta['tempo_changes'],
                'programs': meta['programs'],
                'channels_used': meta['channels_used'],
                'tracks_used': meta['tracks_used'],
                'pitch_hist': meta['pitch_hist'],
                'note_count': meta['note_count'],
                'chord_count_est': meta['chord_count_est'],
                'min_note': min_note,
                'max_note': max_note,
            })
        except Exception:
            pass
        time.sleep(0.1)

        tempo_changes = meta['tempo_changes']
        tempo_map = []
        for tick, tempo in tempo_changes:
            if tempo_map and tempo_map[-1][0] == tick:
                tempo_map[-1] = (tick, tempo)
            else:
                tempo_map.append((tick, tempo))
        if not tempo_map:
            tempo_map = [(0, 500000)]

        seg_start_seconds = [0.0]
        for i in range(1, len(tempo_map)):
            prev_tick, prev_tempo = tempo_map[i - 1]
            curr_tick = tempo_map[i][0]
            seg_start_seconds.append(
                seg_start_seconds[-1] + (curr_tick - prev_tick) * prev_tempo / ticks_per_beat / 1_000_000
            )

        def _locate(ticks):
            lo, hi = 0, len(tempo_map) - 1
            while lo < hi:
                mid = (lo + hi + 1) // 2
                if tempo_map[mid][0] <= ticks:
                    lo = mid
                else:
                    hi = mid - 1
            return lo

        def ticks_to_seconds(ticks):
            lo = _locate(ticks)
            start_tick, tempo = tempo_map[lo]
            return seg_start_seconds[lo] + (ticks - start_tick) * tempo / ticks_per_beat / 1_000_000

        def ticks_to_tempo(ticks):
            return tempo_map[_locate(ticks)][1]

        first_note_ticks = meta['first_note_abs_ticks']
        first_note_sec = ticks_to_seconds(first_note_ticks)
        first_note_tempo = ticks_to_tempo(first_note_ticks)
        beats_for_drop = cfg['beats_for_drop']
        min_drop_floor = cfg['min_drop_floor']
        drop_follows_tempo = cfg['drop_follows_tempo']
        first_tempo = tempo_map[0][1] if tempo_map else 500000
        fixed_drop = max(beats_for_drop * first_tempo / 1_000_000, 0.1)

        def calc_drop_time(tempo):
            if drop_follows_tempo:
                raw = beats_for_drop * tempo / 1_000_000
            else:
                raw = fixed_drop
            return max(raw, min_drop_floor, 0.05)

        first_abs = max(0.0, first_note_sec - calc_drop_time(first_note_tempo))

        n_notes = meta['note_count']
        total_events_est = n_notes * 2 + len(tempo_changes)
        min_drop_val = calc_drop_time(min(t for _, t in tempo_map)) if tempo_map else 0.5
        max_drop_val = calc_drop_time(max(t for _, t in tempo_map)) if tempo_map else 0.5

        try:
            header_q.put({
                'type': 'meta',
                'first_abs': first_abs,
                'n_notes': n_notes,
                'n_ctrls': 0,
                'min_drop': min_drop_val,
                'max_drop': max_drop_val,
            })
        except Exception:
            pass

        BATCH = 100
        note_batch: List[Dict] = []
        ctrl_batch: List[Dict] = []
        pushed = 0
        last_progress = 0
        PROGRESS_EVERY = 5000

        note_stacks: Dict[Tuple[int, int, int], List] = {}

        for ev in reader.iter_events():
            t = ev['type']
            abs_ticks = ev['abs_ticks']
            abs_sec = ticks_to_seconds(abs_ticks)

            if t == 'note_on':
                key = (ev['track'], ev['channel'], ev['note'])
                tempo = ticks_to_tempo(abs_ticks)
                note_stacks.setdefault(key, []).append(
                    (abs_sec, ev['velocity'], ev['channel'], tempo)
                )
            elif t == 'note_off':
                key = (ev['track'], ev['channel'], ev['note'])
                stack = note_stacks.get(key)
                if stack:
                    on_sec, velocity, channel, tempo = stack.pop(0)
                    duration = max(0.001, abs_sec - on_sec)
                    if min_note <= ev['note'] <= max_note:
                        drop_time = calc_drop_time(tempo)
                        rel_spawn = (on_sec - drop_time) - first_abs
                        note_batch.append({
                            'abs_time': on_sec,
                            'note': ev['note'],
                            'velocity': velocity,
                            'track': ev['track'],
                            'channel': channel,
                            'duration': duration,
                            'drop_time': drop_time,
                            'rel_spawn_time': rel_spawn,
                        })
                    if len(note_batch) >= BATCH:
                        note_batch.sort(key=lambda e: e['rel_spawn_time'])
                        try:
                            event_q.put({'type': 'notes', 'batch': note_batch})
                        except Exception:
                            return
                        pushed += len(note_batch)
                        if pushed - last_progress >= PROGRESS_EVERY:
                            _push_stage('pushing', pushed, total_events_est, delay=0.02)
                            last_progress = pushed
                        note_batch = []
            elif t in ('program_change', 'control_change', 'pitchwheel',
                       'aftertouch', 'polytouch'):
                try:
                    if t == 'program_change':
                        msg = mido.Message('program_change',
                                           channel=ev['channel'], program=ev['program'])
                    elif t == 'control_change':
                        msg = mido.Message('control_change',
                                           channel=ev['channel'],
                                           control=ev['control'], value=ev['value'])
                    elif t == 'pitchwheel':
                        msg = mido.Message('pitchwheel',
                                           channel=ev['channel'], pitch=ev['pitch'])
                    elif t == 'aftertouch':
                        msg = mido.Message('aftertouch',
                                           channel=ev['channel'],
                                           note=ev['note'], value=ev['value'])
                    else:
                        continue
                    ctrl_batch.append({
                        'rel_time': abs_sec - first_abs,
                        'bytes': bytes(msg.bytes()),
                    })
                except Exception:
                    continue
                if len(ctrl_batch) >= BATCH:
                    try:
                        event_q.put({'type': 'ctrls', 'batch': ctrl_batch})
                    except Exception:
                        return
                    pushed += len(ctrl_batch)
                    ctrl_batch = []

        if ctrl_batch:
            try:
                event_q.put({'type': 'ctrls', 'batch': ctrl_batch})
            except Exception:
                pass
            pushed += len(ctrl_batch)
        if note_batch:
            note_batch.sort(key=lambda e: e['rel_spawn_time'])
            try:
                event_q.put({'type': 'notes', 'batch': note_batch})
            except Exception:
                return
            pushed += len(note_batch)

        _push_stage('pushing', total_events_est, total_events_est, delay=0.1)

        try:
            event_q.put({'type': 'eof'})
        except Exception:
            pass

    except Exception as e:
        import traceback
        try:
            header_q.put({'type': 'error', 'message': f'{e}\n{traceback.format_exc()}'})
        except Exception:
            pass
    finally:
        try:
            reader.close()
        except Exception:
            pass


class MIDIPlayer:
    SCROLL_START_ROW = 3

    def __init__(self):
        self.output: Optional[mido.ports.BaseOutput] = None
        self.min_note: int = CONFIG.get('min_note', 48)
        self.max_note: int = CONFIG.get('max_note', 84)
        self.instruments: Dict[str, int] = defaultdict(int)
        self.note_pos_map: Dict[int, int] = {}
        self.keyboard_positions: List[Tuple[int, str]] = []
        self.display_width: int = 0
        self.display_height: int = CONFIG.get('display_height', 40)
        self._color_key: str = 'track'
        self._color_map_fg: Dict[int, str] = {}
        self._color_map_bg: Dict[int, str] = {}
        self._middle_c_enabled: bool = bool(CONFIG.get('highlight_middle_c', True))
        self._middle_c_bg: str = CONFIG.get('middle_c_bg', "\033[107m")
        self.middle_c_pos: Optional[int] = None
        self._header_line1_cache: Optional[str] = None
        self._header_filename: str = ""
        self._header_n_tracks: int = 0
        self._mode_str: str = ""
        self._bpm_str: str = ""
        self._total_chord_count: int = 0
        self._total_note_count: int = 0
        self._term_width: int = 120
        self._cc_state: Dict[int, Dict[int, int]] = {}
        self._prog_state: Dict[int, int] = {}
        self._pitch_state: Dict[int, int] = {}
        self._cc_labels = {
            7: LANG.get('cc_vol', '音量'),
            10: LANG.get('cc_pan', '声像'),
            91: LANG.get('cc_rev', '混响'),
            64: LANG.get('cc_sus', '延音'),
        }
        self._prog_label = LANG.get('cc_prog', '音色')
        self._sus_on = LANG.get('cc_sus_on', '开')
        self._sus_off = LANG.get('cc_sus_off', '关')
        self._sync_support: bool = _detect_sync_support()
        self._static_row_chars: List[List[str]] = []
        self._static_row_strs: List[str] = []
        self.note_to_index: Dict[int, int] = {}
        self.cell_starts: List[int] = []
        self.cell_widths: List[int] = []
        self.middle_c_cell: Optional[int] = None
        self._kb_cells: List[Tuple[int, int, int, str, bool]] = []
        self._block_style = str(CONFIG.get('block_style', 'bg')).lower()
        if self._block_style not in ('fg', 'bg'):
            self._block_style = 'bg'
        self._init_keyboard_layout()

    def _char_display_width(self, c):
        o = ord(c)
        if (0x1100 <= o <= 0x115F or 0x2E80 <= o <= 0xA4CF or
                0xAC00 <= o <= 0xD7A3 or 0xF900 <= o <= 0xFAFF or
                0xFE30 <= o <= 0xFE4F or 0xFF00 <= o <= 0xFF60 or
                0xFFE0 <= o <= 0xFFE6):
            return 2
        return 1

    def _visible_width(self, s):
        width = 0
        i = 0
        n = len(s)
        while i < n:
            c = s[i]
            if c == '\033' and i + 1 < n and s[i + 1] == '[':
                j = i + 2
                while j < n and not ('@' <= s[j] <= '~'):
                    j += 1
                i = j + 1
            else:
                width += self._char_display_width(c)
                i += 1
        return width

    def _truncate_ansi(self, s, max_width, suffix='…'):
        if self._visible_width(s) <= max_width:
            return s
        suffix_w = self._char_display_width(suffix) if suffix else 0
        target_w = max_width - suffix_w
        if target_w < 0:
            return suffix
        result = []
        width = 0
        i = 0
        n = len(s)
        while i < n:
            c = s[i]
            if c == '\033' and i + 1 < n and s[i + 1] == '[':
                j = i + 2
                while j < n and not ('@' <= s[j] <= '~'):
                    j += 1
                result.append(s[i:j + 1])
                i = j + 1
            else:
                cw = self._char_display_width(c)
                if width + cw > target_w:
                    break
                result.append(c)
                width += cw
                i += 1
        result.append(suffix)
        result.append(COLOR_RESET)
        return ''.join(result)

    def _write_synced(self, s):
        if not s:
            return
        data = s.encode('utf-8', errors='replace')
        if self._sync_support:
            data = b'\033[?2026h' + data + b'\033[?2026l'
        buf = getattr(sys.stdout, 'buffer', None)
        if buf is not None:
            try:
                buf.write(data)
                buf.flush()
                return
            except Exception:
                pass
        sys.stdout.write(s)
        sys.stdout.flush()

    def _reset_effect_state(self):
        self._cc_state = {}
        self._prog_state = {}
        self._pitch_state = {}

    def _update_effect_state(self, msg):
        if msg.type == 'control_change':
            self._cc_state.setdefault(msg.channel, {})[msg.control] = msg.value
        elif msg.type == 'program_change':
            self._prog_state[msg.channel] = msg.program
        elif msg.type == 'pitchwheel':
            self._pitch_state[msg.channel] = msg.pitch

    def _update_effect_state_from_bytes(self, data):
        try:
            msg = mido.Message.from_bytes(data)
            self._update_effect_state(msg)
        except Exception:
            pass

    def _show_progress(self, label, current, total, width=30):
        if total <= 0:
            return
        filled = int(width * current / total)
        bar = '█' * filled + '░' * (width - filled)
        percent = current / total * 100
        sys.stdout.write(f'\r\033[K{label} [{bar}] {current}/{total} ({percent:.1f}%)')
        sys.stdout.flush()

    def _clear_progress(self):
        sys.stdout.write('\r\033[K')
        sys.stdout.flush()

    def _assign_colors(self, channels_used, tracks_used, sample_events):
        color_by = str(CONFIG.get('color_by', 'auto')).lower()
        if color_by == 'auto':
            color_by = 'channel' if len(channels_used) > 1 else 'track'
        if color_by not in ('channel', 'track'):
            color_by = 'track'
        keys = sorted(channels_used) if color_by == 'channel' else sorted(tracks_used)
        if not keys:
            keys = [0]
        fg_list, bg_list = _make_palette_pair(len(keys))
        self._color_key = color_by
        self._color_map_fg = {k: fg_list[i] for i, k in enumerate(keys)}
        self._color_map_bg = {k: bg_list[i] for i, k in enumerate(keys)}
        if CONFIG.get('debug_info', False):
            kind = 'channel' if color_by == 'channel' else 'track'
            print(f"[info] 颜色分组依据: {kind}, 共 {len(keys)} 组: {keys}")

    def _get_key_bg(self, key_val) -> str:
        return self._color_map_bg.get(key_val, "\033[41m")

    def _get_block_visual(self, block) -> Tuple[str, str]:
        key = block.get(self._color_key, 0)
        if self._block_style == 'bg':
            color = self._color_map_bg.get(key, "\033[41m")
            return color, ' '
        else:
            color = self._color_map_fg.get(key, "\033[91m")
            return color, '█'

    def _init_keyboard_layout(self):
        """所有列宽度统一。
        base = max(1, W // n)，display_width = base * n。
        若终端宽度不是 n 的整数倍，右侧留少量空白，但保证每格等宽。
        """
        n = self.max_note - self.min_note + 1
        W = max(self._term_width, n)

        base = max(1, W // n)
        display_width = base * n

        self.note_to_index = {}
        self.cell_starts = []
        self.cell_widths = []
        self.note_pos_map = {}
        self.keyboard_positions = []
        self.middle_c_cell = None
        self.middle_c_pos = None

        pos = 0
        for i, note in enumerate(range(self.min_note, self.max_note + 1)):
            w = base
            center = pos + (w - 1) // 2
            note_name = NOTE_NAMES[note % 12]

            self.note_to_index[note] = i
            self.cell_starts.append(pos)
            self.cell_widths.append(w)
            self.note_pos_map[note] = center
            self.keyboard_positions.append((center, note_name))

            if note == 60:
                self.middle_c_cell = i
                self.middle_c_pos = center

            pos += w

        self.display_width = display_width
        self._build_static_rows()

        self._kb_cells = []
        for i, note in enumerate(range(self.min_note, self.max_note + 1)):
            note_name = NOTE_NAMES[note % 12]
            is_sharp = '#' in note_name
            self._kb_cells.append((
                self.cell_starts[i],
                self.cell_widths[i],
                note,
                note_name,
                is_sharp,
            ))

    def _build_static_rows(self):
        self._static_row_chars = []
        self._static_row_strs = []
        for _ in range(self.display_height):
            chars = [' '] * self.display_width
            for center, _ in self.keyboard_positions:
                if 0 <= center < self.display_width:
                    chars[center] = ' '
            self._static_row_chars.append(chars)
            self._static_row_strs.append(''.join(chars))

    def select_output_port(self):
        ports = mido.get_output_names()
        if not ports:
            print(LANG.get('no_midi_output', "没有找到可用的MIDI输出设备"))
            return None
        print(LANG.get('available_ports', "可用的MIDI输出设备:"))
        for i, port in enumerate(ports):
            print(f"{i}: {port}")
        while True:
            try:
                choice = input(LANG.get("select_port", "请选择设备编号(默认0): "))
                if not choice:
                    choice = 0
                else:
                    choice = int(choice)
                if 0 <= choice < len(ports):
                    return mido.open_output(ports[choice])
                print(LANG.get("invalid_choice", "无效的选择，请重试"))
            except ValueError:
                print(LANG.get("enter_number", "请输入数字"))

    def get_instrument_name(self, program: int) -> str:
        return instruments[program] if program < len(instruments) else f"乐器{program + 1}"

    def _build_header_line1(self) -> str:
        if self._header_line1_cache is not None:
            return self._header_line1_cache
        instruments_list = list(self.instruments.items())
        if len(instruments_list) > 3:
            instruments_str = "、".join(f"{c}{n}" for n, c in instruments_list[:3])
            instruments_str += f"等{len(instruments_list)}种"
        else:
            instruments_str = "、".join(f"{c}{n}" for n, c in instruments_list) if instruments_list else "-"
        text = (
            f"\033[1m{LANG.get('filename', '文件名:')}\033[0m{self._header_filename} "
            f"\033[1m{LANG.get('tracks', '音轨数:')}\033[0m{self._header_n_tracks} "
            f"\033[1m{LANG.get('instrument', '乐器:')}\033[0m{instruments_str} "
            f"\033[1m{LANG.get('tempo', '速度:')}\033[0m{self._bpm_str} "
            f"\033[1m{self._mode_str}\033[0m"
        )
        self._header_line1_cache = text
        return text

    def _build_header_line2(self, played_count, chord_count) -> str:
        parts = [
            f"\033[1m{LANG.get('notes', '音符数:')}\033[0m{played_count}/{self._total_note_count}",
            f"\033[1m{LANG.get('chords', '和弦数:')}\033[0m{chord_count}",
        ]
        for ch in sorted(self._cc_state.keys())[:3]:
            cc = self._cc_state[ch]
            items = []
            if 7 in cc:
                items.append(f"{self._cc_labels[7]}={cc[7]}")
            if 10 in cc:
                items.append(f"{self._cc_labels[10]}={cc[10]}")
            if 91 in cc:
                items.append(f"{self._cc_labels[91]}={cc[91]}")
            if 64 in cc:
                sus_str = self._sus_on if cc[64] >= 64 else self._sus_off
                items.append(f"{self._cc_labels[64]}={sus_str}")
            prog = self._prog_state.get(ch)
            if prog is not None:
                items.append(f"{self._prog_label}={prog}")
            if items:
                parts.append(f"ch{ch}[{' '.join(items)}]")
        return ' | '.join(parts)

    def display_header(self):
        parts = ['\033[2J\033[H']
        w = self._term_width
        line1 = self._build_header_line1()
        line2 = self._build_header_line2(0, self._total_chord_count)
        parts.append(self._truncate_ansi(line1, w) + '\033[K\n')
        parts.append(self._truncate_ansi(line2, w) + '\033[K\n')
        self._write_synced(''.join(parts))

    def _apply_header_info(self, h):
        self._header_filename = h['filename']
        self._header_n_tracks = h['n_tracks']
        self._header_line1_cache = None

        self.instruments = defaultdict(int)
        for prog in h['programs']:
            self.instruments[self.get_instrument_name(prog)] += 1

        tempo_changes = h['tempo_changes']
        tempo_set = set(t for _, t in tempo_changes if t > 0)
        if tempo_set:
            bpms = sorted(int(round(60_000_000.0 / t)) for t in tempo_set)
            if bpms[0] == bpms[-1]:
                self._bpm_str = f"{bpms[0]}BPM"
            else:
                unit = LANG.get('bpm_unit', 'BPM')
                var = LANG.get('tempo_variable', '变速')
                self._bpm_str = f"{bpms[0]}~{bpms[-1]}{unit} ({var})"
        else:
            self._bpm_str = "120BPM"

        pitch_hist = h['pitch_hist']
        major_score = [sum([pitch_hist[n] for n in scale]) for scale in MAJOR_SCALES]
        minor_score = [sum([pitch_hist[n] for n in scale]) for scale in MINOR_SCALES]
        best_major = max(range(12), key=lambda i: major_score[i])
        best_minor = max(range(12), key=lambda i: minor_score[i])
        if major_score[best_major] >= minor_score[best_minor]:
            self._mode_str = (f"{LANG.get('mode', '调式:')}{NOTE_NAMES[best_major]}"
                              f"{LANG.get('major', '大调')} ({LANG.get('match', '匹配度:')}{major_score[best_major]})")
        else:
            self._mode_str = (f"{LANG.get('mode', '调式:')}{NOTE_NAMES[best_minor]}"
                              f"{LANG.get('minor', '小调')} ({LANG.get('match', '匹配度:')}{minor_score[best_minor]})")

        self._total_chord_count = h['chord_count_est']
        self._total_note_count = h['note_count']

        term_cols, term_lines = _get_terminal_size_safe()
        self._term_width = term_cols
        new_h = max(10, term_lines - 5)
        if new_h != self.display_height:
            self.display_height = new_h

        if h['min_note'] != self.min_note or h['max_note'] != self.max_note:
            self.min_note = h['min_note']
            self.max_note = h['max_note']

        self._init_keyboard_layout()

        channels_used = set(h.get('channels_used', [0]))
        tracks_used = set(h.get('tracks_used', [0]))
        self._assign_colors(channels_used, tracks_used, [])

        if CONFIG.get('debug_info', False):
            n_cells = max(1, self.max_note - self.min_note + 1)
            wasted = self._term_width - self.display_width
            print(f"[info] 终端 {term_cols}×{term_lines}, "
                  f"display_height={self.display_height}, display_width={self.display_width}, "
                  f"每格宽={self.cell_widths[0] if self.cell_widths else 0}, "
                  f"右侧留白={wasted}, block_style={self._block_style}")

    _CHORD_DEFS = None

    def _get_chord_defs(self):
        if self._CHORD_DEFS is not None:
            return self._CHORD_DEFS
        defs = [
            ("",    [0, 4, 7]), ("maj", [0, 4, 7]), ("m",   [0, 3, 7]),
            ("min", [0, 3, 7]), ("dim", [0, 3, 6]), ("aug", [0, 4, 8]),
            ("sus2",[0, 2, 7]), ("sus4",[0, 5, 7]), ("7",    [0, 4, 7, 10]),
            ("maj7", [0, 4, 7, 11]), ("m7",   [0, 3, 7, 10]),
            ("m7b5", [0, 3, 6, 10]), ("dim7", [0, 3, 6, 9]),
            ("mMaj7",[0, 3, 7, 11]), ("7b5",  [0, 4, 6, 10]),
            ("7#5",  [0, 4, 8, 10]), ("aug7", [0, 4, 8, 10]),
            ("6",    [0, 4, 7, 9]), ("m6",   [0, 3, 7, 9]),
            ("6/9",  [0, 4, 7, 9, 14]), ("9",    [0, 4, 7, 10, 14]),
            ("maj9", [0, 4, 7, 11, 14]), ("m9",   [0, 3, 7, 10, 14]),
            ("add9", [0, 4, 7, 14]), ("madd9",[0, 3, 7, 14]),
            ("11",    [0, 4, 7, 10, 14, 17]), ("maj11", [0, 4, 7, 11, 14, 17]),
            ("m11",   [0, 3, 7, 10, 14, 17]), ("7#11",  [0, 4, 7, 10, 14, 18]),
            ("13",    [0, 4, 7, 10, 14, 17, 21]), ("maj13", [0, 4, 7, 11, 14, 17, 21]),
            ("m13",   [0, 3, 7, 10, 14, 17, 21]), ("7b9",   [0, 4, 7, 10, 13]),
            ("7#9",   [0, 4, 7, 10, 15]), ("7b13",  [0, 4, 7, 10, 14, 20]),
            ("alt7",  [0, 4, 6, 10, 13, 18]), ("7sus2", [0, 2, 7, 10]),
            ("7sus4", [0, 5, 7, 10]), ("9sus4", [0, 5, 7, 10, 14]),
            ("m7b9",  [0, 3, 7, 10, 13]), ("m7#5",  [0, 3, 8, 10]),
            ("m9b5",  [0, 3, 6, 10, 14]), ("7b9#9", [0, 4, 7, 10, 13, 15]),
            ("7b9#11",[0, 4, 7, 10, 13, 18]), ("7/6",   [0, 4, 7, 9, 10]),
            ("9/13",  [0, 4, 7, 10, 14, 21]),
        ]
        defs.sort(key=lambda x: -len(x[1]))
        self._CHORD_DEFS = defs
        return defs

    def detect_chord(self, notes):
        if not notes:
            return "", [], None
        notes = sorted(set(notes))
        root = notes[0] % 12
        intervals = set((n - notes[0]) % 12 for n in notes)
        for name, pattern in self._get_chord_defs():
            if set(pattern).issubset(intervals) and len(intervals) >= len(pattern):
                chord_name = NOTE_NAMES[root] + name
                chord_notes = [NOTE_NAMES[(root + i) % 12] for i in pattern]
                return chord_name, chord_notes, root
        return "", [], None

    def _build_chord_line(self, chord_name, chord_notes, duration):
        if chord_name:
            return (
                f"\033[1m{LANG.get('label', '和弦:')}\033[0m {chord_name}  "
                f"\033[1m{LANG.get('duration', '持续:')}\033[0m {duration:.2f}{LANG.get('seconds', 's')}  "
                f"\033[1m{LANG.get('components', '构成:')}\033[0m {' '.join(chord_notes)}\033[K"
            )
        return f"\033[1m{LANG.get('label', '和弦:')}\033[0m {LANG.get('none', '无')}\033[K"

    def _render_row_with_blocks(self, line, row_blocks):
        chars = list(self._static_row_chars[line])
        colors = [''] * self.display_width

        for note, ansi_color, fill_char in row_blocks:
            i = self.note_to_index.get(note)
            if i is None:
                continue
            start = self.cell_starts[i]
            w = self.cell_widths[i]
            end = min(start + w, self.display_width)
            for k in range(start, end):
                chars[k] = fill_char
                colors[k] = ansi_color

        parts = []
        last_color = ''
        for col in range(self.display_width):
            c = colors[col]
            ch = chars[col]
            if c != last_color:
                if last_color:
                    parts.append(COLOR_RESET)
                if c:
                    parts.append(c)
                last_color = c
            parts.append(ch)
        if last_color:
            parts.append(COLOR_RESET)
        return ''.join(parts)

    def _render_keyboard_line(self, active_note_map):
        cols = [(' ', '') for _ in range(self.display_width)]

        for start, width, note, name, is_sharp in self._kb_cells:
            bg = ''
            if note in active_note_map and active_note_map[note]:
                key = active_note_map[note][-1].get(self._color_key, 0)
                bg = self._get_key_bg(key)
            elif self._middle_c_enabled and note == 60:
                bg = self._middle_c_bg

            full_code = (bg + '\033[1m\033[30m') if bg else ''

            for k in range(width):
                col = start + k
                if 0 <= col < self.display_width:
                    cols[col] = (' ', full_code)

            if width >= len(name):
                text = name
            else:
                text = '#' if is_sharp else name[0]

            text_len = len(text)
            pad = max(0, (width - text_len) // 2)
            for ti, ch in enumerate(text):
                col = start + pad + ti
                if 0 <= col < self.display_width:
                    cols[col] = (ch, full_code)

        parts = []
        last_code = ''
        for ch, code in cols:
            if code != last_code:
                if last_code:
                    parts.append(COLOR_RESET)
                if code:
                    parts.append(code)
                last_code = code
            parts.append(ch)
        if last_code:
            parts.append(COLOR_RESET)
        return ''.join(parts)

    def play_midi_loop(self, midi_path: str, output_port_name: str = None):
        out = None
        own_output = False
        if self.output:
            out = self.output
        else:
            try:
                out = mido.open_output(output_port_name) if output_port_name else mido.open_output()
                own_output = True
            except Exception as e:
                print(f"{LANG.get('output_failed', '打开MIDI输出失败: ')} {e}")
                return

        cfg = {
            'min_note': self.min_note,
            'max_note': self.max_note,
            'use_config_range': 'min_note' in CONFIG and 'max_note' in CONFIG,
            'beats_for_drop': float(CONFIG.get('beats_for_drop', 2.0)),
            'drop_follows_tempo': bool(CONFIG.get('drop_follows_tempo', True)),
            'min_drop_floor': float(CONFIG.get('min_drop_time_seconds', 0.15)),
        }

        header_q = multiprocessing.Queue()
        event_q = multiprocessing.Queue(maxsize=5000)

        producer = multiprocessing.Process(
            target=_producer_proc,
            args=(midi_path, header_q, event_q, cfg),
        )
        producer.daemon = True
        producer.start()

        header_displayed = False
        meta_received = False
        meta_received_at = None
        start_time = None
        stage_msg = None
        self._reset_effect_state()
        ctrl_events: List[Dict] = []
        ctrl_idx = 0
        note_buffer: deque = deque()
        producer_finished = False

        spawned: List[Dict] = []
        active_note_map: Dict[int, List[Dict]] = defaultdict(list)
        played_count = 0
        pending_offs: List[Tuple[float, int, int]] = []

        prev_scroll_lines: Optional[List[str]] = None
        prev_kb_line: Optional[str] = None
        prev_row_blocks: Dict[int, List[Tuple[int, str, str]]] = {}
        prev_notes_key: Optional[tuple] = None
        prev_chord_name = ""
        prev_chord_notes: List[str] = []
        prev_chord_display: Optional[str] = None
        prev_chord_time = 0.0
        prev_chord_line: Optional[str] = None
        prev_header_snapshot = None

        refresh_rate = max(30.0, min(float(CONFIG.get('refresh_rate', 60)), 60.0))
        frame_interval = 1.0 / refresh_rate

        loop_start_time = time.time()
        print("\033[2J\033[H", end='')
        sys.stdout.flush()

        try:
            sys.stdout.write('\033[?25l')
            sys.stdout.flush()

            while True:
                if start_time is not None:
                    drained = 0
                    while drained < 1000:
                        try:
                            msg = event_q.get(timeout=0.002)
                        except Q_ERRS:
                            break
                        t = msg['type']
                        if t == 'ctrls':
                            ctrl_events.extend(msg['batch'])
                        elif t == 'notes':
                            note_buffer.extend(msg['batch'])
                        elif t == 'eof':
                            producer_finished = True
                        drained += 1

                try:
                    while True:
                        try:
                            h = header_q.get(timeout=0.002)
                        except Q_ERRS:
                            break
                        ht = h['type']
                        if ht == 'error':
                            self._clear_progress()
                            print(f"\n{LANG.get('load_failed', '加载失败: ')} {h['message']}")
                            return
                        elif ht == 'header':
                            self._apply_header_info(h)
                            self.display_header()
                            header_displayed = True
                        elif ht == 'stage':
                            stage_msg = h
                        elif ht == 'progress':
                            stage_msg = {
                                'name': 'building',
                                'cur': h.get('cur', 0),
                                'total': h.get('total', 1),
                            }
                        elif ht == 'meta':
                            self._total_note_count = h['n_notes']
                            meta_received = True
                except Exception:
                    pass

                if not header_displayed:
                    if not producer.is_alive() and not meta_received:
                        self._clear_progress()
                        sys.stdout.write('\n\033[1;31m[错误] 生产者进程已退出，请检查上方错误信息\033[0m\n')
                        sys.stdout.flush()
                        return

                    if stage_msg is not None:
                        cur = int(stage_msg.get('cur', 0))
                        total = max(int(stage_msg.get('total', 1)), 1)
                        name = stage_msg.get('name', '')
                        bar_w = 20
                        filled = max(0, min(bar_w, int(bar_w * cur / total)))
                        bar = '█' * filled + '░' * (bar_w - filled)
                        pct = cur / total * 100
                        label_map = {
                            'loading':  LANG.get('stage_loading', '加载'),
                            'scanning': LANG.get('stage_scanning', '扫描'),
                            'building': LANG.get('stage_building', '分析'),
                            'pushing':  LANG.get('stage_pushing', '收集'),
                        }
                        label = label_map.get(name, name)
                        elapsed = time.time() - loop_start_time
                        if name == 'loading' and cur == 0:
                            line2 = f"{label} [{bar}] 已耗时 {elapsed:.1f}s"
                        else:
                            line2 = f"{label} [{bar}] {cur}/{total} ({pct:.1f}%)"
                    else:
                        spinner = '⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏'
                        idx = int(time.time() * 12) % len(spinner)
                        elapsed = time.time() - loop_start_time
                        line2 = LANG.get('loading', '加载 MIDI 文件') + f'... {spinner[idx]} 已耗时 {elapsed:.1f}s'

                    self._write_synced(
                        f'\033[2;1H{self._truncate_ansi(line2, self._term_width)}\033[K'
                    )
                    time.sleep(0.05)
                    continue

                if start_time is None:
                    if meta_received:
                        if meta_received_at is None:
                            meta_received_at = time.time()
                        elapsed = time.time() - meta_received_at
                        if elapsed < 0.3:
                            bar = '█' * 20
                            label = LANG.get('loading_done', '准备就绪')
                            line2 = f"{label} [{bar}] {self._total_note_count} 音符"
                            self._write_synced(
                                f'\033[2;1H{self._truncate_ansi(line2, self._term_width)}\033[K'
                            )
                            time.sleep(0.03)
                            continue
                        start_time = time.time()
                        self._clear_progress()
                        line2 = self._build_header_line2(0, self._total_chord_count)
                        self._write_synced(
                            f'\033[2;1H{self._truncate_ansi(line2, self._term_width)}\033[K'
                        )
                        prev_header_snapshot = None
                    else:
                        if stage_msg is not None:
                            cur = int(stage_msg.get('cur', 0))
                            total = max(int(stage_msg.get('total', 1)), 1)
                            name = stage_msg.get('name', '')
                            bar_w = 20
                            filled = max(0, min(bar_w, int(bar_w * cur / total)))
                            bar = '█' * filled + '░' * (bar_w - filled)
                            pct = cur / total * 100
                            label_map = {
                                'loading':  LANG.get('stage_loading', '加载'),
                                'scanning': LANG.get('stage_scanning', '扫描'),
                                'building': LANG.get('stage_building', '分析'),
                                'pushing':  LANG.get('stage_pushing', '收集'),
                            }
                            label = label_map.get(name, name)
                            line2 = f"{label} [{bar}] {cur}/{total} ({pct:.1f}%)"
                        else:
                            spinner = '⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏'
                            idx = int(time.time() * 12) % len(spinner)
                            line2 = LANG.get('analyzing', '分析 MIDI 文件') + '... ' + spinner[idx]
                        self._write_synced(
                            f'\033[2;1H{self._truncate_ansi(line2, self._term_width)}\033[K'
                        )
                        time.sleep(0.03)
                        continue

                now = time.time() - start_time

                while ctrl_idx < len(ctrl_events):
                    e = ctrl_events[ctrl_idx]
                    if now >= e['rel_time']:
                        try:
                            out.send(mido.Message.from_bytes(e['bytes']))
                        except Exception:
                            pass
                        self._update_effect_state_from_bytes(e['bytes'])
                        ctrl_idx += 1
                    else:
                        break

                while note_buffer and note_buffer[0]['rel_spawn_time'] <= now:
                    spawned.append(note_buffer.popleft())

                new_spawned = []
                for block in spawned:
                    dt = block['drop_time']
                    progress = (now - block['rel_spawn_time']) / dt
                    if progress >= 1.0:
                        try:
                            out.send(mido.Message(
                                'note_on',
                                channel=block['channel'],
                                note=block['note'],
                                velocity=block['velocity'],
                            ))
                        except Exception:
                            pass
                        active_note_map[block['note']].append({
                            'end_time': now + block['duration'],
                            'track': block['track'],
                            'channel': block['channel'],
                        })
                        heapq.heappush(
                            pending_offs,
                            (now + block['duration'], block['channel'], block['note'])
                        )
                        played_count += 1
                    else:
                        block['steps'] = min(int(progress * self.display_height),
                                             self.display_height - 1)
                        new_spawned.append(block)
                spawned = new_spawned

                while pending_offs and pending_offs[0][0] <= now:
                    _, ch, nt = heapq.heappop(pending_offs)
                    try:
                        out.send(mido.Message('note_off', channel=ch, note=nt))
                    except Exception:
                        pass

                for note in list(active_note_map.keys()):
                    active_note_map[note] = [a for a in active_note_map[note] if a['end_time'] > now]
                    if not active_note_map[note]:
                        del active_note_map[note]

                row_blocks: Dict[int, List[Tuple[int, str, str]]] = {}
                for block in spawned:
                    note = block['note']
                    if note in self.note_to_index:
                        line = block['steps']
                        color, ch = self._get_block_visual(block)
                        row_blocks.setdefault(line, []).append((note, color, ch))

                if prev_scroll_lines is None:
                    scroll_lines = list(self._static_row_strs)
                    for row, rb in row_blocks.items():
                        scroll_lines[row] = self._render_row_with_blocks(row, rb)
                else:
                    scroll_lines = list(prev_scroll_lines)
                    changed = set(prev_row_blocks.keys()) | set(row_blocks.keys())
                    for row in changed:
                        rb = row_blocks.get(row)
                        if rb:
                            scroll_lines[row] = self._render_row_with_blocks(row, rb)
                        else:
                            scroll_lines[row] = self._static_row_strs[row]

                kb_line = self._render_keyboard_line(active_note_map)

                frame_parts = []
                start_row = self.SCROLL_START_ROW

                header_snapshot = (
                    played_count,
                    tuple(sorted(
                        (ch, tuple(sorted(cc.items())))
                        for ch, cc in self._cc_state.items()
                    )),
                )
                if header_snapshot != prev_header_snapshot:
                    line2 = self._build_header_line2(played_count, self._total_chord_count)
                    frame_parts.append(
                        f'\033[2;1H{self._truncate_ansi(line2, self._term_width)}\033[K'
                    )
                    prev_header_snapshot = header_snapshot

                if prev_scroll_lines is None:
                    for i, line in enumerate(scroll_lines):
                        frame_parts.append(f'\033[{start_row + i};1H{line}')
                else:
                    for i, line in enumerate(scroll_lines):
                        if prev_scroll_lines[i] != line:
                            frame_parts.append(f'\033[{start_row + i};1H{line}')

                if prev_kb_line != kb_line:
                    frame_parts.append(
                        f'\033[{start_row + self.display_height};1H{kb_line}'
                    )

                notes_key = tuple(sorted(active_note_map.keys()))
                if notes_key != prev_notes_key:
                    prev_chord_name, prev_chord_notes, _ = self.detect_chord(list(notes_key))
                    prev_notes_key = notes_key
                if prev_chord_name != prev_chord_display:
                    prev_chord_display = prev_chord_name
                    prev_chord_time = now
                duration = now - prev_chord_time if prev_chord_name else 0
                chord_line = self._build_chord_line(prev_chord_name, prev_chord_notes, duration)
                if chord_line != prev_chord_line:
                    chord_row = self.SCROLL_START_ROW + self.display_height + 1
                    frame_parts.append(f'\033[{chord_row};1H{chord_line}')
                    prev_chord_line = chord_line

                if frame_parts:
                    self._write_synced(''.join(frame_parts))

                prev_scroll_lines = scroll_lines
                prev_kb_line = kb_line
                prev_row_blocks = row_blocks

                if (producer_finished and not spawned and not active_note_map
                        and not pending_offs and not note_buffer
                        and ctrl_idx >= len(ctrl_events)):
                    break

                target = start_time + (int(max(now, 0) / frame_interval) + 1) * frame_interval
                wait = target - time.time()
                if wait > 0:
                    time.sleep(wait)

        except KeyboardInterrupt:
            print('\n' + LANG.get('playback_stopped', '播放停止'))
        finally:
            try:
                producer.terminate()
                producer.join(timeout=0.5)
            except Exception:
                pass
            for note in range(128):
                for ch in range(16):
                    try:
                        out.send(mido.Message('note_off', channel=ch, note=note))
                    except Exception:
                        pass
            if own_output:
                try:
                    out.close()
                except Exception:
                    pass
            sys.stdout.write('\033[?25h\033[J')
            sys.stdout.flush()


if __name__ == "__main__":
    multiprocessing.freeze_support()
    player = MIDIPlayer()
    print("\033[2J\033[H")
    file_path = input(LANG.get("enter_midi_path", "请输入MIDI文件路径:")).strip()
    ports = mido.get_output_names()
    if not ports:
        print(LANG.get("no_midi_output", "没有找到可用的MIDI输出设备"))
    else:
        print(LANG.get("select_port", "请选择设备编号(默认0):"))
        for i, port in enumerate(ports):
            print(f"{i}: {port}")
        choice = input(LANG.get("select_port", "请输入设备编号:")).strip()
        print("\033[?25l\033[2J\033[H")
        if not choice:
            choice = 0
        else:
            try:
                choice = int(choice)
            except ValueError:
                choice = 0
        output_port = ports[choice] if 0 <= choice < len(ports) else ports[0]
        try:
            player.play_midi_loop(file_path, output_port)
        finally:
            print('\033[?25h', end='')