//! Optional two-dimensional native launcher. One LVGL owner; callbacks record
//! input, maintenance persists settings, and the frame timer advances motion.
use crate::fw_api::honeycomb as fw;
use crate::honeycomb_layout::{Camera, Frame, ReturnAnimation, Route, MAX_ICONS};
use crate::{fw_api, log};

const CONFIG: &[u8] = b"/data/chaos/honeycomb.cfg\0";
static mut LOADED: bool = false;
static mut ENABLED: bool = false;
static mut DIRTY: bool = false;
static mut SAVE_WARNED: bool = false;
static mut OWNER: u32 = 0;
static mut COUNT: usize = 0;
static mut REVISION: u32 = 0;
static mut CAMERA: Camera = Camera::new();
static mut DRAWING: bool = false;
static mut IN_LAYOUT: bool = false;
static mut RENDER_PENDING: bool = false;
static mut FAULT_PENDING: bool = false;
static mut HOOK_REVISION: u32 = 0;
static mut EDIT_GRID: u32 = 0;
static mut RETURN: ReturnAnimation = ReturnAnimation::new();
static mut REAPPLY_PENDING: bool = false;

unsafe fn view_frame(index: usize) -> Frame {
    if crate::s541_launcher::enabled() {
        crate::s541_launcher::frame(index, CAMERA.pan())
    } else { RETURN.frame(index, CAMERA.frame(index)) }
}

unsafe fn active() -> bool { ENABLED || crate::s541_launcher::enabled() }

pub(crate) unsafe fn s541_changed(at_end: bool) {
    CAMERA = Camera::new(); RETURN.cancel();
    if crate::s541_launcher::enabled() {
        let range = crate::s541_launcher::range(COUNT);
        CAMERA.linear_bounds(COUNT, range);
        if at_end { CAMERA.linear_end(COUNT, range); }
    }
    RENDER_PENDING = true;
    if OWNER != 0 && fw::launcher() == OWNER && fw::visible(OWNER) { fw::frame_timer_active(true); }
}

pub(crate) unsafe fn s541_ui_tick() {
    if DRAWING || IN_LAYOUT || OWNER == 0 || fw::launcher() != OWNER || !fw::visible(OWNER) { return; }
    let mut icons = [0u32; MAX_ICONS];
    let n = fw::icons(OWNER, &mut icons);
    if n != 0 { crate::s541_launcher::render(OWNER, &icons[..n], CAMERA.pan(), moving()); }
}

unsafe fn moving() -> bool {
    CAMERA.moving() || RETURN.active()
}

unsafe fn finish_return() {
    if RETURN.active() {
        RETURN.cancel();
        RENDER_PENDING = true;
    }
}

unsafe fn forget_owner() {
    OWNER = 0;
    COUNT = 0;
    CAMERA = Camera::new();
    RETURN.cancel();
    REVISION = 0;
    HOOK_REVISION = 0;
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
    let mut b = [0u8; 5];
    let n = fw_api::read(fd, b.as_mut_ptr(), b.len() as u32);
    fw_api::close(fd);
    if n == 5 && &b[..4] == b"HCB1" && b[4] <= 1 {
        ENABLED = b[4] != 0;
        if ENABLED {
            crate::desktop_scale::disable_for_honeycomb();
            log::line("蜂窝视图: 已恢复开启状态");
        }
    }
}

/// 蜂窝视图是否处在需要快拍的活跃态(编辑中/重排挂起/镜头或回位动画在动)。
/// 维护定时器的节拍自适应用: 不活跃时亮屏也可以降频到中档。
pub(crate) unsafe fn busy() -> bool {
    EDIT_GRID != 0 || DRAWING || REAPPLY_PENDING || RENDER_PENDING || crate::s541_launcher::pending() || (active() && moving())
}

pub(crate) unsafe fn enabled() -> bool {
    load();
    ENABLED
}

pub(crate) unsafe fn toggle() {
    set_enabled(!enabled());
}

pub(crate) unsafe fn set_enabled(on: bool) {
    load();
    if on {
        crate::s541_launcher::disable();
        crate::desktop_scale::disable_for_honeycomb();
    }
    if ENABLED != on {
        ENABLED = on;
        DIRTY = true;
    }
}

unsafe fn save() {
    if !DIRTY {
        return;
    }
    let b = [b'H', b'C', b'B', b'1', ENABLED as u8];
    let fd = fw_api::open(
        CONFIG.as_ptr(),
        fw_api::oflag::WRONLY | fw_api::oflag::CREAT,
        0o640,
    );
    let ok = if fd >= 0 {
        let n = fw_api::write(fd, b.as_ptr(), b.len() as u32);
        fw_api::close(fd);
        n == b.len() as i32
    } else {
        false
    };
    if ok {
        DIRTY = false;
        SAVE_WARNED = false;
    } else if !SAVE_WARNED {
        log::line("蜂窝视图: 设置保存失败，将重试");
        SAVE_WARNED = true;
    }
}

/// Always enumerate CURRENT native children; no icon handle is cached between
/// event/timer turns, so uninstall/reorder/rebuild cannot reuse stale handles.
unsafe fn draw() {
    if DRAWING || IN_LAYOUT {
        return;
    }
    let owner = OWNER;
    if owner == 0 || fw::launcher() != owner {
        return;
    }
    let mut icons = [0u32; MAX_ICONS];
    let n = fw::icons(owner, &mut icons);
    if n == 0 {
        FAULT_PENDING = true;
        return;
    }
    if COUNT != n {
        RETURN.cancel();
        CAMERA = Camera::new();
        CAMERA.cancel();
        COUNT = n;
    }
    DRAWING = true;
    if crate::s541_launcher::enabled() { CAMERA.linear_bounds(COUNT, crate::s541_launcher::range(COUNT)); }
    let mut hooks = 2166136261u32;
    for &icon in &icons[..n] {
        hooks = hooks.wrapping_mul(16777619) ^ icon;
    }
    if hooks != HOOK_REVISION {
        if HOOK_REVISION != 0 {
            RETURN.cancel();
        }
        if !fw::hook_icons(&icons[..n]) {
            DRAWING = false;
            FAULT_PENDING = true;
            return;
        }
        HOOK_REVISION = hooks;
    }
    fw::begin_frame(owner, &icons[..n]);
    for (i, &icon) in icons[..n].iter().enumerate() {
        if !fw::place(icon, view_frame(i)) {
            DRAWING = false;
            FAULT_PENDING = true;
            return;
        }
    }
    fw::present(owner);
    if crate::s541_launcher::enabled() {
        crate::s541_launcher::render(owner, &icons[..n], CAMERA.pan(), CAMERA.moving());
    }
    for (i, &icon) in icons[..n].iter().enumerate() {
        if !fw::geometry_matches(icon, view_frame(i)) {
            log::kv("hc geometry icon", i as u32);
            DRAWING = false;
            FAULT_PENDING = true;
            return;
        }
    }
    REVISION = fw::revision(owner);
    RENDER_PENDING = false;
    DRAWING = false;
}

/// Layout callbacks must settle: no visibility/zoom/style changes, hooks,
/// allocation, animation deletion, invalidation, I/O or detach here. Clearing
/// HIDDEN on an already visible icon dirties its parent again in this firmware.
pub(crate) unsafe extern "C" fn layout_event(object: u32, _data: u32) {
    if object != OWNER || !active() || DRAWING || IN_LAYOUT || FAULT_PENDING {
        return;
    }
    IN_LAYOUT = true;
    let mut icons = [0u32; MAX_ICONS];
    let n = fw::icons(object, &mut icons);
    if n == 0 {
        FAULT_PENDING = true;
    } else {
        if n != COUNT {
            RENDER_PENDING = true;
        }
        for (i, &icon) in icons[..n].iter().enumerate() {
            if !fw::layout_rect(icon, view_frame(i)) {
                FAULT_PENDING = true;
                break;
            }
        }
        if !FAULT_PENDING {
            fw::commit_rects(object);
        }
    }
    IN_LAYOUT = false;
}

unsafe fn consume_fault() {
    if !FAULT_PENDING {
        return;
    }
    FAULT_PENDING = false;
    log::line("蜂窝视图: 布局检查失败，已关闭并恢复系统布局");
    crate::s541_launcher::disable();
    set_enabled(false);
    restore();
}

unsafe fn restore() {
    RETURN.cancel();
    if OWNER == 0 {
        return;
    }
    let mut icons = [0u32; MAX_ICONS];
    let n = fw::icons(OWNER, &mut icons);
    let owner = OWNER;
    OWNER = 0;
    COUNT = 0;
    CAMERA = Camera::new();
    REVISION = 0;
    HOOK_REVISION = 0;
    fw::detach(owner, &icons[..n]);
    log::line("蜂窝视图: 已恢复系统布局");
}

/// Explicit switch application on the UI owner: release old per-object hooks,
/// apply native parameters, then bind the selected layout before returning.
pub(crate) unsafe fn reapply_now() {
    if DRAWING || IN_LAYOUT {
        REAPPLY_PENDING = true;
        return;
    }
    load();
    REAPPLY_PENDING = false;
    let grid = fw::launcher();
    if OWNER != 0 && OWNER != grid {
        forget_owner();
    }
    restore();
    fw::stop_watch();
    FAULT_PENDING = false; // A fresh binding validates geometry again.
    RENDER_PENDING = false;
    if !active() {
        save(); // Persist release before enabling RW in the other config file.
    }
    crate::desktop_scale::reapply_now();
    tick();
    if !active() {
        save();
    }
}

/// Can run from an entry notification before the maintenance queue. No file
/// I/O here: native construction may make the grid ready during the gesture.
unsafe fn prepare(grid: u32) -> bool {
    if grid == 0 || DRAWING || IN_LAYOUT || FAULT_PENDING || !fw::supported(grid) {
        return false;
    }
    if OWNER != 0 && OWNER != grid {
        // The native destructor normally clears this first. Never dereference
        // an old global/owner if the parent membership no longer matches.
        forget_owner();
    }
    if OWNER == grid {
        return true;
    }
    let mut icons = [0u32; MAX_ICONS];
    let n = fw::icons(grid, &mut icons);
    if n == 0 {
        return false;
    }
    OWNER = grid;
    COUNT = n;
    CAMERA = Camera::new();
    RETURN.cancel();
    let returning = EDIT_GRID == grid && fw::visible(grid);
    if returning {
        for (i, &icon) in icons[..n].iter().enumerate() {
            RETURN.set_start(i, fw::current_frame(icon).unwrap_or(CAMERA.frame(i)));
        }
        fw::stop_return_animations(grid, &icons[..n]);
    }
    if !fw::attach(grid, &icons[..n]) {
        OWNER = 0;
        COUNT = 0;
        CAMERA = Camera::new();
        return false;
    }
    if returning {
        CAMERA.cancel(); // A press caught during reflow cannot launch later.
        RETURN.begin(fw_api::capture_now_ms(), n);
    }
    EDIT_GRID = 0;
    RENDER_PENDING = true;
    fw::frame_timer_start();
    fw::frame_timer_active(fw::visible(grid) && moving());
    log::line("蜂窝视图: 双向拖动已启用");
    true
}

pub(crate) unsafe fn tick() {
    load();
    if REAPPLY_PENDING && !DRAWING && !IN_LAYOUT {
        reapply_now();
        return;
    }
    consume_fault();
    save();
    let grid = fw::launcher();
    if EDIT_GRID != 0 && (!active() || EDIT_GRID != grid) {
        EDIT_GRID = 0;
    }
    if OWNER != 0 && OWNER != grid {
        forget_owner();
    }
    if !active() {
        fw::frame_timer_active(false);
        restore();
        fw::stop_watch();
        return;
    }
    let fast = fw::frame_timer_start();
    if grid == 0 {
        fw::stop_watch();
        fw::frame_timer_active(false);
        return;
    }
    if !fw::supported(grid) {
        restore();
        fw::watch_entry(grid);
        fw::frame_timer_active(false);
        return;
    }
    fw::watch_entry(grid);
    if !prepare(grid) {
        fw::watch_entry(grid);
        return;
    }
    let mut changed = false;
    // Attach and prepare while off-screen, before the native entry animation
    // exposes its first frame. The motion observer synchronizes native resets.
    if !fw::visible(grid) {
        CAMERA.cancel();
        finish_return();
    } else if !fast {
        changed |= CAMERA.tick(COUNT);
        if RETURN.active() {
            changed |= RETURN.tick(fw_api::capture_now_ms());
        }
    }
    // Also repairs native entry transitions or an app-list refresh. All writes
    // target this verified launcher's own native images, never other apps.
    changed |= RENDER_PENDING;
    RENDER_PENDING = false;
    if changed || (!moving() && fw::revision(grid) != REVISION) {
        draw();
    }
    fw::frame_timer_active(fw::visible(grid) && moving());
    consume_fault();
}

/// A separate UI animation timer; the general 50 ms maintenance queue keeps
/// its original cadence. An idle/off-screen cloud returns to a 1000 ms timer.
pub(crate) unsafe extern "C" fn animation_tick(_timer: u32) {
    if !active() || OWNER == 0 || fw::launcher() != OWNER || !fw::visible(OWNER) {
        finish_return();
        fw::frame_timer_active(false);
        return;
    }
    if !fw::supported(OWNER) {
        fw::frame_timer_active(false);
        return;
    }
    let mut changed = CAMERA.tick(COUNT);
    if RETURN.active() {
        changed |= RETURN.tick(fw_api::capture_now_ms());
    }
    changed |= RENDER_PENDING;
    RENDER_PENDING = false;
    if changed {
        draw();
    }
    consume_fault();
    fw::frame_timer_active(moving() || RENDER_PENDING);
}

pub(crate) unsafe extern "C" fn entry_event(event: u32) {
    if !active() || DRAWING || IN_LAYOUT || !matches!(fw_api::event_get_code(event), 9 | 11 | 12) {
        return;
    }
    let grid = fw::launcher();
    if grid != 0 && fw::entry_source(grid, event) && prepare(grid) {
        CAMERA.cancel();
        finish_return();
        // This observer runs after the native position callback and before
        // drawing. No timer-sized interval showing the stock geometry.
        if RENDER_PENDING || fw::revision(OWNER) != REVISION {
            draw();
        }
    }
}

/// Called before the native/C-background position callback can capture a
/// stock screenshot. The post observer alone runs too late for that image.
#[no_mangle]
pub unsafe extern "C" fn chaos_honeycomb_pre_scroll(event: u32) -> u32 {
    let code = fw_api::event_get_code(event);
    if !active() || DRAWING || IN_LAYOUT || !matches!(code, 9 | 11 | 12) {
        return 0;
    }
    let grid = fw::launcher();
    if grid == 0 || !fw::entry_source(grid, event) || !prepare(grid) {
        return 0;
    }
    CAMERA.cancel();
    finish_return();
    if code != 9 {
        if RENDER_PENDING || fw::revision(grid) != REVISION {
            draw();
        }
        return 0; // Native end/position bookkeeping must still run.
    }
    if !fw::clear_snapshot(OWNER) {
        RENDER_PENDING = true; // A transient/foreign snapshot is retried later.
        return 0;
    }
    draw();
    if FAULT_PENDING {
        return 0;
    }
    fw::begin_transition(OWNER, event) as u32
}

pub(crate) unsafe fn destroyed(object: u32) {
    if EDIT_GRID == object {
        EDIT_GRID = 0;
    }
    if OWNER == object {
        OWNER = 0;
        COUNT = 0;
        CAMERA = Camera::new();
        RETURN.cancel();
        HOOK_REVISION = 0;
    }
}

pub(crate) unsafe extern "C" fn class_event(_class: u32, event: u32) {
    if OWNER == 0 || fw::current_target(event) != OWNER {
        fw::native_event(event);
        return;
    }
    let code = fw_api::event_get_code(event);
    if RETURN.active() {
        fw::base_event(event);
        return;
    }
    if code == 5 && CAMERA.can_click() && fw::pressed_icon(event, OWNER) {
        // Atomically release our layout/callbacks before entering the native
        // editor. No intermediate stock refresh and no second 50 ms handoff.
        let owner = OWNER;
        let mut icons = [0u32; MAX_ICONS];
        let n = fw::icons(owner, &mut icons);
        if n == 0 {
            FAULT_PENDING = true;
            return;
        }
        OWNER = 0;
        EDIT_GRID = owner;
        COUNT = 0;
        REVISION = 0;
        HOOK_REVISION = 0;
        CAMERA = Camera::new();
        RETURN.cancel();
        RENDER_PENDING = false;
        fw::frame_timer_active(false);
        fw::detach_for_edit(owner, &icons[..n]);
        fw::native_event(event);
        return;
    }
    if code == 1 {
        fw::native_event(event);
    } else {
        fw::base_event(event);
    }
    if OWNER == 0 {
        return;
    }
    match code {
        1 => {
            if crate::s541_launcher::enabled() { CAMERA.press_linear(fw::point(event)); }
            else { CAMERA.press_at(fw::point(event)); }
            fw::navigation(OWNER, event, 0);
            fw::frame_timer_active(true);
        }
        2 => {
            // PRESSING, one real 32-bit x/y vector
            if CAMERA.drag(fw::vector(event), COUNT) {
                RENDER_PENDING = true;
            }
            let chain = match CAMERA.route() {
                Route::Cloud => 0,
                Route::Horizontal => 0x100,
                Route::Vertical => 0x200,
            };
            fw::navigation(OWNER, event, chain);
            fw::frame_timer_active(true);
        }
        3 | 20 => CAMERA.cancel(), // PRESS_LOST / INDEV_RESET
        8 => {
            CAMERA.release();
            fw::frame_timer_active(moving() || RENDER_PENDING);
        }
        _ => {}
    }
}

pub(crate) unsafe extern "C" fn click_event(event: u32) -> u32 {
    if RETURN.active() {
        return 0;
    }
    let icon = fw::current_target(event);
    if OWNER == 0 || fw::parent(icon) != OWNER || CAMERA.can_click() {
        // The native callback receives its unmodified target and app_id.
        // No synthetic page keys, registry writes or copied icon resources.
        fw::launch(event);
    }
    0
}
