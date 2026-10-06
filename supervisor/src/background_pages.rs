//! 通知页面生命周期接管。
//!
//! 通知聚合页和详情页不是 home 描述符的子树，它们各自有页面描述符。
//! 先调用原始 on_create，再把固件传入的真实根交给背景对象事件链，覆盖晚建卡片。

use crate::fw_api;
use crate::mem::{rd32, safe_ptr};
use crate::{st_rd, st_wr};

static mut HOOKED: [u32; 2] = [0; 2];

// 详情页的中间内容区由固件自己的黑色容器绘制；对它注入全屏背景会
// 把该容器的合成层改成黑块。因此只接管通知聚合页，详情页保持原样。
const ENABLED: [bool; 2] = [true, false];

unsafe extern "C" fn page_detail_create(page: u32, root: u32, start: u32) -> i32 {
    page_create(1, page, root, start)
}

unsafe extern "C" fn page_aggregation_create(page: u32, root: u32, start: u32) -> i32 {
    page_create(0, page, root, start)
}

unsafe fn page_create(slot: usize, page: u32, root: u32, start: u32) -> i32 {
    let result = fw_api::background_page_invoke(
        fw_api::BACKGROUND_NOTIFY_CREATE_ORIGINALS[slot], page, root, start);
    if crate::background_home::mode() != 0 {
        let descriptor_root = if safe_ptr(page) {
            rd32((page + 0x30) as *const u32)
        } else { 0 };
        let actual = if safe_ptr(root) { root } else { descriptor_root };
        if safe_ptr(actual) {
            crate::background_surfaces::set_page_root(slot, actual);
        }
    }
    result
}

fn callbacks() -> [u32; 2] {
    [page_aggregation_create as *const () as u32,
     page_detail_create as *const () as u32]
}

/// 按页面复合键校验后替换两个通知页的 on_create 槽。
pub(crate) unsafe fn install() -> bool {
    let callbacks = callbacks();
    let mut any = false;
    for slot in 0..2 {
        if !ENABLED[slot] { continue; }
        let desc = fw_api::BACKGROUND_NOTIFY_DESCS[slot];
        if rd32((desc + 0x14) as *const u32) != fw_api::BACKGROUND_NOTIFY_KEYS[slot] {
            continue;
        }
        let address = desc + fw_api::BACKGROUND_NOTIFY_CREATE_SLOT;
        if st_rd!(HOOKED[slot]) == 0 {
            if rd32(address as *const u32) != fw_api::BACKGROUND_NOTIFY_CREATE_ORIGINALS[slot] {
                continue;
            }
            core::ptr::write_volatile(address as *mut u32, callbacks[slot]);
            st_wr!(HOOKED[slot], 1);
        }
        let existing = rd32((desc + 0x30) as *const u32);
        if safe_ptr(existing) {
            crate::background_surfaces::set_page_root(slot, existing);
        }
        any = rd32(address as *const u32) == callbacks[slot] || any;
    }
    any
}

pub(crate) unsafe fn uninstall() {
    let callbacks = callbacks();
    for slot in 0..2 {
        if !ENABLED[slot] { continue; }
        let desc = fw_api::BACKGROUND_NOTIFY_DESCS[slot];
        let address = desc + fw_api::BACKGROUND_NOTIFY_CREATE_SLOT;
        if rd32(address as *const u32) == callbacks[slot] {
            core::ptr::write_volatile(
                address as *mut u32,
                fw_api::BACKGROUND_NOTIFY_CREATE_ORIGINALS[slot]);
        }
        st_wr!(HOOKED[slot], 0);
    }
    crate::background_surfaces::clear_page_roots();
}

pub(crate) unsafe fn pending() -> bool {
    st_rd!(HOOKED).iter().any(|value| *value != 0)
        && crate::background_surfaces::pending()
}

pub(crate) unsafe fn hooked() -> bool {
    st_rd!(HOOKED).iter().any(|value| *value != 0)
}
