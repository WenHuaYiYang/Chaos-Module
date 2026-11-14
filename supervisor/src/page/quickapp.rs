//! 主页 -> 快应用动画。
use crate::page::*;
use crate::page::kit::{MenuSpec, RowModel, ToggleDef};

pub(crate) struct QuickappPage;
pub(crate) static PAGE: QuickappPage = QuickappPage;
static OPEN_ANIMATION: ToggleDef = ToggleDef {
    get: crate::quickapp_animation::enabled,
    toggle: crate::quickapp_animation::toggle,
    status: Some(crate::quickapp_animation::status),
};
static ROWS: &[RowModel] = &[
    RowModel::toggle("快应用开启动画\0".as_bytes(), &OPEN_ANIMATION),
];

impl MenuSpec for QuickappPage {
    fn rows(&self) -> &'static [RowModel] { ROWS }
}
impl Page for QuickappPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "快应用动画\0".as_bytes()); }
    }
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { frame::build(self, ctx, ctx.title) }
    }
    fn is_switch_row(&self, idx: usize) -> bool { kit::menu_is_switch(self, idx) }
    fn on_click(&self, idx: usize) { unsafe { kit::menu_click(self, idx) } }
    fn on_switch(&self, idx: usize) { unsafe { kit::menu_switch(self, idx) } }
}
