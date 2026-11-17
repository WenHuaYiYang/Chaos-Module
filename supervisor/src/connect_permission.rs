//! Persisted device policy and an atomic mailbox between protocol and LVGL owners.
use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use crate::{fw_api, connect_log};
pub(crate) const ALLOW:u32=0;
pub(crate) const DENY:u32=1;
pub(crate) const ASK:u32=2;
static INSTALL_POLICY:AtomicU32=AtomicU32::new(ASK);
static INSTALL_NAME:[AtomicU8;13]=[const{AtomicU8::new(0)};13];
static POLICY:AtomicU32=AtomicU32::new(ASK);
static STATE:AtomicU32=AtomicU32::new(0);
static TOKEN:AtomicU32=AtomicU32::new(0);
static DECISION:AtomicU32=AtomicU32::new(0);
static mut LOADED:bool=false;
static PLUGIN_POLICY:AtomicU32=AtomicU32::new(ASK);
static PLUGIN_MASK:AtomicU32=AtomicU32::new(1);
const PLUGIN_PATH:&[u8]=b"/data/chaos/connect-plugin.cfg\0";
const PLUGIN_TEMP:&[u8]=b"/data/chaos/connect-plugin.tmp\0";
static ANDROID_POLICY:AtomicU32=AtomicU32::new(ASK);
static ANDROID_MASK:AtomicU32=AtomicU32::new(1);
const ANDROID_PATH:&[u8]=b"/data/chaos/connect-android.cfg\0";
const ANDROID_TEMP:&[u8]=b"/data/chaos/connect-android.tmp\0";
pub(crate) const READ:u32=1;
pub(crate) const WRITE:u32=2;
pub(crate) const MANAGE:u32=4;
pub(crate) const DEVELOPER:u32=8;
pub(crate) const NSH:u32=32;
pub(crate) const CONTROL:u32=16; // Reserved: no control opcode is implemented yet.

const PATH:&[u8]=b"/data/chaos/connect-permissions.cfg\0";
const TEMP:&[u8]=b"/data/chaos/connect-permissions.tmp\0";

pub(crate) unsafe fn load(){
    if LOADED{return;}LOADED=true;
    let fd=fw_api::open(b"/data/chaos/connect-install.cfg\0".as_ptr(),fw_api::oflag::RDONLY,0);
    if fd>=0{let mut b=[0;5];let n=fw_api::read(fd,b.as_mut_ptr(),5);fw_api::close(fd);if n==5&&&b[..4]==b"CCI1"&&b[4]<=2{INSTALL_POLICY.store(b[4]as u32,Ordering::Release);}}
    let fd=fw_api::open(PLUGIN_PATH.as_ptr(),fw_api::oflag::RDONLY,0);
    if fd>=0 {
        let mut b=[0;7];let n=fw_api::read(fd,b.as_mut_ptr(),7);fw_api::close(fd);
        if n==7 && &b[..4]==b"CCL1" && b[4]<=2 && b[5]&!63==0 && b[6]==(b[4]^b[5]^0xa5) {
            PLUGIN_POLICY.store(b[4]as u32,Ordering::Release);PLUGIN_MASK.store(b[5]as u32,Ordering::Release);
        }
    }
    let fd=fw_api::open(ANDROID_PATH.as_ptr(),fw_api::oflag::RDONLY,0);
    if fd>=0 {
        let mut b=[0;7];let n=fw_api::read(fd,b.as_mut_ptr(),7);fw_api::close(fd);
        if n==7 && &b[..4]==b"CCA1" && b[4]<=2 && b[5]&!63==0 && b[6]==(b[4]^b[5]^0xa5) {
            ANDROID_POLICY.store(b[4]as u32,Ordering::Release);ANDROID_MASK.store(b[5]as u32,Ordering::Release);
        }
    }
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
pub(crate) fn disconnect(){
    STATE.store(0,Ordering::Release);TOKEN.store(0,Ordering::Release);DECISION.store(0,Ordering::Release);
}
pub(crate) fn pending()->u32{
    if state()&1!=0&&((state()>>2)&7==1 || prompt_is_connection() || prompt_is_install()){TOKEN.load(Ordering::Acquire)}else{0}
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

pub(crate) fn android_policy()->u32 {ANDROID_POLICY.load(Ordering::Acquire)}
pub(crate) fn android_mask()->u32 {ANDROID_MASK.load(Ordering::Acquire)}
pub(crate) unsafe fn set_android(policy:u32,mask:u32) {
    load();if policy>ASK || mask&!63!=0 {return;}
    let b=[b'C',b'C',b'A',b'1',policy as u8,mask as u8,(policy^mask^0xa5)as u8];
    let fd=fw_api::open(ANDROID_TEMP.as_ptr(),fw_api::oflag::WRONLY|fw_api::oflag::CREAT,0o640);
    let mut ok=false;
    if fd>=0 {ok=fw_api::write(fd,b.as_ptr(),7)==7;ok=fw_api::close(fd)==0 && ok;}
    if ok {ok=fw_api::capture_rename(ANDROID_TEMP.as_ptr(),ANDROID_PATH.as_ptr())==0;}
    if ok {ANDROID_POLICY.store(policy,Ordering::Release);ANDROID_MASK.store(mask,Ordering::Release);connect_log::line("Android权限已保存，由手环实时执行");}
    else {fw_api::fs_remove(ANDROID_TEMP.as_ptr());connect_log::line("Android权限保存失败，保持原策略");}
}
#[no_mangle] pub extern "C" fn chaos_connect_android_policy()->u32 {android_policy()}
#[no_mangle] pub extern "C" fn chaos_connect_android_mask()->u32 {android_mask()}
pub(crate) fn plugin_policy()->u32 {PLUGIN_POLICY.load(Ordering::Acquire)}
pub(crate) fn plugin_mask()->u32 {PLUGIN_MASK.load(Ordering::Acquire)}
pub(crate) unsafe fn set_plugin(policy:u32,mask:u32) {
    load();if policy>ASK || mask&!63!=0 {return;}
    let b=[b'C',b'C',b'L',b'1',policy as u8,mask as u8,(policy^mask^0xa5)as u8];
    let fd=fw_api::open(PLUGIN_TEMP.as_ptr(),fw_api::oflag::WRONLY|fw_api::oflag::CREAT,0o640);
    let mut ok=false;
    if fd>=0 {ok=fw_api::write(fd,b.as_ptr(),7)==7;ok=fw_api::close(fd)==0 && ok;}
    if ok {ok=fw_api::capture_rename(PLUGIN_TEMP.as_ptr(),PLUGIN_PATH.as_ptr())==0;}
    if ok {PLUGIN_POLICY.store(policy,Ordering::Release);PLUGIN_MASK.store(mask,Ordering::Release);connect_log::line("插件权限已保存，由手环实时执行");}
    else {fw_api::fs_remove(PLUGIN_TEMP.as_ptr());connect_log::line("插件权限保存失败，保持原策略");}
}
#[no_mangle] pub extern "C" fn chaos_connect_plugin_policy()->u32 {plugin_policy()}
#[no_mangle] pub extern "C" fn chaos_connect_plugin_mask()->u32 {plugin_mask()}
pub(crate) fn prompt_is_connection()->bool {(state()>>6)&7==1}

pub(crate) fn install_policy()->u32{INSTALL_POLICY.load(Ordering::Acquire)}
pub(crate) unsafe fn set_install_policy(value:u32){
    load();if value>ASK{return;}
    let b=[b'C',b'C',b'I',b'1',value as u8];
    let temp=b"/data/chaos/connect-install.tmp\0";let path=b"/data/chaos/connect-install.cfg\0";
    let fd=fw_api::open(temp.as_ptr(),fw_api::oflag::WRONLY|fw_api::oflag::CREAT,0o640);
    let mut ok=false;if fd>=0{let n=fw_api::write(fd,b.as_ptr(),5);ok=fw_api::close(fd)==0&&n==5;}
    if ok{ok=fw_api::capture_rename(temp.as_ptr(),path.as_ptr())==0;}
    if ok{INSTALL_POLICY.store(value,Ordering::Release);connect_log::line("Android安装资源包权限已保存");}else{fw_api::fs_remove(temp.as_ptr());}
}
#[no_mangle] pub extern "C" fn chaos_connect_install_policy()->u32{install_policy()}
#[no_mangle] pub unsafe extern "C" fn chaos_connect_install_name(p:*const u8,n:u32){
    for i in 0..13{INSTALL_NAME[i].store(if i<(n.min(12)as usize){*p.add(i)}else{0},Ordering::Release);}
}
pub(crate) fn prompt_is_install()->bool{state()&(1<<9)!=0}
pub(crate) fn install_name(out:&mut [u8;13]){for i in 0..13{out[i]=INSTALL_NAME[i].load(Ordering::Acquire);}}
