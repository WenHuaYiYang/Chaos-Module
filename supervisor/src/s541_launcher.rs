//! 参考用户提供的 syspatch Q63 控制按钮；所有原生调用使用 p67/.043 边界。
use crate::{fw_api as fw, mem::{rd32, safe_ptr}};
use crate::fw_api::honeycomb as native;
use crate::honeycomb_layout::MAX_ICONS;

const CONFIG: &[u8] = b"/data/chaos/s541-launcher.cfg\0";
static mut LOADED: bool = false;
static mut ENABLED: bool = false;
static mut MODE: usize = 0; // visual: icons, icons+names, list
static mut OWNER: u32 = 0;
static mut ROOT: u32 = 0;
static mut VIEW: u32 = 0;
static mut BUTTON: u32 = 0;
static mut GRID_HIDDEN: bool = false;
static mut BUTTON_HIDDEN: bool = false;
static mut DIRTY: bool = false;
static mut REVISION: u32 = 0;
static mut SAVED_ICONS: [u32; MAX_ICONS] = [0; MAX_ICONS];
static mut ICON_HIDDEN: [bool; MAX_ICONS] = [false; MAX_ICONS];
static mut ICON_COUNT: usize = 0;

#[repr(align(64))]
struct Pixels([u8; 110592]);
static PIXELS: [Pixels; 3] = [
    Pixels(*include_bytes!("../assets/s541/selector-0.bgra")),
    Pixels(*include_bytes!("../assets/s541/selector-2.bgra")),
    Pixels(*include_bytes!("../assets/s541/selector-1.bgra")),
];
// Original preference indices are 0=icons, 2=named, 1=list.
static mut IMAGES: [[u32; 7]; 3] = [[0; 7]; 3];

pub(crate) unsafe fn enabled() -> bool {
    if !LOADED {
        LOADED = true;
        let fd = fw::open(CONFIG.as_ptr(), fw::oflag::RDONLY, 0);
        if fd >= 0 {
            let mut b = [0u8; 6];
            let n = fw::read(fd, b.as_mut_ptr(), 6); fw::close(fd);
            if n == 6 && &b[..4] == b"S541" && b[4] <= 1 && b[5] < 3 {
                ENABLED = b[4] != 0; MODE = b[5] as usize;
                if ENABLED { crate::honeycomb::set_enabled(false); }
            }
        }
    }
    ENABLED
}

#[cfg(test)]
#[path = "../tests/s541_cases.rs"]
mod tests;

unsafe fn save() {
    fw::capture_mkdir_path(b"/data/chaos\0".as_ptr());
    let b = [b'S', b'5', b'4', b'1', ENABLED as u8, MODE as u8];
    let fd = fw::open(CONFIG.as_ptr(), fw::oflag::WRONLY | fw::oflag::CREAT, 0o640);
    if fd >= 0 { fw::write(fd, b.as_ptr(), 6); fw::close(fd); }
}

pub(crate) unsafe fn toggle() {
    ENABLED = !enabled();
    if ENABLED { crate::honeycomb::set_enabled(false); }
    DIRTY = true; save();
}

pub(crate) unsafe fn disable() {
    if enabled() { ENABLED = false; DIRTY = true; save(); }
}

unsafe fn alive() -> bool {
    safe_ptr(ROOT) && safe_ptr(VIEW) && fw::obj_is_child_of(ROOT, VIEW)
        && native::launcher() == OWNER
}

unsafe fn clear() {
    if safe_ptr(ROOT) && safe_ptr(VIEW) && fw::obj_is_child_of(ROOT, VIEW) {
        fw::obj_delete(VIEW);
    }
    if OWNER != 0 && native::launcher() == OWNER {
        if !GRID_HIDDEN { fw::obj_clear_flag(OWNER, 1); }
        for i in 0..ICON_COUNT {
            let icon = SAVED_ICONS[i];
            if fw::obj_is_child_of(OWNER, icon) && !ICON_HIDDEN[i] {
                fw::obj_clear_flag(icon, 1);
            }
        }
        if BUTTON != 0 && fw::obj_is_child_of(ROOT, BUTTON) && !BUTTON_HIDDEN {
            fw::obj_clear_flag(BUTTON, 1);
        }
    }
    VIEW = 0; OWNER = 0; ROOT = 0; BUTTON = 0; REVISION = 0; ICON_COUNT = 0;
}

unsafe extern "C" fn deleted(event: u32) {
    if fw::event_get_code(event) == 36 && native::current_target(event) == VIEW {
        // 原生页销毁时立即作废句柄，避免下一次同址复用被误认为我们自己的控件。
        VIEW = 0; DIRTY = true;
    }
}

unsafe extern "C" fn launch(event: u32) {
    if alive() && fw::event_get_code(event) == 4 { native::launch(event); }
}

unsafe extern "C" fn select(event: u32) {
    if !alive() || fw::event_get_code(event) != 4 { return; }
    let next = fw::event_user_data(event) as usize;
    if next < 3 && next != MODE { MODE = next; DIRTY = true; save(); }
}

unsafe fn object(parent: u32, x: i32, y: i32, w: u32, h: u32) -> u32 {
    let o = fw::obj_create(parent, 0);
    if o != 0 {
        fw::obj_clear_box_spacing(o); fw::background_set_opa(o, 0);
        fw::obj_clear_scrollable(o); fw::obj_set_size_wh(o, w, h);
        fw::obj_align(o, fw::align::TOP_LEFT, x, y);
    }
    o
}

unsafe fn hide(o: u32) {
    if rd32((o + 0x2c) as *const u32) & 1 == 0 { fw::obj_add_flag(o, 1); }
}

unsafe fn build(grid: u32, icons: &[u32]) -> bool {
    OWNER = grid; ROOT = native::parent(grid); BUTTON = native::s541_button(grid);
    GRID_HIDDEN = rd32((grid + 0x2c) as *const u32) & 1 != 0;
    BUTTON_HIDDEN = BUTTON != 0 && rd32((BUTTON + 0x2c) as *const u32) & 1 != 0;
    ICON_COUNT = icons.len();
    for (i, &icon) in icons.iter().enumerate() {
        SAVED_ICONS[i] = icon;
        ICON_HIDDEN[i] = rd32((icon + 0x2c) as *const u32) & 1 != 0;
    }
    VIEW = object(ROOT, 0, 0, 336, 480);
    if VIEW == 0 { return false; }
    fw::obj_add_event(VIEW, deleted as *const () as u32, 36, 0);
    fw::obj_add_flag(VIEW, fw::LV_OBJ_FLAG_CLICKABLE | fw::LV_OBJ_FLAG_SCROLLABLE
        | fw::LV_OBJ_FLAG_SCROLL_ELASTIC | fw::LV_OBJ_FLAG_GESTURE_BUBBLE | 0x40000);
    fw::obj_set_scroll_dir(VIEW, 12); // .043 LV_DIR_VER
    let columns = if MODE == 2 { 1 } else { 3 };
    let step = if MODE == 1 { 112 } else { 88 };
    for (i, &icon) in icons.iter().enumerate() {
        let Some((id, source, name)) = native::s541_icon(icon) else { return false; };
        if name.is_null() { return false; }
        let x = if columns == 1 { 8 } else { (i % 3) as i32 * 108 + 6 };
        let y = (i / columns) as i32 * step + 12;
        let cell = object(VIEW, x, y, if columns == 1 { 320 } else { 104 }, step as u32);
        if cell == 0 { return false; }
        fw::obj_add_flag(cell, fw::LV_OBJ_FLAG_CLICKABLE | fw::LV_OBJ_FLAG_SCROLL_CHAIN);
        fw::obj_add_event(cell, launch as *const () as u32, 4, id);
        let image = fw::background_image_create(cell);
        if image == 0 { return false; }
        fw::background_image_set_source(image, source);
        fw::background_image_set_pivot(image, 0, 0);
        let width = rd32((icon + 0x44) as *const u32).max(1);
        let scale = (64 * 256 / width).clamp(1, 512) as i32;
        fw::image_set_scale(image, scale, scale);
        fw::obj_clear_scrollable(image); fw::obj_clear_flag(image, fw::LV_OBJ_FLAG_CLICKABLE);
        fw::obj_align(image, fw::align::TOP_LEFT, if columns == 1 { 8 } else { 20 }, 4);
        if MODE != 0 {
            let label = fw::label_create(cell);
            if label == 0 { return false; }
            fw::style_apply(label, crate::FW_STYLE_HR_NAME, 255, 0);
            fw::obj_set_size_wh(label, if columns == 1 { 224 } else { 104 }, 36);
            fw::label_set_text(label, name);
            fw::obj_clear_flag(label, fw::LV_OBJ_FLAG_CLICKABLE);
            fw::obj_align(label, fw::align::TOP_LEFT, if columns == 1 { 88 } else { 0 }, if columns == 1 { 24 } else { 72 });
        }
    }
    let end = icons.len().div_ceil(columns) as i32 * step + 28;
    let selector = object(VIEW, 24, end, 288, 112);
    if selector == 0 { return false; }
    IMAGES[MODE] = [0x11019, 288 | (96 << 16), 1152, 110592,
        PIXELS[MODE].0.as_ptr() as u32, 0, 0];
    let image = fw::background_image_create(selector);
    if image == 0 { return false; }
    fw::background_image_set_source(image, IMAGES[MODE].as_ptr() as u32);
    fw::obj_clear_flag(image, fw::LV_OBJ_FLAG_CLICKABLE);
    for i in 0..3 {
        let button = object(selector, 6 + i * 96, 6, 83, 83);
        if button == 0 { return false; }
        fw::obj_add_flag(button, fw::LV_OBJ_FLAG_CLICKABLE | fw::LV_OBJ_FLAG_SCROLL_CHAIN);
        fw::obj_add_event(button, select as *const () as u32, 4, i as u32);
    }
    // 保留网格自身的DRAW事件，Corona/旧版模糊都依赖它绘制背景；只隐藏原应用图标。
    for &icon in icons { hide(icon); }
    if BUTTON != 0 { hide(BUTTON); }
    true
}

pub(crate) unsafe fn tick() {
    let on = enabled();
    if !on || crate::honeycomb::enabled() { if VIEW != 0 { clear(); } return; }
    let grid = native::launcher();
    if grid == 0 { clear(); return; }
    if !native::supported(grid) || !fw::screen_is_on() { return; }
    if fw::top_app_id() != fw::HOME_APP_ID { return; }
    let mut icons = [0u32; MAX_ICONS];
    let n = native::icons(grid, &mut icons);
    if n == 0 { return; }
    let mut revision = 2166136261u32 ^ native::s541_button(grid);
    for &icon in &icons[..n] {
        revision = revision.wrapping_mul(16777619) ^ icon;
        revision ^= rd32((icon + 0x34) as *const u32);
    }
    if alive() && !DIRTY && REVISION == revision {
        for &icon in &icons[..n] { hide(icon); }
        if BUTTON != 0 && fw::obj_is_child_of(ROOT, BUTTON) { hide(BUTTON); }
        return;
    }
    clear(); DIRTY = false;
    if build(grid, &icons[..n]) { REVISION = revision; }
    else { clear(); crate::log::line("s541控制按钮：创建失败，保留系统桌面"); }
}
