#![allow(dead_code,static_mut_refs)]
macro_rules! st_rd {($v:expr)=>{$v}}
macro_rules! st_wr {($v:expr,$n:expr)=>{$v=$n}}
static mut ROOT:u32=100;
static mut REGISTERED:u32=0;
static mut ADDS:u32=0;
static mut FAIL:bool=false;
static mut SCREEN:bool=true;
static mut TOP:u32=1;
static mut VIBRATIONS:u32=0;
static mut EVENT:[u32;4]=[0;4];
unsafe fn rd32(p:*const u32)->u32 {EVENT[(p as usize-0x1000)/4]}
unsafe fn rd16(p:*const u16)->u16 {EVENT[(p as usize-0x1000)/4] as u16}
mod fw_api {
    pub mod oflag {pub const RDONLY:u32=1;pub const WRONLY:u32=2;pub const CREAT:u32=4;}
    pub const HOME_APP_ID:u32=1;
    pub unsafe fn open(_: *const u8,_:u32,_:u32)->i32{-1}
    pub unsafe fn read(_:i32,_:*mut u8,_:u32)->i32{0}
    pub unsafe fn write(_:i32,_:*const u8,_:u32)->i32{0}
    pub unsafe fn close(_:i32){}
    pub unsafe fn top_app_id()->u32 {super::TOP}
    pub unsafe fn screen_is_on()->bool {super::SCREEN}
    pub unsafe fn vibrate(v:u32){assert_eq!(v,0xD);super::VIBRATIONS+=1;}
    pub unsafe fn background_watch_launcher(_:u32)->u32 {
        if super::FAIL {return 0;}
        if super::REGISTERED!=super::ROOT {super::REGISTERED=super::ROOT;super::ADDS+=1;}
        super::ROOT
    }
}
include!(env!("CHAOS_SWIPE_SOURCE"));
unsafe fn emit(current:u32,target:u32,code:u32){EVENT=[current,target,code,0];sv_event(0x1000);}
fn main(){unsafe{
    sv_watch_tick();assert_eq!(ADDS,0);
    // This sequence runs regardless of V2 already owning all backgrounds.
    set_swipe_vib(true,1);sv_watch_tick();assert_eq!(SV_ROOT,100);assert_eq!(ADDS,1);
    for _ in 0..100 {sv_watch_tick();}assert_eq!(ADDS,1);
    emit(100,200,4);assert_eq!(VIBRATIONS,0);vib_tick();assert_eq!(VIBRATIONS,1);
    emit(100,200,4);vib_tick();assert_eq!(VIBRATIONS,1);
    emit(100,200,9);vib_tick();assert_eq!(VIBRATIONS,2);
    vib_tick();
    for code in 24..36 {emit(100,100,code);vib_tick();}assert_eq!(VIBRATIONS,2);
    emit(300,301,4);vib_tick();assert_eq!(VIBRATIONS,2);
    set_swipe_vib(true,0);emit(100,200,4);vib_tick();assert_eq!(VIBRATIONS,2);
    set_swipe_vib(true,1);emit(100,200,4);vib_tick();assert_eq!(VIBRATIONS,3);
    // DELETE and native callback-table replacement both recover on next tick.
    emit(100,100,36);assert_eq!(SV_ROOT,0);ROOT=400;REGISTERED=0;
    FAIL=true;sv_watch_tick();assert_eq!(SV_ROOT,0);FAIL=false;
    sv_watch_tick();assert_eq!(SV_ROOT,400);assert_eq!(ADDS,2);
    REGISTERED=0;sv_watch_tick();assert_eq!(ADDS,3);
    vib_tick();emit(100,100,4);vib_tick();assert_eq!(VIBRATIONS,3);
    emit(400,500,4);vib_tick();assert_eq!(VIBRATIONS,4);
    // Cooldown survives wrapping tick arithmetic.
    SV_TICK=u32::MAX;SV_LAST_VIB=u32::MAX-1;emit(400,500,4);vib_tick();assert_eq!(VIBRATIONS,4);
    emit(400,500,4);vib_tick();assert_eq!(VIBRATIONS,5);
    SCREEN=false;REGISTERED=0;sv_watch_tick();assert_eq!(ADDS,3);SCREEN=true;
    set_swipe_vib(false,1);TOP=99;vib_tick();TOP=1;vib_tick();assert_eq!(VIBRATIONS,6);
    vib_test_req();vib_tick();assert_eq!(VIBRATIONS,7);
}}
