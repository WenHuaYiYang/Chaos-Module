//! 模糊V2参数：版本化二进制配置、范围校验及UI-owner延迟应用。
use crate::fw_api;
pub(crate) const COUNT: usize = 6;
pub(crate) const DEFAULTS: [u32; COUNT] = [12, 70, 44, 75, 38, 500];
pub(crate) const MIN: [u32; COUNT] = [1, 10, 0, 0, 0, 100];
pub(crate) const MAX: [u32; COUNT] = [40, 100, 100, 100, 100, 2000];
const CONFIG: &[u8] = b"/data/chaos/blur_v2_params.cfg\0";
static mut VALUES: [u32; COUNT] = DEFAULTS;
static mut LOADED: bool = false;
static mut DIRTY: bool = true;
static mut SAVE_FAILED: bool = false;
extern "C" { fn chaos_blur_v2_set_params(values: *const u32, count: u32) -> i32; }
fn valid(v: &[u32; COUNT]) -> bool { (0..COUNT).all(|i| v[i]>=MIN[i] && v[i]<=MAX[i]) }
fn encode(v: &[u32; COUNT]) -> [u8;17] {
    let mut out=[0u8;17];out[..4].copy_from_slice(b"BVP1");
    for i in 0..COUNT { out[4+i*2..6+i*2].copy_from_slice(&(v[i] as u16).to_le_bytes()); }
    out[16]=out[..16].iter().fold(0xa5,|a,b|a^b);out
}
fn decode(data: &[u8;17]) -> Option<[u32;COUNT]> {
    if &data[..4]!=b"BVP1" || data[..16].iter().fold(0xa5,|a,b|a^b)!=data[16] { return None; }
    let mut v=DEFAULTS;
    for i in 0..COUNT { v[i]=u16::from_le_bytes([data[4+i*2],data[5+i*2]]) as u32; }
    if valid(&v) {Some(v)} else {None}
}
unsafe fn load() {
    if LOADED {return;}LOADED=true;
    let fd=fw_api::open(CONFIG.as_ptr(),fw_api::oflag::RDONLY,0);
    if fd<0 {return;}
    let mut data=[0u8;17];let mut n=0;
    while n<data.len() {
        let got=fw_api::read(fd,data.as_mut_ptr().add(n),(data.len()-n) as u32);
        if got<=0 || got as usize>data.len()-n {break;}n+=got as usize;
    }
    fw_api::close(fd);
    if n==data.len() {if let Some(v)=decode(&data) {VALUES=v;}}
}
unsafe fn persist() -> bool {
    let data=encode(&VALUES);
    let fd=fw_api::open(CONFIG.as_ptr(),fw_api::oflag::WRONLY|fw_api::oflag::CREAT,0o640);
    if fd<0 {return false;}
    let mut n=0;
    while n<data.len() {
        let got=fw_api::write(fd,data.as_ptr().add(n),(data.len()-n) as u32);
        if got<=0 || got as usize>data.len()-n {break;}n+=got as usize;
    }
    let closed=fw_api::close(fd);n==data.len() && closed==0
}
pub(crate) unsafe fn get(index: usize) -> u32 {load();if index<COUNT {VALUES[index]} else {0}}
pub(crate) unsafe fn set(index: usize,value: u32) -> bool {
    load();if index>=COUNT || value<MIN[index] || value>MAX[index] {return false;}
    if VALUES[index]!=value {VALUES[index]=value;DIRTY=true;}
    SAVE_FAILED=!persist();!SAVE_FAILED
}
pub(crate) unsafe fn reset() -> bool {load();VALUES=DEFAULTS;DIRTY=true;SAVE_FAILED=!persist();!SAVE_FAILED}
pub(crate) unsafe fn apply() {
    load();if !DIRTY {return;}
    if chaos_blur_v2_set_params(core::ptr::addr_of!(VALUES).cast(),COUNT as u32)==0 {DIRTY=false;}
}
pub(crate) unsafe fn save_failed() -> bool {SAVE_FAILED}

/// The frame/poll bridge uses the existing verified screen-state accessor.
#[no_mangle]
pub unsafe extern "C" fn chaos_blur_screen_on() -> u32 { fw_api::screen_is_on() as u32 }
