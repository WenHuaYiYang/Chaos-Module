#![allow(dead_code,static_mut_refs)]
use std::cell::RefCell;
#[derive(Default)] struct State {files:[u8;2],mounts:u32,algorithm:u32,mask:u32,busy:bool}
thread_local! {static STATE:RefCell<State>=RefCell::new(State::default());}
fn state<T>(f:impl FnOnce(&mut State)->T)->T{STATE.with(|s|f(&mut s.borrow_mut()))}
mod fw_api {
    pub mod oflag {pub const RDONLY:u32=0;pub const WRONLY:u32=1;pub const CREAT:u32=2;}
    pub unsafe fn open(p:*const u8,_:u32,_:u32)->i32 {
        if std::ffi::CStr::from_ptr(p.cast()).to_bytes().ends_with(b"/blur_v2.cfg"){4}else{3}
    }
    pub unsafe fn read(fd:i32,p:*mut u8,_:u32)->i32{*p=super::state(|s|s.files[(fd-3)as usize]);1}
    pub unsafe fn write(fd:i32,p:*const u8,_:u32)->i32{super::state(|s|s.files[(fd-3)as usize]=*p);1}
    pub unsafe fn close(_:i32){}
}
mod background_home {pub unsafe fn remount(){super::state(|s|s.mounts+=1);}}
#[path="../src/corona_blur.rs"] mod controller;
#[no_mangle] extern "C" fn chaos_blur_select_algorithm(v:u32)->i32 {
    state(|s|{if s.busy{return -2145;}s.algorithm=v;0})
}
#[no_mangle] extern "C" fn chaos_corona_backgrounds_commit(_:u32,mask:u32)->i32{state(|s|s.mask=mask);0}
#[no_mangle] extern "C" fn chaos_corona_backgrounds_poll(){}
#[no_mangle] extern "C" fn chaos_corona_backgrounds_pending()->u32{0}
#[no_mangle] extern "C" fn chaos_corona_backgrounds_idle()->i32{1}
#[no_mangle] extern "C" fn chaos_corona_backgrounds_owned()->u32{1}
#[no_mangle] extern "C" fn chaos_corona_diagnostics(_: *mut u8,_:u32)->u32{0}
#[test] fn independent_choices_persist_switch_algorithm_and_defer_when_busy(){unsafe{
    assert!(!controller::active());
    controller::toggle_corona();assert!(controller::enabled());
    assert_eq!(controller::backgrounds_commit(true,15),0);
    state(|s|{assert_eq!(s.files,[1,0]);assert_eq!(s.algorithm,0);assert_eq!(s.mask,15);});
    controller::toggle_v2();
    assert!(!controller::enabled());assert!(controller::v2_enabled());
    state(|s|{assert_eq!(s.files,[0,1]);s.busy=true;});
    assert_eq!(controller::backgrounds_commit(true,5),-2145);
    state(|s|{assert_eq!(s.algorithm,0);s.busy=false;});
    assert_eq!(controller::backgrounds_commit(true,5),0);
    state(|s|{assert_eq!(s.algorithm,1);assert_eq!(s.mask,5);});
    controller::toggle_corona();
    assert!(!controller::v2_enabled());state(|s|assert_eq!(s.files,[1,0]));
    controller::set_enabled(false);controller::set_v2_enabled(false);
    assert!(!controller::active());state(|s|assert_eq!(s.files,[0,0]));
}}
