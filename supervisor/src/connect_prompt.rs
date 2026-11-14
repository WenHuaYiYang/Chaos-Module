//! System confirmation on the verified display top layer; LVGL timer/event context only.
use crate::{fw_api as fw,mem::{safe_ptr,rd32},connect_permission as permission};
static mut OBJECT:u32=0;
static mut TOP:u32=0;
static mut TOKEN:u32=0;
static mut ALERTED:u32=0;
static mut BUTTONS:[u32;10]=[0;10];
unsafe fn close(){
    let old=OBJECT;OBJECT=0;TOKEN=0;
    if safe_ptr(TOP)&&old!=0&&fw::obj_is_child_of(TOP,old){fw::obj_delete(old);}
    TOP=0;
}
pub(crate) unsafe fn choose(event:u32,allow:bool){
    if fw::event_get_code(event)!=fw::ev::CLICKED||OBJECT==0{return;}
    let token=TOKEN;close();permission::decide(token,allow);
}
unsafe extern "C" fn deny(e:u32){choose(e,false);}
unsafe extern "C" fn allow(e:u32){choose(e,true);}
unsafe extern "C" fn deleted(_:u32){
    if OBJECT!=0{let token=TOKEN;OBJECT=0;TOP=0;TOKEN=0;permission::decide(token,false);}
}
pub(crate) unsafe fn tick(){
    let token=permission::pending();
    if token==0{ALERTED=0;}
    if token!=0&&token!=ALERTED{
        ALERTED=token;
        if !fw::screen_is_on(){fw::screen_wake_for_connect();}
        fw::vibrate(0x0d);
    }
    if OBJECT!=0{
        if token!=TOKEN||!fw::screen_is_on(){close();return;}
        if !safe_ptr(TOP)||!fw::obj_is_child_of(TOP,OBJECT){let old=TOKEN;OBJECT=0;TOP=0;TOKEN=0;permission::decide(old,false);}
        return;
    }
    if token==0||!fw::screen_is_on(){return;}
    // Existing system dialogs retain priority. A pending request still times out closed.
    let display=fw::display_default();if !safe_ptr(display){return;}
    TOP=fw::display_top_layer(display);if !safe_ptr(TOP){TOP=0;return;}
    let existing=rd32(fw::MSGBOX_SINGLETON as *const u32);
    if safe_ptr(existing)&&fw::obj_is_child_of(TOP,existing){TOP=0;return;}
    let mb=fw::msgbox_create(TOP,0);
    if !safe_ptr(mb){TOP=0;permission::decide(token,false);crate::connect_log::line("授权弹窗创建失败，拒绝开发者模式请求");return;}
    OBJECT=mb;TOKEN=token;
    BUTTONS=[0;10];BUTTONS[0]=(deny as *const ()as u32)|1;BUTTONS[5]=(allow as *const ()as u32)|1;
    fw::msgbox_fill(mb,5,0,"是否允许插件启用开发者模式？\0".as_ptr(),0,core::ptr::addr_of!(BUTTONS).cast(),2);
    fw::obj_add_event(mb,deleted as *const ()as u32,36,0);
    crate::connect_log::line("已弹出开发者模式授权提醒，等待用户选择");
}
