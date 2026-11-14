//! 快应用冷启动画面：先完成原生打开动画，再交给系统构建引擎。
use crate::{fw_api as fw,mem::{rd32,safe_ptr}};
use core::mem::transmute;
static mut COVER:u32=0;
static mut TOP:u32=0;
static mut APP:u32=0;
static mut START:u32=0;
static mut DRAWN:bool=false;
static mut LAUNCHED:bool=false;
static mut READY_TICKS:u8=0;
static mut FADE:Option<u32>=None;
unsafe fn word(p:u32,o:u32)->u32{rd32((p+o)as *const u32)}
pub(crate) unsafe fn owns(id:u32)->bool{COVER!=0&&APP==id}
pub(crate) unsafe fn active()->bool{COVER!=0}
pub(crate) unsafe fn app()->u32{APP}
pub(crate) unsafe fn front(root:u32,id:u32)->bool{
    if !safe_ptr(root){return false;}
    if fw::top_app_id()==id{return true;}
    // 快应用可由共享代理原生页承载，前台page的app_id未必等于动态ID。
    let page=fw::top_page_desc();if !safe_ptr(page){return false;}
    let page_root=word(page,fw::DESC_ROOT);
    safe_ptr(page_root)&&(root==page_root||fw::obj_is_child_of(page_root,root))
}
pub(crate) unsafe fn cancel(){
    let old=COVER;COVER=0;APP=0;DRAWN=false;FADE=None;READY_TICKS=0;
    if old!=0&&safe_ptr(TOP)&&fw::obj_is_child_of(TOP,old){fw::obj_delete(old);}
    TOP=0;
}
unsafe extern "C" fn event(e:u32){
    let target=word(e,0);
    if target!=COVER{return;}
    match fw::event_get_code(e){25=>DRAWN=true,36=>{COVER=0;APP=0;},_=>{}}
}
pub(crate) unsafe fn start(id:u32,context:*const u32)->bool{
    cancel();
    let display=fw::display_default();if !safe_ptr(display){return false;}
    // miwear get_layer_top -> 0x0CAC0219 -> 原生display+0x2C8。
    let top:unsafe extern "C" fn(u32)->u32=transmute(0x0cac0219usize);
    TOP=top(display);if !safe_ptr(TOP){TOP=0;return false;}
    let lookup:unsafe extern "C" fn(u32)->u32=transmute(0x0ca69935usize);
    let node=lookup(id);if !safe_ptr(node){return false;}
    let icon=word(node,0xc);
    if !crate::mem::plausible_ptr(icon){return false;}
    COVER=fw::obj_create(TOP,0);if COVER==0{return false;}
    fw::obj_clear_box_spacing(COVER);fw::obj_clear_scrollable(COVER);
    fw::obj_set_size_wh(COVER,336,480);fw::obj_align(COVER,fw::align::TOP_LEFT,0,0);
    fw::obj_add_flag(COVER,0x40000|fw::LV_OBJ_FLAG_CLICKABLE);
    fw::obj_set_local_style_prop(COVER,0x1c,0,0);
    fw::background_set_opa(COVER,255);
    fw::obj_add_event(COVER,event as *const ()as u32,0,0);
    let image=fw::background_image_create(COVER);
    if image==0{cancel();return false;}
    fw::background_image_set_source(image,icon);
    fw::obj_clear_flag(image,fw::LV_OBJ_FLAG_CLICKABLE);
    fw::obj_align(image,fw::align::CENTER,0,0);
    APP=id;START=fw::capture_now_ms();LAUNCHED=false;DRAWN=false;
    let mut ctx=[0u32;14];
    core::ptr::copy_nonoverlapping(context,ctx.as_mut_ptr(),14);
    ctx[11]=200;ctx[12]=0; // 固件打开过渡200ms，无延迟。
    let animate:unsafe extern "C" fn(u32,u32,*const u32)=transmute(0x0ca46249usize);
    animate(COVER,0,ctx.as_ptr());
    true
}
/// 返回true时UI可提交原启动请求。未绘制首帧时不因时间到了提前加载。
pub(crate) unsafe fn launch_ready()->bool{
    if COVER==0||LAUNCHED{return false;}
    if !fw::screen_is_on()||fw::top_app_id()!=fw::HOME_APP_ID{cancel();return false;}
    if !DRAWN&&fw::capture_now_ms().wrapping_sub(START)>=2000 {
        // 首帧通知缺失时释放遮罩，保留系统正常启动能力。
        cancel();return true;
    }
    if DRAWN&&fw::capture_now_ms().wrapping_sub(START)>=240 {
        LAUNCHED=true;return true;
    }
    false
}
pub(crate) unsafe fn tick(id:u32,root:u32){
    if COVER==0{return;}
    if !safe_ptr(TOP)||!fw::obj_is_child_of(TOP,COVER){COVER=0;APP=0;return;}
    if !LAUNCHED{return;}
    let elapsed=fw::capture_now_ms().wrapping_sub(START);
    if elapsed>=10000||!fw::screen_is_on(){cancel();return;}
    if id!=APP{return;}
    if !front(root,APP) {
        if fw::top_app_id()!=fw::HOME_APP_ID{cancel();}
        return;
    }
    if !safe_ptr(root)||word(root,0x2c)&1!=0||fw::obj_child_count(root)<=0{return;}
    if READY_TICKS<2 {READY_TICKS+=1;fw::obj_invalidate(root);return;}
    let started=*FADE.get_or_insert(fw::capture_now_ms());
    let progress=fw::capture_now_ms().wrapping_sub(started);
    if progress>=180 {cancel();return;}
    fw::obj_set_local_style_prop(COVER,95,255-progress*255/180,0);
    fw::obj_invalidate(root);
}
