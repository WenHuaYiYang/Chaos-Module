#![allow(dead_code, static_mut_refs, unsafe_op_in_unsafe_fn)]

use std::sync::{Mutex, OnceLock};

pub use core::ptr::{read_volatile, write_volatile};
pub const FW_STYLE_HR_NAME: u32 = 1;
pub const APP_ID: u32 = 200;
pub static mut FG_PAGE: u32 = 34;

#[derive(Clone, Copy)]
pub struct PageState { pub built: u32, pub content: u32, pub rows: [u32; 12] }
impl PageState { pub const EMPTY: Self = Self { built: 0, content: 0, rows: [0; 12] }; }
pub static mut PAGES: [PageState; 35] = [PageState::EMPTY; 35];

#[repr(C)] #[derive(Clone, Copy)] pub struct MockDevice { pub address: [u8;6], pub rssi: i8, pub name: [u8;21] }
#[repr(C)] #[derive(Clone, Copy)] pub struct MockPeer { pub device: MockDevice, pub probe_index: u32 }
#[repr(C)] #[derive(Clone, Copy)] pub struct MockNetwork {
    pub phase:u32, pub count:u32, pub checked:u32, pub candidates:u32, pub selected:u32,
    pub passkey:u32, pub error_serial:u32, pub error:i32, pub error_stage:u32,
    pub cid:u16, pub mtu:u16, pub connected_name:[u8;32], pub address:[u8;6],
    pub peers:[MockPeer;16], pub enabled:u32,
}
#[repr(C)] #[derive(Clone, Copy)] pub struct MockIpInfo { pub ip:[u8;4], pub mask:[u8;4], pub gateway:[u8;4], pub dns:[u8;4], pub mac:[u8;6], pub mtu:u16, pub ready:u32 }
#[repr(C)] #[derive(Clone, Copy)] pub struct MockHttp { pub enabled:u32, pub listening:u32, pub clients:u32, pub requests:u32, pub failures:u32, pub refused:u32, pub last_error:i32 }
#[repr(C)] #[derive(Clone, Copy)] pub struct MockDiag { pub adapter_state:u32, pub scan_stage:u32, pub scan_events:u32, pub scan_seen_start:u32, pub command:u32, pub ipc_result:i32, pub service_result:i32, pub sys_errno:i32, pub client_state:u32, pub client_fd:i32, pub l2_event:u32, pub l2_events:u32, pub error:i32, pub error_stage:u32 }
#[derive(Clone, Debug, PartialEq, Eq)] pub struct RowUpdate { pub row:u32, pub main:Vec<u8>, pub sub:Vec<u8>, pub selected:u8 }
pub struct HostState {
    pub network:MockNetwork, pub ip:MockIpInfo, pub http:MockHttp, pub diag:MockDiag,
    pub pan_enable_calls:usize, pub scan_calls:usize, pub connect_calls:Vec<u32>, pub disconnect_calls:usize, pub pair_calls:usize,
    pub auto_apply_pan:bool, pub spawn_result:i32, pub spawn_calls:usize, pub row_create_calls:usize, pub fail_row_at:Option<usize>, pub next_handle:u32,
    pub fail_label:bool, pub next_label:u32, pub row_updates:Vec<RowUpdate>, pub label_updates:Vec<(u32,Vec<u8>)>,
    pub credentials:Option<Vec<u8>>, pub credential_set_calls:usize, pub http_enable_calls:Vec<i32>, pub screen_on:bool, pub live_pages:[bool;35], pub now_ms:u32,
}
impl HostState { fn fresh()->Self { Self {
    network:unsafe{core::mem::zeroed()}, ip:unsafe{core::mem::zeroed()}, http:unsafe{core::mem::zeroed()}, diag:unsafe{core::mem::zeroed()},
    pan_enable_calls:0, scan_calls:0, connect_calls:Vec::new(), disconnect_calls:0, pair_calls:0, auto_apply_pan:true, spawn_result:1, spawn_calls:0,
    row_create_calls:0, fail_row_at:None, next_handle:100, fail_label:false, next_label:10000, row_updates:Vec::new(), label_updates:Vec::new(),
    credentials:None, credential_set_calls:0, http_enable_calls:Vec::new(), screen_on:true, live_pages:[true;35], now_ms:0,
} } }
static HOST:OnceLock<Mutex<HostState>>=OnceLock::new();
pub fn host_state()->&'static Mutex<HostState>{HOST.get_or_init(||Mutex::new(HostState::fresh()))}
pub fn with_host(f:impl FnOnce(&mut HostState)){f(&mut host_state().lock().unwrap())}
pub fn take_row_updates()->Vec<RowUpdate>{std::mem::take(&mut host_state().lock().unwrap().row_updates)}
pub fn take_label_updates()->Vec<(u32,Vec<u8>)>{std::mem::take(&mut host_state().lock().unwrap().label_updates)}
fn read_cstr(p:*const u8, cap:usize)->Vec<u8>{if p.is_null(){return Vec::new();}unsafe{let mut v=Vec::new();for i in 0..cap{let b=read_volatile(p.add(i));if b==0{break;}v.push(b);}v}}

pub fn reset_mock() {
    *host_state().lock().unwrap()=HostState::fresh();
    unsafe {
        PAGES = [PageState::EMPTY; 35];
        FG_PAGE = 34;
    }
}

pub unsafe fn safe_ptr(p: u32) -> bool { p != 0 }
pub fn page_is_live(pid: usize) -> bool { host_state().lock().unwrap().live_pages[pid.min(34)] }
pub unsafe fn put_str(dst: *mut u8, src: &[u8]) { for (i, b) in src.iter().enumerate() { write_volatile(dst.add(i), *b); } }
pub fn nav_goto(pid: u32, _: u32) { unsafe { FG_PAGE = pid; } }
pub fn nav_back() { unsafe { FG_PAGE = 0; } }

pub struct PageCtx { pub content: u32, pub title: u32, pub rows: *mut u32 }
pub type PageResult = Result<(), PageError>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum PageError { RowFailed(usize) }

#[derive(Clone, Copy)]
pub struct RowSpec<'a> { pub text: &'a [u8], pub sub: *const u8, pub trailing: u32, pub clickable: bool }
impl<'a> RowSpec<'a> {
    pub fn display(text: &'a [u8], sub: *const u8) -> Self { Self { text, sub, trailing: 0, clickable: false } }
    pub fn tap(text: &'a [u8], sub: *const u8) -> Self { Self { text, sub, trailing: 0, clickable: true } }
    pub fn switch(text: &'a [u8]) -> Self { Self { text, sub: core::ptr::null(), trailing: fw_api::trailing::SWITCH, clickable: true } }
}
pub unsafe fn build_row(ctx: &PageCtx, _: u32, slot: usize, _: &RowSpec) -> u32 {
    if slot >= 12 { return 0; }
    let row = (slot + 1) as u32;
    write_volatile(ctx.rows.add(slot), row);
    row
}

pub mod text {
    pub struct W { buf: *mut u8, cap: usize, len: usize }
    impl W {
        pub fn new(buf: *mut u8, cap: usize) -> Self { Self { buf, cap, len: 0 } }
        pub unsafe fn s(&mut self, s: &[u8]) -> &mut Self { for &b in s { self.c(b); } self }
        pub unsafe fn c(&mut self, b: u8) -> &mut Self { if self.len + 1 < self.cap { *self.buf.add(self.len) = b; self.len += 1; } self }
        pub unsafe fn n(&mut self, mut n: u32) -> &mut Self { let mut d = [0u8; 10]; let mut i = 0; loop { d[i] = b'0' + (n % 10) as u8; i += 1; n /= 10; if n == 0 { break; } } while i != 0 { i -= 1; self.c(d[i]); } self }
        pub unsafe fn end(&mut self) { if self.cap != 0 { *self.buf.add(self.len.min(self.cap - 1)) = 0; } }
    }
}

pub mod fw_api {
    pub const MAX_ROWS_PER_PAGE: usize = 12;
    pub const NONE_PTR: *const u8 = core::ptr::null();
    pub const LV_DIR_HOR: u32 = 3;
    pub mod trailing { pub const SWITCH: u32 = 1; }
    pub mod align { pub const OUT_BOTTOM_LEFT: u32 = 1; pub const OUT_BOTTOM_MID: u32 = 2; }
    pub fn screen_is_on() -> bool { crate::host_state().lock().unwrap().screen_on }
    pub fn top_page_ids() -> (u32, u32) { (crate::APP_ID, unsafe { crate::FG_PAGE }) }
    pub unsafe fn row_update(row:u32, _: *const u8, main:*const u8, sub:*const u8, _:u32, selected:u8) { crate::host_state().lock().unwrap().row_updates.push(crate::RowUpdate { row, main:crate::read_cstr(main,128), sub:crate::read_cstr(sub,256), selected }); }
    pub fn row_create(_: u32, _: *const u8, _: *const u8, _: u32) -> u32 { let mut s=crate::host_state().lock().unwrap(); s.row_create_calls+=1; if s.fail_row_at==Some(s.row_create_calls){0}else{let h=s.next_handle;s.next_handle=s.next_handle.saturating_add(1);h} }
    pub fn row_trailing(_: u32) -> u32 { 0 }
    pub fn obj_add_event(_: u32, _: u32, _: u32, _: u32) {}
    pub fn obj_align_to(_: u32, _: u32, _: u32, _: i32, _: i32) {}
    pub fn label_create(_: u32) -> u32 { let mut s=crate::host_state().lock().unwrap(); if s.fail_label{0}else{let h=s.next_label;s.next_label+=1;h} }
    pub fn style_apply(_: u32, _: u32, _: u32, _: u32) {}
    pub fn obj_set_width(_: u32, _: i32) {}
    pub unsafe fn label_set_text(label:u32, text:*const u8) { crate::host_state().lock().unwrap().label_updates.push((label,crate::read_cstr(text,2048))); }
    pub unsafe fn nsh_spawn(_: *const u8, _: unsafe extern "C" fn(i32, *mut *mut u8) -> i32, _: *mut usize) -> i32 { let mut s=crate::host_state().lock().unwrap();s.spawn_calls+=1;s.spawn_result }
    pub fn network_monotonic_ms() -> u32 { crate::host_state().lock().unwrap().now_ms }
    pub fn nsh_usleep(_: u32) {}
}

pub mod page { pub trait Page: Sync { fn fill_title(&self, _: *mut u8); fn render(&self, _: &crate::PageCtx) -> crate::PageResult; fn on_click(&self, _: usize); fn is_switch_row(&self, _: usize) -> bool { false } fn on_switch(&self, _: usize) {} fn needs_tick(&self) -> bool { false } fn tick(&self) {} } }
pub mod keyboard { #[derive(Clone, Copy)] pub enum Completion { WebPassword } pub unsafe fn open(_: *const u8, _: Completion) {} }

#[no_mangle] pub extern "C" fn chaos_pan_enable(on:i32) { let mut s=host_state().lock().unwrap();s.pan_enable_calls+=1;if s.auto_apply_pan{s.network.enabled=(on!=0)as u32;} }
#[no_mangle] pub extern "C" fn chaos_pan_scan() { host_state().lock().unwrap().scan_calls+=1; }
#[no_mangle] pub extern "C" fn chaos_pan_connect(index:u32) { host_state().lock().unwrap().connect_calls.push(index); }
#[no_mangle] pub extern "C" fn chaos_pan_disconnect() { host_state().lock().unwrap().disconnect_calls+=1; }
#[no_mangle] pub extern "C" fn chaos_pan_pair_confirm() { host_state().lock().unwrap().pair_calls+=1; }
#[no_mangle] pub unsafe extern "C" fn network_service_view(out:*mut u8) { let s=host_state().lock().unwrap();core::ptr::copy_nonoverlapping((&s.network as *const MockNetwork).cast::<u8>(),out,596); }
#[no_mangle] pub unsafe extern "C" fn pan_ip_info(out:*mut u8) { let s=host_state().lock().unwrap();core::ptr::copy_nonoverlapping((&s.ip as *const MockIpInfo).cast::<u8>(),out,28); }
#[no_mangle] pub extern "C" fn chaos_pan_bt_poll() {}
#[no_mangle] pub extern "C" fn chaos_pan_net_poll(_: u32) {}
#[no_mangle] pub extern "C" fn chaos_net_set_credentials(_: *const u8, pass:*const u8) -> i32 { let mut s=host_state().lock().unwrap();s.credential_set_calls+=1;s.credentials=Some(read_cstr(pass,64));0 }
#[no_mangle] pub unsafe extern "C" fn chaos_web_credentials(user:*mut u8,ucap:u32,pass:*mut u8,pcap:u32)->i32 { let s=host_state().lock().unwrap();let Some(p)=s.credentials.as_ref() else{return -1;};let u=b"chaos";for(i,b)in u.iter().enumerate().take(ucap.saturating_sub(1)as usize){*user.add(i)=*b;}*user.add(u.len().min(ucap.saturating_sub(1)as usize))=0;for(i,b)in p.iter().enumerate().take(pcap.saturating_sub(1)as usize){*pass.add(i)=*b;}*pass.add(p.len().min(pcap.saturating_sub(1)as usize))=0;0 }
#[no_mangle] pub extern "C" fn chaos_httpd_set_enable(on:i32) { let mut s=host_state().lock().unwrap();s.http_enable_calls.push(on);s.http.enabled=(on!=0)as u32; }
#[no_mangle] pub extern "C" fn chaos_httpd_enabled()->i32 { host_state().lock().unwrap().http.enabled as i32 }
#[no_mangle] pub unsafe extern "C" fn chaos_httpd_snapshot(out:*mut u8) { let s=host_state().lock().unwrap();core::ptr::copy_nonoverlapping((&s.http as *const MockHttp).cast::<u8>(),out,28); }
#[no_mangle] pub unsafe extern "C" fn bt_probe_diagnostics(out:*mut u8) { let s=host_state().lock().unwrap();core::ptr::copy_nonoverlapping((&s.diag as *const MockDiag).cast::<u8>(),out,56); }

#[path = "../src/btpan.rs"] mod btpan;
