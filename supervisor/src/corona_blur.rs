//! Corona native blur bridge.
//!
//! The complete four-surface hooks and provider live in the AGPL-3.0-only
//! files under supervisor/corona_vendor. This module owns only Chaos
//! configuration and the serialized UI-owner tick that drives the provider.

use crate::fw_api;

const CONFIG: &[u8] = b"/data/chaos/corona_blur.cfg\0";

extern "C" {
    fn chaos_corona_backgrounds_commit(enabled: u32, region_mask: u32) -> i32;
    fn chaos_corona_backgrounds_poll();
    fn chaos_corona_backgrounds_pending() -> u32;
    fn chaos_corona_backgrounds_idle() -> i32;
}

static mut LOADED: bool = false;
static mut ENABLED: bool = false;

unsafe fn load() {
    if LOADED { return; }
    LOADED = true;
    let fd = fw_api::open(CONFIG.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 { return; }
    let mut value = 0u8;
    if fw_api::read(fd, &mut value as *mut u8, 1) == 1 && value <= 1 {
        ENABLED = value != 0;
    }
    fw_api::close(fd);
}

unsafe fn persist() {
    let fd = fw_api::open(CONFIG.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd >= 0 {
        let value = [if ENABLED { 1u8 } else { 0u8 }];
        let _ = fw_api::write(fd, value.as_ptr(), value.len() as u32);
        fw_api::close(fd);
    }
}

pub(crate) unsafe fn enabled() -> bool {
    load();
    ENABLED
}

pub(crate) unsafe fn set_enabled(on: bool) {
    load();
    if ENABLED == on { return; }
    ENABLED = on;
    persist();
    crate::background_home::remount();
}

pub(crate) unsafe fn set(on: bool) {
    set_enabled(on);
}

/// Commit Corona's background configuration using the shared region mask:
/// launcher, control center, widgets, notifications (bits 0 through 3).
/// Unselected regions retain the system background. The C
/// implementation is the verbatim p67_backgrounds/p67_launcher state machine;
/// a non-zero return means it is still busy with a native scroll/delete and
/// must be retried on a later UI-owner tick.
pub(crate) unsafe fn backgrounds_commit(enabled: bool, region_mask: u32) -> i32 {
    chaos_corona_backgrounds_commit(enabled as u32, region_mask & 0x0F)
}

pub(crate) unsafe fn backgrounds_poll() {
    chaos_corona_backgrounds_poll();
}

pub(crate) unsafe fn backgrounds_pending() -> bool {
    chaos_corona_backgrounds_pending() != 0
}

/// Reuse the upstream native-idle and gesture gates before removing the old
/// Chaos scene. Neither this bridge nor Backdrop takes ownership of Corona's
/// image descriptors; publication/retirement remain entirely in Corona.
pub(crate) unsafe fn backgrounds_idle() -> bool {
    chaos_corona_backgrounds_idle() != 0
}

