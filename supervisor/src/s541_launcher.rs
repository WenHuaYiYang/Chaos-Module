//! 仅替换官方布局按钮，不接管图标、名称、布局、排序或输入。
use crate::{fw_api as fw, mem::{rd32, safe_ptr}};
use crate::fw_api::honeycomb as native;
use crate::honeycomb_layout::Point;
const CONFIG: &[u8] = b"/data/chaos/s541-launcher.cfg\0";
static mut LOADED: bool = false;
static mut ENABLED: bool = false;
static mut MODE: usize = 0;
static mut ROOT: u32 = 0;
static mut OWNER: u32 = 0;
static mut VIEW: u32 = 0;
static mut LAYER: u32 = 0;
static mut BUTTON: u32 = 0;
static mut BUTTON_ALPHA: u32 = 255;
static mut BUTTON_CLICKABLE: bool = false;
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
    save(); clear();
}
pub(crate) unsafe fn disable() {
    if enabled() { ENABLED = false; save(); clear(); }
}
unsafe fn clear() {
    if safe_ptr(ROOT) {
        if LAYER != 0 && fw::obj_is_child_of(ROOT, LAYER) { fw::obj_delete(LAYER); }
        if BUTTON != 0 && fw::obj_is_child_of(ROOT, BUTTON) {
            fw::obj_set_local_style_prop(BUTTON, 95, BUTTON_ALPHA, 0);
            if BUTTON_CLICKABLE { fw::obj_add_flag(BUTTON, fw::LV_OBJ_FLAG_CLICKABLE); }
        }
    }
    VIEW = 0; LAYER = 0; ROOT = 0; OWNER = 0; GLASS = 0; BUTTON = 0;
    SHOWN_MODE = 3;
    GLASS_VALID = false; GLASS_WAIT = 2;
    NEEDS_GLASS = false; GLASS_FAILURES = 0;
    LAST_PAN = Point { x: i32::MAX, y: i32::MAX };
}
unsafe extern "C" fn deleted(event: u32) {
    if fw::event_get_code(event) != 36 { return; }
    let object = native::current_target(event);
    if object == LAYER { LAYER = 0; VIEW = 0; GLASS = 0; }
    if object == VIEW { VIEW = 0; GLASS = 0; }
}
unsafe extern "C" fn select(event: u32) {
    if !enabled() || fw::event_get_code(event) != 4 { return; }
    let next = fw::event_user_data(event) as usize;
    if next < 3 && next != MODE && native::s541_select([0, 2, 1][next]) { MODE = next; save(); }
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
unsafe fn create(grid: u32, button: u32) -> bool {
    OWNER = grid; ROOT = native::parent(grid); SHOWN_MODE = MODE;
    BUTTON = button; BUTTON_ALPHA = fw::fw_style_get_prop(button, 0, 95);
    BUTTON_CLICKABLE = rd32((button + 0x2c) as *const u32) & fw::LV_OBJ_FLAG_CLICKABLE != 0;
    // 附加层只承载底部控制条；应用图标与名称完全由官方launcher管理。
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
    hide(VIEW, true); GLASS_VALID = false; GLASS_WAIT = 2;
    true
}
pub(crate) unsafe fn render(grid: u32) {
    if !enabled() || fw::top_app_id() != fw::HOME_APP_ID { return; }
    let Some((button, x, y)) = native::s541_anchor(grid) else { clear(); return; };
    // 仅上移自有控制条，不改变官方按钮占位和应用布局。
    let y = y - 32;
    let preference = native::s541_preference();
    MODE = match preference { 0 => 0, 2 => 1, 1 => 2, _ => { clear(); return; } };
    let valid = VIEW != 0 && LAYER != 0 && ROOT == native::parent(grid) && fw::obj_is_child_of(ROOT, LAYER);
    if !valid || OWNER != grid || SHOWN_MODE != MODE || BUTTON != button {
        clear(); if !create(grid, button) { clear(); return; }
    }
    // 仅遮掉原按钮外观和点击，保留原生HIDDEN、坐标及滚动布局计算。
    if fw::fw_style_get_prop(button, 0, 95) != 0 { fw::obj_set_local_style_prop(button, 95, 0, 0); }
    fw::obj_clear_flag(button, fw::LV_OBJ_FLAG_CLICKABLE);
    if rd32((button + 0x2c) as *const u32) & 1 != 0 { hide(VIEW, true); return; }
    let pos = Point { x, y };
    if LAST_PAN != pos { fw::obj_align(VIEW, fw::align::TOP_LEFT, x, y); LAST_PAN = pos; }
    NEEDS_GLASS = !GLASS_VALID && x >= 0 && x <= 48 && y >= 0 && y + 96 <= 480;
    if !NEEDS_GLASS && !GLASS_VALID { hide(VIEW, true); return; }
    if !GLASS_VALID {
        // 先让原生背景在没有控制条的情况下完成一帧，避免捕获自身。
        if GLASS_WAIT != 0 { GLASS_WAIT -= 1; hide(VIEW, true); return; }
        if rd32((ROOT + 0x20) as *const u32) != 0 || !fw::s541_frame_idle() { return; }
        if fw::s541_capture_region(&mut SAMPLE, x as u32, y as u32).is_err() {
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
    if crate::honeycomb::enabled() { clear(); return; }
    let grid = native::launcher();
    if grid == 0 || !native::s541_supported(grid) { clear(); return; }
    if fw::top_app_id() != fw::HOME_APP_ID { GLASS_VALID = false; GLASS_WAIT = 2; GLASS_FAILURES = 0; return; }
    if !native::visible(grid) { if VIEW != 0 { hide(VIEW, true); } return; }
    render(grid);
}
pub(crate) unsafe fn pending() -> bool {
    ENABLED && NEEDS_GLASS && OWNER != 0 && native::launcher() == OWNER
        && fw::screen_is_on() && fw::top_app_id() == fw::HOME_APP_ID && native::visible(OWNER)
}

#[cfg(test)]
#[path = "../tests/s541_cases.rs"]
mod tests;
