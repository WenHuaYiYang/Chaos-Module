//! MsgSlot: 统一的成功/失败反馈。
//!
//! 一处单槽(同一时刻只有一条反馈), 固定 64B 文案 + 语气 + 到期拍。挂在页 content
//! 底部安全区上方; 到期由页面 tick 里的一句 `feedback::tick(pid)` 清除。
//! 文字没变不重写(label_set_text 每次都失效重绘)。
//!
//! 语气配色只取 theme.rs 里的 token, 不引入新色值。

#![allow(dead_code)]

use crate::*;

pub(crate) const TONE_INFO: u8 = 0;
pub(crate) const TONE_SUCCESS: u8 = 1;
pub(crate) const TONE_WARNING: u8 = 2;
pub(crate) const TONE_DANGER: u8 = 3;

const MSG_CAP: usize = 64;

static mut MSG_LABEL: u32 = 0;
static mut MSG_PID: u32 = 0xFFFF_FFFF;
static mut MSG_TEXT: [u8; MSG_CAP] = [0; MSG_CAP];
static mut MSG_TONE: u8 = TONE_INFO;
static mut MSG_LEFT: u32 = 0;          // 剩余拍数
static mut MSG_TONE_SHOWN: u8 = 0xFF;  // 已上屏的语气(变了才重染色)

fn tone_color(tone: u8) -> u32 {
    match tone {
        TONE_SUCCESS => T::SUCCESS,
        TONE_WARNING => T::WARNING,
        TONE_DANGER => T::DANGER,
        _ => T::SECONDARY,
    }
}

/// 显示一条反馈。同拍重复显示同文案只续期不重写。
/// pid 必须是当前前台页 —— 反馈标签是建在那一页 content 上的。
pub(crate) unsafe fn show(pid: u32, text: &[u8], tone: u8, ticks: u32) {
    if st_rd!(FG_PAGE) != pid { return; }
    let pid = pid as usize;
    if pid > MAX_PID || PAGES[pid].built == 0 { return; }
    // 文案截断到 63 字节 + NUL
    let mut buf: [u8; MSG_CAP] = [0; MSG_CAP];
    let n = text.len().min(MSG_CAP - 1);
    let mut i = 0usize;
    while i < n { buf[i] = text[i]; i += 1; }
    let changed = {
        let cur = core::ptr::addr_of!(MSG_TEXT) as *const u8;
        let diff = read_volatile(cur) != buf[0];
        let mut j = 0usize;
        while j <= n {
            write_volatile((core::ptr::addr_of_mut!(MSG_TEXT) as *mut u8).add(j), buf[j]);
            j += 1;
        }
        diff
    };
    st_wr!(MSG_PID, pid as u32);
    st_wr!(MSG_TONE, tone);
    st_wr!(MSG_LEFT, ticks.max(1));
    let lb = st_rd!(MSG_LABEL);
    if lb == 0 {
        let c = PAGES[pid].content;
        if c == 0 { return; }
        let nlb = fw_api::label_create(c);
        if nlb == 0 { return; }
        fw_api::style_apply(nlb, crate::FW_STYLE_HR_NAME, 255, 0);
        fw_api::obj_set_width(nlb, 320);
        fw_api::label_set_text(nlb, core::ptr::addr_of!(MSG_TEXT) as *const u8);
        fw_api::obj_align(nlb, fw_api::align::BOTTOM_MID, 0, -(T::SAFE_BOTTOM + T::SP_2));
        st_wr!(MSG_LABEL, nlb);
        fw_api::obj_style_color_a70(nlb, tone_color(tone), 0);
        st_wr!(MSG_TONE_SHOWN, tone);
        return;
    }
    if changed {
        fw_api::label_set_text(lb, core::ptr::addr_of!(MSG_TEXT) as *const u8);
    }
    if st_rd!(MSG_TONE_SHOWN) != tone {
        fw_api::obj_style_color_a70(lb, tone_color(tone), 0);
        st_wr!(MSG_TONE_SHOWN, tone);
    }
}

/// 页面 tick 里调: 到期清除。返回 true = 还有反馈在显示(页面 needs_tick 用)。
pub(crate) unsafe fn tick(pid: u32) -> bool {
    if st_rd!(MSG_PID) != pid { return false; }
    let left = st_rd!(MSG_LEFT);
    if left == 0 { return false; }
    st_wr!(MSG_LEFT, left - 1);
    if left - 1 == 0 {
        let lb = st_rd!(MSG_LABEL);
        if lb != 0 { fw_api::obj_delete(lb); }
        st_wr!(MSG_LABEL, 0);
        st_wr!(MSG_PID, 0xFFFF_FFFF);
        st_wr!(MSG_TONE_SHOWN, 0xFF);
        return false;
    }
    true
}

/// 是否有反馈在显示(本页)。
pub(crate) unsafe fn pending(pid: u32) -> bool {
    st_rd!(MSG_PID) == pid && st_rd!(MSG_LEFT) != 0
}
