# MIDI 钢琴卷帘可视化播放器（Rust 版）

终端里把 MIDI 渲染成下落音符并输出到 MIDI 设备。Rust 版即 2.0，Python 版（main 分支）已停止维护。

## 运行

```bash
cargo run --release
```

按提示输入 MIDI 文件路径和 MIDI 输出设备编号。

## 配置

- `config.json`：语言、配色、下落速度、帧率、拖尾、双行插值、半格、每秒音符数限制等。
- `lang/`：多语言文案。

## 构建

```bash
cargo build --release
```
