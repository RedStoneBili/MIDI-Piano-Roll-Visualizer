# MIDI 钢琴卷帘可视化播放器（Rust 版）

终端里把 MIDI 渲染成下落音符并输出到 MIDI 设备。Rust 版即 2.0，Python 版（main 分支）已停止维护。

## 项目结构

```
src/
  main.rs        入口、播放列表循环、生产者、MIDI 发送线程
  config.rs      Config / FxConfig / PlaylistConfig（加载 + 自修正补全）
  playlist.rs    路径收集、顺序/循环/随机
  player.rs      布局、调色、和弦、调式、渲染
  midi.rs        MIDI 解析
  util.rs        ANSI / 宽度 / JSON 小工具
  effects/
    mod.rs       Effect trait、FrameCtx、FxManager 加载器
    velocity_glow.rs、trail_fade.rs、attack_flash.rs、beat_grid.rs
    theme.rs、mode_palette.rs、key_ripple.rs、percussion_mark.rs
    chord_degrees.rs、screen_flash.rs、particles.rs、aurora.rs、shockwave.rs、fireworks.rs
  climax.rs      高潮检测（全曲扫描 → 高潮点）
  effects/climax_shake.rs  高潮抖动
```

新增特效：在 `effects/` 下加一个模块，实现 `Effect`，并暴露两个函数：

```rust
pub fn defaults() -> Value;                  // 本特效声明的默认配置
pub fn build(v: &Value) -> Box<dyn Effect>;  // 按配置实例化
```

然后在 `effects/mod.rs` 的 `registry()` 里加一行。配置加载器会自动补全 `fx.effects.<名字>` 的所有缺失项，未知键会被清理。

## 运行

```bash
cargo run --release
```

按提示输入 MIDI 文件路径和 MIDI 输出设备编号。

## 配置

- `config.json`：语言、配色、下落速度、帧率、拖尾、双行插值、半格、每秒音符数限制等。
- 每个字段的中文说明见 [CONFIG.md](CONFIG.md)。
- `lang/`：多语言文案（以中文为底叠加，缺失项自动回落）。
- 更新记录见 [CHANGELOG.md](CHANGELOG.md)。

## 播放列表

- 启动时输入一个或多个路径，**文件夹**会自动收集其中的 `.mid`/`.midi`；多个路径用 `;` 分隔。
- 随后用键盘选择播放模式：`1` 顺序（默认，播完即止）、`2` 循环、`3` 随机。
- 文件夹默认不递归；`config.json` 的 `playlist.recursive` 可开启递归。

## 特效

所有特效都在 `config.json` 的 `fx` 节里独立配置，可单独开关：

```json
"fx": {
  "enabled": true,
  "level": 1,
  "effects": {
    "velocity_glow": { "enabled": true, "threshold": 100 },
    "trail_fade":    { "enabled": true, "chars": "█▓▒░" },
    "attack_flash":  { "enabled": true, "frames": 3 },
    "beat_grid":     { "enabled": true, "beat_char": "·", "bar_char": "─", "beats_per_bar": 4 },
    "theme":         { "enabled": true, "name": "neon" },
    "mode_palette":  { "enabled": true },
    "key_ripple":    { "enabled": true, "radius": 3, "frames": 6 },
    "percussion_mark": { "enabled": true, "glyph": "◆" },
    "chord_degrees": { "enabled": true },
    "screen_flash":  { "enabled": false, "on_bar": true },
    "particles":     { "enabled": false, "density": 20 },
    "aurora":        { "enabled": false },
    "shockwave":     { "enabled": false, "beats_per_bar": 4 },
    "fireworks":     { "enabled": false }
  }
}
```

- 主题：`neon` / `midnight` / `candy` / `cyberpunk`。
- 底部内置进度条由 `progress_bar` 控制。
- 节拍网格按 tempo 同步下落（`beat_grid.beats_per_bar` 指定每小节拍数）。
- 特效由统一加载器按配置实例化；每个特效的参数独立读取，未知的键会原样保留，方便扩展/自定义。
- `fx.enabled` 为总开关，`fx.level` 为强度档位。

## 配置自修正

启动时程序会读取 `config.json`，自动补全缺失字段、修正非法值，并在有变化时写回；未知的自定义字段会保留，不会被删除。

## MIDI 发送

MIDI 发送在独立线程中进行，渲染循环只入队、不等待设备：`midi_queue_size` 为 `0` 时无界队列（默认），大于 `0` 时改为有界队列，满了丢弃新消息。

## 构建与测试

```bash
cargo build --release   # 构建
cargo test              # 单元测试（配置校验、播放列表、调式拼写、和弦、MIDI 解析等）
cargo clippy --release  # 静态检查（当前 0 警告）
```

## 健壮性与终端行为

- **尺寸自适应**：运行中改变终端窗口大小，会自动重建布局并整屏重绘。
- **退出还原**：正常结束、`Ctrl+C`、panic 都会恢复终端（自动换行、光标、备用屏幕），不会留下“光标不见了/整屏卡住”的状态。
- **配置容错**：`note_names` 不足 12 项、数值越界、`min_note > max_note`、tempo 为 0 等都会自动修正，不会崩溃。
- **配置备份**：目标 `config.json` 若不是本程序的配置，补全前先存一份 `config.json.bak`。
- **多语言兜底**：语言文件以中文文案为底叠加，只翻译了一部分（或文件为空）也不会出现空白文案。
- **MIDI 解析**：截断/损坏的轨道只跳过，不会死循环；变速表查找用二分，几百万音符的大文件也不会拖慢每帧。
