//! Compile the unmodified production GIF image port for the ARM probe.
#![no_std]
#![no_main]
#![allow(dead_code, static_mut_refs)]
#[path = "../src/fw_api/fwaddr.rs"] pub mod addresses;
#[path = "../src/fw_api/dynamic_icon.rs"] pub mod production_port;
#[path = "../src/dynamic_icon_frames.rs"] mod dynamic_icon_frames;
#[path = "../src/mem.rs"] mod mem;
pub use fw_api::*;
mod honeycomb_layout { pub const MAX_ICONS: usize = 96; }
mod state {
    pub const FW_INIT_BUFFER: u32 = 0x0C513A45;
    pub static ICON_PATH: [u8; 27] = *b"/data/chaos/chaos_icon.bin\0";
}
mod dynamic_icon {
    #[derive(Clone, Copy)]
    pub struct Icon { pub object: u32, pub source: u32, pub original: bool, pub visible: bool }
    pub unsafe extern "C" fn animation_tick(_: u32) {}
}
mod fw_api {
    pub use crate::addresses as fwaddr;
    pub const HOME_APP_ID: u32 = 16;
    pub unsafe fn top_app_id() -> u32 { 16 }
    pub unsafe fn watchface_is_on() -> u32 { 0 }
    pub unsafe fn obj_child_count(_: u32) -> i32 { 0 }
    pub unsafe fn obj_get_child(_: u32, _: u32) -> u32 { 0 }
    pub mod honeycomb {
        pub unsafe fn launcher() -> u32 { 0 }
        pub unsafe fn supported(_: u32) -> bool { false }
        pub unsafe fn visible(_: u32) -> bool { false }
    }
    // Same ABI shims as fw_api/glass.rs and sys.rs. Python intercepts display
    // plumbing and cache eviction; the firmware source setter executes.
    pub unsafe fn background_image_set_source(object: u32, source: u32) {
        let f: unsafe extern "C" fn(u32, u32) = core::mem::transmute(0x0C587FA1usize);
        f(object, source);
    }
    pub unsafe fn fw_img_free_by_path(source: u32) {
        let f: unsafe extern "C" fn(u32) = core::mem::transmute(0x0C5890D1usize);
        f(source);
    }
    pub unsafe fn timer_create(callback: u32, period: u32, data: u32) -> u32 {
        let f: unsafe extern "C" fn(u32, u32, u32) -> u32 = core::mem::transmute(fwaddr::LV_TIMER_CREATE as usize);
        f(callback, period, data)
    }
    pub unsafe fn timer_period_of(timer: u32) -> u32 { core::ptr::read_volatile(timer as *const u32) }
    pub unsafe fn timer_set_period(timer: u32, period: u32) {
        let f: unsafe extern "C" fn(u32, u32) = core::mem::transmute(0x0C16D545usize); f(timer, period);
    }
}
#[no_mangle]
pub unsafe extern "C" fn test_frame(object: u32, source: u32, index: u32) -> u32 {
    let icon = dynamic_icon::Icon { object, source, original: true, visible: true };
    let animation = dynamic_icon_frames::Animation::parse(dynamic_icon_frames::PACKAGED).unwrap();
    production_port::show_frame(icon, &animation, index as usize).unwrap_or(0)
}
#[no_mangle]
pub unsafe extern "C" fn test_original(object: u32, source: u32) -> u32 {
    let icon = dynamic_icon::Icon { object, source, original: false, visible: true };
    production_port::restore_original(icon); 1
}
#[no_mangle]
pub unsafe extern "C" fn test_retire() -> u32 { production_port::retire_frames(); 1 }
#[no_mangle]
pub unsafe extern "C" fn test_timer(active: u32, period: u32) -> u32 {
    production_port::animation_active(active != 0, period) as u32
}
#[no_mangle]
pub unsafe extern "C" fn test_pack_timer(active: u32, period: u32) -> u32 {
    production_port::pack_animation_active(active != 0, period) as u32
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! { loop { core::hint::spin_loop(); } }
