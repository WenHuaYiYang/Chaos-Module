# LVGL 符号地图（p67 / vela_ap.bin）

> 目的：把 p67 上 LVGL 的类描述、构造入口和常用绘制函数一次标定完，避免每次现查。
> 数据来源：`firmware/10pro/fwx/vela_ap.bin` 的 `.data/.rodata` + 运行时 LVGL 代码段 dump（`0x1C060000..0x1C0A0000`）。
> 生成器：`tools/lvgl_map.js`（扫描类描述）。

## 1. 类描述表

类描述结构（`lv_obj_class_t`）：

| 偏移 | 字段 |
|---|---|
| +0x00 | base_class |
| +0x04 | constructor |
| +0x08 | destructor |
| +0x0C | event_cb |
| +0x10 | user_data |
| +0x14 | name（字符串指针） |
| +0x18 | width_def |
| +0x1C | height_def |
| +0x20 | instance_size |

基类 `lv_obj_class = 0x2CCE0308`。

| name | class VA | constructor | event_cb | instance_size |
|---|---|---|---|---|
| arc | 0x2CCE5F0C | 0x1C064069 | 0x1C064EED | 0x681 |
| bar | 0x2CCE5F30 | 0x1C065D4D | 0x1C065C25 | 0x7c0 |
| btn | 0x2CCE5F54 | 0x1C0660B5 | 0x0 | 0x344 |
| btnmatrix | 0x2CCE5F78 | 0x1C066B29 | 0x1C066C95 | 0x505 |
| calendar | 0x2CCE5FB8 | 0x1C067B19 | 0x0 | 0x1d04 |
| calendar-header-arrow | 0x2CCE6034 | 0x1C067DF1 | 0x0 | 0x0 |
| calendar-header-dropdown | 0x2CCE6058 | 0x1C067EF9 | 0x0 | 0x0 |
| chart | 0x2CCE611C | 0x1C069F39 | 0x1C06AAF1 | 0xc80 |
| checkbox | 0x2CCE6140 | 0x1C06B26D | 0x1C06B2AD | 0x3c4 |
| dropdown-list | 0x2CCE6184 | 0x1C06B8E1 | 0x1C06B9F1 | 0x380 |
| dropdown | 0x2CCE61A8 | 0x1C06B93D | 0x1C06C679 | 0x585 |
| image | 0x2CCE61EC | 0x1C06C8D9 | 0x1C06D0C5 | 0x640 |
| imagebutton | 0x2CCE6210 | 0x1C06D92D | 0x1C06DA69 | 0x1540 |
| label | 0x2CCE6880 | 0x1C06F619 | 0x1C06F1E9 | 0x700 |
| led | 0x2CCE68A4 | 0x1C06FA21 | 0x1C06FA49 | 0x380 |
| line | 0x2CCE68C8 | 0x1C06FFD5 | 0x1C06FFF5 | 0x400 |
| list | 0x2CCE6934 | 0x0 | 0x0 | 0x0 |
| menu-separator | 0x2CCE69E8 | 0x0 | 0x0 | 0x0 |
| menu-section | 0x2CCE6A0C | 0x1C070481 | 0x0 | 0x0 |
| menu-cont | 0x2CCE6A30 | 0x1C070499 | 0x0 | 0x0 |
| menu-page | 0x2CCE6A54 | 0x1C070671 | 0x0 | 0x3c0 |
| menu | 0x2CCE6A78 | 0x1C070529 | 0x0 | 0x740 |
| msgbox-backdrop | 0x2CCE6A9C | 0x0 | 0x0 | 0x340 |
| msgbox-header-button | 0x2CCE6AC0 | 0x0 | 0x0 | 0x344 |
| msgbox-footer-button | 0x2CCE6AE4 | 0x0 | 0x0 | 0x344 |
| msgbox-footer | 0x2CCE6B08 | 0x0 | 0x0 | 0x340 |
| msgbox-header | 0x2CCE6B2C | 0x0 | 0x0 | 0x340 |
| msgbox | 0x2CCE6B50 | 0x0 | 0x0 | 0x440 |
| roller | 0x2CCE6BB4 | 0x1C070DAD | 0x1C071025 | 0x485 |
| scale | 0x2CCE6BD8 | 0x1C0724D1 | 0x1C072E01 | 0x681 |
| span | 0x2CCE6C54 | 0x1C0738C5 | 0x1C0744F9 | 0x640 |
| switch | 0x2CCE6CC0 | 0x1C075EE5 | 0x1C075F0D | 0x384 |
| table | 0x2CCE6CE4 | 0x1C076C09 | 0x1C076D41 | 0x505 |
| tabview | 0x2CCE6D08 | 0x1C0773FD | 0x1C0771A1 | 0x3c0 |
| textarea | 0x2CCE6D70 | 0x1C077B85 | 0x1C078569 | 0x7c4 |
| tile | 0x2CCE6D94 | 0x1C078A8D | 0x0 | 0x380 |
| tileview | 0x2CCE6DB8 | 0x1C07898D | 0x0 | 0x380 |
| win | 0x2CCE6DDC | 0x1C078AA9 | 0x0 | 0x340 |

Total: 38 classes. Base lv_obj_class = 0x2CCE0308.


## 2. 控件构造函数

以下地址均为 **Thumb 入口（已含 bit0）**，可直接 `transmute` 调用；函数原型统一是
`Obj (*)(Obj parent)`。

| widget | create (Thumb) |
|---|---|
| arc | 0x1C0641D0 |
| bar | 0x1C065DE8 |
| btn | 0x1C0660D0 |
| btnmatrix | 0x1C066B58 |
| calendar | 0x1C067760 |
| chart | 0x1C06A224 |
| checkbox | 0x1C06B630 |
| dropdown | 0x1C06BC90 |
| image | 0x1C06CAC0 |
| imagebutton | 0x1C06DCB8 |
| label | 0x1C06E27C |
| led | 0x1C06FF1C |
| line | 0x1C07034C |
| roller | 0x1C0715A8 |
| span | 0x1C073958 |
| switch | 0x1C076070 |
| textarea | 0x1C077990 |

未列出的类（scale/table 等）在 dump 里没有找到独立的 create 函数——它们由更上层页面代码直接
调 `lv_obj_class_create_obj` 构造，或当前 dump 窗口未覆盖。

## 3. arc 的已验证细节

| 项目 | 地址 / 偏移 |
|---|---|
| 类描述 | `0x2CCE5F0C` |
| `lv_arc_create` | `0x1C0641D0`（Thumb `0x1C0641D1`） |
| value | `obj + 0x34`，i32，0..100 |
| min / max | `obj + 0x38` / `obj + 0x3C`，i32 |
| bg_start / bg_end | `obj + 0x40` / `obj + 0x42`，u16 角度 |
| indic_start / indic_end | `obj + 0x44` / `obj + 0x46`，u16 角度 |
| rotation | `obj + 0x48`，u16 |
| 强制重绘 | `0x1C02B304`（Thumb `0x1C02B305`） |
| 绘制实现 | `0x1C063EE0` 起（读 +0x34 算角度） |
| 角度 setter 候选 | `0x1C0641F0` / `0x1C064338` / `0x1C06447C` |

当前 `fw_api::arc_create` / `arc_set_pct` 就是按这张表实现的。

## 4. 通用对象函数

| 功能 | 地址 |
|---|---|
| `lv_obj_class_create_obj` | `0x1C028F80` |
| `lv_obj_class_init_obj` | `0x1C029074` |
| `lv_obj_invalidate` | `0x1C02B304` |

## 5. 调用注意事项

1. 0x1C 段是独立 XIP 代码库，调用要传 **奇数 Thumb 地址**（`|1`），否则 INVSTATE。
2. 这些是 LVGL 内部入口，升级固件后地址会变；换固件必须重新跑 `tools/lvgl_map.js` 并复核 create 表。
3. 已有 `fw_api` 封装优先用封装；本表用于封装缺失时兜底。
4. 创建后仍需 `obj_set_size_wh` / `obj_align` 等设置几何；类构造只负责对象本身和默认样式。

## 6. 重新生成

```bash
node tools/lvgl_map.js <vela_ap.bin> > docs/LVGL-MAP.md
```

（当前脚本内的数据基址 `0x2C0C0000`、基类地址 `0x2CCE0308` 按本固件写死，换固件要同步改。）
