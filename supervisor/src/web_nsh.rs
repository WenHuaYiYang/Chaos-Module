//! Independent p67 / 3.101.043 NSH for the authenticated web console.
//! The PAN worker is the ONLY caller/producer/consumer of the public API.
//! A separate resident task owns this PTY; none of the local UI NSH state or
//! queues is shared. See docs/WEB-NSH.md for the asynchronous stop contract.

use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicUsize, Ordering};
#[path = "nsh_queue.rs"]
mod queue;
use queue::Spsc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum State { Idle, Starting, Running, Exited, Failed, Stopping }

const EINTR: i32 = -4;
const EIO: i32 = -5;
const EBADF: i32 = -9;
const EAGAIN: i32 = -11;
const ENOMEM: i32 = -12;
const EBUSY: i32 = -16;
const EINVAL: i32 = -22;
const EPIPE: i32 = -32;
const EMSGSIZE: i32 = -90;
const ENOTSUP: i32 = -95;
const RX_CAP: usize = 8192;
const TX_CAP: usize = 2048;
const IO_CHUNK: usize = 256;
const LIVE_READ_BUDGET: usize = 8;
const TAIL_READ_BUDGET: usize = 64;
const ACTIVE_SLEEP_US: u32 = 20_000;
const IDLE_SLEEP_US: u32 = 200_000;
const TASK_NAME: &[u8] = b"chaos.web.nsh\0";
const WORKER_NAME: &[u8] = b"chaos.web.nsh.worker\0";

// p67 readline accepts BS and limits its command line to 255 bytes. Never
// prepend LF to exit: that could execute an unfinished command. Ctrl-C goes
// through the verified stock PTY foreground-PID path; BS clears any partial
// shell line when readline resumes. A command ignoring SIGINT may not exit.
const STOP_SEQUENCE: [u8; 262] = {
    let mut bytes = [8u8; 262];
    bytes[0] = 3;
    bytes[257] = b'e'; bytes[258] = b'x'; bytes[259] = b'i';
    bytes[260] = b't'; bytes[261] = b'\n';
    bytes
};

static STATE: AtomicU8 = AtomicU8::new(State::Idle as u8);
static API_ERROR: AtomicI32 = AtomicI32::new(0);
static SESSION_ERROR: AtomicI32 = AtomicI32::new(0);
static WORKER_STARTED: AtomicBool = AtomicBool::new(false);
static REQUEST: AtomicBool = AtomicBool::new(false);
static STOP_REQUEST: AtomicBool = AtomicBool::new(false);
static CHILD_DONE: AtomicBool = AtomicBool::new(false);
static CHILD_EXIT: AtomicI32 = AtomicI32::new(0);
static CHILD_CONSOLE: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_EXIT: AtomicUsize = AtomicUsize::new(0);
static RX: Spsc<RX_CAP> = Spsc::new();
static TX: Spsc<TX_CAP> = Spsc::new();
static DROPPED: AtomicUsize = AtomicUsize::new(0);
static NOTICE_COUNT: AtomicUsize = AtomicUsize::new(0);
static NOTICE_OFFSET: AtomicUsize = AtomicUsize::new(0);

fn state() -> State {
    match STATE.load(Ordering::Acquire) {
        0 => State::Idle, 1 => State::Starting, 2 => State::Running,
        3 => State::Exited, 5 => State::Stopping, _ => State::Failed,
    }
}
fn error() -> i32 {
    let api = API_ERROR.load(Ordering::Acquire);
    if api != 0 { api } else { SESSION_ERROR.load(Ordering::Acquire) }
}
fn api_error(code: i32) -> Result<(), i32> {
    API_ERROR.store(code, Ordering::Release);
    Err(code)
}
fn start() -> Result<(), i32> {
    let current = state();
    if current == State::Stopping { return api_error(EBUSY); }
    if matches!(current, State::Starting | State::Running) { return Ok(()); }
    if STATE.compare_exchange(current as u8, State::Starting as u8,
        Ordering::AcqRel, Ordering::Acquire).is_err() { return api_error(EBUSY); }
    // Final-state publication guarantees the previous worker has released
    // all ring/fd ownership before the sole PAN caller resets these queues.
    unsafe { RX.reset_quiescent(); TX.reset_quiescent(); }
    DROPPED.store(0, Ordering::Relaxed);
    NOTICE_COUNT.store(0, Ordering::Relaxed);
    NOTICE_OFFSET.store(0, Ordering::Relaxed);
    API_ERROR.store(0, Ordering::Release);
    SESSION_ERROR.store(0, Ordering::Release);
    STOP_REQUEST.store(false, Ordering::Relaxed);
    REQUEST.store(true, Ordering::Release);
    if !WORKER_STARTED.swap(true, Ordering::AcqRel) {
        let pid = unsafe { crate::fw_api::nsh_spawn(WORKER_NAME.as_ptr(), worker_entry, core::ptr::null_mut()) };
        if pid <= 0 {
            let code = if pid < 0 { pid } else { EIO };
            REQUEST.store(false, Ordering::Release);
            WORKER_STARTED.store(false, Ordering::Release);
            SESSION_ERROR.store(code, Ordering::Release);
            STATE.store(State::Failed as u8, Ordering::Release);
            return Err(code);
        }
    }
    Ok(())
}
fn send(input: &[u8]) -> Result<(), i32> {
    if input.len() > TX_CAP { return api_error(EMSGSIZE); }
    if input.iter().any(|&b| b == 0) { return api_error(EINVAL); }
    if input.iter().any(|&b| b == 4) { return api_error(ENOTSUP); }
    if input.is_empty() { API_ERROR.store(0, Ordering::Release); return Ok(()); }
    if !matches!(state(), State::Starting | State::Running) { return api_error(EBUSY); }
    let failure = SESSION_ERROR.load(Ordering::Acquire);
    if failure != 0 { return api_error(failure); }
    if !unsafe { TX.push_all(input) } { return api_error(EBUSY); }
    API_ERROR.store(0, Ordering::Release);
    Ok(())
}
fn stop() {
    loop {
        let current = state();
        if !matches!(current, State::Starting | State::Running) { return; }
        if STATE.compare_exchange(current as u8, State::Stopping as u8,
            Ordering::AcqRel, Ordering::Acquire).is_ok() {
            // The PAN caller can no longer send. Publish after closing that
            // gate; the worker alone discards any queued/in-flight TX suffix.
            API_ERROR.store(0, Ordering::Release);
            STOP_REQUEST.store(true, Ordering::Release);
            return;
        }
    }
}

fn overflow_note(out: &mut [u8], count: usize) -> usize {
    let mut text = [0u8; 72];
    let prefix = b"\n[Web NSH: dropped ";
    let suffix = b" output bytes]\n";
    text[..prefix.len()].copy_from_slice(prefix);
    let mut digits = [0u8; 20];
    let mut n = count;
    let mut first = digits.len();
    loop {
        first -= 1; digits[first] = b'0' + (n % 10) as u8; n /= 10;
        if n == 0 { break; }
    }
    let end = prefix.len() + digits.len() - first;
    text[prefix.len()..end].copy_from_slice(&digits[first..]);
    text[end..end + suffix.len()].copy_from_slice(suffix);
    let length = end + suffix.len();
    let offset = NOTICE_OFFSET.load(Ordering::Relaxed);
    let got = out.len().min(length - offset);
    out[..got].copy_from_slice(&text[offset..offset + got]);
    if offset + got == length {
        NOTICE_OFFSET.store(0, Ordering::Relaxed);
        NOTICE_COUNT.store(0, Ordering::Relaxed);
    } else { NOTICE_OFFSET.store(offset + got, Ordering::Relaxed); }
    got
}
fn receive(out: &mut [u8]) -> usize {
    if out.is_empty() { return 0; }
    let pending = NOTICE_COUNT.load(Ordering::Relaxed);
    if pending != 0 { return overflow_note(out, pending); }
    let got = unsafe { RX.pop(out) };
    if got != 0 { return got; }
    let dropped = DROPPED.swap(0, Ordering::AcqRel);
    if dropped == 0 { return 0; }
    NOTICE_COUNT.store(dropped, Ordering::Relaxed);
    overflow_note(out, dropped)
}

unsafe extern "C" fn shell_exit(console: usize, rc: i32) {
    CHILD_EXIT.store(rc, Ordering::Relaxed);
    CHILD_DONE.store(true, Ordering::Release);
    let callback = ORIGINAL_EXIT.swap(0, Ordering::AcqRel);
    if callback != 0 {
        // Stock callback frees console and never normally returns. A signal
        // in the clear-to-free window may leak, but cannot cause double-free.
        CHILD_CONSOLE.store(0, Ordering::Release);
        crate::fw_api::nsh_console_call_exit(callback, console, rc);
    }
}
unsafe extern "C" fn shell_entry(argc: i32, argv: *mut *mut u8) -> i32 {
    let console = crate::fw_api::nsh_console_new();
    if console == 0 {
        CHILD_EXIT.store(ENOMEM, Ordering::Relaxed);
        CHILD_DONE.store(true, Ordering::Release);
        return ENOMEM;
    }
    CHILD_CONSOLE.store(console, Ordering::Release);
    let original = crate::fw_api::nsh_console_get_exit(console);
    if original == 0 {
        CHILD_EXIT.store(EIO, Ordering::Relaxed);
        CHILD_DONE.store(true, Ordering::Release);
        return EIO;
    }
    ORIGINAL_EXIT.store(original, Ordering::Release);
    crate::fw_api::nsh_console_set_exit(console, shell_exit);
    let rc = crate::fw_api::nsh_console_run(console, argc, argv);
    if !CHILD_DONE.load(Ordering::Acquire) { shell_exit(console, rc); }
    rc
}

struct Fd(i32);
impl Drop for Fd {
    fn drop(&mut self) { unsafe { crate::fw_api::nsh_close(self.0); } }
}
struct Actions(usize);
impl Drop for Actions {
    fn drop(&mut self) { unsafe { crate::fw_api::nsh_actions_destroy(&mut self.0); } }
}
struct Session {
    master: Fd, child: i32, pending: [u8; IO_CHUNK],
    pending_len: usize, pending_offset: usize, failure: i32,
    write_failed: bool, stopping: bool, stop_offset: usize,
}
impl Session {
    unsafe fn open() -> Result<Self, i32> {
        let master = crate::fw_api::nsh_master_open();
        if master < 0 { return Err(master); }
        let master = Fd(master);
        if master.0 <= 2 { return Err(EBADF); }
        let index = crate::fw_api::nsh_master_prepare(master.0);
        if index < 0 { return Err(index); }
        if index > 255 { return Err(EINVAL); }
        let slave = crate::fw_api::nsh_slave_open(index);
        if slave < 0 { return Err(slave); }
        let slave = Fd(slave);
        let mut head = 0usize;
        let rc = crate::fw_api::nsh_actions_init(&mut head);
        if rc != 0 { return Err(if rc < 0 { rc } else { -rc }); }
        let mut actions = Actions(head);
        for target in [0, 1, 2, master.0] {
            let rc = crate::fw_api::nsh_actions_adddup2(&mut actions.0, slave.0, target);
            if rc != 0 { return Err(if rc < 0 { rc } else { -rc }); }
        }
        CHILD_DONE.store(false, Ordering::Relaxed);
        CHILD_EXIT.store(0, Ordering::Relaxed);
        CHILD_CONSOLE.store(0, Ordering::Relaxed);
        ORIGINAL_EXIT.store(0, Ordering::Relaxed);
        let child = crate::fw_api::nsh_spawn(TASK_NAME.as_ptr(), shell_entry, &mut actions.0);
        drop(actions);
        drop(slave);
        if child <= 0 { return Err(if child < 0 { child } else { EIO }); }
        Ok(Self { master, child, pending: [0; IO_CHUNK], pending_len: 0,
            pending_offset: 0, failure: 0, write_failed: false, stopping: false, stop_offset: 0 })
    }
    fn fail(&mut self, code: i32) {
        if self.failure == 0 {
            self.failure = code; SESSION_ERROR.store(code, Ordering::Release);
        }
    }
    unsafe fn begin_stop(&mut self) {
        self.stopping = true;
        self.pending_len = 0; self.pending_offset = 0;
        // Producer is gated by State::Stopping. Only the consumer advances
        // tail; never reset a live ring or overwrite producer-owned storage.
        for _ in 0..TX_CAP / IO_CHUNK {
            if TX.pop(&mut self.pending) == 0 { break; }
        }
    }
    unsafe fn write_once(&mut self) {
        if self.write_failed { return; }
        if self.stopping {
            if self.stop_offset == STOP_SEQUENCE.len() { return; }
            let remaining = &STOP_SEQUENCE[self.stop_offset..];
            let result = crate::fw_api::nsh_write(self.master.0, remaining);
            if result > 0 && result as usize <= remaining.len() {
                self.stop_offset += result as usize;
            } else if result == EIO || result == EPIPE {
                // Ctrl-C can end the shell before the suffix is sent. Retain
                // ownership and wait for positive waitpid; no early cleanup.
                self.write_failed = true;
            } else if result != 0 && result != EINTR && result != EAGAIN {
                self.fail(if result < 0 { result } else { EIO }); self.write_failed = true;
            }
            return;
        }
        if self.pending_offset == self.pending_len {
            self.pending_len = TX.pop(&mut self.pending); self.pending_offset = 0;
        }
        if self.pending_len == 0 { return; }
        let remaining = &self.pending[self.pending_offset..self.pending_len];
        let result = crate::fw_api::nsh_write(self.master.0, remaining);
        if result > 0 && result as usize <= remaining.len() {
            self.pending_offset += result as usize;
        } else if result != 0 && result != EINTR && result != EAGAIN {
            self.fail(if result < 0 { result } else { EIO }); self.write_failed = true;
        }
    }
    unsafe fn read_burst(&mut self, budget: usize) -> i32 {
        let mut chunk = [0u8; IO_CHUNK];
        for _ in 0..budget {
            let result = crate::fw_api::nsh_read(self.master.0, &mut chunk);
            if result > 0 && result as usize <= chunk.len() {
                let count = result as usize;
                let kept = RX.push_some(&chunk[..count]);
                if kept != count {
                    let lost = count - kept;
                    let mut previous = DROPPED.load(Ordering::Relaxed);
                    loop {
                        match DROPPED.compare_exchange_weak(previous, previous.saturating_add(lost),
                            Ordering::Release, Ordering::Relaxed) {
                            Ok(_) => break, Err(current) => previous = current,
                        }
                    }
                }
            } else if result == EINTR { continue; }
            else {
                return if result == 0 || result == EAGAIN { 0 }
                    else if result < 0 { result } else { EIO };
            }
        }
        0
    }
    unsafe fn complete(&mut self, status: i32) -> i32 {
        let _ = self.read_burst(TAIL_READ_BUDGET);
        let console = CHILD_CONSOLE.swap(0, Ordering::AcqRel);
        if console != 0 { crate::fw_api::nsh_console_free_after_exit(console); }
        ORIGINAL_EXIT.store(0, Ordering::Release);
        let rc = if CHILD_DONE.load(Ordering::Acquire) { CHILD_EXIT.load(Ordering::Relaxed) }
            else { ((status as u32 >> 8) & 0xff) as i32 };
        if self.failure != 0 { self.failure } else { rc }
    }
    unsafe fn pump(&mut self) -> Option<i32> {
        let mut status = 0i32;
        let waited = crate::fw_api::nsh_waitpid_nohang(self.child, &mut status);
        if waited == self.child { return Some(self.complete(status)); }
        if waited != 0 && waited != EINTR && waited != EAGAIN {
            self.fail(if waited < 0 { waited } else { EIO });
        }
        if STOP_REQUEST.swap(false, Ordering::AcqRel) { self.begin_stop(); }
        self.write_once();
        let read_error = self.read_burst(LIVE_READ_BUDGET);
        if read_error == EIO {
            let waited = crate::fw_api::nsh_waitpid_nohang(self.child, &mut status);
            if waited == self.child { return Some(self.complete(status)); }
            if waited != 0 && waited != EINTR && waited != EAGAIN {
                self.fail(if waited < 0 { waited } else { EIO });
            }
            if !self.stopping && !CHILD_DONE.load(Ordering::Acquire) { self.fail(read_error); }
        } else if read_error != 0 { self.fail(read_error); }
        None
    }
}
unsafe fn worker_step(session: &mut Option<Session>) {
    if let Some(active) = session.as_mut() {
        if let Some(result) = active.pump() {
            *session = None; // master/local storage must die before publication
            SESSION_ERROR.store(result, Ordering::Release);
            STATE.store(if result < 0 { State::Failed } else { State::Exited } as u8, Ordering::Release);
        }
    } else if REQUEST.swap(false, Ordering::AcqRel) {
        if STOP_REQUEST.swap(false, Ordering::AcqRel) {
            STATE.store(State::Exited as u8, Ordering::Release);
            return; // stop before opening: no child, fd or console to reclaim
        }
        match Session::open() {
            Ok(active) => {
                *session = Some(active);
                // Do not overwrite Stopping if the PAN task requested stop
                // while this task was in open/spawn. It pumps on next tick.
                let _ = STATE.compare_exchange(State::Starting as u8, State::Running as u8,
                    Ordering::AcqRel, Ordering::Acquire);
            }
            Err(code) => {
                SESSION_ERROR.store(code, Ordering::Release);
                STATE.store(State::Failed as u8, Ordering::Release);
            }
        }
    }
}
unsafe extern "C" fn worker_entry(_argc: i32, _argv: *mut *mut u8) -> i32 {
    let mut session = None;
    loop {
        worker_step(&mut session);
        let _ = crate::fw_api::nsh_usleep(if session.is_some() { ACTIVE_SLEEP_US } else { IDLE_SLEEP_US });
    }
}

#[no_mangle]
pub extern "C" fn chaos_web_terminal_start() -> i32 { start().err().unwrap_or(0) }
/// One caller (PAN worker); successful commit returns 0, never a partial count.
#[no_mangle]
pub unsafe extern "C" fn chaos_web_terminal_send(data: *const u8, length: u32) -> i32 {
    if length == 0 { return send(&[]).err().unwrap_or(0); }
    if data.is_null() { return api_error(EINVAL).err().unwrap(); }
    if length as usize > TX_CAP { return api_error(EMSGSIZE).err().unwrap(); }
    send(core::slice::from_raw_parts(data, length as usize)).err().unwrap_or(0)
}
/// Caller supplies a writable buffer. Bounded, nonblocking, no firmware calls.
#[no_mangle]
pub unsafe extern "C" fn chaos_web_terminal_receive(out: *mut u8, capacity: u32) -> u32 {
    if capacity == 0 { return 0; }
    if out.is_null() { let _ = api_error(EINVAL); return 0; }
    receive(core::slice::from_raw_parts_mut(out, (capacity as usize).min(RX_CAP))) as u32
}
#[no_mangle]
pub extern "C" fn chaos_web_terminal_stop() { stop(); }
#[no_mangle]
pub extern "C" fn chaos_web_terminal_state() -> i32 { state() as i32 }
#[no_mangle]
pub extern "C" fn chaos_web_terminal_error() -> i32 { error() }

#[cfg(test)]
#[path = "../tests/web_nsh_cases.rs"]
mod tests;
