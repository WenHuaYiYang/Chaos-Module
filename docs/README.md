# Chaos 文档索引

本仓库是 [qxdho/Chaos-Module](https://github.com/qxdho/Chaos-Module)，上游为 [WenHuaYiYang/Chaos-Module](https://github.com/WenHuaYiYang/Chaos-Module)。当前主线面向小米手环 10 Pro（p67）固件 **3.101.043**；其它固件的研究资料不代表已支持安装。

初次使用按「安装 → 使用」阅读；修改源码按「开发 → 构建 → 测试」阅读。带日期的报告与计划保留原路径，供追溯，不作为当前操作指南。

## 安装与使用

| 文档 | 内容 |
|---|---|
| [安装与升级](INSTALL.md) | 固件要求、覆盖安装、重启、回退与排障 |
| [使用说明](USAGE.md) | 页面地图、功能入口和前置条件 |
| [应用管理](APP-MANAGER.md) | 隐藏模式、排除项与固件限制 |
| [蜂窝列表](HONEYCOMB.md) | 拖动、缩放、编辑与实验边界 |
| [灵动点](FLOATING-BALL.md) | 全局悬浮球、快捷菜单、常亮与亮度调节 |
| [陀螺仪触控点](GYRO-POINTER.md) | 单手转腕移动、停留点击、边缘滑动及可调参数 |
| [截屏与录屏](CAPTURE.md) | 格式、存储目录与文件管理 |
| [图标包](ICON-PACKS.md) / [应用图标](DYNAMIC-ICON.md) | 投递格式与 Chaos 自身图标选择 |
| [上次充电](LAST-CHARGE.md) / [屏幕旋转](SCREEN-ROTATION.md) / [键盘](KEYBOARD.md) | 专项功能说明 |
| [S541 控制条](S541-LAUNCHER.md) / [快应用动画](QUICKAPP-ANIMATION.md) | 启动入口与动画实现；后续修正见性能指南 |

## 开发与验证

| 文档 | 内容 |
|---|---|
| [开发工作流](DEVELOPMENT.md) | fork、分支、提交范围与 GitHub Desktop |
| [构建与发版](BUILD.md) | 工具链、缓存、产物、版本与 CI |
| [测试指南](TESTING.md) | 按改动选择主机测试与真机验收 |
| [架构](ARCHITECTURE.md) | 启动链、页面模型与 IPC |
| [灵动点系统服务 ABI](FLOATING-BALL-SERVICES-ABI.md) | 电量与返回表盘接口的固件依据和验证边界 |
| [当前性能说明](PERFORMANCE.md) | 已实施优化、体验取舍、验证范围和后续计划 |
| [更新日志](CHANGELOG.md) | 版本历史及本 fork 未发布变更 |
| [调试页](DEBUG-PAGE.md) / [日志级别](LOG-LEVEL.md) | 专项调试说明；通用闪存运行日志已删除 |

## 图形与模糊实现

- [Blur V2 移植](BLUR-V2-PORT.md)
- [控制中心按钮模糊](CONTROL-CENTER-BUTTON-BLUR.md)
- [Corona 背景恢复与所有权](CORONA-BLUR-RECOVERY.md)
- [LVGL 地址与类描述](LVGL-MAP.md)

这些文档保留实现依据；当前调度策略与滑动中的视觉取舍以 [性能说明](PERFORMANCE.md) 为入口。

## 连接、网络与协议

- [ChaosConnect 文件桥与 AstroBox 插件](24-FILE-BRIDGE.md)
- [Android 端](CHAOSCONNECT-ANDROID.md) / [ChaosConnect NSH](CHAOSCONNECT-NSH.md)
- [Web 服务](WEB-SERVER.md) / [Web NSH](WEB-NSH.md)
- [BTPAN ABI](BTPAN-ABI.md)
- [NSH ABI](NSH-ABI.md) / [PTY](NSH-PTY.md) / [进程启动](NSH-SPAWN.md)
- [通知图标 ABI](NOTIFICATION-ICON-ABI.md)
- [CCN1 数据样例](CHAOSCONNECT-V1.hex)（协议数据，非使用指南）

## 固件适配与研究

- [多固件符号方案](MULTI-FIRMWARE-SYMBOLS.md) / [symbol 提取与绑定](../symbol/README.md)
- [Redmi Watch 6 移植线](../ports/rw6/README.md)（独立研究目录）
- [远程屏幕可行性](25-REMOTE-SCREEN-FEASIBILITY.md)
- [蓝牙音频与 Lyra UI 参考](23-UI参考-蓝牙音频与Lyra.md)

## 历史记录与计划

完整入口见 [历史记录索引](history/README.md)，包含性能审查、分阶段修复、体积与界面重构计划，以及旧版本对比。计划中的目标不等于已实现功能，主机测试结果不等于真机帧率或续航。

## 维护约定

- 行为变化时同步使用说明；构建参数变化时同步构建与测试指南。
- 新文档加入本索引；已被替代的说明添加状态提示，保留原链接。
- 历史报告保留日期、基线与验证条件，避免将不同构建的数字混作当前结果。
- 发版前核对源码、打包器、锁文件与中英文徽章版本，详见 [版本号规则](BUILD.md#版本号规则)。
