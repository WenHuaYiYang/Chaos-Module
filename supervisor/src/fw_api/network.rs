//! p67 / 3.101.043 network clock, read-only status, and isolated reboot task.
//! Exact-image evidence: docs/WEB-NSH.md and tools/probe_network_abi.py.
use core::{mem::transmute, ptr};
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use super::{fwaddr, Cp};

const EINVAL: i32 = -22;
const EIO: i32 = -5;
const EINTR: i32 = -4;
const EAGAIN: i32 = -11;
const EBUSY: i32 = -16;

#[repr(C, align(8))]
struct Timespec { sec: i64, nsec: i32, _pad: i32 }
static LAST_MS: AtomicU32 = AtomicU32::new(0);
// -1 reserves the spawn window; only the PAN worker reaps a positive pid.
static REBOOT_PID: AtomicI32 = AtomicI32::new(0);
static REBOOT_RESULT: AtomicI32 = AtomicI32::new(0);

unsafe fn libc_result(value: i32) -> i32 {
    if value != -1 { return value; }
    #[cfg(test)]
    let code = crate::network_mock_errno();
    #[cfg(not(test))]
    let code = {
        let f: unsafe extern "C" fn() -> *const i32 = transmute(fwaddr::NSH_ERRNO_LOCATION as usize);
        let location = f();
        if location.is_null() { return EIO; }
        ptr::read_volatile(location)
    };
    if code > 0 { -code } else { EIO }
}
unsafe fn clock(ts: &mut Timespec) -> i32 {
    #[cfg(test)]
    { crate::network_mock_clock(&mut ts.sec, &mut ts.nsec) }
    #[cfg(not(test))]
    {
        let f: unsafe extern "C" fn(i32, *mut Timespec) -> i32 = transmute(fwaddr::CLOCK_GETTIME as usize);
        libc_result(f(1, ts))
    }
}
fn monotonic() -> Result<Timespec, i32> {
    let mut ts = Timespec { sec: 0, nsec: 0, _pad: 0 };
    let rc = unsafe { clock(&mut ts) };
    if rc != 0 { return Err(rc); }
    if ts.sec < 0 || ts.nsec < 0 || ts.nsec >= 1_000_000_000 { return Err(EIO); }
    Ok(ts)
}
fn reap_reboot() {
    let pid = REBOOT_PID.load(Ordering::Acquire);
    if pid <= 0 { return; }
    let mut status = 0;
    let waited = unsafe { super::nsh_waitpid_nohang(pid, &mut status) };
    if waited == pid { REBOOT_PID.store(0, Ordering::Release); }
    // No positive reap: keep ownership, including ECHILD/unknown failures.
}
/// True monotonic milliseconds with the u32 wrap required by lwIP timers.
/// A transient clock error preserves the last valid timestamp, not time zero.
/// The same single PAN caller also reaps its optional one-shot reboot task.
pub fn network_monotonic_ms() -> u32 {
    reap_reboot();
    if let Ok(ts) = monotonic() {
        let ms = (ts.sec as u32).wrapping_mul(1000).wrapping_add(ts.nsec as u32 / 1_000_000);
        LAST_MS.store(ms, Ordering::Release);
        ms
    } else { LAST_MS.load(Ordering::Acquire) }
}
#[no_mangle]
pub extern "C" fn chaos_network_monotonic_ms() -> u32 { network_monotonic_ms() }
#[no_mangle]
pub unsafe extern "C" fn chaos_network_uptime_seconds(out: *mut u32) -> i32 {
    if out.is_null() { return EINVAL; }
    match monotonic() {
        Ok(ts) => { *out = (ts.sec as u64).min(u32::MAX as u64) as u32; 0 }
        Err(code) => code,
    }
}

/// Fixed-size proc-file read; at most 32 reads (including interrupted calls).
/// Only the trusted internal caller supplies path. No write/seek/ioctl API.
pub unsafe fn network_read_file(path: Cp, out: &mut [u8]) -> i32 {
    if path.is_null() { return EINVAL; }
    if out.is_empty() { return 0; }
    let fd = libc_result(super::open(path, super::oflag::RDONLY, 0));
    if fd < 0 { return fd; }
    let mut used = 0usize;
    let mut result = EAGAIN;
    for _ in 0..32 {
        let remaining = out.len() - used;
        if remaining == 0 { result = used as i32; break; }
        let n = libc_result(super::read(fd, out.as_mut_ptr().add(used), remaining as u32));
        if n > 0 {
            if n as usize > remaining { result = EIO; break; }
            used += n as usize; result = used as i32; continue;
        }
        if n == 0 { result = used as i32; break; }
        if n == EINTR { continue; }
        result = if n == EAGAIN && used != 0 { used as i32 } else { n }; break;
    }
    let _ = super::close(fd);
    result
}
#[no_mangle]
pub unsafe extern "C" fn chaos_network_read_meminfo(out: *mut u8, capacity: u32) -> i32 {
    if out.is_null() || capacity == 0 { return EINVAL; }
    let buffer = core::slice::from_raw_parts_mut(out, (capacity as usize).min(4096));
    network_read_file(b"/proc/meminfo\0".as_ptr(), buffer)
}
unsafe extern "C" fn reboot_task(_argc: i32, _argv: *mut *mut u8) -> i32 {
    #[cfg(test)]
    let rc = crate::network_mock_system(b"reboot\0");
    #[cfg(not(test))]
    let rc = {
        let f: unsafe extern "C" fn(Cp) -> i32 = transmute(fwaddr::NSH_SYSTEM as usize);
        libc_result(f(b"reboot\0".as_ptr()))
    };
    REBOOT_RESULT.store(rc, Ordering::Release);
    rc
}
/// Queue exactly system("reboot") on a separate task. system() blocks in
/// waitpid, so it must never run on the PAN worker or an HTTP/BT callback.
#[no_mangle]
pub extern "C" fn chaos_network_reboot() -> i32 {
    reap_reboot();
    if REBOOT_PID.compare_exchange(0, -1, Ordering::AcqRel, Ordering::Acquire).is_err() { return EBUSY; }
    REBOOT_RESULT.store(0, Ordering::Release);
    let pid = unsafe { super::nsh_spawn(b"chaos.web.reboot\0".as_ptr(), reboot_task, ptr::null_mut()) };
    if pid <= 0 {
        REBOOT_PID.store(0, Ordering::Release);
        let rc = if pid < 0 { pid } else { EIO };
        REBOOT_RESULT.store(rc, Ordering::Release);
        return rc;
    }
    REBOOT_PID.store(pid, Ordering::Release);
    0
}

#[cfg(test)]
#[path = "../../tests/network_platform_cases.rs"]
mod tests;
