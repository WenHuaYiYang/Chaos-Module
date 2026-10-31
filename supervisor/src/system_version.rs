//! 设置 → 我的设备 → 关于本机：“OS版本”下方的显示值（3.101.043）。
//!
//! 只包装关于本机页面的 on_create，在原页面创建完成后一并改写设备名称、手环型号、
//! OS 版本与手环系统文本值标签。不全局拦截 lvx_label_set_text，其他页面和资源文本不受影响。

use crate::*;

pub(crate) const PAGE_PID: u32 = 22;

const DESCRIPTOR: u32 = 0x200D_2030;
const PAGE_KEY: u32 = 0x0016_0018;
const CREATE_SLOT: u32 = DESCRIPTOR + 0x4C;
const ORIGINAL_CREATE: u32 = 0x0C54_D635;
const DEVICE_NAME_LABEL_SLOT: u32 = 0x2011_1C30; // “设备名称”标题下方的值标签
const DEVICE_MODEL_LABEL_SLOT: u32 = 0x2011_1C38; // “手环型号”标题下方的值标签
const LABEL_SLOT: u32 = 0x2011_1C48; // “OS版本”标题下方的值标签
const SYSTEM_TEXT_LABEL_SLOT: u32 = 0x2011_1C50; // 手环系统文本值标签
const SELECT_CONFIG: &[u8] = b"/data/chaos/system-version.cfg\0";
const CUSTOM_CONFIG: &[u8] = b"/data/chaos/system-version.txt\0";
const CUSTOM_CAPACITY: usize = 31;

static PRESETS: [&[u8]; 3] = [
    b"3.101.043\0",
    b"4.103.022\0",
    b"91.78.13\0",
];

static mut SELECTED: u8 = 0;
static mut CUSTOM: [u8; CUSTOM_CAPACITY + 1] = [0; CUSTOM_CAPACITY + 1];
static mut CUSTOM_LEN: usize = 0;
static mut LOADED: bool = false;
static mut INSTALLED: bool = false;

#[inline]
fn valid_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'.'
}

pub(crate) unsafe fn ensure_loaded() {
    if LOADED {
        return;
    }
    LOADED = true;

    let fd = fw_api::open(SELECT_CONFIG.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd >= 0 {
        let mut value = 0u8;
        if fw_api::read(fd, &mut value, 1) == 1 && value <= 3 {
            SELECTED = value;
        }
        fw_api::close(fd);
    }

    let fd = fw_api::open(CUSTOM_CONFIG.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd >= 0 {
        let mut input = [0u8; CUSTOM_CAPACITY];
        let got = fw_api::read(fd, input.as_mut_ptr(), CUSTOM_CAPACITY as u32);
        fw_api::close(fd);
        let mut out = 0usize;
        if got > 0 {
            for &c in input.iter().take(got as usize) {
                if valid_byte(c) {
                    CUSTOM[out] = c;
                    out += 1;
                }
            }
        }
        CUSTOM[out] = 0;
        CUSTOM_LEN = out;
    }
    if SELECTED == 3 && CUSTOM_LEN == 0 {
        SELECTED = 0;
    }
}

fn persist_selection() {
    unsafe {
        let fd = fw_api::open(
            SELECT_CONFIG.as_ptr(),
            fw_api::oflag::WRONLY | fw_api::oflag::CREAT,
            0o640,
        );
        if fd >= 0 {
            let value = SELECTED;
            let _ = fw_api::write(fd, &value, 1);
            fw_api::close(fd);
        }
    }
}

fn persist_custom() {
    unsafe {
        let fd = fw_api::open(
            CUSTOM_CONFIG.as_ptr(),
            fw_api::oflag::WRONLY | fw_api::oflag::CREAT,
            0o640,
        );
        if fd >= 0 {
            // 固定覆写整个缓冲区，较短的自定义值不会留下上一次保存的尾部字节。
            let _ = fw_api::write(
                fd,
                core::ptr::addr_of!(CUSTOM) as *const u8,
                (CUSTOM_CAPACITY + 1) as u32,
            );
            fw_api::close(fd);
        }
    }
}

pub(crate) unsafe fn selected() -> usize {
    ensure_loaded();
    SELECTED as usize
}

pub(crate) unsafe fn custom_display() -> *const u8 {
    ensure_loaded();
    if CUSTOM_LEN == 0 {
        "未设置\0".as_bytes().as_ptr()
    } else {
        core::ptr::addr_of!(CUSTOM) as *const u8
    }
}

pub(crate) unsafe fn choose(index: usize) {
    ensure_loaded();
    if index < PRESETS.len() {
        SELECTED = index as u8;
        persist_selection();
    }
}

pub(crate) unsafe fn edit_custom() {
    ensure_loaded();
    let initial: *const u8 = if CUSTOM_LEN == 0 {
        b"\0".as_ptr()
    } else {
        core::ptr::addr_of!(CUSTOM) as *const u8
    };
    crate::keyboard::open(initial, crate::keyboard::Completion::SystemVersion);
}

/// 键盘完成动作调用。只有非空且全部为英文、数字或小数点的值才会保存。
pub(crate) unsafe fn save_custom(text: *const u8) -> bool {
    ensure_loaded();
    if text.is_null() {
        return false;
    }
    let mut len = 0usize;
    while len <= CUSTOM_CAPACITY {
        let c = read_volatile(text.add(len));
        if c == 0 {
            break;
        }
        if len == CUSTOM_CAPACITY || !valid_byte(c) {
            return false;
        }
        CUSTOM[len] = c;
        len += 1;
    }
    if len == 0 {
        return false;
    }
    CUSTOM[len] = 0;
    CUSTOM_LEN = len;
    SELECTED = 3;
    persist_custom();
    persist_selection();
    true
}

unsafe fn current_text() -> *const u8 {
    ensure_loaded();
    if SELECTED == 3 && CUSTOM_LEN != 0 {
        core::ptr::addr_of!(CUSTOM) as *const u8
    } else {
        PRESETS[(SELECTED as usize).min(PRESETS.len() - 1)].as_ptr()
    }
}

/// 设备名称保持固定可用宽度，仅让高度随换行后的内容变化。
/// 禁止标签自身滚动，避免长文本被 LVGL 当作可横向拖动的内容。
unsafe fn set_device_name(label: u32) {
    const MAX_LINE_WIDTH: u32 = 280;
    const DISPLAY_CAPACITY: usize = 511;
    const LV_SIZE_CONTENT: i32 = 0x7FFF_FFFF;
    static mut DISPLAY: [u8; DISPLAY_CAPACITY + 1] = [0; DISPLAY_CAPACITY + 1];

    let src = crate::device_name::current_text();
    let out = core::ptr::addr_of_mut!(DISPLAY) as *mut u8;
    let mut input = 0usize;
    let mut written = 0usize;
    let mut line_width = 0u32;
    loop {
        let c = read_volatile(src.add(input));
        if c == 0 || written >= DISPLAY_CAPACITY {
            break;
        }
        let (char_len, char_width) = if c < 0x80 {
            let width = match c {
                b' ' | b'i' | b'j' | b'l' | b'I' | b'.' | b',' | b':' | b';' | b'!' => 6,
                b'a'..=b'h' | b'k'..=b'z' => 14,
                b'0'..=b'9' => 16,
                b'A'..=b'Z' => 20,
                _ => 24,
            };
            (1usize, width)
        } else if c & 0xE0 == 0xC0 {
            (2, 24)
        } else if c & 0xF0 == 0xE0 {
            (3, 24)
        } else if c & 0xF8 == 0xF0 {
            (4, 24)
        } else {
            (1, 24)
        };
        if line_width != 0 && line_width + char_width > MAX_LINE_WIDTH {
            write_volatile(out.add(written), b'\n');
            written += 1;
            line_width = 0;
        }
        let mut copied = 0usize;
        while copied < char_len && written < DISPLAY_CAPACITY {
            write_volatile(out.add(written), read_volatile(src.add(input + copied)));
            written += 1;
            copied += 1;
        }
        if copied != char_len {
            break;
        }
        input += char_len;
        line_width += char_width;
    }
    write_volatile(out.add(written), 0);
    fw_api::label_set_text(label, out);
    // LVGL 9 的 LV_SIZE_CONTENT；043 固件也以 0x7FFFFFFF 作为坐标上限。
    fw_api::obj_set_size(label, 304, LV_SIZE_CONTENT);
    fw_api::obj_clear_scrollable(label);
}

/// 原固件只在页面创建时对齐一次。自定义文本改变标签高度后，必须按原来的
/// 相对位置关系重排整链，才能同时消除短文本留白和长文本与后续字段重叠。
unsafe fn realign_about_fields() {
    const OUT_BOTTOM_MID: u32 = 14;
    const PAIRS: &[(u32, u32, i32)] = &[
        (DEVICE_NAME_LABEL_SLOT, 0x2011_1C2C, 0),
        (0x2011_1C34, DEVICE_NAME_LABEL_SLOT, 20),
        (DEVICE_MODEL_LABEL_SLOT, 0x2011_1C34, 0),
        (0x2011_1C4C, DEVICE_MODEL_LABEL_SLOT, 20),
        (SYSTEM_TEXT_LABEL_SLOT, 0x2011_1C4C, 0),
        (0x2011_1C44, SYSTEM_TEXT_LABEL_SLOT, 20),
        (LABEL_SLOT, 0x2011_1C44, 0),
        (0x2011_1C3C, LABEL_SLOT, 20),
        (0x2011_1C40, 0x2011_1C3C, 0),
        (0x2011_1C64, 0x2011_1C40, 20),
        (0x2011_1C68, 0x2011_1C64, 0),
    ];
    for &(object_slot, base_slot, offset_y) in PAIRS {
        let object = read_volatile(object_slot as *const u32);
        let base = read_volatile(base_slot as *const u32);
        if safe_ptr(object) && safe_ptr(base) {
            fw_api::obj_align_to(object, base, OUT_BOTTOM_MID, 0, offset_y);
        }
    }
}

unsafe extern "C" fn create(page: u32, root: u32, start: u32) -> i32 {
    let original: unsafe extern "C" fn(u32, u32, u32) -> i32 =
        core::mem::transmute(ORIGINAL_CREATE as usize);
    let result = original(page, root, start);
    let device_name_label = read_volatile(DEVICE_NAME_LABEL_SLOT as *const u32);
    if safe_ptr(device_name_label) {
        set_device_name(device_name_label);
    }
    let device_model_label = read_volatile(DEVICE_MODEL_LABEL_SLOT as *const u32);
    if safe_ptr(device_model_label) {
        fw_api::label_set_text(device_model_label, crate::device_model::current_text());
    }
    let label = read_volatile(LABEL_SLOT as *const u32);
    if safe_ptr(label) {
        fw_api::label_set_text(label, current_text());
    }
    let system_text_label = read_volatile(SYSTEM_TEXT_LABEL_SLOT as *const u32);
    if safe_ptr(system_text_label) {
        fw_api::label_set_text(system_text_label, crate::system_text::current_text());
    }
    realign_about_fields();
    result
}

unsafe fn install() -> i32 {
    if read_volatile((DESCRIPTOR + 0x14) as *const u32) != PAGE_KEY {
        return -301;
    }
    let replacement = create as *const () as u32 | 1;
    let current = read_volatile(CREATE_SLOT as *const u32);
    if current == replacement {
        INSTALLED = true;
        return 0;
    }
    // 固件不符或其他模块已经接管时保持原槽，不覆盖未知函数。
    if INSTALLED || current != ORIGINAL_CREATE {
        return -301;
    }
    write_volatile(CREATE_SLOT as *mut u32, replacement);
    INSTALLED = read_volatile(CREATE_SLOT as *const u32) == replacement;
    if INSTALLED { 0 } else { -301 }
}

pub(crate) unsafe fn tick() {
    ensure_loaded();
    if !INSTALLED {
        let _ = install();
    }
}
