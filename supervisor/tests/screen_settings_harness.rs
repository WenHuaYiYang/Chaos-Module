#![allow(dead_code, static_mut_refs)]
use std::{cell::RefCell, collections::BTreeMap};
#[derive(Default)]
struct State {
    files: BTreeMap<String, Vec<u8>>, fds: BTreeMap<i32, (String, usize)>,
    timeout: u32, palm: bool, timeout_calls: u32, palm_calls: u32,
    display: u32, rotations: Vec<u32>, invalidated: Vec<u32>, opens: u32, writes: u32,
    short_io: usize, interrupts: u32, write_error: bool, rename_error: bool, close_error: bool,
    input: String,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
fn state<T>(f: impl FnOnce(&mut State)->T)->T { STATE.with(|s| f(&mut s.borrow_mut())) }
fn text(p: *const u8)->String { unsafe { std::ffi::CStr::from_ptr(p.cast()).to_str().unwrap().to_owned() } }
mod fw_api {
    use super::*;
    pub mod oflag {pub const RDONLY:u32=1;pub const WRONLY:u32=2;pub const CREAT:u32=4;}
    pub unsafe fn open(p:*const u8, flags:u32, _:u32)->i32 { let p=text(p); state(|s| {
        s.opens+=1;
        if flags & oflag::CREAT != 0 { s.files.entry(p.clone()).or_default(); }
        if !s.files.contains_key(&p) {return -2;}
        if flags & 0x20 != 0 {s.files.get_mut(&p).unwrap().clear();}
        let fd=s.fds.keys().next_back().copied().unwrap_or(2)+1;
        s.fds.insert(fd,(p,0)); fd
    }) }
    pub unsafe fn close(fd:i32)->i32 {state(|s| {assert!(s.fds.remove(&fd).is_some());if s.close_error {-5}else{0}})}
    pub unsafe fn capture_result(rc:i32)->i32 {rc}
    pub unsafe fn capture_mkdir_path(_: *const u8)->i32 {0}
    pub unsafe fn nsh_read(fd:i32, out:&mut [u8])->i32 {state(|s| {
        if s.interrupts>0 {s.interrupts-=1;return -4;}
        let (p,cursor)=s.fds.get_mut(&fd).unwrap();let data=&s.files[p];
        let n=out.len().min(data.len()-*cursor).min(s.short_io);
        out[..n].copy_from_slice(&data[*cursor..*cursor+n]);*cursor+=n;n as i32
    })}
    pub unsafe fn nsh_write(fd:i32, input:&[u8])->i32 {state(|s| {
        if s.interrupts>0 {s.interrupts-=1;return -4;}
        s.writes+=1;if s.write_error{return -28;}
        let (p,cursor)=s.fds.get_mut(&fd).unwrap();let data=s.files.get_mut(p).unwrap();
        let n=input.len().min(s.short_io);data.resize(*cursor+n,0);
        data[*cursor..*cursor+n].copy_from_slice(&input[..n]);*cursor+=n;n as i32
    })}
    pub unsafe fn capture_rename(a:*const u8,b:*const u8)->i32 {state(|s| {
        if s.rename_error{return -13;}let bytes=s.files.remove(&text(a)).unwrap();s.files.insert(text(b),bytes);0
    })}
    pub unsafe fn fs_remove(p:*const u8)->i32 {state(|s| {s.files.remove(&text(p));0})}
    pub unsafe fn screen_timeout_ms()->u32 {state(|s|s.timeout)}
    pub unsafe fn screen_timeout_set(n:u32){state(|s|{s.timeout=n;s.timeout_calls+=1;});}
    pub unsafe fn screen_touch_palm_set(on:bool){state(|s|{s.palm=on;s.palm_calls+=1;});}
    pub unsafe fn display_default()->u32{state(|s|s.display)}
    pub unsafe fn display_set_rotation(_:u32,n:u32){state(|s|s.rotations.push(n));}
    pub unsafe fn obj_invalidate(o:u32){state(|s|s.invalidated.push(o));}
}
mod state {pub struct PageState {pub content:u32} pub static mut PAGES:[PageState;1]=[PageState{content:111}];}
pub struct W {p:*mut u8,n:usize}
impl W {
    pub fn new(p:*mut u8,_:usize)->Self{Self{p,n:0}}
    pub fn n(&mut self,n:u32)->&mut Self{let s=n.to_string();unsafe{std::ptr::copy_nonoverlapping(s.as_ptr(),self.p,s.len());}self.n=s.len();self}
    pub fn end(&mut self){unsafe{*self.p.add(self.n)=0;}}
}
mod keyboard {pub enum Completion{DimmerTimeout} pub unsafe fn open(p:*const u8,_:Completion){super::state(|s|s.input=super::text(p));}}
#[path="../src/settings_store.rs"]mod settings_store;
#[path="../src/screen_rotation.rs"]mod screen_rotation;
#[path="../src/dimmer_timeout.rs"]mod dimmer_timeout;
#[path="../src/touch_palm.rs"]mod touch_palm;
const DIMMER:&str="/data/chaos/dimmer-timeout.cfg";
const PALM:&str="/data/chaos/disable-touch-palm.cfg";
fn reset(){unsafe{
    state(|s|*s=State{timeout:30_000,palm:true,short_io:usize::MAX,..Default::default()});
    screen_rotation::reset_for_test();dimmer_timeout::reset_for_test();touch_palm::reset_for_test();
}}
fn clean(){state(|s|{assert!(s.fds.is_empty());assert!(!s.files.keys().any(|p|p.ends_with(".tmp")));});}
#[test]
fn rotation_ignores_old_file_resets_each_injection_and_defers_until_display_ready(){unsafe{
    reset();state(|s|{s.files.insert("/data/chaos/screen-rotation.cfg".into(),vec![2]);});
    assert!(!screen_rotation::enabled());assert!(screen_rotation::toggle());
    state(|s|assert!(s.rotations.is_empty()));
    state(|s|s.display=1);screen_rotation::ensure_applied();screen_rotation::ensure_applied();
    state(|s|{assert_eq!(s.rotations,[2]);assert_eq!(s.invalidated,[111]);assert_eq!(s.opens,0);assert_eq!(s.writes,0);});
    screen_rotation::reset_for_test();screen_rotation::ensure_applied();assert!(!screen_rotation::enabled());
    state(|s|{assert_eq!(s.rotations,[2,0]);assert_eq!(s.opens,0);assert_eq!(s.writes,0);});clean();
}}
#[test]
fn timeout_survives_injection_restores_once_and_never_saves_keep_awake(){unsafe{
    reset();state(|s|{s.short_io=2;s.interrupts=2;});
    assert!(dimmer_timeout::save(b"42\0".as_ptr()));
    state(|s|{assert_eq!(s.timeout,42_000);s.timeout=5_000;});
    dimmer_timeout::reset_for_test();dimmer_timeout::ensure_applied();
    state(|s|{assert_eq!(s.timeout,42_000);assert_eq!(s.timeout_calls,2);s.timeout=i32::MAX as u32;});
    dimmer_timeout::ensure_applied();dimmer_timeout::open();
    state(|s|{assert_eq!(s.timeout,i32::MAX as u32);assert_eq!(s.input,"42");assert_eq!(s.timeout_calls,2);});
    dimmer_timeout::reset_for_test();dimmer_timeout::ensure_applied();
    state(|s|assert_eq!(s.timeout,42_000));clean();
}}
#[test]
fn timeout_rejects_bad_input_and_preserves_previous_value_on_storage_failures(){unsafe{
    reset();assert!(dimmer_timeout::save(b"60\0".as_ptr()));
    let original=state(|s|s.files[DIMMER].clone());
    for input in [b"\0".as_slice(),b"0\0",b"-1\0",b"1a\0",b"2147484\0",b"99999999\0"]{assert!(!dimmer_timeout::save(input.as_ptr()));}
    for mode in 0..3{
        state(|s|{s.write_error=mode==0;s.rename_error=mode==1;s.close_error=mode==2;});
        assert!(!dimmer_timeout::save(b"10\0".as_ptr()));assert_eq!(text(dimmer_timeout::error_text()),"保存失败，请重试");
        state(|s|{assert_eq!(s.files[DIMMER],original);assert_eq!(s.timeout,60_000);});clean();
    }
    state(|s|{s.write_error=false;s.rename_error=false;s.close_error=false;});
    assert!(dimmer_timeout::save(b"2147483\0".as_ptr()));state(|s|assert_eq!(s.timeout,2_147_483_000));clean();
}}
#[test]
fn missing_corrupt_and_overlong_timeout_configs_leave_system_timeout_alone(){unsafe{
    reset();dimmer_timeout::ensure_applied();state(|s|assert_eq!(s.timeout_calls,0));
    assert!(dimmer_timeout::save(b"20\0".as_ptr()));let valid=state(|s|s.files[DIMMER].clone());
    let mut bad=valid.clone();bad[8]^=1;let mut overlong=valid.clone();overlong.push(0);
    for bytes in [vec![],vec![0;9],bad,overlong]{
        dimmer_timeout::reset_for_test();state(|s|{s.files.insert(DIMMER.into(),bytes);s.timeout=90_000;});
        dimmer_timeout::ensure_applied();state(|s|assert_eq!(s.timeout,90_000));clean();
    }
}}
#[test]
fn palm_setting_restores_existing_config_without_opening_chaos_and_is_not_periodic(){unsafe{
    reset();state(|s|{s.files.insert(PALM.into(),vec![1]);});
    touch_palm::ensure_applied();assert!(touch_palm::enabled());state(|s|{assert!(!s.palm);s.palm=true;});
    touch_palm::ensure_applied();state(|s|{assert!(s.palm);assert_eq!(s.palm_calls,1);assert_eq!(s.writes,0);});
    touch_palm::toggle();assert!(!touch_palm::enabled());state(|s|assert_eq!(s.files[PALM],[0]));
    touch_palm::toggle();assert!(touch_palm::enabled());
    touch_palm::reset_for_test();state(|s|s.palm=true);touch_palm::ensure_applied();state(|s|assert!(!s.palm));clean();
}}
#[test]
fn palm_failed_save_keeps_previous_file_and_reports_runtime_only_change(){unsafe{
    reset();touch_palm::ensure_applied();assert!(!touch_palm::enabled());
    touch_palm::toggle();let old=state(|s|s.files[PALM].clone());
    state(|s|s.rename_error=true);touch_palm::toggle();assert!(!touch_palm::enabled());
    assert_eq!(text(touch_palm::status()),"保存失败，仅本次生效");
    state(|s|assert_eq!(s.files[PALM],old));clean();
    state(|s|{s.rename_error=false;s.palm=true;});touch_palm::reset_for_test();touch_palm::ensure_applied();
    assert!(touch_palm::enabled());state(|s|assert!(!s.palm));clean();
}}
