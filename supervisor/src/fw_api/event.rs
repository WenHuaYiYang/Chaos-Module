//! 事件：eventbus 订阅与定时器
//!
//! 本文件是 `fw_api` 的子模块，调用方式仍是 `fw_api::名字`（父模块已 `pub use` 转出）。

use core::mem::transmute;
use super::*;

// ===========================================================================
// 8. eventbus / 定时器 / 其它
// ===========================================================================

/// `eventbus_subscribe(topic_desc, arg1, cb, udata)` → 订阅节点（常驻） 已验证
pub unsafe extern "C" fn eventbus_subscribe(topic: u32, arg1: u32, cb: Cb, udata: u32) -> u32 {
    let f: unsafe extern "C" fn(u32, u32, Cb, u32) -> u32 = transmute(0x0CAA_5079usize);
    f(topic, arg1, cb, udata)
}

/// `quick_guesture` topic 描述符（payload 16B，手势码 = payload+8 u16） 已验证
pub const TOPIC_QUICK_GUESTURE: u32 = 0x2CD4_64E4;

/// `lv_timer_create` 的地址按目标各自的**文件偏移**现算，不拿 VA 对手抄 VA。
/// 谁把哪一档的值抄成了另一档的，对应目标的构建当场失败。
#[cfg(not(feature = "fw-p65"))]
const _: () = assert!(super::fwaddr::LV_TIMER_CREATE == (0x004C_7ED0 + 0x0C0C_0000 + 1));

#[cfg(feature = "fw-p65")]
const _: () = assert!(super::fwaddr::LV_TIMER_CREATE == (0x0068_14D8 + 0x0C0C_0000 + 1));

/// `lv_timer_create(cb, period_ms, user_data)` 回调在 **UI 线程**执行 已验证
///
/// 这是把"重活"从事件回调/派发线程搬到 UI 线程的标准手段，也是**安全销毁对象的唯一时机**。
///
/// 重要: 这一支在两个目标上是两个不同地址（本档是 LVGL 跳转壳，p65 是另一段真身），
/// 值只能从 `fwaddr` 取 —— 在调用点写死字面量等于允许把一个目标的值带进另一个目标的
/// 构建，那种错在构建期完全安静，到设备上表现为整条节拍不跑或直接复位。
pub unsafe extern "C" fn timer_create(cb: Cb, period_ms: u32, udata: u32) -> u32 {
    let f: unsafe extern "C" fn(Cb, u32, u32) -> u32 =
        transmute(super::fwaddr::LV_TIMER_CREATE as usize);
    f(cb, period_ms, udata)
}

/// `lv_timer_set_period(timer, ms)` —— 改一个已存在定时器的周期, 下一拍生效。静态核对
///
/// 依据(逐字读): `0x0C16D544` 的本体就是 `str r1,[r0]` —— 周期在
/// `lv_timer_t+0x00`, 这一点由 `lv_timer_create`(`0x0C16D474`) 体内的
/// `str.w r8,[r0]`(period) / `str r6,[r0,#8]`(cb) / `str -1,[r0,#0x10]`(repeat_count) /
/// `+0x14` = flags(bit0 = paused) 对上。
/// 固件自己也在用: 息屏状态机里 4 个调用点(`0x0C15D802`/`0x0C15E1DC`/`0x0C15E562`
/// = reset→400ms→resume/`0x0C15E912`)在 100/400ms 之间切周期, 与本层的用法同形。
/// handler 每拍从 `[t]` 重读 period(`0x0C16D330`), 所以改了立刻生效、不需要重建定时器
/// (重建才是危险的: 旧句柄只能"遗弃", 见 watchface.rs `shake_arm` 那段并发处理)。
pub unsafe extern "C" fn timer_set_period(t: u32, ms: u32) {
    let f: unsafe extern "C" fn(u32, u32) = transmute(0x0C16_D545usize);
    f(t, ms)
}

/// 读回一个定时器的周期字段(`lv_timer_t+0x00`)。
/// 用途: **建完先验一次它真的是我们刚传进去的那个数**, 验过才允许改(见 watchface.rs)。
pub unsafe fn timer_period_of(t: u32) -> u32 {
    if t == 0 { return 0; }
    core::ptr::read_volatile(t as *const u32)
}
