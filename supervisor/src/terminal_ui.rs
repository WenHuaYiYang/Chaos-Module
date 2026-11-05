//! NSH transcript UI. Only the LVGL thread touches this module's state/labels.
//! Leaving the page preserves the shell; no UI pointer is sent to the worker.

use crate::*;
use core::fmt::Write;
#[path = "terminal_model.rs"]
mod model;

pub(crate) const PAGE_PID: u32 = 31;
pub(crate) static PAGE: TerminalPage = TerminalPage;
static mut OUTPUT_LABEL: u32 = 0;
static mut OUTPUT_BOX: u32 = 0;
static mut OUTPUT_LINES: [u32; model::PAGE_LINES] = [0; model::PAGE_LINES];
static mut LAST_LINES: [[u8; model::LINE_BYTES + 1]; model::PAGE_LINES] = [[0; model::LINE_BYTES + 1]; model::PAGE_LINES];
static mut TILES: [u32; 10] = [0; 10];
static mut HINT_LABEL: u32 = 0;
static mut LAST_PITCH: u32 = 0;
static mut STATUS_LABEL: u32 = 0;
static mut MODEL: model::Model = model::Model::new();
static mut RX: [u8; 256] = [0; 256];
static mut OUTPUT: [u8; model::PAGE_BYTES] = [0; model::PAGE_BYTES];
static mut LAST_OUTPUT: [u8; model::PAGE_BYTES] = [0; model::PAGE_BYTES];
static mut STATUS: [u8; 128] = [0; 128];
static mut LAST_STATUS: [u8; 128] = [0; 128];
static mut OLDER_LINES: usize = 0;
static mut LOCAL_ERROR: i32 = 0;
static mut FORCE_PAINT: bool = true;

pub(crate) unsafe fn forget_ui() {
    OUTPUT_LABEL = 0;
    OUTPUT_BOX = 0;
    OUTPUT_LINES.fill(0);
    TILES.fill(0);
    HINT_LABEL = 0;
    LAST_PITCH = 0;
    STATUS_LABEL = 0;
    FORCE_PAINT = true;
}

fn state_text() -> &'static str {
    match crate::nsh::state() {
        crate::nsh::State::Idle => "未启动",
        crate::nsh::State::Starting => "启动中",
        crate::nsh::State::Running => "运行中",
        crate::nsh::State::Exited => "已退出",
        crate::nsh::State::Failed => "启动/会话失败",
    }
}

struct Text<'a> { out: &'a mut [u8], len: usize }
impl core::fmt::Write for Text<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let mut n = s.len().min(self.out.len().saturating_sub(self.len + 1));
        while !s.is_char_boundary(n) { n -= 1; }
        self.out[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n;
        self.out[self.len] = 0;
        Ok(())
    }
}

unsafe fn send(bytes: &[u8]) -> bool {
    match crate::nsh::send(bytes) {
        Ok(()) => { LOCAL_ERROR = 0; OLDER_LINES = 0; true }
        Err(code) => { LOCAL_ERROR = code; false }
    }
}

pub(crate) unsafe fn send_control(code: u8) -> bool {
    if !matches!(code, 3 | 9 | 10) { LOCAL_ERROR = -22; return false; }
    send(&[code])
}

unsafe fn start() -> bool {
    // UI guard in addition to the backend guard: no second worker/session.
    if matches!(crate::nsh::state(), crate::nsh::State::Starting | crate::nsh::State::Running) {
        return true;
    }
    match crate::nsh::start() {
        Ok(()) => { LOCAL_ERROR = 0; OLDER_LINES = 0; true }
        Err(code) => { LOCAL_ERROR = code; false }
    }
}

pub(crate) unsafe fn submit_command(input: *const u8) -> bool {
    if input.is_null() { LOCAL_ERROR = -22; return false; }
    let mut command = [0u8; 256];
    let mut n = 0;
    while n < 255 {
        let b = core::ptr::read_volatile(input.add(n));
        if b == 0 { break; }
        // The keyboard supplies printable ASCII. Reject injected NUL/newline,
        // overlong or non-ASCII input instead of silently splitting commands.
        if !(0x20..=0x7e).contains(&b) { LOCAL_ERROR = -22; return false; }
        command[n] = b;
        n += 1;
    }
    if n == 255 && core::ptr::read_volatile(input.add(n)) != 0 { LOCAL_ERROR = -7; return false; }
    if !start() { return false; }
    command[n] = b'\n';
    // One all-or-nothing enqueue. Retrying cannot duplicate a partially sent
    // command when the TX queue has space for the text but not its newline.
    send(&command[..n + 1])
}

pub(crate) unsafe fn input_error() -> i32 { LOCAL_ERROR }

/// Drain bounded work on the UI thread; this never touches a widget. It is
/// safe to call while the keyboard is foreground, preserving long output.
pub(crate) unsafe fn poll_output() {
    let before = MODEL.newlines;
    for _ in 0..8 {
        let got = crate::nsh::receive(&mut RX);
        if got == 0 { break; }
        MODEL.feed(&RX[..got.min(RX.len())]);
    }
    // Keep the same historical lines visible while fresh output arrives.
    if OLDER_LINES != 0 {
        OLDER_LINES = OLDER_LINES.saturating_add(MODEL.newlines.wrapping_sub(before) as usize)
            .min(MODEL.max_offset());
    }
}

fn line_pitch(height: u32) -> u32 {
    // The stock 28px MiSans face is about 38px high. A valid custom face
    // determines geometry instead of being squeezed into a fixed 240px box.
    if (8..=256).contains(&height) { height + 4 } else { 42 }
}

unsafe fn relayout() {
    let face = fw_api::fw_style_get_prop(OUTPUT_LABEL, 0, fw_api::LV_STYLE_TEXT_FONT);
    // This is the same checked face+0x0c line-height read used by Chaos's
    // existing font support; see state::FACE_LINE_HEIGHT_OFF evidence.
    let height = if safe_ptr(face) && safe_ptr(face.wrapping_add(FACE_LINE_HEIGHT_OFF)) {
        core::ptr::read_volatile((face + FACE_LINE_HEIGHT_OFF) as *const u16) as u32
    } else { 38 };
    let pitch = line_pitch(height);
    if pitch == LAST_PITCH { return; }
    LAST_PITCH = pitch;
    fw_api::obj_set_size_wh(STATUS_LABEL, 320, pitch * 2);
    fw_api::obj_set_size_wh(OUTPUT_BOX, 328, pitch * model::PAGE_LINES as u32 + 8);
    fw_api::obj_align(OUTPUT_BOX, fw_api::align::TOP_LEFT, 4, (pitch * 2 + 8) as i32);
    for (i, &label) in OUTPUT_LINES.iter().enumerate() {
        fw_api::obj_align(label, fw_api::align::TOP_LEFT, 0, (i as u32 * pitch) as i32);
    }
    let mut anchor = OUTPUT_BOX;
    for [left, right] in [[6, 7], [0, 8], [3, 4], [1, 2], [9, 5]] {
        if !safe_ptr(TILES[left]) || !safe_ptr(TILES[right]) { continue; }
        fw_api::obj_align_to(TILES[left], anchor, fw_api::align::OUT_BOTTOM_LEFT, 0, 8);
        fw_api::obj_align_to(TILES[right], TILES[left], fw_api::align::OUT_RIGHT_MID, 8, 0);
        anchor = TILES[left];
    }
    if HINT_LABEL != 0 { fw_api::obj_align_to(HINT_LABEL, anchor, fw_api::align::OUT_BOTTOM_LEFT, 0, 8); }
}

unsafe fn paint_lines() {
    let mut offset = 0;
    for i in 0..model::PAGE_LINES {
        let mut line = [0u8; model::LINE_BYTES + 1];
        let mut n = 0;
        while OUTPUT[offset] != 0 && OUTPUT[offset] != b'\n' {
            line[n] = OUTPUT[offset];
            n += 1;
            offset += 1;
        }
        if OUTPUT[offset] == b'\n' { offset += 1; }
        if FORCE_PAINT || line != LAST_LINES[i] {
            fw_api::label_set_text(OUTPUT_LINES[i], line.as_ptr());
            LAST_LINES[i] = line;
        }
    }
}

unsafe fn refresh() {
    poll_output();
    if !visible() { return; }
    OLDER_LINES = OLDER_LINES.min(MODEL.max_offset());
    OUTPUT.fill(0);
    MODEL.page(OLDER_LINES, &mut OUTPUT);
    STATUS.fill(0);
    let mut w = Text { out: &mut STATUS, len: 0 };
    let _ = write!(w, "{} · {}", state_text(), if OLDER_LINES == 0 { "最新输出" } else { "历史输出" });
    let code = if LOCAL_ERROR != 0 { LOCAL_ERROR } else { crate::nsh::error() };
    if code != 0 { let _ = write!(w, "\n错误码 {}", code); }
    else if OLDER_LINES != 0 { let _ = write!(w, "\n向前 {} 行", OLDER_LINES); }
    if safe_ptr(OUTPUT_LABEL) && safe_ptr(OUTPUT_BOX) {
        relayout();
    }
    if OUTPUT_LINES.iter().all(|&p| safe_ptr(p)) && (FORCE_PAINT || OUTPUT != LAST_OUTPUT) {
        paint_lines();
        LAST_OUTPUT.copy_from_slice(&OUTPUT);
    }
    if safe_ptr(STATUS_LABEL) && (FORCE_PAINT || STATUS != LAST_STATUS) {
        fw_api::label_set_text(STATUS_LABEL, STATUS.as_ptr());
        LAST_STATUS.copy_from_slice(&STATUS);
    }
    FORCE_PAINT = false;
}

unsafe fn visible() -> bool {
    FG_PAGE == PAGE_PID && page_is_live(PAGE_PID as usize)
        && fw_api::screen_is_on() && fw_api::top_page_ids() == (APP_ID, PAGE_PID)
}

unsafe fn pair(ctx: &PageCtx, anchor: u32, left: (usize, &[u8]), right: (usize, &[u8])) -> Result<u32, PageError> {
    let a = build_tile_below(ctx, anchor, left.0, left.1, TILE_W, 56);
    if a == 0 { return Err(PageError::RowFailed(left.0)); }
    let b = build_tile_right(ctx, a, right.0, right.1, TILE_W, 56);
    if b == 0 { return Err(PageError::RowFailed(right.0)); }
    TILES[left.0] = a;
    TILES[right.0] = b;
    Ok(a)
}

pub(crate) struct TerminalPage;
impl crate::page::Page for TerminalPage {
    fn fill_title(&self, buf: *mut u8) { unsafe { put_str(buf, "NSH终端\0".as_bytes()); } }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            forget_ui();
            STATUS_LABEL = fw_api::label_create(ctx.content);
            if STATUS_LABEL == 0 { return Err(PageError::RowFailed(0)); }
            fw_api::style_apply(STATUS_LABEL, FW_STYLE_HR_NAME, 255, 0);
            fw_api::obj_set_size_wh(STATUS_LABEL, 320, 64);
            fw_api::obj_align(STATUS_LABEL, fw_api::align::TOP_LEFT, 8, 0);
            OUTPUT_BOX = fw_api::obj_create(ctx.content, 0);
            if OUTPUT_BOX == 0 { return Err(PageError::RowFailed(1)); }
            fw_api::obj_clear_box_spacing(OUTPUT_BOX);
            fw_api::obj_set_size_wh(OUTPUT_BOX, 328, 260);
            fw_api::obj_align(OUTPUT_BOX, fw_api::align::TOP_LEFT, 4, 72);
            fw_api::obj_add_flag(OUTPUT_BOX, fw_api::LV_OBJ_FLAG_SCROLLABLE);
            fw_api::obj_set_scroll_dir(OUTPUT_BOX, fw_api::LV_DIR_HOR);
            fw_api::obj_clear_flag(OUTPUT_BOX, fw_api::LV_OBJ_FLAG_GESTURE_BUBBLE | fw_api::LV_OBJ_FLAG_SCROLL_ELASTIC);
            // Each label keeps LVGL's natural content width: wide glyphs do
            // not soft-wrap into an unseen seventh line. Swipe horizontally
            // to read a wide line; the native page scrolls vertically.
            for i in 0..model::PAGE_LINES {
                let label = fw_api::label_create(OUTPUT_BOX);
                if label == 0 { forget_ui(); return Err(PageError::RowFailed(1)); }
                OUTPUT_LINES[i] = label;
                fw_api::style_apply(label, FW_STYLE_HR_NAME, 255, 0);
                fw_api::obj_clear_box_spacing(label);
                fw_api::obj_clear_scrollable(label);
            }
            OUTPUT_LABEL = OUTPUT_LINES[0];
            let a = pair(ctx, OUTPUT_BOX, (6, "更早输出\0".as_bytes()), (7, "更新输出\0".as_bytes()))?;
            let b = pair(ctx, a, (0, "输入命令\0".as_bytes()), (8, "发送回车\0".as_bytes()))?;
            let c = pair(ctx, b, (3, b"Ctrl-C\0"), (4, "发送 exit\0".as_bytes()))?;
            let d = pair(ctx, c, (1, "开始/重开\0".as_bytes()), (2, "清空显示\0".as_bytes()))?;
            let e = pair(ctx, d, (9, "最新输出\0".as_bytes()), (5, "返回\0".as_bytes()))?;
            let hint = fw_api::label_create(ctx.content);
            if hint != 0 {
                fw_api::style_apply(hint, FW_STYLE_HR_NAME, 255, 0);
                HINT_LABEL = hint;
                fw_api::obj_set_width(hint, 320);
                fw_api::label_set_text(hint, "命令直接操作系统，请谨慎执行。\n返回不结束会话。Ctrl-C由前台程序处理；可输入exit退出。\n此页是文本终端，不支持全屏编辑器。\0".as_ptr());
                fw_api::obj_align_to(hint, e, fw_api::align::OUT_BOTTOM_LEFT, 0, 8);
            }
            refresh();
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if !visible() { return; }
            match idx {
                0 => { crate::keyboard::open(core::ptr::null(), crate::keyboard::Completion::NshCommand); return; }
                1 => { let _ = start(); }
                2 => { MODEL.clear(); OLDER_LINES = 0; }
                3 => { let _ = send_control(3); }
                4 => { let _ = send(b"exit\n"); }
                5 => { nav_back(); return; }
                6 => { OLDER_LINES = (OLDER_LINES + model::PAGE_LINES).min(MODEL.max_offset()); }
                7 => { OLDER_LINES = OLDER_LINES.saturating_sub(model::PAGE_LINES); }
                8 => { let _ = send_control(10); }
                9 => { OLDER_LINES = 0; }
                _ => return,
            }
            refresh();
        }
    }
    fn needs_tick(&self) -> bool { true }
    fn tick(&self) { unsafe { refresh(); } }
}

#[cfg(test)]
#[path = "../tests/terminal_ui_cases.rs"]
mod tests;
