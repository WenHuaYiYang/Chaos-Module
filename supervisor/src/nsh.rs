//! Single-session p67 / 3.101.043 interactive NSH.
//! Only the LVGL thread calls start/send/receive. A resident independent task
//! owns the PTY and reaps its child; no UI function reads or waits on a fd.

use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicUsize, Ordering};
#[path = "nsh_queue.rs"]
mod queue;
use queue::Spsc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum State { Idle, Starting, Running, Exited, Failed }

const EIO: i32 = -5;
const EBADF: i32 = -9;
const EAGAIN: i32 = -11;
const EINTR: i32 = -4;
const ENOMEM: i32 = -12;
const EBUSY: i32 = -16;
const EINVAL: i32 = -22;
const ENOTSUP: i32 = -95;
const RX_CAP: usize = 8192;
const TX_CAP: usize = 2048;
const IO_CHUNK: usize = 256;
const LIVE_READ_BUDGET: usize = 8;
const TAIL_READ_BUDGET: usize = 64;
const ACTIVE_SLEEP_US: u32 = 20_000;
const IDLE_SLEEP_US: u32 = 200_000;
const TASK_NAME: &[u8] = b"chaos.nsh\0";
const WORKER_NAME: &[u8] = b"chaos.nsh.worker\0";

static STATE: AtomicU8 = AtomicU8::new(State::Idle as u8);
static API_ERROR: AtomicI32 = AtomicI32::new(0);
static SESSION_ERROR: AtomicI32 = AtomicI32::new(0);
static WORKER_STARTED: AtomicBool = AtomicBool::new(false);
// Starting reserves ownership; REQUEST publishes the completed queue reset.
static REQUEST: AtomicBool = AtomicBool::new(false);
static CHILD_DONE: AtomicBool = AtomicBool::new(false);
static CHILD_EXIT: AtomicI32 = AtomicI32::new(0);
static CHILD_CONSOLE: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_EXIT: AtomicUsize = AtomicUsize::new(0);
static RX: Spsc<RX_CAP> = Spsc::new();
static TX: Spsc<TX_CAP> = Spsc::new();
static DROPPED: AtomicUsize = AtomicUsize::new(0);
// These cursors belong to the UI consumer, not the worker. A short receive
// must preserve the rest of an overflow notice across subsequent calls.
static NOTICE_COUNT: AtomicUsize = AtomicUsize::new(0);
static NOTICE_OFFSET: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn state() -> State {
    match STATE.load(Ordering::Acquire) {
        0 => State::Idle, 1 => State::Starting, 2 => State::Running,
        3 => State::Exited, _ => State::Failed,
    }
}
pub(crate) fn error() -> i32 {
    let api = API_ERROR.load(Ordering::Acquire);
    if api != 0 { api } else { SESSION_ERROR.load(Ordering::Acquire) }
}
fn api_error(code: i32) -> Result<(), i32> {
    API_ERROR.store(code, Ordering::Release);
    Err(code)
}

/// Called once per manual start/restart, by the single UI thread.
pub(crate) fn start() -> Result<(), i32> {
    let current = state();
    if matches!(current, State::Starting | State::Running) { return Ok(()); }
    if STATE.compare_exchange(current as u8, State::Starting as u8,
        Ordering::AcqRel, Ordering::Acquire).is_err() { return Ok(()); }
    // A final state is published only after the worker releases all ownership.
    unsafe { RX.reset_quiescent(); TX.reset_quiescent(); }
    DROPPED.store(0, Ordering::Relaxed);
    NOTICE_COUNT.store(0, Ordering::Relaxed);
    NOTICE_OFFSET.store(0, Ordering::Relaxed);
    API_ERROR.store(0, Ordering::Release);
    SESSION_ERROR.store(0, Ordering::Release);
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

/// One complete block is committed or none of it. Only one UI producer.
pub(crate) fn send(input: &[u8]) -> Result<(), i32> {
    if input.iter().any(|&b| b == 0) { return api_error(EINVAL); }
    // 043 readline discards EOT; it is not a reliable session-close operation.
    if input.iter().any(|&b| b == 4) { return api_error(ENOTSUP); }
    if input.is_empty() { API_ERROR.store(0, Ordering::Release); return Ok(()); }
    if !matches!(state(), State::Starting | State::Running) { return api_error(EBUSY); }
    let failure = SESSION_ERROR.load(Ordering::Acquire);
    if failure != 0 { return api_error(failure); }
    if !unsafe { TX.push_all(input) } { return api_error(EBUSY); }
    API_ERROR.store(0, Ordering::Release);
    Ok(())
}

fn overflow_note(out: &mut [u8], count: usize) -> usize {
    let mut text = [0u8; 72];
    let prefix = b"\n[NSH: dropped ";
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

/// Bounded, nonblocking and free of firmware calls. Single UI consumer only.
pub(crate) fn receive(out: &mut [u8]) -> usize {
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
    // Stock exit frees this console and exits the child. Remove our leftover
    // pointer before delegating, including the callback's non-returning path.
    let callback = ORIGINAL_EXIT.swap(0, Ordering::AcqRel);
    if callback != 0 {
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
        return EIO; // worker frees only after positively reaping this task
    }
    ORIGINAL_EXIT.store(original, Ordering::Release);
    crate::fw_api::nsh_console_set_exit(console, shell_exit);
    let rc = crate::fw_api::nsh_console_run(console, argc, argv);
    // exit normally never returns. Avoid double delegation if a native error
    // path or host mock returns after invoking the instance callback.
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
    master: Fd,
    child: i32,
    pending: [u8; IO_CHUNK],
    pending_len: usize,
    pending_offset: usize,
    failure: i32,
    write_failed: bool,
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
        // Replace the child's inherited live master as well. The worker's
        // fd 0/1/2 stay unchanged: task_spawn gives its child a separate table.
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
            pending_offset: 0, failure: 0, write_failed: false })
    }
    fn fail(&mut self, code: i32) {
        if self.failure == 0 {
            self.failure = code;
            SESSION_ERROR.store(code, Ordering::Release);
        }
    }
    unsafe fn write_once(&mut self) {
        if self.write_failed { return; }
        if self.pending_offset == self.pending_len {
            self.pending_len = TX.pop(&mut self.pending);
            self.pending_offset = 0;
        }
        if self.pending_len == 0 { return; }
        let remaining = &self.pending[self.pending_offset..self.pending_len];
        let result = crate::fw_api::nsh_write(self.master.0, remaining);
        if result > 0 && result as usize <= remaining.len() {
            self.pending_offset += result as usize;
        } else if result != 0 && result != EINTR && result != EAGAIN {
            self.fail(if result < 0 { result } else { EIO });
            self.write_failed = true;
        }
        // Short writes/EINTR/EAGAIN/zero retain this local suffix. The consumer
        // never rewinds TX.tail or writes into producer-owned ring storage.
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
        // EOF/EIO are expected here: the child is already positively reaped.
        let _ = self.read_burst(TAIL_READ_BUDGET);
        // Direct signal/task death can bypass shell_exit. Free the known
        // allocation only, never a destructor closing worker-side fd nums.
        let console = CHILD_CONSOLE.swap(0, Ordering::AcqRel);
        if console != 0 { crate::fw_api::nsh_console_free_after_exit(console); }
        ORIGINAL_EXIT.store(0, Ordering::Release);
        let rc = if CHILD_DONE.load(Ordering::Acquire) { CHILD_EXIT.load(Ordering::Relaxed) }
            else { ((status as u32 >> 8) & 0xff) as i32 };
        // The exact AP waitpid probe confirms record.exit_code << 8. Preserve
        // a nonzero exit code rather than reporting a direct task death as 0.
        if self.failure != 0 { self.failure } else { rc }
    }
    /// None means the child is NOT positively reaped, even if its exit callback
    /// ran or I/O failed. Only Some permits releasing session ownership.
    unsafe fn pump(&mut self) -> Option<i32> {
        let mut status = 0i32;
        let waited = crate::fw_api::nsh_waitpid_nohang(self.child, &mut status);
        if waited == self.child {
            return Some(self.complete(status));
        }
        if waited != 0 && waited != EINTR && waited != EAGAIN {
            self.fail(if waited < 0 { waited } else { EIO });
        }
        self.write_once();
        let read_error = self.read_burst(LIVE_READ_BUDGET);
        if read_error == EIO {
            // The child can exit between the initial wait and read. Confirm
            // once more; callback completion suppresses normal tail EIO, but
            // NEVER permits cleanup without a positive waitpid result.
            let waited = crate::fw_api::nsh_waitpid_nohang(self.child, &mut status);
            if waited == self.child { return Some(self.complete(status)); }
            if waited != 0 && waited != EINTR && waited != EAGAIN {
                self.fail(if waited < 0 { waited } else { EIO });
            }
            if !CHILD_DONE.load(Ordering::Acquire) { self.fail(read_error); }
        } else if read_error != 0 { self.fail(read_error); }
        None
    }
}

unsafe fn worker_step(session: &mut Option<Session>) {
    if let Some(active) = session.as_mut() {
        if let Some(result) = active.pump() {
            // Drop master/local ownership before the final Release allows UI
            // restart/reset. No worker queue operation follows this publication.
            *session = None;
            SESSION_ERROR.store(result, Ordering::Release);
            STATE.store(if result < 0 { State::Failed } else { State::Exited } as u8, Ordering::Release);
        }
    } else if REQUEST.swap(false, Ordering::AcqRel) {
        match Session::open() {
            Ok(active) => {
                *session = Some(active);
                STATE.store(State::Running as u8, Ordering::Release);
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

#[cfg(test)]
#[path = "../tests/nsh_cases.rs"]
mod tests;
