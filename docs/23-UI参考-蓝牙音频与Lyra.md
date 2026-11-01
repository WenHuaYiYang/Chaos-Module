# UI 参考：蓝牙音频模块与 Lyra 播放器的重构

看的是两份刚重构过的实现，结论只保留对 Chaos 有用的部分。

- 蓝牙音频：`Searchstars/Canopus-Module-BluetoothAudio`
  - `7e8a1b5 feat(ui): redesign the Headphones app around what the user needs`
  - `a05797f Use Supervisor-owned native app registration`
- Lyra：`AstralSightStudios/Canopus-App-LyraPlayer`
  - `36a5fc2 Replace main with the contents of ng`（整棵 `lyra-player-core` 重写）
  - `docs/PERFORMANCE.md`（Mirage 实测的界面预算）

两边都基于 Canopus ABI 2 的 UI V2：模块声明页面树、样式变体和事件，Supervisor 负责注册、分配 app id、发布图标、按树建原生控件。Chaos 不走 Canopus，所以只借设计，不借 ABI。

## 1. 页面是数据，不是绘制代码

蓝牙音频：`crates/bluetooth-audio-core/src/ui.rs`

```rust
let mut tree = Tree::begin();
tree.navigation_page(key::PAGE, "耳机")?;
tree.section(20, "连接新耳机")?;
tree.action_row(key::DEVICE_BASE + i, "耳机名", "信号 -62 dBm", EVENT_DEVICE_BASE + i, idle)?;
tree.button(key::SCAN, "搜索耳机", EVENT_SCAN, enabled)?;
tree.set_style(key::SCAN, variant(ComponentVariant::Filled))?;
```

- **语义节点**：`navigation_page / section（分组标题）/ status_row（标签+值）/ action_row（标签+说明+事件）/ switch_row / button`。渲染层只认这几种，页面代码不碰 LVGL。
- **样式变体**：`Default / Filled（页面主操作）/ Destructive（断开、删除）`。每个页面有且只有一个 Filled。
- **状态药丸**：副标题文字 + `ColorRole`（Success / Warning / Danger / Accent）→ 背景着色，不是纯文字。
- **诊断单独一页**：所有底层计数器、错误码、AVDTP 拒绝原因都留在第三页，用户页只留一句人话 + 需要时附错误码。

## 2. 布局度量当常量，且有测试

Lyra：`crates/lyra-player-core/src/layout.rs`

- 每块面板一组 `Metrics`：`Rect{x,y,w,h}` + 字号 + 行高，例如 Band 10 Pro：`width 336, height 480, large_px 32, small_px 24`，列表 `row_height 80、side 10、thumb 60、text_x 84`。Band 11 是另一组常量，页面代码不写 if-else。
- **行高由字体推出来**，不是拍脑袋：MiSans `hhea` 1.326 em，`line_height(px) = (px*1326+999)/1000 + 1`；24px → 32，32px → 43。我们的 88/56/64 也可以按这个换算。
- 测试直接断言几何：行内缩略图居中、`text_x` 右侧至少 120px、标题行高够、`header.bottom <= first_row`、`thumb_radius*4 < thumb.w`、播放页 13 个 `Rect` 两两不重叠（`player_rects()` + `overlaps()`）。

## 3. tick 预算是硬的

Lyra `docs/PERFORMANCE.md`（Mirage，不是真机）：

- 曲库一次 tick 建 64 行要 105 ms；改成 `lv::Budget` 按时间建（24 ms/tick，至少建一个）后最坏 37 ms。
- 歌词页 16～56 ms/tick 分开设预算。
- 结论：**界面对象必须分批建**，一次 tick 做太久，触摸、滚动、进度条全停。Chaos 的 40 行应用列表同理，重建一次全量行在慢设备上是肉眼可见的卡。

## 4. 不重建、不改滚动位置

- 蓝牙音频 `ui_backend.rs:1087`：
  > `lvx_align_to` against the scrolled content itself would add the current scroll offset and jump the page back to its top, so the first object aligns within its parent and the rest follow it.
  即：第一个对象对齐到父容器（`lvx_object_align(ALIGN_TOP_MID)`），后面的用 `lvx_align_to` 串在它下面；这样重建布局不会把列表弹回顶部。
- 两边都用 **稳定的语义 key** 匹配旧对象：蓝牙音频用地址哈希 `device_key(address)`，扫描结果按 RSSI 重排时行对象不换；Lyra 每行按曲目 id。
- 布局有 hash：内容没变就跳过整轮重建（`layout_hash`）。

**这对 Chaos 是当前最大的一条**：`AppMgrPage::tick()` 每次操作后 `render_req(1)` 全量重建 40 行，滚动位置会回顶，且每次点一下都重建。应该改成：
1. 隐藏/显示/勾选这类只改状态的，直接 `row_update` 那一行（副标题颜色 + 勾选态）；
2. 只有列表本身变了（卸载、重扫）才重建；
3. 真要重建时，第一个对象对齐到标题而不是 content。

## 5. 颜色与间距令牌

蓝牙音频 `ui_backend.rs`：

```
WHITE 0xFFFFFF  SECONDARY 0x8E8E93  ACCENT 0x2D7CF6
SUCCESS 0x30D158 WARNING 0xFF9F0A DANGER 0xFF453A
SURFACE 0x2A2A2A  ACCENT_TINT 0x10284D  SUCCESS_TINT 0x0F3D1D
WARNING_TINT 0x4A3008  DANGER_TINT 0x4D1613
```

间距：`TOP 4 / AFTER_ICON 12 / AFTER_TITLE 10 / AFTER_PILL 10 / AFTER_TEXT 6 / BEFORE_GROUP 22 / AFTER_HEADER 6`；按钮高 64（Band 10 Pro）/ 56（Band 11）；行左右内缩 4，文字内缩 24。

Chaos 现在颜色是散的（`C_VISIBLE 0x4A9EFF`、`C_HIDDEN 0xE6A23C`、`TEXT_GRAY_SUB 0x999999`），可以并到一组令牌里：`ACCENT / WARNING / DANGER / SECONDARY`，状态副标题用 `WARNING`（已隐藏）和 `SECONDARY`（显示中），危险操作用 `DANGER`。

## 6. 文案

- 失败说人话，再附错误码：`"连接失败（-1107）"`，页面里不出现 `avdtp_open` 这类内部名。
- 主操作随状态变：没连接时按钮是「搜索耳机」且是 Filled；连接中该按钮禁用（`enabled=false` 变灰）。
- 长名字省略号收尾，不用跑马灯（Lyra 专门为此把滚动标题移出投影区，否则一直重绘，Idle 53%）。

## 7. 建议的落地顺序（Chaos）

1. **P0：页1/批量页的状态改动改成就地更新**，不再整页重建；重建时首对象对齐标题。解决滚动回顶 + 点击卡顿（对应第 4 节）。
2. **P1：列表分批建**，每 tick 预算 ~20 ms，先建首屏 6 行，其余下个 tick 续（对应第 3 节）。
3. **P2：颜色/间距令牌 + 行高按字体换算**（第 2、5 节），顺手把页1 的「状态」行做成药丸样式。
4. **P3：分组标题**（例如「快应用」/「系统应用」/「已隐藏」），以及把调试计数（看门狗指纹、注册表节点数）挪到一个「诊断」页。
