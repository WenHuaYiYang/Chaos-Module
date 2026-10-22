# 更新列表（开发者）

对比基线：`origin/master` = `d31f1e5`（2026-10-18 设备侧 sync 快照） → `7a7555f`（v1.0.3）。
规模：148 个提交，41 个文件，+3128 / −540。

## 固件适配（LVGL arc）

- 新增 `supervisor/src/fw_api/gfx.rs`：`arc_create`(`0x1C0641D1`)、`arc_set_progress`、`arc_set_bg_full`、`arc_set_pct_full`、`arc_disable_input`、`obj_set_size_wh`。
- 字段定稿：`+0x38/+0x3C` INDICATOR 角（f32）、`+0x40/+0x44` MAIN 角（f32）、`+0x48` value、`+0x4C/+0x50` min/max；进度 = INDICATOR `90 → 90 + pct*3.6`，底环 = MAIN `90 → 450`。
- 移除对 `0x0C107790`（样式派发器）的调用——真机 HardFault；该偏移的正确 VA 是 `0x0C1C7790`。
- KNOB 点置透明 + 关掉触摸拖拽，避免用户手拨百分比。
- 新增 `fw_api/page.rs`：页面注册/回收封装；`fw_api/sys.rs`、`fw_api/gfx.rs` 增补若干入口。

## 主页与页面框架

- `supervisor/src/page.rs` +569：存储/内存由横向进度条改为双环（120×120，左上对齐）；百分比居中；纵向滚动只锁圆环行。
- 关于入口从旧位置移到主页末行；关于页去掉"项目主页"行。
- 版本行改为 tap 行，**连点 5 次**进入隐藏调试页；离开回主页即清调试态。
- 调试页：四环对比矩阵（默认角 / 整圆背景 × 25% / 75%）+ LVGL 内存 dump（512 KB，二次确认）。
- `text.rs` +15、`ui.rs` ±6、`state.rs` ±17 配套。

## 运行日志（新增子系统）

- `supervisor/src/log.rs` +104：`/data/chaos/chaos.log`，带时间戳，48 KB 上限，超限删文件重建。
- 日志页（页面 17）：每页 8 行分页、两步确认清空；覆盖批量操作、排除项、模式切换。
- 修复行数组越界导致的界面卡死。

## 应用管理

- `supervisor/src/appmgr.rs` +994：排除项页（页面 7）、深度隐藏全局开关、免重启（keep）显示模式。
- 注册表缓冲 32 KB；旧配置自动迁移并收敛重复的隐藏/排除名单；定长写回 + 迁移回滚。
- 列表事件槽改为"视图下标 → 原始下标"映射，修复显隐状态错乱。
- 排除项不参与批量与开机补偿；显示操作改为"先 del 再 add"，避免重复图标。

## 安装器与打包

- `tools/build.py` +208 重写：一键 ko → 图标 → 容器，参数 `--skip-ko / --no-container / --no-icons`。
- 新增 `tools/p67_pack.py`（188 行）：纯 Python 打 p67 容器（magic `0x1234A55A`、版本字、预览块），去掉外部打包器依赖。
- `tools/p67-preview.block`（3934 B）预览帧；`tools/lvgl_map.js` 辅助脚本。
- `installer/chaos_installer.lua` +49：主界面加"重启"按钮（3 秒倒计时），投递流程与版本字检查调整。
- `tools/build_ko.sh` +9；`.github/workflows/build.yml` +33（CI 构建 `chaos_sup.ko`）。

## 仓库与文档

- `dist/` 全量入库：`chaos_sup.ko` 194608 B、`chaos-installer-p67.bin` 300991 B、四个图标/字体资产；移除 `supervisor/chaos_sup.ko`。
- README 中/英精简；新增 `docs/`：README（索引）、INSTALL、USAGE、BUILD、ARCHITECTURE、CHANGELOG，并纳入原有 APP-MANAGER、LVGL-MAP。
- 版本滚动到 **v1.0.3**：关于页文案、`supervisor/Cargo.toml`、容器头 `0x00010003` 三者一致。

## 未变

- 固件仍只支持 3.101.043，地址表按该版本硬编码；无固件指纹校验。
- IPC 命令集：`0x21/0x10/0x20/0x11/0x30/0x22`；状态块 48 词、版本字 `0x43485302`。
