# 配置说明（config.json）

> 键名必须保持英文（程序按这些名字读取），这里给出每个字段的中文含义。
> 程序启动时会自动补全缺失字段、修正非法值并写回；**未声明/多余的配置项会被自动删除**（未知顶层字段、未注册特效、特效中未声明的参数）。

## 顶层字段

| 键 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `language` | string | `lang/zh-CN.json` | 界面文案语言文件路径 |
| `note_names` | string[12] | C C# D … | 初始音名表；运行时会按调式/调号自动重写（必须 12 项，缺项/空项会自动补默认音名） |
| `color_reset` | string | `\x1b[0m` | 颜色复位转义串 |
| `highlight_middle_c` | bool | `true` | 是否高亮中央 C |
| `middle_c_bg` | string | `\x1b[107m` | 中央 C 的背景色 |
| `color_by` | string | `auto` | 配色依据：`auto` / `channel`（按通道）/ `track`（按轨道） |
| `beats_for_drop` | number | `2.0` | 音符下落覆盖多少拍（越大下落越慢）；屏幕上可见的小节线数量 ≈ `beats_for_drop / beats_per_bar`；自动夹到 0.25–64 |
| `drop_follows_tempo` | bool | `true` | 下落时间是否跟随速度变化 |
| `min_drop_time_seconds` | number | `0.15` | 下落时间下限（秒），自动夹到 0.02–5 |
| `refresh_rate` | number | `60` | 刷新率，自动夹到 30–120 |
| `block_style` | string | `bg` | 音符块样式：`fg`（前景色 `█`）/ `bg`（背景色空格） |
| `trail_length` | int | `0` | 拖尾长度：`-1` 无限、`0` 关闭、`>0` 固定行数 |
| `trail_by_note` | bool | `false` | 拖尾长度是否随音符时值变化 |
| `dual_line_interp` | bool | `false` | 双行插值（半格音符上下两行显示） |
| `half_cell_notes` | bool | `true` | 半格音符（整格变半格） |
| `max_notes_per_second` | int | `0` | 每秒音符上限：`0` 不限、`-1` 全部静音、`N` 最多 N 个（优先丢弱音） |
| `midi_queue_size` | int | `0` | MIDI 发送队列：`0` 无界，`>0` 有界（满丢新） |
| `keyboard_highlight` | string | `fg` | 按键高亮方式：`fg` 前景加粗（背景不变）/ `bg` 背景色高亮 / `off` 不高亮 |
| `progress_bar` | bool | `true` | 底部内置进度条（已播放音符比例） |
| `sync_output` | bool/null | `true` | 同步输出（防撕裂）：true/false/null=自动检测 |
| `debug_info` | bool | `false` | 调试信息 |
| `default_tempo` | int | `500000` | 默认速度（微秒/拍），MIDI 未声明 tempo 时使用；写 0 会回退 500000，MIDI 里 0 的 tempo 也会被抬到 1 |
| `min_note` / `max_note` | int | 可选 | 强制显示的音域下限/上限（不写则自动）；自动夹到 0–127，写反了会自动交换 |

## fx（特效总配置）

| 键 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `fx.enabled` | bool | `true` | 特效总开关 |
| `fx.level` | 0–2 | `1` | 特效强度档位（预留） |
| `fx.effects` | object | — | 每个特效的独立配置；也可把特效键直接写在 `fx` 下 |

每个特效都支持 `"enabled": true/false`，其余参数如下。

### velocity_glow 力度辉光
| 键 | 默认 | 说明 |
|---|---|---|
| `threshold` | `100` | 力度 ≥ 该值时加粗 |

### trail_fade 拖尾渐隐
| 键 | 默认 | 说明 |
|---|---|---|
| `chars` | `"█▓▒░"` | 由近到远的拖尾字符 |

### attack_flash 起音闪光
| 键 | 默认 | 说明 |
|---|---|---|
| `frames` | `3` | 闪光持续帧数 |

### beat_grid 节拍网格（按 tempo 同步下落）
| 键 | 默认 | 说明 |
|---|---|---|
| `beat_unit` | `4` | 以几分音符为一拍：`2`/`4`/`8`/`16`/`32` |
| `beat_full_width` | `false` | 拍线是否横贯整行（`false` 只在最左侧画刻度，写入量小） |
| `beats_per_bar` | `4` | 每小节几拍（以 `beat_unit` 为单位；4/4 且 `beat_unit=4` 时为 4） |
| `beat_char` | `"·"` | 每拍横线的点状字符（隔列绘制） |
| `bar_char` | `"─"` | 小节线字符（整行绘制） |
| `color` | `\x1b[90m` | 网格颜色 |

节拍线会像音符一样随音乐下落，位置由当前拍号和 `beats_for_drop` 决定，不再是固定屏幕行。

### theme 配色主题
| 键 | 默认 | 说明 |
|---|---|---|
| `name` | `"neon"` | `neon` / `midnight` / `candy` / `cyberpunk` |

### mode_palette 调式配色
按当前调的音级给音符头上色（256 色），主音最亮。

### key_ripple 键盘波纹
| 键 | 默认 | 说明 |
|---|---|---|
| `radius` | `3` | 左右扩散半音数 |
| `frames` | `6` | 持续帧数 |

### percussion_mark 鼓轨标记
| 键 | 默认 | 说明 |
|---|---|---|
| `glyph` | `"◆"` | 鼓轨使用的字符 |
| `color` | `\x1b[93m` | 鼓轨颜色 |

### chord_degrees 和弦级数
在和弦行附加罗马数字级数（如 `V`）。

### screen_flash 重音屏闪
| 键 | 默认 | 说明 |
|---|---|---|
| `on_bar` | `true` | 每小节开头高亮 |

### particles 背景粒子
| 键 | 默认 | 说明 |
|---|---|---|
| `density` | `20` | 每千格的粒子数 |

### aurora 极光背景
| 键 | 默认 | 说明 |
|---|---|---|
| `speed` | `1.0` | 流动速度（0.2–4.0） |

整屏流动的彩色极光背景，音符会覆盖在其上。

### shockwave 冲击波
| 键 | 默认 | 说明 |
|---|---|---|
| `beats_per_bar` | `4` | 每小节几拍（用于小节自动触发） |
| `thickness` | `1.0` | 波环厚度 |
| `radius` | `0` | 最大扩散半径（格）；`0` = 自动铺满屏幕 |
| `duration` | `2.2` | 一次冲击波持续秒数 |
| `use_256` | `true` | `true` 用 256 色渐变；`false` 用 16 色（白/黄/红） |

每小节以及力度 ≥110 的音符会从键盘位置扩散出圆环波。

### fireworks 烟花
| 键 | 默认 | 说明 |
|---|---|---|
| `min_velocity` | `100` | 触发烟花的最小力度 |

强力度音符在键盘处炸开粒子，带重力回落。

### climax_shake 高潮抖动
| 键 | 默认 | 说明 |
|---|---|---|
| `duration` | `0.5` | 每次抖动持续秒数 |
| `intensity` | `2.0` | 最大偏移格数（横/纵） |
| `flash` | `true` | 抖动期间是否加亮音符与键盘 |

## climax（高潮检测）

| 键 | 默认 | 说明 |
|---|---|---|
| `enabled` | `true` | 是否进行全曲高潮检测 |
| `bucket` | `0.5` | 强度统计的时间桶（秒） |
| `smooth` | `2.0` | 平滑窗口（秒） |
| `top_n` | `3` | 最多取几个高潮点 |
| `min_gap` | `8.0` | 高潮点之间最小间隔（秒） |
| `exclude_edges` | `3.0` | 排除开头/结尾各多少秒 |
| `min_std` | `1.0` | 峰值至少为“中位数 + 多少倍标准差” |
| `min_median_ratio` | `1.3` | 峰值至少为中位数的多少倍 |
| `weights.density` | `0.35` | 音符密度权重 |
| `weights.velocity` | `0.25` | 力度权重 |
| `weights.pitch` | `0.15` | 音高亮度权重 |
| `weights.range` | `0.10` | 音域跨度权重 |
| `weights.harmony` | `0.10` | 和声丰富度权重 |
| `weights.tempo` | `0.05` | 速度权重 |

检测结果会在全曲扫描时算好，播放到高潮点时由 `climax_shake` 触发全屏偏移抖动。

## playlist（播放列表）

| 键 | 默认 | 说明 |
|---|---|---|
| `mode` | `"order"` | `order` 顺序 / `loop` 循环 / `random` 随机；启动时仍可用键盘临时覆盖 |
| `recursive` | `false` | 输入文件夹时是否递归子目录 |

## 兼容/保留字段

`color_clear`、`color_bg_white`、`ticks_per_beat` 目前不参与渲染，仅为兼容旧配置而保留。

## 运行期行为

- **终端尺寸变化**：捕获到终端窗口大小改变时自动重建布局并整屏重绘（约每 15 帧检查一次）。
- **退出还原**：正常结束、按 Ctrl+C、以及程序 panic 时都会恢复终端（重新打开自动换行、显示光标、切回主屏幕缓冲区）。
- **配置备份**：如果目标 `config.json` 不是本程序的配置（比如别的版本留下的），补全前会先备份成 `config.json.bak`。

## 颜色写法

所有颜色都是 ANSI 转义串，例如：
- `\x1b[91m` 亮红前景、`\x1b[41m` 红背景
- `\x1b[38;5;196m` / `\x1b[48;5;196m` 256 色前景/背景
- `\x1b[1m` 加粗、`\x1b[7m` 反显
