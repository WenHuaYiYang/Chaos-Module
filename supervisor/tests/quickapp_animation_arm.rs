#![no_std]
#![no_main]
#![allow(dead_code,static_mut_refs)]
#[path="../src/mem.rs"] mod mem;
#[path="../src/quickapp_animation.rs"] mod quickapp_animation;
#[path="../src/quickapp_splash.rs"] mod quickapp_splash;
#[path="../src/quickapp_exit.rs"] mod quickapp_exit;
mod fw_api {
    pub const HOME_APP_ID:u32=16;
    pub const DESC_ROOT:u32=0x30;
    pub const LV_OBJ_FLAG_CLICKABLE:u32=2;
    pub mod align{pub const TOP_LEFT:u32=0;pub const CENTER:u32=9;}
    pub const APP_REGISTRY:u32=0x200eb640;
    pub mod oflag{pub const RDONLY:u32=1;pub const WRONLY:u32=2;pub const CREAT:u32=0x100;}
    pub unsafe fn open(_: *const u8,_:u32,_:u32)->i32{-1}
    pub unsafe fn read(_:i32,_:*mut u8,_:u32)->i32{0}
    pub unsafe fn write(_:i32,_:*const u8,_:u32)->i32{1}
    pub unsafe fn close(_:i32){}
    pub unsafe fn screen_is_on()->bool{true}
    pub unsafe fn top_app_id()->u32{core::ptr::read_volatile(0x20050000 as *const u32)}
    pub unsafe fn top_page_desc()->u32{core::ptr::read_volatile(0x20050008 as *const u32)}
    pub unsafe fn display_default()->u32{0x20046000}
    pub unsafe fn capture_now_ms()->u32{core::ptr::read_volatile(0x20050004 as *const u32)}
    pub unsafe fn obj_create(parent:u32,_:u32)->u32{
        let f:unsafe extern "C" fn(u32)->u32=core::mem::transmute(0x0dfe0001usize);f(parent)
    }
    pub unsafe fn background_image_create(p:u32)->u32{obj_create(p,0)}
    pub unsafe fn obj_is_child_of(p:u32,o:u32)->bool{core::ptr::read_volatile((o+4)as *const u32)==p}
    pub unsafe fn obj_delete(o:u32){core::ptr::write_volatile((o+4)as *mut u32,0);}
    pub unsafe fn obj_clear_box_spacing(_:u32){}
    pub unsafe fn obj_clear_scrollable(_:u32){}
    pub unsafe fn obj_set_size_wh(_:u32,_:u32,_:u32){}
    pub unsafe fn obj_align(_:u32,_:u32,_:i32,_:i32){}
    pub unsafe fn obj_add_flag(o:u32,f:u32){let p=(o+0x2c)as *mut u32;core::ptr::write_volatile(p,core::ptr::read_volatile(p)|f);}
    pub unsafe fn obj_clear_flag(o:u32,f:u32){let p=(o+0x2c)as *mut u32;core::ptr::write_volatile(p,core::ptr::read_volatile(p)&!f);}
    pub unsafe fn obj_set_local_style_prop(o:u32,p:u32,v:u32,_:u32){if p==95{core::ptr::write_volatile((o+0x14)as *mut u32,v);}}
    pub unsafe fn background_set_opa(_:u32,_:u32){}
    pub unsafe fn obj_add_event(_:u32,callback:u32,_:u32,_:u32)->u32{core::ptr::write_volatile(0x20050010 as *mut u32,callback);1}
    pub unsafe fn event_get_code(e:u32)->u32{core::ptr::read_volatile((e+8)as *const u32)}
    pub unsafe fn background_image_set_source(_:u32,_:u32){}
    pub unsafe fn obj_child_count(o:u32)->i32{core::ptr::read_volatile((o+0x10)as *const i32)}
    pub unsafe fn obj_invalidate(_:u32){}
    pub unsafe fn s541_frame_idle()->bool{core::ptr::read_volatile(0x2005001c as *const u32)!=0}
    pub unsafe fn fw_img_free_by_path(p:u32){let f:unsafe extern "C" fn(u32)=core::mem::transmute(0x0c5890d1usize);f(p);}
}
#[no_mangle]pub unsafe extern "C" fn test_toggle(){quickapp_animation::toggle();}
#[no_mangle]pub unsafe extern "C" fn test_tick(){quickapp_animation::tick();}
#[no_mangle]pub unsafe extern "C" fn test_close_toggle(){quickapp_animation::toggle_close();}
#[panic_handler]fn panic(_: &core::panic::PanicInfo<'_>)->!{loop{core::hint::spin_loop();}}
