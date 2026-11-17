//! Settings -> cursor switch, movement/click preferences and page pause duration.
use crate::page::*;
use crate::page::kit::{MenuSpec, RowKind, RowModel, RowSub, ToggleDef};
use core::sync::atomic::{AtomicUsize, Ordering};

pub(crate) struct GyroPointerPage;
pub(crate) static PAGE: GyroPointerPage = GyroPointerPage;
static LAST_STATUS: AtomicUsize = AtomicUsize::new(0);
static mut EDITING: usize = 0;
static mut INITIAL: [u8; 16] = [0; 16];
static mut VALUES: [[u8; 64]; 4] = [[0; 64]; 4];

pub(crate) unsafe fn title() -> &'static [u8] {
    match EDITING { 0 => "移动灵敏度（%）\0".as_bytes(),
                   1 => "点击确认时间（毫秒）\0".as_bytes(),
                   2 => "点击位移（像素）\0".as_bytes(),
                   _ => "切页暂停（秒）\0".as_bytes() }
}
unsafe fn open(index: usize) {
    EDITING = index;
    let mut w = W::new(core::ptr::addr_of_mut!(INITIAL).cast(), 16);
    w.n(match index { 0 => crate::gyro_pointer::sensitivity(),
                     1 => crate::gyro_pointer::confirm_ms(), 2 => crate::gyro_pointer::tolerance_px(),
                     _ => crate::gyro_pointer::page_pause_s() });
    w.end();
    crate::keyboard::open(core::ptr::addr_of!(INITIAL).cast(), crate::keyboard::Completion::GyroPointer);
}
unsafe fn open_sensitivity() { open(0); }
unsafe fn open_confirm() { open(1); }
unsafe fn open_tolerance() { open(2); }
unsafe fn open_pause() { open(3); }
unsafe fn value(index: usize) -> *const u8 {
    let out = core::ptr::addr_of_mut!(VALUES[index]).cast();
    let mut w = W::new(out, 64);
    if index == 0 {
        w.n(crate::gyro_pointer::sensitivity()).s(b"%  (").n(crate::gyro_pointer::SENSITIVITY_MIN)
            .c(b'~').n(crate::gyro_pointer::SENSITIVITY_MAX).c(b')');
    } else if index == 1 {
        w.n(crate::gyro_pointer::confirm_ms()).s(" 毫秒  (".as_bytes()).n(crate::gyro_pointer::CONFIRM_MIN_MS)
            .c(b'~').n(crate::gyro_pointer::CONFIRM_MAX_MS).c(b')');
    } else if index == 2 {
        w.n(crate::gyro_pointer::tolerance_px()).s(" 像素  (".as_bytes()).n(crate::gyro_pointer::TOLERANCE_MIN_PX)
            .c(b'~').n(crate::gyro_pointer::TOLERANCE_MAX_PX).c(b')');
    } else {
        w.n(crate::gyro_pointer::page_pause_s()).s(" 秒  (".as_bytes()).n(crate::gyro_pointer::PAGE_PAUSE_MIN_S)
            .c(b'~').n(crate::gyro_pointer::PAGE_PAUSE_MAX_S).c(b')');
    }
    w.end(); out
}
unsafe fn sensitivity_text() -> *const u8 { value(0) }
unsafe fn confirm_text() -> *const u8 { value(1) }
unsafe fn tolerance_text() -> *const u8 { value(2) }
unsafe fn pause_text() -> *const u8 { value(3) }

pub(crate) unsafe fn save(text: *const u8) -> bool {
    if text.is_null() { return false; }
    let mut bytes = [0u8; 4];
    let mut n = 0;
    loop {
        let byte = core::ptr::read_volatile(text.add(n));
        if byte == 0 { break; }
        if n == bytes.len() { return crate::gyro_pointer::set_parameter(EDITING, &[]); }
        bytes[n] = byte; n += 1;
    }
    if !crate::gyro_pointer::set_parameter(EDITING, &bytes[..n]) { return false; }
    render_req(Pid::GyroPointer as u32);
    true
}
pub(crate) unsafe fn error_text() -> *const u8 {
    if crate::gyro_pointer::save_failed() { "保存失败，请重试\0".as_ptr() }
    else if EDITING == 0 { "请输入25～300\0".as_ptr() }
    else if EDITING == 1 { "请输入300～5000毫秒\0".as_ptr() }
    else if EDITING == 2 { "请输入1～30像素\0".as_ptr() }
    else { "请输入0～10秒\0".as_ptr() }
}

static ENABLE: ToggleDef = ToggleDef {
    get: crate::gyro_pointer::enabled, toggle: crate::gyro_pointer::toggle,
    status: Some(crate::gyro_pointer::status),
};
static ROWS: &[RowModel] = &[
    RowModel::toggle("启用触控点\0".as_bytes(), &ENABLE),
    RowModel { label: "移动灵敏度\0".as_bytes(), sub: RowSub::Dyn(sensitivity_text),
               kind: RowKind::Fn(open_sensitivity), trailing: 0, refresh: None },
    RowModel { label: "点击确认时间\0".as_bytes(), sub: RowSub::Dyn(confirm_text),
               kind: RowKind::Fn(open_confirm), trailing: 0, refresh: None },
    RowModel { label: "点击位移灵敏度\0".as_bytes(), sub: RowSub::Dyn(tolerance_text),
               kind: RowKind::Fn(open_tolerance), trailing: 0, refresh: None },
    RowModel { label: "切页暂停时间\0".as_bytes(), sub: RowSub::Dyn(pause_text),
               kind: RowKind::Fn(open_pause), trailing: 0, refresh: None },
    RowModel::note("移动灵敏度控制速度。点击位移灵敏度是允许晃动的半径，数值越大越宽松。切页暂停0秒表示无额外等待，无需静止。碰边立即同向滑动。参数自动保存。\0".as_bytes()),
];

impl MenuSpec for GyroPointerPage { fn rows(&self) -> &'static [RowModel] { ROWS } }
impl Page for GyroPointerPage {
    fn fill_title(&self, out: *mut u8) { unsafe { put_str(out, "触控点\0".as_bytes()); } }
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            let result = frame::build(self, ctx, ctx.title);
            if result.is_ok() { LAST_STATUS.store(crate::gyro_pointer::status() as usize, Ordering::Release); }
            result
        }
    }
    fn on_click(&self, index: usize) { unsafe { kit::menu_click(self, index); } }
    fn is_switch_row(&self, index: usize) -> bool { kit::menu_is_switch(self, index) }
    fn on_switch(&self, index: usize) { unsafe { kit::menu_switch(self, index); } }
    fn needs_tick(&self) -> bool {
        unsafe {
            st_rd!(APP_FG) != 0 && st_rd!(FG_PAGE) == Pid::GyroPointer as u32 && fw_api::screen_is_on()
                && LAST_STATUS.load(Ordering::Acquire) != crate::gyro_pointer::status() as usize
        }
    }
    fn tick(&self) {
        unsafe {
            let pid = Pid::GyroPointer as usize;
            if st_rd!(APP_FG) == 0 || st_rd!(FG_PAGE) as usize != pid
                || PAGES[pid].built == 0 || !page_is_live(pid) { return; }
            let row = read_volatile(page_rows_ptr(pid));
            if row == 0 { return; }
            let status = crate::gyro_pointer::status();
            fw_api::row_update(row, fw_api::NONE_PTR, fw_api::NONE_PTR, status, 0,
                               crate::gyro_pointer::enabled() as u8);
            LAST_STATUS.store(status as usize, Ordering::Release);
        }
    }
}
