# 调试信息页（页37）

**入口**：主页 → 关于（页18）**连点 5 次**进入调试模式 → 行「③ 调试信息」。

整页是一个**纯文本 label**（不建卡片行），内容由 `supervisor/src/debug_info.rs` 的
`refresh()` 生成，铺在 `supervisor/src/page/debug.rs`。文本超出一屏时由 content 原生滚动。

## 用途

所有"需要肉眼确认裸数值"的测试都往这里加，页面本身不用改：

```rust
// debug_info.rs :: refresh()
w.s(b"-- system --").c(b'\n');
w.s(b"top ").n(app).c(b':').n(page).c(b'\n');
// ↑ 在这里继续追加，一行一个字段
```

只读采集：不写固件结构、不改配置。

## 当前内容

```
-- display --
disp=1011540504
flip=0
 +20: .. 八个字节 ..
 ...
 +58: .. 八个字节 ..
-- before flip --
 +20: .. 翻转前那一刻的同一段 ..
-- system --
top 200:37
pages 38
```

- `disp`：`lv_display_get_default()` 返回的显示对象指针；
- `+20:` 起 64 字节是 `disp+0x20..0x5F` 的原始值；`-- before flip --` 是上一次点开关**之前**的同一段，
  两组逐字节对比，变化的才是旋转相关字段；
- 实测未翻转时 `+0x39 = 01`：LVGL 9 的 `render_mode` 枚举是 0=PARTIAL 1=DIRECT 2=FULL，
  而固件 `0x0C13D8FA` 断言该字段取值 1..2（抗锯齿只在 DIRECT/FULL 有效）—— 故 `+0x39` 应为
  `render_mode`，且当前是 **DIRECT**（无二级缓冲，直接写显存，天然会撕裂）。
- `flip`：屏幕翻转开关当前状态；
- `top`：最上层页面的 `app_id:page_id`；`pages`：注册页总数。

## 背景

这些数据原先临时挂在设置页最后一行，现已全部移到本页，设置页恢复为纯开关布局。
