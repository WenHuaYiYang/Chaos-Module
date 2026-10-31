//! 关于本机：“设备名称”显示值。
//!
//! 负责预设、自定义内容与独立持久化；实际标签由 system_version 中关于本机
//! on_create 包装器统一更新。

use crate::*;

pub(crate) const PAGE_PID: u32 = 26;

const SELECT_CONFIG: &[u8] = b"/data/chaos/device-name.cfg\0";
const CUSTOM_CONFIG: &[u8] = b"/data/chaos/device-name.txt\0";
const CUSTOM_CAPACITY: usize = 31;
const CUSTOM_INDEX: u8 = 5;

static PRESETS: [&[u8]; 5] = [
    "小米手环10 Pro\0".as_bytes(),
    "小米手环10 Pro 陶瓷版\0".as_bytes(),
    "小米手环11 Pro\0".as_bytes(),
    "小米手环11 Pro 陶瓷版\0".as_bytes(),
    "三星note7\0".as_bytes(),
];

static mut SELECTED: u8 = 0;
static mut CUSTOM: [u8; CUSTOM_CAPACITY + 1] = [0; CUSTOM_CAPACITY + 1];
static mut CUSTOM_LEN: usize = 0;
static mut LOADED: bool = false;

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
        if fw_api::read(fd, &mut value, 1) == 1 && value <= CUSTOM_INDEX {
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
    if SELECTED == CUSTOM_INDEX && CUSTOM_LEN == 0 {
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
    crate::keyboard::open(initial, crate::keyboard::Completion::DeviceName);
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
    SELECTED = CUSTOM_INDEX;
    persist_custom();
    persist_selection();
    true
}

pub(crate) unsafe fn current_text() -> *const u8 {
    ensure_loaded();
    if SELECTED == CUSTOM_INDEX && CUSTOM_LEN != 0 {
        core::ptr::addr_of!(CUSTOM) as *const u8
    } else {
        PRESETS[(SELECTED as usize).min(PRESETS.len() - 1)].as_ptr()
    }
}
