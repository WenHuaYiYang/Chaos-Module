<div align="center">

# Chaos

**小米手环 10 Pro 的原生应用平台**

一个内核模块加一个 Lua 安装器，借固件自己的注册链把原生页面装进手环桌面。

[简体中文](README.md) | [English](README.en.md)

[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![Firmware](https://img.shields.io/badge/firmware-3.101.043-green.svg)](#兼容性)
[![Device](https://img.shields.io/badge/device-p67-orange.svg)](#兼容性)
[![Release](https://img.shields.io/badge/version-v1.1.10-blue.svg)](docs/CHANGELOG.md)

</div>

---

## 简介

Chaos 把一组 Rust 编写的原生页面注册进小米手环 10 Pro 的固件，让它们在桌面以普通应用的形式出现：
图标、点击、转场都由固件自己的 launcher 完成。模块不联网、不依赖手机，也不改动固件分区。

它最初是为了在 10 Pro 上补研究工具链而写的，后来逐渐长成一套完整的设备管理界面：
应用管理、表盘、字体、图标、亮度、缓存、模糊与震动设置。

> **实现**：Rust（`no_std`）与 C 原生模块，44 个页面 id，Lua 安装器。

## 特性

- **应用管理**：隐藏/显示、一键批量、排除项；普通/深度隐藏两种模式（[说明](docs/APP-MANAGER.md)）。
- **系统美化**：字体切换、静态/动态图标包、系统模糊、表盘通透背景。动态图标包兼容原 CIPK 投递格式，当前可见动态图标全部播放（[制作与协议](docs/ICON-PACKS.md)）。
- **蜂窝应用列表（实验）**：双向拖动与惯性、左缘调节间距、右缘整体缩放、边界返回、原生编辑与删除，与 RW6 效果互斥。开关持久保存，切换时重新绑定列表并立即应用；完整模拟器显示与帧率待复测（[实现与验证](docs/HONEYCOMB.md)）。
- **表盘**：内置/市场表盘切换、摇一摇轮换、表盘管理。
- **设备工具**：亮度、缓存清理、时间/日期设置、震动测试。
- **应用图标**：设置 → 应用图标进入三级单选列表，仅保留原版和动态，统一 96×96；首次安装及从旧版升级自动选择动态，之后保存手动选择。动态播放上传的 GIF（193 帧、5.2 秒循环），暂停时复用原版文件，移除其他主题和额外静态缓冲以缩小体积（[说明](docs/DYNAMIC-ICON.md)）。
- **运行日志**：关键操作写入 `/data/chaos/chaos.log`，设置 → 运行日志可查看与清空。
- **截屏与录屏**：首页进入控制列表，后台录屏可选 GIF 15 fps / APNG 目标 60 fps，息屏后 BMP 截屏；录屏和截图分别选择 `/data/chaos/media` 或 `/data/offlinelog`，三级列表删除管理（[格式与验证边界](docs/CAPTURE.md)）。
- **无外部依赖**：只调固件已验证的 API，不注入 JS、不依赖网络。
- **原生文件桥**：内置 CanopusConnect 兼容端，AstroBox 文件管理器插件可直接连接，不再需要 Canopus 模块/管理器。

## 兼容性

| 项目 | 要求 |
|---|---|
| 设备 | 小米手环 10 Pro（`p67` / `M2551B1`） |
| 固件 | **3.101.043** |
| 模块格式 | ARM ELF32 `ET_REL`，`thumbv8m.main-none-eabi`，软浮点 |
| 安装器 | Lua 容器（magic `0x1234A55A`） |

> 固件地址是硬编码的。**3.101.036 及其它版本不要使用本项目的模块**；升级固件后需要重新适配地址。

## 快速开始

1. 从 [Releases](https://github.com/WenHuaYiYang/Chaos-Module/releases) 下载 `chaos-installer-p67.bin`。
2. 用 AstroBox 等工具的自定义表盘 / 第三方应用入口安装。
3. **重启手环**，模块在开机时加载。
4. 打开 Chaos 应用，按需启用自动注入与后台恢复。

完整步骤与故障排查见 **[docs/INSTALL.md](docs/INSTALL.md)**。

## 构建

```bash
python3 tools/build.py            # ko + 图标 + p67 容器
python3 tools/build.py --skip-ko  # 只重打容器
```

依赖、流程、版本号规则与发版检查单见 **[docs/BUILD.md](docs/BUILD.md)**。

## 文档

| 文档 | 内容 |
|---|---|
| [安装与升级](docs/INSTALL.md) | 安装、回退、常见故障 |
| [使用说明](docs/USAGE.md) | 页面地图与每个功能 |
| [应用管理](docs/APP-MANAGER.md) | 两种隐藏模式的固件依据与代价 |
| [构建与发版](docs/BUILD.md) | 工具链、流程、版本号、CI |
| [架构](docs/ARCHITECTURE.md) | 启动链、页面模型、IPC、固件耦合 |
| [LVGL 地址](docs/LVGL-MAP.md) | p67 固件的 LVGL 类描述与入口 |
| [更新日志](docs/CHANGELOG.md) | 版本历史 |
| [截屏与录屏](docs/CAPTURE.md) | 录屏/截图设置、双目录文件管理、格式与验证边界 |

文档索引：[docs/README.md](docs/README.md)。

## 贡献

1. Fork 本仓库并从 `master` 开分支。
2. 改动模块后至少跑一次 `sh tools/build_ko.sh`（或等 CI）。
3. 涉及固件地址、页面数量、注册链的改动，请在 PR 里附上验证方式（真机日志 / 反汇编证据）。
4. 提交前确认没有把 `.ko`、字体、商业素材或密钥带进仓库。

## 许可证

[AGPL-3.0](LICENSE)。分发修改版时请保留相同许可证与源码获取方式。

字体与第三方素材遵循各自许可证；仓库不包含商业字体、系统图标或固件本体。

“Corona 模糊”移植自 [Canopus-Module-Corona](https://github.com/AstralSightStudios/Canopus-Module-Corona)，
该实现按上游声明采用 **AGPL-3.0-only**。上游实现和[许可证原文](supervisor/corona_vendor/LICENSE)
保留在 `supervisor/corona_vendor/`；来源、文件校验值、Chaos 适配边界和分发注意事项见
[Corona 第三方声明](licenses/CANOPUS-MODULE-CORONA-NOTICE.txt)。设备端可在“关于 → 开放源代码许可”查看许可全文。
“模糊区域”对 Corona 和旧版模糊共同生效；系统模糊页底部“说明”同时列出上游与
[Chaos 修改后源码](https://github.com/WenHuaYiYang/Chaos-Module/tree/master/supervisor)。
分发二进制时需提供与该二进制对应的完整源码和构建/安装脚本；只附上上游链接或许可证并不足以替代相应源码义务。

## 致谢

- 小米 Vela / NuttX 社区与 openvela 文档
- 所有在真机上验证、提交日志与反汇编证据的贡献者

<div align="center">

**如果这个项目帮到了你，欢迎 Star ⭐**

</div>
