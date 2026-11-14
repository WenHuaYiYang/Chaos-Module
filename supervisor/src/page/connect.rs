//! 设置 -> ChaosConnect.
use crate::page::*;
use crate::page::kit::{MenuSpec, RowModel, ToggleDef};

pub(crate) struct ConnectPage;
pub(crate) static PAGE: ConnectPage = ConnectPage;
unsafe fn toggle() { crate::bridge_cfg::toggle(); }
static ENABLED: ToggleDef = ToggleDef {
    get: crate::bridge_cfg::enabled, toggle, status: None,
};
static ROWS: &[RowModel] = &[
    RowModel::toggle("启用\0".as_bytes(), &ENABLED),
    RowModel::nav("ChaosConnect日志\0".as_bytes(), Pid::ConnectLog as u32),
    RowModel::note("通过配套插件浏览、读取和管理文件。\n操作日志仅保存在内存，支持刷新、翻页和清空，重启后清空。\0".as_bytes()),
];
impl MenuSpec for ConnectPage {
    fn rows(&self) -> &'static [RowModel] { ROWS }
}
impl Page for ConnectPage {
    fn fill_title(&self, buf: *mut u8) { unsafe { put_str(buf, b"ChaosConnect\0"); } }
    fn render(&self, ctx: &PageCtx) -> PageResult { unsafe { frame::build(self, ctx, ctx.title) } }
    fn is_switch_row(&self, idx: usize) -> bool { kit::menu_is_switch(self, idx) }
    fn on_click(&self, idx: usize) { unsafe { kit::menu_click(self, idx) } }
    fn on_switch(&self, idx: usize) { unsafe { kit::menu_switch(self, idx) } }
}
