# 投递包安装与插件NSH集成验证

- 默认固件目标：10 Pro / 3.101.043，现有symbol身份与ABI检查通过。未新增固件地址；安装只复用现有VFS，NSH继续使用既有独立PTY后端。
- `python3 tools/test_connect_packages.py`：8项通过。覆盖逐包默认询问、最终名称、字体与图标安装、256个图标边界、非法字体／路径／CRC、拒绝与超时、短写、取消／断连／撤权、同名／满槽、提交失败保留旧文件，以及并发清单变更。
- `python3 tools/test_chaos_connect.py`：31项通过，包括新增安装策略持久化、保存失败保持旧策略、完整中文确认文本、亮屏和震动的宿主测试。
- `python3 tools/test_connect_terminal.py`：27项独立PTY生命周期回归通过。
- 配套Android 0.6.0：41项JVM（其中9项生产Kotlin/C互通）和14项模拟器UI回归通过，构建/lint通过。使用制作台生产构建器生成的合成bin，对照安装后的资源与128字节清单。
- 插件0.3.0：20项Rust测试及WASM/ABP构建通过。NSH需要在手环插件权限中单独允许。
- `python3 tools/build.py` 完整构建通过，保留chaos-fetch、capatch和图片素材，容器产物路径见build-report.json。
- 许可页面移除SysPatch/LVGL模糊入口；源码版权与许可证文件保留，LVGL notice进入安装器既有的第三方说明槽。

本轮未操作实体手环，也未在真实AstroBox运行NSH。主机C互通与模拟器回归不能替代真机无线安装、文件系统和shell联调。
