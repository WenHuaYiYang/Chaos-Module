# 模糊生命周期与真机失效诊断

## 2026-11-15：活力指标退出后失效仍未在真机定位

用户确认此前回调回绑、完整重建和源重试的修改均未解决真机问题。
下文历史修复记录中的“根因”描述属于代码路径分析和主机复现，**不能据此认定
本次真机失效的具体原因已经确认**。当前没有失效时的设备状态快照。

本次重新核对的固件为 p67 / 3.101.043，AP SHA-256：
`519307675665e4866d722a8119a98589c397b614ac3294cb87bfc86de45756ec`。
使用现有 `vela043` Ghidra 工程、只读模式、不重新分析：

- 文件偏移 `0xD17054` 的注册项指向 `com.xiaomi.miwear.vitality`，`+8` 为 `0x36`；
  Chaos 现有前台应用识别没有认错应用。
- 活力指标页面表位于数据别名 `0x2CDD7E14`，创建/恢复/暂停/销毁分别指向
  `0x0C650901` / `0x0C64FA1D` / `0x0C64F9F9` / `0x0C64FCA5`（Thumb）。
  恢复和暂停直接调用各自的数据订阅管理；没有在这两个回调内直接移除模糊 hook
  的证据，不能宣称“活力指标清掉了模糊回调”。
- 当前 Chaos 在观察到 `0x36 → HOME` 后主动进入完整下架流程。
  图标恢复、native 空闲、`commit(false)` 和延迟资源回收中的任一条件持续不满足，
  都会让 `CORONA_VITALITY_RECOVER` 保持置位。开关只修改期望配置，不能越过这些
  安全条件。这能解释“重新开关也没用”的代码机制，但尚不能确定真机卡在哪一步。

本次仅补全 RAM 诊断和等待原因，不再凭推测新增强制重建或强清 GPU 状态。
原生快照槽、任务链、GPU 缓冲的释放条件和所有权规则保持原样。

安装本次构建后，打开活力指标并返回，确认失效后进入“调试 → 模糊调试”。
一次完整文本即可区分以下情况（无需开启磁盘运行日志）：

| 字段 | 含义 |
|---|---|
| `top` / `quiet` | 当前前台应用 / 字体修改队列是否空闲 |
| `recovery` | 0 普通运行，1 已观察到活力指标，2 等待安全下架，3 等待回收 |
| `last_idle_wait` | 上一次 owner 空闲检查：0 通过，1 重入，2 launcher 回调忙，3 原生快照槽占用，4 滚动标志，5 native 渲染/任务忙，6 控制中心，7 通知中心，8 小部件 |
| `surf_refs/moving/dirty/swapped/busy` | 三个 panel 的引用、移动、脏、换图、回调忙状态；bit0 控制中心，bit1 通知，bit2 小部件 |
| `cleanup/cleanup_armed` | 尚有回收工作 / 已安排在刷新边界处理 |
| `observer/root/root_sub/frame52` | 刷新观察器、已绑定表盘根、根订阅、刷新 START 累计次数 |
| `input/out0/out1/retired/out_retired` | 模块持有的快照输入、两张输出和延迟退役状态 |
| `battery_obj/buffer/mask/refs` | 电池图标资源；refs 的 bit0 根、bit1 状态条、bit2 遮罩根 |
| `natidle/idle_error/display/layer` | 当前 native 空闲结果、阻塞原因、显示对象和层链头 |
| `wf_slot/wf_current` | 上次采集查询的表盘根 / 当前固件槽中的根 |
| `gpu/capture_rc` | 最近一次 GPU fence / 原生快照结果 |

`last_idle_wait` 为缓存，不会为了显示诊断再调用对象树/手势 API。
诊断只读取状态和已验证的固定内存位置，不推进回收、不采集快照、不写文件。
按所有数值均为 10 位十进制计算，C 文本最大 1043 字节（含 NUL），小于调用方
1536 字节缓冲，独立模糊调试页仍使用可滚动的完整文本。

## 历史修复记录（不代表本次现场根因）

适用 p67 / 3.101.043。上游实现原样保留在 `supervisor/corona_vendor/`（AGPL-3.0-only），
本文只记录 **Chaos 侧的持有权判定与自愈**，不改上游任何状态机。

## 现场症状

1. 开过两个应用再回主表盘，模糊**直接不显示**，重启才恢复。
2. 先开“完全透明背景”，再切模糊，然后同样触发一次 —— 之后**换什么档位都是透明的**。

## 根因一：原生 owner 被摘掉后没人重新接管

上游的三面（控制中心/通知/小部件）和 launcher 各自有一份「owner 绑定」，
由 `p67_surface_poll` / `rh_platform_backgrounds_poll` 在空闲时安装。
它们的移除走的是**对象 DELETE 事件和手势结束逻辑**，不经过 `commit()`。

于是可能出现：原生 owner 已经被摘掉（日志里 `owned=0`），而 Rust 侧
`CORONA_BG_ACTIVE` 仍然是 `1`、区域掩码也没变 ——
按现有代码，这一拍除了 `poll()` 什么都不做，`commit()` 永远不会重来。
上游的 poll 想自己重装，但它的前置条件（`P67_IMAGE_SLOT`、滚动标记、
descriptor 绑定）任何一个不满足就会持续早退。

**这就是“模糊不动了”，而且换档位也没用**：换档位只是改 `blur_zone` 掩码，
真正画像素的那条钩子根本没装回去。

### 修复：所有权看门狗

`corona_blur.c` 新增 `chaos_corona_backgrounds_owned()`（纯整数读，无 native 调用）：

```c
enabled == 0                      → 1（未启用，不算“丢失”）
launcher 区域是 blur 且
  没有 owner / draw_owner 不是当前 root → 0
  且没有已发布像素、也没在采集      → 0
任一面区域是 blur 但没有 owner    → 0
```

`background_home::corona_tick` 在主表盘上（`top_app_id()==HOME_APP_ID`）连续
**3 拍**读到 `owned==0` 就把 `CORONA_BG_ACTIVE` 归零，下一拍走完整接管流程重新
`commit(true, mask)`；重装后有 **40 拍（≈2s）冷却**，防止 commit / 丢失高频抖动。

## 根因二：固件“滚动进行中”标记卡住

`0x201102D9` 是 launcher 的手势标记：开始时置 1，由固件自己的滚动结束回调清零。
滑动途中切走应用等打断场景下它可能一直是 1，而上游 poll 第一行就是：

```c
if (load32(P67_IMAGE_SLOT) || P67_U8(0x201102d9u)) return;
```

于是 owner 永远装不回去 —— 与根因一叠加，表现就是“模糊死了”。

### 修复：只在确实空闲时清这一个字节

`chaos_corona_latch_rescue()`，在 `commit` 和 `poll` 两条路径上都跑：

```
标记为 1 且 p67_busy==0 且 P67_IMAGE_SLOT==0 且 p67_blur_native_idle()==1
连续 60 次调用（≈3s）→ 清零标记，并累加 latchclr 计数
```

`p67_blur_native_idle()` 检查渲染位、层任务链和 display 一致性。
渲染空闲不等于手势已经结束；历史自愈另结合 `p67_busy`、快照槽和持续等待判断，
其对真实手势中断的兼容性仍需设备验证。
`P67_IMAGE_SLOT` 非零时**不做**任何清理（宁可报错也不泄漏原生缓冲）。

## 根因三：“全透明”是旧引擎台账被污染

`background_surfaces::enable_transparent()` 会把根节点和子对象的 `BG_OPA` 写成 0，
并把原值记在 `SURFACES` 台账里。原来的 `restore()` 有两个洞：

1. 只还原 root 属于 4 个主目标面的条目；**通知聚合页**（独立 page 根）的条目被漏掉；
2. 还原后**不清账**。下一次 `add()` 命中同一条“既有条目”，而
   `ACTIVE==0` 时 `add()` 会用**当前值**覆盖 `original` ——
   当前值正是我们刚写回去的 0。于是“透明”被记成原样，之后换什么档位都透明。

### 修复

- `restore()` 把 page 根也纳入还原范围；对象已不在自己根下时直接丢台账；
- **写回原值后立刻把条目标成 `EMPTY`**，下一次 `add()` 重新采集原值。

## 顺带修掉的越界动作

`home_signal/home_create/home_delete` 在 Corona 持有场景时也会调旧引擎的 `unmount()`
（restore 旧台账、卸 `background_pages` 钩子、`SCENE.detach()`），
违反了 `background_home.rs` 里“状态 1/2 禁止 legacy 变更”的约定。
现在这三个回调统一走 `corona_owns_scene()` 门（暂停期允许，因为那时 corona 已完整下架）。

## 降级：不再无限停在“看着像坏了”

如果 `commit` 连续失败 100 拍（≈5s），或接管后持续抢不回来：

1. 已接管 → 正常下架序列（`commit(false,0)` → poll 到干净）；
2. 之后 **400 拍（≈20s）暂停**，期间完全交给旧引擎，状态行写
   “模糊引擎已暂停，系统背景生效中”；
3. 暂停结束自动重试一次接管。

## 怎么验证 / 怎么报问题

「主页 → 关于 → 连点 5 次 → ③ 调试信息」→ `-- blur --` 段：

```
-- blur --
mode 0 engine 1 v2 1
cov 1 mask 15 reg 15          ← 接管态 / 已生效掩码 / 期望掩码
hold 0 fail 0 lost 0          ← 暂停拍数 / 连续失败 / 丢所有权计数
enabled=1 owned=1 owner=…     ← C 侧状态机快照
draw=1 imgslot=0 latch=0
src=1 pend=0 arm=0 gate=10
try=… copy=… pub=… draws=…
surf=7 commit=… cfail=…
latchclr=… natidle=1
```

判读要点：

| 现象 | 含义 |
|---|---|
| `owned=0` 且 `lost` 在涨 | 原生 owner 反复被摘，看 `draw`/`owner` |
| `latch=1` 持续、`natidle=1` | 正是第二个根因，等 3 秒后应出现 `latchclr` 增加 |
| `src=0` 且 `pend=1` | 采集一直失败：表盘根不可用，看 `gate`（5=根注册 6=身份 7=COPY 8=publish） |
| `imgslot=1` | 原生快照槽没释放，这条我们不碰，只能重启 |
| `hold>0` | 正在降级，等它到期或看 `cfail` |
