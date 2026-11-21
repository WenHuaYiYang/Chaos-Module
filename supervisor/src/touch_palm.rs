//! p67 3.101.043 关闭遮盖熄屏：开关开启时将系统 touch_palm 置零。
use crate::fw_api;
use core::sync::atomic::{AtomicU8, Ordering};

// 新语义独立保存，避免旧版“允许遮盖熄屏”的值被解释为“禁止”。
const CONFIG: &[u8] = b"/data/chaos/disable-touch-palm.cfg\0";
const TEMP: &[u8] = b"/data/chaos/disable-touch-palm.cfg.tmp\0";
static mut SAVE_FAILED: bool = false;
static VALUE: AtomicU8 = AtomicU8::new(0);

pub(crate) unsafe fn ensure_applied() {
    if VALUE.load(Ordering::Acquire) != 0 { return; }
    let value = crate::settings_store::read::<1>(CONFIG).filter(|v| v[0] <= 1).map(|v| v[0]).unwrap_or(0);
    apply(value);
    VALUE.store(value + 1, Ordering::Release);
}

unsafe fn apply(value: u8) {
    fw_api::screen_touch_palm_set(value == 0);
}

pub(crate) unsafe fn enabled() -> bool {
    ensure_applied();
    VALUE.load(Ordering::Acquire) == 2
}

pub(crate) unsafe fn toggle() {
    let value = if enabled() { 0u8 } else { 1u8 };
    apply(value);
    VALUE.store(value + 1, Ordering::Release);
    SAVE_FAILED = !crate::settings_store::save(CONFIG, TEMP, &[value]);
}
pub(crate) unsafe fn status() -> *const u8 {
    if SAVE_FAILED { "保存失败，仅本次生效\0".as_ptr() } else { core::ptr::null() }
}
#[cfg(test)]
pub(crate) unsafe fn reset_for_test() { VALUE.store(0, Ordering::Release); SAVE_FAILED = false; }
