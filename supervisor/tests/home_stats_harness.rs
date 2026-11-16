#![allow(dead_code,static_mut_refs)]
use core::ptr::write_volatile;
#[path="../src/cadence.rs"]mod cadence;
macro_rules! st_rd {($x:expr)=>{$x};}
macro_rules! st_wr {($x:expr,$v:expr)=>{$x=$v};}
const STAT_STRIDE:usize=32;
const ROW_SUB_LB:u32=64;
const MAX_PID:usize=0;
static mut STAT_CADENCE:cadence::Cadence=cadence::Cadence::new();
static mut STAT_TICK:u32=19;
static mut STAT_VAL:[[u8;32];3]=[[0;32];3];
static mut STAT_PCT:[u32;2]=[0;2];
static mut DR_HANDLES:[u32;6]=[10,11,12,20,21,22];
static mut ROWS:[u32;16]=[0;16];
static mut APP_FG:u32=0;
static mut FG_PAGE:u32=0;
static mut HEAL_TRYS:u32=0;
static mut NOW:u32=0;
static mut AWAKE:bool=true;
static mut FILLS:[usize;3]=[0;3];
static mut VALUES:[[u8;32];3]=[[0;32];3];
struct PageState {built:u32}
static mut PAGES:[PageState;1]=[PageState{built:1}];
struct Page;
impl Page {fn on_enter(&self){}}
static PAGE_TABLE:[Page;1]=[Page];
mod page {pub mod motion {pub fn arm(_:u32){}}}
unsafe fn rd16(_: *const u16)->u16 {0}
unsafe fn shake_arm(){}
unsafe fn render_page(_:usize)->i32 {0}
unsafe fn page_is_live(_:usize)->bool {true}
unsafe fn page_rows_ptr(_:usize)->*const u32 {core::ptr::addr_of!(ROWS).cast()}
// The only native raw-object read in the tick is CPU row + ROW_SUB_LB.
unsafe fn read_volatile<T:Copy>(p:*const T)->T {
    if p as usize==30+ROW_SUB_LB as usize { return (core::ptr::addr_of!(CPU_LABEL) as *const T).read(); }
    core::ptr::read_volatile(p)
}
static CPU_LABEL:u32=31;
unsafe fn stat_name(_:usize)->*const u8 {b"test\0".as_ptr()}
unsafe fn stat_fill_pct(i:usize)->u32 {
    FILLS[i]+=1;STAT_VAL[i]=[0;32];STAT_VAL[i][..3].copy_from_slice([b'4',b'2',0].as_slice());42
}
unsafe fn charge_text()->*const u8 {b"charge\0".as_ptr()}
mod fw_api {
    use super::*;
    pub const NONE_PTR:*const u8=core::ptr::null();
    pub unsafe fn capture_now_ms()->u32 {NOW}
    pub unsafe fn screen_is_on()->bool {AWAKE}
    pub unsafe fn label_set_text(label:u32,text:*const u8) {
        let i=match label {11=>0,21=>1,31=>2,_=>panic!("unexpected label")};
        VALUES[i]=[0;32];let s=std::ffi::CStr::from_ptr(text.cast()).to_bytes();VALUES[i][..s.len()].copy_from_slice(s);
    }
    pub unsafe extern "C" fn row_update(_:u32,_:*const u8,_:*const u8,_:*const u8,_:i32,_:u8)->i32 {0}
    pub unsafe fn bar_set_pct(_:u32,_:u32){}
}
include!(env!("CHAOS_HOME_SOURCE"));
fn main(){unsafe {
    ROWS[2]=30;
    // Reproduce the stale foreground flag following pause, with blank caches.
    APP_FG=0;assert!(needs_tick());tick();
    assert_eq!(FILLS,[1;3]);for value in VALUES {assert_eq!(&value[..2],b"42");}
    assert!(!needs_tick());
    NOW=999;assert!(!needs_tick());NOW=1000;assert!(needs_tick());tick();assert_eq!(FILLS,[2;3]);
    APP_FG=0;assert_eq!(resume(0),0);assert_eq!(APP_FG,1);
    STAT_TICK=19;assert!(needs_tick());tick();assert_eq!(FILLS,[3;3]);
    AWAKE=false;APP_FG=0;NOW=5000;assert!(!needs_tick());
    AWAKE=true;resume(0);assert_eq!(APP_FG,1);assert!(needs_tick());tick();assert_eq!(FILLS,[4;3]);
    for _ in 0..65 {NOW+=1000;assert!(needs_tick());tick();}
    assert_eq!(FILLS,[69;3]);
    println!("PASS production home stats: blank-cache first fill, stale foreground flag, 1s cadence, resume, wake and minute wrap");
}}
