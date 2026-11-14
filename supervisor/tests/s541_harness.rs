#![allow(dead_code, static_mut_refs)]
use std::cell::RefCell;
use std::collections::HashMap;
#[derive(Default)]
struct State { next: u32, objects: HashMap<u32, u32>, flags: HashMap<u32, u32>,
    events: Vec<(u32,u32,u32)>, labels: Vec<Vec<u8>>, file: Vec<u8>, fail: bool, launches: u32, event_code: u32,
    preference: u32, selections: Vec<u32>, honeycomb_writes: u32, desktop_writes: u32 }
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
fn state<T>(f: impl FnOnce(&mut State)->T)->T { STATE.with(|s| f(&mut s.borrow_mut())) }
const FW_STYLE_HR_NAME: u32 = 1;
#[path = "../src/honeycomb_layout.rs"] mod honeycomb_layout;
#[path = "../src/s541_launcher.rs"] mod s541_launcher;
#[path = "../src/s541_pixels.rs"] mod s541_pixels;
mod honeycomb {
    pub unsafe fn set_enabled(_: bool) {crate::state(|s|s.honeycomb_writes+=1);} pub unsafe fn enabled()->bool { false }
    pub unsafe fn s541_changed(_:bool) {} pub unsafe fn s541_ui_tick() {}
}
mod desktop_scale { pub unsafe fn disable_for_honeycomb() {crate::state(|s|s.desktop_writes+=1);} }
mod corona_blur { pub unsafe fn backgrounds_idle()->bool{true} }
mod log { pub fn line(_: &str) {} }
mod mem {
    pub fn safe_ptr(p: u32)->bool { p != 0 }
    pub unsafe fn rd32(p: *const u32)->u32 {
        let p=p as u32;
        if p>=0x10000 { return if p&0xff==0x44 {112} else {0x1234}; }
        crate::state(|s| *s.flags.get(&p.saturating_sub(0x2c)).unwrap_or(&0))
    }
}
mod fw_api {
    pub const LV_OBJ_FLAG_SCROLLABLE:u32=16; pub const LV_OBJ_FLAG_SCROLL_ELASTIC:u32=32;
    pub const LV_OBJ_FLAG_CLICKABLE:u32=2; pub const LV_OBJ_FLAG_SCROLL_CHAIN:u32=0x300;
    pub const LV_OBJ_FLAG_GESTURE_BUBBLE:u32=0x8000;
    pub const HOME_APP_ID:u32=16;
    pub mod align { pub const TOP_LEFT:u32=0; }
    pub mod oflag { pub const RDONLY:u32=0;pub const WRONLY:u32=1;pub const CREAT:u32=2; }
    pub unsafe fn open(_: *const u8, f:u32,_:u32)->i32 {
        crate::state(|s| if f==0 && s.file.is_empty(){-1}else{3})
    }
    pub unsafe fn read(_:i32,p:*mut u8,_:u32)->i32 {crate::state(|s|{
        std::ptr::copy_nonoverlapping(s.file.as_ptr(),p,s.file.len()); s.file.len() as i32})}
    pub unsafe fn write(_:i32,p:*const u8,n:u32)->i32 {
        crate::state(|s|s.file=std::slice::from_raw_parts(p,n as usize).to_vec());n as i32}
    pub unsafe fn close(_:i32){}
    pub unsafe fn capture_mkdir_path(_: *const u8)->i32{0}
    pub unsafe fn obj_create(parent:u32,_:u32)->u32 {crate::state(|s|{
        if s.fail{return 0;}s.next+=1;let id=1000+s.next;s.objects.insert(id,parent);id})}
    pub unsafe fn obj_is_child_of(parent:u32,o:u32)->bool {crate::state(|s|
        s.objects.values().filter(|&&p|p==parent).count()<=24 && s.objects.get(&o)==Some(&parent))}
    pub unsafe fn obj_delete(o:u32){crate::state(|s|{s.objects.remove(&o);});}
    pub unsafe fn obj_add_flag(o:u32,f:u32){crate::state(|s|{*s.flags.entry(o).or_default()|=f;});}
    pub unsafe fn obj_clear_flag(o:u32,f:u32){crate::state(|s|{*s.flags.entry(o).or_default()&=!f;});}
    pub unsafe fn obj_clear_box_spacing(_:u32){}
    pub unsafe fn background_set_opa(_:u32,_:u32){}
    pub unsafe fn obj_clear_scrollable(_:u32){}
    pub unsafe fn obj_set_size_wh(_:u32,_:u32,_:u32){}
    pub unsafe fn obj_align(_:u32,_:u32,_:i32,_:i32){}
    pub unsafe fn obj_set_scroll_dir(_:u32,_:u32){}
    pub unsafe fn obj_add_event(o:u32,_:u32,c:u32,d:u32)->u32{crate::state(|s|s.events.push((o,c,d)));1}
    pub unsafe fn event_get_code(_:u32)->u32{crate::state(|s|if s.event_code==0{4}else{s.event_code})}
    pub unsafe fn event_user_data(e:u32)->u32{e}
    pub unsafe fn background_image_create(p:u32)->u32{obj_create(p,0)}
    pub unsafe fn background_image_set_source(_:u32,_:u32){}
    pub unsafe fn background_image_set_pivot(_:u32,_:i32,_:i32){}
    pub unsafe fn image_set_scale(_:u32,_:i32,_:i32){}
    pub unsafe fn label_create(p:u32)->u32{obj_create(p,0)}
    pub unsafe fn style_apply(_:u32,_:u32,_:u32,_:u32){}
    pub unsafe fn fw_style_get_prop(_:u32,_:u32,_:u32)->u32{255}
    pub unsafe fn obj_set_local_style_prop(_:u32,_:u32,_:u32,_:u32){}
    pub unsafe fn label_set_text(_:u32,p:*const u8){crate::state(|s|s.labels.push(std::ffi::CStr::from_ptr(p.cast()).to_bytes().to_vec()));}
    pub unsafe fn screen_is_on()->bool{true}
    pub unsafe fn top_app_id()->u32{HOME_APP_ID}
    pub unsafe fn s541_capture_region(out:&mut[u8;5184],_:u32,_:u32)->Result<(),i32>{out.fill(120);Ok(())}
    pub unsafe fn s541_clean_pixels(_: *const u8,_:u32){}
    pub unsafe fn s541_frame_idle()->bool{true}
    pub unsafe fn fw_img_free_by_path(_:u32){}
    pub mod honeycomb {
        pub unsafe fn s541_preference()->u32{crate::state(|s|s.preference)}
        pub unsafe fn s541_select(p:u32)->bool{crate::state(|s|{s.preference=p;s.selections.push(p);});true}
        pub unsafe fn s541_anchor(_:u32)->Option<(u32,i32,i32)>{Some((30,24,360))}
        pub unsafe fn current_target(e:u32)->u32{e}
        pub unsafe fn launcher()->u32{20}
        pub unsafe fn parent(_:u32)->u32{10}
        pub unsafe fn s541_button(_:u32)->u32{30}
        pub unsafe fn supported(_:u32)->bool{true}
        pub unsafe fn visible(_:u32)->bool{true}
        pub unsafe fn icons(_:u32,o:&mut [u32;crate::honeycomb_layout::MAX_ICONS])->usize{o[..3].copy_from_slice(&[0x10000,0x10100,0x10200]);3}
        pub unsafe fn s541_icon(i:u32)->Option<(u32,u32,*const u8)>{Some((i,0x1234,"应用\0".as_ptr()))}
        pub unsafe fn launch(_:u32){crate::state(|s|s.launches+=1);}
    }
}
