//! 页8/9/10：表盘列表、切换与表盘管理。

use crate::page::*;
use crate::page::Page;
use crate::page::kit::{MenuSpec, RowModel, ToggleDef};

// ===== 页8/9: 表盘列表(切换/轮换) =====

pub(crate) struct WatchfaceListPage {
    /// 本页对应的 page_id (8 或 9)
    pub pid: usize,
}

impl Page for WatchfaceListPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe {
            if self.pid == 8 { put_str(buf, "表盘切换".as_bytes()); }
            else { put_str(buf, "摇一摇轮换".as_bytes()); }
        }
    }

    /// 进页重读表盘清单: 两页过滤独立 —— 表盘页看 WF_SHOW_BUILTIN, 轮换页看 WF_SEL_BUILTIN。
    fn on_enter(&self) {
        unsafe {
            let f = if self.pid == 8 {
                st_rd!(WF_SHOW_BUILTIN)
            } else {
                st_rd!(WF_SEL_BUILTIN)
            };
            load_watchfaces(f);
        }
    }

    fn render(&self, _ctx: &PageCtx) -> PageResult {
        unsafe {
            refresh_watchface_lines();
            rows_layout(self.pid);
            wf_apply_selection(self.pid);
            Ok(())
        }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx == 0 }

    fn on_switch(&self, _idx: usize) {
        unsafe {
            if self.pid == 8 {
                let v = st_rd!(WF_SHOW_BUILTIN);
                st_wr!(WF_SHOW_BUILTIN, if v != 0 { 0 } else { 1 });
                load_watchfaces(st_rd!(WF_SHOW_BUILTIN));
                render_req(Pid::WfList as u32);
            } else {
                let v = st_rd!(WF_SEL_BUILTIN);
                st_wr!(WF_SEL_BUILTIN, if v != 0 { 0 } else { 1 });
                load_watchfaces(st_rd!(WF_SEL_BUILTIN));
                render_req(Pid::WfShake as u32);
            }
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if idx == 0 { return; }
            let cnt = st_rd!(WF_COUNT) as usize;
            let win = st_rd!(WF_WIN) as usize;
            if idx <= 8 {
                let e = win * 8 + (idx - 1);
                if e < cnt {
                    if self.pid == 8 {
                        let sw: unsafe extern "C" fn(*const u8) -> i32 =
                            core::mem::transmute(FW_SET_WATCHFACE as usize);
                        let rc = sw((core::ptr::addr_of!(WF_IDS) as *const u8).add(e * 16));
                        st_wr!(WF_SW_RC, rc);
                        if rc == 0 {
                            let node = read_volatile((core::ptr::addr_of!(WF_NODES) as *const u32).add(e));
                            let rf: unsafe extern "C" fn(u32, u32) -> i32 =
                                core::mem::transmute(FW_REFRESH_CUR_WF as usize);
                            let rfc = rf(1, node);
                            st_wr!(WF_RF_RC, rfc);
                        }
                        load_watchfaces(st_rd!(WF_SHOW_BUILTIN));
                        st_wr!(WF_WIN, win as u32);
                        wf_apply_selection(8);
                    } else {
                        let node = read_volatile((core::ptr::addr_of!(WF_NODES) as *const u32).add(e));
                        let off = !desel_has(node);
                        desel_set(node, off);
                        wf_apply_selection(9);
                    }
                }
            } else if idx == 9 {
                if (win + 1) * 8 < cnt {
                    st_wr!(WF_WIN, (win + 1) as u32);
                    render_req(self.pid as u32);
                }
            } else {
                nav_back();
            }
        }
    }

    fn apply_visibility(&self) {
        unsafe {
            let sh = fw_api::obj_set_hidden;
            let rows = page_rows_ptr(self.pid);
            let cnt = st_rd!(WF_COUNT) as usize;
            let win = st_rd!(WF_WIN) as usize;
            let sw0 = read_volatile(rows.add(0));
            if sw0 != 0 { sh(sw0, 0); }
            for r in 0..8usize {
                let op = read_volatile(rows.add(r + 1));
                if op != 0 {
                    let vis = if cnt == 0 { if r == 0 { 0u32 } else { 1u32 } }
                              else if win * 8 + r < cnt { 0u32 } else { 1u32 };
                    sh(op, vis);
                }
            }
            let op9 = read_volatile(rows.add(9));
            if op9 != 0 { sh(op9, if (win + 1) * 8 < cnt { 0 } else { 1 }); }
        }
    }
}

// ===== 页10: 表盘管理 =====

pub(crate) struct WfMgmtPage;

// 页10 开关/入口定义
unsafe fn wfshake_get() -> bool { st_rd!(SHAKE_EN) != 0 }
unsafe fn wfshake_toggle() {
    // 开关唯一真源 = 全局 SHAKE_EN(摇一摇定时器门控与开机恢复都读它)。
    let v = if st_rd!(SHAKE_EN) != 0 { 0u32 } else { 1u32 };
    st_wr!(SHAKE_EN, v);
    crate::watchface::persist_shake_enabled(v);
}
unsafe fn wf_goto_shake() { nav_goto(Pid::WfShake as u32, 0); }
unsafe fn wf_goto_list() {
    load_watchfaces(st_rd!(WF_SHOW_BUILTIN));
    nav_goto(Pid::WfList as u32, 0);
}

static WFSHAKE_DEF: ToggleDef = ToggleDef { get: wfshake_get, toggle: wfshake_toggle, status: None };
static WFMGR_ROWS: &[RowModel] = &[
    RowModel::toggle("摇一摇\0".as_bytes(), &WFSHAKE_DEF),
    RowModel::action("摇一摇轮换\0".as_bytes(), wf_goto_shake, 0, None),
    RowModel::action("表盘切换\0".as_bytes(), wf_goto_list, 0, None),
];

impl MenuSpec for WfMgmtPage {
    fn rows(&self) -> &'static [RowModel] { WFMGR_ROWS }
}

impl Page for WfMgmtPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "表盘管理\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { frame::build(self, ctx, ctx.title) }
    }

    fn is_switch_row(&self, idx: usize) -> bool { kit::menu_is_switch(self, idx) }
    fn on_click(&self, idx: usize) { unsafe { kit::menu_click(self, idx) } }
    fn on_switch(&self, idx: usize) { unsafe { kit::menu_switch(self, idx) } }
}

pub(super) static PAGE_WF_LIST_8: WatchfaceListPage = WatchfaceListPage { pid: 8 };
pub(super) static PAGE_WF_LIST_9: WatchfaceListPage = WatchfaceListPage { pid: 9 };
pub(super) static PAGE_WF_MGMT: WfMgmtPage = WfMgmtPage;
