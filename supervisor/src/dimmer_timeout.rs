//! p67 3.101.043: screen 服务 +0xD4 为 dimmer_timeout（毫秒）。
//! 原 setter 0x0CA72288 写字段后调用 0x0CA70360 更新计时；这里仅改内存。
use crate::*;

static mut INITIAL: [u8; 16] = [0; 16];
const TIMEOUT: *mut u32 = 0x2013F43C as *mut u32;

pub(crate) unsafe fn open() {
    let p = core::ptr::addr_of_mut!(INITIAL) as *mut u8;
    let mut w = W::new(p, 16);
    w.n(core::ptr::read_volatile(TIMEOUT) / 1000);
    w.end();
    keyboard::open(p, keyboard::Completion::DimmerTimeout);
}

pub(crate) unsafe fn save(text: *const u8) -> bool {
    let mut seconds = 0u32;
    let mut n = 0;
    loop {
        let c = core::ptr::read_volatile(text.add(n));
        if c == 0 { break; }
        if !c.is_ascii_digit() || n >= 7 { return false; }
        seconds = seconds * 10 + (c - b'0') as u32;
        n += 1;
    }
    if seconds == 0 || seconds > (i32::MAX as u32) / 1000 { return false; }
    core::ptr::write_volatile(TIMEOUT, seconds * 1000);
    let refresh: unsafe extern "C" fn() = core::mem::transmute(0x0CA70361usize);
    refresh();
    true
}
