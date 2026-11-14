# 系统日志级别设置

设置 → 日志级别设置，八个等级独立多选。切换立即生效，保存至 `/data/chaos/log-level.cfg`，格式为 `LOG1`、掩码字节、掩码反码字节。先写临时文件，再重命名替换；保存失败显示“保存失败，仅本次生效”。不停止 offline_log、不删除日志。

Chaos 注入后的维护回调自动读取并恢复一次，无须进入设置页；没有有效配置时保留系统当前等级。包括0在内的全部掩码均可恢复。重启后需要 Chaos 再次注入（或已配置自启动）才会恢复。维护回调不重复写配置、不持续覆盖其他工具的设置。

仅选 EMERG 等价于 `setlogmask r`；选择 EMERG 至 WARNING 等价于 `setlogmask w`。命令使用累计掩码，本页支持任意位组合，包括全部关闭（0）。降低记录量不修复已损坏的存储，也不能保证停止所有独立日志通道。

## p67 / Vela 3.101.043 固件证据

镜像：`research/firmware-3.101.043/043_ABI/executables/raw_arm_thumb/vela_ap.bin`，代码映射基址 `0x0C0C0000`，只读数据别名基址 `0x2C0C0000`。

- 命令表中 `setlogmask` 指向 Thumb 入口 `0x0C6EE841`。
- 帮助函数 `0x0C6EE7C0` 列出 `d/i/n/w/e/c/a/r`，分别对应 DEBUG/INFO/NOTICE/WARNING/ERROR/CRITICAL/ALERT/EMERG。
- 命令函数 `0x0C6EE840` 为上述选项分别传入 `0xFF/0x7F/0x3F/0x1F/0x0F/0x07/0x03/0x01`。
- setter `0x0C1F0CE0` 从字节地址 `0x200C8958` 读取旧掩码，写入参数低字节，返回旧值。无任务范围限制、无持久化写入，0 也会直接写入。
- 本页使用同一 setter，仅异或所选位，保留其他等级状态。

新增入口后设置页共有14个行槽；将统一 `MAX_ROWS_PER_PAGE` 从12扩至16，页面状态数组及固件接口边界均引用同一常量。
