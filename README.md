# Chaos — 小米手环 10 Pro 自研平台

一个内核模块加一个 Lua 安装器，借固件自己的注册链把**原生应用**装进手环桌面：
图标出现在 launcher 里，点进去是固件原生的页面与转场动画。全程不联网、不依赖手机、
不改动固件分区。

- 目标设备：小米手环 10 Pro（p67）
- 目标固件：**3.101.043**。036 时代的地址与产物已全部失效，别拿这套代码去跑 036。
- 原生应用“系统美化”中可设置“重启恢复字体和图标”，默认开启。更新后重新选择一次素材即可记录；关闭后下次运行保留系统样式，当前外观不撤销。
- 启用自动注入后，重启注册完成即启动后台恢复队列，首次亮屏后推进，无需打开 Chaos。
- 安装器“运行”在模块已激活时仍可更新磁盘上的 sup.ko，核对成功后重启生效，无需抢在自动注入之前运行。
- 产物：`chaos-installer-10p-043-v1.bin`（170,258 字节）—— 模块、安装器、应用图标打进一个容器。

这份 README 面向项目内部（接手的人、下一个 agent），所以里面直接写主工程路径与完整坑清单。
对外发布的是 `github.com/WenHuaYiYang/Chaos-Module` 那份，203 行，差异是明确的：

- 对外那份第一人称，不写内部历史、内部路径、日期与轮次
- 对外那份只留 8 条最要紧的坑（下面这份是全集），不含"仓库不放哪些素材"那类清点，
  也不含构建产物：`chaos_sup.ko`、`chaos_mod.ko`、`chaos_icon.bin` 都从远程跟踪里拿掉了，
  `.gitignore` 挡 `*.ko *.bin *.a *.o *.elf`。本地这三个文件照旧留着，是工作产物。

技术事实（页面清单、容器格式、投递包规则、地址映射）两边必须一致，改一处要同步另一处。

## 这个目录里有什么

```
Chaos-Module/
├── supervisor/            内核模块 chaos_sup（Rust, no_std，8,524 行）
│   ├── chaos_sup.ko       构建产物（当前 84,972 B）
│   └── src/
│       ├── lib.rs         crate 根：导出与胶水
│       ├── state.rs       全部状态与常量（页数、页名键、描述符缓冲）
│       ├── page.rs        14 个页面的实现与 PAGE_TABLE
│       ├── ui.rs          页面框架：建页 / 行 / 事件分发 / 生命周期
│       ├── fw_api.rs      已验证固件 API 封装层（新界面一律走这里）
│       │   └── gfx.rs / page.rs / fs.rs / sys.rs / event.rs
│       ├── ipc.rs         安装协议：设备文件 IPC 与注册链
│       ├── explorer.rs    文件管理器（目录遍历 / 查看 / strings 提取）
│       ├── watchface.rs   表盘列表、选择、摇一摇轮换
│       ├── font_apply.rs  免重启换字体：样式表分拍写回
│       ├── font_list.rs   页5 字体清单与自由切换
│       ├── font_tree.rs   逐对象补写（一次性任务，不是后台常驻）
│       ├── icon_apply.rs  页6 桌面图标：读清单、跑批换图标、恢复
│       ├── confirm_pop.rs 页5/页6 共用的删除确认框状态机
│       ├── text.rs        文本缓冲与格式化
│       └── mem.rs         st_rd! / st_wr! 与裸地址读写
├── module/                chaos_mod：最小示例模块，只验"加载链 + exec 链"通不通
├── installer/
│   ├── chaos_installer.lua   主安装器（11 步，见下）
│   ├── font_pack.lua         字体投递包的安装侧
│   ├── icon_pack.lua         图标投递包的安装侧
│   └── font_moved.txt        容器第 4 槽的占位文件（字体已移出主包，槽不能少）
├── fonts/                字体源文件 + 霞鹜文楷的 OFL.txt —— **另三份未核授权，发布前要剔除**
│                         见"许可与合规"
└── chaos_icon.bin        应用图标（112x112 ARGB8888，由 scripts/gen_chaos_icon.py 生成）
```

## 功能

注册 **14 个固件页**（这是真机定案的硬上限，见"硬约束"）。页名键是 `page_goto` 的解析用名，
与界面显示无关；显示标题走的是"进入时点的那一行"的名字。

| page_id | 页名键 | 界面 | 干什么 |
|---|---|---|---|
| 0 | `main` | Chaos | 存储 / 内存 / CPU 实时读数，四个入口 |
| 1-4 | `files` `files1` `files2` `files3` | 文件管理 | 目录 1 到 4 级，条目 + 更多 + 返回 |
| 5 | `textsub` | 系统美化 · 更换字体 | 已投递字体的复选框清单、切换、两步删除 |
| 6 | `iconsub` | 系统美化 · 桌面图标 | 图标包列表、应用、恢复系统原图标 |
| 7 | `viewer` | 文件查看 | 文本分段读；二进制按 strings 提取 |
| 8 | `wfaces` | 表盘切换 | 列出内置 + 市场表盘，点选即切 |
| 9 | `wfshake` | 摇一摇轮换 | 勾选哪些表盘参与摇一摇轮换 |
| 10 | `wfmgmt` | 表盘管理 | 摇一摇开关 + 进 8 / 9 两个二级页 |
| 11 | `bright` | 亮度控制 | 当前亮度、自动亮度开关 |
| 12 | `cache` | 缓存清理 | 统计并清理可清理项 |
| 13 | `fontd` | 系统美化（菜单） | 字体、图标、系统背景、表盘通透背景、固定图片背景及状态 |

主界面的行分配：0 存储（带进度条）、1 内存（带进度条）、2 CPU、3 文件管理、
4 表盘管理、5 亮度控制、6 系统美化。展示顺序把 CPU 放在最前，但句柄槽位不变。

## 通透背景

在“系统美化”中打开“持续复用静态背景”，点击“生成或更新表盘背景”，返回表盘后生成一次轻度模糊背景，供桌面列表、小部件、控制中心及消息中心共同使用。后续唤醒、切换页面和重新打开开关都复用该图片，只有手动更新才重新生成。关闭开关恢复系统背景。默认使用系统背景，重启后需要重新生成；素材或接口不匹配时保留原画面。

截图复制后立即归还系统缓冲，模糊与原尺寸展开借用已有 UI 调度分批完成，息屏停止处理。动态创建的消息和健康卡片由有限批次补处理，电池保留原蒙版轮廓与电量行为。固定图片模式读取 `/data/chaos/background.bin`，素材为 84 x 120 RGB888；目前只有 PC 转换工具，没有固定图片投递入口。普通用户可先使用表盘模式。

当前通过原固件离线像素、转场、资源所有权、生产菜单和定时器组合验证，尚未完成真机验收。

覆盖安装新主包后需重启设备一次，才能加载更新的模块。

## 构建

```bash
# 1. 编译内核模块 -> Chaos-Module/supervisor/chaos_sup.ko
powershell -ExecutionPolicy Bypass -File scripts/build_chaos.ps1

# 2. 打包容器 -> chaos-installer-10p-043-v1.bin
python scripts/build_chaos_installer_043.py

# 3. 字体投递包（吃主包当模板，所以必须在第 2 步之后）
python scripts/build_font_pack.py [--src <你的.ttf>] [--name <12字节内短名>] [--pkg <12位数字>]
python scripts/test_font_pack_lua.py        # 离线冒烟，当前 229 项

# 4. 图标投递包（同样吃主包当模板）
python scripts/build_icon_pack.py --src <素材目录>
python scripts/test_icon_pack_lua.py        # 离线冒烟，当前 53 项

# 单独校验 ko（未定义符号必须为 0）
python scripts/verify_chaos_ko.py Chaos-Module/supervisor/chaos_sup.ko
```

注意：编译失败时第 2 步**不会报错**，会拿旧 ko 重新打包。看到 bin 体积没变就要当心。

重要：第 2 步在公开仓库里跑不通，原因**不是**缺模板 —— 壳与缩略图块都已自产（见"容器格式"
一节）。跑不通是因为打包链本身（`scripts/build_chaos_installer_043.py` 等）不对外：它写死了
本地工程路径。对外给的是格式实现 `tools/container_shell.py`，项目侧那点 glue 由使用者自己接。

当然，你可以直接使用`tools/build.py`

### 编译链

```
lib.rs + 各模块
   |  cargo +nightly build --release --target thumbv8m.main-none-eabi
   |    -Z build-std=core,compiler_builtins
   |    -C target-cpu=cortex-m33 -C target-feature=-fpregs
   v  （软浮点、禁浮点寄存器，匹配手环 SoC）
libchaos_sup.a
   |  rust-lld -flavor gnu -r --gc-sections
   |           -u module_main -u chaos_ctor -T merge_sections.ld
   v
chaos_sup.ko（relocatable ELF）
   |  python scripts/fix_ko_layout.py     重写节区 sh_addr 为连续布局
   |  python scripts/verify_chaos_ko.py   未定义符号必须为 0
   v
chaos_sup.ko（可被 Vela 模块加载器识别）
```

| 组件 | 说明 |
|---|---|
| Rust nightly | 需要 `-Z build-std` |
| `rust-src` 组件 | `rustup component add rust-src` |
| `thumbv8m.main-none-eabi` | Cortex-M33，软浮点 |
| `rust-lld` | nightly 自带 |
| Python 3 + Pillow | 后处理、打包、素材转换、离线冒烟 |
| lupa（Lua 桥） | 只跑离线冒烟用的 lvgl mock，非必需依赖 |

关键参数：`-r` 产出可重定位 ELF；`--gc-sections` 控体积；`-u module_main -u chaos_ctor`
强制保留入口；`-T merge_sections.ld` 合并 `.text.*` —— `rust-lld -r` 不会自动合并，
漏了它 `.text` 大小为 0，模块不加载代码。

### 容器格式与安装步骤

安装容器 magic `0x1234A55A`：

```
[0x000 头部] [0x028 包名 12B] [0x068 显示名 64B] [0x0A8 主题表]
[0x148 记录表] [预览块 12B 头 + 数据] [文件区]
每个文件 = [u32(内容长度 | 路径长度 << 24)] + [16B 零] + [路径] + [内容]
```

记录表每条 16 字节 `(0x05000000|槽号, 0, 偏移, 长度)`，尾标记 `(0x05000000, 0, 0, 0)`。
**槽数可以增减**：条数直接决定记录表结束地址（`0x20` 与 `0xAC` 存的就是它）、首记录的第三个
字段、以及 `0xD8` 的文件条数，全部能派生（别人的容器实测有 10 条）。主包第 4 槽那个占位文件
（`installer/font_moved.txt`）现在留下的唯一理由是让重建产物跟真机验证过的那份逐字节相同；
要减槽得先真机复核安装链。整份壳由 `scripts/container_shell.py` 自建，不吃外部模板；
判据是"解析 → 重建 → 逐字节相同"跑过 5 个来源不同的容器（两个外部作者的容器是产物、
不入库，缺失时脚本打印跳过）。缩略图块（预览块）同样自产：`scripts/gen_preview.py` 按固件
解压函数 `0x0CA91EC4` 的格式自己编码，PC 与安卓 App 两端各一份实现，见
`../docs/Chaos_预览块自产_20261001.md`。

| 源 | 容器内路径 |
|---|---|
| `installer/chaos_installer.lua` | `_lua/chaos-installer/main.lua` |
| `supervisor/chaos_sup.ko` | `_lua/chaos-installer/chaos_sup.ko` |
| `chaos_icon.bin` | `_lua/chaos-installer/chaos_icon.bin` |
| `installer/font_moved.txt` | `_lua/chaos-installer/lxgw.ttf`（占位） |

安装器步骤，每步占一个 `lv_timer` 拍（让固件事件循环转一圈再走下一步）：

```
1 部署模块 -> 2 部署应用图标 -> 2.5 部署字体 -> 3 加载模块(insmod) -> 4 检查设备
-> 4.5 设置语言 -> 5 恢复占位 -> 6 注册应用 -> 6.5 通知系统 -> 7 发布应用 -> 8 发布桌面条目
```

第 3 步在检测到旧模块还活着时会主动失败，提示先关机再开机 —— 这是设计，不是 bug。

### 投递包

字体与图标**不在主包里**（2026-09-26 起移出），各自一个 `.bin`，与主包同一个容器壳、
不同的一组槽。包号是表盘容器的身份键（`0x28` 处 12 字节 `pkgName`）：主包固定，
字体包与图标包的号由内容哈希推导，所以同一份内容幂等、不同内容自动分开、多包可并存。

- 设备上落地：图标**按包存** `/data/chaos/icons/<包号>/<stem>.bin`；字体**按槽存**
  `/data/chaos/font/st<槽号>.ttf`，可投递槽位 1 到 8（槽 0 是安装器自带那份，不可投递）
- 清单：`/data/chaos/icons/index.txt` 与 `/data/chaos/font/index.txt`，
  两份**逐字节同一套格式** —— 定长 128 字节（8 行 × 16 字节），行是
  `[2 位包号或槽号][空格][12 字节短名][换行]`，空槽是 16 个空格。
  代码两侧各写一遍（图标那条链已在真机跑通，不抽公共模块），**改一处要全跟着改**
- 设备上**刻意不遍历目录**（真机踩过 procfs 遍历死锁），清单是唯一来源
- 图标素材规格：画布 112×112，内容上限 100×100，四边至少 6px 透明留白 —— 量固件自带
  `more.bin` 得到的，留白是桌面拿去做磁贴间距的，少了会看着比系统图标大一圈

## 硬约束（改代码前必读）

这几条每一条都是真机踩出来的，违反的后果是黑屏重启或功能静默失效。

- **固件调用一律走 `src/fw_api.rs`**，业务代码里不许出现裸 `transmute(0x0C...)`。
- **函数指针地址必须 bit0 为 1**（Thumb 态）。Cortex-M33 没有 ARM 态，偶数地址一调就
  INVSTATE 硬 fault。逆向工具给的是偶数 entry，当指针用之前必须 `|1`。
- **空串指针不等于 NULL**。副标签传 `b"\0".as_ptr()` 会触发惰性创建，界面凭空多一行空白。
- **碰任何 LVGL 对象前先过存活门 `page_is_live(pid)`**。定时器比页面活得久，缺这道门
  在退出应用时必崩。句柄作废放在 `on_create`，`on_destroy` 不碰状态 —— 息屏也会走
  `on_destroy` 但对象树还在，在那里清句柄会让实时刷新永久停摆。
- **常驻 `lv_timer` 不许按固定频率跑**。三条规则一起成立：屏幕门（`fw_api::screen_is_on()`）、
  值不变不写（影子比对）、按忙闲调频（忙 50ms，息屏全空闲 1000ms）。改周期用
  `timer_set_period` 且**写后必读回**，对不上就永久退回原频率。
  违反的直接后果是装完后续航雪崩。
- **`lv_timer` 只建一次**，句柄建立后永不丢弃。清句柄不等于释放对象（定时器挂在内部链表上），
  "疑似失效就重建"会让旧实例继续跑并逐轮累积。
- **不要在 UI 线程 read 设备节点**。`/dev` 下字符/块设备的驱动 `read()` 会阻塞到看门狗复位；
  只有 `DT_REG` 才允许 open/read。目录用 `opendir`/`readdir`/`closedir`，内核 open 不是 POSIX
  （只读标志是 1，不是 0）。
- **槽号到回调禁止 catch-all**。`match` 里写 `_ => ev9` 会让新增槽位静默错配。
- **容量常量一旦改，所有清理循环要同步**，否则留悬空句柄，后果是 use-after-free。
- **原生应用最多注册 14 页**（真机定案：把 `PAGE_COUNT` 抬到 15、第 15 页借用现成页、
  不含任何新代码，安装到第 4.5 步就黑屏自重启）。要加界面就重新分配已有 page_id。
  二级界面必须有独立 page_id —— 只有真 `page_goto`/`page_back` 才有固件的压栈转场动画。
  页名在 `state.rs`、注册在 `ipc.rs`、页表在 `page.rs`，**三处必须同步**。
- **不要在表盘 Lua 回调里做需要内核调度锁的操作**（`os.execute`、procfs 目录遍历必死锁）。
- 固件映射：运行地址 `R` 对应文件偏移 `F` 的关系是 `R = F + 0x0C0C0000`；字符串常量还要
  搜 0x2C 别名域（偏移 `+0x2C0C0000`），xref 两个域都得扫。

## 验证

本项目**没有针对固件行为的单元测试** —— 一切最终判据是真机读数。能离线做的先离线做：

- 构建通过 + `verify_chaos_ko.py` 未定义符号为 0
- 字体投递链离线冒烟 229 项、图标投递链 53 项（lupa 加 lvgl mock，把"表盘 Lua 语法或
  逻辑错"这类在真机上直接表现为黑屏的问题先钉死）
- 真机验证前判据必须**可分辨**：不同失败模式对应不同读数
- 证据等级从高到低：`DEVICE_PROVEN` > `DEVICE_PROBED` > `STATIC_CONFIRMED` > `STATIC_RECOVERED`。
  **编译通过不等于设备可用**
- 探针读数一律用二维码或界面显示，不写文件（设备上读不了文件）；探针达到目的后
  下一次改动时顺手删掉

## 配套安卓 App

`../android/` 是手机侧的制作工具：选字体做子集化与度量归一化、选图片转图标、
在本地生成与 PC 脚本**逐字节等价**的投递包，再传到手环。投递包的缩略图（预览块）也在
App 内按同一套格式生成，与 PC 侧同口径但各自渲染像素（不比字节）。当前 1.0.0，
包名 `com.chaos.bandpack`，自签发布。它内嵌主包壳与固件解出的系统原图标，
所以那个 APK 是自用件，不是外发件。

## 许可与合规

### 代码

本目录的源码（Rust 模块、Lua 安装器、`../scripts/` 的 Python）的许可见
`Chaos-Module/LICENSE`。

历史与现状：早期版本（前身 self_canopus 时代）的 supervisor 部分逻辑与容器壳
**曾经**派生自第三方项目 canopus（AGPL-3.0），因此整包一直按 AGPL-3.0 分发。
2026-10-01 的派生审计（`../docs/Chaos_派生审计_20261001.md`）逐文件核对了全部
派生点并已清理完毕：

- supervisor 的协议骨架表达（ipc.rs/lib.rs）已重写，设备行为字节不变；
- 安装器三份 Lua 中与 canopus 逐字对应的段落已全部重写表达；
- 容器壳由 `../scripts/container_shell.py` 从零生成（多独立样本往返逐字节验证），
  预览缩略图由 `../scripts/gen_preview.py` 自产（压缩格式从固件解码器逆向定案）；
- 固件地址、结构偏移、协议字节格式是**逆向得到的事实**，不受版权保护，
  与 canopus 的重合属于事实重合，不构成派生。

清理后本目录代码为本项目自有版权，许可条款以 `LICENSE` 为准。
审计口径说明：审计基于 2026-10-01 工作区快照，之后新增的代码必须保持自写。

### 不许随源码分发的东西

以下是**版权与授权问题**，与开源许可无关，发布仓库前必须剔除。仓库根 `.gitignore`
已经挡住了 `*.bin` 与 `*.apk`，但下面这些要手工确认：

| 路径 | 为什么不能发 |
|---|---|
| `fw_pkg_043/`、`ota_extract*/`、`*.bin` 固件镜像 | 小米的固件与资源包，无任何授权 |
| `sys_icons/` | 从固件资源包解出的系统原图标，同上 |
| `canopus_src/` | 第三方项目（AGPL-3.0），已无派生关系但也不该镜像进发布仓库 |
| `Chaos-Module/fonts/HYWenHei-85W*.ttf`、`SanJiHuaChaoTi-*.ttf`、`YSHaoShenTi*.ttf` | **仓库里没有它们的授权文本，也没核到上游条款**。商业字库对"分发字体文件"通常是有专门授权的，别按网传免费商用办（这条是待办，不是结论） |
| `assets/delta_lvgl/` | Delta-Icons 是 CC BY-NC-ND 4.0；我们裁过尺寸就已构成改作，ND 这一条过不了 |
| `assets/pure_lvgl/`、`assets/_src_pure/` | PureIconPack 的 APK 内没有任何授权声明，默认保留所有权利 |
| `android/keystore/`、`android/keystore.properties` | 签名私钥与口令，任何时候都不进仓库 |

`assets/fluent_lvgl/` 是 GPL-3.0，**可以**随分发走，但要同时附许可证文本与上游出处，
且微信、亚马逊这类品牌 logo 的商标权独立于主题许可证。

### 字体（可分发，但要带东西）

`Chaos-Module/fonts/lxgw-wenkai-band.ttf` 是霞鹜文楷的子集化改作，`fonts/OFL.txt` 是上游
许可文本逐字落盘（5,171 字节，含保留字体名声明与 ADDITIONAL PERMISSION 那段）。OFL 允许
自由再分发与改作，条件有两个：随附许可文本，且改作物不得沿用保留字体名。子集化与垂直度量
归一化都属改作 —— 所以 `scripts/build_font_subset.py` 把家族名换成 `ChaosKai`（含 nameID 10
说明这是改作物），脚本自带校验（改作名字段里保留字体名必须 0 处），署名与许可字段原样不动。

仍欠一条：投递包（`.bin`）里只有 `ttf`，没把 `OFL.txt` 一起打进去。装到自己的设备上不涉及
再分发，但要把包发给别人，就得同时给许可文本。

### 图标（安卓 App 内）

App 界面图标来自 MingCute（Apache-2.0）。这条已经做对了：APK 内带
`assets/licenses/mingcute-LICENSE.txt` 与 `mingcute-NOTICE.txt`，NOTICE 里写明了改动内容
（只取子集、去掉不渲染的水印路径、填充色归一）。

### 隐私

模块只在设备本地读 `/proc` 与固件状态，把结果显示在手环屏幕上：不联网、不上传、
不采集也不落盘任何用户数据。设备本身有蓝牙，但本模块不使用它 —— 蓝牙数据传输那条线
做过探索，最终整体删除（`state.rs:37` 还留着一行过时注释提到它，代码里没有通路）。

### 免责声明

这套代码通过固件的注册链把原生应用装进你的手环，属于对设备的非授权修改：
可能失去保修、可能变砖、可能随固件升级失效。请在自己的设备上、自担风险地使用。
不要把它刷进任何你不拥有的设备。

### 发布前检查清单（还没做完）

本节描述的是**目标状态**，不是当前仓库状态。四项里两项已结清：

1. 已做：`Chaos-Module/LICENSE` 放入 AGPL-3.0 全文，GNU 官方文本逐字未改
   （34,523 字节，sha256 前缀 `0d96a4ff`）。主工程根目录仍没有 LICENSE，那是另一码事
   —— 只有本目录这套代码已经明确挂上许可。
2. 已做：容器壳自建 + 缩略图块自产（`scripts/container_shell.py`、`scripts/gen_preview.py`），
   对外发布的是前者那份 `tools/container_shell.py`。打包链本身仍不对外（写死本地路径），
   所以使用者拿到源码得自己接这一步 glue。
3. 未做：`Chaos-Module/fonts/OFL.txt` 与安卓 App 的 `assets/licenses/` 那套第三方声明
   合并成一份 `THIRD_PARTY_NOTICES.md`，逐条列许可、上游出处、我们改了什么。
4. 未做（但影响已被绕开）：按上面那张表把不可分发的素材从发布分支剔掉。对外仓库现在只放
   设备侧源码加 `tools/`，`fonts/`、`assets/`、固件解包与第三方素材都没进去 ——
   等于用"不放"代替了"逐项办授权"。真要把素材也发出去才需要办。

## 相关文档

- `../AGENTS.md` — 项目操作手册（命令、约定、边界、陷阱清单）
- `../docs/README.md` — 文档索引
- `../docs/Chaos_043平台适配与能力探索_20260905.md` — 平台适配与历次迭代
- `../docs/Chaos_字体投递_20260926.md` — 投递包线（字体与图标）的定案与落地
- `../docs/Chaos_预览块自产_20261001.md` — 容器缩略图的格式定案与两端编码器
- `../docs/Chaos_Android制作App_20260930.md` — 手机侧工具
- `../docs/Chaos_图标包pure_20261001.md` — 从第三方图标包 APK 提取素材的做法与授权清点
- `../docs/fw_address_table_043.csv` — 固件地址表（改 `fw_api.rs` 必须同步这张表）
- `../docs/icon_reverse_work/图标格式与最终方案定案.md` — 图标格式逆向定案

## 术语

- **ko**：可重定位 ELF 内核模块，被固件的模块加载器 insmod
- **supervisor**：本项目的常驻模块，持有设备节点 `/dev/chaos`
- **容器 / 投递包**：magic `0x1234A55A` 的 `.bin`，主包与字体包、图标包同壳不同槽
- **page_id**：固件在 `on_create` 时传给页面的页号，0 到 13
- **CIPK**：图标包容器（magic 四个字节 `CIPK`），装 N 条 `[stem].bin`
- **app_id**：注册后固件分配的应用号，运行时取白名单空槽，**不是常数**

## 开机自动注入

10 Pro 3.101.043 支持通过独立的自启动管理器加载 Chaos。在安装器中打开“自启动”，点“安装”，然后在管理器的“模块管理”中重建自启动并开启总开关。首次设置后需重启，避免当前加载的旧模块影响验证。

模块注册脚本为 `/data/rc.d/chaos.sh`，启动帧为 `/data/chaos/boot.bin`。注册操作由 UI 定时器逐拍完成；管理器负责启动 hook、安全窗口与总开关。未安装管理器时会明确报错。关闭和启动顺序在管理器中设置；删除 Chaos 的自启动注册后，需要回到管理器重建。

自启动派发与安装器视图参考 [An2em6o/MIBAND10PRO-AUTORUN](https://github.com/An2em6o/MIBAND10PRO-AUTORUN) 的 Chaos 派生代码，参考提交为 `00a1c060e4e674e4c55c86610d3d4e6c99e91bcc`，该模块目录沿用 AGPL-3.0。合入时保留当前安装流水线，并修正入队错误返回、重入保护和空闲周期处理。

Lua 自注入接入另对照该提交的 `Watchface/module.autorun.chaos/_Lua/chaos.lua`，生成的启动脚本与启动帧逐字节一致。目录按单路径逐个创建，并以文件写入和回读判断可用性；重复注册不写盘。按注册契约迁移旧版时，只摘除 `/data/rc` 中属于 Chaos 的旧二跳行，其他字节保留；正常注册不改管理器的产物、策略或总闸。

本地已完成构建和离线验证，重启自动注入尚待本项目真机确认；9 Pro 未接入此机制。