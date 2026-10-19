# 应用管理：隐藏/显示与免重启模式

> 本文说明 Chaos 应用管理页的两种工作模式、它们改动的文件、固件层依据，以及真机验证清单。

## 1. 固件维护三层状态

| 层 | 位置 | 作用 |
|---|---|---|
| 持久化名单 | `/data/apps.json`、`/data/apps.json_hide` | 开机决定“哪些应用已安装” |
| 内存注册表 | `APP_REGISTRY`（0x200E_B640） | 运行时按包名/app_id 找应用；节点 `+0x08` 包名、`+0x0C` 图标路径、`+0x10` app_id、`+0x3C` bit0 = 应隐藏 |
| 桌面图标 | `LAUNCHER_DESKTOP_LIST` + LVGL 对象 | 用户看到的图标；`app_launcher_del/add` 只改这一层 |

`app_launcher_add`（0x0C51_3A44）先按 app_id 查注册表节点；节点不存在直接返回 0。节点存在才进入建记录回调 `FUN_0C513700`：

```text
ldrb.w r2, [r0, #0x3c]
lsls   r1, r2, #31        ; bit0 = “应隐藏”
bmi    0x0C5137A4         ; 打 “The app need hidden”，返回 0
```

所以：节点不存在时，清 `+0x3C` bit0 根本执行不到，`add` 也不可能成功。

## 2. 模式开关

页 1 顶部有 **“免重启显示”** 开关，状态保存在 `/data/chaos/appmgr_apply.cfg`（`0` / `1`）。

### 模式 A：重启生效（默认，开关关）

- 隐藏：条目从 `apps.json` 搬到 `apps.json_hide`，置 `+0x3C` bit0，再 `app_launcher_del`。
- 显示：写回 `apps.json`；本机若还能找到节点则清 bit0 后 `add`，否则提示重启。
- 开机后固件按 `apps.json` 重新注册，所以“显示”最终要重启一次。
- 与其它工具读写同一份配置的语义一致，最安全。

### 模式 B：免重启显示（开关开，保留条目）

- 隐藏：**不动 `apps.json`**，把包名记进 `/data/chaos/appmgr_keep.bin`，置 bit0、`app_launcher_del`。
- 显示：从名单摘掉，清 bit0、`app_launcher_add`，**立即生效**。
- 开机补偿：模块的维护定时器在 UI 线程调用 `boot_apply()`，对名单里的应用重新置位并删图标；节点尚未注册时最多重试 120 个 tick。
- 切回模式 A：把名单里的条目搬进 `apps.json_hide`，随后仍要重启一次。

### 模式 B 的代价

- 开机瞬间图标可能闪现（取决于模块加载与桌面初始化的先后）。
- 其它工具读 `apps.json` 会认为该应用仍可见/已安装。
- 切换模式前已经在 `apps.json_hide` 里的应用，第一次显示仍要重启一次（节点要等下次开机才注册）。
- 只对“由 Chaos 隐藏”的应用成立；别处隐藏的应用仍按原语义处理。

## 3. 排除项

`/data/chaos/appmgr_excl.bin`：magic `EX01` + 40 槽 × 44 字节包名。

- 页 1 列表不显示被排除的应用。
- 一键隐藏/显示跳过被排除的应用。
- 页 7 可逐项取消；页 7 的“管理显隐”模式只列排除项，可单独隐藏/显示。
- 排除项与模式开关相互独立。

## 4. 文件一览

| 文件 | 内容 |
|---|---|
| `/data/apps.json` | 可见应用注册表（固件格式，原位改写） |
| `/data/apps.json_hide` | 隐藏应用注册表（同格式） |
| `/data/chaos/appmgr_keep.bin` | 模式 B 的 Chaos 隐藏名单：`KH01` + 40 × 44 字节包名 |
| `/data/chaos/appmgr_excl.bin` | 排除名单：`EX01` + 40 × 44 字节包名 |
| `/data/chaos/appmgr_apply.cfg` | 模式开关：ASCII `0` / `1` |

## 5. 验证清单

1. 模式 A：隐藏 → 重启 → 点显示 → 提示“重启后显示” → 重启后图标回来。
2. 模式 B：隐藏 → 图标立即消失 → 点显示 → 图标立即回来（不重启）。
3. 模式 B：隐藏 → 重启 → 图标不回来（允许开机瞬间闪现）。
4. 模式 B 切回 A：已有隐藏项进入 `apps.json_hide` → 重启后仍隐藏。
5. 排除项：加入后一键操作跳过；页 1 不显示；页 7 可取消。
6. 损坏文件：把 `appmgr_keep.bin` / `appmgr_excl.bin` 写成乱码，页面应正常打开并按空名单处理。

## 6. 已知限制

- 开机补偿的闪现时长需要真机录像确认。
- 模式 B 与米环管理、SmartHome 等工具的交叉行为未验证。
- 固件没有可安全调用的“运行时重新注册本地快应用”入口；`quickapp_register_app` 传入空 `extra` 会在 `strdup(NULL)` 处触发 busfault（已真机复现）。
