# 图标投递包：静态与动态

本文针对小米手环 10 Pro（p67）、固件 **3.101.043**。动态支持需要本版 Chaos 模块。
系统美化 → 桌面图标负责整套图标包；设置 → 应用图标仍单独控制 Chaos 自己的原版/动态图标。

## 快速制作

安装 Python 依赖：

```sh
python3 -m venv .venv
.venv/bin/python -m pip install -r tools/requirements-build.txt
```

准备一个扁平素材目录，文件名使用下文的桌面 stem：

```text
my-icons/
  activities.gif        # 动态：构建器生成静态封面和动画文件
  alarm.png             # 静态：转换成 96×96 原生图标
  settings.png          # 静态
  ctrl_setting.bin      # 可选：已转换的系统 UI 素材，保留原尺寸
```

生成可以侧载的 p67 投递包：

```sh
.venv/bin/python tools/build_icon_pack.py \
  --assets my-icons --name MyIcons \
  --output chaos-iconpack-MyIcons.bin \
  --raw-output MyIcons-pack.bin
```

Windows 使用 `.venv\Scripts\python.exe`。旧入口 `scripts/build_icon_pack.py` 也转发到相同工具。
`--raw-output` 仅用于额外导出内部 CIPK，可省略；正常安装的是 `chaos-iconpack-MyIcons.bin`。
可选参数：`--title "图标投递 MyIcons"`、`--preview 自定义预览.block`、`--id 12位十进制容器ID`。
包短名必须为 1–12 个 ASCII 字母、数字、下划线或连字符；显示标题可以包含中文。

用 AstroBox 等工具侧载投递包，打开对应表盘，点击“投递图标”。显示“已投递”后，
进入 Chaos → 系统美化 → 桌面图标，选中该包。仅替换提供了素材的图标，其余沿用系统图标。
主模块首次升级后需要重启；之后投递新包、切换图标包无需重刷主安装包。
选中包后等待图标与资源的应用批次完成，再返回桌面查看动画；素材较多时需要数秒。

覆盖同名包前，先在 Chaos 中切回“系统原图标”，完成投递后重新选择该包，以重新载入动画索引。
删除后等待清理完成再投递；删除流水线包含桌面、动画及系统 UI 文件，可能需要约 10 秒。
包清单最多 8 个包，同名投递复用包号；满 8 个时需先删除一包，不会自动覆盖别人的包。

## 素材规则

- 桌面 `.png`：构建器输出 96×96、完整透明度的 BGRA 原生 `.bin`。
- 桌面 `.gif`：生成同名 96×96 静态封面、DGF1 动画和动画索引。保留源 GIF 的逐帧合成、
  disposal、时长及循环次数；源 GIF 应有 1–256 帧，每帧 1–60000 ms。
- 桌面 `.bin`：复用已转换的原生图片。旧静态包的 112×112 等规格继续支持；新动态图标的封面必须为 96×96。
- 同名 `.gif` 可搭配 `.png` 或 `.bin` 指定自定义静态封面；不提供时使用 GIF 首帧。
  同一 stem 同时提供 PNG 和 BIN，或同时提供 GIF 和 `_anim.bin`，会报重复素材错误。
- 可直接输入 `<stem>_anim.bin`，但必须同时提供对应静态封面，且动画满足下文 DGF1 格式。
- 控制中心、设置页、日历磁贴等系统 UI 仍使用原来的静态原生 `.bin` 和原尺寸。
  它们的 `ctrl_`、`set_`、`cal_`、`card_` 等 stem 来自 `supervisor/src/res_hook.rs` 的映射表。
  本版 GIF 动效范围是桌面应用图标，不会在 LVGL 文件钩子内执行动画或修改系统页面对象。

每个静态文件最大 512 KiB，每个动画文件最大 1 MiB；一包最多 256 个文件，包含封面、动画和索引。
外容器用 24 位字段保存资源长度，因此整个内部 `pack.bin` 还必须不超过 16777215 字节（小于 16 MiB）。
单个 GIF 转换后超过上限时，减少帧数或画面复杂度后重新构建。工具不会静默抽帧或降低分辨率。
打包器会验证动画时长、压缩边界、资源长度、重复名和路径；不接受目录、路径穿越或未知 stem。

## 兼容矩阵

| 投递包 | 本版模块 | 旧模块 |
|---|---|---|
| 原来的静态 CIPK 投递包 | 保持原有静态应用方式，无需重做 | 原有行为 |
| 新工具制作的纯静态包 | 静态 | 静态 |
| 新工具制作的混合/动态包 | 静态封面 + 桌面动画 | 使用封面；动画需要升级模块 |

旧模块不认识动画附加文件，无法保证删除时清理这些文件；动态包建议配合本版模块使用。
新模块删除图标包时会清理封面、`_anim.bin`、动画索引及原有系统 UI 素材。
投递包仍使用 `CIPK` 魔数和原来的平面文件记录，`index.txt` 的 128 字节格式没有改变。
原投递包无需替换自己的 Lua 脚本；新制作的包使用仓库当前 `installer/icon_pack.lua`。
新工具制作的纯静态包也附带位图为 0 的索引，便于识别只包含个别图标的部分包。
旧模块可能将不含 `activities.bin` 的部分包标为“未投递”，但仍可选择应用；本版模块能正确识别索引。

## 播放边界与资源占用

动效在桌面前台、图标可见且不处于长按编辑/快照转场时播放，息屏与 AOD 不扫描或绘制。
当前可见的动态图标全部播放，不设置数量上限；可见区域变化后会逐步启用新进入的图标。性能设置留待后续版本。
维护阶段每 50 ms 最多加入一个候选图标；活动动画与 Chaos 自己的动画共用一个 UI 定时器，
采用两者较快的节拍（10–50 ms），全部暂停后降到 1000 ms。实际流畅度取决于存储速度和渲染负载。

动画文件不整包载入内存：每个播放器保留头、调色板、帧表，按当前时间读取对应压缩帧。
每个曾播放过的桌面 stem 按需分配约 24 KiB 的持久播放器，包含两份 10 KiB 原生播放缓冲；
最多 38 份，另共用 18 KiB 压缩帧暂存区，原生解码缓存另计。播放器像素不在每帧分配，也不在
切包时释放，以免延迟删除的原生对象引用悬空。每帧按需打开一个动画文件，读取后关闭，不为每个播放图标长期占用句柄；每次重新打开核对文件头和长度。
内存不足、动画文件缺失、损坏或读取失败时保留/恢复静态封面，失败至少间隔 1 秒后再尝试。

新动画素材只放在独立投递包中，不内嵌进 Chaos 主安装包。此前的 Chaos 自身动画仍保留。

## 第三方打包器协议

### p67 外容器

外层为项目原有的 p67 Lua 容器；使用 `tools/p67_pack.py` 构建，至少包含两个资源：

```text
_lua/xiaomi-band-10-pro/main.lua     # 当前 installer/icon_pack.lua 替换标题/短名后的文本
_lua/xiaomi-band-10-pro/pack.bin     # 以下 CIPK 流
```

脚本中的 `"__PACK_NAME__"`、`"__PACK_TITLE__"` 各替换一次，使用合法 Lua 字符串转义。
不要直接安装内部 CIPK 文件。封面预览块与包中桌面图标是两个不同资源，`--preview` 只替换外容器预览。

### CIPK：原协议不变

所有整数均为小端，无对齐填充：

```text
4 bytes  "CIPK"
u32      file_count (1..256)
重复 file_count 次：
  u32    data_length
  u8     filename_length (1..64)
  bytes  ASCII filename
  bytes  data
```

文件名必须匹配 `^[A-Za-z0-9_]+\.bin$`，不能重复，不能包含目录。动画包推荐输出顺序为静态封面、
动画文件、最后动画索引；只有全部文件按 64 KiB 分块写入并分块回读成功后，投递脚本才提交包清单。
清单采用临时文件、回读核对和 rename，写失败不替换原清单。尾部多余数据会被拒绝。

动态例子：

```text
activities.bin          原生静态封面
activities_anim.bin     该图标的 DGF1 动画
alarm.bin               另一个静态图标
animations.bin          20 字节动画位图索引
```

### animations.bin：CIA1

文件恰好 20 字节，Python 编码格式为 `struct.pack('<4s4HQ', b'CIA1', 96, 96, 38, 0, mask)`。

| 偏移 | 类型 | 值 |
|---|---|---|
| 0 | 4 bytes | `CIA1` |
| 4 | u16 | 宽 96 |
| 6 | u16 | 高 96 |
| 8 | u16 | stem 数 38 |
| 10 | u16 | 保留，必须为 0 |
| 12 | u64 | 动画位图；第 i 位为 1 表示 stem i 有动画 |

第 38–63 位必须为 0。没有索引、索引不合法或没有对应动画时，只使用静态封面。
位顺序固定，与 `icon_apply.rs::ICON_STEMS` 一致：

| bit | stem | bit | stem |
|---|---|---|---|
| 0 | activities | 19 | oxygen |
| 1 | aivs | 20 | perpetual_calendar |
| 2 | alarm | 21 | pressure |
| 3 | alipay | 22 | recorder |
| 4 | breath | 23 | settings |
| 5 | calendar | 24 | share |
| 6 | camera | 25 | sleep |
| 7 | card | 26 | sports |
| 8 | chronograph | 27 | sports_course |
| 9 | compass | 28 | sports_record |
| 10 | dealt | 29 | sports_status |
| 11 | findphone | 30 | timer |
| 12 | flashlight | 31 | todo |
| 13 | heartrate | 32 | tomato_clock |
| 14 | innovation_research | 33 | vitality |
| 15 | interconnect | 34 | weather |
| 16 | mijia | 35 | womenhealth |
| 17 | music | 36 | worldclock |
| 18 | mute | 37 | wxpay |

例如 activities 与 settings 为动态：`mask = (1 << 0) | (1 << 23)`。

### DGF1 动画文件

头部恰好 60 字节，Python 格式为 `<4s4H4I32s`：

| 偏移 | 类型 | 内容 |
|---|---|---|
| 0 | 4 bytes | `DGF1` |
| 4 / 6 | u16 / u16 | 宽 / 高，均为 96 |
| 8 | u16 | 帧数 1..256 |
| 10 | u16 | 保留，必须为 0 |
| 12 | u32 | 一轮总时长 ms，必须等于各帧时长之和 |
| 16 | u32 | 总播放轮数，0 表示无限 |
| 20 | u32 | 帧表偏移，固定 1084 |
| 24 | u32 | 负载偏移，`1084 + frame_count * 12` |
| 28 | 32 bytes | 源 GIF 的 SHA-256 原始字节 |

头后是 1024 字节调色板（256 项，每项按 B、G、R、A 顺序），随后为帧表。
每个帧记录是三个 u32：`delay_ms`、相对文件起点的 `data_offset`、`encoded_length`。
延迟范围 1–60000 ms，各帧数据必须按表序连续存放，最后一帧结束必须等于文件长度。
单帧压缩数据最多 18432 字节，解码后必须恰好为 9216 字节调色板索引。

压缩采用项目的 PackBits 变体：读一个标记 `tag`，长度为 `(tag & 127) + 1`；
若最高位为 1，将紧随的一个字节重复该长度；否则复制紧随的该长度字节。
第三方工具建议直接复用 `tools/build_gif_icon.py::convert` 和
`tools/build_icon_pack.py::validate_animation/encode_cipk`，避免时序、透明度或偏移不一致。

### 包清单 index.txt

路径为 `/data/chaos/icons/index.txt`，恒为 128 字节：8 行，每行 16 字节。
有效行是 `[两位十进制包号][空格][12 字节短名，空格补齐][换行]`；空槽为 16 个空格。
包号范围 01–08。升级协议没有修改这层格式。

## 验证与已知边界

```sh
.venv/bin/python -m pip install -r tools/requirements-test.txt
.venv/bin/python tools/test_icon_pack.py
.venv/bin/python tools/test_dynamic_icon.py
.venv/bin/python tools/test_gif_icon.py
.venv/bin/python tools/verify_dynamic_icon_abi.py /path/to/vela_ap.bin --probe
```

完整主机调度回归需要 Linux 和 Rust；Lua 回归通过 Lupa 执行真实的投递脚本，覆盖旧 CIPK、
大于 512 KiB 的动画、短读、路径穿越、重复记录、满槽及写入失败。ARM 探针核验原生图源、路径所有权
和两类动画共用定时器；桌面多图标的调度与文件读取使用主机模型测试。
目前尚未真机验收多图标动画的帧率、功耗、系统 UI 转场和低内存表现。
