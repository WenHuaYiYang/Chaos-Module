//! 页37：调试信息（纯文本整屏，不建卡片行）。
//!
//! 内容由 `crate::debug_info::refresh()` 生成；本页只负责把它铺成一个可滚动的文本块。
//! 入口在调试模式下的“关于”页（页18）——“③ 调试信息”。

use crate::page::{Page, PageCtx, PageError, PageResult};
use crate::put_str;
use crate::fw_api;

pub(crate) struct DebugInfoPage;
pub(crate) static PAGE_DEBUGP: DebugInfoPage = DebugInfoPage;

impl Page for DebugInfoPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "调试信息\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            let text = crate::debug_info::refresh();
            let label = fw_api::label_create(ctx.content);
            if label == 0 {
                return Err(PageError::RowFailed(0));
            }
            fw_api::style_apply(label, crate::FW_STYLE_HR_NAME, 255, 0);
            fw_api::label_set_text(label, text);
            // 文本可能超过一屏，给足高度让 content 原生滚动承载。
            fw_api::obj_set_size_wh(label, 320, 640);
            fw_api::obj_align(label, fw_api::align::TOP_MID, 0, 12);
            Ok(())
        }
    }

    fn on_click(&self, _idx: usize) {}
}
