//! 页23：通用键盘页面包装，输入逻辑在 `crate::keyboard`。

use crate::page::*;
use crate::page::Page;

// ===== 页23: 通用英文数字键盘 =====
pub(crate) struct KeyboardPage;
pub(super) static PAGE_KEYBOARDP: KeyboardPage = KeyboardPage;

impl Page for KeyboardPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, crate::keyboard::title()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            if crate::keyboard::render(ctx.content) {
                Ok(())
            } else {
                Err(PageError::RowFailed(0))
            }
        }
    }

    fn on_click(&self, _idx: usize) {}
}
