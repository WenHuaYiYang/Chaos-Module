//! 系统日志掩码：仅在注入后恢复一次，用户操作时保存，不周期写闪存。
use crate::fw_api;
const CONFIG: &[u8] = b"/data/chaos/log-level.cfg\0";
const TEMP: &[u8] = b"/data/chaos/log-level.tmp\0";
static mut LOADED: bool = false;
static mut SAVE_FAILED: bool = false;

pub(crate) unsafe fn ensure_applied() {
    if LOADED { return; }
    LOADED = true;
    let fd = fw_api::open(CONFIG.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 { return; }
    let mut b = [0u8; 6];
    let n = fw_api::read(fd, b.as_mut_ptr(), 6);
    fw_api::close(fd);
    if n == 6 && &b[..4] == b"LOG1" && b[5] == !b[4] {
        fw_api::system_log_mask_set(b[4]);
    }
}

pub(crate) unsafe fn mask() -> u8 {
    ensure_applied();
    fw_api::system_log_mask()
}

pub(crate) unsafe fn status() -> *const u8 {
    if SAVE_FAILED { "保存失败，仅本次生效\0".as_ptr() }
    else { core::ptr::null() }
}

pub(crate) unsafe fn set(mask: u8) {
    ensure_applied();
    fw_api::system_log_mask_set(mask);
    fw_api::capture_mkdir_path(b"/data/chaos\0".as_ptr());
    let b = [b'L', b'O', b'G', b'1', mask, !mask];
    let fd = fw_api::open(TEMP.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    SAVE_FAILED = true;
    if fd < 0 { return; }
    let n = fw_api::write(fd, b.as_ptr(), b.len() as u32);
    fw_api::close(fd);
    if n == b.len() as i32 && fw_api::capture_rename(TEMP.as_ptr(), CONFIG.as_ptr()) == 0 {
        SAVE_FAILED = false;
    }
}

#[cfg(test)]
#[test]
fn persistence_restores_every_mask_and_preserves_missing_or_invalid_config() {
    unsafe {
        fw_api::FILE = None;
        fw_api::MASK = 0x7f;
        LOADED = false;
        ensure_applied();
        assert_eq!(mask(), 0x7f);
        for value in 0..=255u8 {
            set(value);
            assert!(!SAVE_FAILED);
            fw_api::MASK = !value;
            LOADED = false;
            ensure_applied();
            assert_eq!(mask(), value);
            fw_api::MASK = !value;
            ensure_applied(); // 不在每拍覆盖其他工具的设置。
            assert_eq!(mask(), !value);
        }
        fw_api::FILE = Some(*b"BAD!\x01\xfe");
        fw_api::MASK = 0x55;
        LOADED = false;
        ensure_applied();
        assert_eq!(mask(), 0x55);
        let prior = fw_api::FILE;
        fw_api::FAIL = true;
        set(1);
        assert!(SAVE_FAILED);
        assert_eq!(mask(), 1);
        assert_eq!(fw_api::FILE, prior);
    }
}
