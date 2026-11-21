//! Module-owned timeout preference; restore RAM once per injection on the UI owner.
use crate::*;
const CONFIG: &[u8] = b"/data/chaos/dimmer-timeout.cfg\0";
const TEMP: &[u8] = b"/data/chaos/dimmer-timeout.cfg.tmp\0";
const MAX_SECONDS: u32 = i32::MAX as u32 / 1000;
static mut INITIAL: [u8; 16] = [0; 16];
static mut LOADED: bool = false;
static mut SECONDS: Option<u32> = None;
static mut SAVE_FAILED: bool = false;

fn encode(seconds: u32) -> [u8; 9] {
    let mut bytes = [0; 9]; bytes[..4].copy_from_slice(b"DIM1");
    bytes[4..8].copy_from_slice(&seconds.to_le_bytes());
    bytes[8] = bytes[..8].iter().fold(0x5a, |a, b| a ^ b); bytes
}
fn decode(bytes: [u8; 9]) -> Option<u32> {
    let seconds = u32::from_le_bytes([bytes[4],bytes[5],bytes[6],bytes[7]]);
    if &bytes[..4] != b"DIM1" || bytes[8] != bytes[..8].iter().fold(0x5a, |a,b| a ^ b)
        || seconds == 0 || seconds > MAX_SECONDS { None } else { Some(seconds) }
}
pub(crate) unsafe fn ensure_applied() {
    if LOADED { return; }
    LOADED = true;
    SECONDS = crate::settings_store::read::<9>(CONFIG).and_then(decode);
    if let Some(seconds) = SECONDS { fw_api::screen_timeout_set(seconds * 1000); }
}
pub(crate) unsafe fn open() {
    ensure_applied();
    let p = core::ptr::addr_of_mut!(INITIAL) as *mut u8;
    let mut w = W::new(p, 16);
    // A temporary keep-awake lease must never become the module preference.
    w.n(SECONDS.unwrap_or_else(|| fw_api::screen_timeout_ms() / 1000));
    w.end(); keyboard::open(p, keyboard::Completion::DimmerTimeout);
}
pub(crate) unsafe fn error_text() -> *const u8 {
    if SAVE_FAILED { "保存失败，请重试\0".as_ptr() }
    else { "请输入1～2147483秒\0".as_ptr() }
}
pub(crate) unsafe fn save(text: *const u8) -> bool {
    SAVE_FAILED = false;
    let mut seconds = 0u32; let mut n = 0;
    loop {
        let c = core::ptr::read_volatile(text.add(n));
        if c == 0 { break; }
        if !c.is_ascii_digit() || n >= 7 { return false; }
        seconds = seconds * 10 + (c - b'0') as u32; n += 1;
    }
    if seconds == 0 || seconds > MAX_SECONDS { return false; }
    if !crate::settings_store::save(CONFIG, TEMP, &encode(seconds)) {
        SAVE_FAILED = true; return false;
    }
    LOADED = true; SECONDS = Some(seconds);
    fw_api::screen_timeout_set(seconds * 1000); true
}
#[cfg(test)]
pub(crate) unsafe fn reset_for_test() { LOADED = false; SECONDS = None; SAVE_FAILED = false; }
