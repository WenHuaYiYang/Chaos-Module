//! Runs the real Rust network ABI wrapper with deterministic host leaves.
#![allow(dead_code, unused_imports)]
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
#[path = "../src/fw_api/network.rs"]
mod network;
pub type Cp = *const u8;
pub mod fwaddr {}
pub mod oflag { pub const RDONLY: u32 = 1; }
pub type Entry = unsafe extern "C" fn(i32,*mut *mut u8)->i32;
static TEST_LOCK: Mutex<()> = Mutex::new(());
static MOCK: OnceLock<Mutex<Mock>> = OnceLock::new();
enum Read { Bytes(Vec<u8>), Error(i32), InvalidCount(i32) }
struct Mock {
    clock: (i64,i32,i32), errno: i32, open_rc: i32, reads: VecDeque<Read>, read_calls: usize,
    closes: Vec<i32>, paths: Vec<Vec<u8>>, spawn_rc: i32, spawns: Vec<(Vec<u8>,Entry)>,
    waits: Vec<i32>, wait_results: VecDeque<i32>, system_commands: Vec<Vec<u8>>, system_rc: i32,
}
impl Default for Mock { fn default()->Self { Self {
    clock:(123,456_000_000,0),errno:5,open_rc:7,reads:VecDeque::new(),read_calls:0,
    closes:vec![],paths:vec![],spawn_rc:300,spawns:vec![],waits:vec![],wait_results:VecDeque::new(),
    system_commands:vec![],system_rc:0,
} } }
fn with<T>(f:impl FnOnce(&mut Mock)->T)->T { f(&mut MOCK.get_or_init(||Mutex::new(Mock::default())).lock().unwrap_or_else(|p|p.into_inner())) }
fn network_mock_errno()->i32 { with(|m|m.errno) }
fn network_mock_clock(sec:&mut i64,nsec:&mut i32)->i32 {with(|m|{*sec=m.clock.0;*nsec=m.clock.1;m.clock.2})}
fn network_mock_system(command:&[u8])->i32 {with(|m|{m.system_commands.push(command.to_vec());m.system_rc})}
unsafe fn open(path:Cp,flags:u32,mode:u32)->i32 {assert_eq!(flags,1);assert_eq!(mode,0);with(|m|{m.paths.push(std::ffi::CStr::from_ptr(path.cast()).to_bytes().to_vec());m.open_rc})}
unsafe fn read(fd:i32,out:*mut u8,capacity:u32)->i32 {with(|m|{
    assert_eq!(fd,m.open_rc);m.read_calls+=1;
    match m.reads.pop_front() {
        Some(Read::Bytes(mut b))=>{let n=b.len().min(capacity as usize);core::ptr::copy_nonoverlapping(b.as_ptr(),out,n);if n<b.len(){b.drain(..n);m.reads.push_front(Read::Bytes(b));}n as i32},
        Some(Read::Error(n))|Some(Read::InvalidCount(n))=>n,None=>0,
    }
})}
unsafe fn close(fd:i32)->i32 {with(|m|m.closes.push(fd));0}
unsafe fn nsh_spawn(name:Cp,entry:Entry,actions:*mut usize)->i32 {assert!(actions.is_null());with(|m|{m.spawns.push((std::ffi::CStr::from_ptr(name.cast()).to_bytes().to_vec(),entry));m.spawn_rc})}
unsafe fn nsh_waitpid_nohang(pid:i32,status:&mut i32)->i32 {with(|m|{m.waits.push(pid);*status=0;m.wait_results.pop_front().unwrap_or(0)})}
