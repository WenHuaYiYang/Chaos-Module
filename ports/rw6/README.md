# Redmi Watch 6（RW6 / p65）适配线

> 独立的第二目标平台适配。**不改动 10 Pro 主线的任何文件**。

Chaos 在 10 Pro（`p67` / 3.101.043）上的形态是 `supervisor/` 里的一枚 Lua 容器模块；
RW6 是另一套平台：NuttX modlib 内核模块（`insmod` 装载）、自己的固件地址表、
自己的表盘容器格式。两条线有大量同名文件（`Cargo.toml`、`build.sh`、`chaos.lua`、
`chaos_sup.ko`、`dist/`），平铺合并必然互相覆盖。因此 RW6 全链收在 `ports/rw6/`
这一个目录里，与本仓库 10 Pro 的 `supervisor/`、`installer/`、`tools/` 完全隔离；
顶层只在 [`docs/README.md`](../../docs/README.md) 加了一条索引。

## 状态（2026-11-08）

- 14 页已落地，`PAGE_N=14` 是硬上限（加到 16 页时模块能装、时间页能用，但导航里点应用管理会把应用静默带走）。
- 出包链已打通到 **RW6 原生表盘容器**，共三种产物：
  `dist/chaos-installer-rw6.bin`（165143 B / 17 条，原生容器）、
  `dist/chaos-watchface-rw6.tar`（112640 B，内置「心情」表盘 Lua 槽位播种包）、
  `dist/chaos-p65-043-v1.tar`（102400 B，旧 USTAR + `install.sh` 路线）。
- 未接通：开机自启（`/etc/init.d` 在只读 romfs，每次重启要手动再跑 `sh /data/install.sh`）、
  字体全局生效、自定义 TTF 投递（硬阻塞）、系统模糊后端、图标包素材、摇一摇、表盘删除。
- 主 supervisor 里的 `--features fw-p65` 地址档（`fw_api/fwaddr.rs` 的 `tp65`）是**另一条并行路**，
  地址没覆盖齐之前被 `compile_error` 挡住，不要与本目录的 `work/p65_chaos` 混用。

逐条结论见 [`docs/STATUS.md`](docs/STATUS.md) 与
[`docs/Chaos_RW6_1to1复刻计划_20261031.md`](docs/Chaos_RW6_1to1复刻计划_20261031.md) 的 §10-§11。

## 目录结构

本目录内部**镜像研究工作区的相对布局**，目的是让脚本里的
`ROOT = dirname(dirname(abspath(__file__)))` 以及 `fw/`、`work/`、`docs/`、`dist/`
这些字面路径在本仓库内原样成立 —— 98 个文件逐字节复制，没有为了换仓库改一行代码。

| 路径 | 内容 |
|---|---|
| `work/p65_chaos/` | RW6 主模块：单文件 `src/lib.rs`（14 页）、`installer/chaos.lua`、`chaos_guest.py` 与冒烟脚本 |
| `work/p65_overlay/`、`work/probe_p65/`、`work/probe_clock/` | overlay、锚点执行级探针、时钟入口探针 |
| `work/rw6_desktop_measure/` | 桌面图标缩放律的逆向取证工具集 |
| `scripts/` | 12 个 RW6 活工具链脚本 + `merge_sections.ld` / `fix_ko_layout.py` 两个构建依赖，见 [`scripts/README.md`](scripts/README.md) |
| `docs/` | 三份移植/安装/复刻文档、状态快照、来源清单、p65 地址表与锚点 CSV |
| `flint-vm/profiles/` | 9 个 RW6 / p65 模拟器 profile 与板级描述 |
| `.qoder/skills/` | 3 个移植工作流 skill（锚点执行验证、加页流程、表驱动动画律） |
| `fw/`、`fw_pkg_rw6/` | 固件 OTA 包里的原始升级脚本（取证素材）；镜像本体不入库，见 [`fw/README.md`](fw/README.md) |

完整的文件来源对照见 [`docs/MANIFEST.md`](docs/MANIFEST.md)。

## 构建与出包

前提：Rust nightly + `rust-src` + `thumbv8m.main-none-eabi`（软浮点，`build-std`）、
本机 `rust-lld`。`work/*/build.sh` 按 Windows rustup 的路径找 `rust-lld`，
Linux/macOS 需自行调整该行；CI 目前只构建 10 Pro 的 `tools/build_ko.sh`，不覆盖本目录。

```bash
cd ports/rw6
bash work/p65_chaos/build.sh                  # -> work/p65_chaos/p65_chaos.ko
python scripts/build_chaos_p65_installer.py    # -> dist/chaos-p65-043-v1.tar
python scripts/build_chaos_p65_watchface.py    # -> dist/chaos-installer-rw6.bin + 播种包
python scripts/p65_pages_accept.py             # 14 页导航回归，需设备在线
```

`build.sh` 依赖的 `scripts/merge_sections.ld` 与 `scripts/fix_ko_layout.py`
已按原路径随本目录提供，与 10 Pro 主线 `tools/` 下的同名文件分开维护。
`dist/`、`*.ko`、`*.bin` 被本仓库 `.gitignore` 屏蔽，产物不入库。

## 不入库的资产

远端仓库屏蔽 `*.bin` / `*.ko` / `dist/`，固件镜像、OTA 签名与表盘容器样本也属第三方材料，
一律不随本目录发布。复现出包链前需自备：

| 路径 | 用途 |
|---|---|
| `fw/rw6_watchface_mood.bin` | RW6 原生表盘容器壳样本（72742 B，从 `vela_resource.bin` 的 `0x1FF3600` 处抠出）；`build_chaos_p65_watchface.py` 逐字节保留它的前缀 |
| `fw_pkg_rw6/vela_ap.bin` | RW6 AP 镜像，锚点取证用 |
| `fw/p65_100_043/vela_ap.bin` 等 | p65 固件解包镜像，锚点脚本用 |
| `dist/chaos-p65-043-v1.tar` | 先由 `build_chaos_p65_installer.py` 生成，再喂给原生容器脚本 |

## 安装口径

市场表盘套用要手机在线；`add_new_watchface` 解出的 market 目录重启后连节点一起被清。
因此实际投递走**内置槽位播种**：把三个文件放进
`/data/app/watchface/builtin/120917365540/_lua/mood/`（全机唯一「可写 + 会执行 Lua」的位置），
切到「心情」表盘点运行即安装。安装器 Lua 不做 `rmmod`，每个动作占一个独立 timer 拍。
详见 [`docs/Chaos_RW6安装与使用_20261030.md`](docs/Chaos_RW6安装与使用_20261030.md)；
注意该文档 §2 / §4 的状态表部分已过时，时间/日期页在 10-31 已落地。

## 关键口径

- **地址**：RW6 的运行地址 = 文件偏移 + `0x1BFD023C`（PSRAM），与 10P 的
  `文件偏移 + 0x0C0C0000|1` 不是一回事，跨档串用会让模块静默不跑。
- **容器**：头 `0x28` 是 12 字节 pkgName；`0xA477` 起是
  `[u24 数据长][u8 名字长][16 字节 0][名字][数据]` 文件链，逐条串到 EOF，无中心表。
- **两道固件门**与 **14 页上限**见上文；页表与原生控件锚点见 `docs/*p65*.csv`。

## 来源与许可

本目录源码沿用本仓库 [`LICENSE`](../../LICENSE)（AGPL-3.0）。
`docs/STATUS.md` 是研究工作区交接文档 §7 的已提交版本快照。
固件镜像、OTA 签名、表盘容器样本不在本仓库，权利归各自权利人。

README 与状态快照写于 2026-11-08；`docs/` 内保留原始文档的日期与结论，
不做回填修改，避免把研究过程抹平成"一直如此"。
