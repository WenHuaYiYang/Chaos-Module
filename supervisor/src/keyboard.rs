//! 通用英文数字键盘。
//!
//! 页面层只负责标题和生命周期；输入缓冲、大小写、按键事件与完成动作都由本组件管理。
//! 后续页面可通过 open + 新的 Completion 变体复用同一套键盘。
//! 数字和三行字母共用横向滚动视口，输入框与操作区不参与滚动。

use crate::*;

pub(crate) const PAGE_PID: u32 = 23;
const CAPACITY: usize = 31;

const KEY_SHIFT: u32 = 0x100;
const KEY_BACKSPACE: u32 = 0x101;
const KEY_CLEAR: u32 = 0x102;
const KEY_DONE: u32 = 0x103;

const VIEWPORT_WIDTH: i32 = 336;
const KEY_WIDTH: i32 = 56;
const KEY_HEIGHT: i32 = 50;
const KEY_GAP: i32 = 8;
const ROW_STEP: i32 = KEY_HEIGHT + KEY_GAP;
const BOARD_WIDTH: i32 = 10 * (KEY_WIDTH + KEY_GAP);
const BOARD_HEIGHT: i32 = 3 * ROW_STEP + KEY_HEIGHT;
const VIEWPORT_TOP: i32 = 56;
const VIEWPORT_HEIGHT: i32 = BOARD_HEIGHT + 8; // 为原生横向滚动条留白
const ACTION_TOP: i32 = VIEWPORT_TOP + VIEWPORT_HEIGHT + 8;
const ACTION_HEIGHT: u32 = 44;

const NUMBER_KEYS: [u32; 10] = [
    b'1' as u32,
    b'2' as u32,
    b'3' as u32,
    b'4' as u32,
    b'5' as u32,
    b'6' as u32,
    b'7' as u32,
    b'8' as u32,
    b'9' as u32,
    b'0' as u32,
];
const QWERTY_KEYS: [u32; 10] = [
    b'q' as u32,
    b'w' as u32,
    b'e' as u32,
    b'r' as u32,
    b't' as u32,
    b'y' as u32,
    b'u' as u32,
    b'i' as u32,
    b'o' as u32,
    b'p' as u32,
];
const HOME_KEYS: [u32; 9] = [
    b'a' as u32,
    b's' as u32,
    b'd' as u32,
    b'f' as u32,
    b'g' as u32,
    b'h' as u32,
    b'j' as u32,
    b'k' as u32,
    b'l' as u32,
];
const BOTTOM_KEYS: [u32; 7] = [
    b'z' as u32,
    b'x' as u32,
    b'c' as u32,
    b'v' as u32,
    b'b' as u32,
    b'n' as u32,
    b'm' as u32,
];

#[derive(Clone, Copy)]
pub(crate) enum Completion {
    SystemVersion,
    SystemText,
    DeviceName,
    DeviceModel,
}

static mut TEXT: [u8; CAPACITY + 1] = [0; CAPACITY + 1];
static mut LENGTH: usize = 0;
static mut UPPERCASE: bool = true;
static mut COMPLETION: u32 = 0;
static mut INPUT_LABEL: u32 = 0;
static mut LETTER_LABELS: [u32; 26] = [0; 26];
static mut SHIFT_LABEL: u32 = 0;

pub(crate) unsafe fn open(initial: *const u8, completion: Completion) {
    LENGTH = 0;
    TEXT[0] = 0;
    if !initial.is_null() {
        while LENGTH < CAPACITY {
            let c = read_volatile(initial.add(LENGTH));
            if c == 0 {
                break;
            }
            TEXT[LENGTH] = c;
            LENGTH += 1;
        }
        TEXT[LENGTH] = 0;
    }
    UPPERCASE = true;
    COMPLETION = completion as u32;
    INPUT_LABEL = 0;
    LETTER_LABELS = [0; 26];
    SHIFT_LABEL = 0;
    nav_goto(PAGE_PID, 0);
}

pub(crate) unsafe fn title() -> &'static [u8] {
    match COMPLETION {
        x if x == Completion::SystemText as u32 => "自定义手环系统文本\0".as_bytes(),
        x if x == Completion::DeviceName as u32 => "自定义设备名称\0".as_bytes(),
        x if x == Completion::DeviceModel as u32 => "自定义手环型号\0".as_bytes(),
        _ => "自定义系统版本\0".as_bytes(),
    }
}

unsafe fn refresh_input() {
    if FG_PAGE as usize == PAGE_PID as usize
        && page_is_live(PAGE_PID as usize)
        && safe_ptr(INPUT_LABEL)
    {
        fw_api::label_set_text(INPUT_LABEL, core::ptr::addr_of!(TEXT) as *const u8);
    }
}

unsafe fn set_key_text(label: u32, code: u32) {
    if code == b' ' as u32 {
        fw_api::label_set_text(label, b"SPACE\0".as_ptr());
    } else if code <= 0xFF {
        let mut c = code as u8;
        if UPPERCASE && c.is_ascii_lowercase() {
            c = c.to_ascii_uppercase();
        }
        fw_api::label_set_text(label, [c, 0].as_ptr());
    } else {
        let text: &[u8] = match code {
            KEY_SHIFT => {
                if UPPERCASE {
                    b"ABC\0"
                } else {
                    b"abc\0"
                }
            }
            KEY_BACKSPACE => b"<\0",
            KEY_CLEAR => b"CLR\0",
            KEY_DONE => b"OK\0",
            _ => b"?\0",
        };
        fw_api::label_set_text(label, text.as_ptr());
    }
}

unsafe fn refresh_case() {
    // 只改活标签，不删对象、不重建页面，保留原生横向滚动位置。
    for i in 0..26 {
        let label = LETTER_LABELS[i];
        if safe_ptr(label) {
            set_key_text(label, b'a' as u32 + i as u32);
        }
    }
    if safe_ptr(SHIFT_LABEL) {
        set_key_text(SHIFT_LABEL, KEY_SHIFT);
    }
}

unsafe fn create_key(
    parent: u32,
    code: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    color: u32,
) -> bool {
    let key = fw_api::obj_create(parent, 0);
    if key == 0 {
        return false;
    }
    // 默认主题的内边距会压缩标签可用宽度，窄键上曾把字符裁成竖线。
    fw_api::obj_clear_box_spacing(key);
    fw_api::obj_set_size_wh(key, width, height);
    fw_api::obj_align(key, fw_api::align::TOP_LEFT, x, y);
    fw_api::obj_style_color(key, color, 0);
    fw_api::obj_clear_scrollable(key);
    fw_api::obj_add_flag(key, fw_api::LV_OBJ_FLAG_CLICKABLE);
    fw_api::obj_add_event(
        key,
        key_event as *const () as u32,
        fw_api::ev::CLICKED,
        code,
    );

    let label = fw_api::label_create(key);
    if label == 0 {
        return false;
    }
    fw_api::style_apply(label, FW_STYLE_HR_NAME, 255, 0);
    fw_api::obj_clear_scrollable(label);
    fw_api::obj_clear_flag(label, fw_api::LV_OBJ_FLAG_CLICKABLE);
    set_key_text(label, code);
    if (b'a' as u32..=b'z' as u32).contains(&code) {
        LETTER_LABELS[(code - b'a' as u32) as usize] = label;
    } else if code == KEY_SHIFT {
        SHIFT_LABEL = label;
    }
    fw_api::obj_align(label, fw_api::align::CENTER, 0, 0);
    true
}

unsafe fn create_char_row(parent: u32, keys: &[u32], y: i32) -> bool {
    // 完整一行放进同一块 640px 键盘，ASDF / ZXCV 行按常见键盘错位居中。
    // 不能按屏宽居中，否则长行的左半部分会落到负坐标而无法滑到。
    let total = keys.len() as i32 * KEY_WIDTH + (keys.len() as i32 - 1) * KEY_GAP;
    let mut x = (BOARD_WIDTH - total) / 2;
    let mut ok = true;
    for &code in keys {
        if !create_key(
            parent,
            code,
            x,
            y,
            KEY_WIDTH as u32,
            KEY_HEIGHT as u32,
            0x26_26_26,
        ) {
            ok = false;
        }
        x += KEY_WIDTH + KEY_GAP;
    }
    ok
}

pub(crate) unsafe fn render(parent: u32) -> bool {
    // 页面仍由固件管理；只有下面的 viewport 可以滚动。操作区最底边为 392，
    // 在 424px content 内保留底部 32px 手势区，输入框、SPACE/OK 不会随键盘滑走。
    INPUT_LABEL = 0;
    LETTER_LABELS = [0; 26];
    SHIFT_LABEL = 0;
    fw_api::obj_clear_scrollable(parent);
    fw_api::obj_clear_box_spacing(parent);

    INPUT_LABEL = fw_api::label_create(parent);
    if INPUT_LABEL == 0 {
        return false;
    }
    fw_api::style_apply(INPUT_LABEL, FW_STYLE_HR_NAME, 255, 0);
    fw_api::label_set_text(INPUT_LABEL, core::ptr::addr_of!(TEXT) as *const u8);
    fw_api::obj_set_size_wh(INPUT_LABEL, 320, 48);
    fw_api::obj_align(INPUT_LABEL, fw_api::align::TOP_LEFT, 8, 4);

    let viewport = fw_api::obj_create(parent, 0);
    if viewport == 0 {
        return false;
    }
    fw_api::obj_clear_box_spacing(viewport);
    fw_api::obj_set_size(viewport, VIEWPORT_WIDTH, VIEWPORT_HEIGHT);
    fw_api::obj_align(viewport, fw_api::align::TOP_LEFT, 0, VIEWPORT_TOP);
    fw_api::obj_style_color(viewport, 0, 0);
    fw_api::obj_add_flag(
        viewport,
        fw_api::LV_OBJ_FLAG_SCROLLABLE | fw_api::LV_OBJ_FLAG_CLICKABLE,
    );
    fw_api::obj_set_scroll_dir(viewport, fw_api::LV_DIR_HOR);
    // 到边界就停；不要把横向拖动继续传给页面触发右滑返回。标题返回键保留。
    fw_api::obj_clear_flag(
        viewport,
        fw_api::LV_OBJ_FLAG_SCROLL_CHAIN
            | fw_api::LV_OBJ_FLAG_GESTURE_BUBBLE
            | fw_api::LV_OBJ_FLAG_SCROLL_ELASTIC,
    );

    let board = fw_api::obj_create(viewport, 0);
    if board == 0 {
        return false;
    }
    fw_api::obj_clear_box_spacing(board);
    fw_api::obj_clear_scrollable(board);
    fw_api::obj_set_size(board, BOARD_WIDTH, BOARD_HEIGHT);
    fw_api::obj_align(board, fw_api::align::TOP_LEFT, 0, 0);
    fw_api::obj_style_color(board, 0, 0);

    let mut ok = create_char_row(board, &NUMBER_KEYS, 0);
    if !create_char_row(board, &QWERTY_KEYS, ROW_STEP) {
        ok = false;
    }
    if !create_char_row(board, &HOME_KEYS, 2 * ROW_STEP) {
        ok = false;
    }
    if !create_char_row(board, &BOTTOM_KEYS, 3 * ROW_STEP) {
        ok = false;
    }

    if !create_key(
        parent,
        KEY_SHIFT,
        8,
        ACTION_TOP,
        80,
        ACTION_HEIGHT,
        0x40_40_40,
    ) {
        ok = false;
    }
    if !create_key(
        parent,
        b' ' as u32,
        96,
        ACTION_TOP,
        144,
        ACTION_HEIGHT,
        0x26_26_26,
    ) {
        ok = false;
    }
    if !create_key(
        parent,
        KEY_BACKSPACE,
        248,
        ACTION_TOP,
        80,
        ACTION_HEIGHT,
        0x40_40_40,
    ) {
        ok = false;
    }

    let bottom = ACTION_TOP + ACTION_HEIGHT as i32 + KEY_GAP;
    if !create_key(parent, KEY_CLEAR, 8, bottom, 80, ACTION_HEIGHT, 0x40_40_40) {
        ok = false;
    }
    if !create_key(
        parent,
        b'.' as u32,
        96,
        bottom,
        56,
        ACTION_HEIGHT,
        0x26_26_26,
    ) {
        ok = false;
    }
    if !create_key(
        parent,
        KEY_DONE,
        160,
        bottom,
        168,
        ACTION_HEIGHT,
        0x53_81_FE,
    ) {
        ok = false;
    }
    ok
}

unsafe extern "C" fn key_event(event: u32) {
    if event == 0
        || FG_PAGE != PAGE_PID
        || !page_is_live(PAGE_PID as usize)
        || fw_api::event_get_code(event) != fw_api::ev::CLICKED
    {
        return;
    }
    let code = fw_api::event_user_data(event);
    match code {
        KEY_SHIFT => {
            UPPERCASE = !UPPERCASE;
            refresh_case();
            return;
        }
        KEY_BACKSPACE => {
            if LENGTH != 0 {
                LENGTH -= 1;
                TEXT[LENGTH] = 0;
            }
        }
        KEY_CLEAR => {
            LENGTH = 0;
            TEXT[0] = 0;
        }
        KEY_DONE => {
            if LENGTH == 0 {
                return;
            }
            let saved = match COMPLETION {
                x if x == Completion::SystemVersion as u32 => {
                    crate::system_version::save_custom(core::ptr::addr_of!(TEXT) as *const u8)
                }
                x if x == Completion::SystemText as u32 => {
                    crate::system_text::save_custom(core::ptr::addr_of!(TEXT) as *const u8)
                }
                x if x == Completion::DeviceName as u32 => {
                    crate::device_name::save_custom(core::ptr::addr_of!(TEXT) as *const u8)
                }
                x if x == Completion::DeviceModel as u32 => {
                    crate::device_model::save_custom(core::ptr::addr_of!(TEXT) as *const u8)
                }
                _ => false,
            };
            if saved {
                nav_back();
            }
            return;
        }
        0x20..=0xFF => {
            if LENGTH < CAPACITY {
                let mut c = code as u8;
                if UPPERCASE && c.is_ascii_lowercase() {
                    c -= b'a' - b'A';
                }
                TEXT[LENGTH] = c;
                LENGTH += 1;
                TEXT[LENGTH] = 0;
            }
        }
        _ => return,
    }
    refresh_input();
}

#[cfg(test)]
#[path = "../tests/keyboard_cases.rs"]
mod tests;
