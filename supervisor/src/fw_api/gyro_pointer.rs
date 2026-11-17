//! p67/.043 gyro and virtual LVGL pointer ABI. See docs/GYRO-POINTER.md.
//! All entrypoints/layouts are checked against the exact AP by verify_gyro_pointer_abi.py.
use core::{mem::transmute, ptr};
use super::*;

/// This firmware packs enum fields to bytes. lv_indev_read zeros exactly 20 bytes;
/// state is at +18 and continue_reading at +19 (file 0x9E938/0x9E99A).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct GyroInputData {
    pub x: i32, pub y: i32, pub key: u32, pub button: u32,
    pub encoder: i16, pub state: u8, pub continue_reading: u8,
}
const _: () = assert!(core::mem::size_of::<GyroInputData>() == 20);

pub unsafe fn gyro_input_create(callback: u32) -> u32 {
    let create: unsafe extern "C" fn() -> u32 = transmute(fwaddr::GYRO_INDEV_CREATE as usize);
    let input = create();
    if !crate::safe_ptr(input) { return 0; }
    let set_type: unsafe extern "C" fn(u32, u32) = transmute(fwaddr::GYRO_INDEV_SET_TYPE as usize);
    let set_read: unsafe extern "C" fn(u32, u32) = transmute(fwaddr::GYRO_INDEV_SET_READ as usize);
    set_type(input, 1); set_read(input, callback);
    gyro_input_period(input, 20);
    input
}

pub unsafe fn gyro_input_period(input: u32, period: u32) {
    if !crate::safe_ptr(input) { return; }
    let timer = ptr::read_volatile((input + 0x28) as *const u32);
    if crate::safe_ptr(timer) && timer_period_of(timer) != period { timer_set_period(timer, period); }
}

pub unsafe fn gyro_input_reset(input: u32) {
    if input == 0 { return; }
    let reset: unsafe extern "C" fn(u32, u32) = transmute(fwaddr::GYRO_INDEV_RESET as usize);
    reset(input, 0);
}

/// Only call from a maintenance timer, after the indev event dispatch has returned.
pub unsafe fn gyro_input_delete(input: u32) {
    if input == 0 { return; }
    gyro_input_reset(input);
    let delete: unsafe extern "C" fn(u32) = transmute(fwaddr::GYRO_INDEV_DELETE as usize);
    delete(input);
}

pub unsafe fn gyro_physical_touch(virtual_input: u32) -> bool {
    let next: unsafe extern "C" fn(u32) -> u32 = transmute(fwaddr::GYRO_INDEV_NEXT as usize);
    let display = ptr::read_volatile((virtual_input + 0x24) as *const u32);
    let mut input = next(0);
    for _ in 0..16 {
        if input == 0 { return false; }
        if !crate::safe_ptr(input) { return true; } // malformed list: suspend input
        if input != virtual_input && ptr::read_volatile(input as *const u8) == 1
            && ptr::read_volatile((input + 0x24) as *const u32) == display
            && ptr::read_volatile((input + 0x0c) as *const u8) == 1 { return true; }
        input = next(input);
    }
    input != 0
}

/// Read-only page identity on the UI owner. Reuses the audited page manager/fields
/// in sys.rs; never dereference a cached page root. Home/launcher share 16:0, so
/// include the existing watch-face visibility flag to distinguish that transition.
pub unsafe fn gyro_page_identity() -> [u32; 4] {
    let desc = top_page_desc();
    if !crate::safe_ptr(desc) { return [0; 4]; }
    let ids = ptr::read_volatile((desc + DESC_PAGE_ID) as *const u32);
    let root = ptr::read_volatile((desc + DESC_ROOT) as *const u32);
    let home_view = if ids >> 16 == HOME_APP_ID { watchface_is_on() } else { 0 };
    [desc, ids, root, home_view]
}

pub unsafe fn gyro_sensor_open() -> i32 {
    // Native sensor_gyro metadata has a 24-byte event, not the newer 32-byte ABI.
    let fd = open(b"/dev/uorb/sensor_gyro0\0".as_ptr(), oflag::RDONLY | 0x40, 0);
    if fd < 0 { return fd; }
    // Native orb_set_interval (file 0x229838) forwards an integer number of us.
    if ioctl(fd, 0x0a81, 20_000) < 0 { close(fd); return -1; }
    fd
}

#[derive(Clone, Copy)]
pub struct GyroOverlay { pub root: u32, pub ring: u32, pub dot: u32, pub radius: i32 }

pub unsafe fn gyro_cursor_create(deleted: u32, size: u32, dot_size: u32) -> Option<GyroOverlay> {
    let display = display_default();
    if !crate::safe_ptr(display) { return None; }
    let system_layer: unsafe extern "C" fn(u32) -> u32 = transmute(fwaddr::GYRO_DISPLAY_SYS_LAYER as usize);
    let layer = system_layer(display);
    if !crate::safe_ptr(layer) { return None; }
    let root = obj_create(layer, 0);
    if root == 0 { return None; }
    obj_add_event(root, deleted, 36, 0);
    obj_clear_box_spacing(root);
    obj_clear_flag(root, LV_OBJ_FLAG_CLICKABLE | LV_OBJ_FLAG_SCROLLABLE | LV_OBJ_FLAG_SCROLL_CHAIN);
    obj_add_flag(root, 0x40000); // FLOATING: never scroll with an application
    obj_set_size_wh(root, size, size);
    background_set_opa(root, 0);
    // Local border width / shadow width; the container itself is transparent.
    obj_set_local_style_prop(root, 0x30, 0, 0);
    obj_set_local_style_prop(root, 60, 0, 0);
    let ring = arc_create(root);
    let dot = obj_create(root, 0);
    if ring == 0 || dot == 0 { obj_delete(root); return None; }
    for obj in [ring, dot] {
        obj_clear_box_spacing(obj);
        obj_clear_flag(obj, LV_OBJ_FLAG_CLICKABLE | LV_OBJ_FLAG_SCROLLABLE | LV_OBJ_FLAG_SCROLL_CHAIN);
        obj_set_local_style_prop(obj, 0x30, 0, 0);
        obj_set_local_style_prop(obj, 60, 0, 0);
    }
    obj_set_size_wh(ring, size, size); obj_align(ring, align::CENTER, 0, 0);
    obj_set_local_style_prop(ring, 80, 4, 0x20000); // ARC_WIDTH / INDICATOR
    obj_set_local_style_prop(ring, 82, 0x32D583, 0x20000); // ARC_COLOR: green
    obj_set_local_style_prop(ring, 83, 255, 0x20000);
    obj_set_local_style_prop(ring, 95, 0, 0x30000); // hide the interactive KNOB
    arc_set_progress(ring, 0);
    obj_set_size_wh(dot, dot_size, dot_size); obj_align(dot, align::CENTER, 0, 0);
    obj_set_local_style_prop(dot, 12, 0x7fff, 0); // circle radius
    obj_set_local_style_prop(dot, 48, 2, 0);
    obj_set_local_style_prop(dot, 49, 0x102018, 0); // dark outline on light backgrounds
    obj_set_local_style_prop(dot, 50, 255, 0);
    background_set_opa(dot, 230);
    Some(GyroOverlay { root, ring, dot, radius: (size / 2) as i32 })
}

pub unsafe fn gyro_cursor_show(overlay: GyroOverlay, x: i32, y: i32, progress: u32, calibrated: bool) {
    obj_clear_flag(overlay.root, 1); // HIDDEN
    obj_align(overlay.root, align::TOP_LEFT, x - overlay.radius, y - overlay.radius);
    obj_set_local_style_prop(overlay.dot, 28, if calibrated { 0xFFFFFF } else { 0xF5B942 }, 0);
    arc_set_progress(overlay.ring, progress);
}

pub unsafe fn gyro_cursor_hide(overlay: GyroOverlay) { obj_add_flag(overlay.root, 1); }
