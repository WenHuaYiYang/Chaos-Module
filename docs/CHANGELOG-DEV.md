# 与基线版本的代码差异（开发者）

基线：`origin/master` = `d31f1e5`（2026-10-18）　当前：`0b1e78d`
逐文件比对源码得出，按子系统列出。

## 页面与导航

- `supervisor/src/page.rs`：`PAGE_TABLE` 17 → **19** 项。
  - 新增 `ExclPage`（页 7，排除项管理）、`LogPage`（页 17，运行日志）、`AboutPage`（页 18，关于）。
  - 删除 `ViewerPage`（原页 7 文件查看，入口早在基线前已注释退役）。
- 主页 `InfoPage`：导航行 `3..6` → `3..7`，新增 `(7, "关于") → nav_goto(18, 0)`；数据行/圆环/顺序未动。
- `SettingsPage`：删掉 5 行关于信息（`Chaos v1.0.2`、编译时间、运行固件、两位开发者），行 5 改为"运行日志 → 页 17"；关于信息迁入独立 `AboutPage`（新增许可证行）。
- `AboutPage`：关于信息迁入独立页；版本行 `v1.0.2 → v1.0.3` 且改为可点（连点 5 次进调试态，见下）。
- 排除项页：从页 1 行 3 进入；行 2 是模式行（`管理排除项 / 管理显隐`）；条目事件槽 = 条目下标 + 12。

## 应用管理 `supervisor/src/appmgr.rs`（约 +994 行）

- `REG_CAP` 10240 → **32768**；`APP_MAX` 仍 40。
- 新增排除名单：`EXCL_PATH = /data/chaos/appmgr_excl.bin`，独立文件，不动系统注册表；排除项不参与批量与开机补偿，页 1 列表不返回它们。
- 新增全局模式开关 `APPLY_MODE`：
  - 开 → 深度隐藏：把条目从 `/data/apps.json` 搬到 `/data/apps.json_hide`，重启后系统不注册；提示"深度隐藏已开启, 重启后生效"。
  - 关 → 普通隐藏：条目留在 `apps.json`，走 `hideFlag` + `app_launcher_del`；切回时提示"深度隐藏已关闭, 存储已迁移; 请重启后生效"。
  - 两个方向都带迁移失败回滚，写配置失败时不改模式。
- 列表事件槽改为"视图下标 → 原始下标"（`view_at`），修掉显隐状态错位；显示操作改为先 `app_launcher_del` 再 `add`。

## 运行日志（新子系统）

- 新增 `supervisor/src/log.rs`；`lib.rs` 增 `mod log`，`ui.rs`/`appmgr.rs` 等处调用。
- 写 `/data/chaos/chaos.log`，48 KB 上限，超限重建。
- `LogPage`：每页 8 行（`LOG_PAGE_LINES`）、"更早/更新一页"、刷新、清空需两步确认（`LOG_CLEAR_ARMED`）。

## 固件 arc（目前只用于调试页）

- `supervisor/src/fw_api/gfx.rs` 新增：`arc_create`(`0x1C0641D1`)、`arc_set_progress`、`arc_set_bg_full`、`arc_set_pct_full`、`arc_disable_input`、`obj_set_size_wh`。
- 字段：`+0x38/+0x3C` INDICATOR 角（f32）、`+0x40/+0x44` MAIN 角（f32）、`+0x48` value、`+0x4C/+0x50` min/max；进度 = INDICATOR `90 → 90+pct*3.6`，底环 = MAIN `90 → 450`。
- `AboutPage` 调试态（版本连点 5 次）：四环对比矩阵 A/B/C/D、`lvgl_dump.bin`（512 KB）导出、关闭调试态回主页时重置。
- 主页数据行最终**没有**改用 arc（`a759189` 回退成基线原版的 `data_row_create` 横条），arc 封装目前只服务调试页。

## 安装器 `installer/chaos_installer.lua`

- 新增"重启"按钮：3 秒倒计时、运行中禁止、`reboot` 失败回滚到可点。
- 布局：卡片高 94 → 68，标题/按钮 `y_ofs` 调整，"注入设置"按钮位置 60 → 2。

## 构建与打包

- `tools/build.py` 88 → 180 行：串起 ko → 图标 → 容器；新增 `--skip-ko / --no-container / --no-icons`。
- 新增 `tools/p67_pack.py`（纯 Python 打 p67 容器）、`tools/p67-preview.block`、`tools/lvgl_map.js`。
- 新增 `.github/workflows/build.yml`（CI 跑 `tools/build_ko.sh` 并上传 `chaos_sup.ko`）。
- 删除 `supervisor/chaos_sup.ko`，产物统一进 `dist/`（ko 194608 B、容器 300991 B）。

## 文档与版本

- 新增 `docs/`（README 索引、INSTALL、USAGE、BUILD、ARCHITECTURE、CHANGELOG、APP-MANAGER、LVGL-MAP）与 `README.en.md`；`README.md` 重写。
- 版本三处同步为 **v1.0.3**：`page.rs` 文案、`supervisor/Cargo.toml`、容器头 `0x00010003`。

## 未变

- IPC 命令集（`0x21 / 0x10 / 0x20 / 0x11 / 0x30 / 0x22`）、状态块 48 词、版本字 `0x43485302`。
- 只支持固件 3.101.043，无固件指纹校验。
- 字体/图标投递包（`installer/font_pack.lua`、`icon_pack.lua`）在基线已存在，本次未改。
