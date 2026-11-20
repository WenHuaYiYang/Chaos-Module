# 屏幕翻转（180°）

设置 → **屏幕翻转** 开关。开启后整屏反向显示（180°，非镜像），触摸坐标跟着转。

## 固件取证

miwear 的 display 属性表在别名 VA `0x2CDC17A0`，键名与处理器成对出现：

```
get_layer_top  get_layer_sys  set_bg_color  set_bg_image  set_bg_opa
get_chroma_key_color  get_next  set_rotation  get_res  get_default  get_scr_act ...
```

| 键 | 处理器 | 内部 thunk | 目标（真实函数） |
|---|---|---|---|
| `get_default` | `0x0CAA7C74` | `0x0CAC08A0` | **`0x1C02F868`** = `lv_display_get_default` |
| `set_rotation` | `0x0CAA75B0` | `0x0CAC0D68` | **`0x1C03046C`** = `lv_display_set_rotation` |

`set_rotation` 处理器的调用形态就是 `(disp, rotation)`，且把系统配置的角度
（90/180/270）映射成 rotation 1/2/3 —— 与 LVGL 的枚举一致：

```
0 = 0°    1 = 90°    2 = 180°    3 = 270°
```

thunk 本身在 flash（`ldr.w pc,[pc,#-0]` + 目标字），可以直接 `bl`，且保留固件的重定位。

## 实现

`supervisor/src/screen_rotation.rs`：

- 翻转状态仅保存在模块本次运行的内存中，不读写配置文件；旧版
  `/data/chaos/screen-rotation.cfg` 即使存在也会被忽略。
- `toggle()` 立即调用 `lv_display_set_rotation(disp, 2|0)`，随后把所有已建页面的
  content 根 `obj_invalidate` 一次，避免旧方向的内容缓存和新画面叠在一起。
- 每次重新注入从正常方向开始，在注入后的 UI 维护定时器中应用；display 未就绪时延后。
  主页、设置页与触控点仍可补应用本次运行方向，但不会恢复上一次注入的翻转状态。

**不用 `transform_rotation`**：那是对象级绘制变换，只转画面不转触摸，翻过来点哪都不对。
display 级旋转由 LVGL 自己同步分辨率与 indev 坐标映射。

## 恢复

关闭“屏幕翻转”即可恢复正常方向；重启并重新注入模块也会以正常方向开始。
不再需要通过删除配置文件恢复。
