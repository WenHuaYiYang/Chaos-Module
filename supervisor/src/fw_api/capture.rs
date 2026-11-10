//! Audited p67/.043 framebuffer and VFS calls; no LVGL calls on the worker.
use core::{cell::UnsafeCell, mem::transmute, ptr, sync::atomic::{AtomicU32, Ordering}};
use super::*;

pub unsafe fn capture_result(rc: i32) -> i32 {
    if rc != -1 { return rc; }
    let f: unsafe extern "C" fn() -> *const i32 = transmute(fwaddr::NSH_ERRNO_LOCATION as usize);
    let p = f();
    if p.is_null() { return -5; }
    let e = ptr::read_volatile(p); if e > 0 { -e } else { -5 }
}
pub unsafe fn capture_mkdir() -> i32 {
    capture_mkdir_path(crate::capture_format::ROOT.as_ptr())
}
pub unsafe fn capture_mkdir_path(path: Cp) -> i32 {
    let f: unsafe extern "C" fn(Cp, u32) -> i32 = transmute(fwaddr::CAPTURE_MKDIR as usize);
    capture_result(f(path, 0o755))
}
pub unsafe fn capture_rename(old: Cp, new: Cp) -> i32 {
    let f: unsafe extern "C" fn(Cp, Cp) -> i32 = transmute(fwaddr::CAPTURE_RENAME as usize);
    capture_result(f(old, new))
}
pub fn capture_now_ms() -> u32 {
    #[repr(C, align(8))]
    struct Ts { sec: i64, nsec: i32, pad: i32 }
    let mut ts = Ts { sec: 0, nsec: 0, pad: 0 };
    unsafe {
        let f: unsafe extern "C" fn(i32, *mut Ts) -> i32 = transmute(fwaddr::CLOCK_GETTIME as usize);
        if f(1, &mut ts) < 0 { return 0; }
    }
    (ts.sec as u32).wrapping_mul(1000).wrapping_add(ts.nsec as u32 / 1_000_000)
}

pub unsafe fn capture_free_bytes() -> Option<u64> {
    let mut report = [0u8; 2048];
    let n = network_read_file(b"/proc/fs/usage\0".as_ptr(), &mut report);
    if n <= 0 { return None; }
    crate::capture_format::data_free(&report[..n as usize])
}

unsafe fn check_video(fd: i32) -> Result<(), i32> {
        // Native caller at file 0x3f7aa4 uses these exact requests and fields.
        let mut video = [0u32; 8];
        let rc = capture_result(ioctl(fd, 0x2801, video.as_mut_ptr() as u32));
        if rc < 0 { return Err(rc); }
        let bytes = core::slice::from_raw_parts(video.as_ptr().cast::<u8>(), 32);
        if bytes[0] != 12 || u16::from_le_bytes([bytes[2], bytes[3]]) != 336
            || u16::from_le_bytes([bytes[4], bytes[5]]) != 480 { return Err(-95); }
        Ok(())
}
unsafe fn read_plane(fd: i32) -> Result<[u32; 8], i32> {
        let mut plane = [0u32; 8];
        let rc = capture_result(ioctl(fd, 0x2802, plane.as_mut_ptr() as u32));
        if rc < 0 { return Err(rc); }
        let bytes = core::slice::from_raw_parts(plane.as_ptr().cast::<u8>(), 32);
        if u16::from_le_bytes([bytes[8], bytes[9]]) != 1008 || bytes[11] != 24 { return Err(-95); }
        Ok(plane)
}
unsafe fn read_frame(fd: i32, out: &mut [u8]) -> Result<(u32, u32), i32> {
        if out.len() != crate::capture_format::FRAME_BYTES { return Err(-22); }
        let plane = read_plane(fd)?;
        // p67 plane info has u32 virtual dimensions/xoffset/yoffset at +12..24.
        // Read the visible window when the driver reports a panned buffer.
        let length = plane[1] as u64;
        let x = plane[5] as u64; let y = plane[6] as u64;
        let offset = y * 1008 + x * 3;
        if x != 0 || y + 480 > plane[4] as u64 || offset + out.len() as u64 > length { return Err(-95); }
        if lseek(fd, offset as i64, seek::SET) != offset as i64 { return Err(-5); }
        let mut used = 0;
        let mut interrupted = 0;
        while used < out.len() {
            let n = nsh_read(fd, &mut out[used..]);
            if n == -4 && interrupted < 8 { interrupted += 1; continue; }
            if n <= 0 { return Err(if n < 0 { n } else { -5 }); }
            if n as usize > out.len() - used { return Err(-5); }
            used += n as usize;
        }
        Ok((plane[0], plane[6]))
}

/// Screenshot after OFF: never wait for a vsync that a sleeping panel cannot emit.
pub unsafe fn capture_frame(out: &mut [u8]) -> Result<(), i32> {
    let fd = capture_result(open(b"/dev/fb0\0".as_ptr(), oflag::RDONLY, 0));
    if fd < 0 { return Err(fd); }
    let result = check_video(fd).and_then(|_| read_frame(fd, out).map(|_| ()));
    let close_rc = capture_result(close(fd));
    result.and(if close_rc < 0 { Err(close_rc) } else { Ok(()) })
}

// Exact .043 pollfd: six WORDS (not Linux's 8-byte fd/events/revents).
// Driver owns the two UnsafeCells until poll teardown. The allocation is
// stable even when the Rust reader moves between task-stack frames.
#[repr(C)]
struct SyncState {
    fd: i32, events: u32, revents: UnsafeCell<u32>, arg: u32, callback: u32,
    private: UnsafeCell<u32>, sequence: AtomicU32, fault: AtomicU32,
    semaphore: UnsafeCell<[u32; 3]>,
}
const _: () = {
    assert!(core::mem::size_of::<SyncState>() == 44);
    assert!(core::mem::offset_of!(SyncState, revents) == 8);
    assert!(core::mem::offset_of!(SyncState, callback) == 16);
    assert!(core::mem::offset_of!(SyncState, private) == 20);
    assert!(core::mem::offset_of!(SyncState, semaphore) == 32);
};
unsafe extern "C" fn vsync_event(state: *mut SyncState) {
    let events = ptr::read_volatile((*state).revents.get());
    // Acknowledge before publishing: native poll_notify suppresses unchanged bits.
    ptr::write_volatile((*state).revents.get(), 0);
    if events & 0x38 != 0 { (*state).fault.store(events & 0x38, Ordering::Release); }
    if events & 2 != 0 { (*state).sequence.fetch_add(1, Ordering::Release); }
    // The firmware's own poll callback coalesces the semaphore to one token
    // and posts with its IRQ-safe nxsem_post path.
    let wake: unsafe extern "C" fn(*mut SyncState) = transmute(fwaddr::CAPTURE_POLL_WAKE as usize);
    wake(state);
}
unsafe fn file_poll(file: u32, state: *mut SyncState, setup: bool) -> i32 {
    let f: unsafe extern "C" fn(u32, *mut SyncState, u32) -> i32 = transmute(fwaddr::CAPTURE_FILE_POLL as usize);
    f(file, state, setup as u32)
}

/// Resident reader: its own file reference and POLLPRI subscription. No global
/// LVGL hooks, blocking WAITFORVSYNC ioctl, or callback pointers into task stacks.
pub struct CaptureReader { fd: i32, file: u32, sync: *mut SyncState, registered: bool, last: u32, missed: u32, unstable: u32 }
impl CaptureReader {
    pub unsafe fn open() -> Result<Self, i32> {
        let fd = capture_result(open(b"/dev/fb0\0".as_ptr(), oflag::RDONLY, 0));
        if fd < 0 { return Err(fd); }
        let mut reader = Self { fd, file: 0, sync: ptr::null_mut(), registered: false, last: 0, missed: 0, unstable: 0 };
        check_video(fd)?;
        let get: unsafe extern "C" fn(i32, *mut u32) -> i32 = transmute(fwaddr::CAPTURE_GET_FILE as usize);
        let rc = get(fd, &mut reader.file);
        if rc < 0 { return Err(rc); }
        reader.sync = heap_malloc(core::mem::size_of::<SyncState>() as u32) as *mut SyncState;
        if reader.sync.is_null() { return Err(-12); }
        ptr::write(reader.sync, SyncState {
            fd, events: 2, revents: UnsafeCell::new(0), arg: 0,
            callback: vsync_event as *const () as u32 | 1, private: UnsafeCell::new(0),
            sequence: AtomicU32::new(0), fault: AtomicU32::new(0),
            semaphore: UnsafeCell::new([0; 3]),
        });
        let sem = (*reader.sync).semaphore.get().cast::<u32>();
        (*reader.sync).arg = sem as u32;
        let init: unsafe extern "C" fn(*mut u32, u32, u32) -> i32 = transmute(fwaddr::CAPTURE_SEM_INIT as usize);
        let rc = init(sem, 0, 0); if rc < 0 { return Err(rc); }
        let rc = file_poll(reader.file, reader.sync, true);
        if rc < 0 { return Err(rc); }
        reader.registered = true;
        Ok(reader)
    }
    pub unsafe fn pending(&self) -> Result<bool, i32> {
        if (*self.sync).fault.load(Ordering::Acquire) != 0 { return Err(-5); }
        Ok((*self.sync).sequence.load(Ordering::Acquire) != self.last && screen_is_on())
    }
    pub unsafe fn pause(&mut self) { self.last = (*self.sync).sequence.load(Ordering::Acquire); }
    pub fn missed(&self) -> u32 { self.missed }
    pub fn unstable(&self) -> u32 { self.unstable }
    pub unsafe fn frame(&mut self, out: &mut [u8], timer_due: bool) -> Result<Option<u32>, i32> {
        if out.len() != crate::capture_format::FRAME_BYTES { return Err(-22); }
        if (*self.sync).fault.load(Ordering::Acquire) != 0 { return Err(-5); }
        let before = (*self.sync).sequence.load(Ordering::Acquire);
        if !screen_is_on() || (before == self.last && !timer_due) { return Ok(None); }
        if self.last != 0 { self.missed = self.missed.saturating_add(before.wrapping_sub(self.last).saturating_sub(1)); }
        // Keep the relaxed fast path. If a quick copy crossed a display event,
        // take just one newer snapshot immediately, without another vsync wait
        // or a discard loop. Slow copies retain the first complete snapshot.
        let started = capture_now_ms();
        let _ = read_frame(self.fd, out)?;
        let mut captured = capture_now_ms();
        let mut after = (*self.sync).sequence.load(Ordering::Acquire);
        if after != before { self.unstable = self.unstable.saturating_add(1); }
        if crate::capture::retry_fast_snapshot(after != before, captured.wrapping_sub(started)) && screen_is_on() {
            let retry_before = after;
            let _ = read_frame(self.fd, out)?;
            captured = capture_now_ms();
            after = (*self.sync).sequence.load(Ordering::Acquire);
            if after != retry_before { self.unstable = self.unstable.saturating_add(1); }
        }
        // Coalesce notifications arriving during this copy instead of causing
        // an immediate duplicate copy. Timed capture remains available.
        self.last = after;
        Ok(Some(captured))
    }
    pub unsafe fn wait_vsync(&mut self) {
        if (*self.sync).sequence.load(Ordering::Acquire) != self.last
            || (*self.sync).fault.load(Ordering::Acquire) != 0 { return; }
        let wait: unsafe extern "C" fn(*mut u32, u32) -> i32 = transmute(fwaddr::CAPTURE_SEM_WAIT as usize);
        // One .043 kernel tick (10 ms); vsync wakes immediately, OFF times out.
        let _ = wait((*self.sync).semaphore.get().cast(), 1);
    }
}
impl Drop for CaptureReader {
    fn drop(&mut self) { unsafe {
        let detached = !self.registered || file_poll(self.file, self.sync, false) >= 0;
        // If native teardown fails, retain the 44 bytes rather than expose an
        // ISR to freed storage. Closing our fd/ref still closes our driver context.
        if detached && !self.sync.is_null() {
            let destroy: unsafe extern "C" fn(*mut u32) -> i32 = transmute(fwaddr::CAPTURE_SEM_DESTROY as usize);
            let _ = destroy((*self.sync).semaphore.get().cast()); fw_free(self.sync as u32);
        }
        if self.file != 0 {
            let put: unsafe extern "C" fn(u32) = transmute(fwaddr::CAPTURE_PUT_FILE as usize);
            put(self.file);
        }
        if self.fd >= 0 { close(self.fd); }
    } }
}
