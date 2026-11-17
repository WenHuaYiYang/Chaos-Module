# 灵动点系统电量与返回表盘接口（p67 / 3.101.043）

本轮修正真机反馈的两个问题：电量一直显示“暂不可用”；“回到表盘”只回到上一级。圆点、扇形、菜单配置和既有其他功能保持原有行为。

## 精确固件身份

- 全量包：`miwear.watch.p67tc_v3.101.043_full_a4ce8564.bin`。
- 全量包SHA-256：`a4ce8564778624f8287ea91015867dc1725b649fbc10f2c4fa4b6813af7bc4d9`。
- AP：`vela_ap.bin`，13795728字节。
- AP SHA-256：`519307675665e4866d722a8119a98589c397b614ac3294cb87bfc86de45756ec`，与项目043符号清单一致。
- 获取来源：[固件归档](https://hrk666666.github.io/firmware-archives-public/)，对应[原厂全量包镜像](https://github.com/hrk666666/firmware-archives-public/releases/download/firmware-miwear.watch.p67tc-v3.101.043-feeb89d7/miwear.watch.p67tc_v3.101.043_full_a4ce8564.bin)。源码包不附带原厂固件。

下表函数均为静态核对并执行原始ARM指令验证，尚未在实体设备上验证本次修改。

## 电量

上一版按通用NuttX设备路径探测，扫描`/dev/charger/`、`/dev/battery/`，遗漏固件实际使用的`/dev/charge/`。本版移除设备探测，改用系统已经维护的电量缓存。

| 项目 | 精确证据 |
| --- | --- |
| getter可调用入口 | `0x0CA6E6A9`，无参数，返回32位电量值 |
| 代码入口 | `0x0CA6E6A8` |
| 实际读取 | 从字面量`0x2013EF10`的`+0xB0`读取32位值，即`0x2013EFC0` |
| 系统调用点 | `0x0C52FF22`调用该函数，随后用于原生UI电量判断与日志 |
| 驱动路径旁证 | AP包含`/dev/charge/soft_gauge`及`/dev/charge/batt_gauge`；新实现无需打开它们 |

`fw_api/battery.rs`只调用该getter，并验证0～100范围。101、负值（无符号表示为高位值）及其他异常返回None。控制器仍每5秒同步，不在动画帧里访问文件或设备。

## 返回表盘

原来的`page_goto(16 << 16, 0)`是普通页面导航，不能承担系统归主页及主页子面板复位的职责。

固件自身的“timeout screen return to home”回调在`0x0C624B30`：

1. `0x0C624B46`准备参数0；`0x0C624B4C`调用`0x0C50FD7C`。
2. `0x0C624B50`加载`0x200EB6E0`，再次置参数0。
3. `0x0C624B54`从接口的`+0x10`取函数；`0x0C624B56`调用该函数。

本版`fw_api/navigation.rs`执行同一顺序：`HOME_PREPARE_WATCHFACE(0)`，然后调用`[0x200EB6F0](0)`。调用前检查请求入口有Thumb位并落在项目既有可信指针范围；服务未就绪时显示短暂失败提示，不继续调用。

`0x0C50FD7C`在屏幕ON时进入`0x0C50B2C8`，对仍存在且需要移动的四个主页面板复位：

| 记录 | 原生目标 |
| --- | --- |
| `0x20110168` | y = 480 |
| `0x20110154` | y = 0 |
| `0x20110140` | x = 336 |
| `0x2011012C` | x = 0 |

参数0沿用系统原生归主页行为。控制器以既有的`top_app_id()==16`和`watchface_is_on()!=0`同时成立作为完成条件；若原生页面先退一级，维护UI拍会以600ms间隔有界继续同一归主页请求，最多8秒，不调用`page_back`。已经在表盘时不重复请求。用户打开更深的新页面、触碰圆点、关闭功能或息屏时取消未完成请求；页面栈过渡时暂为0不会被误判为用户新导航。上述入口只在既有维护UI定时器里执行，事件回调仍只登记动作。

## 可复现验证

源代码与ARM测试适配的地址绑定检查不依赖原厂固件：

```bash
python3 tools/test_floating_ball.py
python3 tools/verify_floating_ball_services.py
```

使用上方精确AP可执行静态身份检查和机器码探针；后者需要项目ARM Rust工具链、Unicorn与pyelftools：

```bash
python3 tools/verify_floating_ball_services.py /path/to/vela_ap.bin
python3 tools/probe_floating_ball_services.py /path/to/vela_ap.bin
python3 tools/build.py --firmware 10p-3.101.043
```

探针编译真实生产`battery.rs`、`navigation.rs`为ARM，并加载原厂AP：

- 执行真实电量getter，验证0～100全部值，拒绝101、255和负值。
- 比较生产归主页适配和原厂归主页回调的调用顺序及参数。
- 验证空、偶数及非法服务地址不会触发主页修改或请求。
- 执行真实主页面板复位代码，验证四个面板的目标轴、坐标及参数0。

图形底层、通知/日志和最终应用分派是明确的替身；探针不能验证真实应用栈切换、实体触摸、显示帧率或电量随充放电刷新。仍需在对应手环确认本次修复的最终效果。
