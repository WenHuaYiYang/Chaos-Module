# 通知空消息图标 ABI 核对

仅适用于小米手环 10 Pro **p67 / 3.101.043**，不采用 S441、O63 或其他固件的地址。
核对日期：2026-11-04。本文记录静态镜像事实，不等于真机验收。

复现：在项目根运行 `python3 tools/verify_notification_icon_abi.py`。脚本只读，
使用 Python 标准库，复用 `verify_keyboard_abi.py` 的 Thumb BL 和启动搬运解析；
不运行构建、不写设备、不更改固件或生产代码。缺镜像、错误版本或错误资源直接失败。

AP：`research/firmware-3.101.043/043_ABI/executables/raw_arm_thumb/vela_ap.bin`，
SHA-256 `519307675665e4866d722a8119a98589c397b614ac3294cb87bfc86de45756ec`。
下列小地址均是文件偏移，不能把启动搬运副本直接当 flash 执行地址使用。

## 图标与图像源

原资源为 `/resource/app/notifications/no_message.bin`，不是 `swipe_none.bin`。
解包文件 SHA-256 为 `5c7a938ab677293e148412b27355c2efe6465553927933d9cde23801cafa0949`。
文件大小 10252 字节，12 字节头为 `19 0a 00 00 60 00 60 00 60 00 00 00`：
magic=25、format=10、96×96、stride=96，随后是 256×4 字节调色板和 9216 字节索引。
唯一纯黑调色板项是 index 0，原 alpha=255，被 5036 个像素引用。旧版
`background_assets::empty_image()` 只把该项的第 4 字节 alpha 清零，保留其他颜色。

固件 `decode_indexed` 在 `0xA295A..0xA2978` 取 format 并减 7；
`0xC24F70/0xC24F80` 两张表分别给出 2/4/16/256 个颜色和 8/16/64/1024 字节。
`0xA29EE` 的分派表将 format=10 送至 `0xA2A7E` 的 8-bit 分支；
`0xA2A10..0xA2A26` 读取索引后，以 index×4 读取完整 palette word 并写入
format=16 输出图，包含 alpha 字节，不把透明度丢弃。

| 项目 | p67 / .043 证据 |
|---|---|
| 图像类 | `img_create` 的 flash thunk `0x4C8D58` → `0x1C06CAC1`，搬运前本体 `0xB9F00`；本体的 `0xB9F1C` 字面量为 `0x2CCE61EC`。类首项为父类 `0x2CCE0308`。 |
| 图源 +0x34 | `img_set_src` thunk `0x4C7FA0` → `0x1C06CEB5`，本体 `0xBA2F4`；`0xBA438` 读取 `[obj+0x34]`，`0xBA416` 写变量图像描述符。 |
| 原路径所有权 | `0xBA402..0xBA416` 遇到旧文件/符号源时先经过 `0xBA4B2` 释放原字符串，再写 descriptor。绝不能保存旧 heap 路径指针用于恢复；应重新传静态 `EMPTY_PATH`，由 setter 复制。 |
| 恢复可能失败 | `0xBA450` / `0xBA4D0` 调 `0xADA78` strdup；分配返回 0 时不替换当前源。setter 是 void，恢复后必须读回确认，不能仅凭调用已返回就清除自有替换台账。 |

## 事件布局与返回值

`lv_event_t`：`+0=current_target`、`+4=original target`、`+8=u16 code`
（低 15 位有效）、`+12=user_data`。原 getter 本体依次为
`0x49BB0`、`0x49BC0`、`0x49BD0`、`0x49BF4`。
`0x49508` 初始将同一对象写到前两项；`0x494B2..0x494CE` 冒泡时把父对象写回
`+0`，因此不能把这两个名称反过来。拒绝冒泡事件时比较两项相等即可。

DELETE=0x24：`0x7C00C..0x7C012` 在销毁对象前发送；
STYLE_CHANGED=0x2D：`refresh_style` 的 `0x47100/0x47160` 发送。
两者最终调用同一 `lv_event_send` 本体 `0x494FC`。样式变化不是对象死亡证明，
不能按 DELETE 处理或清除恢复台账。事件回调只登记/清理逻辑状态，不分配图像或修改样式。

`obj_add_event` 的 flash thunk `0x4C83F0` → SRAM `0x002BF7AD`，本体 `0x49618`，
调用 `0x49A90` 并返回新建的 12 字节事件描述符：
`+0=callback`、`+4=user_data`、`+8=filter`。返回值是指针，不是事件回调返回值。
用户 callback 在 `0x49A24` 以一个 event 参数调用，随后立即重读 event flags，
不使用 callback 的 R0；Rust `unsafe extern "C" fn(u32)` 的 void 签名符合该调用链。

**内存不足限制**：`0x49AA2` 分配 12 字节后，`0x49AA8` 直接写 `[r0]`，没有 NULL 分支。
所以本模块检查 `obj_add_event(...) == 0` 是有用的防御，但不能据此宣称固件在 OOM
下一定安全返回。host mock 的“注册失败”用例不覆盖原生分配器崩溃或断言。

## 通知聚合页存活边界

固件通知页数组 `0xC61638/0xC6163C` 分别存放 RAM 描述符
`0x200D1134`（详情）和 `0x200D10C0`（聚合）。对应 initializer 位于
`0xD0D7AC`、`0xD0D738`，名字为 `page_detail` 和 `page_aggregation`。

| 字段 | 聚合页 | 详情页（不属于此图标 hook 的扫描范围） |
|---|---|---|
| `+0x14` 复合键 | `0x00140001` | `0x00140000` |
| `+0x30` 当前 root | 初始 0，由页面框架设置/清理 | 同左 |
| `+0x4C` on_create | `0x0C525AFD` | `0x0C52613D` |
| `+0x64` on_destroy | `0x0C525B19` | `0x0C5260A9` |

通用创建流程 `0x9AAE64` 在 `0x9AAECC` 把 root 写进 `desc+0x30`；
`0x9AAF60..0x9AAF68` 调用 create(desc, root, start)，回调之后
`0x9AAF90` 把 `desc+0x28` 状态字节设为 5。聚合页 create 本体
`0x465AFC` 也把传入 root 存到它自己的全局 `0x20110E88`。

通用销毁流程 `0x9AAB80` 先调用 `desc+0x64`，再于 `0x9AABEE` 调用原生
对象删除，`0x9AABFE` 才将 `desc+0x30` 清零，`0x9AAC12` 将状态设为 4。
`desc+0x24 != 0` 的延迟销毁路径则会**先**把状态设为 4，root 暂不清零；
后续 `0x9AB52C` 删除对象，`0x9AB538` 清 root。

因此，在调用方保证 UI owner、非事件派发栈、原生 idle 的前提下，可以每拍从
已核对的 home 通知根和聚合页 descriptor 重新发现晚建/重建对象，不必改写 create 槽。
聚合页最低检查包含复合键匹配、root 非零且在可写指针范围、状态 `+0x28 != 4`。
这仍不是任意线程上的存活证明：DELETE 派发期间 descriptor 的 root 尚未清零；
指针范围检查本身也不能防止 UAF。root/image 的 DELETE 回调与代际 token 台账仍然必要。

本核对未证明 `+0x2A` 必须取某值用于通用聚合页判断，不能直接照搬 home 特有的
状态门；也未证明所有异常第三方删除路径都会维护 descriptor。真机仍需验证
晚建、重建、开关/区域关闭恢复、息屏唤醒和内存压力，不把静态签名/主机测试当作设备通过。
