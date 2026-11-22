# Android 手环端 Chaos 设置

适配目标由模块现有 symbol/ABI 表选择，本次构建为 10 Pro / 3.101.043。
Android“功能列表 → 手环端Chaos设置”从手环读取分组目录，支持显示与交互、桌面美化、
系统模糊与全部 V2/V3 参数、日志级别、截图录屏、快应用动画、触控点、灵动点、系统关于修改、
字体与图标、表盘、应用管理、日期时间、缓存清理及网络服务。
文件管理和 NSH 继续使用已有专用入口与独立权限。恢复 fetch 是说明页，不包含运行时开关。

ChaosConnect 自身的启用、配对、连接确认及权限设置不在目录中，手机不能通过此接口修改。
“设备控制”权限位为 `0x10`，默认关闭；界面不再显示“预留”。插件不通告本设置能力，
即使 HELLO 伪造 `CCA1` 也不能使用 Android 设置指令，身份由实际承载来源确定。

## CCN1 扩展

HELLO capability `0x40` 表示支持设置目录，`0x80` 表示支持批量目录与关联值回执。以下操作均要求 Android 已获连接许可、
ChaosConnect 已启用且设备控制权限仍开启。所有整数为小端；字符串为 `u16字节数 + UTF-8`。

| opcode | 请求 | 正常响应 |
| --- | --- | --- |
| `0x70` SETTINGS_LIST | `offset:u16`，新客户端可追加 `limit:u8=1..32` | `token:u32` |
| `0x71` SETTINGS_WRITE | `id:u16, length:u16, value:utf8` | `token:u32` |
| `0x72` SETTINGS_RESULT | `token:u32` | `done:u8, status:u8, body` |
| `0x73` SETTINGS_APPLY | 同 WRITE，仅支持 `0x80` 的模块 | `token:u32` |

提交成功只表示排入 UI owner 邮箱。RESULT 的 `done=0,status=0` 表示尚未执行；
`done=1` 后检查内部 status，只有 0 才表示该操作成功。忙返回 CCN1 status 8，可稍后重试。
断连、停用或撤权会使待执行请求过期，旧 token 不得用于新会话。结果读取无副作用，
可重用状态请求 ID；写请求 ID 仍不能循环复用。

LIST 结果 body：`version:u8=1, total:u16, next:u16, count:u8`，旧请求每页最多 8 项；显式 limit 可提高到 32 项，仍按 2304 字节
载荷预算截断条目数量。每项：

```
id:u16, kind:u8, min:u32, max:u32,
group:string, label:string, currentValue:string, options:string
```

kind 为 0布尔、1整数、2单选、3文本、4动作、5只读。单选 options 使用 `|` 分隔，
整数和选择值使用十进制文本；动作值必须为空。WRITE 成功 body：`id:u16, actualValue:string`。
输入限制由手环白名单和参数范围校验，最大 240 字节，不接受 NUL、非法 UTF-8 或任意内存地址。

目录固定项定义于 `supervisor/src/control_catalog.rs`。应用项 `1000..1063`、表盘项
`2000..2023` 对应最近一次从 offset=0 获取的目录快照，内部保存完整包名／表盘 ID，
不会因系统重新排序而操作另一个应用。新会话必须先重新读取目录。
字体／图标选项按真实清单显示槽位名称；空槽不能成功应用。

## 执行与生效时机

协议线程只提交和取结果，固件读写、LVGL 及应用操作由 UI owner 执行，启动延迟结束后才受理。
权限在入队和执行前分别检查。锁为 try-lock，没有自旋等待；仅目录同步期间短暂保持快节拍。
手机不再在每次修改后重新读取整份目录。APPLY 在完成操作的同一 UI tick 回读该项及相关分组的
实际状态，通过 RESULT 一次返回；通常省去十几页以上的目录提交／结果轮询、全部名称和选项的重复传输，
也不再为一次开关操作重新枚举应用、表盘和所有字体清单。手机只合并这些回执，不提前假定操作成功。
旧模块仍使用 WRITE 的实际值回执，手动“刷新手环设置”可更新其他关联项。首次载入支持逐页显示进度。

APPLY 成功 body：`version:u8=1, incomplete:u8, count:u16`，随后 count 个补丁：
`id:u16, flags:u8`，按 flags 顺序读取可选字段：bit0=value:string、bit1=label:string、
bit2=options:string；bit3 表示移除该行。options 仍以 `|` 分隔。请求项始终包含实际 value；
应用隐藏／显示／卸载会同步更新相应列表行，表盘轮换会更新参与状态。snapshot ID 不重新编号，
卸载的应用项被标记无效，避免后续旧行操作其他应用。极端情况下元数据超出载荷预算时设置
incomplete=1，主操作和已返回的实际值仍有效，界面提示可手动刷新余下列表。
旧 WRITE 的响应格式和 8 项分页均保留，旧 Android 端不受新格式影响。

原生功能原有的延后生效规则保留：字体切换在退出手环 Chaos 前台后应用；图标切换／删除按
原有逐拍队列完成；截图和录屏由原有采集任务处理。屏幕翻转仅本次运行有效；熄屏秒数、
关闭遮盖熄屏等仍使用模块自己的持久化设置。删除和动作需要手机确认。

构建与相关验证：`python3 tools/build.py`、`python3 tools/test_connect_control.py`。
C 测试验证实际分发器的授权／撤权／客户端来源与会话过期，不模拟固件执行。
