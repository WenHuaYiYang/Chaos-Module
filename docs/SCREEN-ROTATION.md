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

- 配置 `/data/chaos/screen-rotation.cfg`，1 字节：`1`=正常、`2`=翻转。
- `toggle()` 写配置并立即 `lv_display_set_rotation(disp, 2|0)`，随后把所有已建页面的
  content 根 `obj_invalidate` 一次，避免旧方向的内容缓存和新画面叠在一起。
- `ensure_applied()` 在主页与设置页 render 时调用，只在"配置值 ≠ 已下发值"时才动 display，
  所以开机后进一次 Chaos 主页就会恢复上次方向；display 未就绪（返回 0）时留到下一次。

**不用 `transform_rotation`**：那是对象级绘制变换，只转画面不转触摸，翻过来点哪都不对。
display 级旋转由 LVGL 自己同步分辨率与 indev 坐标映射。

## 恢复

若开启后无法操作（例如驱动不支持软件旋转导致画面异常），删除配置文件即可回到正常方向：

```
rm /data/chaos/screen-rotation.cfg
```

（NSH 终端或文件桥都能做。）
