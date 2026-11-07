//! p67 / 3.101.043 NSH ABI, independently recovered from the exact AP.
//! See docs/NSH-{ABI,PTY,SPAWN}.md and the machine-code probes. Static/host
//! evidence only: no claim of on-device scheduling or lifetime verification.
use core::{mem::transmute, ptr};
use super::fwaddr;

pub type NshTaskEntry = unsafe extern "C" fn(i32, *mut *mut u8) -> i32;
pub type NshExit = unsafe extern "C" fn(usize, i32);
type TaskSpawn = unsafe extern "C" fn(*const u8, NshTaskEntry, *mut usize, *mut u8, *const *const u8, *const *const u8) -> i32;

/// Public libc calls return -1 and put a positive errno in the current TLS.
/// Internal task_spawn instead returns negative errno directly; do NOT run
/// its result through this conversion. Read errno before any other call.
unsafe fn libc_result(value: i32) -> i32 {
    if value != -1 { return value; }
    let location: unsafe extern "C" fn() -> *const i32 =
        transmute(fwaddr::NSH_ERRNO_LOCATION as usize);
    let errno = location();
    if errno.is_null() { return -5; }
    let code = ptr::read_volatile(errno);
    if code > 0 { -code } else { -5 }
}

fn posix_result(value: i32) -> i32 {
    if value > 0 { -value } else { value }
}

/// Creates an isolated NuttX task (not a pthread sharing the UI fd table).
/// The stock caller and a raw-instruction probe establish the six arguments.
/// A NULL argv is essential: p67 prepends name itself, producing argc=1.
pub unsafe fn nsh_spawn(name: *const u8, entry: NshTaskEntry, actions: *mut usize) -> i32 {
    // Five words, not an unaligned byte array. +0x10 remains NULL stackaddr.
    let mut attr = [0u32; 5];
    let attr_ptr = attr.as_mut_ptr().cast::<u8>();
    let init: unsafe extern "C" fn(*mut u8) -> i32 = transmute(fwaddr::NSH_ATTR_INIT as usize);
    let priority: unsafe extern "C" fn(*mut u8, *const u32) -> i32 = transmute(fwaddr::NSH_ATTR_PRIORITY as usize);
    let stack: unsafe extern "C" fn(*mut u8, u32) -> i32 = transmute(fwaddr::NSH_ATTR_STACK as usize);
    let flags: unsafe extern "C" fn(*mut u8, u32) -> i32 = transmute(fwaddr::NSH_ATTR_FLAGS as usize);
    let destroy: unsafe extern "C" fn(*mut u8) -> i32 = transmute(fwaddr::NSH_ATTR_DESTROY as usize);
    let rc = posix_result(init(attr_ptr));
    if rc != 0 { return rc; }
    let scheduling_priority = 100u32;
    let mut rc = posix_result(priority(attr_ptr, &scheduling_priority));
    if rc == 0 { rc = posix_result(stack(attr_ptr, 0x8000)); }
    if rc == 0 { rc = posix_result(flags(attr_ptr, 4)); }
    if rc == 0 {
        let spawn: TaskSpawn = transmute(fwaddr::NSH_TASK_SPAWN as usize);
        rc = spawn(name, entry, actions, attr_ptr, ptr::null(), ptr::null());
    }
    let _ = destroy(attr_ptr);
    rc
}

pub unsafe fn nsh_actions_init(head: *mut usize) -> i32 {
    let f: unsafe extern "C" fn(*mut usize) -> i32 = transmute(fwaddr::NSH_ACTIONS_INIT as usize);
    posix_result(f(head))
}

pub unsafe fn nsh_actions_adddup2(head: *mut usize, oldfd: i32, newfd: i32) -> i32 {
    let f: unsafe extern "C" fn(*mut usize, i32, i32) -> i32 = transmute(fwaddr::NSH_ACTIONS_ADDDUP2 as usize);
    posix_result(f(head, oldfd, newfd))
}

pub unsafe fn nsh_actions_destroy(head: *mut usize) {
    let f: unsafe extern "C" fn(*mut usize) -> i32 = transmute(fwaddr::NSH_ACTIONS_DESTROY as usize);
    let _ = f(head);
}

pub unsafe fn nsh_master_open() -> i32 {
    libc_result(super::open(b"/dev/ptmx\0".as_ptr(), 0x403, 0))
}

/// Returns the PTY index on success. FIONBIO takes int*, but unlock takes a
/// VALUE. Only make the master nonblocking; NSH's slave must stay blocking.
pub unsafe fn nsh_master_prepare(master: i32) -> i32 {
    let mut nonblock = 1i32;
    let rc = libc_result(super::ioctl(master, 0x30A, ptr::addr_of_mut!(nonblock) as u32));
    if rc < 0 { return rc; }
    let rc = libc_result(super::ioctl(master, 0x12C, 0));
    if rc < 0 { return rc; }
    let mut index = -1i32;
    let rc = libc_result(super::ioctl(master, 0x12B, ptr::addr_of_mut!(index) as u32));
    if rc < 0 { return rc; }
    if !(0..=255).contains(&index) { return -22; }
    index
}

pub unsafe fn nsh_slave_open(index: i32) -> i32 {
    if !(0..=255).contains(&index) { return -22; }
    // /dev/pts/ + at most three digits + NUL. No NUL in the prefix.
    let mut path = [0u8; 13];
    path[..9].copy_from_slice(b"/dev/pts/");
    let mut len = 9;
    if index >= 100 { path[len] = b'0' + (index / 100) as u8; len += 1; }
    if index >= 10 { path[len] = b'0' + (index / 10 % 10) as u8; len += 1; }
    path[len] = b'0' + (index % 10) as u8;
    libc_result(super::open(path.as_ptr(), super::oflag::RDWR, 0))
}

pub unsafe fn nsh_close(fd: i32) {
    let _ = super::close(fd);
}

pub unsafe fn nsh_read(fd: i32, buffer: &mut [u8]) -> i32 {
    libc_result(super::read(fd, buffer.as_mut_ptr(), buffer.len() as u32))
}

pub unsafe fn nsh_write(fd: i32, buffer: &[u8]) -> i32 {
    libc_result(super::write(fd, buffer.as_ptr(), buffer.len() as u32))
}

pub unsafe fn nsh_console_new() -> usize {
    let f: unsafe extern "C" fn(i32) -> usize = transmute(fwaddr::NSH_CONSOLE_NEW as usize);
    f(1)
}

/// +0x2C belongs to this allocated instance, not a shared/global vtable.
pub unsafe fn nsh_console_get_exit(console: usize) -> usize {
    ptr::read_unaligned((console + 0x2C) as *const usize)
}

pub unsafe fn nsh_console_set_exit(console: usize, callback: NshExit) {
    ptr::write_unaligned((console + 0x2C) as *mut usize, callback as usize);
}

pub unsafe fn nsh_console_run(console: usize, argc: i32, argv: *mut *mut u8) -> i32 {
    let f: unsafe extern "C" fn(usize, i32, i32, *mut *mut u8) -> i32 =
        transmute(fwaddr::NSH_SESSION as usize);
    f(console, 1, argc, argv)
}

/// The native exit callback frees console and terminates the CHILD task.
/// Invoke only from that child; never touch console after this call.
pub unsafe fn nsh_console_call_exit(callback: usize, console: usize, result: i32) {
    let f: NshExit = transmute(callback);
    f(console, result);
}

/// Only after positive waitpid reaping proves the child can no longer use it,
/// and only when the child did NOT delegate to the native exit callback.
/// Free memory with its own allocator; never close child's fd numbers here.
pub unsafe fn nsh_console_free_after_exit(console: usize) {
    let f: unsafe extern "C" fn(usize) = transmute(fwaddr::NSH_USER_FREE as usize);
    f(console);
}

pub unsafe fn nsh_waitpid_nohang(pid: i32, status: &mut i32) -> i32 {
    let f: unsafe extern "C" fn(i32, *mut i32, i32) -> i32 = transmute(fwaddr::NSH_WAITPID as usize);
    // WNOHANG=2 on p67; Linux's 1 is incorrect.
    libc_result(f(pid, status, 2))
}

pub unsafe fn nsh_usleep(usec: u32) -> i32 {
    let f: unsafe extern "C" fn(u32) -> i32 = transmute(fwaddr::NSH_USLEEP as usize);
    libc_result(f(usec))
}
