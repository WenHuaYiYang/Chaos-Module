# 精确固件 symbol 与构建入口

每个目标独立存放在 `symbol/<机型-完整系统版本>/`，例如：

```text
symbol/10p-3.101.043/
symbol/10p-3.101.036/
```

当前状态：

| 目标 | 核验地址导出 | 完整 Chaos 构建 |
|---|---:|---|
| 10p-3.101.043 | 128（含 6 个 Chaos 启动绑定） | 支持现有 .043 基线 |
| 10p-3.101.036 | 63 | 尚不支持，缺少完整私有 ABI 适配 |

“核验”保留每条记录原有的验证等级，不代表所有入口都经过 Chaos 真机测试。
这些地址可被 C/Rust/链接器直接引用；完整应用能否运行还取决于结构布局、线程、
hook 协议和所有子模块，不能仅用导出数量判定。

## 快速提取

在项目根目录运行：

```bash
python3 tools/firmware_symbols.py extract \
  '/Users/ikun_cxkpro/Projects/固件修改/10p/3.101.043/miwear.watch.p67tc_v3.101.043_full_a4ce8564.bin' \
  --target 10p-3.101.043
```

工具仅使用 Python 标准库。直接读取 ZIP 里的 `vela_ap.bin`，校验 ZIP CRC、完整输入
SHA-256、精确 AP SHA-256 和长度，不解压大型图片分区，不执行 ota.sh。
也支持输入原始 AP。需要单独保存 AP 时加 `--unpack /path/to/work-dir`；镜像不应提交。

036 的局部核验导出与参考搜索：

```bash
python3 tools/firmware_symbols.py extract \
  '/Users/ikun_cxkpro/Projects/固件修改/10p/3.101.036/miwear.watch.p67tc_v3.101.036_full_17a1ea48.bin' \
  --target 10p-3.101.036 --exports-only \
  --reference-image '/Users/ikun_cxkpro/Projects/固件修改/10p/3.101.043/miwear.watch.p67tc_v3.101.043_full_a4ce8564.bin' \
  --reference-target 10p-3.101.043
```

本机实际执行上述两条命令约 0.09 秒 / 0.33 秒；第二条得到 26 个唯一 64-byte
参考搜索结果。时间只表示提取、资料核验和搜索速度，不表示已经完成新固件适配。

默认提取结束会检查完整 Chaos 构建资格；缺少 ABI 时返回非零退出码。
`--exports-only` 明确允许仅导出已有的局部核验记录。这时输出 JSON 中
`full_chaos_build_ready` 仍为 false，编译入口也不会因此放行。

## 文件用途

| 文件 | 用途 |
|---|---|
| target.json | 机型/版本、AP 摘要、CPU/浮点 ABI、启动绑定、完整构建阻塞项 |
| reviewed.json | 已核验记录及其独立来源、原型、原有验证等级；维护输入 |
| symbols.json | 规范化的核验地址导出 |
| firmware.rs | Rust 常量、启动绑定、嵌入模块的目标标识 |
| firmware.h | 与 Rust 同一来源的 C 地址宏 |
| firmware.ld | 可选择使用的绝对链接符号定义 |
| manifest.json | 输入身份及所有编译输入的 SHA-256 |
| candidates.json | 待核对的匹配搜索结果，不含 callable 导出，不参与编译 |
| REPORT.md | 当前输出范围及完整构建状态 |

示例：C 使用 `CHAOS_SYM_OPEN` 这类宏，Rust 使用 `SYM_OPEN` 这类常量；具体名字见
生成文件。函数值保留 Thumb bit，全局数据不会被错误地统一加 1。
采用 `firmware.ld` 时只选择一套固件，且需单独验证加载器的绝对符号处理；当前生产
构建使用生成常量，不引入新的 modlib 符号解析需求。

## 构建

```bash
python3 tools/build.py --firmware 10p-3.101.043
```

产物：

```text
dist/10p-3.101.043/chaos-installer-10p-3.101.043.bin
dist/10p-3.101.043/chaos_sup.ko
dist/10p-3.101.043/fetch-module.ko
dist/10p-3.101.043/capatch.elf
dist/10p-3.101.043/build-report.json
```

不传 `--firmware` 默认选择 10p-3.101.043。安装包文件名和显示名称均标注系统版本。
Lua 安装/加载流程保持原样，容器内部仍使用原来的模块文件名。

Rust 构造/注册链实际从生成绑定取值；Cargo 跟踪资料与生成器变动，C 也导入同一份
头文件。Rust、模糊/Connect C、network、fetch、capatch 的缓存按目标分开。
全部功能和图片照常构建，不通过删除模块减少适配要求。

`--skip-ko` / `--skip-fetch` 只能复用目标目录下已有的同目标、同 symbol 摘要、同内容
产物；检查记录在 `artifact-target.json`。其他固件或改动过的旧模块不能混包。

直接编译脚本也执行相同检查：

```bash
CHAOS_FIRMWARE=10p-3.101.043 sh tools/build_ko.sh
CHAOS_FIRMWARE=10p-3.101.043 python3 chaos-fetch/build.py
CHAOS_FIRMWARE=10p-3.101.043 python3 chaos-fetch/capatch/build_capatch.py
```

## 新固件的边界与资料补充

这两份 AP 没有可直接恢复完整函数名称/原型的编译器符号表。当前工具从精确镜像确认
身份，并生成已有核验资料；参考搜索用于缩小新入口的核对范围。未知 AP 不会继承
参考固件地址。64-byte 唯一匹配也可能包含邻接代码/字面量池，故不会被自动晋级。

可以在提取时加 `--reviewed-dir /path/to/symbol-json-directory` 导入已有核验资料。
支持现有 Canopus symbol JSON 的 name/kind/prototype/callable_address/provenance 等
字段，只导入精确 AP 摘要一致且已 APPROVED 的地址记录，保留原有证据等级。
PENDING/REJECTED/FORBIDDEN/WITHDRAWN 不进入编译绑定；已有记录发生冲突则报错。
项目内已保存核验资料快照，因此正常运行不依赖外部 Canopus 工程或本机绝对路径。

新增完整构建目标还需完成私有结构、字体、四区模糊、launcher、PAN、ChaosConnect、
fetch/capatch 等依赖闭包，并迁移其尚未查表的调用点。本次没有把 .036 标成可运行的
完整 Chaos，也没有提供强制跳过检查的构建开关。

## 验证

```bash
python3 tools/test_firmware_symbols.py
python3 tools/test_build.py
python3 tools/test_corona_blur.py
python3 tools/test_blur_v2.py
```

已测试错误固件拒绝、输入不被覆盖、生成物篡改拒绝、局部 symbol 与候选隔离、
APPROVED 记录导入、同名冲突保护、重复字节匹配拒绝及跨目标缓存拒绝。
两套生成文件还分别经 C 头文件编译、绝对符号链接、Rust include 编译；这些是主机
构建验证，不是固件加载、函数调用或完整 .036 真机运行验证。
