//! p67 3.101.043 遮盖熄屏：直接修改屏幕服务的运行时配置。
use crate::fw_api;
use core::sync::atomic::{AtomicU8, Ordering};

const CONFIG: &[u8] = b"/data/chaos/touch-palm.cfg\0";
static VALUE: AtomicU8 = AtomicU8::new(0);

pub(crate) unsafe fn ensure_applied() {
    if VALUE.load(Ordering::Acquire) != 0 { return; }
    let mut value = 0u8;
    let fd = fw_api::open(CONFIG.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd >= 0 {
        if fw_api::read(fd, &mut value, 1) != 1 || value != 1 { value = 0; }
        fw_api::close(fd);
    }
    apply(value);
    VALUE.store(value + 1, Ordering::Release);
}

unsafe fn apply(value: u8) {
    // 0x0CA7290C 写 +D2，随后调用 0x0CA714B4 刷新触摸策略。
    // 跳过原 setter 的持久属性写入，只更新内存。
    core::ptr::write_volatile(0x2013F43Ausize as *mut u8, value);
    let refresh: unsafe extern "C" fn() = core::mem::transmute(0x0CA714B5usize);
    refresh();
}

pub(crate) unsafe fn enabled() -> bool {
    ensure_applied();
    VALUE.load(Ordering::Acquire) == 2
}

pub(crate) unsafe fn toggle() {
    let value = if enabled() { 0u8 } else { 1u8 };
    apply(value);
    VALUE.store(value + 1, Ordering::Release);
    let fd = fw_api::open(CONFIG.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd >= 0 {
        fw_api::write(fd, &value, 1);
        fw_api::close(fd);
    }
}
