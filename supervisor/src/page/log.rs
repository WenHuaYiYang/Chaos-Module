//! 页17：运行日志尾部读取、折行与滚动显示。

use crate::page::*;
use crate::page::Page;

// ===== 页17: 运行日志 =====
// 只读日志尾部，从最新一行往前拼成一段可滚动的文字。
//
// 2026-11-02 修：此固件的 label 不会响应 LV_SIZE_CONTENT，文字超过对象高度时
// 是直接裁掉而不是把对象撑开；旧版给 label 写死 240 高、又没有打开滚动，
// 长行折行后底部就永远看不见。这里按 24px MiSans 的保守字宽手动折行、
// 算出总行数再换算成 label/内层 board 的高度，外层 viewport 打开纵向滚动
// （同一套写法见 keyboard.rs 的横向 viewport）。
pub(crate) struct LogPage;
pub(super) static PAGE_LOG: LogPage = LogPage;

// 日志页只展示最近 60 条，16 KB 远大于 8 KB 文本上限；省下的静态 RAM 留给应用注册表。
const LOG_VIEW_CAP: usize = 16 * 1024;
const LOG_TEXT_CAP: usize = 8 * 1024;
const LOG_TEXT_W: u32 = 320;
const LOG_LINE_H: i32 = 36;
// 字宽估算是保守值，正常不会多折行；留几行余量，个别字形略宽也不会裁掉末行。
const LOG_SLACK_ROWS: i32 = 3;
const LOG_MAX_ENTRIES: usize = 60;
const LOG_VIEWPORT_W: u32 = 328;
const LOG_VIEWPORT_H: u32 = 280;

static mut LOG_VIEW: [u8; LOG_VIEW_CAP] = [0; LOG_VIEW_CAP];
static mut LOG_TEXT: [u8; LOG_TEXT_CAP] = [0; LOG_TEXT_CAP];
static mut LOG_LINE_OFF: [u32; 1024] = [0; 1024];
static mut LOG_TEXT_H: i32 = 0;
static mut LOG_MSG: [u8; 48] = [0; 48];
static mut LOG_CLEAR_ARMED: u32 = 0;

/// 24px MiSans 的保守字宽估计（只用于手算折行；宁愿多折，不能让 LVGL 再折一次）。
fn log_char_metrics(c: u8) -> (usize, u32) {
    if c < 0x80 {
        let w = match c {
            b' ' => 10,
            b'0'..=b'9' => 14,
            b'a'..=b'z' => 20,
            b'A'..=b'Z' => 22,
            _ => 12,
        };
        (1, w)
    } else if c & 0xE0 == 0xC0 {
        (2, 16)
    } else if c & 0xF0 == 0xE0 {
        (3, 26)
    } else if c & 0xF8 == 0xF0 {
        (4, 26)
    } else {
        (1, 12)
    }
}

/// 往 LOG_TEXT 里拼文字的小写入器：自己折行，并累计视觉行数。
struct LogText {
    out: *mut u8,
    len: usize,
    line_w: u32,
    rows: i32,
}

impl LogText {
    unsafe fn new(out: *mut u8) -> LogText {
        write_volatile(out, 0);
        LogText { out, len: 0, line_w: 0, rows: 1 }
    }

    unsafe fn newline(&mut self) {
        if self.len + 1 >= LOG_TEXT_CAP { return; }
        write_volatile(self.out.add(self.len), b'\n');
        self.len += 1;
        self.rows += 1;
        self.line_w = 0;
    }

    unsafe fn put_char(&mut self, src: *const u8, n: usize, w: u32) -> bool {
        if self.len + n + 1 >= LOG_TEXT_CAP { return false; }
        if self.line_w != 0 && self.line_w + w > LOG_TEXT_W {
            self.newline();
        }
        let mut k = 0usize;
        while k < n {
            write_volatile(self.out.add(self.len + k), read_volatile(src.add(k)));
            k += 1;
        }
        self.len += n;
        self.line_w += w;
        true
    }

    unsafe fn put_bytes(&mut self, s: &[u8]) -> bool {
        let mut i = 0usize;
        while i < s.len() {
            let (mut n, w) = log_char_metrics(s[i]);
            if i + n > s.len() { n = 1; }
            if !self.put_char(s.as_ptr().add(i), n, w) { return false; }
            i += n;
        }
        true
    }

    unsafe fn put_num(&mut self, mut v: u32) -> bool {
        let mut b = [0u8; 10];
        let mut i = 0usize;
        loop {
            b[i] = b'0' + (v % 10) as u8;
            i += 1;
            v /= 10;
            if v == 0 || i == b.len() { break; }
        }
        while i > 0 {
            i -= 1;
            if !self.put_char(b.as_ptr().add(i), 1, 14) { return false; }
        }
        true
    }

    /// 从日志缓冲追加一行；CR/TAB 换成空格，按 UTF-8 字宽折行。
    unsafe fn put_line(&mut self, buf: *const u8, s: usize, e: usize) -> bool {
        let mut i = s;
        while i < e {
            let raw = read_volatile(buf.add(i));
            let c = if raw == b'\r' || raw == b'\t' { b' ' } else { raw };
            let (mut n, w) = log_char_metrics(c);
            if i + n > e { n = 1; }
            let mut tmp = [0u8; 4];
            tmp[0] = c;
            let mut k = 1usize;
            while k < n {
                tmp[k] = read_volatile(buf.add(i + k));
                k += 1;
            }
            if !self.put_char(tmp.as_ptr(), n, w) { return false; }
            i += n;
        }
        true
    }

    unsafe fn trim_newline(&mut self) {
        if self.len > 0 && read_volatile(self.out.add(self.len - 1)) == b'\n' {
            self.len -= 1;
            if self.rows > 1 { self.rows -= 1; }
        }
    }
}

unsafe fn log_page_load() {
    let buf = core::ptr::addr_of_mut!(LOG_VIEW) as *mut u8;
    let n = crate::log::tail(buf, LOG_VIEW_CAP - 1);
    if n + 1 < LOG_VIEW_CAP { write_volatile(buf.add(n), 0); }

    // LOG_LINE_OFF 是环形缓冲：只保留最近 1024 行的行首。旧写法到 1024 行就停，
    // 日志行数一多就停在最旧的那段上，“最新在上”反而看不到最新日志。
    let offs = core::ptr::addr_of_mut!(LOG_LINE_OFF) as *mut u32;
    let mut count = 0usize;
    let mut i = 0usize;
    while i < n {
        write_volatile(offs.add(count % 1024), i as u32);
        count += 1;
        while i < n && read_volatile(buf.add(i)) != b'\n' { i += 1; }
        if i < n { i += 1; }
    }

    let out = core::ptr::addr_of_mut!(LOG_TEXT) as *mut u8;
    let mut t = LogText::new(out);

    // 清空结果只显示一次：读走就清零，避免下次进页面还挂着旧提示。
    let msg = core::ptr::addr_of_mut!(LOG_MSG) as *mut u8;
    let mlen = cstr_len(msg);
    if mlen != 0 {
        t.put_bytes(core::slice::from_raw_parts(msg, mlen));
        t.newline();
        write_volatile(msg, 0);
    }

    if count != 0 {
        t.put_bytes(concat!("v", env!("CARGO_PKG_VERSION"), " · 共 ").as_bytes());
        t.put_num(count as u32);
        t.put_bytes(" 行".as_bytes());
        t.newline();

        // 从最新一行往前拼，最新在上；旧日志在下面，随 viewport 滚动查看。
        let mut shown = 0usize;
        while shown < count && shown < LOG_MAX_ENTRIES {
            let li = count - 1 - shown;
            let s = read_volatile(offs.add(li % 1024)) as usize;
            let e = if li + 1 < count { read_volatile(offs.add((li + 1) % 1024)) as usize } else { n };
            // 范围里带着行尾的 \n；这里先剥掉，换行统一由下面的 newline() 记一次。
            // 否则字面 \n 和 newline() 会叠成空行，且 rows 少算一行/条，标签高度不够。
            let mut ee = e;
            while ee > s {
                let b = read_volatile(buf.add(ee - 1));
                if b == b'\n' || b == b'\r' { ee -= 1; } else { break; }
            }
            if ee > s {
                if !t.put_line(buf, s, ee) { break; }
                t.newline();
            }
            shown += 1;
        }
    } else if mlen == 0 {
        t.put_bytes("暂无日志".as_bytes());
    }
    t.trim_newline();
    if t.len + 1 < LOG_TEXT_CAP { write_volatile(out.add(t.len), 0); }
    // 行数换算成像素高：固定行高 + 预留余量（固件不会按内容自动撑高）。
    LOG_TEXT_H = (t.rows + LOG_SLACK_ROWS) * LOG_LINE_H;
}

impl Page for LogPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "运行日志\0".as_bytes()); }
    }
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            log_page_load();
            // 顶部两排按钮固定不动，文字放进独立纵向滚动视口：
            // 视口底 = content 相对 392 = 屏幕 448，正好避开底部 32px 手势区。
            let t0 = build_tile_below(ctx, ctx.title, 0, "返回\0".as_bytes(), TILE_W, 44);
            if t0 == 0 { return Err(PageError::RowFailed(0)); }
            let _t1 = build_tile_right(ctx, t0, 1, "刷新\0".as_bytes(), TILE_W, 44);
            let cl: &[u8] = if LOG_CLEAR_ARMED != 0 { "再点一次确认清空\0".as_bytes() } else { "清空日志\0".as_bytes() };
            let t2 = build_tile_below(ctx, t0, 2, cl, TILE_ROW_W, 44);
            if t2 == 0 { return Err(PageError::RowFailed(2)); }

            let vp = fw_api::obj_create(ctx.content, 0);
            if vp != 0 {
                fw_api::obj_clear_box_spacing(vp);
                fw_api::obj_set_size(vp, LOG_VIEWPORT_W as i32, LOG_VIEWPORT_H as i32);
                fw_api::obj_align_to(vp, t2, fw_api::align::OUT_BOTTOM_MID, 0, 8);
                fw_api::obj_style_color(vp, 0, 0);
                fw_api::obj_add_flag(vp, fw_api::LV_OBJ_FLAG_SCROLLABLE | fw_api::LV_OBJ_FLAG_CLICKABLE);
                fw_api::obj_set_scroll_dir(vp, fw_api::LV_DIR_VER);
                // 滚到头就停，不要把纵向拖动传给 content/系统手势。
                fw_api::obj_clear_flag(vp,
                    fw_api::LV_OBJ_FLAG_SCROLL_CHAIN
                        | fw_api::LV_OBJ_FLAG_GESTURE_BUBBLE
                        | fw_api::LV_OBJ_FLAG_SCROLL_ELASTIC);

                let board_h = if LOG_TEXT_H > LOG_VIEWPORT_H as i32 { LOG_TEXT_H } else { LOG_VIEWPORT_H as i32 };
                let board = fw_api::obj_create(vp, 0);
                if board != 0 {
                    fw_api::obj_clear_box_spacing(board);
                    fw_api::obj_clear_scrollable(board);
                    fw_api::obj_set_size(board, LOG_VIEWPORT_W as i32, board_h);
                    fw_api::obj_align(board, fw_api::align::TOP_LEFT, 0, 0);
                    fw_api::obj_style_color(board, 0, 0);
                    let lab = fw_api::label_create(board);
                    if lab != 0 {
                        fw_api::style_apply(lab, crate::FW_STYLE_MISANS_REG_24, 255, 0);
                        fw_api::label_set_text(lab, core::ptr::addr_of!(LOG_TEXT) as *const u8);
                        fw_api::obj_set_size_wh(lab, LOG_TEXT_W, LOG_TEXT_H as u32);
                        fw_api::obj_align(lab, fw_api::align::TOP_LEFT, 4, 0);
                        fw_api::obj_clear_scrollable(lab);
                    }
                }
            }
            Ok(())
        }
    }
    fn on_click(&self, idx: usize) {
        unsafe {
            if idx == 0 {
                LOG_CLEAR_ARMED = 0;
                nav_back();
            } else if idx == 1 {
                LOG_CLEAR_ARMED = 0;
                render_req(17);
            } else if idx == 2 {
                if LOG_CLEAR_ARMED == 0 {
                    LOG_CLEAR_ARMED = 1;
                    render_req(17);
                } else {
                    let ok = crate::log::clear();
                    LOG_CLEAR_ARMED = 0;
                    put_str(core::ptr::addr_of_mut!(LOG_MSG) as *mut u8,
                        if ok { "日志已清空\0".as_bytes() } else { "清空失败\0".as_bytes() });
                    render_req(17);
                }
            }
        }
    }
}
