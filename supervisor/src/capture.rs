//! A single independent task owns framebuffer buffers and recording files.
//! The UI only publishes requests/reads atomic status; leaving a page is harmless.
use core::{ptr, sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicU32, Ordering}};
use crate::{fw_api as fw, capture_format as format};
use format::{Dictionary, Kind, Sink, Storage, VideoFormat};
use crate::{capture_apng as apng, capture_settings::{self, Settings}};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum State { Idle, Starting, Recording, Stopping }
pub(crate) fn state() -> State {
    match STATE.load(Ordering::Acquire) { 1 => State::Starting, 2 => State::Recording, 3 => State::Stopping, _ => State::Idle }
}
pub(crate) fn error() -> i32 { ERROR.load(Ordering::Acquire) }
pub(crate) fn shot_error() -> i32 { SHOT_ERROR.load(Ordering::Acquire) }
pub(crate) fn shot_state() -> u8 { SHOT.load(Ordering::Acquire) }
pub(crate) fn frames() -> u32 { FRAMES.load(Ordering::Acquire) }
pub(crate) fn fps() -> u32 { FPS.load(Ordering::Acquire) }
pub(crate) fn constrained() -> bool { CONSTRAINED.load(Ordering::Acquire) }
pub(crate) fn revision() -> u32 { REVISION.load(Ordering::Acquire) }
pub(crate) fn limited() -> bool { LIMITED.load(Ordering::Acquire) }
pub(crate) fn active_format() -> VideoFormat {
    if state() == State::Idle { capture_settings::get().format }
    else { Settings::unpack(REQUEST_SETTINGS.load(Ordering::Acquire)).format }
}
pub(crate) fn settings_changed() { changed(); }

static STATE: AtomicU8 = AtomicU8::new(0);
// Starting reserves the slot; this separate release publishes the reset fields.
static START_REQUEST: AtomicBool = AtomicBool::new(false);
static REQUEST_SETTINGS: AtomicU32 = AtomicU32::new(0);
static SHOT_STORAGE: AtomicU8 = AtomicU8::new(0);
// Screenshot: 0 idle, 1 waiting for OFF/AOD, 2 capturing, 3 saved.
static SHOT: AtomicU8 = AtomicU8::new(0);
static SHOT_STARTED: AtomicU32 = AtomicU32::new(0);
static ERROR: AtomicI32 = AtomicI32::new(0);
static SHOT_ERROR: AtomicI32 = AtomicI32::new(0);
static WORKER: AtomicBool = AtomicBool::new(false);
static FRAMES: AtomicU32 = AtomicU32::new(0);
static FPS: AtomicU32 = AtomicU32::new(0);
static CONSTRAINED: AtomicBool = AtomicBool::new(false);
static REVISION: AtomicU32 = AtomicU32::new(0);
static SEQUENCE: AtomicU32 = AtomicU32::new(0);
static LIMITED: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
const FRAME_MS: u32 = 67;
const LIMIT_MS: u32 = 5 * 60 * 1000;
const LIMIT_BYTES: u32 = 16 * 1024 * 1024;
const FRAME_RESERVE: u32 = (format::WIDTH * format::HEIGHT * 2 + 8192) as u32;
const STORAGE_RESERVE: u64 = 2 * 1024 * 1024;
const OUTPUT_BUFFER_BYTES: usize = 16 * 1024;
fn space_low() -> bool {
    unsafe { fw::capture_free_bytes().is_some_and(|n| n < STORAGE_RESERVE + FRAME_RESERVE as u64) }
}

/// One immediate repair is cheap only when the first snapshot was fast.
/// Callers never loop on this: a second crossing is accepted to preserve flow.
pub(crate) fn retry_fast_snapshot(changed: bool, elapsed_ms: u32) -> bool { changed && elapsed_ms <= 4 }

fn changed() { REVISION.fetch_add(1, Ordering::Release); }
fn ensure_worker() -> Result<(), i32> {
    if WORKER.swap(true, Ordering::AcqRel) { return Ok(()); }
    let pid = unsafe { fw::nsh_spawn(b"chaos.capture\0".as_ptr(), worker_entry, ptr::null_mut()) };
    if pid > 0 { Ok(()) } else { WORKER.store(false, Ordering::Release); Err(if pid < 0 { pid } else { -5 }) }
}

pub(crate) fn toggle_recording() {
    if STATE.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire).is_ok() {
        ERROR.store(0, Ordering::Release); LIMITED.store(false, Ordering::Release);
        FRAMES.store(0, Ordering::Release);
        FPS.store(0, Ordering::Release); CONSTRAINED.store(false, Ordering::Release);
        REQUEST_SETTINGS.store(capture_settings::get().pack(), Ordering::Relaxed);
        START_REQUEST.store(true, Ordering::Release);
        if let Err(e) = ensure_worker() {
            START_REQUEST.store(false, Ordering::Release);
            ERROR.store(e, Ordering::Release); STATE.store(0, Ordering::Release);
        }
    } else if matches!(state(), State::Starting | State::Recording) {
        STATE.store(State::Stopping as u8, Ordering::Release);
    }
    changed();
}

/// A second tap cancels the pending shot; recording and the one-shot can coexist.
pub(crate) fn toggle_shot() {
    if SHOT.compare_exchange(1, 0, Ordering::AcqRel, Ordering::Acquire).is_err() {
        if shot_state() == 2 { return; }
        SHOT_ERROR.store(0, Ordering::Release);
        SHOT_STORAGE.store(capture_settings::get().screenshot as u8, Ordering::Relaxed);
        SHOT_STARTED.store(fw::capture_now_ms(), Ordering::Relaxed); SHOT.store(1, Ordering::Release);
        if let Err(e) = ensure_worker() { SHOT_ERROR.store(e, Ordering::Release); SHOT.store(0, Ordering::Release); }
    }
    changed();
}

struct Buffer(*mut u8);
impl Buffer {
    unsafe fn new(size: usize) -> Result<Self, i32> {
        let p = fw::heap_malloc(size as u32) as *mut u8;
        if p.is_null() { Err(-12) } else { Ok(Self(p)) }
    }
    unsafe fn frame(&mut self) -> &mut [u8] { core::slice::from_raw_parts_mut(self.0, format::FRAME_BYTES) }
}
impl Drop for Buffer { fn drop(&mut self) { unsafe { fw::fw_free(self.0 as _); } } }

struct Output {
    fd: i32, bytes: u32, final_path: [u8; 96], temporary: [u8; 96], committed: bool,
    pending: Buffer, pending_len: usize,
}
impl Output {
    unsafe fn create(kind: Kind, video: VideoFormat, storage: Storage) -> Result<Self, i32> {
        if storage == Storage::Media {
            let rc = fw::capture_mkdir_path(b"/data/chaos\0".as_ptr());
            if rc < 0 && rc != -17 { return Err(rc); }
        }
        let rc = fw::capture_mkdir_path(storage.root().as_ptr());
        if rc < 0 && rc != -17 { return Err(rc); }
        for _ in 0..128 {
            let mut name = [0; format::NAME_BYTES];
            format::name_for(&mut name, kind, video, fw::capture_now_ms() / 1000, SEQUENCE.fetch_add(1, Ordering::Relaxed));
            let mut out = Self { fd: -1, bytes: 0, final_path: [0; 96], temporary: [0; 96], committed: true,
                pending: Buffer::new(OUTPUT_BUFFER_BYTES)?, pending_len: 0 };
            format::path_at(&name, false, storage, &mut out.final_path)?;
            format::path_at(&name, true, storage, &mut out.temporary)?;
            // Never overwrite completed captures, including across restarts.
            let probe = fw::capture_result(fw::open(out.final_path.as_ptr(), fw::oflag::RDONLY, 0));
            if probe >= 0 { fw::close(probe); continue; }
            if probe != -2 { return Err(probe); }
            // NuttX O_EXCL=8: reserve the temporary name atomically.
            let fd = fw::capture_result(fw::open(out.temporary.as_ptr(), fw::oflag::WRONLY | fw::oflag::CREAT | 8, 0o644));
            if fd == -17 { continue; }
            if fd < 0 { return Err(fd); }
            out.fd = fd; out.committed = false;
            return Ok(out);
        }
        Err(-17)
    }
    unsafe fn commit(&mut self) -> Result<(), i32> {
        self.flush()?;
        let fd = core::mem::replace(&mut self.fd, -1);
        let rc = fw::capture_result(fw::close(fd));
        if rc < 0 { return Err(rc); }
        let rc = fw::capture_rename(self.temporary.as_ptr(), self.final_path.as_ptr());
        if rc < 0 { return Err(rc); }
        self.committed = true;
        Ok(())
    }
    fn flush(&mut self) -> Result<(), i32> {
        let mut used = 0; let mut interrupted = 0;
        while used < self.pending_len {
            let n = unsafe { fw::nsh_write(self.fd, core::slice::from_raw_parts(self.pending.0.add(used), self.pending_len - used)) };
            if n == -4 && interrupted < 8 { interrupted += 1; continue; }
            if n <= 0 { return Err(if n < 0 { n } else { -5 }); }
            if n as usize > self.pending_len - used { return Err(-5); }
            used += n as usize;
        }
        self.pending_len = 0;
        Ok(())
    }
}
impl Sink for Output {
    fn put(&mut self, bytes: &[u8]) -> Result<(), i32> {
        let mut used = 0;
        while used < bytes.len() {
            let n = (OUTPUT_BUFFER_BYTES - self.pending_len).min(bytes.len() - used);
            unsafe { ptr::copy_nonoverlapping(bytes.as_ptr().add(used), self.pending.0.add(self.pending_len), n); }
            self.pending_len += n; used += n; self.bytes = self.bytes.saturating_add(n as u32);
            if self.pending_len == OUTPUT_BUFFER_BYTES { self.flush()?; }
        }
        Ok(())
    }
}
impl Drop for Output {
    fn drop(&mut self) { unsafe {
        if self.fd >= 0 { fw::close(self.fd); }
        if !self.committed { fw::fs_remove(self.temporary.as_ptr()); }
    } }
}

struct Recording {
    output: Output, dictionary: Option<Buffer>, reader: fw::CaptureReader, snapshot: Buffer, indexed: Buffer, current: Buffer,
    video: VideoFormat, apng: apng::Encoder,
    pending: Option<format::Rect>, timeline: u32, played_ms: u32,
    started: u32, capture_start: u32, sampled: u32, last_sample: u32, space_checked: u32,
    accepted: u32, first_sample: u32,
}
impl Recording {
    unsafe fn new() -> Result<Self, i32> {
        if space_low() { return Err(-28); }
        let settings = Settings::unpack(REQUEST_SETTINGS.load(Ordering::Acquire));
        let dictionary = if settings.format == VideoFormat::Gif { Some(Buffer::new(core::mem::size_of::<Dictionary>())?) } else { None };
        // Establish initialized storage without a large temporary on the task stack.
        if let Some(ref dictionary) = dictionary { ptr::write_bytes(dictionary.0, 0, core::mem::size_of::<Dictionary>()); }
        let reader = fw::CaptureReader::open()?;
        let snapshot = Buffer::new(format::FRAME_BYTES)?;
        let indexed = Buffer::new(format::PIXELS)?;
        let current = Buffer::new(format::PIXELS)?;
        let mut output = Output::create(Kind::Recording, settings.format, settings.recording)?;
        if settings.format == VideoFormat::Gif { format::gif_header(&mut output)?; } else { apng::header(&mut output)?; }
        let now = fw::capture_now_ms();
        Ok(Self { output, dictionary, reader, snapshot, indexed, current, video: settings.format, apng: apng::Encoder::new(),
            pending: None, timeline: now, played_ms: 0,
            started: now, capture_start: now, sampled: 0, last_sample: now, space_checked: now,
            accepted: 0, first_sample: now })
    }
    unsafe fn frame(&mut self) -> Result<(), i32> {
        let attempt = fw::capture_now_ms();
        if self.sampled == 0 { self.capture_start = attempt; }
        // Fresh display events may wake capture immediately. If they stall or
        // coalesce, a bounded timer still samples the current framebuffer.
        let timer_due = self.accepted == 0 || self.video == VideoFormat::Gif
            || attempt.wrapping_sub(self.last_sample) >= 1000u32.div_ceil(self.rate());
        let captured = self.reader.frame(self.snapshot.frame(), timer_due)?;
        if self.reader.missed() != 0 || self.reader.unstable() != 0 { CONSTRAINED.store(true, Ordering::Release); }
        let Some(now) = captured else { return Ok(()); };
        self.advance_deadline(attempt);
        self.last_sample = now;
        if self.accepted == 0 { self.first_sample = now; }
        self.accepted += 1;
        if self.accepted > 1 {
            let elapsed = now.wrapping_sub(self.first_sample).max(1);
            FPS.store((((self.accepted - 1) * 1000 + elapsed / 2) / elapsed).min(999), Ordering::Release);
        }
        let snapshot = self.snapshot.frame();
        let current = core::slice::from_raw_parts_mut(self.current.0, format::PIXELS);
        format::quantize(snapshot, current);
        let indexed = core::slice::from_raw_parts_mut(self.indexed.0, format::PIXELS);
        let rect = if self.pending.is_none() { Some(format::Rect::FULL) }
            else { format::indexed_changed_rect(current, indexed) };
        let Some(rect) = rect else { return Ok(()); }; // keep duration, emit nothing
        if self.pending.is_some() { self.emit(now)?; }
        else { self.timeline = now; }
        core::mem::swap(&mut self.indexed, &mut self.current);
        self.pending = Some(rect);
        FRAMES.fetch_add(1, Ordering::Release);
        Ok(())
    }
    unsafe fn emit(&mut self, now: u32) -> Result<(), i32> {
        let Some(rect) = self.pending else { return Err(-110); };
        // One-frame lookahead knows the duration BEFORE writing the image.
        // Carry centisecond rounding across frames; never seek/rewrite flash.
        let elapsed = now.wrapping_sub(self.timeline);
        let indices = core::slice::from_raw_parts(self.indexed.0, format::PIXELS);
        if self.video == VideoFormat::Gif {
            let delay = (elapsed / 10).saturating_sub(self.played_ms / 10).clamp(1, 65535) as u16;
            format::gif_frame(&mut self.output, indices, rect, delay,
                &mut *self.dictionary.as_mut().unwrap().0.cast::<Dictionary>())?;
            self.played_ms += delay as u32 * 10;
        } else {
            let delay = elapsed.saturating_sub(self.played_ms).max(1);
            self.apng.frame(&mut self.output, indices, rect, delay)?; self.played_ms += delay;
        }
        Ok(())
    }
    fn low_space(&mut self, now: u32) -> bool {
        if now.wrapping_sub(self.space_checked) < 1000 { return false; }
        self.space_checked = now; space_low()
    }
    fn rate(&self) -> u32 { if self.video == VideoFormat::Apng { 60 } else { 15 } }
    fn advance_deadline(&mut self, now: u32) {
        let rate = self.rate();
        self.sampled = (now.wrapping_sub(self.capture_start) * rate + rate - 1) / 1000 + 1;
    }
    fn wait_ms(&self, now: u32) -> u32 {
        if self.sampled == 0 { return 0; }
        // A bounded native wait gives both vsync wakeup and timer fallback.
        // Never add a second sleep after a missing notification.
        if self.video == VideoFormat::Apng { return 0; }
        (self.sampled * 1000 / self.rate()).saturating_sub(now.wrapping_sub(self.capture_start))
    }
    fn due(&self, now: u32) -> bool { self.wait_ms(now) == 0 }
    unsafe fn finish(mut self, now: u32) -> Result<(), i32> {
        self.emit(now)?;
        if self.video == VideoFormat::Gif { self.output.put(&[0x3b])?; }
        else {
            self.apng.end(&mut self.output)?;
            self.output.flush()?;
            let end = self.output.bytes;
            if fw::lseek(self.output.fd, apng::ACTL_OFFSET, fw::seek::SET) != apng::ACTL_OFFSET { return Err(-5); }
            apng::animation_control(&mut self.output, self.apng.frames)?; self.output.flush()?;
            self.output.bytes = end;
            if fw::lseek(self.output.fd, end as i64, fw::seek::SET) != end as i64 { return Err(-5); }
        }
        self.output.commit()
    }
}

struct Worker { recording: Option<Recording>, frame: Option<Buffer> }
impl Worker {
    const fn new() -> Self { Self { recording: None, frame: None } }
    unsafe fn buffer(&mut self) -> Result<&mut Buffer, i32> {
        // OFF screenshots can reuse the recording's owned snapshot buffer.
        if let Some(recording) = self.recording.as_mut() { return Ok(&mut recording.snapshot); }
        if self.frame.is_none() { self.frame = Some(Buffer::new(format::FRAME_BYTES)?); }
        Ok(self.frame.as_mut().unwrap())
    }
    unsafe fn step(&mut self) {
        if state() == State::Starting && self.recording.is_none() && START_REQUEST.swap(false, Ordering::AcqRel) {
            let result = (|| {
                let mut rec = Recording::new()?;
                rec.frame()?;
                Ok(rec)
            })();
            match result {
                Ok(rec) => {
                    self.recording = Some(rec);
                    // An end tap during initialization must not be lost.
                    if FRAMES.load(Ordering::Acquire) != 0 { let _ = STATE.compare_exchange(1, 2, Ordering::AcqRel, Ordering::Acquire); }
                }
                Err(e) => { ERROR.store(e, Ordering::Release); STATE.store(0, Ordering::Release); }
            }
            changed();
        }
        // Initialization performs allocation/IO. Sample AFTER it, otherwise an
        // earlier timestamp minus rec.started wraps and immediately hits the limit.
        let now = fw::capture_now_ms();
        let stop = state() == State::Stopping;
        let low = self.recording.as_mut().is_some_and(|r| r.low_space(now));
        let limit = self.recording.as_ref().is_some_and(|r|
            now.wrapping_sub(r.started) >= LIMIT_MS || r.output.bytes >= LIMIT_BYTES - 2 * FRAME_RESERVE || low);
        if stop || limit {
            START_REQUEST.store(false, Ordering::Release);
            if let Some(rec) = self.recording.take() {
                if let Err(e) = rec.finish(now) { ERROR.store(e, Ordering::Release); }
            }
            if low && error() == 0 { ERROR.store(-28, Ordering::Release); }
            LIMITED.store(limit, Ordering::Release); STATE.store(0, Ordering::Release); changed();
        } else if let Some(rec) = self.recording.as_mut() {
            if !fw::screen_is_on() { rec.reader.pause(); }
            else if rec.due(now) {
                if let Err(e) = rec.frame() {
                    self.recording = None; ERROR.store(e, Ordering::Release); STATE.store(0, Ordering::Release); changed();
                } else if FRAMES.load(Ordering::Acquire) != 0 && STATE.compare_exchange(1, 2, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                    changed();
                }
            }
        }
        if shot_state() == 1 {
            // Bounded one-shot: never silently capture on some later day.
            let start = SHOT_STARTED.load(Ordering::Acquire);
            let now = fw::capture_now_ms();
            if now.wrapping_sub(start) >= 60_000 {
                if SHOT.compare_exchange(1, 0, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                    SHOT_ERROR.store(-110, Ordering::Release); changed();
                }
            } else if !fw::screen_is_on() && SHOT.compare_exchange(1, 2, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                let result = (|| {
                    if space_low() { return Err(-28); }
                    let frame = self.buffer()?;
                    fw::capture_frame(frame.frame())?;
                    let storage = if SHOT_STORAGE.load(Ordering::Acquire) == 1 { Storage::Media } else { Storage::OfflineLog };
                    let mut out = Output::create(Kind::Screenshot, VideoFormat::Gif, storage)?;
                    format::bmp(&mut out, frame.frame())?; out.commit()
                })();
                match result {
                    Ok(()) => { SHOT.store(3, Ordering::Release); }
                    Err(e) => { SHOT_ERROR.store(e, Ordering::Release); SHOT.store(0, Ordering::Release); }
                }
                changed();
            }
        }
        // A standalone screenshot releases its temporary snapshot immediately.
        if shot_state() != 2 { self.frame = None; }
    }
}

unsafe extern "C" fn worker_entry(_argc: i32, _argv: *mut *mut u8) -> i32 {
    let mut worker = Worker::new();
    loop {
        worker.step();
        let active = state() != State::Idle || shot_state() == 1;
        if let Some(recording) = worker.recording.as_mut().filter(|_| fw::screen_is_on()) {
            let remaining = recording.wait_ms(fw::capture_now_ms());
            if remaining != 0 {
                // Pending events don't bypass the format's deadline. Bound
                // sleep so stop/OFF and armed screenshots stay responsive.
                let _ = fw::nsh_usleep(remaining.min(20) * 1000);
            } else { recording.reader.wait_vsync(); }
        } else { let _ = fw::nsh_usleep(if active { 5_000 } else { 200_000 }); }
    }
}

#[cfg(test)]
#[path = "../tests/capture_cases.rs"]
mod tests;
