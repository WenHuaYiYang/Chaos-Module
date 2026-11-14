//! Persisted device policy and an atomic mailbox between protocol and LVGL owners.
use core::sync::atomic::{AtomicU32, Ordering};
use crate::{fw_api, connect_log};
pub(crate) const ALLOW:u32=0;
pub(crate) const DENY:u32=1;
pub(crate) const ASK:u32=2;
static POLICY:AtomicU32=AtomicU32::new(ASK);
static STATE:AtomicU32=AtomicU32::new(0);
static TOKEN:AtomicU32=AtomicU32::new(0);
static DECISION:AtomicU32=AtomicU32::new(0);
static mut LOADED:bool=false;
const PATH:&[u8]=b"/data/chaos/connect-permissions.cfg\0";
const TEMP:&[u8]=b"/data/chaos/connect-permissions.tmp\0";

pub(crate) unsafe fn load(){
    if LOADED{return;}LOADED=true;
    let fd=fw_api::open(PATH.as_ptr(),fw_api::oflag::RDONLY,0);if fd<0{return;}
    let mut b=[0;5];let n=fw_api::read(fd,b.as_mut_ptr(),5);fw_api::close(fd);
    if n==5&&&b[..4]==b"CCP1"&&b[4]<=2{POLICY.store(b[4]as u32,Ordering::Release);}
}
pub(crate) fn policy()->u32{POLICY.load(Ordering::Acquire)}
pub(crate) unsafe fn set_policy(value:u32){
    if value>ASK{return;}load();
    let b=[b'C',b'C',b'P',b'1',value as u8];
    let fd=fw_api::open(TEMP.as_ptr(),fw_api::oflag::WRONLY|fw_api::oflag::CREAT,0o640);
    let mut ok=false;
    if fd>=0{let n=fw_api::write(fd,b.as_ptr(),5);ok=fw_api::close(fd)==0&&n==5;}
    if ok{ok=fw_api::capture_rename(TEMP.as_ptr(),PATH.as_ptr())==0;}
    if ok{POLICY.store(value,Ordering::Release);connect_log::line(match value{ALLOW=>"允许开发者模式权限：开启",DENY=>"允许开发者模式权限：关闭",_=>"允许开发者模式权限：询问"});}
    else{fw_api::fs_remove(TEMP.as_ptr());connect_log::line("ChaosConnect权限保存失败，保持原策略");}
}
pub(crate) fn state()->u32{STATE.load(Ordering::Acquire)}
pub(crate) fn pending()->u32{
    if state()&1!=0&&(state()>>2)&7==1{TOKEN.load(Ordering::Acquire)}else{0}
}
pub(crate) fn decide(token:u32,allow:bool){
    if token==0||pending()!=token{return;}
    // One atomic word prevents pairing an old answer with a newer request token.
    // Tokens are opaque nonzero 30-bit values issued by the file owner.
    if token>0x3fff_ffff{return;}
    DECISION.store((token<<2)|if allow{1}else{2},Ordering::Release);
}
#[no_mangle] pub extern "C" fn chaos_connect_policy()->u32{policy()}
#[no_mangle] pub extern "C" fn chaos_connect_now_ms()->u32{fw_api::capture_now_ms()}
#[no_mangle] pub extern "C" fn chaos_connect_state(flags:u32,token:u32){
    TOKEN.store(token,Ordering::Release);STATE.store(flags,Ordering::Release);
}
#[no_mangle] pub extern "C" fn chaos_connect_take_decision(token:u32)->u32{
    let encoded=DECISION.swap(0,Ordering::AcqRel);
    if token!=0&&encoded>>2==token{encoded&3}else{0}
}
