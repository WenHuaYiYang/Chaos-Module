//! Third-level icon picker: home -> settings -> application icon.
//! 走 kit/checklist(单选 RADIO): 选项 = dynamic_icon 的图标名清单, 选中即切换。
use crate::page::*;
use crate::page::kit::checklist::{click as ck_click, render as ck_render, CheckSpec};
use crate::dynamic_icon;

pub(crate) struct AppIconPage;
pub(crate) static PAGE_APP_ICON: AppIconPage = AppIconPage;

struct AppIconCheck;
impl CheckSpec for AppIconCheck {
    fn count(&self) -> usize { dynamic_icon::NAMES.len() }
    unsafe fn label(&self, i: usize) -> *const u8 { dynamic_icon::name(i as u8).as_ptr() }
    fn trailing(&self) -> u32 { fw_api::trailing::RADIO }
    unsafe fn selected(&self, i: usize) -> bool { i == dynamic_icon::selected() as usize }
    unsafe fn choose(&self, i: usize) { dynamic_icon::select(i as u8); }
}

impl Page for AppIconPage {
    fn fill_title(&self, buffer: *mut u8) {
        unsafe { put_str(buffer, "应用图标\0".as_bytes()); }
    }
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { ck_render(&AppIconCheck, ctx).map(|_| ()) }
    }
    fn on_click(&self, index: usize) {
        unsafe { ck_click(&AppIconCheck, Pid::AppIcon as u32, index) }
    }
}
