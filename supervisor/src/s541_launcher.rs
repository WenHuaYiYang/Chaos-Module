//! s541控制条：复用蜂窝的原生图标布局/输入接管，不创建替代launcher或应用图标。
use crate::{fw_api as fw, mem::{rd32, safe_ptr}};
use crate::fw_api::honeycomb as native;
use crate::honeycomb_layout::{Frame, Point, MAX_ICONS};
const CONFIG: &[u8] = b"/data/chaos/s541-launcher.cfg\0";
static mut LOADED: bool = false;
static mut ENABLED: bool = false;
static mut MODE: usize = 0;
static mut ROOT: u32 = 0;
static mut OWNER: u32 = 0;
static mut VIEW: u32 = 0;
static mut LAYER: u32 = 0;
static mut LABELS: [u32; MAX_ICONS] = [0; MAX_ICONS];
static mut ICONS: [u32; MAX_ICONS] = [0; MAX_ICONS];
static mut COUNT: usize = 0;
static mut SHOWN_MODE: usize = 3;
static mut GLASS: u32 = 0;
static mut GLASS_VALID: bool = false;
static mut GLASS_WAIT: u8 = 2;
static mut GLASS_INDEX: usize = 0;
static mut NEEDS_GLASS: bool = false;
static mut GLASS_FAILURES: u8 = 0;
static mut LAST_PAN: Point = Point { x: i32::MAX, y: i32::MAX };
#[repr(align(64))]
struct Pixels([u8; 110592]);
static PIXELS: [Pixels; 3] = [
    Pixels(*include_bytes!("../assets/s541/selector-0.bgra")),
    Pixels(*include_bytes!("../assets/s541/selector-2.bgra")),
    Pixels(*include_bytes!("../assets/s541/selector-1.bgra")),
];
static mut IMAGES: [[u32; 7]; 3] = [[0; 7]; 3];
static mut GLASS_IMAGES: [[u32; 7]; 2] = [[0; 7]; 2];
static mut GLASS_PIXELS: [Pixels; 2] = [Pixels([0; 110592]), Pixels([0; 110592])];
static mut SAMPLE: [u8; 5184] = [0; 5184];
static mut WORK: [u8; 5184] = [0; 5184];
static MASK: &[u8; 27648] = include_bytes!("../assets/s541/background.alpha");

pub(crate) unsafe fn enabled() -> bool {
    if !LOADED {
        LOADED = true;
        let fd = fw::open(CONFIG.as_ptr(), fw::oflag::RDONLY, 0);
        if fd >= 0 {
            let mut b = [0u8; 6];
            let n = fw::read(fd, b.as_mut_ptr(), 6); fw::close(fd);
            if n == 6 && &b[..4] == b"S541" && b[4] <= 1 && b[5] < 3 {
                ENABLED = b[4] != 0; MODE = b[5] as usize;
                if ENABLED {
                    crate::desktop_scale::disable_for_honeycomb();
                    crate::honeycomb::s541_changed(false);
                }
            }
        }
    }
    ENABLED
}
unsafe fn save() {
    fw::capture_mkdir_path(b"/data/chaos\0".as_ptr());
    let b = [b'S', b'5', b'4', b'1', ENABLED as u8, MODE as u8];
    let fd = fw::open(CONFIG.as_ptr(), fw::oflag::WRONLY | fw::oflag::CREAT, 0o640);
    if fd >= 0 { fw::write(fd, b.as_ptr(), 6); fw::close(fd); }
}
pub(crate) unsafe fn toggle() {
    ENABLED = !enabled();
    if ENABLED { crate::honeycomb::set_enabled(false); crate::desktop_scale::disable_for_honeycomb(); }
    save(); clear(); crate::honeycomb::s541_changed(false);
}
pub(crate) unsafe fn disable() {
    if enabled() { ENABLED = false; save(); clear(); crate::honeycomb::s541_changed(false); }
}
fn columns(mode: usize) -> usize { if mode == 2 { 1 } else { 3 } }
fn step(mode: usize) -> i32 { if mode == 1 { 112 } else { 88 } }
pub(crate) unsafe fn range(count: usize) -> i32 {
    (count.div_ceil(columns(MODE)) as i32 * step(MODE) + 144 - 480).max(0)
}
pub(crate) unsafe fn frame(index: usize, pan: Point) -> Frame {
    let y = 48 + (index / columns(MODE)) as i32 * step(MODE) + pan.y;
    Frame { x: if MODE == 2 { 50 } else { 60 + (index % 3) as i32 * 108 },
        y, diameter: if MODE == 0 { 72 } else { 64 }, visible: y > -80 && y < 560 }
}
unsafe fn clear() {
    if safe_ptr(ROOT) {
        if LAYER != 0 && fw::obj_is_child_of(ROOT, LAYER) { fw::obj_delete(LAYER); }
    }
    VIEW = 0; LAYER = 0; ROOT = 0; OWNER = 0; COUNT = 0; GLASS = 0;
    LABELS = [0; MAX_ICONS]; ICONS = [0; MAX_ICONS]; SHOWN_MODE = 3;
    GLASS_VALID = false; GLASS_WAIT = 2;
    NEEDS_GLASS = false; GLASS_FAILURES = 0;
    LAST_PAN = Point { x: i32::MAX, y: i32::MAX };
}
unsafe extern "C" fn deleted(event: u32) {
    if fw::event_get_code(event) != 36 { return; }
    let object = native::current_target(event);
    if object == LAYER { LAYER = 0; VIEW = 0; GLASS = 0; LABELS = [0; MAX_ICONS]; }
    if object == VIEW { VIEW = 0; GLASS = 0; }
    for i in 0..COUNT { if LABELS[i] == object { LABELS[i] = 0; } }
}
unsafe extern "C" fn select(event: u32) {
    if !enabled() || fw::event_get_code(event) != 4 { return; }
    let next = fw::event_user_data(event) as usize;
    if next < 3 && next != MODE { MODE = next; save(); crate::honeycomb::s541_changed(true); }
}
unsafe fn plain(parent: u32, w: u32, h: u32) -> u32 {
    let o = fw::obj_create(parent, 0);
    if o != 0 {
        fw::obj_clear_box_spacing(o); fw::background_set_opa(o, 0);
        fw::obj_clear_scrollable(o); fw::obj_set_size_wh(o, w, h);
        fw::obj_add_flag(o, 0x40000 | fw::LV_OBJ_FLAG_GESTURE_BUBBLE);
    }
    o
}
unsafe fn hide(o: u32, hidden: bool) {
    let old = rd32((o + 0x2c) as *const u32) & 1 != 0;
    if hidden && !old { fw::obj_add_flag(o, 1); }
    else if !hidden && old { fw::obj_clear_flag(o, 1); }
}
unsafe fn create(grid: u32, icons: &[u32]) -> bool {
    OWNER = grid; ROOT = native::parent(grid); COUNT = icons.len(); SHOWN_MODE = MODE;
    ICONS[..COUNT].copy_from_slice(icons);
    // 名称放入单个透明、不可点击的附加层，不增加原生root的大量直接子对象。
    // 原生边界对子表有24项护栏，不能直接在root挂128个名称标签。
    LAYER = plain(ROOT, 336, 480);
    if LAYER == 0 { return false; }
    fw::obj_align(LAYER, fw::align::TOP_LEFT, 0, 0);
    fw::obj_clear_flag(LAYER, fw::LV_OBJ_FLAG_CLICKABLE);
    fw::obj_add_event(LAYER, deleted as *const () as u32, 36, 0);
    VIEW = plain(LAYER, 288, 96);
    if VIEW == 0 { return false; }
    fw::obj_add_event(VIEW, deleted as *const () as u32, 36, 0);
    GLASS = fw::background_image_create(VIEW);
    let mask = fw::background_image_create(VIEW);
    if GLASS == 0 || mask == 0 { return false; }
    IMAGES[MODE] = [0x11019, 288 | (96 << 16), 1152, 110592, PIXELS[MODE].0.as_ptr() as u32, 0, 0];
    fw::background_image_set_source(mask, IMAGES[MODE].as_ptr() as u32);
    fw::obj_clear_flag(mask, fw::LV_OBJ_FLAG_CLICKABLE);
    fw::obj_clear_flag(GLASS, fw::LV_OBJ_FLAG_CLICKABLE);
    for i in 0..3 {
        let b = plain(VIEW, 83, 83); if b == 0 { return false; }
        fw::obj_align(b, fw::align::TOP_LEFT, 6 + i * 96, 6);
        fw::obj_add_flag(b, fw::LV_OBJ_FLAG_CLICKABLE);
        fw::obj_add_event(b, select as *const () as u32, 4, i as u32);
    }
    if MODE != 0 {
        for (i, &icon) in icons.iter().enumerate() {
            let Some((_, _, name)) = native::s541_icon(icon) else { return false; };
            if name.is_null() { return false; }
            let label = fw::label_create(LAYER); if label == 0 { return false; }
            LABELS[i] = label;
            fw::obj_add_event(label, deleted as *const () as u32, 36, 0);
            fw::obj_add_flag(label, 0x40000);
            fw::obj_clear_flag(label, fw::LV_OBJ_FLAG_CLICKABLE);
            fw::style_apply(label, crate::FW_STYLE_HR_NAME, 255, 0);
            fw::obj_set_size_wh(label, if MODE == 2 { 224 } else { 104 }, 36);
            fw::label_set_text(label, name);
        }
    }
    hide(VIEW, true); GLASS_VALID = false; GLASS_WAIT = 2;
    true
}
pub(crate) unsafe fn render(grid: u32, icons: &[u32], pan: Point, moving: bool) {
    if !enabled() || fw::top_app_id() != fw::HOME_APP_ID { return; }
    let valid = VIEW != 0 && LAYER != 0 && ROOT == native::parent(grid) && fw::obj_is_child_of(ROOT, LAYER);
    if !valid || OWNER != grid || SHOWN_MODE != MODE || &ICONS[..COUNT] != icons {
        clear(); if !create(grid, icons) { clear(); return; }
    }
    if LAST_PAN != pan { for i in 0..COUNT {
        if LABELS[i] == 0 { continue; }
        let f = frame(i, pan);
        fw::obj_align(LABELS[i], fw::align::TOP_LEFT,
            if MODE == 2 { 96 } else { f.x - 52 }, if MODE == 2 { f.y - 16 } else { f.y + 36 });
        hide(LABELS[i], !f.visible);
    } }
    let y = icons.len().div_ceil(columns(MODE)) as i32 * step(MODE) + 24 + pan.y;
    if LAST_PAN != pan { fw::obj_align(VIEW, fw::align::TOP_LEFT, 24, y); LAST_PAN = pan; }
    NEEDS_GLASS = !GLASS_VALID && y >= 0 && y + 96 <= 480 && !moving;
    if !NEEDS_GLASS && !GLASS_VALID { hide(VIEW, true); return; }
    if !GLASS_VALID {
        // 先让原生背景在没有控制条的情况下完成一帧，避免捕获自身。
        if GLASS_WAIT != 0 { GLASS_WAIT -= 1; hide(VIEW, true); return; }
        if rd32((ROOT + 0x20) as *const u32) != 0 || !fw::s541_frame_idle() { return; }
        if fw::s541_capture_region(&mut SAMPLE, 24, y as u32).is_err() {
            GLASS_FAILURES += 1;
            if GLASS_FAILURES >= 3 {
                crate::log::line("s541控制条：背景采样失败，使用透明底图");
                GLASS_VALID = true; NEEDS_GLASS = false; hide(VIEW, false);
            }
            return;
        }
        crate::s541_pixels::blur(&mut SAMPLE, &mut WORK);
        let next = GLASS_INDEX ^ 1;
        if GLASS_IMAGES[next][0] != 0 { fw::fw_img_free_by_path(GLASS_IMAGES[next].as_ptr() as u32); }
        crate::s541_pixels::glass(&SAMPLE, MASK, &mut GLASS_PIXELS[next].0);
        fw::s541_clean_pixels(GLASS_PIXELS[next].0.as_ptr(), 110592);
        GLASS_IMAGES[next] = [0x11019, 288 | (96 << 16), 1152, 110592, GLASS_PIXELS[next].0.as_ptr() as u32, 0, 0];
        fw::background_image_set_source(GLASS, GLASS_IMAGES[next].as_ptr() as u32);
        GLASS_INDEX = next; GLASS_VALID = true; NEEDS_GLASS = false;
    }
    hide(VIEW, false);
}
pub(crate) unsafe fn tick() {
    if !enabled() { if ROOT != 0 { clear(); } return; }
    if crate::honeycomb::enabled() { crate::honeycomb::set_enabled(false); }
    let grid = native::launcher();
    if grid == 0 || !native::supported(grid) { clear(); return; }
    if fw::top_app_id() != fw::HOME_APP_ID { GLASS_VALID = false; GLASS_WAIT = 2; GLASS_FAILURES = 0; return; }
    crate::honeycomb::s541_ui_tick();
}
pub(crate) unsafe fn pending() -> bool {
    ENABLED && NEEDS_GLASS && OWNER != 0 && native::launcher() == OWNER
        && fw::screen_is_on() && fw::top_app_id() == fw::HOME_APP_ID && native::visible(OWNER)
}

#[cfg(test)]
#[path = "../tests/s541_cases.rs"]
mod tests;
