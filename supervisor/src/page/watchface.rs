//! 页8/9/10：表盘列表、切换与表盘管理。

use crate::page::*;
use crate::page::Page;

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
                render_req(8);
            } else {
                let v = st_rd!(WF_SEL_BUILTIN);
                st_wr!(WF_SEL_BUILTIN, if v != 0 { 0 } else { 1 });
                load_watchfaces(st_rd!(WF_SEL_BUILTIN));
                render_req(9);
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
                    render_req(self.pid);
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
            let op10 = read_volatile(rows.add(10));
            if op10 != 0 { sh(op10, 0); }
        }
    }
}

// ===== 页10: 表盘管理 =====

pub(crate) struct WfMgmtPage;

impl Page for WfMgmtPage {

    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "表盘管理".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            let mut tbuf: [u8; 88] = [0; 88];
            shake_toggle_text(tbuf.as_mut_ptr());
            let tlen = cstr_len(tbuf.as_ptr());
            let text = core::slice::from_raw_parts(tbuf.as_ptr(), tlen + 1);
            let prev = build_rows(ctx, ctx.title, &[
                (0, RowSpec::switch(text)),
                (1, RowSpec::clickable("摇一摇轮换\0".as_bytes())),
                (2, RowSpec::clickable("表盘切换\0".as_bytes())),
            ])?;
            // 补写开关初态
            let rows = page_rows_ptr(10);
            let s0 = read_volatile(rows.add(0));
            if s0 != 0 {
                let upd = fw_api::row_update;
                let en = st_rd!(SHAKE_EN);   // 开关真源 = 全局(与开机恢复/定时器门控同源)
                upd(s0, core::ptr::null(), tbuf.as_ptr(), core::ptr::null(), 0, en as u8);
            }
            let _ = prev;
            Ok(())
        }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx == 0 }

    fn on_switch(&self, _idx: usize) {
        // 开关唯一真源 = 全局 SHAKE_EN(摇一摇定时器门控与开机恢复都读它)。
        // 页面不再持本地副本 —— 本地副本会让开关初态与持久化状态脱钩
        // (旧实现: 本地默认 1, 重启后永远显示"开", 且第一次拨动写的值与所见相反)。
        unsafe {
            let v = if st_rd!(SHAKE_EN) != 0 { 0u32 } else { 1u32 };
            st_wr!(SHAKE_EN, v);
            crate::watchface::persist_shake_enabled(v);
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            match idx {
                1 => nav_goto(9, 0),
                2 => {
                    load_watchfaces(st_rd!(WF_SHOW_BUILTIN));
                    nav_goto(8, 0);
                }
                _ => {}
            }
        }
    }
}


pub(super) static PAGE_WF_LIST_8: WatchfaceListPage = WatchfaceListPage { pid: 8 };
pub(super) static PAGE_WF_LIST_9: WatchfaceListPage = WatchfaceListPage { pid: 9 };
pub(super) static PAGE_WF_MGMT: WfMgmtPage = WfMgmtPage;
