#![no_std]
#![no_main]
#![allow(dead_code,static_mut_refs)]
#[path="../src/mem.rs"] mod mem;
#[path="../src/quickapp_animation.rs"] mod quickapp_animation;
mod fw_api {
    pub const APP_REGISTRY:u32=0x200eb640;
    pub mod oflag{pub const RDONLY:u32=1;pub const WRONLY:u32=2;pub const CREAT:u32=0x100;}
    pub unsafe fn open(_: *const u8,_:u32,_:u32)->i32{-1}
    pub unsafe fn read(_:i32,_:*mut u8,_:u32)->i32{0}
    pub unsafe fn write(_:i32,_:*const u8,_:u32)->i32{1}
    pub unsafe fn close(_:i32){}
    pub unsafe fn screen_is_on()->bool{true}
    pub unsafe fn top_app_id()->u32{core::ptr::read_volatile(0x20050000 as *const u32)}
}
#[no_mangle]pub unsafe extern "C" fn test_toggle(){quickapp_animation::toggle();}
#[no_mangle]pub unsafe extern "C" fn test_tick(){quickapp_animation::tick();}
#[panic_handler]fn panic(_: &core::panic::PanicInfo<'_>)->!{loop{core::hint::spin_loop();}}
