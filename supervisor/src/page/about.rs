//! 页18/20/21：关于、开放源代码许可列表与正文。

use crate::page::*;
use crate::page::Page;

static mut ABOUT_FW: [u8; 32] = [0; 32];
static mut ABOUT_BT: [u8; 40] = [0; 40];

unsafe fn about_firmware() -> &'static [u8] {
    let out = core::ptr::addr_of_mut!(ABOUT_FW) as *mut u8;
    let fd = fw_api::open(b"/etc/build.prop\0".as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd >= 0 {
        let mut b = [0u8; 512];
        let n = fw_api::read(fd, b.as_mut_ptr(), 511);
        fw_api::close(fd);
        let key = b"ro.build.version=";
        let mut i = 0usize;
        while i + key.len() < n.max(0) as usize {
            let mut same = true;
            for j in 0..key.len() { if b[i + j] != key[j] { same = false; break; } }
            if same {
                let start = i + key.len();
                let mut end = start;
                while end < n as usize && b[end] != b'\n' && b[end] != b'\r' && b[end] != 0 { end += 1; }
                let len = core::cmp::min(end - start, 31);
                for j in 0..len { write_volatile(out.add(j), b[start + j]); }
                write_volatile(out.add(len), 0);
                return core::slice::from_raw_parts(out, len + 1);
            }
            i += 1;
        }
    }
    put_str(out, "未知\0".as_bytes());
    core::slice::from_raw_parts(out, 7)
}

unsafe fn about_build_time() -> &'static [u8] {
    let out = core::ptr::addr_of_mut!(ABOUT_BT) as *mut u8;
    let src = option_env!("CHAOS_BUILD_TIME").unwrap_or("unknown").as_bytes();
    let len = core::cmp::min(src.len(), 39);
    for i in 0..len { write_volatile(out.add(i), src[i]); }
    write_volatile(out.add(len), 0);
    core::slice::from_raw_parts(out, len + 1)
}

// ===== 页18: 关于 =====
pub(crate) struct AboutPage;
pub(super) static PAGE_ABOUT_PAGE: AboutPage = AboutPage;
static mut DBG_VAL: u32 = 50;
static mut DBG_TXT: [u8; 8] = [0; 8];
static mut DBG_DUMP_MSG: [u8; 32] = [0; 32];
static mut DBG_DUMP_ARM: u32 = 0;
static mut DBG_TICKS: u32 = 0;

unsafe fn dbg_text() {
    let v = DBG_VAL;
    let d = core::ptr::addr_of_mut!(DBG_TXT) as *mut u8;
    if v >= 100 { write_volatile(d.add(0), b'1'); write_volatile(d.add(1), b'0'); write_volatile(d.add(2), b'0'); write_volatile(d.add(3), b'%'); write_volatile(d.add(4), 0); }
    else if v >= 10 { write_volatile(d.add(0), b'0' + (v / 10) as u8); write_volatile(d.add(1), b'0' + (v % 10) as u8); write_volatile(d.add(2), b'%'); write_volatile(d.add(3), 0); }
    else { write_volatile(d.add(0), b'0' + v as u8); write_volatile(d.add(1), b'%'); write_volatile(d.add(2), 0); }
}

unsafe fn dbg_fields(a: fw_api::Obj) {
    if a == 0 { return; }
    let p = a as usize;
    crate::log::kv("dbg34 ", core::ptr::read_volatile((p + 0x34) as *const u32));
    crate::log::kv("dbg38 ", core::ptr::read_volatile((p + 0x38) as *const u32));
    crate::log::kv("dbg3C ", core::ptr::read_volatile((p + 0x3C) as *const u32));
    crate::log::kv("dbg40 ", core::ptr::read_volatile((p + 0x40) as *const u32));
    crate::log::kv("dbg44 ", core::ptr::read_volatile((p + 0x44) as *const u32));
    crate::log::kv("dbg48 ", core::ptr::read_volatile((p + 0x48) as *const u32));
    crate::log::kv("dbg4C ", core::ptr::read_volatile((p + 0x4C) as *const u32));
    crate::log::kv("dbg50 ", core::ptr::read_volatile((p + 0x50) as *const u32));
}

unsafe fn dbg_dump() {
    const BASE: usize = 0x1C06_0000;
    const LEN: usize = 512 * 1024;
    let path = b"/data/chaos/lvgl_dump.bin\0";
    let fd = fw_api::open(path.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    let mut ok = fd >= 0;
    if ok {
        let mut off = 0usize;
        while off < LEN {
            let got = fw_api::write(fd, (BASE + off) as *const u8, (LEN - off) as u32);
            if got <= 0 { ok = false; break; }
            off += got as usize;
        }
        fw_api::close(fd);
    }
    let m = core::ptr::addr_of_mut!(DBG_DUMP_MSG) as *mut u8;
    let t: &[u8] = if ok { "dump 完成 512KB\0".as_bytes() } else { "dump 失败\0".as_bytes() };
    for i in 0..t.len() { write_volatile(m.add(i), t[i]); }
}

impl Page for AboutPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe {
            if DBG_ON != 0 { put_str(buf, "调试\0".as_bytes()); }
            else { put_str(buf, "关于\0".as_bytes()); }
        }
    }
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            if DBG_ON != 0 {
                dbg_text();
                // 4 环 = 2x2 验证矩阵（全部只写角度字段，不碰样式 API）：
                //   A/B 灰底环(MAIN 整圈) + 蓝进度(INDICATOR 90..90+pct*3.6) 25%/75%
                //   C   只有 INDICATOR 整圈 90..450，验证整圈特例
                //   D   对照：一个字段都不写，看构造函数默认值到底画出什么
                let specs: [(i32, i32, u32, bool, bool); 4] = [(52, 96, 25, true, true), (196, 96, 75, true, true), (52, 268, 100, false, true), (196, 268, 0, false, false)];
                let caps: [&[u8]; 4] = [b"A 25%\0" as &[u8], b"B 75%\0", b"C 100%\0", b"D def\0"];
                for (i, &(x, y, pct, track, progress)) in specs.iter().enumerate() {
                    if track {
                        let t = fw_api::arc_create(ctx.content);
                        if t != 0 {
                            fw_api::obj_set_size_wh(t, 120, 120);
                            fw_api::obj_align(t, fw_api::align::TOP_LEFT, x, y);
                            fw_api::arc_set_bg_full(t);
                            fw_api::arc_disable_input(t);
                        }
                    }
                    if progress {
                        let arc = fw_api::arc_create(ctx.content);
                        if arc != 0 {
                            fw_api::obj_set_size_wh(arc, 120, 120);
                            fw_api::obj_align(arc, fw_api::align::TOP_LEFT, x, y);
                            fw_api::arc_set_progress(arc, pct);
                            fw_api::arc_disable_input(arc);
                        }
                    }
                    let cl = fw_api::label_create(ctx.content);
                    if cl != 0 {
                        fw_api::style_apply(cl, crate::FW_STYLE_HR_NAME, 255, 0);
                        fw_api::label_set_text(cl, caps[i].as_ptr());
                        fw_api::obj_align(cl, fw_api::align::TOP_LEFT, x + 34, y + 122);
                    }
                }
                let _ = build_rows(ctx, ctx.content, &[
                    (0, RowSpec::tap("① 重绘四个对比环\0".as_bytes(), core::ptr::null())),
                    (1, RowSpec::tap("② 导出 LVGL dump\0".as_bytes(), core::ptr::null())),
                ]);
                let msg = fw_api::label_create(ctx.content);
                if msg != 0 {
                    fw_api::style_apply(msg, crate::FW_STYLE_HR_NAME, 255, 0);
                    fw_api::label_set_text(msg, core::ptr::addr_of!(DBG_DUMP_MSG) as *const u8);
                    fw_api::obj_align(msg, fw_api::align::TOP_MID, 0, 420);
                }
                return Ok(());
            }
            let bt = about_build_time();
            let fw = about_firmware();
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::tap("Chaos\0".as_bytes(), concat!("v", env!("CARGO_PKG_VERSION"), ".dev\0").as_bytes().as_ptr())),
                (1, RowSpec::display("编译时间\0".as_bytes(), bt.as_ptr())),
                (2, RowSpec::display("运行固件\0".as_bytes(), fw.as_ptr())),
                (3, RowSpec::display("文华逸洋\0".as_bytes(), "开发者\0".as_bytes().as_ptr())),
                (4, RowSpec::display("IKUN_CXKPRO\0".as_bytes(), "开发者\0".as_bytes().as_ptr())),
                (5, RowSpec::display("谦虚的海鸥\0".as_bytes(), "开发者\0".as_bytes().as_ptr())),
                (6, RowSpec::display("@wzx\0".as_bytes(), "贡献者(fetch实现)\0".as_bytes().as_ptr())),
                (7, RowSpec::display("许可证\0".as_bytes(), "AGPL-3.0\0".as_bytes().as_ptr())),
                (8, RowSpec::clickable("开放源代码许可\0".as_bytes())),
            ])?;
            Ok(())
        }
    }
    fn on_click(&self, idx: usize) {
        unsafe {
            if DBG_ON != 0 {
                if DBG_TICKS < 5 { return; }
                if idx == 1 {
                    if DBG_DUMP_ARM == 0 {
                        DBG_DUMP_ARM = 1;
                        let m = core::ptr::addr_of_mut!(DBG_DUMP_MSG) as *mut u8;
                        for (i, c) in "再点一次确认导出\0".as_bytes().iter().enumerate() { write_volatile(m.add(i), *c); }
                    } else {
                        DBG_DUMP_ARM = 0;
                        dbg_dump();
                    }
                    crate::ui::render_req(18);
                    return;
                }
                crate::ui::render_req(18);
                return;
            }
            if idx == 8 {
                nav_goto(20, 0);
            }

            // debug入口

            // else if idx == 0 {
            //     DBG_CLICKS += 1;
            //     if DBG_CLICKS >= 5 { DBG_ON = 1; DBG_CLICKS = 0; DBG_TICKS = 0; DBG_DUMP_ARM = 0; crate::ui::render_req(18); }
            // }
        }
    }
    fn needs_tick(&self) -> bool { unsafe { DBG_ON != 0 } }
    fn tick(&self) { unsafe { if DBG_ON != 0 { DBG_TICKS = DBG_TICKS.wrapping_add(1); } } }
}

// ===== 页20: 关于 -> 开放源代码许可 =====
//
// Corona 以 AGPL-3.0-only 发布；此处保留完整许可证正文入口，同时保留项目中已有的
// MIBAND10PRO-TUNMODULE MPL-2.0 文本，便于用户在设备上查看所有第三方许可。
pub(crate) struct LicensesPage;
pub(super) static PAGE_LICENSESP: LicensesPage = LicensesPage;

impl Page for LicensesPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "开放源代码许可\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::tap("Canopus-Module-Corona\0".as_bytes(),
                    "AGPL-3.0-only\0".as_bytes().as_ptr())),
                (1, RowSpec::tap("MIBAND10PRO-TUNMODULE\0".as_bytes(),
                    "MPL-2.0\0".as_bytes().as_ptr())),
                // (2, RowSpec::display("Corona 上游源代码\0".as_bytes(),
                //     "https://github.com/AstralSightStudios/Canopus-Module-Corona\0".as_bytes().as_ptr())),
            ]).map(|_| ())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if idx < 2 {
                LICENSE_KIND = idx as u32;
                LICENSE_PAGE = 0;
                nav_goto(21, 0);
            }
        }
    }
}

static CORONA_LICENSE_TEXT: &[u8] = include_bytes!("../../corona_vendor/LICENSE");
static MPL_LICENSE_TEXT: &[u8] = include_bytes!("../../../licenses/MIBAND10PRO-TUNMODULE-MPL-2.0.txt");
static mut LICENSE_KIND: u32 = 0;
static mut LICENSE_PAGE: usize = 0;
static mut LICENSE_VIEW: [u8; 4096] = [0; 4096];
static mut LICENSE_STATUS: [u8; 48] = [0; 48];
const LICENSE_PAGE_LINES: usize = 7;

unsafe fn license_text() -> &'static [u8] {
    if LICENSE_KIND == 0 { CORONA_LICENSE_TEXT } else { MPL_LICENSE_TEXT }
}

unsafe fn license_line_count() -> usize {
    let mut lines = 1usize;
    for &c in license_text() { if c == b'\n' { lines += 1; } }
    lines
}

unsafe fn license_page_count() -> usize {
    (license_line_count() + LICENSE_PAGE_LINES - 1) / LICENSE_PAGE_LINES
}

unsafe fn license_render_page() {
    let text = license_text();
    let pages = license_page_count();
    if LICENSE_PAGE >= pages { LICENSE_PAGE = pages.saturating_sub(1); }
    let target_line = LICENSE_PAGE * LICENSE_PAGE_LINES;
    let mut line = 0usize;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < text.len() && line < target_line {
        if text[i] == b'\n' { line += 1; }
        i += 1;
        start = i;
    }
    let mut end = start;
    line = 0;
    while end < text.len() && line < LICENSE_PAGE_LINES {
        if text[end] == b'\n' { line += 1; }
        end += 1;
    }
    let out = core::ptr::addr_of_mut!(LICENSE_VIEW) as *mut u8;
    let count = core::cmp::min(end.saturating_sub(start), 4095);
    for j in 0..count { write_volatile(out.add(j), text[start + j]); }
    write_volatile(out.add(count), 0);

    let status = core::ptr::addr_of_mut!(LICENSE_STATUS) as *mut u8;
    let mut buf = [0u8; 48];
    let mut w = crate::text::W::new(buf.as_mut_ptr(), buf.len());
    w.s("第 ".as_bytes());
    w.n((LICENSE_PAGE + 1) as u32);
    w.s("/".as_bytes());
    w.n(pages as u32);
    w.s(" 页".as_bytes());
    w.end();
    let len = w.len();
    for j in 0..len { write_volatile(status.add(j), buf[j]); }
    write_volatile(status.add(len), 0);
}

pub(crate) struct LicenseTextPage;
pub(super) static PAGE_LICENSEP: LicenseTextPage = LicenseTextPage;

impl Page for LicenseTextPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe {
            if LICENSE_KIND == 0 { put_str(buf, "Corona AGPL-3.0-only\0".as_bytes()); }
            else { put_str(buf, "MPL-2.0 license\0".as_bytes()); }
        }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            license_render_page();
            let pages = license_page_count();
            let prev = build_rows(ctx, ctx.title, &[
                (0, RowSpec::clickable(if LICENSE_PAGE > 0 { "上一页\0".as_bytes() } else { "已到首页\0".as_bytes() })),
                (1, RowSpec::clickable(if LICENSE_PAGE + 1 < pages { "下一页\0".as_bytes() } else { "已到末页\0".as_bytes() })),
                (2, RowSpec::display(
                    core::slice::from_raw_parts(core::ptr::addr_of!(LICENSE_STATUS) as *const u8,
                        cstr_len(core::ptr::addr_of!(LICENSE_STATUS) as *const u8) + 1),
                    core::ptr::null())),
            ])?;
            let label = fw_api::label_create(ctx.content);
            if label != 0 {
                fw_api::style_apply(label, crate::FW_STYLE_HR_NAME, 255, 0);
                fw_api::label_set_text(label, core::ptr::addr_of!(LICENSE_VIEW) as *const u8);
                // 英文原文的一行会折成多行；高度随文字增长，不能把正文裁在 320px 内。
                fw_api::obj_set_width(label, 320);
                fw_api::obj_align_to(label, prev, fw_api::align::OUT_BOTTOM_MID, 0, 8);
            }
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            let pages = license_page_count();
            if idx == 0 && LICENSE_PAGE > 0 { LICENSE_PAGE -= 1; crate::ui::render_req(21); }
            else if idx == 1 && LICENSE_PAGE + 1 < pages { LICENSE_PAGE += 1; crate::ui::render_req(21); }
        }
    }
}
