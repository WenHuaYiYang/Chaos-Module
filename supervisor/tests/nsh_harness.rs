//! Actual backend source with deterministic host firmware mocks, no device I/O.
#![allow(dead_code)]
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
#[path = "../src/nsh.rs"]
mod nsh;
static TEST_LOCK: Mutex<()> = Mutex::new(());
static MOCK: OnceLock<Mutex<Mock>> = OnceLock::new();

#[derive(Clone)]
enum Read { Bytes(Vec<u8>), Error(i32) }
struct Mock {
    worker_result: i32, child_result: i32, master_result: i32, prepare_result: i32, slave_result: i32,
    init_result: i32, add_fail: Option<usize>, add_attempts: usize, opens: usize, action_inits: usize,
    spawns: Vec<(Vec<u8>, fw_api::NshTaskEntry, bool)>, actions: Vec<(i32, i32)>, destroys: usize,
    closed: Vec<i32>, waits: Vec<i32>, wait_results: VecDeque<i32>, wait_status: i32,
    reads: VecDeque<Read>, read_calls: usize, writes: Vec<Vec<u8>>, written: Vec<u8>, write_results: VecDeque<i32>,
    console: usize, original_exit: usize, callback: Option<fw_api::NshExit>, run_result: i32, callback_in_run: bool,
    native_exits: Vec<(usize, usize, i32)>, freed: Vec<usize>, events: Vec<String>, sleeps: Vec<u32>,
}
impl Default for Mock {
    fn default() -> Self { Self {
        worker_result: 100, child_result: 200, master_result: 5, prepare_result: 7, slave_result: 6,
        init_result: 0, add_fail: None, add_attempts: 0, opens: 0, action_inits: 0, spawns: vec![], actions: vec![], destroys: 0,
        closed: vec![], waits: vec![], wait_results: VecDeque::new(), wait_status: 0, reads: VecDeque::new(), read_calls: 0,
        writes: vec![], written: vec![], write_results: VecDeque::new(), console: 0x12345000,
        original_exit: 0x1234, callback: None, run_result: 0, callback_in_run: false,
        native_exits: vec![], freed: vec![], events: vec![], sleeps: vec![],
    } }
}
fn with<T>(f: impl FnOnce(&mut Mock) -> T) -> T {
    f(&mut MOCK.get_or_init(|| Mutex::new(Mock::default())).lock().unwrap_or_else(|p| p.into_inner()))
}
mod fw_api {
    pub type NshTaskEntry = unsafe extern "C" fn(i32, *mut *mut u8) -> i32;
    pub type NshExit = unsafe extern "C" fn(usize, i32);
    pub unsafe fn nsh_spawn(name: *const u8, entry: NshTaskEntry, _actions: *mut usize) -> i32 {
        let name = std::ffi::CStr::from_ptr(name.cast()).to_bytes().to_vec();
        crate::with(|m| {
            // worker 现在也带一个空 actions 链, 不能再按 actions 是否 NULL 区分角色。
            let is_worker = name.as_slice() == b"chaos.nsh.worker";
            m.spawns.push((name, entry, is_worker));
            if is_worker { m.worker_result } else { m.child_result }
        })
    }
    pub mod oflag { pub const RDWR: u32 = 3; }
    pub unsafe fn open(_: *const u8, _: u32, _: u32) -> i32 {
        crate::with(|m| { m.opens += 1; 10 + m.opens as i32 })
    }
    pub unsafe fn nsh_master_open() -> i32 { crate::with(|m| m.master_result) }
    pub unsafe fn nsh_master_prepare(fd: i32) -> i32 { crate::with(|m| { assert_eq!(fd, m.master_result); m.prepare_result }) }
    pub unsafe fn nsh_slave_open(index: i32) -> i32 { crate::with(|m| { assert_eq!(index, m.prepare_result); m.slave_result }) }
    pub unsafe fn nsh_close(fd: i32) { crate::with(|m| { m.closed.push(fd); m.events.push(format!("close:{fd}")); }); }
    pub unsafe fn nsh_actions_init(head: *mut usize) -> i32 { crate::with(|m| {
        // 第一次是 worker 的空链(必须成功), 之后才是 session 的链(可注入失败)。
        let attempt = m.action_inits; m.action_inits += 1;
        let rc = if attempt == 0 { 0 } else { m.init_result };
        if rc == 0 { *head = 1; }
        rc
    }) }
    pub unsafe fn nsh_actions_adddup2(head: *mut usize, from: i32, to: i32) -> i32 { crate::with(|m| {
        assert_eq!(*head, 1); let attempt = m.add_attempts; m.add_attempts += 1;
        if m.add_fail == Some(attempt) { return -12; }
        m.actions.push((from, to)); 0
    }) }
    pub unsafe fn nsh_actions_destroy(head: *mut usize) { crate::with(|m| {
        if *head == 0 { return; } // 空链 head 为 0 时固件 destroy 是 no-op
        assert_eq!(*head, 1); *head = 0; m.destroys += 1; m.events.push("actions_destroy".into());
    }); }
    pub unsafe fn nsh_read(_: i32, out: &mut [u8]) -> i32 { crate::with(|m| {
        m.read_calls += 1;
        match m.reads.pop_front() {
            Some(crate::Read::Bytes(mut data)) => {
                let n = data.len().min(out.len()); out[..n].copy_from_slice(&data[..n]);
                if n < data.len() { data.drain(..n); m.reads.push_front(crate::Read::Bytes(data)); }
                n as i32
            },
            Some(crate::Read::Error(e)) => e, None => -11,
        }
    }) }
    pub unsafe fn nsh_write(_: i32, data: &[u8]) -> i32 { crate::with(|m| {
        m.writes.push(data.to_vec());
        let n = m.write_results.pop_front().unwrap_or(data.len() as i32);
        if n > 0 { m.written.extend_from_slice(&data[..(n as usize).min(data.len())]); }
        n
    }) }
    pub unsafe fn nsh_waitpid_nohang(pid: i32, status: &mut i32) -> i32 { crate::with(|m| {
        m.waits.push(pid); *status = m.wait_status; let result = m.wait_results.pop_front().unwrap_or(0);
        m.events.push(format!("wait:{pid}:{result}")); result
    }) }
    pub unsafe fn nsh_console_new() -> usize { crate::with(|m| m.console) }
    pub unsafe fn nsh_console_get_exit(console: usize) -> usize { crate::with(|m| { assert_eq!(console, m.console); m.original_exit }) }
    pub unsafe fn nsh_console_set_exit(console: usize, callback: NshExit) { crate::with(|m| { assert_eq!(console, m.console); m.callback = Some(callback); }); }
    pub unsafe fn nsh_console_run(console: usize, argc: i32, _: *mut *mut u8) -> i32 {
        assert_eq!(argc, 1);
        let (callback, invoke, rc) = crate::with(|m| (m.callback, m.callback_in_run, m.run_result));
        if invoke { callback.unwrap()(console, rc); }
        rc
    }
    pub unsafe fn nsh_console_call_exit(callback: usize, console: usize, result: i32) { crate::with(|m| {
        assert_eq!(callback, m.original_exit); m.native_exits.push((callback, console, result));
        m.events.push("native_exit".into());
    }); }
    pub unsafe fn nsh_console_free_after_exit(console: usize) { crate::with(|m| {
        m.freed.push(console); m.events.push(format!("free:{console}"));
    }); }
    pub unsafe fn nsh_usleep(usec: u32) -> i32 { crate::with(|m| m.sleeps.push(usec)); 0 }
}
