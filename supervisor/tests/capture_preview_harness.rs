#![allow(dead_code,static_mut_refs)]
use std::cell::RefCell;
#[derive(Default)]struct World {foreground:u32,next:u32,awake:bool,fence:bool,free:Vec<u32>,sources:Vec<(u32,u32)>,spawns:u32,statuses:Vec<String>}
thread_local!{static WORLD:RefCell<World>=RefCell::new(World::default());}
fn world<T>(f:impl FnOnce(&mut World)->T)->T{WORLD.with(|w|f(&mut w.borrow_mut()))}
#[macro_export]macro_rules! st_rd{($v:expr)=>{crate::world(|w|w.foreground)};}
#[path="../src/capture_decode.rs"]mod capture_decode;
#[path="../src/capture_preview.rs"]mod capture_preview;
mod fw_api {
 pub mod oflag{pub const RDONLY:u32=1;}pub mod seek{pub const SET:u32=0;pub const END:u32=2;}pub mod align{pub const TOP_LEFT:u32=1;}
 pub unsafe fn heap_malloc(_:u32)->u32{0}pub unsafe fn fw_free(p:u32){super::world(|w|w.free.push(p));}
 pub unsafe fn capture_result(v:i32)->i32{v}pub unsafe fn open(_: *const u8,_:u32,_:u32)->i32{-2}
 pub unsafe fn close(_:i32)->i32{0}pub unsafe fn lseek(_:i32,_:i64,_:u32)->i64{-2}
 pub unsafe fn nsh_read(_:i32,_:&mut[u8])->i32{-2}
 pub unsafe fn nsh_spawn(_: *const u8,_:unsafe extern "C" fn(i32,*mut *mut u8)->i32,_:*mut usize)->i32{super::world(|w|{w.spawns+=1;42})}
 pub unsafe fn obj_add_event(_:u32,_:u32,_:u32,_:u32)->u32{1}
 pub unsafe fn event_get_code(_:u32)->u32{36}pub unsafe fn event_user_data(tag:u32)->u32{tag}
 pub unsafe fn row_update(_:u32,_:*const u8,_:*const u8,p:*const u8,_:i32,_:u8)->i32{super::world(|w|w.statuses.push(std::ffi::CStr::from_ptr(p.cast()).to_str().unwrap().into()));0}
 pub unsafe fn s541_frame_idle()->bool{super::world(|w|w.fence)}pub unsafe fn screen_is_on()->bool{super::world(|w|w.awake)}
 pub unsafe fn fw_img_free_by_path(_:u32){}pub unsafe fn s541_clean_pixels(_:*const u8,_:u32){}
 pub unsafe fn background_image_set_source(o:u32,p:u32){super::world(|w|w.sources.push((o,p)));}
 pub unsafe fn background_image_set_scale(_:u32,_:u32,_:u32){}pub unsafe fn background_image_set_pivot(_:u32,_:i32,_:i32){}
 pub unsafe fn obj_align(_:u32,_:u32,_:i32,_:i32){}
}
