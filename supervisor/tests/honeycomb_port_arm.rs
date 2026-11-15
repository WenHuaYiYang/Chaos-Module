//! ARM integration probe: compile the actual production firmware boundary.
//! The Python runner supplies firmware machine code and model object memory.
#![no_std]
#![no_main]
#![allow(dead_code, static_mut_refs)]
mod s541_launcher { pub unsafe fn disable() {} pub unsafe fn enabled()->bool{false} }
#[path = "../src/fw_api/fwaddr.rs"]
pub mod addresses;
#[path = "../src/desktop_scale.rs"]
mod desktop_scale;
#[path = "../src/honeycomb_layout.rs"]
mod honeycomb_layout;
#[path = "../src/mem.rs"]
mod mem;
#[path = "../src/fw_api/honeycomb.rs"]
pub mod production_port;
pub use fw_api::*;
mod state {
    pub const FW_INIT_BUFFER: u32 = 0x0C513A45;
}
mod log {
    pub fn line(_: &str) {}
    pub fn kv(_: &str, _: u32) {}
}

#[no_mangle]
pub extern "C" fn rh_platform_launcher_scroll_callback() -> u32 {
    0x20066001
}

mod honeycomb {
    static mut ON: bool = true;
    pub unsafe fn enabled() -> bool {
        ON
    }
    pub unsafe fn set_enabled(on: bool) {
        ON = on;
    }
    pub unsafe extern "C" fn class_event(_: u32, _: u32) {}
    pub unsafe extern "C" fn click_event(_: u32) -> u32 {
        0
    }
    pub unsafe extern "C" fn layout_event(_: u32, _: u32) {}
    pub unsafe extern "C" fn entry_event(_: u32) {}
    pub unsafe extern "C" fn animation_tick(_: u32) {}
    pub unsafe extern "C" fn chaos_honeycomb_pre_scroll(event: u32) -> u32 {
        if ON && crate::fw_api::event_get_code(event) == 9 {
            return crate::production_port::begin_transition(0x20030000, event) as u32;
        }
        0
    }
    pub unsafe fn destroyed(_: u32) {}
}

mod fw_api {
    pub use crate::addresses as fwaddr;
    // Thin ABI shims to the SAME native helpers used by fw_api/gfx.rs. The
    // position/geometry algorithm is imported above unchanged from production.
    pub const LV_OBJ_FLAG_SCROLLABLE: u32 = 0x10;
    pub const LV_OBJ_FLAG_SCROLL_CHAIN: u32 = 0x300;
    pub const LV_OBJ_FLAG_GESTURE_BUBBLE: u32 = 0x8000;
    pub const LV_OBJ_FLAG_CLICKABLE: u32 = 2;
    pub mod honeycomb {
        pub unsafe fn refresh_native(grid: u32) -> bool {
            crate::production_port::refresh_native(grid)
        }
    }
    pub mod oflag {
        pub const RDONLY: u32 = 1;
        pub const WRONLY: u32 = 2;
        pub const CREAT: u32 = 0x40;
    }
    pub unsafe fn open(_: *const u8, _: u32, _: u32) -> i32 {
        -1
    }
    pub unsafe fn read(_: i32, _: *mut u8, _: u32) -> i32 {
        -1
    }
    pub unsafe fn write(_: i32, _: *const u8, _: u32) -> i32 {
        -1
    }
    pub unsafe fn close(_: i32) -> i32 {
        0
    }
    pub unsafe fn obj_child_count(o: u32) -> i32 {
        let f: unsafe extern "C" fn(u32) -> i32 = core::mem::transmute(0x0C5881D9usize);
        f(o)
    }
    pub unsafe fn obj_get_child(o: u32, i: u32) -> u32 {
        let f: unsafe extern "C" fn(u32, u32) -> u32 = core::mem::transmute(0x0C5886E1usize);
        f(o, i)
    }
    pub unsafe fn obj_is_child_of(p: u32, o: u32) -> bool {
        let n = obj_child_count(p);
        if n < 0 || n > 24 {
            return false;
        }
        (0..n as u32).any(|i| obj_get_child(p, i) == o)
    }
    pub unsafe fn obj_add_flag(o: u32, flags: u32) {
        let f: unsafe extern "C" fn(u32, u32) = core::mem::transmute(0x1C028BD9usize);
        f(o, flags)
    }
    pub unsafe fn obj_clear_flag(o: u32, flags: u32) {
        let f: unsafe extern "C" fn(u32, u32) = core::mem::transmute(0x1C028C85usize);
        f(o, flags)
    }
    pub unsafe fn obj_add_event(o: u32, cb: u32, filter: u32, data: u32) -> u32 {
        let f: unsafe extern "C" fn(u32, u32, u32, u32) -> u32 =
            core::mem::transmute(0x0C5883F1usize);
        f(o, cb, filter, data)
    }
    pub unsafe fn obj_invalidate(o: u32) {
        let f: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(0x0C10CBB5usize);
        f(o);
    }
    pub unsafe fn obj_delete(o: u32) {
        let f: unsafe extern "C" fn(u32) = core::mem::transmute(0x0C588DE9usize);
        f(o)
    }
    pub unsafe fn event_get_code(e: u32) -> u32 {
        let f: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(0x0C588211usize);
        f(e)
    }
    pub fn screen_is_on() -> bool {
        true
    } // not used by the geometry probe
    pub unsafe fn timer_create(_: u32, _: u32, _: u32) -> u32 {
        0
    }
    pub unsafe fn timer_period_of(_: u32) -> u32 {
        0
    }
    pub unsafe fn timer_set_period(_: u32, _: u32) {}
}

#[no_mangle]
pub unsafe extern "C" fn test_render(grid: u32, index: u32, pan_x: i32, pan_y: i32) -> u32 {
    let icon = fw_api::obj_get_child(grid, index);
    let f = honeycomb_layout::frame(
        index as usize,
        honeycomb_layout::Point { x: pan_x, y: pan_y },
    );
    crate::production_port::begin_frame(grid, &[icon]);
    if !crate::production_port::place(icon, f) {
        return 0;
    }
    crate::production_port::present(grid);
    crate::production_port::geometry_matches(icon, f) as u32
}

#[no_mangle]
pub unsafe extern "C" fn test_frame(grid: u32) -> u32 {
    // Simulate an ordinary LVGL layout pass after a frame has already committed.
    crate::production_port::commit_rects(grid);
    1
}

#[no_mangle]
pub unsafe extern "C" fn test_current_frame(grid: u32, index: u32, output: *mut i32) -> u32 {
    let icon = fw_api::obj_get_child(grid, index);
    let Some(frame) = crate::production_port::current_frame(icon) else {
        return 0;
    };
    core::ptr::write(
        output.cast::<[i32; 4]>(),
        [frame.x, frame.y, frame.diameter, frame.visible as i32],
    );
    1
}

#[no_mangle]
pub unsafe extern "C" fn test_apply_current_frame(grid: u32, index: u32) -> u32 {
    let icon = fw_api::obj_get_child(grid, index);
    let Some(frame) = crate::production_port::current_frame(icon) else {
        return 0;
    };
    crate::production_port::begin_frame(grid, &[icon]);
    if !crate::production_port::place(icon, frame) {
        return 0;
    }
    crate::production_port::present(grid);
    crate::production_port::geometry_matches(icon, frame) as u32
}

#[no_mangle]
pub unsafe extern "C" fn test_stop_return(grid: u32) -> u32 {
    let n = fw_api::obj_child_count(grid);
    if n < 0 || n as usize > honeycomb_layout::MAX_ICONS {
        return 0;
    }
    let mut icons = [0; honeycomb_layout::MAX_ICONS];
    for i in 0..n as usize {
        icons[i] = fw_api::obj_get_child(grid, i as u32);
    }
    crate::production_port::stop_return_animations(grid, &icons[..n as usize]);
    1
}

#[no_mangle]
pub unsafe extern "C" fn test_zoom(grid: u32, index: u32, zoom: i32) -> u32 {
    test_spacing(grid, index, zoom, 256)
}

#[no_mangle]
pub unsafe extern "C" fn test_spacing(grid: u32, index: u32, zoom: i32, spacing: i32) -> u32 {
    let icon = fw_api::obj_get_child(grid, index);
    let f = honeycomb_layout::frame_spaced(
        index as usize,
        honeycomb_layout::Point { x: 0, y: 0 },
        zoom,
        spacing,
    );
    if !crate::production_port::place(icon, f) {
        return 0;
    }
    crate::production_port::present(grid);
    crate::production_port::geometry_matches(icon, f) as u32
}

#[no_mangle]
pub unsafe extern "C" fn test_navigation(grid: u32, event: u32, chain: u32) {
    crate::production_port::navigation(grid, event, chain);
}

#[no_mangle]
pub unsafe extern "C" fn test_motion_parent(grid: u32) -> u32 {
    crate::production_port::motion_parent(grid)
}

#[no_mangle]
pub unsafe extern "C" fn test_entry_source(grid: u32, event: u32) -> u32 {
    crate::production_port::entry_source(grid, event) as u32
}

#[no_mangle]
pub unsafe extern "C" fn test_watch_entry(grid: u32) -> u32 {
    crate::production_port::watch_entry(grid) as u32
}

#[no_mangle]
pub unsafe extern "C" fn test_stop_watch() {
    crate::production_port::stop_watch();
}

#[no_mangle]
pub unsafe extern "C" fn test_entry_dispatch(event: u32, on: u32) {
    honeycomb::set_enabled(on != 0);
    crate::production_port::entry_dispatch(event);
}

#[no_mangle]
pub unsafe extern "C" fn test_clear_snapshot(grid: u32) -> u32 {
    crate::production_port::clear_snapshot(grid) as u32
}

#[no_mangle]
pub unsafe extern "C" fn test_layout(grid: u32, index: u32, pan_x: i32, pan_y: i32) -> u32 {
    let icon = fw_api::obj_get_child(grid, index);
    let f = honeycomb_layout::frame(
        index as usize,
        honeycomb_layout::Point { x: pan_x, y: pan_y },
    );
    if !crate::production_port::layout_rect(icon, f) {
        return 0;
    }
    crate::production_port::commit_rects(grid);
    crate::production_port::geometry_matches(icon, f) as u32
}

#[no_mangle]
pub unsafe extern "C" fn test_bad_layout(grid: u32, _: u32) {
    // Minimal v1.1.8 pattern: a seemingly idempotent flag setter inside layout.
    let icon = fw_api::obj_get_child(grid, 0);
    fw_api::obj_clear_flag(icon, 1);
}

#[no_mangle]
pub unsafe extern "C" fn test_layout_cycle(grid: u32, _: u32) {
    let icon = fw_api::obj_get_child(grid, 0);
    let f = honeycomb_layout::frame(0, honeycomb_layout::Point { x: 0, y: 0 });
    crate::production_port::layout_rect(icon, f);
    crate::production_port::commit_rects(grid);
}

#[no_mangle]
pub unsafe extern "C" fn test_rw_tick(honeycomb_on: u32) -> u32 {
    honeycomb::set_enabled(honeycomb_on != 0);
    desktop_scale::tick();
    1
}

#[no_mangle]
pub unsafe extern "C" fn test_rw_apply() {
    honeycomb::set_enabled(false);
    desktop_scale::disable_for_honeycomb();
    desktop_scale::set_lens(true);
    desktop_scale::reapply_now();
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
