//! 页37：调试信息（纯文本整屏，不建卡片行）。
//!
//! 内容由 `crate::debug_info::refresh()` 生成；本页只负责把它铺成一个可滚动的文本块。
//! 入口在调试模式下的“关于”页（页18）——“③ 调试信息”。

use crate::page::{Page, PageCtx, PageError, PageResult};
use crate::put_str;
use crate::fw_api;

pub(crate) struct DebugInfoPage;
pub(crate) static PAGE_DEBUGP: DebugInfoPage = DebugInfoPage;

pub(crate) struct BlurDebugPage;
pub(crate) static PAGE_BLUR_DEBUG: BlurDebugPage = BlurDebugPage;

unsafe fn render_text(ctx: &PageCtx, text: *const u8) -> PageResult {
    let label = fw_api::label_create(ctx.content);
    if label == 0 { return Err(PageError::RowFailed(0)); }
    // 此固件不自动撑高长文本。按换行和保守字宽计入折行，避免末尾被裁掉。
    let mut rows = 1u32;
    let mut width = 0u32;
    let mut i = 0usize;
    while i < 4095 {
        let c = core::ptr::read_volatile(text.add(i));
        if c == 0 { break; }
        if c == b'\n' { rows += 1; width = 0; }
        else if c & 0xC0 != 0x80 {
            let w = if c < 128 { 24 } else { 36 };
            if width + w > 320 { rows += 1; width = 0; }
            width += w;
        }
        i += 1;
    }
    fw_api::style_apply(label, crate::FW_STYLE_HR_NAME, 255, 0);
    fw_api::obj_set_size_wh(label, 320, (rows + 2) * 44);
    fw_api::label_set_text(label, text);
    fw_api::obj_align(label, fw_api::align::TOP_MID, 0, 12);
    Ok(())
}

impl Page for BlurDebugPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "模糊调试\0".as_bytes()); }
    }
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { render_text(ctx, crate::debug_info::blur_refresh()) }
    }
    fn on_click(&self, _: usize) {}
}

impl Page for DebugInfoPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "调试信息\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            let text = crate::debug_info::refresh();
            render_text(ctx, text)
        }
    }

    fn on_click(&self, _idx: usize) {}
}
