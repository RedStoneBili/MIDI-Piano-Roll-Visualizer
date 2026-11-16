# MIDI Piano Roll Visualizer (Rust)

终端里的 MIDI 下落式钢琴卷帘可视化播放器，Rust 重写版。

## 运行

```bash
cargo run --release
```

按提示输入 MIDI 文件路径，选择 MIDI 输出设备。

## 配置

- `config.json`：语言、调色、下落速度、刷新率、拖尾、双行插值、半格音符等。
- `lang/*.json`：多语言文案。
