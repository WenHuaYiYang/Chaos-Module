# RW6 适配线状态（摘自交接文档 §7）

> 来源：研究工作区 `docs/Chaos_交接文档_20261107.md` 的**已提交版本**（commit `d5baf2b5` 起）§7「进度：Redmi Watch 6（p65 / best1502p）」。
> 原文随研究仓库继续演进，本文件是 2026-11-08 的快照；最新结论见 `docs/Chaos_RW6_1to1复刻计划_20261031.md` §11。

## 7. 进度：Redmi Watch 6（p65 / best1502p）

**跟 043 完全不是一套**：自己的表盘容器格式、自己的 fw_api 地址、NuttX modlib
内核模块（`work/p65_chaos`，单文件 `src/lib.rs`），靠 `insmod` 装载、靠固件 app
框架注册成应用（app_id 6，包名 `chaos`）。

- **14 页，`PAGE_N=14` 是硬上限**（加到 16 页时模块能装、时间页也能用，但导航里点
  应用管理会把应用静默带走）。页 0 主页 / 1 关于 / 2 应用管理 / 3 缓存清理 /
  4 亮度 / 5 时间设置 / 6 桌面图标 / 7 震动 / 8 日期设置 / 9 表盘切换 / 10 设置 /
  11 系统美化 / 12 表盘管理 / 13 运行日志。
  （常量名还叫 `PAGE_FILES`/`PAGE_STORAGE`，是退役页让给时间/日期的历史残留。）
- **出包链已打通到原生容器**（比 `AGENTS.md` 写的"现状仍是 tar"更新）：
  - `dist/chaos-installer-rw6.bin`（165143 B，17 条）——RW6 原生表盘容器。
    格式已反解：头 `0x00` 魔数/版本、`0x10` 记录表尾、`0x20` 预览块起点、
    `0x28` 十二字节 pkgName；**`0xA477` 起是文件链，每条 =
    `[u24 数据长][u8 名字长][16 字节 0][名字][数据]`，没有中心表**，逐条串到 EOF。
    `scripts/build_chaos_p65_watchface.py` 保留前缀逐字节、只换 pkgName 与
    `_lua/mood/mood.lua`，ko 与图标追加在链尾，带往返校验。
  - `dist/chaos-watchface-rw6.tar`（112640 B）——**播种包**：把三个文件塞进内置
    「心情」表盘的 Lua 槽位（整台机器上唯一"可写 + 会执行 Lua"的位置），
    之后切表盘点运行就是安装。
  - `dist/chaos-p65-043-v1.tar`（102400 B）——旧的 USTAR + `install.sh` 路线。
- **两道固件的门**（与包格式无关）：市场表盘套用要手机在线（guest 整屏"蓝牙已断开"）；
  `add_new_watchface` 解出来的 market 目录重启后连节点一起被清。所以走内置槽位播种。
- **未接通**：开机自启（`/etc/init.d` 在只读 romfs，固件不从 `/data` 执行任何东西
  ⇒ 每次重启要手动再跑一次 `sh /data/install.sh`）、字体全局生效、自定义 TTF 投递
  （字体登记薄与 `lv_freetype_font_create` 在 UI 定时器和装载线程都会把任务卡死，
  **硬阻塞**）、系统模糊/模糊区域后端、图标包素材、摇一摇（无事件源）、
  表盘删除（API 返回 0 但文件不动）。
- 主 supervisor 里另有 `--features fw-p65` 一档地址表（`fw_api/fwaddr.rs` 的 `tp65`），
  **地址没覆盖齐之前被 `compile_error` 挡住**，与 `work/p65_chaos` 是两条并行的路。

详见 `docs/Chaos_RW6_1to1复刻计划_20261031.md`（§10-§11 是最新状态）与
`docs/Chaos_RW6安装与使用_20261030.md`（**§4 那张状态表部分已过时**：时间/日期
在 10-31 已落地，该文档还写着"没找到时钟入口"）。
