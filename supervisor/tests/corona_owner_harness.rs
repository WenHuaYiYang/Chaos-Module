//! Host tests for actual function bodies extracted from background_home.rs.
//! No firmware calls: the C ABI and old scene are injected seams below.
#![allow(dead_code, static_mut_refs)]

use std::sync::Mutex;

macro_rules! st_rd { ($value:expr) => { $value }; }
macro_rules! st_wr { ($slot:expr, $value:expr) => { $slot = $value }; }
static mut CORONA_BG_ACTIVE: u32 = 0;
static mut CORONA_BG_MASK: u32 = 0;
static mut CORONA_VITALITY_SEEN: bool = false;
static mut CORONA_VITALITY_RECOVER: bool = false;
static mut CORONA_FAIL: u32 = 0;
static mut CORONA_HOLD: u32 = 0;
static mut CORONA_HOLD_ARM: u32 = 0;
static mut CORONA_COOLDOWN: u32 = 0;
static mut CORONA_OWN_LOST: u32 = 0;
// 与 background_home.rs 里的常量保持一致（这里只做主机侧模型）。
const CORONA_FAIL_LIMIT: u32 = 100;
const CORONA_HOLD_TICKS: u32 = 400;
const CORONA_OWN_LOST_LIMIT: u32 = 3;
const CORONA_RETRY_TICKS: u32 = 40;
static mut ATTACHED: u32 = 0;
static mut SOURCE_MODE: u32 = 0;
static mut REFRESH: u32 = 0;
static mut REQUEST: u32 = 0;
static mut GENERATE: u32 = 0;
static mut MODE: u32 = 0;
static mut DIRTY: u32 = 0;
static mut BOOT_DELAY: u32 = 0;
static mut DELETED: [(u32, u32); 4] = [(0, 0); 4];
static mut SCENE: Scene = Scene;

struct State {
    enabled: bool, screen: bool, idle: bool, targets: bool, pages: bool,
    scene_unmounted: bool, unmount_ok: bool, release_ok: bool, cleanup_pending: bool,
    commit_rc: i32, region_mask: u32, commits: Vec<(bool, u32)>,
    calls: Vec<&'static str>, status: u32, top: u32, owned: bool,
    saved_mask: Option<u8>, opens: u32, reads: u32,
    v2_enabled: bool, icon_hooked: bool, icon_restore_ok: bool,
}
impl State {
    fn new() -> Self {
        Self { enabled: false, screen: true, idle: true, targets: true, pages: false,
               scene_unmounted: true, unmount_ok: true, release_ok: true, cleanup_pending: false,
               commit_rc: 0, region_mask: 0x0f, commits: Vec::new(), calls: Vec::new(), status: 99,
               top: 200, owned: true,
               saved_mask: None, opens: 0, reads: 0,
               v2_enabled: false, icon_hooked: false, icon_restore_ok: true }
    }
}
static STATE: Mutex<Option<State>> = Mutex::new(None);
fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    f(STATE.lock().unwrap().as_mut().unwrap())
}
fn reset() {
    *STATE.lock().unwrap() = Some(State::new());
    unsafe {
        CORONA_BG_ACTIVE = 0; CORONA_BG_MASK = 0; ATTACHED = 0; SOURCE_MODE = 0; REFRESH = 0;
        REQUEST = 0; GENERATE = 0; MODE = 0; DIRTY = 0; BOOT_DELAY = 0;
        CORONA_FAIL = 0; CORONA_HOLD = 0; CORONA_HOLD_ARM = 0;
        CORONA_COOLDOWN = 0; CORONA_OWN_LOST = 0;
        CORONA_VITALITY_SEEN = false; CORONA_VITALITY_RECOVER = false;
        DELETED = [(0, 0); 4];
        region_storage::reset();
    }
}

#[test]
fn vitality_return_fully_retires_before_recreating_blur_twice() {
    reset();
    with(|s| { s.enabled = true; s.top = fw_api::HOME_APP_ID; });
    unsafe { assert!(corona_tick()); }
    for _ in 0..2 {
        with(|s| s.top = 0x36);
        unsafe { assert!(corona_tick()); }
        with(|s| { s.top = fw_api::HOME_APP_ID; s.cleanup_pending = true; });
        unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 2); }
        let count = with(|s| s.commits.len());
        unsafe { assert!(corona_tick()); }
        with(|s| { assert_eq!(s.commits.len(), count); s.cleanup_pending = false; });
        unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 0); }
        unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1); }
        with(|s| assert_eq!(&s.commits[s.commits.len()-2..], &[(false, 0), (true, 15)]));
    }
}

#[test]
fn other_apps_do_not_trigger_vitality_recovery() {
    reset();
    with(|s| { s.enabled = true; s.top = 0x35; });
    unsafe { assert!(corona_tick()); }
    with(|s| s.top = fw_api::HOME_APP_ID);
    unsafe { assert!(corona_tick()); assert!(!CORONA_VITALITY_RECOVER); }
    with(|s| assert_eq!(s.commits, [(true, 15)]));
}
struct Scene;
impl Scene {
    fn unmounted(&self) -> bool { with(|s| s.scene_unmounted) }
    fn release(&mut self) -> bool { with(|s| {
        assert_eq!(s.calls.last(), Some(&"clear"));
        s.calls.push("release"); s.release_ok
    }) }
    fn pending(&self) -> bool { false }
}
unsafe fn unmount() -> bool {
    with(|s| { s.calls.push("unmount"); if s.unmount_ok { s.scene_unmounted = true; } s.unmount_ok })
}
unsafe fn uninstall() { with(|s| s.calls.push("uninstall")); }
unsafe fn set_status(value: u32) { with(|s| s.status = value); }
mod fw_api {
    pub mod oflag { pub const RDONLY: u32 = 1; }
    pub unsafe fn open(path: *const u8, mode: u32, _: u32) -> i32 {
        assert_eq!(std::ffi::CStr::from_ptr(path.cast()).to_bytes(), b"/data/chaos/blur_regions.cfg");
        assert_eq!(mode, oflag::RDONLY);
        crate::with(|s| { s.opens += 1; if s.saved_mask.is_some() { 1 } else { -1 } })
    }
    pub unsafe fn read(fd: i32, value: *mut u8, size: u32) -> i32 {
        assert_eq!(fd, 1); assert_eq!(size, 1);
        *value = crate::with(|s| { s.reads += 1; s.saved_mask.unwrap() }); 1
    }
    pub unsafe fn close(fd: i32) { assert_eq!(fd, 1); }
    pub unsafe fn screen_is_on() -> bool { crate::with(|s| s.screen) }
    pub const HOME_APP_ID: u32 = 200;
    pub unsafe fn top_app_id() -> u32 { crate::with(|s| s.top) }
    pub unsafe fn background_home_targets() -> Option<[u32; 4]> {
        crate::with(|s| s.targets.then_some([10, 20, 30, 40]))
    }
}
mod background_pages {
    pub unsafe fn hooked() -> bool { crate::with(|s| s.pages) }
}
mod blur_zone {
    pub unsafe fn mask() -> u32 { crate::with(|s| s.region_mask) }
    pub unsafe fn on(slot: usize) -> bool { slot < 4 && mask() & (1 << slot) != 0 }
}
mod region_storage {
    use crate::fw_api;
    static mut MASK: u32 = 0x0f;
    static mut LOADED: u32 = 0;
    const CFG: &[u8] = b"/data/chaos/blur_regions.cfg\0";
    pub unsafe fn reset() { MASK = 0x0f; LOADED = 0; }
    include!(env!("CHAOS_CORONA_REGION_SOURCE"));
}
mod notification_icon {
    pub unsafe fn pending() -> bool { crate::with(|s| s.icon_hooked) }
    pub unsafe fn tick(wanted: bool) -> bool {
        crate::with(|s| {
            assert!(s.screen && s.idle, "icon writes require screen-on/native-idle");
            if wanted {
                s.icon_hooked = true;
                s.calls.push("icon_tick");
                true
            } else {
                s.calls.push("icon_restore");
                if s.icon_restore_ok { s.icon_hooked = false; }
                s.icon_restore_ok
            }
        })
    }
}
mod background_surfaces {
    pub unsafe fn pending() -> bool { false }
    pub unsafe fn clear_background() { crate::with(|s| {
        assert_eq!(s.calls.last(), Some(&"unmount"));
        assert!(s.scene_unmounted);
        s.calls.push("clear");
    }); }
}
mod corona_blur {
    pub unsafe fn enabled() -> bool { crate::with(|s| s.enabled) }
    pub unsafe fn v2_enabled() -> bool { crate::with(|s| s.v2_enabled) }
    pub unsafe fn active() -> bool { enabled() || v2_enabled() }
    pub unsafe fn backgrounds_idle() -> bool { crate::with(|s| s.idle) }
    pub unsafe fn backgrounds_owned() -> bool { crate::with(|s| s.owned) }
    pub unsafe fn backgrounds_pending() -> bool { crate::with(|s| s.cleanup_pending) }
    pub unsafe fn backgrounds_poll() { crate::with(|s| s.calls.push("poll")); }
    pub unsafe fn backgrounds_commit(enabled: bool, region_mask: u32) -> i32 {
        crate::with(|s| {
            s.commits.push((enabled, region_mask));
            s.calls.push(if enabled { "on" } else { "off" });
            s.commit_rc
        })
    }
}

// Generated by tools/test_corona_blur.py from production function bodies.
include!(env!("CHAOS_CORONA_OWNER_SOURCE"));

#[test]
fn disabled_default_does_not_call_corona() {
    reset();
    unsafe { assert!(!corona_tick()); assert!(!pending()); }
    with(|s| assert!(s.calls.is_empty()));
}

#[test]
fn enabled_from_system_mode_unmounts_then_commits_all_hooks() {
    reset(); with(|s| s.enabled = true);
    unsafe {
        assert!(pending()); assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1);
        assert_eq!(CORONA_BG_MASK, 0x0f);
        assert_eq!(MODE, 0); assert_eq!(ATTACHED, 0); assert_eq!(SOURCE_MODE, 0);
    }
    with(|s| assert_eq!(s.calls, ["unmount", "clear", "release", "uninstall", "on", "poll"]));
}

#[test]
fn active_mask_changes_commit_only_after_native_acceptance() {
    reset(); with(|s| s.enabled = true);
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_MASK, 0x0f); }
    with(|s| { s.region_mask = 0b0101; s.calls.clear(); s.commit_rc = -2145; });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_MASK, 0x0f); }
    with(|s| { assert_eq!(s.calls, ["on", "poll"]); s.calls.clear(); s.commit_rc = 0; });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_MASK, 0b0101); }
    with(|s| { assert_eq!(s.calls, ["on", "poll"]); s.calls.clear(); });
    unsafe { assert!(corona_tick()); }
    with(|s| assert_eq!(s.calls, ["poll"]));
}

#[test]
fn all_regions_disabled_keeps_corona_owner_without_legacy_fallback() {
    reset(); with(|s| { s.enabled = true; s.region_mask = 0; });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1); assert_eq!(CORONA_BG_MASK, 0); }
    with(|s| assert_eq!(s.calls, ["unmount", "clear", "release", "uninstall", "on", "poll"]));
}

#[test]
fn busy_enable_is_retried_without_losing_desired_state() {
    reset(); with(|s| { s.enabled = true; s.commit_rc = -2145; });
    unsafe {
        assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 0); assert!(pending());
    }
    with(|s| { assert!(!s.calls.contains(&"poll")); s.commit_rc = 0; });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1); }
}

#[test]
fn existing_scene_is_not_torn_down_during_swipe_or_without_roots() {
    reset(); with(|s| { s.enabled = true; s.idle = false; });
    unsafe { ATTACHED = 1; assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 0); }
    // 忙的时候只服务 Corona（滚动标记的自愈就挂在 poll 上），绝不拆旧场景。
    with(|s| {
        assert_eq!(s.calls, ["poll"]);
        for call in ["unmount", "clear", "release", "uninstall"] {
            assert!(!s.calls.contains(&call), "native busy must not mutate the old scene");
        }
        s.calls.clear(); s.idle = true; s.targets = false;
    });
    unsafe { assert!(corona_tick()); }
    with(|s| assert!(s.calls.is_empty()));
}

#[test]
fn lost_native_ownership_recommits_instead_of_staying_stuck() {
    reset(); with(|s| s.enabled = true);
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1); }
    with(|s| { s.owned = false; s.calls.clear(); });
    // 连续两拍只计数；第三拍才认定原生 owner 真的被摘掉了。
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1); }
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1); }
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 0); }
    with(|s| { s.owned = true; s.calls.clear(); });
    unsafe {
        assert!(CORONA_COOLDOWN > 0, "a lost owner must be retried after a cooldown");
        let mut ticks = 0;
        while CORONA_BG_ACTIVE != 1 && ticks < 200 { assert!(corona_tick()); ticks += 1; }
        assert_eq!(CORONA_BG_ACTIVE, 1);
        assert_eq!(CORONA_BG_MASK, 0x0f);
    }
    with(|s| assert!(s.commits.contains(&(true, 0x0f))));
}

#[test]
fn persistent_commit_failure_degrades_to_legacy_then_retries() {
    reset(); with(|s| { s.enabled = true; s.commit_rc = -2145; });
    unsafe {
        let mut ticks = 0;
        while CORONA_HOLD == 0 && ticks < 300 { assert!(corona_tick()); ticks += 1; }
        assert!(CORONA_HOLD > 0, "persistent failure must degrade to the legacy engine");
        assert_eq!(CORONA_BG_ACTIVE, 0);
        assert_eq!(ticks, CORONA_FAIL_LIMIT);
    }
    with(|s| s.commit_rc = 0);
    unsafe {
        let mut ticks = 0;
        while CORONA_BG_ACTIVE != 1 && ticks < 1000 { let _ = corona_tick(); ticks += 1; }
        assert_eq!(CORONA_BG_ACTIVE, 1);
        assert_eq!(CORONA_HOLD, 0);
        assert_eq!(CORONA_FAIL, 0);
    }
}

#[test]
fn failed_detach_or_release_cannot_install_corona() {
    reset(); with(|s| { s.enabled = true; s.unmount_ok = false; });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 0); }
    with(|s| { assert_eq!(s.calls, ["unmount"]); s.calls.clear(); s.unmount_ok = true; s.release_ok = false; });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 0); }
    with(|s| assert_eq!(s.calls, ["unmount", "clear", "release"]));
}

#[test]
fn busy_disable_keeps_servicing_corona_until_idle() {
    reset(); unsafe { CORONA_BG_ACTIVE = 1; }
    with(|s| s.commit_rc = -2145);
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1); assert!(pending()); }
    with(|s| assert_eq!(s.calls, ["off", "poll"]));
}

#[test]
fn cleanup_is_polled_until_safe_before_resuming_static_mode() {
    reset(); unsafe { CORONA_BG_ACTIVE = 1; MODE = 1; }
    with(|s| s.cleanup_pending = true);
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 2); assert_eq!(REQUEST, 0); }
    with(|s| { assert_eq!(s.calls, ["off", "poll"]); s.calls.clear(); });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 2); assert!(pending()); }
    with(|s| { assert_eq!(s.calls, ["poll"]); s.cleanup_pending = false; });
    unsafe {
        assert!(!corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 0);
        assert_eq!(REQUEST, 1); assert_eq!(GENERATE, 1); assert_eq!(REFRESH, 0);
    }
    with(|s| assert_eq!(s.status, 0));
}

#[test]
fn cleanup_resume_preserves_system_and_transparent_modes() {
    for mode in [0, 3] {
        reset(); unsafe { CORONA_BG_ACTIVE = 2; MODE = mode; GENERATE = 1; }
        unsafe {
            assert!(!corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 0);
            assert_eq!(REQUEST, 1); assert_eq!(GENERATE, 0); assert_eq!(MODE, mode);
        }
    }
}

#[test]
fn quick_off_on_during_cleanup_retries_without_legacy_mutations() {
    reset(); unsafe { CORONA_BG_ACTIVE = 2; }
    with(|s| { s.enabled = true; s.cleanup_pending = true; s.commit_rc = -2145; });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 2); }
    with(|s| { assert_eq!(s.calls, ["on", "poll"]); s.calls.clear(); s.commit_rc = 0; });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1); }
    with(|s| assert_eq!(s.calls, ["on", "poll"]));
}

#[test]
fn sleeping_does_not_spin_but_pending_survives_next_wake() {
    reset(); with(|s| { s.screen = false; s.enabled = true; });
    unsafe { assert!(!pending()); assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 0); }
    with(|s| { assert!(s.calls.is_empty()); s.screen = true; });
    unsafe { assert!(pending()); assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1); }
}

#[test]
fn mode_zero_toggle_wakes_owner_and_rerenders_settings() {
    reset(); unsafe { MODE = 0; remount(); assert_eq!(REQUEST, 1); assert_eq!(DIRTY, 1); assert!(pending()); }
}

#[test]
fn every_saved_region_mask_is_loaded_once_before_corona_enable() {
    for saved in 0u8..16 {
        reset(); with(|s| { s.saved_mask = Some(saved); s.enabled = true; });
        unsafe {
            let loaded = region_storage::mask();
            assert_eq!(loaded, saved as u32);
            assert_eq!(region_storage::mask(), loaded);
            with(|s| s.region_mask = loaded);
            assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1);
            assert_eq!(CORONA_BG_MASK, loaded);
        }
        with(|s| { assert_eq!(s.opens, 1); assert_eq!(s.reads, 1); assert_eq!(s.commits, [(true, saved as u32)]); });
    }
}

#[test]
fn missing_or_invalid_region_file_keeps_full_default() {
    for saved in [None, Some(16), Some(255)] {
        reset(); with(|s| s.saved_mask = saved);
        unsafe { assert_eq!(region_storage::mask(), 15); assert_eq!(region_storage::mask(), 15); }
        with(|s| { assert_eq!(s.opens, 1); assert_eq!(s.reads, if saved.is_some() { 1 } else { 0 }); });
    }
}

#[test]
fn unchanged_mask_never_recommits_during_steady_polling() {
    reset(); with(|s| { s.enabled = true; s.region_mask = 6; });
    unsafe { for _ in 0..20 { assert!(corona_tick()); } assert_eq!(CORONA_BG_MASK, 6); }
    with(|s| assert_eq!(s.commits, [(true, 6)]));
}

#[test]
fn rapid_busy_mask_changes_apply_only_the_latest_value() {
    reset(); with(|s| { s.enabled = true; s.region_mask = 15; });
    unsafe { assert!(corona_tick()); }
    with(|s| { s.calls.clear(); s.commit_rc = -2145; });
    for mask in [1, 8, 4, 0] {
        with(|s| s.region_mask = mask);
        unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1); assert_eq!(CORONA_BG_MASK, 15); }
    }
    with(|s| { assert!(!s.calls.contains(&"unmount")); s.region_mask = 10; s.commit_rc = 0; });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_MASK, 10); }
    with(|s| assert_eq!(s.commits, [(true, 15), (true, 1), (true, 8), (true, 4), (true, 0), (true, 10)]));
}

#[test]
fn zero_mask_reconfiguration_retains_corona_even_with_legacy_mode_saved() {
    reset(); with(|s| s.enabled = true);
    unsafe { MODE = 1; assert!(corona_tick()); REQUEST = 0; }
    with(|s| { s.region_mask = 0; s.calls.clear(); });
    unsafe {
        assert!(corona_tick()); assert!(corona_tick());
        assert_eq!(CORONA_BG_ACTIVE, 1); assert_eq!(CORONA_BG_MASK, 0);
        assert_eq!(REQUEST, 0); assert_eq!(MODE, 1);
    }
    with(|s| assert_eq!(s.calls, ["on", "poll", "poll"]));
}

#[test]
fn disable_uses_zero_but_legacy_restores_the_shared_region_selection() {
    for mask in 0..16 {
        reset(); with(|s| { s.enabled = true; s.region_mask = mask; });
        unsafe { MODE = 1; assert!(corona_tick()); }
        with(|s| s.enabled = false);
        unsafe {
            assert!(!corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 0);
            assert_eq!(CORONA_BG_MASK, 0); assert_eq!(REQUEST, 1); assert_eq!(GENERATE, 1);
            let expected = [if mask & 1 != 0 { 10 } else { 0 }, if mask & 2 != 0 { 20 } else { 0 },
                            if mask & 4 != 0 { 30 } else { 0 }, if mask & 8 != 0 { 40 } else { 0 }];
            assert_eq!(masked_targets([10, 20, 30, 40]), expected);
        }
        with(|s| { assert_eq!(s.commits.last(), Some(&(false, 0))); assert_eq!(s.region_mask, mask); });
    }
}

#[test]
fn reenable_during_cleanup_uses_latest_mask_and_retries_without_legacy() {
    reset(); with(|s| { s.enabled = true; s.region_mask = 3; });
    unsafe { assert!(corona_tick()); }
    with(|s| { s.enabled = false; s.cleanup_pending = true; });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 2); }
    with(|s| { s.enabled = true; s.region_mask = 12; s.commit_rc = -2145; s.calls.clear(); });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 2); assert_eq!(CORONA_BG_MASK, 3); }
    with(|s| { assert_eq!(s.calls, ["on", "poll"]); s.region_mask = 4; s.commit_rc = 0; });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1); assert_eq!(CORONA_BG_MASK, 4); }
    with(|s| assert_eq!(s.commits, [(true, 3), (false, 0), (true, 12), (true, 4)]));
}

#[test]
fn v2_uses_corona_engine_and_installs_notification_icon_hook() {
    reset(); with(|s| s.v2_enabled = true);
    unsafe { assert!(pending()); assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1); }
    with(|s| {
        assert!(!s.enabled); assert!(s.icon_hooked);
        assert_eq!(s.commits, [(true, 15)]);
        assert_eq!(s.calls,
            ["unmount", "clear", "release", "uninstall", "on", "icon_tick", "poll"]);
    });
}

#[test]
fn turning_v2_off_restores_icon_without_disabling_corona() {
    reset(); with(|s| { s.enabled = true; s.v2_enabled = true; });
    unsafe { assert!(corona_tick()); }
    with(|s| { s.calls.clear(); s.v2_enabled = false; });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1); }
    with(|s| {
        assert_eq!(s.calls, ["icon_restore", "poll"]);
        assert_eq!(s.commits, [(true, 15)]); assert!(!s.icon_hooked);
        s.calls.clear();
    });
    unsafe { assert!(corona_tick()); }
    with(|s| assert_eq!(s.calls, ["poll"]));
}

#[test]
fn both_switches_share_one_corona_commit_and_no_second_legacy_teardown() {
    reset(); with(|s| { s.enabled = true; s.v2_enabled = true; });
    unsafe { assert!(corona_tick()); }
    with(|s| { s.calls.clear(); assert_eq!(s.commits, [(true, 15)]); });
    unsafe { for _ in 0..3 { assert!(corona_tick()); } }
    with(|s| {
        assert_eq!(s.commits, [(true, 15)]);
        assert_eq!(s.calls, ["icon_tick", "poll", "icon_tick", "poll", "icon_tick", "poll"]);
    });
}

#[test]
fn toggling_corona_while_v2_remains_enabled_does_not_recommit() {
    reset(); with(|s| s.enabled = true);
    unsafe { assert!(corona_tick()); }
    with(|s| { s.calls.clear(); s.v2_enabled = true; });
    unsafe { assert!(corona_tick()); }
    with(|s| { assert_eq!(s.calls, ["icon_tick", "poll"]); s.calls.clear(); s.enabled = false; });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1); }
    with(|s| {
        assert_eq!(s.calls, ["icon_tick", "poll"]);
        assert_eq!(s.commits, [(true, 15)]); assert!(s.icon_hooked);
    });
}

#[test]
fn v2_uses_only_the_notification_bit_for_icon_ownership() {
    for mask in 0..16 {
        reset(); with(|s| { s.v2_enabled = true; s.region_mask = mask; });
        unsafe {
            MODE = 1; assert!(corona_tick());
            assert_eq!(CORONA_BG_ACTIVE, 1); assert_eq!(CORONA_BG_MASK, mask);
            assert_eq!(REQUEST, 0);
        }
        with(|s| {
            assert_eq!(s.commits, [(true, mask)]);
            assert_eq!(s.icon_hooked, mask & 8 != 0);
            assert_eq!(s.calls.contains(&"icon_tick"), mask & 8 != 0);
            assert!(!s.calls.contains(&"icon_restore"));
        });
    }
}

#[test]
fn notification_region_off_restores_icon_and_on_reinstalls_after_commit() {
    reset(); with(|s| s.v2_enabled = true);
    unsafe { assert!(corona_tick()); }
    with(|s| { s.calls.clear(); s.region_mask = 7; });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1); assert_eq!(CORONA_BG_MASK, 7); }
    with(|s| {
        assert_eq!(s.calls, ["on", "icon_restore", "poll"]); assert!(!s.icon_hooked);
        s.calls.clear();
    });
    unsafe { assert!(corona_tick()); }
    with(|s| { assert_eq!(s.calls, ["poll"]); s.calls.clear(); s.region_mask = 15; });
    unsafe { assert!(corona_tick()); }
    with(|s| {
        assert_eq!(s.calls, ["on", "icon_tick", "poll"]); assert!(s.icon_hooked);
        assert_eq!(s.commits, [(true, 15), (true, 7), (true, 15)]);
    });
}

#[test]
fn busy_region_changes_preserve_applied_mask_and_icon_until_acceptance() {
    for (applied, desired) in [(15, 7), (7, 15)] {
        reset(); with(|s| { s.v2_enabled = true; s.region_mask = applied; });
        unsafe { assert!(corona_tick()); }
        with(|s| { s.calls.clear(); s.region_mask = desired; s.commit_rc = -2145; });
        unsafe {
            assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1);
            assert_eq!(CORONA_BG_MASK, applied); assert_eq!(REQUEST, 0);
        }
        with(|s| {
            assert_eq!(s.calls, ["on", "poll"]); assert_eq!(s.icon_hooked, applied & 8 != 0);
            s.calls.clear(); s.commit_rc = 0;
        });
        unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_MASK, desired); }
        with(|s| {
            assert_eq!(s.calls, ["on", if desired & 8 != 0 { "icon_tick" } else { "icon_restore" }, "poll"]);
            assert_eq!(s.icon_hooked, desired & 8 != 0);
        });
    }
}

#[test]
fn v2_busy_initial_commit_does_not_install_icon_before_engine() {
    reset(); with(|s| { s.v2_enabled = true; s.commit_rc = -2145; });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 0); assert!(pending()); }
    with(|s| {
        assert_eq!(s.calls, ["unmount", "clear", "release", "uninstall", "on"]);
        assert!(!s.icon_hooked); s.calls.clear(); s.commit_rc = 0;
    });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1); }
    with(|s| {
        assert_eq!(s.calls, ["unmount", "clear", "release", "uninstall", "on", "icon_tick", "poll"]);
        assert!(s.icon_hooked);
    });
}

#[test]
fn sleeping_or_native_busy_prevents_icon_enable_but_keeps_engine_polling() {
    for (screen, idle) in [(false, true), (true, false), (false, false)] {
        reset(); with(|s| s.enabled = true);
        unsafe { assert!(corona_tick()); }
        with(|s| { s.calls.clear(); s.v2_enabled = true; s.screen = screen; s.idle = idle; });
        unsafe { assert!(corona_tick()); assert_eq!(pending(), screen); }
        with(|s| {
            assert_eq!(s.calls, ["poll"]); assert!(!s.icon_hooked);
            assert_eq!(s.commits, [(true, 15)]);
            s.calls.clear(); s.screen = true; s.idle = true;
        });
        unsafe { assert!(corona_tick()); }
        with(|s| { assert_eq!(s.calls, ["icon_tick", "poll"]); assert!(s.icon_hooked); });
    }
}

#[test]
fn sleeping_or_native_busy_defers_v2_restore_and_never_hands_off_legacy() {
    for keep_corona in [false, true] {
        for (screen, idle) in [(false, true), (true, false), (false, false)] {
            reset(); with(|s| { s.enabled = true; s.v2_enabled = true; });
            unsafe { MODE = 1; assert!(corona_tick()); }
            with(|s| {
                s.calls.clear(); s.enabled = keep_corona; s.v2_enabled = false;
                s.screen = screen; s.idle = idle;
            });
            unsafe {
                assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1);
                assert_eq!(REQUEST, 0); assert_eq!(GENERATE, 0); assert_eq!(pending(), screen);
            }
            with(|s| {
                assert_eq!(s.calls, ["poll"]); assert!(s.icon_hooked);
                assert_eq!(s.commits, [(true, 15)]);
                s.calls.clear(); s.screen = true; s.idle = true;
            });
            unsafe { assert_eq!(corona_tick(), keep_corona); }
            with(|s| {
                assert!(!s.icon_hooked);
                if keep_corona { assert_eq!(s.calls, ["icon_restore", "poll"]); }
                else { assert_eq!(s.calls, ["icon_restore", "off", "poll"]); }
            });
        }
    }
}

#[test]
fn all_switches_off_restores_icon_before_committing_shutdown() {
    reset(); with(|s| { s.enabled = true; s.v2_enabled = true; });
    unsafe { MODE = 1; assert!(corona_tick()); }
    with(|s| { s.calls.clear(); s.enabled = false; s.v2_enabled = false; s.cleanup_pending = true; });
    unsafe {
        assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 2);
        assert_eq!(REQUEST, 0); assert_eq!(GENERATE, 0);
    }
    with(|s| {
        assert_eq!(s.calls, ["icon_restore", "off", "poll"]);
        assert_eq!(s.commits, [(true, 15), (false, 0)]); assert!(!s.icon_hooked);
        s.calls.clear(); s.cleanup_pending = false;
    });
    unsafe { assert!(!corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 0); assert_eq!(REQUEST, 1); }
    with(|s| assert_eq!(s.calls, ["poll"]));
}

#[test]
fn failed_icon_restore_blocks_shutdown_and_legacy_until_retry_succeeds() {
    reset(); with(|s| s.v2_enabled = true);
    unsafe { MODE = 1; assert!(corona_tick()); }
    with(|s| { s.calls.clear(); s.v2_enabled = false; s.icon_restore_ok = false; });
    for _ in 0..3 {
        unsafe {
            assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1); assert_eq!(CORONA_BG_MASK, 15);
            assert_eq!(REQUEST, 0); assert_eq!(GENERATE, 0); assert!(pending());
        }
        with(|s| {
            assert_eq!(s.calls, ["icon_restore", "poll"]); assert!(s.icon_hooked);
            assert_eq!(s.commits, [(true, 15)]); s.calls.clear();
        });
    }
    with(|s| s.icon_restore_ok = true);
    unsafe { assert!(!corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 0); assert_eq!(REQUEST, 1); assert_eq!(GENERATE, 1); }
    with(|s| { assert_eq!(s.calls, ["icon_restore", "off", "poll"]); assert!(!s.icon_hooked); });
}

#[test]
fn failed_icon_restore_with_corona_still_enabled_retries_without_recommit() {
    reset(); with(|s| { s.enabled = true; s.v2_enabled = true; });
    unsafe { assert!(corona_tick()); }
    with(|s| { s.calls.clear(); s.v2_enabled = false; s.icon_restore_ok = false; });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1); }
    with(|s| {
        assert_eq!(s.calls, ["icon_restore", "poll"]); assert!(s.icon_hooked);
        s.calls.clear(); s.icon_restore_ok = true;
    });
    unsafe { assert!(corona_tick()); }
    with(|s| {
        assert_eq!(s.calls, ["icon_restore", "poll"]); assert!(!s.icon_hooked);
        assert_eq!(s.commits, [(true, 15)]);
    });
}

#[test]
fn restored_icon_is_not_rewritten_when_native_shutdown_stays_busy() {
    reset(); with(|s| s.v2_enabled = true);
    unsafe { assert!(corona_tick()); }
    with(|s| { s.calls.clear(); s.v2_enabled = false; s.commit_rc = -2145; });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1); }
    with(|s| {
        assert_eq!(s.calls, ["icon_restore", "off", "poll"]); assert!(!s.icon_hooked);
        s.calls.clear();
    });
    unsafe { assert!(corona_tick()); assert_eq!(REQUEST, 0); }
    with(|s| { assert_eq!(s.calls, ["off", "poll"]); s.calls.clear(); s.commit_rc = 0; });
    unsafe { assert!(!corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 0); }
    with(|s| assert_eq!(s.calls, ["off", "poll"]));
}

#[test]
fn rapid_v2_reenable_after_failed_icon_restore_keeps_the_existing_engine() {
    reset(); with(|s| s.v2_enabled = true);
    unsafe { assert!(corona_tick()); }
    with(|s| { s.calls.clear(); s.v2_enabled = false; s.icon_restore_ok = false; });
    unsafe { assert!(corona_tick()); }
    with(|s| { assert_eq!(s.calls, ["icon_restore", "poll"]); s.calls.clear(); s.v2_enabled = true; });
    unsafe { assert!(corona_tick()); assert_eq!(CORONA_BG_ACTIVE, 1); assert_eq!(REQUEST, 0); }
    with(|s| {
        assert_eq!(s.calls, ["icon_tick", "poll"]); assert!(s.icon_hooked);
        assert_eq!(s.commits, [(true, 15)]);
    });
}
