//! Production switch config: the native gate must change before storage or maintenance.
#![allow(dead_code,static_mut_refs)]
use std::sync::Mutex;
#[derive(Default)]struct Fixture{config:Option<Vec<u8>>,gate:bool,fail_write:bool,started:bool,revoked:bool,events:Vec<String>,now:u32}
static FIX:Mutex<Option<Fixture>>=Mutex::new(None);
fn with<T>(f:impl FnOnce(&mut Fixture)->T)->T{f(FIX.lock().unwrap().as_mut().unwrap())}
#[macro_export]macro_rules! st_rd{($e:expr)=>{core::ptr::read_volatile(core::ptr::addr_of!($e))};}
#[macro_export]macro_rules! st_wr{($e:expr,$v:expr)=>{core::ptr::write_volatile(core::ptr::addr_of_mut!($e),$v)};}
mod state{pub static mut BRIDGE_STATE:u32=0;}
mod connect_log{pub unsafe fn line(_: &str){}}
mod connect_radio{pub fn ensure_worker(){} pub unsafe fn load_enabled(){}}
mod connect_permission{
    pub unsafe fn load(){}
    pub fn disconnect(){super::with(|f|{assert!(!f.gate);f.revoked=true;f.events.push("revoke".into());});}
}
mod fw_api{
    pub mod oflag{pub const RDONLY:u32=1;pub const WRONLY:u32=2;pub const CREAT:u32=4;}
    // 维护节拍现在按毫秒退避重启(生产代码读 capture_now_ms 算 RETRY_AT)。每次读表前进
    // 1 秒, 连续 tick 之间足够越过 RETRY_MS 窗口, 用例不必关心具体时间点。
    pub unsafe fn capture_now_ms()->u32{super::with(|f|{f.now=f.now.wrapping_add(1000);f.now})}
    pub unsafe fn open(_: *const u8,flags:u32,_:u32)->i32{super::with(|f|{
        if flags&2!=0{f.events.push("save".into());2}else if f.config.is_some(){1}else{-1}
    })}
    pub unsafe fn read(_:i32,p:*mut u8,n:u32)->i32{super::with(|f|{
        let b=f.config.as_ref().unwrap();let len=b.len().min(n as usize);core::ptr::copy_nonoverlapping(b.as_ptr(),p,len);len as i32
    })}
    pub unsafe fn write(_:i32,p:*const u8,n:u32)->i32{super::with(|f|{
        if f.fail_write{return -1;}f.config=Some(core::slice::from_raw_parts(p,n as usize).to_vec());n as i32
    })}
    pub unsafe fn close(_:i32)->i32{0}
}
#[no_mangle]extern "C" fn chaos_bridge_set_enabled(enabled:u32){with(|f|{f.gate=enabled!=0;f.events.push(format!("gate={enabled}"));});}
#[no_mangle]extern "C" fn chaos_bridge_start()->i32{with(|f|{assert!(f.gate);f.started=true;f.events.push("start".into());});0}
#[no_mangle]extern "C" fn chaos_bridge_stop(){with(|f|{assert!(!f.gate);f.started=false;f.events.push("stop".into());});}
#[path="../src/bridge_cfg.rs"]mod bridge_cfg;
fn main(){
    let case=std::env::args().nth(1).unwrap();
    *FIX.lock().unwrap()=Some(Fixture::default());
    unsafe{match case.as_str(){
        "default"=>{assert!(!bridge_cfg::enabled());bridge_cfg::tick();with(|f|assert!(!f.gate&&!f.started));}
        "saved"=>{with(|f|f.config=Some(b"BRG1\x01".to_vec()));assert!(bridge_cfg::enabled());with(|f|assert!(f.gate));bridge_cfg::tick();with(|f|assert!(f.started));}
        "immediate"|"failed-save"=>{
            assert!(bridge_cfg::toggle());bridge_cfg::tick();with(|f|{assert!(f.started);f.events.clear();f.fail_write=case=="failed-save";});
            assert!(!bridge_cfg::toggle());
            with(|f|{assert!(!f.gate&&f.revoked&&f.started);assert_eq!(&f.events[..3],["gate=0","revoke","save"]);});
            bridge_cfg::tick();with(|f|assert!(!f.started&&!f.gate));
        }
        "rapid"=>{
            bridge_cfg::toggle();bridge_cfg::tick();
            assert!(!bridge_cfg::toggle());assert!(bridge_cfg::toggle());assert!(!bridge_cfg::toggle());
            with(|f|assert!(!f.gate));bridge_cfg::tick();with(|f|assert!(!f.started&&!f.gate));
            assert!(bridge_cfg::toggle());with(|f|assert!(f.gate));bridge_cfg::tick();with(|f|assert!(f.started));
        }
        _=>panic!("unknown case"),
    }}
    println!("PASS native switch {case}");
}
