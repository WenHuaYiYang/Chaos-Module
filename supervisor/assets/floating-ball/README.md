# 灵动点素材

- `sphere-source.png`：留档外形参考，不参与当前平面样式的生成。
- `sphere.bgra`：原生64×64平面底色及中央电池图标的预乘BGRA；纯色、无高光/渐变/阴影，机端支持半透明开关。
- `battery-stroke.a8`：64×64完整360°粗环的抗锯齿覆盖率；运行时以浅色/灰色显示进度，空心小圆点跟随收缩端移动。
- `arc-masks.bin`：32～72px球体对应的圆角弧带与灰色短隔条覆盖率，4倍采样后抗锯齿降采样，ARB2有界PackBits编码。
- `glyphs.bin`：GAF1灰度字形图集，来自下述OFL字体子集，用于逐字旋转的弧形文字。
- `label-font.otf`：Noto Sans CJK SC Regular的字形子集，修改后的字体族名为Chaos Arc UI。字体著作权与许可保留在字体名称表及`OFL.txt`中。
- `ASSETS.json`：字体来源、字符范围和生成物SHA-256。

重新生成：`python3 tools/build_floating_ball_assets.py`。正常构建不需要联网下载字体。字体来源：https://github.com/notofonts/noto-cjk/tree/main/Sans 。
