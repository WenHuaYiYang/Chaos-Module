//! 在原生HIDE/DESTROY之前保存画面，返回桌面后播放原生关闭过渡。
use crate::{fw_api as fw,mem::{rd32,safe_ptr}};
use core::mem::transmute;
static mut APP:u32=0;
static mut COVER:u32=0;
static mut TOP:u32=0;
static mut IMAGE:u32=0;
static mut SNAPSHOT:u32=0;
static mut RETIRED:[u32;2]=[0;2];
static mut CONTEXT:[u32;14]=[0;14];
static mut REQUESTED:u32=0;
static mut START:Option<u32>=None;
static mut DRAWN:bool=false;
static mut BUSY:bool=false;
unsafe fn word(p:u32,o:u32)->u32{rd32((p+o)as *const u32)}
pub(crate) unsafe fn active()->bool{APP!=0||RETIRED.iter().any(|&p|p!=0)}
unsafe fn collect(){
    if !RETIRED.iter().any(|&p|p!=0)||!fw::s541_frame_idle(){return;}
    let free:unsafe extern "C" fn(u32)=transmute(0x0cac0db9usize);
    for p in &mut RETIRED {
        if *p!=0 {let old=*p;*p=0;fw::fw_img_free_by_path(old);free(old);}
    }
}
pub(crate) unsafe fn cancel(){
    let cover=COVER;COVER=0;APP=0;START=None;DRAWN=false;IMAGE=0;
    if cover!=0&&safe_ptr(TOP)&&fw::obj_is_child_of(TOP,cover){fw::obj_delete(cover);}
    TOP=0;
    if SNAPSHOT!=0 {
        // snapshot()分配前要求至少一个空退役槽，只由本组件使用。
        if let Some(slot)=RETIRED.iter_mut().find(|p|**p==0){*slot=SNAPSHOT;SNAPSHOT=0;}
    }
}
unsafe fn snapshot(root:u32)->u32{
    collect();
    if !RETIRED.contains(&0)||!fw::s541_frame_idle(){return 0;}
    let alloc:unsafe extern "C" fn(u32,u32,u32,u32)->u32=transmute(0x0c587f39usize);
    let descriptor=alloc(336,480,15,0);
    if descriptor==0{return 0;}
    let capture:unsafe extern "C" fn(u32,u32,u32)->i32=transmute(0x0cac0879usize);
    let result=capture(root,15,descriptor);
    if result!=1||!fw::s541_frame_idle(){
        if let Some(slot)=RETIRED.iter_mut().find(|p|**p==0){*slot=descriptor;}
        return 0;
    }
    descriptor
}
pub(crate) unsafe fn capture(app:u32,root:u32,origin:*const u32){
    if BUSY||APP==app||!fw::screen_is_on()||crate::quickapp_splash::owns(app)
        ||!safe_ptr(root)||word(root,0x2c)&1!=0
        ||(!crate::quickapp_splash::front(root,app)&&fw::top_app_id()!=fw::HOME_APP_ID){return;}
    BUSY=true;cancel();
    let shot=snapshot(root);
    let init:unsafe extern "C" fn(*mut u32)=transmute(0x0ca45609usize);
    init(core::ptr::addr_of_mut!(CONTEXT).cast());
    CONTEXT[9]=*origin.add(9);CONTEXT[10]=*origin.add(10);
    CONTEXT[11]=200;CONTEXT[12]=0;
    APP=app;SNAPSHOT=shot;REQUESTED=fw::capture_now_ms();BUSY=false;
}
unsafe extern "C" fn event(e:u32){
    if word(e,0)!=COVER{return;}
    match fw::event_get_code(e){25=>DRAWN=true,36=>{COVER=0;},_=>{}}
}
unsafe fn create()->bool{
    let display=fw::display_default();if !safe_ptr(display){return false;}
    let top:unsafe extern "C" fn(u32)->u32=transmute(0x0cac0219usize);
    TOP=top(display);if !safe_ptr(TOP){return false;}
    let mut source=SNAPSHOT;
    if source==0 {
        let lookup:unsafe extern "C" fn(u32)->u32=transmute(0x0ca69935usize);
        let node=lookup(APP);if !safe_ptr(node){return false;}
        source=word(node,0xc);if !crate::mem::plausible_ptr(source){return false;}
    }
    COVER=fw::obj_create(TOP,0);if COVER==0{return false;}
    fw::obj_clear_box_spacing(COVER);fw::obj_clear_scrollable(COVER);
    fw::obj_add_flag(COVER,0x40000);
    fw::obj_clear_flag(COVER,fw::LV_OBJ_FLAG_CLICKABLE);
    fw::obj_set_size_wh(COVER,336,480);fw::obj_align(COVER,fw::align::TOP_LEFT,0,0);
    fw::obj_set_local_style_prop(COVER,0x1c,0,0);fw::background_set_opa(COVER,255);
    fw::obj_add_event(COVER,event as *const ()as u32,0,0);
    IMAGE=fw::background_image_create(COVER);if IMAGE==0{return false;}
    fw::background_image_set_source(IMAGE,source);fw::obj_clear_flag(IMAGE,fw::LV_OBJ_FLAG_CLICKABLE);
    fw::obj_align(IMAGE,if SNAPSHOT!=0{fw::align::TOP_LEFT}else{fw::align::CENTER},0,0);
    // 原生OUT：默认alpha 255->0、scale 256->204，200ms，与原生应用配方一致。
    let animate:unsafe extern "C" fn(u32,u32,*const u32)=transmute(0x0ca462d9usize);
    animate(COVER,0,core::ptr::addr_of!(CONTEXT).cast());
    START=Some(fw::capture_now_ms());true
}
pub(crate) unsafe fn tick(on:bool){
    collect();
    if APP==0{return;}
    if !on||!fw::screen_is_on(){cancel();return;}
    if let Some(start)=START {
        if fw::top_app_id()!=fw::HOME_APP_ID||COVER==0
            ||!fw::obj_is_child_of(TOP,COVER)
            ||(DRAWN&&fw::capture_now_ms().wrapping_sub(start)>=240)
            ||fw::capture_now_ms().wrapping_sub(start)>=1500 {cancel();}
    }else if fw::top_app_id()==fw::HOME_APP_ID {
        if !create(){cancel();}
    }else if fw::capture_now_ms().wrapping_sub(REQUESTED)>=1500 {cancel();}
}
