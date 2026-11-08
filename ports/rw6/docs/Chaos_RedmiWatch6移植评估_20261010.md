# Chaos 移植可行性评估：Redmi Watch 6（p65 / 3.100.043）

> 2026-10-10。输入：`C:\Users\YiYang\Downloads\miwear.watch.p65_v3.100.043_full_662aa7db.bin`。
> 结论先行：**可行，且属于"新 target 适配工程"级别的工作（与 canopus 给 Band 11 做过的那次同级，可能略重）**；
> 不是"改几个地址就能跑"，也不是"要重写一套"。证据与差距见下。

## 0. 固件档案

| 项 | Redmi Watch 6 (p65) | 小米手环 10 Pro (p67) 对照 |
|---|---|---|
| 包名 | `miwear.watch.p65_v3.100.043_full_662aa7db.bin`（120,039,968B，zip） | `871466e63fabcb5adda09ca4fe4a4b55_upd_miwear.watch.p67cn.bin`（90,342,166B） |
| 包内条目 | ota.sh / vela_ap.bin(11,793,540) / vela_audio.bin / vela_ota.bin / vela_resource.bin(218MB) / vela_sensor.bin + META-INF | ota.sh / vela_ap.bin(13,616,208) / vela_bl2.bin / vela_ota.bin / vela_resource.bin(174MB) + META-INF |
| 芯片 | **best1502** | best1503 |
| 版本 | 3.100.043 | 3.101.043（043）/ 3.101.036（036） |
| AP 镜像 SHA-256 | `a19b601477569765103eaf468b937bd8026b4d68ffa5c896bde222a79e475858` | 043: `51930767...` / 036: `662d67f5...` |
| 解包位置 | `fw/p65_100_043/` | `fw_pkg_043/`、`fw_pkg_871466/` |

ota.sh 机制逐条相同：`setprop ota.progress.*` + `dd if=/ota/<file> of=/dev/<part> bs=32768 verify` + 失败 `reboot`。
p65 多 audio/sensor 两个分区、无 bl2（bl2 在别的包或内嵌）。

## 1. Chaos 的依赖链（移植要凑齐什么）

Chaos = 内核模块（`chaos_sup.ko`，NuttX modlib ELF ET_REL）+ Lua 安装器（伪装成"表盘"容器经自定义表盘渠道进设备）。
端到端链：

1. **投放**：安装器容器（表盘格式）经手机端自定义表盘渠道写入设备
2. **执行**：固件表盘引擎（Lua 5.4）运行容器内脚本
3. **模块加载**：Lua `os.execute("insmod /data/chaos/sup.ko chaos_sup")` → NuttX modlib 装载 .ko
4. **原生应用注册**：ko 调 `app_install` + `app_launcher_add` 把图标与页面注册进 launcher
5. **UI 栈**：lv_/lvx_ 组件（行、标题、label、msgbox）、页面导航（page_goto/page_finish）、事件系统
6. **系统服务**：VFS、表盘管理器（枚举/切换）、eventbus 手势、字体样式

## 2. 逐条证据（静态）

### 2.1 平台同族性（全部命中的）

| 依赖 | p65 状态 | 证据（本镜像内） |
|---|---|---|
| NuttX + **modlib 装载器** | 在 | `module/mod_insmod.c`、`module/mod_rmmod.c`、`module/mod_modhandle.c`、`modlib/modlib_load.c`、`modlib_depend.c`、`modlib_registry.c`、`modlib_symtab.c` 字符串全在；nsh 的 `insmod <file> <name>` 命令在 |
| **Lua 5.4 运行时** | 在 | `package.path` 标准串与 043 **逐字节相同**；`luaopen_activity/dataman/topic/miwear/screen/navigator/vibrator` 七个库入口与 043 **一一对应** |
| **表盘子系统** | 在 | `/data/app/watchface/{builtin,market,builtin/aod}/` 目录常量与 043 相同；表盘传输协议日志串 278 条与 043 逐字共有（"Respond with watchface list." 等）；`watchface_manager_set_watchface_by_id`、`refresh_cur_watchface`、`create_watchface_list_json` 等名字全在 |
| **launcher/应用体系** | 在 | `app_launcher_hidden|app_launcher_update|app_launcher_del|load_app_info|app_launcher_add|reset_order_info_list` 名字块与 043 同次序同内容；`app_install`、`page_goto` 语义名在 |
| **LVGL v9** | 在 | `lvgl/lvgl/src/core/lv_obj_class.c` 等 `__FILE__` 路径串与 043 完全一致（v9 的 lv_display/lv_draw_buf 模块在） |
| **lvx_ UI 组件库** | 在 | 135 个 lvx_* 名字，与 043 共有 116 个；差异为产品组件（如 p65 多 `lvx_page_title_battery_create`，043 多 `lvx_crown_vibration`） |
| **eventbus / 手势** | 在 | `quick_guesture` 7 处、`wrist_tilt` 订阅日志、`MiWearScreen` tag 全在 |
| **屏幕状态/系统管理** | 在 | `MiWearScreen` + `failed to subscribe wrist_tilt` 与 043 同模式 |

### 2.2 地址映射（同一条自证）

`R = F + 0x0C0C0000` 对 p65 成立（镜像自证：file 0x10 的 `ldr r0,[pc,#0x50]; bx r0` 字面量 = 0x0C0C0015，指向自身）。
crt0 前 32 字节与 043 **逐字节同构**（`14 48 00 47` + bl + `msr MSP/MSPLIM` + `CONTROL=0` + `isb`），只有 bl 的相对偏移不同。

### 2.3 名字池（符号名资产）与定位手段现状

- p65 有自己的"函数名池"（file 约 0x100000..0x3FFFFF 混排区内）：**18,551 个名字**（043 19,251），交集 12,280。
  名字块结构两边同构（同样的"编译单元内函数名连续"组织），但**名字池顺序 ≠ 地址顺序**（用 p67 侧
  `watchface_app` 组实测：名字序 take_snapshot→resume→pause→reset→…→refresh→update_slot→edit_request，
  地址序 resume(0x5F2C28)→edit_request(0x5F2EB0)→refresh(0x5F32B8)→reset(0x5F3418)→…，不重合）。
- 关键差异：**043 的名字在自己的函数体内被引用（名字→函数自证关系，`find_fn_by_name.py` 靠这个）；
  p65 的名字几乎不被任何代码/数据引用（抽样 300 个仅 2 个命中，且复核后是加密区数据里的巧合）**。
  Ghidra 独立复核一致：`watchface_manager_set_watchface_by_id` 名字只有 1 处命中（字符串本体），零引用。
- 更大的口径差异：**p65 的"静态字符串引用"整体极少**。全镜像 ldr-literal 指向 0x2C 域（flash 别名，
  043 的字符串直读通道）只有 **261** 处，而 043 有 **122,439** 处；p65 的代码大量引用 0x38/0x3C
  域（PSRAM）地址（约 14.6 万，含常量混入）。合理解释：p65 把只读数据/字符串安排到运行时 PSRAM
  副本上访问，静态镜像里看不到对字符串的"字面量指针"。
- **推论（对工作量的直接影响）**：p65 上不能靠"名字自引"或"字符串 xref"这类单点捷径定位函数，
  定位主力必须是**跨固件结构/调用图/数据流匹配**（canopus fw-match 方法论）+ 手工逆向。
  好消息：Ghidra 全量分析已跑通（见 §6），恢复了完整的函数与引用网络，可作为匹配的底料。

### 2.4 段布局与内存模型差异（主要挑战）

| 项 | p65 | 043 |
|---|---|---|
| 代码区 | file 0x0-0x17FFFF 小段 + 0x400000-0xB3F484 主段，中间 0x180000-0x3FFFFF 是名字池/只读数据/表混排 | file 0-约0xA00000 连续代码，0xA00000+ 尾段名字池 |
| ldr-literal 目标域 | **0x38(PSRAM) 14.6 万次**、0x0C 1.6 万、0x2C 261 | **0x2C(flash别名) 12.2 万次**、0x20(SRAM) 4.2 万、0x38 仅 3267 |
| 全镜像指向 2C 域的字 | 7,013 | 117,818 |

两固件把"代码访问数据"放进了**不同的地址空间**（043 大量 flash/cache 别名直读；p65 大量 PSRAM）。这对跨固件字节级匹配是主要干扰源。

### 2.5 代码同源度（32B 窗口、步进 4、任意位置匹配）

| 组合 | 逐字节相同 | 掩掉重定位字后 |
|---|---|---|
| 036 → 043（同设备相邻版本，当年成功迁移） | **55.30%** | 63.30% |
| 043 → p65（跨设备） | 8.41% | 11.67% |
| p65 → 043 | 9.84% | 13.63% |

p65 与 p67 的"逐字节相同代码"约为 036↔043 的 1/5。含义：大量函数需要**重新定位**（不能靠固定平移或简单签名），必须走 canopus 那套"多算法证据融合"的 target 适配流程（`canopus_src/docs/firmware-symbol-porting-plan.md`）。

### 2.6 先例：跨设备适配已做过

- `canopus_src/targets/` 已有 `xiaomi-band-11-4.100.108`（123 符号，全部 STATIC_RECOVERED / PENDING，由 fw-match ensemble 从 band10-036 迁移）与 band9/9pro 的 static candidate pack。
- 即：**"给新设备建一个 target pack"在这套工具链里有完整方法论与先例**；p65 是同类工作，挑战在于同源度比 band10→band11 可能更低，以及 p65 没有 IDA corpus 提取环境（fw-match 的 extract_corpus 依赖 IDA/idapro；本机无 IDA）。

## 3. 差距清单（要做的实际工作）

1. **符号适配（主体）**：~120-145 个固件符号（app_registry / launcher / modlib / nuttx / ui / activity / watchface 等，见 043 target pack）在 p65 上重新定位 + ABI 验证。
   - 定位手段：字符串 xref（日志格式串在 p65 可用，已验证有引用形态）、结构/调用图匹配、对拍既有产物。
   - 需要先把 canopus 的 corpus 提取从 IDA 换成 Ghidra（或找到 IDA 环境）。
2. **UI 常量与布局**：屏幕分辨率/尺寸与 10 Pro 不同（具体值待测），Chaos 页面的 content/行布局常量要按新屏调。
3. **安装器容器**：确认 p65 的表盘容器格式（0x28 pkgName 等 12 字段）与现有打包脚本的兼容性；必要时给 build_chaos_installer 加目标参数。
4. **真机 gate**：按 canopus 的 T0-T6 分阶段（identity → 模块注册 → app_lookup → app_install → 页面 → label/事件 → launcher_add）逐级验证。

## 4. 风险与不确定

- **SoC 不同（best1502 vs 1503）**：应用层机制已证同族；若个别服务（显示、存储、传感器）在 SoC 层换实现，相关符号的逆向量更大。暂未发现阻断项。
- **无官方 pack 与符号池引用机制缺失**：定位效率低于 043 时代，工作量上浮。
- **Launcher/注册链的 ABI 细节**（app/page 描述符布局、count 语义）需要在 p65 上重新核对，不能照抄 10 Pro。
- 音频/传感器分区是手表新增，与 Chaos 主线无关（不需要动）。

## 5. 建议路线（按投入产出排序）

1. **先做"锚点定位"验证**：用 Ghidra（TMode 已修，见下）+ 字符串 xref 把 5-6 个关键函数在 p65 上定出来（app_install、launcher_add、insmod/modlib、lvx 行/标题、page_goto），验证"定位链路"能在一个可接受工时内跑通。这一步决定整体估时。
2. 跑通后再决定：是否先复刻 canopus 的 `lvgl_v9 backend` 单页 demo（比 Chaos 全量轻）。
3. 全量 target pack 适配 → 按 T0-T6 gate 上真机。

## 6. 本轮工具产出

- `flint-vm/profiles/firmware-watch6-p65-100043.json`：p65 固件 profile（ghidra_fw.py / r2_fw.py 已可读，`list` 能看到）。
- `flint-vm/tools/ghidra/SetupThumb.java`：**修复 raw Thumb 固件在 Ghidra 里"一个函数都认不出"的根因**——
  BinaryLoader 导入时不带指令集状态，分析器按 ARM（TMode=0）解码全灭。脚本流程：清 code units →
  置 `TMode=1` → 反汇编入口 → 重跑全量分析。**修复后 p65 工程的函数与引用网络完整可用**
  （`fn 0x10` 能返回向量表槽函数 + 一串 `FUN_xxxxxxxx` 引用者，据此可做反编译/交叉引用/调用图查询）。
  043/o66 工程将来重建时同样要用它。
  （注意两件事：①现有 ghidra 工程的地址空间是 **file offset**，不是运行地址；查询时用 `运行地址 - 0x0C0C0000`。
  ②`ghidra_fw.py` 的 `sym` 通配查询在本版本 Ghidra 上返回 0（FUN_ 名字查不到），函数浏览走 `fn`/`block`。）
- `fw/p65_100_043/`（本地留档，bin 已在 `.gitignore` 挡掉）：解包产物，`ota.sh`/`META-INF` 入库。

## 7. 附带发现（对后续 flint 模拟器适配有用）

- **SRAM 比 10 Pro 大 256KB**：crt0 初始 MSP = `0x2019FFE0`（043 是 `0x2015FEC0`），MSPLIM = `0x2019F3E0`。
  p65 的 SRAM 顶应为 `0x201A0000`（10 Pro 是 `0x2015FFF0`+）。作 profile/RAM 区间时要按这个算。
- **crt0 的 .data 搬运段异常小**（copy 4 字节、clear 0 字节；043 侧是 147KB+350KB）——待核，
  可能是 p65 把数据初始化挪给了别处的加载器（与"数据放 PSRAM 运行"同源）。
- crt0 的搬运源 `0x2CBFF308` 按 `2C0C0000+F` 公式落在镜像外——**p65 的 2C 域映射公式待核**
  （不影响本评估结论，影响的是将来归一化解析）。

## 8. 下一步（若继续做）

1. **锚点定位验证（第一优先）**：用修好的 Ghidra 工程把 5-6 个关键函数在 p65 上定出来
   （app_install、launcher_add、modlib.insmod、lvx 行/标题工厂、page_goto），验证定位链路的实际工效。
2. 打通"Ghidra → corpus"导出，替代 fw-match 依赖 IDA 的 extract_corpus.py。
3. 按 canopus target 流程建 p65 target pack（先 candidate 后 ABI 验证）。
4. UI 参数（分辨率、行高等）实测后给 build 脚本加设备维度。
