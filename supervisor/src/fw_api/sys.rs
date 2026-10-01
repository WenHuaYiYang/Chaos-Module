//! 系统状态读取与服务：页/屏幕/表盘状态、亮度、表盘管理器、堆（第 7、8、9 节）
//!
//! 本文件是 `fw_api` 的子模块，调用方式仍是 `fw_api::名字`（父模块已 `pub use` 转出）。

use core::mem::transmute;
use super::*;
use crate::mem::{rd16, rd8};

// ===========================================================================
// 7. 表盘（watchface manager）
// ===========================================================================

/// `set_watchface_by_id(id)` **只做数据层**（遍历链表 strcmp → 置 in_use → 持久化 JSON）。
/// 它**不更新引擎指针**，单独调它永远不会换面。 已验证
pub unsafe extern "C" fn wf_set_by_id(id: Cp) -> i32 {
    let f: unsafe extern "C" fn(Cp) -> i32 = transmute(0x0CA9_5C31usize);
    f(id)
}

/// 堆分配（NuttX） 注意：**模块上下文里 malloc 可能返回 NULL**，优先用 BSS 缓冲
pub unsafe extern "C" fn heap_malloc(n: u32) -> u32 {
    let f: unsafe extern "C" fn(u32) -> u32 = transmute(0x0C1F_903Dusize);
    f(n)
}
// ===========================================================================
// 8. 亮度控制（brightness）
// ===========================================================================

/// 亮度全局状态结构 @ `0x2013F148`
///
/// | 偏移 | 字段 |
/// |------|------|
/// | +0x04 | auto_adjustment 开关 (u8) |
/// | +0x0C | 手动亮度值 (u32, 0-255 UI 标度) |
/// | +0x18 | full_power 标志 (u8) |
/// | +0x19 | 屏幕状态 (u8, 2=亮屏) |
pub const BRIGHT_STATE: u32 = 0x2013_F148;

/// `brightness_set_value(v)` — 手动设亮度 0-255（内部经 clamp_manual 钳到 0-600 硬件标度）静态核对
/// 地址必须 |1（Thumb bit0）：符号查找给出的是偶数代码地址，直接当函数指针会崩
pub unsafe fn brightness_set_value(v: u32) {
    let f: unsafe extern "C" fn(u32) = transmute(0x0CA6_F661usize);
    f(v);
}

/// 读当前目标亮度 `state[+0x10]`（硬件标度 nit; apply_brightness_smoothly/now 都写它）
pub unsafe fn brightness_get_target() -> u32 {
    let p = (BRIGHT_STATE + 0x10) as *const u32;
    core::ptr::read_volatile(p)
}

/// `brightness_set_auto_adjustment(on)` — 开/关自动亮度 静态核对
pub unsafe fn brightness_set_auto_adjustment(on: u32) {
    let f: unsafe extern "C" fn(u32) = transmute(0x0CA6_F6F9usize);
    f(on);
}

/// 读自动亮度开关 `state[+0x04]`
pub unsafe fn brightness_get_auto_adjustment() -> u32 {
    let p = (BRIGHT_STATE + 0x04) as *const u8;
    core::ptr::read_volatile(p) as u32
}

/// `brightness_set_full_power(on)` — 全功率模式（亮屏时强制 600）静态核对
pub unsafe fn brightness_set_full_power(on: u32) {
    let f: unsafe extern "C" fn(u32) = transmute(0x0CA6_F7D9usize);
    f(on);
}

/// `apply_brightness_smoothly(v)` — 直接渐变应用亮度值，**不经手动钳位** 静态核对
///
/// 自动亮度强光路径（1200/2000）走的就是这个函数，所以 >600 的值是合法的。
/// `brightness_set_value` 会经 `clamp_manual` 钳到 600，想突破手动上限必须用这个。
pub unsafe fn brightness_apply_raw(v: u32) {
    let f: unsafe extern "C" fn(u32) = transmute(0x0CA6_EF35usize);
    f(v);
}

// ===========================================================================
// 状态读取：屏幕 / 最上层页 / 表盘显示 / 持久化设置
// ===========================================================================

// ---------------------------------------------------------------------------
// 手势门控（与系统 handler 完全一致）
// ---------------------------------------------------------------------------
//
// 摇一摇只在"表盘在最前面"时生效，门控共三条（都由真机验证过）：
//   1. 屏幕亮着（`screen_is_on`）—— 挡住息屏与 AOD
//   2. 最上层页仍属 home 应用（`top_page_ids().0 == HOME_APP_ID`）—— 挡住设置与其它应用
//   3. 固件的"表盘在显示"标志非 0（`watchface_is_on`）—— 挡住 launcher
//
// 系统自己的手势 handler（`topic_quick_guesture_handler` 0x0C4BD0FC）另有一套四谓词
// （勿扰/睡眠、桩、前台有应用、最上层页非 home）。**注意它挡不住 launcher**：
// launcher 不是固件页（真机读数与表盘同为 top=16:0、页数 1），四谓词在 launcher 里
// 全为 0；固件自己的消费者就是这条链，所以固件若做摇一摇切表盘，在 launcher 里也会切。

/// MiWearScreen 状态结构（`apply_screen_state_change`(0x0CA702B8) 的 r4）。
/// 结构内 `+0x86A` = 当前屏幕状态，取值由该函数里的名字表定死：
///   **0 = OFF（息屏）/ 1 = AOD（息屏显示）/ 2 = ON（亮屏）**
/// 依据：`ldr.w r2,[<状态名表> + state*4]` → 表项依次为 "OFF"/"AOD"/"ON"。 静态核对
pub const SCREEN_MGR: u32 = 0x2013_F368;

/// 读当前屏幕状态：0=OFF 1=AOD 2=ON
pub unsafe fn screen_state() -> u32 {
    rd8((SCREEN_MGR + 0x86A) as *const u8) as u32
}

/// 屏幕是否亮着（ON）—— 摇一摇的门控条件之一
pub unsafe fn screen_is_on() -> bool {
    screen_state() == 2
}

/// activitymanager 的页管理器结构（`activitymanager_get_top_page_root` 读它）。
/// 字段：`+0` u16 页数, `+2` u16 最上层页索引+1。 静态核对
pub const PAGE_MGR: u32 = 0x2013_EDE4;

/// 取页管理器中第 idx 个页面的**描述符**（`0x0CA6A7CC(manager, idx)`）。
pub const FW_PAGE_BY_INDEX: u32 = 0x0CA6_A7CD;

// ---- 页描述符字段偏移 ----
// 依据：对固件镜像的静态核对（036 与 043 两版页描述符定义逐字段一致，已 diff 核对）。
// 其中 page_kind 与 async_destroy_state 这两个字段，只从调用点反汇编看不出来。
pub const DESC_PAGE_ID: u32 = 0x14;        // u16 导航复合键低半
pub const DESC_APP_ID: u32 = 0x16;         // u16 导航复合键高半（手电筒实测 26）
pub const DESC_ASYNC_DESTROY: u32 = 0x24;  // u32 非 0 = 走"排队销毁"，树随时会被删
pub const DESC_LIFECYCLE: u32 = 0x28;      // u8 注册默认 4，随 create/resume/pause/destroy 变
pub const DESC_KIND: u32 = 0x2a;           // u8 页种类，见下
pub const DESC_ROOT: u32 = 0x30;           // 根对象；on_create 建、destroy 递归删后清 0

/// `page_kind` 取值：注册默认 0 会被填成 2（原生页）；**快应用页是 3**。
/// 2 这一条逐字核对过：`activitymanager_page_register`(0x0CA6AD10) 里
/// `0x0CA6AD64 ldrb r3,[r0,#0x2a]` + `cmp r3,#0` + `beq 0x0CA6ADF6`，而
/// `0x0CA6ADF6: movs r3,#2; strb.w r3,[r0,#0x2a]` —— 没显式给 kind 的页一律变 2。
/// 3 那一条推断待验：写它的那段代码在已被厂商清零的模块里，镜像内可读代码只有
/// 注册默认这一处，所以按"只认 2"来用：
/// **不等于 2 的描述符一律不碰它的对象树**（快应用、脏描述符、未知种类都在外面）。
pub const PAGE_KIND_NATIVE: u32 = 2;

/// 页栈深度 = 当前**活着**的页数（`0x0CA6A7CC` 这个访问器自己用的上界就是它：
/// `0x0CA6A7D2 ldrh r5,[r0,#2]` + `cmp r5,r1` + `bls` 越界直接返回 0）。
/// 索引 0 = 栈底（home/表盘），最深的一层 = 最上层页。静态核对
pub unsafe fn page_stack_depth() -> u32 {
    rd16((PAGE_MGR + 2) as *const u16) as u32
}

/// 页栈里第 idx 页的描述符（0 = 越界或取不到）。与 `top_page_desc` 同一个访问器，
/// 只是不替调用方挑索引 —— 需要一次扫遍所有活页时用它把整条栈摊开。
pub unsafe fn page_desc_by_index(idx: u32) -> u32 {
    let f: unsafe extern "C" fn(u32, u32) -> u32 = transmute(FW_PAGE_BY_INDEX as usize);
    f(PAGE_MGR, idx)
}

/// 最上层页的描述符指针（0 = 取不到页/取不到描述符）。
/// 逻辑与固件 `activitymanager_get_top_page_root`(0x0CA6CF4C) 逐字同形，只是停在
/// 描述符不取根：页数/索引任一为 0 就返回 0，再 `FW_PAGE_BY_INDEX(mgr, idx-1)`。
/// 需要同时看 kind/lifecycle/root 的调用方一律用这个，避免分两次取到不一致的快照。
pub unsafe fn top_page_desc() -> u32 {
    let mgr = PAGE_MGR;
    let cnt = rd16(mgr as *const u16) as u32;
    let idx1 = rd16((mgr + 2) as *const u16) as u32;
    if cnt == 0 || idx1 == 0 {
        return 0;
    }
    let f: unsafe extern "C" fn(u32, u32) -> u32 = transmute(FW_PAGE_BY_INDEX as usize);
    f(mgr, idx1 - 1)
}

/// 最上层页面的 (app_id, page_id)；取不到返回 (0xFFFF, 0xFFFF)
pub unsafe fn top_page_ids() -> (u32, u32) {
    let desc = top_page_desc();
    if desc == 0 {
        return (0xFFFF, 0xFFFF);
    }
    let pid = rd16((desc + DESC_PAGE_ID) as *const u16) as u32;
    let aid = rd16((desc + DESC_APP_ID) as *const u16) as u32;
    (aid, pid)
}

/// 最上层页的 app_id（取不到返回 0xFFFF）。摇一摇门控用这个就够, 不必解整对 id。
pub unsafe fn top_app_id() -> u32 {
    top_page_ids().0
}

/// 表盘所属的 home 应用 app_id（真机读数：表盘页与 launcher 都是 top=16:0，
/// 设置=22:0，Chaos 自己=200:0）。实测
pub const HOME_APP_ID: u32 = 16;

/// 表盘显示状态三个具名状态量（固件自己维护，用于判断"表盘在不在显示"）。
///
/// - `watchface_on`  @ 0x2010C980：`get_watchface_on`(0x0C4BF560) 读它；
///   写它的微型 setter 0x0C4BF54C 被 `on_destroy`/`on_pause` 路径调用（置 0）。
/// - `watchface_refresh` @ 0x2010C981：`set_watchface_refresh`(0x0C4BF51C) 写它，
///   调用点在 0x0C5F2770(on_destroy) / 0x0C5F2974(on_pause) / 0x0C5F2BA0(screen_state_changed_cb)。
/// - 可见标志：`watchface_config_on_watchface_visible_changed`(0x0CA7C548) 把可见性写进
///   `*(u32*)0x2013FE6C` 这个结构的 `+0x6C`（`+0x68` 是当前表盘对象、`+0x6D` 是伴随标志）。
/// 三者语义都是"表盘是否正在显示/刷新"。 静态核对
pub const WATCHFACE_ON: u32 = 0x2010_C980;
/// 姊妹字节 `watchface_refresh` @ 0x2010C981（`set_watchface_refresh` 0x0C4BF51C 写），
/// 与 on 同步变化，仅作旁证，代码里不用。

/// 读"表盘是否在显示"标志 `*(u8*)0x2010C980`。
///
/// **真机判别成立**（实测）：表盘页摇一摇读到 1，launcher 里读到 0，
/// 设置/其它应用同理。这正是"当前是不是表盘在最前面"的判据。
pub unsafe fn watchface_is_on() -> u32 {
    rd8(WATCHFACE_ON as *const u8) as u32
}

/// 持久化设置读取：`persist_get(const char *key, int32_t def)`。
/// 依据：`screen_state_changed_cb`(0x0C4BCB7A) 用它读 `persist.switch_guestre_state`，
/// 取到 0 时打印 "guesture is off" —— 说明**固件自带一套摇一摇开关**。 静态核对
pub const FW_PERSIST_GET: u32 = 0x0C3D_AE95;

/// 读一个 persist 设置（失败/不存在返回 def）
pub unsafe fn persist_get(key: *const u8, def: i32) -> i32 {
    let f: unsafe extern "C" fn(*const u8, i32) -> i32 = transmute(FW_PERSIST_GET as usize);
    f(key, def)
}

/// `persist.switch_guestre_state` —— 固件自带摇一摇功能的开关
pub static KEY_SWITCH_GUESTRE: [u8; 29] = *b"persist.switch_guestre_state\0";


// ===== 链表原语(固件 lv_ll; 从 vg_font_create_core/lvx 字体遍历反汇编取形) =====

/// `0x0C589050(list_head*)` 返回首个节点(空=0)。 静态核对
/// (0x0C85F524/0x0C85F6CC 的固件遍历循环都调它)
pub unsafe extern "C" fn fw_list_head(l: u32) -> u32 {
    let f: unsafe extern "C" fn(u32) -> u32 = transmute(0x0C58_9051usize);
    f(l)
}

/// `0x0C588810(list_head*, node*)` 返回下一节点(到尾=0)。 静态核对
pub unsafe extern "C" fn fw_list_next(head: u32, node: u32) -> u32 {
    let f: unsafe extern "C" fn(u32, u32) -> u32 = transmute(0x0C58_8811usize);
    f(head, node)
}

// ===========================================================================
// 11. 应用注册表与桌面图标
// ===========================================================================
//
// 注册表是一根**带哨兵的循环链表**，逐字取自 app_lookup(0x0CA69934)：
//   0x0CA6993A `ldr r1,[r3]`        r1 = *(APP_REGISTRY)      = 哨兵
//   0x0CA6993C `ldr r3,[r1,#4]`     r3 = 第一个节点(链表指针在节点 +0x04)
//   0x0CA6993E `cmp r1,r3 / beq`    回到哨兵 = 空表/到尾
//   0x0CA6994A `ldrh r2,[r3,#0x10]` 节点 +0x10 u16 = app_id
// 节点字段(由 app_install 0x0CA6A380..0x0CA6A3A6 的逐字段 strdup 与
// launcher 的读取点共同确定)：+0x08 包名 / +0x0C 图标路径 / +0x10 u16 app_id /
// +0x14 其它字符串。`app_install` 对 +0x08/+0x0C/+0x14 都调 `str_dup_x`，
// 注销路径(0x0CA6A484..0x0CA6A498)再逐个 `free` —— 所以这些字段是**堆上字符串、
// 归注册表所有**：我们替换 +0x0C 时必须交回同一分配器的指针(见 fw_str_dup)，
// 否则固件注销时会对我们的 .rodata 地址调 free = 堆损坏。

/// 注册表哨兵的存放位置：`head = *(APP_REGISTRY)`。 静态核对(app_lookup 体内唯一取址)
pub const APP_REGISTRY: u32 = 0x200E_B640;
pub const APPN_ICON: u32 = 0x0C;     // 节点: 图标路径字符串指针
pub const APPN_ID: u32 = 0x10;       // 节点: u16 app_id

/// `str_dup_x(s)` = strlen + malloc(len+1) + memcpy，返回堆指针(0=失败)。
/// 0x0C1F06B4: `bl 0xc5885b0`(strlen) `bl 0xc1f903c`(malloc) `bl 0xc588db0`(memcpy)。
/// 注册表所有字符串都经它分配，所以换 +0x0C 只认这个来源的指针。 静态核对
pub unsafe extern "C" fn fw_str_dup(s: Cp) -> u32 {
    let f: unsafe extern "C" fn(Cp) -> u32 = transmute(0x0C1F_06B5usize);
    f(s)
}

/// `free(p)` —— 与 `fw_str_dup` 同一分配器。0x0C1F8FF8(app_install 注销路径实参)。
/// 静态核对
pub unsafe extern "C" fn fw_free(p: u32) {
    let f: unsafe extern "C" fn(u32) = transmute(0x0C1F_8FF9usize);
    f(p)
}

/// `img_free_by_path(path)` —— 按路径串释放 LVGL 图像缓存。0x0C5890D0 是 thunk
/// (`ldr.w pc,[pc,#0]` -> 0x1C0596D5，函数体在已清零的 0x1C 模块，不可反汇编)。
/// 用法照抄固件自己那处：hidden_and_show 0x0C51421E `ldr r0,[r4,#0xc]` ->
/// 0x0C514222 `bl 0xc5890d0`，即**传记录当前持有的路径**，且必须在改指针之前调。
/// 静态核对(调用点已核；函数体在已清零模块里，不可证)
pub unsafe extern "C" fn fw_img_free_by_path(path: u32) {
    let f: unsafe extern "C" fn(u32) = transmute(0x0C58_90D1usize);
    f(path)
}

/// 桌面记录链表头(当前页)。`fw_list_head(LAUNCHER_DESKTOP_LIST) == 0` 表示此刻没有
/// 桌面记录(例如我们正待在自己的应用里, 桌面页没在用) —— 这时 `launcher_refresh_app`
/// 会在 0x0C513BD0 `cmp r0,#0 / beq` 直接早退: 只换指针不刷新界面 —— 换图标时
/// "恢复原图标有时点了没反应"就是这个原因。换指针本身仍然有效: 下次进桌面时 0x0C513E24 会清空并按注册链重建
/// 记录, 那时重读到新路径。三个读点: rebuild_all 0x0C51344E / hidden_and_show
/// 0x0C514212 / launcher_refresh_app 0x0C513BCA。 静态核对
pub const LAUNCHER_DESKTOP_LIST: u32 = 0x200D_0CBC;

/// `launcher_refresh_app(app_id)` —— 桌面单应用刷新，0x0C513BC0。逐字读法：
///   0x0C513BE8 在桌面记录链(0x200D0CBC)里按 `ldrh [rec]` == app_id 找记录
///   0x0C513BEE..BF6 `[[APP_REGISTRY+0x18]+4](app_id)` = 注册表 lookup
///   0x0C513BF8/FA `desc+0x0C -> rec+0x0C`            = **重读图标路径**
///   0x0C513C06 `bl 0xc517fc8`                        = 释放旧图片对象 + 重新加载
///   0x0C513C60 `bl 0xc513440`                        = launcher_rebuild_all
/// 即改完注册表 +0x0C 之后调它，桌面就会用新路径重画。 静态核对
pub unsafe extern "C" fn launcher_refresh_app(app_id: u32) {
    let f: unsafe extern "C" fn(u32) = transmute(0x0C51_3BC1usize);
    f(app_id)
}
