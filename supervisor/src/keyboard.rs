//! 通用英文数字键盘。
//!
//! 页面层只负责标题和生命周期；输入缓冲、大小写、按键事件与完成动作都由本组件管理。
//! 后续页面可通过 open + 新的 Completion 变体复用同一套键盘。

use crate::*;

pub(crate) const PAGE_PID: u32 = 23;
const CAPACITY: usize = 31;

const KEY_SHIFT: u32 = 0x100;
const KEY_BACKSPACE: u32 = 0x101;
const KEY_CLEAR: u32 = 0x102;
const KEY_DONE: u32 = 0x103;

const NUMBER_KEYS: [u32; 10] = [
    b'1' as u32, b'2' as u32, b'3' as u32, b'4' as u32, b'5' as u32,
    b'6' as u32, b'7' as u32, b'8' as u32, b'9' as u32, b'0' as u32,
];
const QWERTY_KEYS: [u32; 10] = [
    b'q' as u32, b'w' as u32, b'e' as u32, b'r' as u32, b't' as u32,
    b'y' as u32, b'u' as u32, b'i' as u32, b'o' as u32, b'p' as u32,
];
const HOME_KEYS: [u32; 9] = [
    b'a' as u32, b's' as u32, b'd' as u32, b'f' as u32, b'g' as u32,
    b'h' as u32, b'j' as u32, b'k' as u32, b'l' as u32,
];
const BOTTOM_KEYS: [u32; 7] = [
    b'z' as u32, b'x' as u32, b'c' as u32, b'v' as u32,
    b'b' as u32, b'n' as u32, b'm' as u32,
];

#[derive(Clone, Copy)]
pub(crate) enum Completion {
    SystemVersion,
    SystemText,
}

static mut TEXT: [u8; CAPACITY + 1] = [0; CAPACITY + 1];
static mut LENGTH: usize = 0;
static mut UPPERCASE: bool = true;
static mut COMPLETION: u32 = 0;
static mut INPUT_LABEL: u32 = 0;

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
    nav_goto(PAGE_PID, 0);
}

pub(crate) unsafe fn title() -> &'static [u8] {
    if COMPLETION == Completion::SystemText as u32 {
        "自定义手环系统文本\0".as_bytes()
    } else {
        "自定义系统版本\0".as_bytes()
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
    fw_api::obj_set_size_wh(key, width, height);
    fw_api::obj_align(key, fw_api::align::TOP_LEFT, x, y);
    fw_api::obj_style_color(key, color, 0);
    fw_api::obj_clear_scrollable(key);
    fw_api::obj_add_flag(key, 0x2); // LV_OBJ_FLAG_CLICKABLE
    fw_api::obj_add_event(key, key_event as *const () as u32, fw_api::ev::CLICKED, code);

    let label = fw_api::label_create(key);
    if label == 0 {
        return false;
    }
    fw_api::style_apply(label, FW_STYLE_HR_NAME, 255, 0);
    if code <= 0xFF {
        let mut c = code as u8;
        if UPPERCASE && c.is_ascii_lowercase() {
            c -= b'a' - b'A';
        }
        let text = [c, 0];
        fw_api::label_set_text(label, text.as_ptr());
    } else {
        let text: &[u8] = match code {
            KEY_SHIFT => if UPPERCASE { b"ABC\0" } else { b"abc\0" },
            KEY_BACKSPACE => b"<\0",
            KEY_CLEAR => b"CLR\0",
            KEY_DONE => b"OK\0",
            _ => b"?\0",
        };
        fw_api::label_set_text(label, text.as_ptr());
    }
    fw_api::obj_align(label, fw_api::align::CENTER, 0, 0);
    true
}

unsafe fn create_char_row(parent: u32, keys: &[u32], y: i32) -> bool {
    // 旧布局每行 10 个 28px 按键，主题内边距占掉大部分宽度，字母被裁成竖线。
    // 每行最多 5 个 56px 大按键，保证字符及 CLR 等操作文字都有完整显示空间。
    const WIDTH: i32 = 56;
    const HEIGHT: u32 = 52;
    const GAP: i32 = 8;
    let total = keys.len() as i32 * WIDTH + (keys.len() as i32 - 1) * GAP;
    let mut x = (336 - total) / 2;
    let mut ok = true;
    for &code in keys {
        if !create_key(parent, code, x, y, WIDTH as u32, HEIGHT, 0x26_26_26) {
            ok = false;
        }
        x += WIDTH + GAP;
    }
    ok
}

unsafe fn create_char_rows(parent: u32, keys: &[u32], y: i32) -> bool {
    const ROW_STEP: i32 = 60;
    let mut ok = true;
    for (row, chunk) in keys.chunks(5).enumerate() {
        if !create_char_row(parent, chunk, y + row as i32 * ROW_STEP) {
            ok = false;
        }
    }
    ok
}

pub(crate) unsafe fn render(parent: u32) -> bool {
    // content 默认带原生纵向滚动；按键自身不可滚动，把拖动手势交给父容器。
    // 所有按键使用绝对坐标，最底行会自然撑出可滚动区域。
    fw_api::obj_add_flag(parent, 0x10); // LV_OBJ_FLAG_SCROLLABLE
    fw_api::content_pad_bottom(parent, 16, 0);

    INPUT_LABEL = fw_api::label_create(parent);
    if INPUT_LABEL == 0 {
        return false;
    }
    fw_api::style_apply(INPUT_LABEL, FW_STYLE_HR_NAME, 255, 0);
    fw_api::label_set_text(INPUT_LABEL, core::ptr::addr_of!(TEXT) as *const u8);
    fw_api::obj_set_size_wh(INPUT_LABEL, 320, 48);
    fw_api::obj_align(INPUT_LABEL, fw_api::align::TOP_LEFT, 8, 4);

    let mut ok = create_char_rows(parent, &NUMBER_KEYS, 64);
    if !create_char_rows(parent, &QWERTY_KEYS, 184) { ok = false; }
    if !create_char_rows(parent, &HOME_KEYS, 304) { ok = false; }
    if !create_char_rows(parent, &BOTTOM_KEYS, 424) { ok = false; }

    if !create_key(parent, KEY_SHIFT, 20, 544, 88, 52, 0x40_40_40) { ok = false; }
    if !create_key(parent, KEY_BACKSPACE, 228, 544, 88, 52, 0x40_40_40) { ok = false; }

    if !create_key(parent, b'.' as u32, 20, 604, 56, 52, 0x26_26_26) { ok = false; }
    if !create_key(parent, KEY_CLEAR, 84, 604, 88, 52, 0x40_40_40) { ok = false; }
    if !create_key(parent, KEY_DONE, 180, 604, 136, 52, 0x53_81_FE) { ok = false; }
    ok
}

unsafe extern "C" fn key_event(event: u32) {
    if event == 0 || fw_api::event_get_code(event) != fw_api::ev::CLICKED {
        return;
    }
    let code = fw_api::event_user_data(event);
    match code {
        KEY_SHIFT => {
            UPPERCASE = !UPPERCASE;
            render_req(PAGE_PID as usize);
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
                x if x == Completion::SystemVersion as u32 => crate::system_version::save_custom(
                    core::ptr::addr_of!(TEXT) as *const u8,
                ),
                x if x == Completion::SystemText as u32 => crate::system_text::save_custom(
                    core::ptr::addr_of!(TEXT) as *const u8,
                ),
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
