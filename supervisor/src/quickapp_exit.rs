//! 在原生HIDE/DESTROY之前接住画面，返回桌面后无缝播放原生关闭过渡。
use crate::{fw_api as fw,mem::{rd32,safe_ptr}};
use core::mem::transmute;
static mut APP:u32=0;
static mut COVER:u32=0;
static mut TOP:u32=0;
static mut SNAPSHOT:u32=0;
static mut RETIRED:[u32;2]=[0;2];
static mut CONTEXT:[u32;14]=[0;14];
static mut REQUESTED:u32=0;
static mut START:Option<u32>=None;
static mut DONE:bool=false;
static mut OWNER:u32=0;
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
    let cover=COVER;COVER=0;APP=0;START=None;DONE=false;OWNER=0;
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
    // 图标不能代替当前画面：整页切成黑底图标本身就会产生一次闪屏。
    // 捕获不可用时直接保留系统退出流程，不插入不同内容的遮罩。
    if shot==0{BUSY=false;return;}
    let init:unsafe extern "C" fn(*mut u32)=transmute(0x0ca45609usize);
    init(core::ptr::addr_of_mut!(CONTEXT).cast());
    CONTEXT[9]=*origin.add(9);CONTEXT[10]=*origin.add(10);
    CONTEXT[11]=200;CONTEXT[12]=0;
    APP=app;SNAPSHOT=shot;OWNER=fw::top_app_id();REQUESTED=fw::capture_now_ms();
    // 必须在原HIDE/DESTROY之前建立全屏遮罩，不能等200ms维护拍再盖回桌面。
    if !create(){cancel();}
    BUSY=false;
}
unsafe extern "C" fn event(e:u32){
    if word(e,0)!=COVER{return;}
    if fw::event_get_code(e)==36{COVER=0;}
}
unsafe extern "C" fn completed(animation:u32){
    // 0x0CA45C20在两个OUT分量均完成、删除原生过渡图像后回传lv_anim_t*。
    // 此处只登记，交给UI维护销毁源对象，避免干扰原生完成回调的栈和共享数据。
    if safe_ptr(animation)&&word(animation,0)==COVER{DONE=true;}
}
unsafe fn create()->bool{
    let display=fw::display_default();if !safe_ptr(display){return false;}
    let top:unsafe extern "C" fn(u32)->u32=transmute(0x0cac0219usize);
    TOP=top(display);if !safe_ptr(TOP){return false;}
    COVER=fw::obj_create(TOP,0);if COVER==0{return false;}
    fw::obj_clear_box_spacing(COVER);fw::obj_clear_scrollable(COVER);
    fw::obj_add_flag(COVER,0x40000);
    fw::obj_clear_flag(COVER,fw::LV_OBJ_FLAG_CLICKABLE);
    fw::obj_set_size_wh(COVER,336,480);fw::obj_align(COVER,fw::align::TOP_LEFT,0,0);
    fw::obj_set_local_style_prop(COVER,0x1c,0,0);fw::background_set_opa(COVER,255);
    fw::obj_add_event(COVER,event as *const ()as u32,0,0);
    let image=fw::background_image_create(COVER);if image==0{return false;}
    fw::background_image_set_source(image,SNAPSHOT);fw::obj_clear_flag(image,fw::LV_OBJ_FLAG_CLICKABLE);
    fw::obj_align(image,fw::align::TOP_LEFT,0,0);
    true
}
/// 原关闭回调返回时立即尝试交接；HOME尚未成为前台时保留完整画面等待维护。
pub(crate) unsafe fn after_close(){
    if APP==0{return;}
    let top=fw::top_app_id();
    if !fw::screen_is_on()||COVER==0||!safe_ptr(TOP)||!fw::obj_is_child_of(TOP,COVER){cancel();return;}
    if let Some(start)=START {
        if top!=fw::HOME_APP_ID||DONE||fw::capture_now_ms().wrapping_sub(start)>=1500{cancel();}
        return;
    }
    if top!=fw::HOME_APP_ID{
        if top!=OWNER||fw::capture_now_ms().wrapping_sub(REQUESTED)>=1500{cancel();}
        return;
    }
    // 原生OUT：默认alpha 255->0、scale 256->204，200ms，与原生应用配方一致。
    // 原生驱动先创建过渡图像再隐藏源cover，完成后cover继续隐藏，绝不重新显示。
    START=Some(fw::capture_now_ms());DONE=false;
    let animate:unsafe extern "C" fn(u32,u32,*const u32)=transmute(0x0ca462d9usize);
    animate(COVER,completed as *const ()as u32,core::ptr::addr_of!(CONTEXT).cast());
}
pub(crate) unsafe fn tick(on:bool){
    collect();
    if APP==0{return;}
    if !on||!fw::screen_is_on(){cancel();return;}
    after_close();
}
