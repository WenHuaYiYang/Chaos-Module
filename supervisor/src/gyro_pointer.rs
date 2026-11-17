//! Global wrist cursor. All sensor/UI/input work belongs to LVGL's owner thread.
//! The settings callback only changes the gate; input devices are retired later.
use crate::{fw_api as fw, state::SIM_LITE};
use core::{ptr, sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering}};
#[path = "gyro_pointer/logic.rs"]
mod logic;
#[path = "gyro_pointer/config.rs"]
mod config;
use logic::{Controller, Frame, Motion, raw_point};
use config::Config;
pub(crate) use logic::{SENSITIVITY_MIN, SENSITIVITY_MAX, CONFIRM_MIN_MS, CONFIRM_MAX_MS,
                      TOLERANCE_MIN_PX, TOLERANCE_MAX_PX, PAGE_PAUSE_MIN_S, PAGE_PAUSE_MAX_S};

const CONFIG: &[u8] = b"/data/chaos/gyro-pointer.cfg\0";
const TEMP: &[u8] = b"/data/chaos/gyro-pointer.cfg.tmp\0";
// 0 = not loaded, 1 = off, 2 = on.
static VALUE: AtomicU8 = AtomicU8::new(0);
static SENSITIVITY: AtomicU32 = AtomicU32::new(logic::SENSITIVITY_DEFAULT);
static CONFIRM_MS: AtomicU32 = AtomicU32::new(logic::DWELL_MS);
static TOLERANCE_PX: AtomicU32 = AtomicU32::new(logic::TOLERANCE_DEFAULT_PX);
static PAGE_PAUSE_S: AtomicU32 = AtomicU32::new(logic::PAGE_PAUSE_DEFAULT_S);
static SAVE_FAILED: AtomicBool = AtomicBool::new(false);
static EPOCH: AtomicU32 = AtomicU32::new(0);
static ROOT: AtomicU32 = AtomicU32::new(0);
static STATUS: AtomicU8 = AtomicU8::new(0);
const WAITING: u8 = 0;
const CALIBRATING: u8 = 1;
const RUNNING: u8 = 2;
const SENSOR_FAILED: u8 = 3;
const SLEEPING: u8 = 4;

struct Runtime {
    controller: Controller,
    input: u32,
    sensor: i32,
    overlay: Option<fw::GyroOverlay>,
    retry: Option<u32>,
    drawn: Option<(logic::Point, u32, bool)>,
    sleeping: bool,
    sensor_started: u32,
    last_packet: Option<(u64, u32)>,
    page: Option<[u32; 4]>,
    epoch: u32,
}
impl Runtime {
    const fn new() -> Self {
        Self { controller: Controller::new(), input: 0, sensor: -1,
               overlay: None, retry: None, drawn: None, sleeping: false,
               sensor_started: 0, last_packet: None, page: None, epoch: 0 }
    }
    unsafe fn close_sensor(&mut self) {
        if self.sensor >= 0 { fw::close(self.sensor); self.sensor = -1; }
        self.last_packet = None;
    }
    unsafe fn reset(&mut self) {
        self.close_sensor();
        self.controller.suspend(); self.drawn = None;
        self.page = None;
        fw::gyro_input_reset(self.input);
    }
    unsafe fn stop(&mut self) {
        self.reset();
        // Never delete the currently dispatching input from a switch callback.
        let input = self.input; self.input = 0;
        fw::gyro_input_delete(input);
        if let Some(overlay) = self.overlay.take() {
            if ROOT.load(Ordering::Acquire) == overlay.root { fw::obj_delete(overlay.root); }
        }
        ROOT.store(0, Ordering::Release);
        self.retry = None; self.sleeping = false;
    }
    unsafe fn draw(&mut self, frame: Frame) {
        if let Some(overlay) = self.overlay {
            if ROOT.load(Ordering::Acquire) != overlay.root { return; }
            let drawing = (frame.cursor, frame.progress, frame.calibrated);
            if self.drawn != Some(drawing) {
                fw::gyro_cursor_show(overlay, frame.cursor.x, frame.cursor.y,
                                     frame.progress, frame.calibrated);
                self.drawn = Some(drawing);
            }
        }
    }
}
static mut RUNTIME: Runtime = Runtime::new();

pub(crate) unsafe fn enabled() -> bool {
    if VALUE.load(Ordering::Acquire) == 0 {
        let mut bytes = [0u8; config::LENGTH + 1];
        let fd = fw::open(CONFIG.as_ptr(), fw::oflag::RDONLY, 0);
        let n = if fd >= 0 {
            let n = fw::read(fd, bytes.as_mut_ptr(), bytes.len() as u32);
            fw::close(fd); n
        } else { -1 };
        let settings = if n > 0 && n as usize <= bytes.len() {
            Config::decode(&bytes[..n as usize]).unwrap_or(Config::defaults())
        } else { Config::defaults() };
        SENSITIVITY.store(settings.sensitivity, Ordering::Release);
        CONFIRM_MS.store(settings.confirm_ms, Ordering::Release);
        TOLERANCE_PX.store(settings.tolerance_px, Ordering::Release);
        PAGE_PAUSE_S.store(settings.page_pause_s, Ordering::Release);
        VALUE.store(if settings.enabled { 2 } else { 1 }, Ordering::Release);
    }
    VALUE.load(Ordering::Acquire) == 2
}

unsafe fn save(settings: Config) -> bool {
    let bytes = settings.encode();
    let fd = fw::open(TEMP.as_ptr(), fw::oflag::WRONLY | fw::oflag::CREAT | 0x20, 0o640);
    if fd < 0 { return false; }
    let mut used = 0usize;
    while used < bytes.len() {
        let n = fw::write(fd, bytes.as_ptr().add(used), (bytes.len() - used) as u32);
        if n <= 0 || n as usize > bytes.len()-used {
            fw::close(fd); fw::fs_remove(TEMP.as_ptr()); return false;
        }
        used += n as usize;
    }
    if fw::close(fd) < 0 { fw::fs_remove(TEMP.as_ptr()); return false; }
    let fd = fw::open(TEMP.as_ptr(), fw::oflag::RDONLY, 0);
    if fd < 0 { fw::fs_remove(TEMP.as_ptr()); return false; }
    let mut check = [0u8; config::LENGTH + 1];
    let n = fw::read(fd, check.as_mut_ptr(), check.len() as u32);
    let closed = fw::close(fd);
    if n != bytes.len() as i32 || check[..bytes.len()] != bytes || closed < 0 {
        fw::fs_remove(TEMP.as_ptr()); return false;
    }
    if fw::capture_rename(TEMP.as_ptr(), CONFIG.as_ptr()) < 0 {
        fw::fs_remove(TEMP.as_ptr()); return false;
    }
    true
}

pub(crate) unsafe fn toggle() {
    let on = !enabled();
    // Publish off first: the next input frame cannot finish an armed click.
    VALUE.store(if on { 2 } else { 1 }, Ordering::Release);
    EPOCH.fetch_add(1, Ordering::AcqRel);
    STATUS.store(WAITING, Ordering::Release);
    SAVE_FAILED.store(!save(current()), Ordering::Release);
}

unsafe fn current() -> Config {
    Config { enabled: enabled(), sensitivity: SENSITIVITY.load(Ordering::Acquire),
             confirm_ms: CONFIRM_MS.load(Ordering::Acquire), tolerance_px: TOLERANCE_PX.load(Ordering::Acquire),
             page_pause_s: PAGE_PAUSE_S.load(Ordering::Acquire) }
}
pub(crate) unsafe fn sensitivity() -> u32 { current().sensitivity }
pub(crate) unsafe fn confirm_ms() -> u32 { current().confirm_ms }
pub(crate) unsafe fn tolerance_px() -> u32 { current().tolerance_px }
pub(crate) unsafe fn page_pause_s() -> u32 { current().page_pause_s }
pub(crate) fn save_failed() -> bool { SAVE_FAILED.load(Ordering::Acquire) }
pub(crate) unsafe fn set_parameter(index: usize, text: &[u8]) -> bool {
    let Some(value) = config::parse_number(text) else { SAVE_FAILED.store(false, Ordering::Release); return false; };
    let mut next = current();
    match index { 0 => next.sensitivity = value, 1 => next.confirm_ms = value,
                 2 => next.tolerance_px = value, 3 => next.page_pause_s = value, _ => return false }
    if !next.valid() { SAVE_FAILED.store(false, Ordering::Release); return false; }
    if !save(next) { SAVE_FAILED.store(true, Ordering::Release); return false; }
    SENSITIVITY.store(next.sensitivity, Ordering::Release);
    CONFIRM_MS.store(next.confirm_ms, Ordering::Release);
    TOLERANCE_PX.store(next.tolerance_px, Ordering::Release);
    PAGE_PAUSE_S.store(next.page_pause_s, Ordering::Release);
    SAVE_FAILED.store(false, Ordering::Release);
    STATUS.store(WAITING, Ordering::Release); EPOCH.fetch_add(1, Ordering::AcqRel);
    true
}

pub(crate) unsafe fn status() -> *const u8 {
    if SAVE_FAILED.load(Ordering::Acquire) {
        return "设置未保存，请重试\0".as_ptr();
    }
    if !enabled() { return "转腕移动，停稳点击，碰边滑动\0".as_ptr(); }
    match STATUS.load(Ordering::Acquire) {
        RUNNING => "停稳点击，碰边立即同向滑动\0".as_ptr(),
        SENSOR_FAILED => "传感器暂不可用，正在重试\0".as_ptr(),
        SLEEPING => "息屏暂停\0".as_ptr(),
        _ => "准备中，稍候自动恢复\0".as_ptr(),
    }
}

unsafe extern "C" fn overlay_deleted(event: u32) {
    if event != 0 {
        let target = ptr::read_volatile(event as *const u32);
        let _ = ROOT.compare_exchange(target, 0, Ordering::AcqRel, Ordering::Acquire);
    }
}

/// The existing maintenance timer owns creation and deletion, including when Chaos
/// is in the background. The separate indev's timer owns 20ms motion/input frames.
pub(crate) unsafe fn tick() {
    let runtime = &mut *ptr::addr_of_mut!(RUNTIME);
    if !enabled() || crate::st_rd!(SIM_LITE) != 0 {
        if runtime.input != 0 || runtime.overlay.is_some() { runtime.stop(); }
        return;
    }
    if !fw::screen_is_on() { return; }
    let now = fw::capture_now_ms();
    if runtime.retry.is_some_and(|t| now.wrapping_sub(t) < 2000) { return; }
    if runtime.overlay.is_some_and(|o| ROOT.load(Ordering::Acquire) != o.root) {
        runtime.reset(); runtime.overlay = None;
    }
    if runtime.overlay.is_none() {
        runtime.overlay = fw::gyro_cursor_create(overlay_deleted as *const () as u32,
                                                logic::CURSOR_SIZE, logic::DOT_SIZE);
        if let Some(overlay) = runtime.overlay {
            ROOT.store(overlay.root, Ordering::Release); runtime.drawn = None;
        } else {
            runtime.retry = Some(now); STATUS.store(SENSOR_FAILED, Ordering::Release); return;
        }
    }
    if runtime.input == 0 {
        crate::screen_rotation::ensure_applied();
        runtime.controller = Controller::new();
        runtime.epoch = EPOCH.load(Ordering::Acquire);
        runtime.input = fw::gyro_input_create(read_input as *const () as u32);
        if runtime.input == 0 {
            runtime.retry = Some(now); STATUS.store(SENSOR_FAILED, Ordering::Release);
        }
    }
}

/// Read at most eight events per frame: a backed-up stream cannot stall the UI.
unsafe fn latest_motion(runtime: &mut Runtime) -> Result<(Option<Motion>, bool), ()> {
    let mut latest = None;
    for _ in 0..8 {
        let mut bytes = [0u8; 24];
        let n = fw::capture_result(fw::read(runtime.sensor, bytes.as_mut_ptr(), 24));
        if n == -11 || n == -61 || n == 0 { return Ok((latest, false)); }
        if n == -4 { continue; }
        if n != 24 { return Err(()); }
        let motion = Motion {
            timestamp_us: u64::from_le_bytes(bytes[..8].try_into().unwrap()),
            x: f32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            y: f32::from_le_bytes(bytes[12..16].try_into().unwrap()),
            z: f32::from_le_bytes(bytes[16..20].try_into().unwrap()),
        };
        if latest.is_none_or(|old: Motion| motion.timestamp_us > old.timestamp_us) { latest = Some(motion); }
    }
    // Do not act on a backlog. Drain it over bounded frames before recalibrating.
    Ok((None, true))
}

unsafe extern "C" fn read_input(input: u32, data: *mut fw::GyroInputData) {
    if data.is_null() { return; }
    let runtime = &mut *ptr::addr_of_mut!(RUNTIME);
    if input != runtime.input { return; }
    let epoch = EPOCH.load(Ordering::Acquire);
    if runtime.epoch != epoch {
        runtime.reset(); runtime.retry = None; runtime.epoch = epoch;
    }
    let now = fw::capture_now_ms();
    let flipped = crate::screen_rotation::enabled();
    let awake = enabled() && crate::st_rd!(SIM_LITE) == 0 && fw::screen_is_on();
    let overlay_alive = runtime.overlay.is_some_and(|o| ROOT.load(Ordering::Acquire) == o.root);
    let frame;
    if !awake || !overlay_alive {
        frame = runtime.controller.suspend();
        runtime.close_sensor(); runtime.drawn = None;
        if let Some(overlay) = runtime.overlay {
            if overlay_alive && !runtime.sleeping { fw::gyro_cursor_hide(overlay); }
        }
        runtime.sleeping = true;
        runtime.page = None;
        fw::gyro_input_period(input, 1000);
        STATUS.store(SLEEPING, Ordering::Release);
    } else {
        runtime.sleeping = false;
        fw::gyro_input_period(input, 20);
        if runtime.sensor < 0 && runtime.retry.is_none_or(|t| now.wrapping_sub(t) >= 2000) {
            runtime.sensor = fw::gyro_sensor_open();
            runtime.retry = if runtime.sensor < 0 { Some(now) } else { None };
            runtime.sensor_started = now; runtime.last_packet = None;
        }
        if runtime.sensor < 0 {
            frame = runtime.controller.suspend();
            STATUS.store(SENSOR_FAILED, Ordering::Release);
        } else {
            match latest_motion(runtime) {
                Ok((motion, backlog)) => {
                    if let Some(m) = motion {
                        if runtime.last_packet.is_none_or(|(timestamp, _)| m.timestamp_us > timestamp) {
                            runtime.last_packet = Some((m.timestamp_us, now));
                        }
                    }
                    let last_seen = runtime.last_packet.map_or(runtime.sensor_started, |(_, time)| time);
                    if now.wrapping_sub(last_seen) >= 2000 {
                        runtime.close_sensor(); runtime.retry = Some(now);
                        frame = runtime.controller.suspend();
                        STATUS.store(SENSOR_FAILED, Ordering::Release);
                    } else if backlog {
                        frame = runtime.controller.suspend();
                        STATUS.store(CALIBRATING, Ordering::Release);
                    } else {
                        let page = fw::gyro_page_identity();
                        if runtime.page.is_some_and(|old| old != page) {
                            runtime.controller.reset_page(now);
                            fw::gyro_input_reset(input);
                        }
                        runtime.page = Some(page);
                        frame = runtime.controller.configure(sensitivity(), confirm_ms(), tolerance_px(), page_pause_s(), now)
                            .unwrap_or_else(|| runtime.controller.update(now, motion, fw::gyro_physical_touch(input), flipped));
                        STATUS.store(if frame.calibrated { RUNNING } else { CALIBRATING }, Ordering::Release);
                    }
                }
                Err(()) => {
                    runtime.close_sensor(); runtime.retry = Some(now);
                    frame = runtime.controller.suspend();
                    STATUS.store(SENSOR_FAILED, Ordering::Release);
                }
            }
        }
        runtime.draw(frame);
    }
    if frame.reset_input { fw::gyro_input_reset(input); }
    let point = raw_point(frame.touch.point, flipped);
    ptr::write(data, fw::GyroInputData {
        x: point.x, y: point.y, state: u8::from(frame.touch.pressed),
        continue_reading: 0, ..fw::GyroInputData::default()
    });
}

#[cfg(test)]
#[path = "../tests/gyro_pointer_runtime_cases.rs"]
mod tests;
