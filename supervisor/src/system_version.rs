//! 设置 → 关于本机：系统版本显示值（3.101.043）。
//!
//! 只包装关于本机页面的 on_create，在原页面创建完成后改写系统版本标签。
//! 不全局拦截 lvx_label_set_text，因此其他页面和资源文本不受影响。

use crate::*;

pub(crate) const PAGE_PID: u32 = 22;

const DESCRIPTOR: u32 = 0x200D_2030;
const PAGE_KEY: u32 = 0x0016_0018;
const CREATE_SLOT: u32 = DESCRIPTOR + 0x4C;
const ORIGINAL_CREATE: u32 = 0x0C54_D635;
const LABEL_SLOT: u32 = 0x2011_1C50;
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

unsafe extern "C" fn create(page: u32, root: u32, start: u32) -> i32 {
    let original: unsafe extern "C" fn(u32, u32, u32) -> i32 =
        core::mem::transmute(ORIGINAL_CREATE as usize);
    let result = original(page, root, start);
    let label = read_volatile(LABEL_SLOT as *const u32);
    if safe_ptr(label) {
        fw_api::label_set_text(label, current_text());
    }
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
