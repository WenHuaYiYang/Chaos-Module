//! 页1/29/30：应用管理 UI、应用详情与批量操作。

use crate::page::*;
use crate::page::Page;

static mut P_EXCL_LB: [u32; 40] = [0; 40];
static mut P_STATE_LB: [u32; 40] = [0; 40];
static mut LAST_MSG: [u8; 64] = [0; 64];
static mut LAST_HDR: [u8; 64] = [0; 64];

/// 文本是否变化; 变化时把新文本存进 cache(供下次比较)。避免每 tick 重设同一段文字。
pub(crate) unsafe fn text_changed(cache: *mut u8, txt: *const u8, cap: usize) -> bool {
    let mut i = 0usize;
    let mut diff = false;
    loop {
        let b = core::ptr::read_volatile(txt.add(i));
        let a = core::ptr::read_volatile(cache.add(i));
        if a != b { diff = true; }
        if b == 0 || i + 1 >= cap { break; }
        i += 1;
    }
    if diff {
        let mut j = 0usize;
        while j <= i && j + 1 < cap {
            core::ptr::write_volatile(cache.add(j), core::ptr::read_volatile(txt.add(j)));
            j += 1;
        }
        if j + 1 < cap { core::ptr::write_volatile(cache.add(j), 0); }
    }
    diff
}

/// 在副标题胶囊右侧追加“排除状态”胶囊: 已排除=红, 未排除=灰。
pub(crate) unsafe fn add_excl_pill(row: u32, sub_lab: u32, i: usize, skip: bool) -> u32 {
    if skip || sub_lab == 0 { return 0; }
    let p = fw_api::label_create(row);
    if p == 0 { return 0; }
    let (txt, col, bg): (&[u8], u32, u32) = if crate::appmgr::excl_on(i) {
        ("已排除\0".as_bytes(), C_DANGER, 0x4D1613)
    } else {
        ("未排除\0".as_bytes(), C_SECONDARY, 0x3A3A3E)
    };
    fw_api::label_set_text(p, txt.as_ptr());
    pill(p, col, bg, txt);
    // 与第一个胶囊垂直居中对齐
    fw_api::obj_align_to(p, sub_lab, fw_api::align::OUT_RIGHT_MID, 8, 0);
    p
}

pub(crate) unsafe fn set_excl_pill(p: u32, i: usize) {
    if p == 0 { return; }
    let (txt, col, bg): (&[u8], u32, u32) = if crate::appmgr::excl_on(i) {
        ("已排除\0".as_bytes(), C_DANGER, 0x4D1613)
    } else {
        ("未排除\0".as_bytes(), C_SECONDARY, 0x3A3A3E)
    };
    fw_api::label_set_text(p, txt.as_ptr());
    pill(p, col, bg, txt);
}

/// 把副标题标签做成彩色小胶囊(状态药丸): 角色 tint 底色 + 角色色文字。

// ===== 页1: 应用管理(Shell++ shellpp-ii 核心逻辑移植, 见 appmgr.rs) =====
//
// (2026-10-15 二版) 列表形态改为**全量条目**: 去掉"更多"分页, 注册表里的每个应用
// 一行复选框(勾 = 当前可见), 行数超过一屏靠 content 滚动(lvx_page_content 本就是
// 可滚容器, content_pad_bottom 就是给滚动收尾用的)。
// 行句柄/事件槽位约束: 条目行**不走 ctx.rows**(只有 12 格) —— 句柄自管在
// appmgr::APP_ROWS, 事件槽号 = 条目下标 + 12(12..=51; 0..11 是 ctx.rows 的
// 批量按钮/状态行, 条目复用会被派发成点按钮), 回调是 lib.rs 的 chaos_row_ev12..51。
// 布局: 行0=批量操作 行1=深度隐藏开关 行2=状态 行3=重新应用隐藏 —— 放最上面,
// 列表再长也不怕按钮滚出屏幕; 之后是应用条目(已排除的条目不在本页显示)。

pub(crate) struct AppMgrPage;

static mut AM_STATUS_HID: [u8; 12] = [0; 12];
static mut AM_STATUS_VIS: [u8; 12] = [0; 12];

/// 列表行原地刷新(页1/页30 共用): 行数没变就逐行改副标题与颜色, 不重建整页,
/// 这样列表滚动位置不会跳回顶部, 也不会在快速连点时销毁/重建行对象。
/// 返回 false = 行数或句柄对不上, 调用方需要整页重建。
static mut ROW_STATE: [u8; 40] = [0xFF; 40];

unsafe fn refresh_list_rows(selected_mode: bool) -> bool {
    if list_building() { return true; } // 还在分批建行, 先不刷
    let n = crate::appmgr::ui_rows() as usize;
    let rows = crate::appmgr::rows_ptr();
    for v in 0..n {
        let i = core::ptr::read_volatile((core::ptr::addr_of!(LB_APP) as *const u16).add(v)) as usize;
        if i == 0xFFFF || i >= crate::appmgr::APP_MAX { return false; }
        let r = core::ptr::read_volatile(rows.add(v));
        if r == 0 { return false; }
        let vis = crate::appmgr::visible(i);
        let picked = selected_mode && crate::appmgr::sel_has(i);
        let st = (vis as u8) | ((picked as u8) << 1);
        let cache = core::ptr::addr_of_mut!(ROW_STATE) as *mut u8;
        if core::ptr::read_volatile(cache.add(v)) == st { continue; }
        core::ptr::write_volatile(cache.add(v), st);
        // 副标题必须继续传占位符: 传空会让固件把行退化成单行(标题垂直居中), 而且会打隐藏标志
        let ph: [u8; 3] = [0xc2, 0xa0, 0];
        fw_api::row_update(r, fw_api::NONE_PTR,
            crate::appmgr::entry_primary(i), ph.as_ptr(), 0, 0);
        // 无论固件怎么排, 标题统一钉在左上 (与左边距一致)
        let title = crate::mem::rd32((r + 0x3c) as *const u32);
        if title != 0 { fw_api::obj_align_to(title, r, fw_api::align::TOP_LEFT, 10, 10); }
        let sp = core::ptr::read_volatile((core::ptr::addr_of!(P_STATE_LB) as *const u32).add(v));
        let stxt: &[u8] = if vis { "显示中\0".as_bytes() } else { "已隐藏\0".as_bytes() };
        if sp != 0 {
            fw_api::label_set_text(sp, stxt.as_ptr());
            fw_api::obj_clear_flag(sp, 0x01);
            if vis { pill(sp, C_VISIBLE, C_VISIBLE_TINT, stxt); }
            else { pill(sp, C_HIDDEN, C_HIDDEN_TINT, stxt); }
        }
        if selected_mode {
            let title = crate::mem::rd32((r + 0x3c) as *const u32);
            if title != 0 {
                fw_api::obj_style_color_a70(title, if picked { C_ACCENT } else { 0xFFFFFF }, 0);
            }
        }
        let p = core::ptr::read_volatile((core::ptr::addr_of!(P_EXCL_LB) as *const u32).add(v));
        if p != 0 { set_excl_pill(p, i); }
    }
    true
}

/// 状态行原地刷新(页1 的"已隐藏 N / 显示中 M"提示)
unsafe fn refresh_msg_row() {
    let r = core::ptr::read_volatile(core::ptr::addr_of!(AM_MSG_ROW));
    if r != 0 {
        fw_api::row_update(r, fw_api::NONE_PTR,
            crate::appmgr::msg().as_ptr(), fw_api::NONE_PTR, 0, 0);
    }
}


// ---- 列表分批建立 ----
// 打开页面时一次建几十行会长时间占住 UI 线程；这里先建首屏几行, 其余的每次 tick 续几行。
// kind: 1 = 页1(无复选框), 2 = 页30(带复选框)。副标题用调用方传进来的状态文案。
static mut LB_KIND: u32 = 0;
static mut LB_V: u32 = 0;
static mut LB_N: u32 = 0;
static mut LB_PREV: u32 = 0;
static mut LB_CONTENT: u32 = 0;
static mut LB_FIRST: u32 = 0;
static mut LB_DELAY: u32 = 0;
static mut LB_SUB_VIS: u32 = 0;
static mut LB_SUB_HID: u32 = 0;
// 列表第 v 行对应的应用下标(批量页跳过排除项后, 行号 != 视图号)
static mut LB_APP: [u16; 64] = [0xFFFF; 64];

static mut LIST_REBUILD: u8 = 0;

const LB_FIRST_CHUNK: u32 = 0;
const LB_TICK_CHUNK: u32 = 5;

pub(crate) fn list_building() -> bool { unsafe { LB_V < LB_N } }

unsafe fn list_begin(kind: u32, prev: u32, content: u32, sub_vis: *const u8, sub_hid: *const u8) {
    LB_KIND = kind;
    LB_V = 0;
    LB_N = crate::appmgr::view_count() as u32;
    LB_PREV = prev;
    LB_CONTENT = content;
    LB_FIRST = 1;
    LB_SUB_VIS = sub_vis as u32;
    LB_SUB_HID = sub_hid as u32;
    {
        let cache = core::ptr::addr_of_mut!(ROW_STATE) as *mut u8;
        let mut i = 0usize;
        while i < 40 { core::ptr::write_volatile(cache.add(i), 0xFF); i += 1; }
        let pl = core::ptr::addr_of_mut!(P_EXCL_LB) as *mut u32;
        let mut i = 0usize;
        while i < 40 { core::ptr::write_volatile(pl.add(i), 0); i += 1; }
        let lm = core::ptr::addr_of_mut!(LAST_MSG) as *mut u8;
        let lh = core::ptr::addr_of_mut!(LAST_HDR) as *mut u8;
        let mut i = 0usize;
        while i < 64 { core::ptr::write_volatile(lm.add(i), 0xFF); core::ptr::write_volatile(lh.add(i), 0xFF); i += 1; }
        let mut z = 0usize;
        let sp_ = core::ptr::addr_of_mut!(P_STATE_LB) as *mut u32;
        let ep_ = core::ptr::addr_of_mut!(P_EXCL_LB) as *mut u32;
        while z < 40 { core::ptr::write_volatile(sp_.add(z), 0); core::ptr::write_volatile(ep_.add(z), 0); z += 1; }
    }
    LIST_REBUILD = 0;
    LB_DELAY = 3;
    {
        let a = core::ptr::addr_of_mut!(LB_APP) as *mut u16;
        let mut i = 0usize;
        while i < 64 { core::ptr::write_volatile(a.add(i), 0xFFFF); i += 1; }
    }
    let expected = if kind == 2 { crate::appmgr::batch_total() as u32 } else { LB_N };
    crate::appmgr::set_ui_rows(expected);
    crate::log::kv("应用总数 ", crate::appmgr::count() as u32);
    crate::log::kv("可见列表条目数 ", LB_N);
    list_build_chunk(LB_FIRST_CHUNK);
}

unsafe fn list_build_chunk(limit: u32) {
    let rows = crate::appmgr::rows_ptr();
    let mut k = 0u32;
    while k < limit && LB_V < LB_N {
        let v = LB_V as usize;
        let i = crate::appmgr::view_at(v);
        if i == usize::MAX { LB_V = LB_N; break; }
        // 批量页跳过被排除的应用(直接不建行, 不留空白)
        if LB_KIND == 2 && crate::appmgr::excluded(i) { LB_V += 1; continue; }
        let app = core::ptr::addr_of_mut!(LB_APP) as *mut u16;
        core::ptr::write_volatile(app.add(v), i as u16);
        let vis = crate::appmgr::visible(i);
        // 批量页也不再使用复选框: 和页 1 一样用行 + 胶囊, 选中态靠标题变色
        let trailing = 0;
        // 页 1 本来就是两行行型, 副标题用不换行空格占位即可;
        // 批量页的复选框行会被固件当成单行(标题垂直居中), 用真实字形占位把它撑成两行,
        // 这个占位稍后被胶囊完全盖住
        // 批量页把状态文本直接交给固件副标题(有真实文字 -> 行是两行行型, 标题在左上),
        // 之后把这个副标题本身做成胶囊; 页 1 仍是占位 + 自建胶囊
        let picked = LB_KIND == 2 && crate::appmgr::sel_has(i);
        let stxt: &[u8] = if vis { "显示中\0".as_bytes() } else { "已隐藏\0".as_bytes() };
        let sub_ph: &[u8] = b"\xc2\xa0\0";
        let r = fw_api::row_create(LB_CONTENT, crate::appmgr::entry_primary(i),
            sub_ph.as_ptr(), trailing);
        if r == 0 { crate::log::kv("列表行创建失败, 已建 ", LB_V); LB_V = LB_N; break; }
        write_volatile(rows.add(v), r);
        fw_api::obj_add_event(r, row_ev_fn(v + 12), 7, 0);
        fw_api::obj_set_size_wh(r, 320, 100);
        fw_api::obj_set_local_style_prop(r, crate::fw_api::fwaddr::p65_style::PAD_3, 10, 0);
        cardify(r);
        // 页 1 自建胶囊; 批量页直接用固件副标题当胶囊(保持可见文本, 行才不会退化单行)
        let sp = fw_api::label_create(r);
        if sp != 0 {
            fw_api::label_set_text(sp, stxt.as_ptr());
            if picked { pill(sp, C_ACCENT, C_VISIBLE_TINT, stxt); }
            else if vis { pill(sp, C_VISIBLE, C_VISIBLE_TINT, stxt); }
            else { pill(sp, C_HIDDEN, C_HIDDEN_TINT, stxt); }
            // 锚在固件自己排出来的第二行(副标签)上: 垂直居中于该行, 水平用 dx 拉回左边距。
            // 第二行一定在卡片内部, 所以胶囊不会再掉到卡片外面。
            let sub = crate::mem::rd32((r + 0x40) as *const u32);
            let base = if sub != 0 { sub } else { r };
            let w = pill_w(stxt) as i32;
            fw_api::obj_align_to(sp, base, fw_api::align::CENTER, -(150 - w / 2), 0);
        }
        P_STATE_LB[v] = sp;
        let exp = add_excl_pill(r, sp, i, false);
        P_EXCL_LB[v] = exp;
        // 批量页: 选中的应用名称变色
        if LB_KIND == 2 {
            let title = crate::mem::rd32((r + 0x3c) as *const u32);
            if title != 0 {
                fw_api::obj_style_color_a70(title, if picked { C_ACCENT } else { 0xFFFFFF }, 0);
            }
        }
        if LB_KIND == 1 {
            let cache = core::ptr::addr_of_mut!(ROW_STATE) as *mut u8;
            core::ptr::write_volatile(cache.add(v), (vis as u8) | 0);
        }
        let ddx = if LB_KIND == 2 && LB_FIRST != 0 { TILE_CENTER_DX } else { 0 };
        fw_api::obj_align_to(r, LB_PREV, 14, ddx, 10);
        LB_FIRST = 0;
        LB_PREV = r;
        LB_V += 1;
        k += 1;
    }
}

impl Page for AppMgrPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "应用管理\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            crate::appmgr::ensure();
            crate::appmgr::rows_reset();
            // 顶部: 批量操作 + 深度隐藏开关 + 状态行 + 重新应用隐藏
            let t0 = build_tile_below(ctx, ctx.title, 0,
                "批量操作 >\0".as_bytes(), TILE_ROW_W, TILE_H);
            if t0 == 0 { return Err(PageError::RowFailed(0)); }
            // 行1: 深度隐藏开关。开 = 条目搬进 apps.json_hide；关 = 普通隐藏(keep 名单)。
            let sw = build_row(ctx, t0, 1,
                &RowSpec::switch(crate::appmgr::apply_mode_label()));
            if sw == 0 { return Err(PageError::RowFailed(1)); }
            fw_api::row_update(sw,
                fw_api::NONE_PTR, fw_api::NONE_PTR, fw_api::NONE_PTR,
                0, crate::appmgr::deep_on() as u8);
            let s2 = build_row(ctx, sw, 2,
                &RowSpec::display(crate::appmgr::msg(), core::ptr::null()));
            row_shrink(s2, 56);
            AM_MSG_ROW = read_volatile(ctx.rows.add(2));
            if s2 != 0 {
                let t3 = build_tile_below(ctx, s2, 3,
                    "重新应用隐藏\0".as_bytes(), 328, 56);
                tile_primary(t3);
            }
            crate::appmgr::sort_view_visible_first();

            // 状态文案(静态缓冲, 供每行的副标签使用)
            put_str(core::ptr::addr_of_mut!(AM_STATUS_HID) as *mut u8, "已隐藏\0".as_bytes());
            put_str(core::ptr::addr_of_mut!(AM_STATUS_VIS) as *mut u8, "显示中\0".as_bytes());

            // 条目行: 先建首屏, 其余由 tick 分批续建
            let mut prev = read_volatile(ctx.rows.add(3));
            if prev == 0 { prev = ctx.title; }

            // 分组说明行: "快应用 N / 已隐藏 M"
            let hdr = fw_api::row_create(ctx.content,
                crate::appmgr::header_text().as_ptr(), fw_api::NONE_PTR, 0);
            if hdr != 0 {
                row_shrink(hdr, 44);
                fw_api::obj_align_to(hdr, prev, 14, 0, 14);
                let hl = crate::mem::rd32((hdr + ROW_SUB_LB) as *const u32);
                if hl != 0 { fw_api::obj_style_color_a70(hl, C_SECONDARY, 0); }
                prev = hdr;
                AM_HDR_ROW = hdr;
            }
            list_begin(1, prev, ctx.content,
                core::ptr::addr_of!(AM_STATUS_VIS) as *const u8,
                core::ptr::addr_of!(AM_STATUS_HID) as *const u8);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        // 页1 顶部按钮槽位: 0=批量, 3=重新应用隐藏; 槽1 是开关(只走 on_switch)
        unsafe {
            if idx == 0 {
                nav_goto(30, 0);            // 批量操作
            } else if idx == 3 {
                crate::appmgr::request_reapply();
                render_req(1);
            } else if idx >= 12 {
                let v = idx - 12;
                if v < crate::appmgr::ui_rows() as usize {
                    let e = core::ptr::read_volatile((core::ptr::addr_of!(LB_APP) as *const u16).add(v)) as usize;
                    if e != 0xFFFF && e < crate::appmgr::APP_MAX {
                        crate::appmgr::detail_open(e);
                        nav_goto(29, 0);    // 应用详情
                    }
                }
            }
        }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx == 1 }

    fn on_switch(&self, idx: usize) {
        if idx == 1 {
            unsafe {
                crate::appmgr::toggle_apply_mode();
                render_req(1);
            }
        }
    }

    fn needs_tick(&self) -> bool {
        unsafe {
            list_building() || crate::appmgr::has_req() || crate::appmgr::batch_pending()
                || crate::appmgr::reapply_pending() || crate::appmgr::mode_pending()
        }
    }

    fn tick(&self) {
        unsafe {
            if list_building() {
                if LB_DELAY > 0 { LB_DELAY -= 1; }
                else { list_build_chunk(LB_TICK_CHUNK); }
            }
            if crate::appmgr::mode_pending() {
                crate::appmgr::do_mode_apply();
                render_req(1);
            } else if crate::appmgr::reapply_pending() {
                crate::appmgr::do_reapply();
            } else if crate::appmgr::batch_pending() {
                crate::appmgr::do_batch();
            } else if crate::appmgr::has_req() {
                crate::appmgr::do_op();
            }
            // 只改了隐藏位/名单时逐行原地刷新, 行数变了(卸载、重扫)才整页重建。
            // 整页重建会销毁行对象, 列表滚动位置会跳回顶部。
            if text_changed(core::ptr::addr_of_mut!(LAST_MSG) as *mut u8,
                crate::appmgr::msg().as_ptr(), 64) {
                refresh_msg_row();
            }
            let h = core::ptr::read_volatile(core::ptr::addr_of!(AM_HDR_ROW));
            if h != 0 && text_changed(core::ptr::addr_of_mut!(LAST_HDR) as *mut u8,
                crate::appmgr::header_text().as_ptr(), 64) {
                fw_api::row_update(h, fw_api::NONE_PTR,
                    crate::appmgr::header_text().as_ptr(), fw_api::NONE_PTR, 0, 0);
            }
            // 行句柄失效时不再整页重建: 重建会销毁 40 个对象并再次触发本 tick, 形成每帧重建的卡顿。
            // 行数真的变了(卸载/重扫)由对应操作自己 render_req。
            let _ = refresh_list_rows(false);
        }
    }
}


static mut AM_MSG_ROW: u32 = 0;
static mut AM_HDR_ROW: u32 = 0;

// ===== 页29: 应用管理 -> 应用详情(P0 占位) =====
/// 把以 NUL 结尾的静态缓冲当 &[u8] 用(RowSpec 吃切片, 不收裸指针)。
unsafe fn cstr_static(p: *const u8) -> &'static [u8] {
    let mut n = 0usize;
    while read_volatile(p.add(n)) != 0 { n += 1; }
    core::slice::from_raw_parts(p, n + 1)
}

pub(crate) struct AppDetailPage;
pub(super) static PAGE_DETAILP: AppDetailPage = AppDetailPage;

static mut DT_LBL_SHOW: [u8; 24] = [0; 24];
static mut DT_LBL_HIDE: [u8; 24] = [0; 24];
static mut DT_LBL_UN: [u8; 32] = [0; 32];
static mut DT_LBL_UN2: [u8; 32] = [0; 32];

impl Page for AppDetailPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "应用详情\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            if !crate::appmgr::detail_valid() {
                let r = build_row(ctx, ctx.title, 0,
                    &RowSpec::tap("应用不存在, 返回\0".as_bytes(), core::ptr::null()));
                if r == 0 { return Err(PageError::RowFailed(0)); }
                return Ok(());
            }
            put_str(core::ptr::addr_of_mut!(DT_LBL_SHOW) as *mut u8, "显示该应用\0".as_bytes());
            put_str(core::ptr::addr_of_mut!(DT_LBL_HIDE) as *mut u8, "隐藏该应用\0".as_bytes());
            put_str(core::ptr::addr_of_mut!(DT_LBL_UN) as *mut u8, "卸载应用\0".as_bytes());
            put_str(core::ptr::addr_of_mut!(DT_LBL_UN2) as *mut u8, "确认卸载?\0".as_bytes());

            let toggle: &[u8] = crate::appmgr::detail_action_label();
            let un: &[u8] = if crate::appmgr::detail_confirm() {
                cstr_static(core::ptr::addr_of!(DT_LBL_UN2) as *const u8)
            } else {
                cstr_static(core::ptr::addr_of!(DT_LBL_UN) as *const u8)
            };
            let r0 = build_row(ctx, ctx.title, 0,
                &RowSpec::display(crate::appmgr::detail_name(),
                    crate::appmgr::detail_pkg().as_ptr()));
            if r0 == 0 { return Err(PageError::RowFailed(0)); }
            fw_api::obj_set_size_wh(r0, 320, 88);
            fw_api::obj_align_to(r0, ctx.title, 14, 0, 10);
            cardify(r0);
            let r1 = build_row(ctx, r0, 1,
                &RowSpec::display(cstr_static(crate::appmgr::detail_fmt()),
                    core::ptr::null()));
            if r1 == 0 { return Err(PageError::RowFailed(1)); }
            fw_api::obj_set_size_wh(r1, 320, 56);
            fw_api::obj_align_to(r1, r0, 14, 0, 10);
            cardify(r1);
            // 隐藏/显示 | 排除/取消排除 并排, 卸载整宽一行
            let t2 = build_tile_below(ctx, r1, 2, toggle, TILE_W, TILE_H);
            if t2 == 0 { return Err(PageError::RowFailed(2)); }
            tile_primary(t2);
            let xt = crate::appmgr::detail_excl_label();
            let _t3 = build_tile_right(ctx, t2, 3, xt, TILE_W, TILE_H);
            let t4 = build_tile_below(ctx, t2, 4, "打开应用\0".as_bytes(), TILE_ROW_W, 56);
            if t4 == 0 { return Err(PageError::RowFailed(4)); }
            tile_primary(t4);
            let t5 = build_tile_below(ctx, t4, 5, un, TILE_ROW_W, 56);
            if t5 == 0 { return Err(PageError::RowFailed(5)); }
            tile_danger(t5);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if !crate::appmgr::detail_valid() {
                if idx == 0 { nav_back(); }
                return;
            }
            if idx == 2 {
                crate::appmgr::detail_toggle_hide();
                render_req(29);
            } else if idx == 3 {
                crate::appmgr::detail_toggle_excl();
                render_req(29);
            } else if idx == 4 {
                crate::appmgr::detail_launch();
            } else if idx == 5 {
                if crate::appmgr::detail_uninstall_click() {
                    // 已卸载: 回到应用管理列表
                    nav_back();
                    render_req(1);
                } else {
                    render_req(29);
                }
            }
        }
    }

    fn needs_tick(&self) -> bool {
        unsafe { crate::appmgr::has_req() || crate::appmgr::excl_has_req() }
    }

    fn tick(&self) {
        unsafe {
            let mut dirty = false;
            if crate::appmgr::has_req() {
                crate::appmgr::do_op();
                dirty = true;
            }
            if crate::appmgr::excl_has_req() {
                crate::appmgr::do_excl();
                dirty = true;
            }
            if dirty {
                crate::appmgr::detail_refresh();
                render_req(29);
            }
        }
    }
}

// ===== 页30: 应用管理 -> 批量操作 =====
pub(crate) struct BatchPage;
pub(super) static PAGE_BATCHP: BatchPage = BatchPage;

static mut B_SEL_TXT: [u8; 48] = [0; 48];
static mut B_ST_HID: [u8; 12] = [0; 12];
static mut B_ST_VIS: [u8; 12] = [0; 12];
static mut B_LBL_ALL: [u8; 32] = [0; 32];
static mut B_LBL_UNSEL: [u8; 32] = [0; 32];
static mut B_LBL_DEL: [u8; 32] = [0; 32];
static mut B_LBL_DEL2: [u8; 32] = [0; 32];
static mut B_COUNT_ROW: u32 = 0;

unsafe fn b_sel_text() -> *const u8 {
    let d = core::ptr::addr_of_mut!(B_SEL_TXT) as *mut u8;
    let mut w = W::new(d, 48);
    w.s("已选 ".as_bytes()).n(crate::appmgr::sel_count() as u32)
        .s(" / 共 ".as_bytes()).n(crate::appmgr::batch_total() as u32)
        .s("\0".as_bytes());
    w.end();
    d
}

/// 批量页的"已选 N / 共 M"行原地刷新
unsafe fn refresh_batch_count_row() {
    let r = core::ptr::read_volatile(core::ptr::addr_of!(B_COUNT_ROW));
    if r != 0 {
        fw_api::row_update(r, fw_api::NONE_PTR,
            cstr_static(b_sel_text()).as_ptr(), fw_api::NONE_PTR, 0, 0);
    }
}

impl Page for BatchPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "批量操作\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            crate::appmgr::rows_reset();
            put_str(core::ptr::addr_of_mut!(B_ST_HID) as *mut u8, "已隐藏\0".as_bytes());
            put_str(core::ptr::addr_of_mut!(B_ST_VIS) as *mut u8, "显示中\0".as_bytes());
            put_str(core::ptr::addr_of_mut!(B_LBL_ALL) as *mut u8, "全选\0".as_bytes());
            put_str(core::ptr::addr_of_mut!(B_LBL_DEL) as *mut u8, "删除选中\0".as_bytes());
            put_str(core::ptr::addr_of_mut!(B_LBL_DEL2) as *mut u8, "确认删除?\0".as_bytes());
            let _ = &B_LBL_UNSEL;

            let all: &[u8] = if crate::appmgr::sel_count() == 0 {
                cstr_static(core::ptr::addr_of!(B_LBL_ALL) as *const u8)
            } else {
                // "取消全选"
                put_str(core::ptr::addr_of_mut!(B_LBL_UNSEL) as *mut u8, "取消全选\0".as_bytes());
                cstr_static(core::ptr::addr_of!(B_LBL_UNSEL) as *const u8)
            };
            let del: &[u8] = if crate::appmgr::sel_del_confirmed() {
                cstr_static(core::ptr::addr_of!(B_LBL_DEL2) as *const u8)
            } else {
                cstr_static(core::ptr::addr_of!(B_LBL_DEL) as *const u8)
            };
            // 顶部: 已选统计 + 四个操作按钮并排两行, 返回交给标题栏返回键
            let s0 = build_row(ctx, ctx.title, 0,
                &RowSpec::display(cstr_static(b_sel_text()), core::ptr::null()));
            if s0 == 0 { return Err(PageError::RowFailed(0)); }
            row_shrink(s0, 56);
            B_COUNT_ROW = s0;
            let t1 = build_tile_below(ctx, s0, 1, all, TILE_W, TILE_H);
            if t1 == 0 { return Err(PageError::RowFailed(1)); }
            let t2 = build_tile_right(ctx, t1, 2, "隐藏选中\0".as_bytes(), TILE_W, TILE_H);
            tile_primary(t2);
            let t3 = build_tile_below(ctx, t1, 3, "显示选中\0".as_bytes(), TILE_W, TILE_H);
            if t3 == 0 { return Err(PageError::RowFailed(3)); }
            let t4 = build_tile_right(ctx, t3, 4, del, TILE_W, TILE_H);
            tile_danger(t4);

            // 勾选行: 先建首屏, 其余由 tick 分批续建
            crate::appmgr::sort_view_visible_first();
            list_begin(2, t3, ctx.content,
                core::ptr::addr_of!(B_ST_VIS) as *const u8,
                core::ptr::addr_of!(B_ST_HID) as *const u8);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if idx == 1 {
                crate::appmgr::sel_toggle_all();
                if !refresh_list_rows(true) { render_req(30); }
                refresh_batch_count_row();
            } else if idx == 2 {
                crate::appmgr::batch_request_sel(true);
                render_req(30);
            } else if idx == 3 {
                crate::appmgr::batch_request_sel(false);
                render_req(30);
            } else if idx == 4 {
                if crate::appmgr::sel_del_confirmed() {
                    crate::appmgr::batch_request_del();
                } else {
                    crate::appmgr::sel_del_arm();
                }
                render_req(30);
            } else if idx == 5 {
                crate::appmgr::sel_del_disarm();
                nav_back();
            } else if idx >= 12 {
                let v = idx - 12;
                if v < crate::appmgr::ui_rows() as usize {
                    let e = core::ptr::read_volatile((core::ptr::addr_of!(LB_APP) as *const u16).add(v)) as usize;
                    if e != 0xFFFF && e < crate::appmgr::APP_MAX { crate::appmgr::sel_toggle(e); }
                    if !refresh_list_rows(true) { render_req(30); }
                    refresh_batch_count_row();
                }
            }
        }
    }

    fn needs_tick(&self) -> bool {
        unsafe {
            list_building() || crate::appmgr::batch_pending() || crate::appmgr::del_pending()
                || crate::appmgr::has_req()
        }
    }

    fn tick(&self) {
        unsafe {
            if list_building() {
                if LB_DELAY > 0 { LB_DELAY -= 1; }
                else { list_build_chunk(LB_TICK_CHUNK); }
            }
            if crate::appmgr::has_req() {
                crate::appmgr::do_op();
            } else if crate::appmgr::batch_pending() {
                crate::appmgr::do_batch();
            } else if crate::appmgr::del_pending() {
                crate::appmgr::do_del_batch();
            }
            // 同上: 勾选/状态变化只原地刷新, 不再自动整页重建
            let _ = refresh_list_rows(true);
            refresh_batch_count_row();
        }
    }
}


pub(super) static PAGE_APPMGR: AppMgrPage = AppMgrPage;
