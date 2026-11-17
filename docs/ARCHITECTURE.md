# 架构

Chaos 是一个**内核模块 + Lua 安装器**的组合：安装器把模块送进 `/data` 并写开机脚本，模块加载后
借固件自己的 launcher 注册链把原生页面挂进桌面。没有 JS 注入、没有网络、不改固件分区。

## 一图看懂

```text
开机 (/data/rc.d/chaos.sh)
  └─ insmod /data/chaos/sup.ko
       └─ module_main → chaos_ctor
            ├─ 注册 /dev/chaos（fops 12 槽 + 192 词状态块）
            ├─ 启动维护定时器（等待桌面对象树就绪）
            └─ 注入：页面注册 + app_launcher 条目
                 └─ 桌面出现 Chaos 图标 → 点开 = 页 0（主页）

安装器 (chaos-installer-p67.bin)
  ├─ 部署 sup.ko / 图标到 /data/chaos/
  ├─ 写 /data/rc.d/chaos.sh
  └─ 通过 /dev/chaos 发命令字：0 注册链 / 0x13 完整链 / 0x43 notify / 0x30 字体…
```

## 目录结构

```text
Chaos-Module/
├── supervisor/                  内核模块（Rust, no_std）
│   ├── src/
│   │   ├── lib.rs               crate 根：chaos_ctor/dtor、panic handler、isr 入口
│   │   ├── page.rs              页面框架：Page trait、行/按钮构建器、PAGE_TABLE
│   │   ├── page/                页0..31，按功能分组
│   │   ├── btpan.rs             页32..35：BTPAN、网页服务器、连接信息、实验性功能
│   │   │   ├── home.rs          页 0 主页与实时状态
│   │   │   ├── appmgr.rs        页 1/29/30 应用管理 UI
│   │   │   ├── time.rs          页 2/3 时间与日期
│   │   │   ├── beautify.rs      页 4/5/6/7/13/15/28 美化
│   │   │   ├── watchface.rs     页 8/9/10 表盘
│   │   │   ├── settings.rs      页 11/12/14/16/19 设置类
│   │   │   ├── connect_log.rs   页 17 ChaosConnect内存操作日志
│   │   │   ├── connect.rs       页 47 ChaosConnect入口与启用开关
│   │   │   ├── about.rs         页 18/20/21 关于与许可
│   │   │   ├── system.rs        页 22/24/25/26/27 系统关于
│   │   │   └── keyboard.rs      页 23 键盘页面包装
│   │   ├── ui.rs                页面运行时：建页、事件分发、页面存活判定
│   │   ├── state.rs             全局常量、页描述、槽位、静态变量
│   │   ├── ipc.rs               /dev/chaos：命令、状态块、字体/设置投递
│   │   ├── appmgr.rs            应用管理共享状态与对外 API
│   │   ├── appmgr/              应用管理实现：json/registry/excl/mode/rewrite/detail/ops
│   │   ├── fw_api.rs            fw_api 入口（re-export）
│   │   ├── fw_api/              已验证固件 API 封装 + 地址表
│   │   ├── font_*.rs            字体清单、切换、热替换
│   │   ├── icon_apply.rs        图标包应用/回退
│   │   ├── icon_pack_animation.rs  图标包动画流式播放/暂停/切包
│   │   ├── background_*.rs      通透背景、模糊、桌面滑动
│   │   ├── watchface*.rs        表盘切换、摇一摇轮换
│   │   ├── gyro_pointer.rs     陀螺仪触控点配置、传感器和虚拟输入生命周期
│   │   ├── gyro_pointer/logic.rs 转腕滤波、停留点击、边缘滑动与取消
│   │   ├── connect_log.rs       ChaosConnect专用INFO日志（32KiB内存，不写闪存）
│   │   ├── connect_log_buffer.rs 完整记录环形缓冲与分页
│   │   └── ...
│   └── build.rs                 注入 CHAOS_BUILD_TIME
├── installer/                   Lua 安装器与投递包
│   ├── chaos_installer.lua      安装/升级 UI + 分步注册
│   ├── font_pack.lua            字体投递包
│   └── icon_pack.lua            图标投递包（CIPK 容器）
├── module/                      最小示例模块（验加载链）
├── tools/                       构建脚本（见 BUILD.md）
├── docs/                        文档（本目录）
├── dist/                        发布产物（入库存）
└── .github/workflows/build.yml  CI
```

## 页面模型

- `PAGE_TABLE: [&dyn Page; PAGE_COUNT]`（`page/registry.rs`）—— 62 个页面，页号 0..61；38 为截屏与录屏，39/40 分别为录屏/截屏文件管理，41/42 为录屏/截图设置，43 为应用图标单选列表，见 [CAPTURE.md](CAPTURE.md) 和 [DYNAMIC-ICON.md](DYNAMIC-ICON.md)。58..60 为灵动点设置、大小与功能单选列表，见 [FLOATING-BALL.md](FLOATING-BALL.md)。61 为触控点三级设置，见 [GYRO-POINTER.md](GYRO-POINTER.md)。
- 每个页面实现 `Page`：`fill_title` / `render` / `on_click(ctx, idx)` / 可选的 resume、destroy 钩子。
- 行控件由 `build_rows(ctx, title, specs)` 统一建立，每行一个 LVGL 对象；行事件按**视图下标**派发，
  页面用 `match idx` 或 `idx >= N && idx < view_count()` 自守边界。
- 页面存活用 `page_is_live(pid)`（读固件页描述符 +0x30）判定，所有渲染/分发路径都要先过这道门。
- 句柄作废只在 `on_ui_destroy`：固件递归删对象前会回调，之后 `PAGES[pid]` 的指针全部清零。

### 模块边界

```text
page.rs / page/*        页面渲染与事件；只通过 ui、fw_api 和领域服务工作
ui.rs                   页面运行时：建页、分发、生命周期
appmgr.rs / appmgr/*    应用管理服务：状态、持久化、隐藏/显示、卸载
fw_api.rs / fw_api/*    固件 ABI 封装与地址表
state.rs                全局页描述、槽位和跨模块状态
```

页面模块之间不直接调用彼此的私有实现；跨页共享的只有 `page.rs` 的框架和显式 `pub(crate)` 常量。应用管理页面只调用 `crate::appmgr` 的对外函数，不直接读它的静态缓冲区。

## IPC：`/dev/chaos`

| 部分 | 说明 |
|---|---|
| fops | 12 个槽位，本轮只写 open/close/read/write 四个入口 + 版本位 |
| 状态块 | 48 词（192 字节），含版本字 `0x43485302`（"CHS2"），安装器据此确认模块活着 |
| 命令 | 0 = 注册链（止于 init_buffer）；1/2 = publish（不调固件）；0x13 = 完整注册链；0x43 = 完整 notify；0x30 = 字体切换；0x18/0x19 = 语言；0x44 = 设置请求 |
| 缓冲 | `chaos_read` 支持 ≥4096 的读走 THUNKS 通道；`chaos_write` 是 16 字节定长帧，`WRITE_BUSY` 防重入 |

Lua 侧 `installer/chaos_installer.lua` 与 `font_pack.lua` 是唯一的客户端；协议改动要两边同步改。

## 固件耦合

Chaos 直接调用固件里已静态核对的函数地址（`supervisor/src/fw_api/fwaddr.rs` 是地址表，
调用点按名字引用）。三条硬约束：

1. **只支持 3.101.043**。地址表按这个固件标定；换固件必须重新适配。
2. **软浮点 ABI**。模块编译时用 `-C target-feature=-fpregs`，调固件浮点函数要显式按软浮点传参；
   反过来，个别函数（如 `lv_arc_set_value`）固件是硬浮点 ABI，需要内联汇编传值。
3. **地址基址**。本仓库所有常量都是"文件内偏移 + 一个基址"，三个视图（代码 / 只读数据 / flash）
   基址不同，写新代码前先读地址表文件的注释。

## 数据与持久化

| 路径 | 写入方 | 内容 |
|---|---|---|
| `/data/chaos/sup.ko` | 安装器 | 模块本体 |
| `/data/chaos/settings.cfg` | 模块 | 用户设置 |
| `/data/chaos/appmgr_*.bin` / `appmgr_apply.cfg` | 模块 | 排除名单、批量操作状态 |
| `/data/chaos/font/index.txt` | 模块 | 字体槽位清单（定长文本） |
| `/data/chaos/bridge.cfg` | 模块 | ChaosConnect启用配置；操作日志仅存内存 |
| `/data/apps.json` / `/data/apps.json_hide` | 固件 + 模块 | 应用注册表（隐藏模式见 APP-MANAGER） |
| `/data/rc.d/chaos.sh` | 安装器 | 开机加载脚本 |

写盘约定：所有配置都"先写临时文件 → 回读校验 → rename"。模块侧没有文件系统事务，
所以坏文件必须能被下一次写入覆盖，而不是让启动卡死。

## 生命周期

| 回调 | 时机 | 要点 |
|---|---|---|
| `chaos_ctor` | 模块加载 | 幂等；注册驱动成功才置 `DRIVER_ON` 并启维护定时器 |
| `chaos_on_create` | 页面首次创建 | 作废旧句柄、建行、置 `APP_FG` |
| `chaos_on_resume` | 息屏唤醒/返回 | 恢复前台标志、按需重绘 |
| `chaos_on_pause` | 息屏/退后台 | 清前台标志，不重建定时器 |
| `chaos_on_ui_destroy` | 页面销毁前 | 清该页全部句柄 |
| `chaos_dtor` | 卸载（保留但安装器从不调用） | 只注销驱动 |

## 已知边界

- 不做 `rmmod`：注册链上的条目会悬空（详见 [INSTALL.md](INSTALL.md#卸载与回退)）。
- 无固件指纹校验：装错固件时模块可能加载失败或崩溃。
- 单核、无 MMU 保护：越界写就是 HardFault，没有恢复机会。

## ChaosConnect 文件桥

`supervisor/bridge/` 直接实现私有 CCN1 协议与 NuttX 文件操作：

- `chaos_connect.c`：CRC帧编解码、私有类型与宿主protobuf承载。
- `chaos_files.c` / `chaos_io.c`：文件事务、目录、直接固件VFS调用，没有模块ABI或fake context。
- `bridge.c`：私有消息过滤、有界RX/TX、协商块大小、连接epoch、owner/BT loop分工与停止恢复。
- `bridge_cfg.rs`：默认关闭的持久开关，由UI维护启动与停止。

插件通过官方v4 `transport` / `register` 访问已认证的设备会话，未新增裸L2CAP PSM。
详见 [ChaosConnect文件桥](24-FILE-BRIDGE.md) 的固件证据、测速与验证边界。

## 相关文档

- 构建与发版：[BUILD.md](BUILD.md)
- LVGL 地址：[LVGL-MAP.md](LVGL-MAP.md)
- 应用管理两种模式：[APP-MANAGER.md](APP-MANAGER.md)
