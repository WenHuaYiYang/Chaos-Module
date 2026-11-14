//! Audited p67/.043 launcher boundary. All operations run on the LVGL owner.
//! See docs/HONEYCOMB.md; static/host evidence does not imply device acceptance.
use super::{fwaddr as a, *};
use crate::honeycomb_layout::{Frame, Point, HEIGHT, MAX_ICONS};
use crate::mem::{rd32, safe_ptr};
use core::{
    mem::transmute,
    ptr::{addr_of, write_volatile},
};

// lv_obj_class_t: base, constructor, destructor, event, user_data, name,
// width, height, packed instance size/flags. Copy all 36 bytes, deriving from
// the ORIGINAL class so firmware type checks and base destructor traversal work.
static mut CLASS: [u32; 9] = [0; 9];
static mut OWNER: u32 = 0;
static mut FLAGS: u32 = 0;
static mut BUTTON: u32 = 0;
static mut BUTTON_FLAGS: u32 = 0;
static mut LAYOUT_ID: u32 = 0;
static mut ORIGINAL_LAYOUT: u32 = 0;
static mut MOTION_PARENT: u32 = 0;
static mut WATCH_GRID: u32 = 0;
static mut FRAME_TIMER: u32 = 0;
static mut FRAME_PERIOD: u32 = 0;

extern "C" {
    fn rh_platform_launcher_scroll_callback() -> u32;
}

unsafe fn class_address() -> u32 {
    addr_of!(CLASS) as u32
}
unsafe fn word(object: u32, offset: u32) -> u32 {
    rd32((object + offset) as *const u32)
}
unsafe fn write(object: u32, offset: u32, value: u32) {
    write_volatile((object + offset) as *mut u32, value);
}

/// Resolve through the live parent's child table BEFORE dereferencing grid.
/// The subclass destructor invalidates OWNER before the native object is freed.
pub(crate) unsafe fn launcher() -> u32 {
    let root = word(a::LAUNCHER_ROOT_SLOT, 0);
    let grid = word(a::LAUNCHER_GRID_SLOT, 0);
    if !safe_ptr(root) || !safe_ptr(grid) || !obj_is_child_of(root, grid) {
        return 0;
    }
    let class = word(grid, 0);
    if class != a::LAUNCHER_GRID_CLASS && class != class_address() {
        return 0;
    }
    grid
}

pub(crate) unsafe fn supported(grid: u32) -> bool {
    // 0x10000 is the native edit state. Never take ownership while sorting.
    matches!(word(grid, 0x89c), 1 | 2)
        && word(a::LAUNCHER_GRID_CLASS, 4) == a::LAUNCHER_GRID_CTOR
        && word(a::LAUNCHER_GRID_CLASS, 8) == a::LAUNCHER_GRID_DTOR
        && word(a::LAUNCHER_GRID_CLASS, 12) == a::LAUNCHER_GRID_EVENT
}

/// 控制条也支持官方列表模式3；蜂窝的模式限制保持1/2不变。
pub(crate) unsafe fn s541_supported(grid: u32) -> bool {
    matches!(word(grid, 0x89c), 1 | 2 | 3)
        && word(grid, 0) == a::LAUNCHER_GRID_CLASS
        && word(a::LAUNCHER_GRID_CLASS, 4) == a::LAUNCHER_GRID_CTOR
        && word(a::LAUNCHER_GRID_CLASS, 8) == a::LAUNCHER_GRID_DTOR
        && word(a::LAUNCHER_GRID_CLASS, 12) == a::LAUNCHER_GRID_EVENT
}

pub(crate) unsafe fn visible(grid: u32) -> bool {
    // Stock root_pos_change_cb reads the launcher's absolute top at +0x20:
    // 0 is the open launcher, 480 is the watch-face/off-screen position.
    let top = word(parent(grid), 0x20) as i32;
    top > -HEIGHT && top < HEIGHT && super::screen_is_on()
}

/// Detect native entry/rebuild writes without repainting a stationary cloud.
/// Every pointer comes from the current child table; none is cached for reuse.
pub(crate) unsafe fn revision(grid: u32) -> u32 {
    let n = obj_child_count(grid);
    if n < 0 || n as usize > MAX_ICONS + 8 {
        return 0;
    }
    let mut h = word(grid, 0x8ac).wrapping_add(word(parent(grid), 0x20));
    if BUTTON != 0 && obj_is_child_of(parent(grid), BUTTON) {
        h = h.wrapping_mul(16777619) ^ word(BUTTON, 0x2c);
    }
    for i in 0..n as u32 {
        let child = obj_get_child(grid, i);
        if !safe_ptr(child) {
            return 0;
        }
        h = h.wrapping_mul(16777619) ^ child;
        for off in [0x1c, 0x20, 0x24, 0x28, 0x2c, 0x50, 0x54] {
            h = h.wrapping_mul(16777619) ^ word(child, off);
        }
        let ud = word(child, 0x18);
        if safe_ptr(ud) {
            for off in [0, 4, 8, 12] {
                h = h.wrapping_mul(16777619) ^ word(ud, off);
            }
        }
    }
    h
}

unsafe fn descriptor(icon: u32, callback: u32) -> u32 {
    let count: unsafe extern "C" fn(u32) -> u32 = transmute(a::OBJ_EVENT_COUNT as usize);
    let get: unsafe extern "C" fn(u32, u32) -> u32 = transmute(a::OBJ_EVENT_DESCRIPTOR as usize);
    let n = count(icon);
    if n > 32 {
        return 0;
    }
    for i in 0..n {
        let d = get(icon, i);
        if safe_ptr(d) && word(d, 0) == callback {
            return d;
        }
    }
    0
}

pub(crate) unsafe fn motion_parent(grid: u32) -> u32 {
    // In the home launcher the stock observer is on root.parent.parent, with
    // root.parent as user data (create_main: 0x45826C..0x458284). Other native
    // contexts can reparent the tree. Find the existing callback identity.
    let mut object = parent(grid);
    let background_cb = rh_platform_launcher_scroll_callback();
    let mut observer = 0;
    for _ in 0..12 {
        if !safe_ptr(object) {
            break;
        }
        if descriptor(object, a::LAUNCHER_POSITION_EVENT) != 0
            || descriptor(object, entry_dispatch as *const () as u32) != 0
            || (background_cb != 0 && descriptor(object, background_cb) != 0)
        {
            return object;
        }
        if observer == 0
            && descriptor(object, crate::honeycomb::entry_event as *const () as u32) != 0
        {
            observer = object;
        }
        object = parent(object);
    }
    // A surviving observer on an old ancestor must not hide a native callback
    // newly registered farther up the chain during a rebuild.
    observer
}

unsafe fn ancestor_of_grid(grid: u32, ancestor: u32) -> bool {
    let mut object = parent(grid);
    for _ in 0..12 {
        if !safe_ptr(object) {
            return false;
        }
        if object == ancestor {
            return true;
        }
        object = parent(object);
    }
    false
}

/// Watch before the native grid has populated or selected a supported mode.
/// Recheck the live chain so native rebuilds/rebindings cannot remove the hook
/// permanently while our layout remains attached.
pub(crate) unsafe fn watch_entry(grid: u32) -> bool {
    let next = motion_parent(grid);
    if WATCH_GRID != grid || MOTION_PARENT != next {
        stop_watch();
        WATCH_GRID = grid;
        MOTION_PARENT = next;
    }
    if next == 0 {
        return true; // Native construction may register its callback later.
    }
    let original = descriptor(next, a::LAUNCHER_POSITION_EVENT);
    if original != 0 {
        write(original, 0, entry_dispatch as *const () as u32);
    }
    let cb = crate::honeycomb::entry_event as *const () as u32;
    descriptor(next, cb) != 0 || obj_add_event(next, cb, 0, 0) != 0
}

pub(crate) unsafe fn stop_watch() {
    let grid = WATCH_GRID;
    if grid != 0 && launcher() == grid {
        unhook_position(grid);
        if MOTION_PARENT != 0 && ancestor_of_grid(grid, MOTION_PARENT) {
            let remove: unsafe extern "C" fn(u32, u32) -> u32 =
                transmute(a::OBJ_REMOVE_EVENT_CALLBACK as usize);
            remove(
                MOTION_PARENT,
                crate::honeycomb::entry_event as *const () as u32,
            );
        }
    }
    MOTION_PARENT = 0;
    WATCH_GRID = 0;
}

/// Rebind the native layout and recompute current icon geometry after RW table
/// changes, even if no scroll arrives. Never refresh the honeycomb subclass.
pub(crate) unsafe fn refresh_native(grid: u32) -> bool {
    if launcher() != grid || !supported(grid) || word(grid, 0) != a::LAUNCHER_GRID_CLASS {
        return false;
    }
    if core::ptr::read_volatile(a::LAUNCHER_TRANSITION_SLOT as *const u8) != 0
        || word(grid, 0x8b4) != 0
        || !clear_snapshot(grid)
    {
        return false;
    }
    let get: unsafe extern "C" fn(u32, u32, u32) -> u32 =
        transmute(a::OBJ_GET_STYLE_VALUE as usize);
    let set: unsafe extern "C" fn(u32, u32) = transmute(a::OBJ_SET_LAYOUT as usize);
    let layout = get(grid, 0, 0x16);
    set(grid, layout);
    let refresh: unsafe extern "C" fn(u32) = transmute(a::LAUNCHER_GRID_REFRESH as usize);
    refresh(grid);
    commit_rects(grid);
    obj_invalidate(grid);
    true
}

/// No registry pointers or icon source buffers are retained or duplicated.
/// Match the native app callback, excluding the editor ghost and layout button.
pub(crate) unsafe fn icons(grid: u32, out: &mut [u32; MAX_ICONS]) -> usize {
    let n = obj_child_count(grid);
    if n < 0 || n as usize > MAX_ICONS + 8 {
        return 0;
    }
    let mut used = 0;
    for i in 0..n as u32 {
        let icon = obj_get_child(grid, i);
        if safe_ptr(icon) && descriptor(icon, a::LAUNCHER_ICON_CLICK) != 0 {
            if used == MAX_ICONS {
                return 0;
            } // never show a truncated launcher
            out[used] = icon;
            used += 1;
        }
    }
    used
}

/// .043 原生 SHORT_CLICKED 回调的 userdata 是 app_id，源图像在 image+0x34。
/// 应用名按 launcher_data_reload_app_name (0x0C514284) 调 registry+0x1C。
pub(crate) unsafe fn s541_icon(icon: u32) -> Option<(u32, u32, *const u8)> {
    let d = descriptor(icon, a::LAUNCHER_ICON_CLICK);
    if d == 0 { return None; }
    let id = word(d, 4);
    let lookup: unsafe extern "C" fn(u32) -> u32 = transmute(0x0CA69935usize);
    let node = lookup(id);
    if !safe_ptr(node) { return None; }
    let callback = word(node, 0x1c);
    // 用户安装的原生模块（包括Chaos）的回调位于0x3xxxxxxx RAM，不能只认固件flash。
    if callback & 1 == 0 || !crate::mem::plausible_ptr(callback) { return None; }
    // 原生0x0C5142BE取回调后直接BLX，R0仍是lookup返回的应用节点。
    // 某些内置应用（如0x0C549634）会读取node+0x30，不能当无参函数调用。
    let name: unsafe extern "C" fn(u32) -> *const u8 = transmute(callback as usize);
    let source = word(icon, 0x34);
    if source == 0 { return None; }
    Some((id, source, name(node)))
}

pub(crate) unsafe fn s541_button(grid: u32) -> u32 {
    let button = word(grid, 0x5c);
    if safe_ptr(button) && obj_is_child_of(parent(grid), button) { button } else { 0 }
}

/// 官方三布局偏好getter/setter，setter自行写入KV并发出0x10000刷新事件。
pub(crate) unsafe fn s541_preference() -> u32 {
    let get: unsafe extern "C" fn() -> u32 = transmute(0x0C514361usize);
    get()
}
pub(crate) unsafe fn s541_select(preference: u32) -> bool {
    if preference > 2 { return false; }
    let set: unsafe extern "C" fn(u32, u32) -> u32 = transmute(0x0C514301usize);
    set(preference, 1) != 0
}
pub(crate) unsafe fn s541_anchor(grid: u32) -> Option<(u32, i32, i32)> {
    let button = s541_button(grid);
    if button == 0 || word(grid, 0) != a::LAUNCHER_GRID_CLASS || word(grid, 0x89c) & 0x10000 != 0 { return None; }
    let root = parent(grid);
    let x = (word(button, 0x1c) as i32 + word(button, 0x24) as i32 + 1 - 288) / 2 - word(root, 0x1c) as i32;
    let mut y = (word(button, 0x20) as i32 + word(button, 0x28) as i32 + 1 - 96) / 2 - word(root, 0x20) as i32;
    // RW缩放表与列表的按钮占位不等于实际绘制内容的末端。
    // 只收紧自有控制条的空白，不改原生按钮、应用坐标或滚动范围。
    let compact = word(grid, 0x89c) == 3 || crate::desktop_scale::lens_on()
        || crate::desktop_scale::pitch_on() || crate::desktop_scale::inset_on();
    if compact {
        let mut apps = [0; MAX_ICONS];
        let count = icons(grid, &mut apps);
        let mut bottom = None;
        for &icon in &apps[..count] {
            let Some(frame) = current_frame(icon) else { continue; };
            if !frame.visible { continue; }
            let mut edge = word(grid, 0x20) as i32 + frame.y + (frame.diameter + 1) / 2;
            // 原生图标文字是图标的子对象，底部间距必须连名称一起计算。
            let children = obj_child_count(icon);
            if (0..=8).contains(&children) {
                for i in 0..children as u32 {
                    let child = obj_get_child(icon, i);
                    if safe_ptr(child) && word(child, 0x2c) & 1 == 0 {
                        let child_bottom = word(child, 0x28) as i32 + 1;
                        if (child_bottom - edge).abs() < 480 { edge = edge.max(child_bottom); }
                    }
                }
            }
            let local = edge - word(root, 0x20) as i32;
            bottom = Some(bottom.map_or(local, |old: i32| old.max(local)));
        }
        y = crate::honeycomb_layout::selector_anchor(y, bottom);
    }
    Some((button, x, y))
}

pub(crate) unsafe fn attach(grid: u32, icons: &[u32]) -> bool {
    if LAYOUT_ID == 0 {
        let register: unsafe extern "C" fn(u32, u32) -> u32 =
            transmute(a::LAYOUT_REGISTER as usize);
        let id = register(crate::honeycomb::layout_event as *const () as u32, 0);
        if id == 0 || id > 255 {
            return false;
        }
        LAYOUT_ID = id;
    }
    let get_style: unsafe extern "C" fn(u32, u32, u32) -> u32 =
        transmute(a::OBJ_GET_STYLE_VALUE as usize);
    ORIGINAL_LAYOUT = get_style(grid, 0, 0x16); // native LV_STYLE_LAYOUT
    for i in 0..9 {
        write(class_address(), i * 4, word(a::LAUNCHER_GRID_CLASS, i * 4));
    }
    write(class_address(), 0, a::LAUNCHER_GRID_CLASS);
    write(class_address(), 4, 0);
    write(class_address(), 8, destroyed as *const () as u32);
    write(
        class_address(),
        12,
        crate::honeycomb::class_event as *const () as u32,
    );
    OWNER = grid;
    FLAGS = word(grid, 0x2c);
    BUTTON = word(grid, 0x5c);
    // Native create (0x457B10) makes the style button a SIBLING of grid under
    // the launcher root, then stores it at grid+0x5C. Check that live child
    // table before dereferencing; a grid itself can have more than 24 icons.
    if !safe_ptr(BUTTON) || !obj_is_child_of(parent(grid), BUTTON) {
        BUTTON = 0;
    }
    if BUTTON != 0 {
        BUTTON_FLAGS = word(BUTTON, 0x2c);
        if BUTTON_FLAGS & 1 == 0 {
            obj_add_flag(BUTTON, 1);
        }
    }
    let del: unsafe extern "C" fn(u32, u32) -> u32 = transmute(a::ANIMATION_DELETE as usize);
    del(grid, 0); // stop the native one-axis entry/fling writer before taking over
    write(grid, 0, class_address());
    obj_clear_flag(
        grid,
        LV_OBJ_FLAG_SCROLLABLE | LV_OBJ_FLAG_SCROLL_CHAIN | LV_OBJ_FLAG_GESTURE_BUBBLE,
    );
    obj_add_flag(grid, LV_OBJ_FLAG_CLICKABLE);
    if !hook_icons(icons) {
        detach(grid, icons);
        return false;
    }
    // Changing class events alone does NOT change the registered layout stage.
    // This per-object layout callback owns geometry before every LVGL draw.
    let set_layout: unsafe extern "C" fn(u32, u32) = transmute(a::OBJ_SET_LAYOUT as usize);
    set_layout(grid, LAYOUT_ID);
    if !watch_entry(grid) {
        detach(grid, icons);
        return false;
    }
    if !clear_snapshot(grid) {
        detach(grid, icons);
        return false;
    }
    true
}

/// In the native position slot, prepare begin BEFORE capture. Keep native
/// SCROLL_END bookkeeping and its user data/filter unchanged.
pub(crate) unsafe extern "C" fn entry_dispatch(event: u32) {
    if crate::honeycomb::chaos_honeycomb_pre_scroll(event) == 0 {
        let original: unsafe extern "C" fn(u32) = transmute(a::LAUNCHER_POSITION_EVENT as usize);
        original(event);
    }
}

pub(crate) unsafe fn clear_snapshot(grid: u32) -> bool {
    let image = word(a::LAUNCHER_SNAPSHOT_SLOT, 0);
    if image == 0 {
        return true;
    }
    let page = parent(parent(grid));
    if !safe_ptr(image) || !safe_ptr(page) || !obj_is_child_of(page, image) {
        return false;
    }
    // Native image DELETE callback frees its bitmap, clears this slot and
    // reveals the live root. Never free its private pixel buffer ourselves.
    obj_delete(image);
    word(a::LAUNCHER_SNAPSHOT_SLOT, 0) == 0
}

pub(crate) unsafe fn begin_transition(grid: u32, event: u32) -> bool {
    let page = word(event, 12); // same user data as root_pos_change_cb
    if !safe_ptr(page) || page != parent(parent(grid)) {
        return false;
    }
    let y = word(page, 0x20) as i32;
    if y > HEIGHT {
        return false;
    }
    core::ptr::write_volatile(a::LAUNCHER_TRANSITION_SLOT as *mut u8, 1);
    if y == 0 {
        // Preserve the native input-block bit at close begin, without the
        // native geometry reset that precedes its screenshot capture.
        let flags = (grid + 0x8b8) as *mut u8;
        core::ptr::write_volatile(flags, core::ptr::read_volatile(flags) | 4);
    }
    // Keep the native bitmap animation, capturing the prepared honeycomb.
    // Calling the original position handler here would first reset to stock.
    if y == 0 || y == HEIGHT {
        let capture: unsafe extern "C" fn(u32) = transmute(a::LAUNCHER_CREATE_SNAPSHOT as usize);
        capture(page);
    }
    true
}

unsafe fn hide_button() {
    if BUTTON != 0 && obj_is_child_of(parent(OWNER), BUTTON) && word(BUTTON, 0x2c) & 1 == 0 {
        obj_add_flag(BUTTON, 1);
    }
}

pub(crate) unsafe fn hook_icons(icons: &[u32]) -> bool {
    hide_button();
    let cb = crate::honeycomb::click_event as *const () as u32;
    for &icon in icons {
        let d = descriptor(icon, a::LAUNCHER_ICON_CLICK);
        if d == 0 || !matches!(word(d, 8), 4 | 0x7fff) {
            return false;
        }
        if descriptor(icon, cb) == 0 && obj_add_event(icon, cb, 4, word(d, 4)) == 0 {
            return false;
        }
        // Preserve the ORIGINAL callback identity and app_id: firmware's
        // launcher_page_get_icon_appid scans for that callback regardless of
        // filter. Temporarily disarm only its dispatch filter, and route actual
        // SHORT_CLICKED through the extra instance descriptor with the SAME id.
        write(d, 8, 0x7fff);
    }
    true
}

pub(crate) unsafe fn detach(grid: u32, icons: &[u32]) {
    release(grid, icons, true);
}

pub(crate) unsafe fn detach_for_edit(grid: u32, icons: &[u32]) {
    release(grid, icons, false);
}

unsafe fn unhook_position(grid: u32) {
    if MOTION_PARENT != 0 && ancestor_of_grid(grid, MOTION_PARENT) {
        let d = descriptor(MOTION_PARENT, entry_dispatch as *const () as u32);
        if d != 0 {
            write(d, 0, a::LAUNCHER_POSITION_EVENT);
        }
    }
}

unsafe fn release(grid: u32, icons: &[u32], refresh: bool) {
    if OWNER != grid {
        return;
    }
    let remove: unsafe extern "C" fn(u32, u32) -> u32 =
        transmute(a::OBJ_REMOVE_EVENT_CALLBACK as usize);
    stop_watch();
    let restore_icon = |icon: u32| {
        let d = descriptor(icon, a::LAUNCHER_ICON_CLICK);
        if d != 0 && word(d, 8) == 0x7fff {
            write(d, 8, 4);
        }
        remove(icon, crate::honeycomb::click_event as *const () as u32);
    };
    for &icon in icons {
        restore_icon(icon);
    }
    if icons.is_empty() {
        // The list may have grown beyond our display cap since attach. Recover
        // current descriptors too, without dereferencing any old icon handle.
        let n = obj_child_count(grid).clamp(0, 512);
        for i in 0..n as u32 {
            let icon = obj_get_child(grid, i);
            if safe_ptr(icon) {
                restore_icon(icon);
            }
        }
    }
    // Restore only fields still owned by us. No other module's class is replaced.
    let set_layout: unsafe extern "C" fn(u32, u32) = transmute(a::OBJ_SET_LAYOUT as usize);
    set_layout(grid, ORIGINAL_LAYOUT);
    if word(grid, 0) == class_address() {
        write(grid, 0, a::LAUNCHER_GRID_CLASS);
    }
    let mask = LV_OBJ_FLAG_SCROLLABLE
        | LV_OBJ_FLAG_GESTURE_BUBBLE
        | LV_OBJ_FLAG_CLICKABLE
        | if word(grid, 0x89c) < 0x10000 {
            LV_OBJ_FLAG_SCROLL_CHAIN
        } else {
            0
        };
    write(grid, 0x2c, (word(grid, 0x2c) & !mask) | (FLAGS & mask));
    if BUTTON != 0 && obj_is_child_of(parent(grid), BUTTON) {
        write(BUTTON, 0x2c, (word(BUTTON, 0x2c) & !1) | (BUTTON_FLAGS & 1));
    }
    OWNER = 0;
    BUTTON = 0;
    if refresh {
        let refresh: unsafe extern "C" fn(u32) = transmute(a::LAUNCHER_GRID_REFRESH as usize);
        refresh(grid);
    }
}

unsafe extern "C" fn destroyed(_class: u32, object: u32) {
    if OWNER == object {
        unhook_position(object);
        MOTION_PARENT = 0;
        WATCH_GRID = 0;
        OWNER = 0;
        BUTTON = 0;
        crate::honeycomb::destroyed(object);
    }
    // LVGL walks the base chain itself; the native destructor must run ONCE.
}

pub(crate) unsafe fn base_event(event: u32) {
    let f: unsafe extern "C" fn(u32, u32) -> u32 = transmute(a::OBJ_EVENT_BASE as usize);
    // Skip the native one-axis grid handler; preserve its generic base visuals.
    f(a::LAUNCHER_GRID_CLASS, event);
}

pub(crate) unsafe fn current_target(event: u32) -> u32 {
    let f: unsafe extern "C" fn(u32) -> u32 = transmute(a::EVENT_CURRENT_TARGET as usize);
    f(event)
}

pub(crate) unsafe fn parent(object: u32) -> u32 {
    word(object, 4)
}

pub(crate) unsafe fn native_event(event: u32) {
    let f: unsafe extern "C" fn(u32, u32) = transmute(a::LAUNCHER_GRID_EVENT as usize);
    f(a::LAUNCHER_GRID_CLASS, event);
}

pub(crate) unsafe fn vector(event: u32) -> Point {
    let get: unsafe extern "C" fn(u32) -> u32 = transmute(a::EVENT_INPUT_DEVICE as usize);
    let vect: unsafe extern "C" fn(u32, *mut Point) = transmute(a::INPUT_DEVICE_VECTOR as usize);
    let mut p = Point::default();
    vect(get(event), &mut p);
    p
}

pub(crate) unsafe fn point(event: u32) -> Point {
    let get: unsafe extern "C" fn(u32) -> u32 = transmute(a::EVENT_INPUT_DEVICE as usize);
    let point: unsafe extern "C" fn(u32, *mut Point) = transmute(a::INPUT_DEVICE_POINT as usize);
    let mut p = Point::default();
    point(get(event), &mut p);
    p
}

pub(crate) unsafe fn entry_source(grid: u32, event: u32) -> bool {
    let target = current_target(event);
    if !safe_ptr(target) || !ancestor_of_grid(grid, target) {
        return false;
    }
    let background = rh_platform_launcher_scroll_callback();
    target == MOTION_PARENT
        || descriptor(target, a::LAUNCHER_POSITION_EVENT) != 0
        || descriptor(target, entry_dispatch as *const () as u32) != 0
        || (background != 0 && descriptor(target, background) != 0)
}

pub(crate) unsafe fn pressed_icon(event: u32, grid: u32) -> bool {
    // LVGL event.original_target is +4 (current_target is +0). Native grid
    // events similarly accept an icon or a label whose parent is the icon.
    let mut icon = word(event, 4);
    if !safe_ptr(icon) || icon == grid {
        return false;
    }
    if parent(icon) != grid {
        icon = parent(icon);
    }
    safe_ptr(icon) && parent(icon) == grid && descriptor(icon, a::LAUNCHER_ICON_CLICK) != 0
}

pub(crate) unsafe fn navigation(grid: u32, event: u32, chain: u32) {
    let mask = LV_OBJ_FLAG_SCROLL_CHAIN | LV_OBJ_FLAG_GESTURE_BUBBLE;
    let old = word(grid, 0x2c);
    let next = chain
        | if chain != 0 {
            LV_OBJ_FLAG_GESTURE_BUBBLE
        } else {
            0
        };
    write(grid, 0x2c, (old & !mask) | next);
    if chain != 0 && old & LV_OBJ_FLAG_SCROLL_CHAIN == 0 {
        let get: unsafe extern "C" fn(u32) -> u32 = transmute(a::EVENT_INPUT_DEVICE as usize);
        let input = get(event);
        if safe_ptr(input) {
            // The native scroll chooser (0x9FE70) adds the next pointer delta
            // to input+0x58/5C. Discard distance already consumed by the cloud,
            // so handing off at an edge cannot jump the parent by that distance.
            write(input, 0x58, 0);
            write(input, 0x5c, 0);
        }
    }
}

pub(crate) unsafe fn frame_timer_start() -> bool {
    if FRAME_TIMER == 0 {
        FRAME_TIMER = timer_create(
            crate::honeycomb::animation_tick as *const () as u32,
            1000,
            0,
        );
        FRAME_PERIOD = 1000;
    }
    FRAME_TIMER != 0 && timer_period_of(FRAME_TIMER) == FRAME_PERIOD
}

pub(crate) unsafe fn frame_timer_active(active: bool) {
    if FRAME_TIMER == 0 || timer_period_of(FRAME_TIMER) != FRAME_PERIOD {
        return;
    }
    let period = if active { 16 } else { 1000 };
    if period != FRAME_PERIOD {
        timer_set_period(FRAME_TIMER, period);
        FRAME_PERIOD = period;
    }
}

pub(crate) unsafe fn launch(event: u32) {
    let f: unsafe extern "C" fn(u32) = transmute(a::LAUNCHER_ICON_CLICK as usize);
    f(event); // original target, original app_id, original app-manager dispatch
}

/// Capture what is currently drawn, not user_data's already-final destination.
/// The native edit-return writer can still be halfway through its 200 ms move.
pub(crate) unsafe fn current_frame(icon: u32) -> Option<Frame> {
    let grid = parent(icon);
    if !safe_ptr(grid) {
        return None;
    }
    let width = word(icon, 0x44);
    let zoom = word(icon, 0x50);
    if !(1..=512).contains(&width) || !(1..=1024).contains(&zoom) {
        return None;
    }
    let x = (word(icon, 0x1c) as i32 as i64 + word(icon, 0x24) as i32 as i64 + 1) / 2
        - word(grid, 0x1c) as i32 as i64;
    let y = (word(icon, 0x20) as i32 as i64 + word(icon, 0x28) as i32 as i64 + 1) / 2
        - word(grid, 0x20) as i32 as i64;
    if x.abs() > 8192 || y.abs() > 8192 {
        return None;
    }
    Some(Frame {
        x: x as i32,
        y: y as i32,
        diameter: ((width * zoom) >> 8).max(1) as i32,
        visible: word(icon, 0x2c) & 1 == 0,
    })
}

pub(crate) unsafe fn stop_return_animations(grid: u32, icons: &[u32]) {
    let del: unsafe extern "C" fn(u32, u32) -> u32 = transmute(a::ANIMATION_DELETE as usize);
    // Mode-transition animations have icon vars but need not set ud+0x13.
    // Stop these once after capturing their current geometry, before reflow.
    for &icon in icons {
        del(icon, 0);
        let ud = word(icon, 0x18);
        if safe_ptr(ud) {
            core::ptr::write_volatile((ud + 0x13) as *mut u8, 0);
        }
    }
    write(grid, 0x8b4, 0);
}

pub(crate) unsafe fn begin_frame(grid: u32, icons: &[u32]) {
    hide_button();
    let del: unsafe extern "C" fn(u32, u32) -> u32 = transmute(a::ANIMATION_DELETE as usize);
    // Grid entry/fling animation is cancelled once on attach. Do not walk the
    // global animation list again for every drag frame.
    for &icon in icons {
        let ud = word(icon, 0x18);
        if safe_ptr(ud) && core::ptr::read_volatile((ud + 0x13) as *const u8) != 0 {
            del(icon, 0);
            core::ptr::write_volatile((ud + 0x13) as *mut u8, 0);
        }
    }
    // Native per-icon entry/position animations gate the geometry commit. They
    // are cancelled above before clearing their pending counter.
    write(grid, 0x8b4, 0);
}

/// Safe inside the layout phase: only native rectangle writes, no dirty setter.
pub(crate) unsafe fn commit_rects(grid: u32) {
    let commit: unsafe extern "C" fn(u32, u32) = transmute(a::LAUNCHER_COMMIT_RECTS as usize);
    commit(grid, 0);
}

/// UI timer only, after preparing transforms and visibility.
pub(crate) unsafe fn present(grid: u32) {
    commit_rects(grid);
    // Match the stock renderer's final invalidation. Geometry writes alone do
    // not schedule pixels, especially when pan changes but zoom stays equal.
    obj_invalidate(grid);
}

pub(crate) unsafe fn place(icon: u32, f: Frame) -> bool {
    let ud = word(icon, 0x18);
    let grid = parent(icon);
    if !safe_ptr(ud) || !safe_ptr(grid) {
        return false;
    }
    if !f.visible {
        if word(icon, 0x2c) & 1 == 0 {
            obj_add_flag(icon, 1);
        }
        return true;
    }
    let w = word(icon, 0x44).max(1);
    // Preserve the captured integer diameter across a zoom -> pixels -> zoom
    // round trip; truncating twice can shrink the takeover frame by one pixel.
    let zoom = (f.diameter as u32 * 256).div_ceil(w).clamp(1, 512);
    let set: unsafe extern "C" fn(u32, u32) = transmute(a::IMAGE_SET_ZOOM as usize);
    if word(icon, 0x50) != zoom || word(icon, 0x54) != zoom {
        set(icon, zoom);
    }
    if !layout_rect(icon, f) {
        return false;
    }
    // In .043 this API dirties the parent even when HIDDEN was already clear.
    if word(icon, 0x2c) & 1 != 0 {
        obj_clear_flag(icon, 1);
    }
    true
}

/// Pure geometry source for the registered layout phase. Read the prepared
/// transform; never call setters here, even ones which appear to be no-ops.
pub(crate) unsafe fn layout_rect(icon: u32, f: Frame) -> bool {
    let ud = word(icon, 0x18);
    let grid = parent(icon);
    if !safe_ptr(ud) || !safe_ptr(grid) {
        return false;
    }
    if !f.visible {
        return true;
    }
    let width = ((word(icon, 0x44).max(1) * word(icon, 0x50).max(1)) >> 8).max(1);
    let height = ((word(icon, 0x48).max(1) * word(icon, 0x54).max(1)) >> 8).max(1);
    // The launcher uses user_data->{x,y,w,h} as its geometry source. A generic
    // obj_align style is overwritten by that registered layout on the next pass.
    write(
        ud,
        0,
        (word(grid, 0x1c) as i32 + f.x - (width / 2) as i32) as u32,
    );
    write(
        ud,
        4,
        (word(grid, 0x20) as i32 + f.y - (height / 2) as i32) as u32,
    );
    write(ud, 8, width);
    write(ud, 12, height);
    true
}

/// Observe the coordinates AFTER the native layout commit. This catches the
/// reported failure on device instead of silently leaving a scale-only view.
pub(crate) unsafe fn geometry_matches(icon: u32, f: Frame) -> bool {
    if !f.visible {
        return true;
    }
    let grid = parent(icon);
    if !safe_ptr(grid) {
        return false;
    }
    let cx = (word(icon, 0x1c) as i32 + word(icon, 0x24) as i32 + 1) / 2;
    let cy = (word(icon, 0x20) as i32 + word(icon, 0x28) as i32 + 1) / 2;
    let dx = cx - (word(grid, 0x1c) as i32 + f.x);
    let dy = cy - (word(grid, 0x20) as i32 + f.y);
    dx.abs() <= 1 && dy.abs() <= 1
}
