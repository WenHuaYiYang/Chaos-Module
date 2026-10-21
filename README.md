<div align="center">

# Chaos

**小米手环 10 Pro 的原生应用平台**

一个内核模块加一个 Lua 安装器，借固件自己的注册链把原生页面装进手环桌面。

[简体中文](README.md) | [English](README.en.md)

[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![Firmware](https://img.shields.io/badge/firmware-3.101.043-green.svg)](#兼容性)
[![Device](https://img.shields.io/badge/device-p67-orange.svg)](#兼容性)
[![Platform](https://img.shields.io/badge/platform-Vela-lightgrey.svg)](#兼容性)

</div>

---

## 简介

Chaos 把一组用 Rust 编写的原生页面注册进小米手环 10 Pro 的固件，让它们在桌面以普通应用的形式出现：图标、点击、转场都由固件自己的 launcher 完成。模块不联网、不依赖手机，也不改动固件分区。

它最初是为了在 10 Pro 上补研究工具链而写的，后来逐渐长成一套完整的设备管理界面：应用管理、表盘、字体、图标、亮度、缓存、模糊与震动设置。

> **规模**：约 1.4 万行 Rust，17 个固件页，产出的 `chaos_sup.ko` 约 175 KB。

## 特性

- **应用管理**：隐藏/显示、一键批量、排除项；普通/深度隐藏两种存储模式（见 [应用管理说明](docs/APP-MANAGER.md)）。
- **系统美化**：字体切换、桌面图标包、系统模糊、表盘通透背景。
- **表盘**：内置/市场表盘切换、摇一摇轮换、表盘管理。
- **设备工具**：亮度、缓存清理、时间/日期设置、震动测试。
- **运行日志**：关键操作写入 `/data/chaos/chaos.log`；设置 → 运行日志可查看尾部、清空。
- **无外部依赖**：模块直接调用固件已验证的 API，不注入 JS、不依赖网络。

## 兼容性

| 项目 | 要求 |
|---|---|
| 设备 | 小米手环 10 Pro（`p67` / `M2551B1`） |
| 固件 | **3.101.043** |
| 模块格式 | ARM ELF32 `ET_REL`，`thumbv8m.main-none-eabi`，软浮点 |
| 安装器 | Lua 容器（magic `0x1234A55A`） |

> 固件地址是硬编码的。**3.101.036 及其它版本不要使用本项目的模块**；升级固件后需要重新适配地址。

## 安装

1. 从 [Releases](https://github.com/WenHuaYiYang/Chaos-Module/releases) 或 GitHub Actions 的 artifact 下载安装器 `.bin`。
2. 用 AstroBox 等第三方工具的自定义表盘或第三方应用入口安装。
3. 安装完成后重启手环，让模块在开机时加载。
4. 打开 Chaos 应用，按需启用自动注入与后台恢复。

> 首次安装、覆盖安装新模块后都需要重启一次。刷机有风险，请自行备份并确认固件版本。

## 使用

### 应用管理

- **单点条目**：隐藏/显示该应用。
- **一键隐藏/显示**：批量操作当前列表；加入排除项的应用会被跳过。
- **排除项管理**（页 7）：勾选不参与一键操作的应用；页 1 不再显示它们。
- **普通 / 深度隐藏**（页 1 开关）：
  - 关（默认，普通隐藏）：条目保留在 `/data/apps.json`，Chaos 用私有名单 + 开机补偿维持隐藏；隐藏/显示都**不用重启**；代价是开机瞬间可能闪一下图标。
  - 开（深度隐藏）：把隐藏条目搬进 `/data/apps.json_hide`，开机不注册、不闪；**显示需要重启**。
  - 切换模式本身要重启一次才完全生效；切换只改变存储方式，不改变“哪些应用被隐藏”。

两种模式的固件依据、代价与验证清单见 [应用管理说明](docs/APP-MANAGER.md)。

### 系统美化

- **更换字体**：从已投递字体中切换，两步确认删除。
- **桌面图标**：应用图标包、恢复系统原图标。
- **系统模糊 / 模糊区域**：选择参与模糊的区域。
- **表盘通透背景**：生成并复用静态背景；固定图片模式需要 `background.bin`。

## 从源码构建

### GitHub Actions（推荐）

仓库自带 [`.github/workflows/build.yml`](.github/workflows/build.yml)：push / PR / 手动触发时安装 nightly 工具链，执行 `sh tools/build_ko.sh`，并上传 `supervisor/chaos_sup.ko`。

### 本地构建

需要：

- Rust nightly（`-Z build-std`）
- `rust-src` 组件
- target `thumbv8m.main-none-eabi`
- Python 3 + Pillow（图标生成）
- 系统 C 链接器（`cc`，Rust build script 需要）

一条命令出全部产物：

```bash
python3 tools/build.py
```

输出：

```text
dist/chaos_sup.ko               内核模块
dist/chaos-installer-p67.bin    可直接安装的 p67 容器
dist/assets/                   图标、容器内 ko 副本、字体占位
```

`dist/` 内的发布产物随仓库提交；`dist/stage/` 是临时目录，不入库。

```text
```

常用组合：

```bash
python3 tools/build.py --skip-ko        # 复用已编译的 ko，只重打容器
python3 tools/build.py --no-container   # 只出 ko 和图标
python3 tools/build.py --no-icons       # 复用已有图标
```

`tools/build_ko.sh` 的流程：

```text
cargo +nightly build --release --target thumbv8m.main-none-eabi
  -Z build-std=core,compiler_builtins
  -C target-cpu=cortex-m33 -C target-feature=-fpregs
        ↓
rust-lld -r --gc-sections -u module_main -u chaos_ctor -T merge_sections.ld
        ↓
fix_ko_layout.py -> verify_chaos_ko.py（未定义符号必须为 0）
```

## 目录结构

```text
Chaos-Module/
├── README.md / README.en.md     中英文说明
├── LICENSE                      AGPL-3.0
├── supervisor/                  内核模块（Rust, no_std）
│   ├── src/
│   │   ├── lib.rs               crate 根、chaos_ctor/dtor
│   │   ├── page.rs              页面与 PAGE_TABLE（18 页）
│   │   ├── ui.rs                页面框架、行、事件分发
│   │   ├── appmgr.rs            应用管理：隐藏/显示、排除项、深度隐藏
│   │   ├── state.rs             全局常量、页描述、槽位
│   │   ├── ipc.rs               /dev/chaos 命令与状态块
│   │   ├── fw_api.rs            fw_api 模块入口
│   │   ├── fw_api/             已验证固件 API 封装
│   │   └── ...                  表盘、字体、图标、日志、设置
│   └── build.rs                 注入 CHAOS_BUILD_TIME
├── installer/                   Lua 安装器
│   ├── chaos_installer.lua      安装/升级/卸载 UI
│   ├── font_pack.lua            字体投递包
│   └── icon_pack.lua            图标投递包
├── module/                      最小示例模块
├── tools/
│   ├── build.py                 一键构建（ko + 图标 + 容器）
│   ├── build_ko.sh              Rust 编译/链接/布局修正
│   ├── p67_pack.py              p67 容器打包/解析（纯 Python）
│   ├── p67-preview.block        336x480 预览块
│   ├── gen_chaos_icon.py        图标生成（Pillow）
│   ├── fix_ko_layout.py         ko 节区重排
│   ├── verify_chaos_ko.py       ko 校验
│   └── lvgl_map.js              LVGL 类描述扫描
├── docs/
│   ├── APP-MANAGER.md           应用管理说明
│   └── LVGL-MAP.md              LVGL 符号地图
├── dist/                        构建产物（不入库）
└── .github/workflows/build.yml  CI
```

## 文档

- [应用管理：隐藏/显示与免重启模式](docs/APP-MANAGER.md)
- 固件研究、地址手册与格式分析：见研究仓库的 `docs/`。
- 安装器实现：[`installer/chaos_installer.lua`](installer/chaos_installer.lua)

## 贡献

1. Fork 本仓库并从 `master` 开分支。
2. 改动模块后至少跑一次 `sh tools/build_ko.sh`（或等 CI）。
3. 涉及固件地址、页面数量、注册链的改动，请在 PR 里附上验证方式（真机日志 / 反汇编证据）。
4. 提交前确认没有把 `.ko`、字体、商业素材或密钥带进仓库。

## 许可证

[AGPL-3.0](LICENSE)。分发修改版时请保留相同许可证与源码获取方式。

字体与第三方素材遵循各自许可证；仓库不包含商业字体、系统图标或固件本体。

## 致谢

- 小米 Vela / NuttX 社区与 openvela 文档
- 所有在真机上验证、提交日志与反汇编证据的贡献者

<div align="center">

**如果这个项目帮到了你，欢迎 Star ⭐**

</div>
