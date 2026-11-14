//! 刷新总线: 每页一位的重建请求位图 + 每页 epoch。
//!
//! 取代旧的单槽 RENDER_REQ —— 单槽后写覆盖先写, 两个页面同时请求重建时先来的那个
//! 会被冲掉。位图版本: mark 可以从点击回调与后台跑批两侧随意调用, 谁都不会丢。
//! 消费在主定时器(lv_timer 上下文, 销毁/重建对象树安全): 每拍最多消费一个页,
//! 前台页优先, 其余请求按页号保留到后续的拍。
//!
//! epoch: 每次整页重建自增。页面 tick 里做原地写行(row_update)之前可以比对
//! epoch, 识别"句柄是重建前的旧树"这类竞态 —— 行句柄在重建开头统一清零,
//! epoch 是这层保护之外的兜底。

use crate::*;

/// 请求级别: 整页重建 / 只刷数值(页面 tick 原地刷新, 不动对象树)。
pub(crate) const D_FULL: u8 = 0;
pub(crate) const D_VALUES: u8 = 1;

static mut DIRTY: [u32; 2] = [0; 2];      // bit i = 页 i 有待消费请求
static mut DIRTY_FULL: [u32; 2] = [0; 2]; // bit i = 页 i 挂的是整页重建(Full 盖过 Values)
static mut EPOCH: [u16; PAGE_COUNT] = [0; PAGE_COUNT];

#[inline(always)]
fn bitw(pid: usize) -> (usize, u32) { (pid >> 5, 1u32 << (pid & 31)) }

/// 登记某页的刷新请求。可在事件派发回调里调用(只置位, 不碰对象树)。
/// pid 越界一律丢弃(与旧 render_req 的行为一致)。
pub(crate) fn mark(pid: u32, kind: u8) {
    let pid = pid as usize;
    if pid >= PAGE_COUNT { return; }
    unsafe {
        let (w, b) = bitw(pid);
        st_wr!(DIRTY[w], st_rd!(DIRTY[w]) | b);
        if kind == D_FULL {
            st_wr!(DIRTY_FULL[w], st_rd!(DIRTY_FULL[w]) | b);
        }
    }
}

/// 还有没有待消费的刷新请求(节拍自适应用: 有请求就保持快节拍)。
pub(crate) fn any_pending() -> bool {
    unsafe { (st_rd!(DIRTY[0]) | st_rd!(DIRTY[1])) != 0 }
}

/// 消费一个请求: 前台页优先, 其余按页号升序保留到后续的拍。
/// 返回 (页号, 级别); None = 队列空。
pub(crate) fn take_next() -> Option<(u32, u8)> {
    unsafe {
        let fg = st_rd!(FG_PAGE) as usize;
        let mut pass = 0usize;   // 0 = 只看前台页, 1 = 扫其余页号
        while pass < 2 {
            let mut pid = if pass == 0 { fg } else { 0 };
            while pid < PAGE_COUNT {
                if pass == 0 || pid != fg {
                    let (w, b) = bitw(pid);
                    if st_rd!(DIRTY[w]) & b != 0 {
                        let full = st_rd!(DIRTY_FULL[w]) & b != 0;
                        st_wr!(DIRTY[w], st_rd!(DIRTY[w]) & !b);
                        if !full {
                            st_wr!(DIRTY_FULL[w], st_rd!(DIRTY_FULL[w]) & !b);
                        }
                        return Some((pid as u32, if full { D_FULL } else { D_VALUES }));
                    }
                }
                pid += 1;
            }
            pass += 1;
        }
        None
    }
}

/// 当前 epoch(页 tick 比对用): 返回值与上次读到的不一样 = 这期间整页重建过。
/// (行句柄在重建开头统一清零已是第一层保护; 这是留给页面的显式口径。)
#[allow(dead_code)]
pub(crate) fn epoch_of(pid: u32) -> u16 {
    let pid = pid as usize;
    if pid >= PAGE_COUNT { return 0; }
    unsafe { read_volatile(core::ptr::addr_of!(EPOCH[pid])) }
}

/// 整页重建完成后自增(框架在 render_page 建树成功时调用)。
pub(crate) fn bump(pid: u32) {
    let pid = pid as usize;
    if pid >= PAGE_COUNT { return; }
    unsafe {
        let p = core::ptr::addr_of_mut!(EPOCH[pid]);
        write_volatile(p, read_volatile(p).wrapping_add(1));
    }
}
