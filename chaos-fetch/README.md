# tun 模块

`bt-net` 网卡 + DNS 拦截。裸机 freestanding（无 libc、零未定义符号）。

## 构建

需要 **clang + ld.lld**。脚本依次查找显式工具链环境变量、Android NDK、PATH 和 Homebrew LLVM；可用 `CLANG` / `LLD` / `NM` / `READELF` 覆盖：

```bash
python build.py
# 产物 build/tun-3.101.043.bin

python build.py --output ../dist/fetch-module.ko
# 验证通过后额外复制到 Chaos 的构建目录
```

构建脚本自带的检查：

- 零未定义导入（`nm -u` 无输出）
- 段布局 / 重定位归一化（R_ARM_TARGET1 → ABS32）
- 装载体积 < 65536 的闸门
- 打印 `.text/.rodata/.data` 尺寸

## 关键约束

| 约束 | 原因 |
|---|---|
| **不能调用任何 VFS**（open/stat/read/write 对文件）| 会重入 VFS 直接死锁，见 `docs/PITFALLS.md#1` |
| 命令只能从「读节点 / 周期服务」里执行 | `mt_cmd_poll()` 的调用点是这两个 |
| 改代码必须同步 `src/mpu_trap.h` 的 `MT_BUILD` | 表盘靠它判断内存里是不是这一版 |

## 布防阶梯

`1` 双向量+MEMFAULTENA → `2` MPU+预检 → `3` 布防解析器窗口 → `4` 替换路径验证。
**第 3 步才是 DNS 生效点**，四步顺序不能乱，每步之后要读一次节点。

## 调试开关

- `echo 98 > /dev/tunentry` → `/dev/tunsvc` 输出完整诊断页
- `echo 97 > /dev/tunentry` → 回到一行
