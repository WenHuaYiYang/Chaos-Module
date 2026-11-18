# 插件NSH取消独立权限

配套插件0.3.2起，手环“ChaosConnect → 权限设置 → 插件权限”移除“NSH终端”开关。插件NSH的OPEN／WRITE／READ／CLOSE不检查插件mask 0x20位；定时维护也不因插件mask变化关闭PTY。旧配置位保持兼容读取，无需迁移。

Android端NSH依然检查独立0x20权限。两端仍需本次连接已允许、服务已启用，并遵守当前lease与真实载波身份；拒绝连接、关闭服务、断连或心跳超时仍关闭终端。

插件输入改为AstroBox原生输入弹窗，确认后一次性提交草稿，再点发送；取消保留旧草稿，不再逐字发送Input/Focus/Blur插件事件。

验证：`python3 tools/test_chaos_connect.py` 31项通过，覆盖插件mask为0仍可开/写/读NSH、插件连接拒绝回收旧lease、Android缺少0x20拒绝/撤销后停止、服务关闭和未确认连接不启动。`python3 tools/build.py` 默认10p-3.101.043完整构建成功；未新增固件地址。插件另有34项Rust回归和WASM构建。

本轮未在实体手环与真实AstroBox验证延迟。需要同时安装新的模块与插件，Android端无需更新。
