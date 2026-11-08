# Corona 模糊：失效与“全透明”的两条根因与自愈机制

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

`p67_blur_native_idle()` 已经检查渲染位、层任务链和 display 一致性，
真实手势期间必然为 0，所以这条自愈不会打断滑动。
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
