//! V2/V3 官方启动器布局按钮区域独立保存。
use crate::fw_api;
const CONFIG: &[u8] = b"/data/chaos/blur_launcher_button.cfg\0";
static mut LOADED: bool = false;
// 默认关闭(2026-11-21): 开启会让 P67_BLUR_EXTRA_DEMAND 恒真, 模糊引擎
// 永久有需求 —— 静置 CPU/耗电明显变高。需要玻璃效果的人在 模糊区域 里手动开。
static mut ENABLED: bool = false;
static mut SAVE_FAILED: bool = false;

pub(crate) unsafe fn enabled() -> bool {
    if !LOADED {
        LOADED = true;
        let fd = fw_api::open(CONFIG.as_ptr(), fw_api::oflag::RDONLY, 0);
        if fd >= 0 {
            let mut value = 0u8;
            if fw_api::read(fd, &mut value, 1) == 1 && value <= 1 { ENABLED = value != 0; }
            fw_api::close(fd);
        }
    }
    ENABLED
}
pub(crate) unsafe fn set(on: bool) -> bool {
    let changed = enabled() != on;
    if !changed && !SAVE_FAILED { return false; }
    ENABLED = on;
    let value = on as u8;
    let fd = fw_api::open(CONFIG.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    SAVE_FAILED = if fd >= 0 {
        let saved = fw_api::write(fd, &value, 1) == 1;
        fw_api::close(fd);
        !saved
    } else { true };
    changed
}
