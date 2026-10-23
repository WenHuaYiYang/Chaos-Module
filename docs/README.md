# Chaos 文档

本目录是 Chaos 的全部项目文档。`README.md` 只保留概览与快速开始，细节都在这里。

## 按角色找

| 我想…… | 看这篇 |
|---|---|
| 把它装到手表上 | [INSTALL.md](INSTALL.md) |
| 知道每个页面能做什么 | [USAGE.md](USAGE.md) |
| 理解应用隐藏/显示的两种模式 | [APP-MANAGER.md](APP-MANAGER.md) |
| 从源码构建、打包、发版 | [BUILD.md](BUILD.md) |
| 改代码前了解整体结构 | [ARCHITECTURE.md](ARCHITECTURE.md) |
| 查固件 LVGL 类描述与构造入口 | [LVGL-MAP.md](LVGL-MAP.md) |
| 看版本改了什么 | [CHANGELOG.md](CHANGELOG.md) |
| 对比基线版本的完整改动 | [CHANGELOG-DEV.md](CHANGELOG-DEV.md)（开发者） · [CHANGELOG-USER.md](CHANGELOG-USER.md)（使用者） |

## 文档清单

| 文件 | 内容 | 读者 |
|---|---|---|
| [INSTALL.md](INSTALL.md) | 安装、升级、回退、常见故障 | 使用者 |
| [USAGE.md](USAGE.md) | 页面地图、每个功能的用法与前置条件 | 使用者 |
| [APP-MANAGER.md](APP-MANAGER.md) | 普通/深度隐藏的固件依据、改动文件、验证清单 | 使用者 + 维护者 |
| [BUILD.md](BUILD.md) | 工具链、构建流程、版本号规则、发布检查单 | 维护者 |
| [ARCHITECTURE.md](ARCHITECTURE.md) | 目录结构、启动链、页面模型、IPC、固件耦合 | 维护者 |
| [LVGL-MAP.md](LVGL-MAP.md) | p67 固件里 LVGL 类描述、构造入口、绘制函数地址 | 维护者 |
| [CHANGELOG.md](CHANGELOG.md) | 版本历史 | 所有人 |
| [CHANGELOG-DEV.md](CHANGELOG-DEV.md) | 与基线 `d31f1e5` 的逐项技术差异 | 维护者 |
| [CHANGELOG-USER.md](CHANGELOG-USER.md) | 与上一公开版的差异，含排除项/深度隐藏说明 | 使用者 |

## 约定

- **版本号**：容器头 `0x000X0Y0Z` 对应 `vX.Y.Z`，页面里显示的是同一个号（见 [BUILD.md](BUILD.md#版本号规则)）。
- **文档与代码同 PR**：改行为就改对应文档，改版本就补 CHANGELOG。
- 固件研究、地址手册、格式分析不在本仓库，见研究仓库的 `docs/`。
