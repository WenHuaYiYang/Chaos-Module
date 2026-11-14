# 上次充电时间（p67 / 3.101.043）

系统使用持久属性 `persist.charge_timestamp` 保存充电时间，关联属性还有 `persist.charge_state` 和 `persist.charge_has_timestamp`。不是 Chaos 自行记录的插线时间。

固件证据（`research/firmware-3.101.043/043_ABI/executables/raw_arm_thumb/vela_ap.bin`）：

- 字符串文件偏移 `0xAC696C`，只读映射地址 `0x2CB8696C`。
- 充电事件处理 `0x0C5D8834` 在 `0x0C5D887E` 调用 `time()`（`0x0C1F1664`），从栈中取秒数，在 `0x0C5D8886` 调用整数属性 setter `0x0C3DAF80` 写入该属性。
- `time()` 调用 `clock_gettime(CLOCK_REALTIME)`，返回64位 Unix 秒数；充电属性写入其中低32位。
- 读取使用同固件整数属性 getter `0x0C3DAFE0`，默认0。此处只读，不触发充电、不修改属性。
- 格式化复用固件 `localtime_r`（`0x0C1F49AC`），完整分配44字节 `struct tm`；当前时间通过 `clock_gettime` 读取。

主页在内存占用下显示 `YYYY-MM-DD HH:mm（x天前）`，天数按经过的完整24小时计算；不足一天显示0天前。无记录显示“暂无记录”，未来时间显示“系统时间异常”。进入主页时读取一次，亮屏主页每分钟刷新，熄屏不刷新。

固件另有 `persist.charger.charger_on_time`，供充电管理器时序使用，本功能不使用它。
