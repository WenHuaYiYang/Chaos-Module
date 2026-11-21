//! 实验性「极致省电」开关。
//!
//! 默认关闭。打开后把维护节拍、Corona 上游轮询、通知图标扫描三处进一步拉长
//! 间隔，换取更低的静置 CPU 与耗电；代价是模糊自愈、通知变化、页面后台刷新
//! 的响应从"亚秒级"变成"秒级"。有活时（装配/滚动/采集未完成/失败恢复）仍然
//! 走快节拍，功能正确性不受影响。

use crate::fw_api;

const CONFIG: &[u8] = b"/data/chaos/power_saver.cfg\0";

static mut LOADED: bool = false;
static mut ENABLED: bool = false;
static mut SAVE_FAILED: bool = false;

extern "C" {
    /// C 侧 poll 降频档: 空闲时每 n 次 poll 才完整跑一次上游观察者校验。
    fn chaos_corona_set_poll_skip(n: u32);
}

unsafe fn load() {
    if LOADED {
        return;
    }
    LOADED = true;
    let fd = fw_api::open(CONFIG.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 {
        return;
    }
    let mut value = 0u8;
    if fw_api::read(fd, &mut value as *mut u8, 1) == 1 {
        ENABLED = value != 0;
    }
    fw_api::close(fd);
    chaos_corona_set_poll_skip(poll_skip());
}

pub(crate) unsafe fn enabled() -> bool {
    load();
    ENABLED
}

pub(crate) unsafe fn save_failed() -> bool {
    SAVE_FAILED
}

/// 空闲时上游 poll 的间隔倍数。
/// 2026-11-21 实测: 拉到 4(配合 1s 节拍 = 4s 一次) 会让模糊 owner 心跳超时,
/// 上游反复 detach/commit 重建整屏纹理, 静置 CPU 反而从 20-30% 飙到 95%。
/// 降频只在节拍那一层做, poll 一律保持 2 —— 宁可少省一点, 不能引发抖动。
pub(crate) unsafe fn poll_skip() -> u32 {
    2
}

/// 通知图标扫描的空闲跳拍: 标准 8 拍, 极致省电 12 拍。
pub(crate) unsafe fn quiet_skip() -> u32 {
    if enabled() {
        12
    } else {
        8
    }
}

pub(crate) unsafe fn set_enabled(on: bool) {
    load();
    if ENABLED == on {
        return;
    }
    ENABLED = on;
    let fd = fw_api::open(
        CONFIG.as_ptr(),
        fw_api::oflag::WRONLY | fw_api::oflag::CREAT,
        0o640,
    );
    let value = [on as u8];
    SAVE_FAILED = fd < 0 || fw_api::write(fd, value.as_ptr(), 1) != 1;
    if fd >= 0 {
        fw_api::close(fd);
    }
    chaos_corona_set_poll_skip(poll_skip());
}
