//! Opt-in, global assistive ball. Native objects are owned by the UI timer;
//! event callbacks only queue actions, positions and configuration changes.
#[path = "floating_ball/curved.rs"]
pub(crate) mod curved;
#[path = "floating_ball/model.rs"]
pub(crate) mod model;
#[path = "floating_ball/pixels.rs"]
pub(crate) mod pixels;
use crate::{fw_api as fw, fw_api::floating_ball as port};
use model::{Action, Config, Gesture, Point, Tween, View, CONFIG_LEN};

const FILE: &[u8] = b"/data/chaos/floating-ball.cfg\0";
const TEMP: &[u8] = b"/data/chaos/floating-ball.cfg.tmp\0";
const MAX_TIMEOUT: u32 = i32::MAX as u32;
static mut CONFIG: Config = Config::DEFAULT;
static mut LOADED: bool = false;
static mut SAVE_PENDING: bool = false;
static mut SAVE_FAILED: bool = false;
static mut OVERLAY: Option<port::Overlay> = None;
static mut POSITION: Point = Point { x: 276, y: 12 };
static mut GESTURE: Gesture = Gesture::NEW;
static mut VIEW: View = View::Closed;
static mut DIRTY: bool = true;
static mut FAN: Tween = Tween::ZERO;
static mut SNAP_X: Tween = Tween::ZERO;
static mut SNAP_Y: Tween = Tween::ZERO;
static mut PANEL: Tween = Tween::ZERO;
static mut ANIMATING: bool = false;
static mut CLOCK: [u8; 6] = *b"--:--\0";
static mut CLOCK_VALID: bool = false;
static mut LAST_CLOCK: u32 = 0;
static mut LAST_TEXT: u32 = 0;
static mut PENDING: Option<Action> = None;
static mut HOME_STARTED: Option<u32> = None;
static mut HOME_LAST: u32 = 0;
static mut HOME_DEPTH: u32 = 0;
static mut PENDING_BRIGHTNESS: Option<u32> = None;
static mut SLIDER_VALUE: u32 = 600;
static mut SLIDER_DRAG: bool = false;
static mut SLIDER_FINE: bool = false;
static mut SLIDER_COORD: i32 = 0;
static mut SLIDER_PROGRESS: i32 = 0;
static mut LAST_BRIGHTNESS: u32 = 0;
static mut BATTERY: Option<u8> = None;
static mut LAST_BATTERY: Option<u32> = None;
static mut MESSAGE_STARTED: Option<u32> = None;
static mut FINE_POINT: Point = Point { x: 0, y: 0 };
static mut FINE_VALUE: u32 = 600;
static mut PENDING_DURATION: Option<usize> = None;
static mut KEEP_STARTED: u32 = 0;
static mut KEEP_DURATION: u32 = 0;
static mut TIMEOUT_LEASE: Option<u32> = None;
static mut LAST_KEEP: u32 = 0;
static mut SHOT_STARTED: Option<u32> = None;
static mut SHOT_QUEUED: bool = false;
static mut MESSAGE: &'static [u8] = b"\0";

unsafe fn read_config(path: *const u8) -> Result<([u8; CONFIG_LEN], usize), i32> {
    let fd = fw::capture_result(fw::open(path, fw::oflag::RDONLY, 0));
    if fd < 0 {
        return Err(fd);
    }
    let mut bytes = [0; CONFIG_LEN];
    let mut used = 0;
    let mut retries = 0;
    let mut result = Ok(());
    while used < bytes.len() {
        let n = fw::nsh_read(fd, &mut bytes[used..]);
        if n == -4 && retries < 8 {
            retries += 1;
            continue;
        }
        if n == 0 {
            break;
        }
        if n < 0 || n as usize > bytes.len() - used {
            result = Err(if n < 0 { n } else { -5 });
            break;
        }
        used += n as usize;
    }
    if result.is_ok() {
        let mut extra = [0];
        if fw::nsh_read(fd, &mut extra) != 0 {
            result = Err(-5);
        }
    }
    let closed = fw::capture_result(fw::close(fd));
    if closed < 0 {
        return Err(closed);
    }
    result.map(|_| (bytes, used))
}
unsafe fn load() {
    if LOADED {
        return;
    }
    LOADED = true;
    CONFIG = read_config(FILE.as_ptr())
        .ok()
        .and_then(|(b, n)| Config::decode(&b[..n]))
        .unwrap_or(Config::DEFAULT);
    POSITION = CONFIG.anchor();
}
unsafe fn persist() -> Result<(), i32> {
    let rc = fw::capture_mkdir_path(b"/data/chaos\0".as_ptr());
    if rc < 0 && rc != -17 {
        return Err(rc);
    }
    let bytes = CONFIG.encode();
    let fd = fw::capture_result(fw::open(
        TEMP.as_ptr(),
        fw::oflag::WRONLY | fw::oflag::CREAT | 0x20,
        0o640,
    ));
    if fd < 0 {
        return Err(fd);
    }
    let mut used = 0;
    let mut retries = 0;
    let mut result = Ok(());
    while used < bytes.len() {
        let n = fw::nsh_write(fd, &bytes[used..]);
        if n == -4 && retries < 8 {
            retries += 1;
            continue;
        }
        if n <= 0 || n as usize > bytes.len() - used {
            result = Err(if n < 0 { n } else { -5 });
            break;
        }
        used += n as usize;
    }
    let closed = fw::capture_result(fw::close(fd));
    if closed < 0 {
        result = Err(closed);
    }
    if result.is_ok() {
        result = match read_config(TEMP.as_ptr()) {
            Ok((found, CONFIG_LEN)) if found == bytes => Ok(()),
            Ok(_) => Err(-5),
            Err(e) => Err(e),
        };
    }
    if result.is_ok() {
        let rc = fw::capture_rename(TEMP.as_ptr(), FILE.as_ptr());
        if rc < 0 {
            result = Err(rc);
        }
    }
    if result.is_err() {
        fw::fs_remove(TEMP.as_ptr());
    }
    result
}

pub(crate) unsafe fn config() -> Config {
    load();
    CONFIG
}
pub(crate) unsafe fn update(next: Config) -> bool {
    load();
    if !next.valid() {
        return false;
    }
    if next != CONFIG {
        CONFIG = next;
        HOME_STARTED = None;
        SAVE_PENDING = true;
        DIRTY = true;
        POSITION = CONFIG.anchor();
        GESTURE = Gesture::NEW;
        VIEW = View::Closed;
        FAN = Tween::ZERO;
        SNAP_X = Tween::ZERO;
        SNAP_Y = Tween::ZERO;
        PANEL = Tween::ZERO;
        crate::render_req(crate::page::Pid::FloatingBall as u32);
    } else if SAVE_FAILED {
        SAVE_PENDING = true;
    }
    true
}
pub(crate) unsafe fn battery_level() -> Option<u8> {
    BATTERY
}
pub(crate) unsafe fn save_failed() -> bool {
    SAVE_FAILED
}
pub(crate) unsafe fn keep_awake() -> bool {
    TIMEOUT_LEASE.is_some()
}
pub(crate) unsafe fn busy() -> bool {
    SAVE_PENDING
        || PENDING.is_some()
        || HOME_STARTED.is_some()
        || PENDING_BRIGHTNESS.is_some()
        || PENDING_DURATION.is_some()
        || SHOT_STARTED.is_some()
        || GESTURE.dragging()
}
unsafe fn view(next: View) {
    let now = fw::capture_now_ms();
    let previous = VIEW;
    FAN = Tween {
        from: FAN.value(now),
        to: if next.band() { 1000 } else { 0 },
        started: now,
        duration: 180,
    };
    let crossfade = previous != next && (previous.band() && next.band() || next == View::Message);
    PANEL = Tween {
        from: if crossfade { 0 } else { 1000 },
        to: 1000,
        started: now,
        duration: 180,
    };
    if next != View::Brightness {
        SLIDER_DRAG = false;
        SLIDER_FINE = false;
    }
    VIEW = next;
    MESSAGE_STARTED = if next == View::Message {
        Some(now)
    } else {
        None
    };
    DIRTY = true;
    LAST_TEXT = now.wrapping_sub(1000);
    port::frames_active(true);
}
unsafe fn message(text: &'static [u8]) {
    MESSAGE = text;
    view(View::Message);
}
unsafe fn release_keep_awake() {
    KEEP_DURATION = 0;
    KEEP_STARTED = 0;
    DIRTY = true;
    if let Some(previous) = TIMEOUT_LEASE.take() {
        // A change made in system settings while our lease was active wins.
        if fw::screen_timeout_ms() == MAX_TIMEOUT {
            fw::screen_timeout_set(previous);
        }
    }
}

unsafe extern "C" fn event(event: u32) {
    handle(port::input(event), fw::capture_now_ms());
}

pub(crate) unsafe fn handle(input: port::Input, now: u32) {
    if input.tag == port::ROOT && input.code == 36 {
        // The native tree is already being deleted. Never delete from this callback.
        OVERLAY = None;
        VIEW = View::Closed;
        GESTURE.cancel();
        DIRTY = true;
        return;
    }
    if !CONFIG.enabled || SHOT_STARTED.is_some() {
        return;
    }
    if input.tag == port::BALL {
        match input.code {
            1 => {
                HOME_STARTED = None;
                if SNAP_X.duration != 0 || SNAP_Y.duration != 0 {
                    POSITION = Point {
                        x: SNAP_X.value(now),
                        y: SNAP_Y.value(now),
                    };
                    SNAP_X = Tween::ZERO;
                    SNAP_Y = Tween::ZERO;
                }
                GESTURE.press(input.point, POSITION, now);
            }
            5 => {
                GESTURE.hold();
                view(View::Closed);
            }
            2 => {
                if let Some(position) = GESTURE.motion(input.point, now, CONFIG.size) {
                    POSITION = position;
                    view(View::Closed);
                    DIRTY = true;
                }
            }
            8 => {
                if let Some(dock) = GESTURE.release(input.point, now, CONFIG.size) {
                    if dock != CONFIG.dock {
                        CONFIG.dock = dock;
                        SAVE_PENDING = true;
                    }
                }
                let target = CONFIG.anchor();
                SNAP_X = Tween {
                    from: POSITION.x,
                    to: target.x,
                    started: now,
                    duration: 200,
                };
                SNAP_Y = Tween {
                    from: POSITION.y,
                    to: target.y,
                    started: now,
                    duration: 200,
                };
                POSITION = target;
                DIRTY = true;
            }
            3 | 20 => {
                GESTURE.cancel();
                POSITION = CONFIG.anchor();
                DIRTY = true;
            }
            7 if GESTURE.can_click() => view(
                if matches!(
                    VIEW,
                    View::Closed | View::Duration | View::Brightness | View::Message
                ) {
                    View::Fan
                } else {
                    View::Closed
                },
            ),
            _ => {}
        }
        port::frames_active(true);
        return;
    }
    if input.tag == port::BAND {
        if FAN.value(now) < 850 || PANEL.value(now) < 500 {
            return;
        }
        if VIEW == View::Brightness {
            match input.code {
                1 => {
                    SLIDER_FINE = false;
                    SLIDER_DRAG = CONFIG.band_sector(input.point).is_some();
                    if SLIDER_DRAG {
                        // Relative capture: touching anywhere never jumps the value.
                        FINE_POINT = input.point;
                        SLIDER_COORD = CONFIG.band_progress(input.point) as i32;
                        SLIDER_PROGRESS = model::brightness_progress(SLIDER_VALUE) as i32;
                    }
                }
                2 if SLIDER_DRAG => slider(input.point),
                5 if SLIDER_DRAG
                    && (input.point.x - FINE_POINT.x).abs() <= 6
                    && (input.point.y - FINE_POINT.y).abs() <= 6 =>
                {
                    SLIDER_FINE = true;
                    FINE_POINT = input.point;
                    FINE_VALUE = SLIDER_VALUE;
                }
                8 => {
                    if SLIDER_DRAG {
                        slider(input.point);
                    }
                    SLIDER_DRAG = false;
                    SLIDER_FINE = false;
                }
                3 | 20 => {
                    SLIDER_DRAG = false;
                    SLIDER_FINE = false;
                }
                _ => {}
            }
        } else if input.code == 7 {
            if let Some(index) = CONFIG.band_sector(input.point) {
                if VIEW == View::Fan {
                    let action = CONFIG.slots[index];
                    if action != Action::Time {
                        PENDING = Some(action);
                    }
                } else if VIEW == View::Duration {
                    PENDING_DURATION = Some(index);
                }
            } else {
                view(View::Closed);
            }
        }
        port::frames_active(true);
        return;
    }
    if input.code != 7 {
        return;
    }
    match input.tag {
        port::DISMISS => view(View::Closed),
        port::CLOSE => view(View::Fan),
        _ => {}
    }
}

unsafe fn slider(point: Point) {
    let value = if SLIDER_FINE {
        let origin = CONFIG.band_coordinates(FINE_POINT);
        let now = CONFIG.band_coordinates(point);
        model::adjusted_brightness(FINE_VALUE, (now.y - now.x) - (origin.y - origin.x))
    } else {
        let coordinate = CONFIG.band_progress(point) as i32;
        let delta = coordinate - SLIDER_COORD;
        // Small jitter accumulates before affecting hardware; reverse motion at
        // either endpoint starts changing immediately, without a dead zone.
        if delta.abs() < 3 {
            return;
        }
        SLIDER_COORD = coordinate;
        SLIDER_PROGRESS = (SLIDER_PROGRESS + delta).clamp(0, 1000);
        model::slider_brightness(SLIDER_PROGRESS as u32)
    };
    if SLIDER_VALUE != value {
        SLIDER_VALUE = value;
        PENDING_BRIGHTNESS = Some(value);
        DIRTY = true;
    }
}
unsafe fn active_preset() -> Option<u8> {
    if TIMEOUT_LEASE.is_none() {
        return None;
    }
    model::KEEP_MINUTES
        .iter()
        .position(|&m| m * 60_000 == KEEP_DURATION)
        .map(|i| i as u8)
}

unsafe fn actions(now: u32) {
    if let Some(action) = PENDING.take() {
        match action {
            Action::Time => {}
            Action::Brightness => {
                SLIDER_VALUE = fw::brightness_get_target().clamp(1, 2000);
                view(View::Brightness);
            }
            Action::Screenshot => {
                view(View::Closed);
                SHOT_STARTED = Some(now);
                SHOT_QUEUED = false;
            }
            Action::KeepAwake => view(View::Duration),
            Action::Watchface => {
                view(View::Closed);
                HOME_DEPTH = fw::page_stack_depth();
                if fw::top_app_id() == fw::HOME_APP_ID && fw::watchface_is_on() != 0 {
                    HOME_STARTED = None;
                } else if fw::floating_ball_home() {
                    HOME_STARTED = Some(now);
                    HOME_LAST = now;
                } else {
                    HOME_STARTED = None;
                    message("返回表盘暂不可用\0".as_bytes());
                }
            }
            Action::Vibration => {
                let muted = !fw::vibration_muted();
                fw::vibration_set_muted(muted);
                message(if muted {
                    "震动已关闭\0".as_bytes()
                } else {
                    "震动已开启\0".as_bytes()
                });
            }
            Action::AutoBrightness => {
                PENDING_BRIGHTNESS = None;
                let on = fw::brightness_get_auto_adjustment() == 0;
                fw::brightness_set_auto_adjustment(on as u32);
                message(if on {
                    "自动亮度已开启\0".as_bytes()
                } else {
                    "自动亮度已关闭\0".as_bytes()
                });
            }
            Action::Recording => {
                crate::capture::toggle_recording();
                message(match crate::capture::state() {
                    crate::capture::State::Starting => "正在启动录屏\0".as_bytes(),
                    crate::capture::State::Recording => "正在录屏\0".as_bytes(),
                    crate::capture::State::Stopping => "正在保存录屏\0".as_bytes(),
                    crate::capture::State::Idle => "录屏启动失败\0".as_bytes(),
                });
            }
            Action::Rotate => {
                let flipped = crate::screen_rotation::toggle();
                message(if flipped {
                    "已翻转屏幕\0".as_bytes()
                } else {
                    "已恢复屏幕方向\0".as_bytes()
                });
            }
        }
    }
    if let Some(index) = PENDING_DURATION.take() {
        if let Some(&minutes) = model::KEEP_MINUTES.get(index) {
            if TIMEOUT_LEASE.is_some() && fw::screen_timeout_ms() != MAX_TIMEOUT {
                TIMEOUT_LEASE = None;
            }
            if active_preset() == Some(index as u8) {
                release_keep_awake();
            } else {
                if TIMEOUT_LEASE.is_none() {
                    TIMEOUT_LEASE = Some(fw::screen_timeout_ms());
                    fw::screen_timeout_set(MAX_TIMEOUT);
                }
                KEEP_STARTED = now;
                KEEP_DURATION = minutes * 60_000;
                LAST_KEEP = now;
            }
            view(View::Fan);
        }
    }
    apply_brightness(now);
    if PENDING_BRIGHTNESS.is_none() && VIEW == View::Brightness && !SLIDER_DRAG {
        let target = fw::brightness_get_target().clamp(1, 2000);
        if SLIDER_VALUE != target {
            SLIDER_VALUE = target;
            DIRTY = true;
        }
    }
    if TIMEOUT_LEASE.is_some() {
        if fw::screen_timeout_ms() != MAX_TIMEOUT {
            TIMEOUT_LEASE = None;
            KEEP_DURATION = 0;
            DIRTY = true;
        } else if now.wrapping_sub(KEEP_STARTED) >= KEEP_DURATION {
            release_keep_awake();
        } else if now.wrapping_sub(LAST_KEEP) >= 1000 {
            fw::screen_timeout_refresh();
            LAST_KEEP = now;
        }
    }
}
/// Coalesce pointer events to <= 31 writes/sec. The firmware already eases
/// each hardware target. This path does not read files or allocate objects.
unsafe fn apply_brightness(now: u32) {
    if PENDING_BRIGHTNESS.is_none() || now.wrapping_sub(LAST_BRIGHTNESS) < 32 {
        return;
    }
    if let Some(value) = PENDING_BRIGHTNESS.take() {
        LAST_BRIGHTNESS = now;
        if fw::brightness_get_auto_adjustment() != 0 {
            fw::brightness_set_auto_adjustment(0);
        }
        fw::brightness_apply_raw(value.clamp(1, 2000));
    }
}
unsafe fn battery(now: u32) {
    if SLIDER_DRAG
        || GESTURE.dragging()
        || FAN.active(now)
        || SNAP_X.active(now)
        || SNAP_Y.active(now)
    {
        return;
    }
    if LAST_BATTERY.is_some_and(|last| now.wrapping_sub(last) < 5000) {
        return;
    }
    LAST_BATTERY = Some(now);
    let next = fw::battery_percent();
    if BATTERY != next {
        BATTERY = next;
        DIRTY = true;
        crate::render_req(crate::page::Pid::FloatingBall as u32);
    }
}
/// Finish only after the stock watchface is actually on top. Some native
/// stacks finish their current page before the app-level home request takes
/// effect; continue the same request at a bounded UI cadence in that case.
unsafe fn finish_home(now: u32) {
    let Some(started) = HOME_STARTED else {
        return;
    };
    if fw::top_app_id() == fw::HOME_APP_ID && fw::watchface_is_on() != 0 {
        HOME_STARTED = None;
        return;
    }
    let depth = fw::page_stack_depth();
    if depth != 0 && HOME_DEPTH != 0 && depth > HOME_DEPTH {
        // New navigation by the user takes priority over this pending action.
        HOME_STARTED = None;
        return;
    }
    if depth != 0 {
        HOME_DEPTH = depth;
    }
    if now.wrapping_sub(started) >= 8000 {
        HOME_STARTED = None;
        message("返回表盘未完成，请重试\0".as_bytes());
        return;
    }
    if now.wrapping_sub(HOME_LAST) >= 600 {
        HOME_LAST = now;
        if !fw::floating_ball_home() {
            HOME_STARTED = None;
            message("返回表盘暂不可用\0".as_bytes());
        }
    }
}
unsafe fn screenshot(now: u32) {
    let Some(started) = SHOT_STARTED else {
        return;
    };
    if !SHOT_QUEUED {
        // At least 150ms with the whole overlay hidden, using the existing UI timer.
        if now.wrapping_sub(started) < 150 {
            return;
        }
        match crate::capture::request_shot_now() {
            Ok(()) => SHOT_QUEUED = true,
            Err(_) => {
                SHOT_STARTED = None;
                message("截屏启动失败\0".as_bytes());
            }
        }
    } else if !matches!(crate::capture::shot_state(), 1 | 2 | 4) {
        SHOT_STARTED = None;
        SHOT_QUEUED = false;
        message(if crate::capture::shot_state() == 3 {
            "截屏已保存\0".as_bytes()
        } else {
            "截屏失败\n请检查存储空间\0".as_bytes()
        });
    }
}
unsafe fn text(ui: &port::Overlay, now: u32) {
    if now.wrapping_sub(LAST_TEXT) < 1000 {
        return;
    }
    LAST_TEXT = now;
    if VIEW == View::Message {
        ui.text("灵动点\0".as_bytes(), MESSAGE);
    }
}

unsafe fn clock(now: u32) {
    if CLOCK_VALID && now.wrapping_sub(LAST_CLOCK) < 1000 {
        return;
    }
    LAST_CLOCK = now;
    CLOCK_VALID = true;
    if let Some(tm) =
        fw::local_time_now().filter(|t| (0..24).contains(&t[2]) && (0..60).contains(&t[1]))
    {
        let h = tm[2] as u8;
        let m = tm[1] as u8;
        let next = [
            b'0' + h / 10,
            b'0' + h % 10,
            b':',
            b'0' + m / 10,
            b'0' + m % 10,
            0,
        ];
        if CLOCK != next {
            CLOCK = next;
            if VIEW == View::Fan {
                DIRTY = true;
            }
        }
    }
}

/// The 16ms path contains no config IO, framebuffer capture or object allocation.
unsafe extern "C" fn frame(_timer: u32) {
    if !CONFIG.enabled || !fw::screen_is_on() || SHOT_STARTED.is_some() {
        port::frames_active(false);
        return;
    }
    let now = fw::capture_now_ms();
    apply_brightness(now);
    paint(now);
}
unsafe fn paint(now: u32) {
    let active = FAN.active(now) || SNAP_X.active(now) || SNAP_Y.active(now) || PANEL.active(now);
    if let Some(ui) = OVERLAY.as_mut() {
        if !ui.live() {
            port::frames_active(false);
            return;
        }
        if DIRTY || active || ANIMATING {
            let position = if SNAP_X.duration != 0 || SNAP_Y.duration != 0 {
                Point {
                    x: SNAP_X.value(now),
                    y: SNAP_Y.value(now),
                }
            } else {
                POSITION
            };
            let ball_pending = !ui.ball(CONFIG, position, BATTERY);
            let fan_level = FAN.value(now).clamp(0, 1000) as u32;
            let shown = if VIEW == View::Closed && fan_level > 0 {
                View::Fan
            } else {
                VIEW
            };
            let pending = ui.render(
                CONFIG,
                shown,
                fan_level,
                PANEL.value(now).clamp(0, 1000) as u32,
                &CLOCK,
                active_preset(),
                SLIDER_VALUE,
            );
            ANIMATING = active || pending || ball_pending;
            DIRTY = false;
        }
    } else {
        ANIMATING = false;
    }
    if !active && !ANIMATING {
        port::frames_active(GESTURE.dragging() || SLIDER_DRAG || PENDING_BRIGHTNESS.is_some());
    } else {
        port::frames_active(true);
    }
}

pub(crate) unsafe fn tick() {
    load();
    if SAVE_PENDING {
        SAVE_PENDING = false;
        SAVE_FAILED = persist().is_err();
        crate::render_req(crate::page::Pid::FloatingBall as u32);
    }
    if !CONFIG.enabled || !fw::screen_is_on() {
        PENDING = None;
        HOME_STARTED = None;
        PENDING_BRIGHTNESS = None;
        PENDING_DURATION = None;
        SLIDER_DRAG = false;
        SLIDER_FINE = false;
        release_keep_awake();
        GESTURE.cancel();
        VIEW = View::Closed;
        DIRTY = true;
        FAN = Tween::ZERO;
        PANEL = Tween::ZERO;
        SNAP_X = Tween::ZERO;
        SNAP_Y = Tween::ZERO;
        ANIMATING = false;
        port::frames_active(false);
        if !CONFIG.enabled {
            if let Some(ui) = OVERLAY.take() {
                ui.destroy();
            }
        } else if let Some(ui) = OVERLAY.as_mut() {
            if ui.live() {
                ui.show(false);
            } else {
                OVERLAY = None;
            }
        }
        // 2026-11-19: 这里原来只在"没排队"时清 SHOT_STARTED。一旦请求已经交给
        // capture(`SHOT_QUEUED = true`)之后紧接着息屏或关掉灵动点, 这个分支就再也
        // 走不到 screenshot(), SHOT_STARTED 永远留着 → busy() 恒真 → 维护节拍被钉在
        // 50ms(息屏也一样), 圆点全部输入失效(handle 开头直接 return), 直到下次亮屏
        // 才"复活"并弹一条与当下操作无关的提示。截图任务本身由 capture 的 worker
        // 自行收尾, 灵动点不需要等它, 所以这里两个标志都清掉。
        SHOT_STARTED = None;
        SHOT_QUEUED = false;
        return;
    }
    let now = fw::capture_now_ms();
    if OVERLAY.as_ref().is_some_and(|ui| !ui.live()) {
        OVERLAY = None;
        VIEW = View::Closed;
        FAN = Tween::ZERO;
        PANEL = Tween::ZERO;
        SLIDER_DRAG = false;
        DIRTY = true;
    }
    if OVERLAY.is_none() {
        OVERLAY = port::Overlay::create(event as *const () as u32 | 1);
        port::ensure_frames(frame as *const () as u32 | 1);
        POSITION = CONFIG.anchor();
        DIRTY = true;
    }
    actions(now);
    finish_home(now);
    clock(now);
    battery(now);
    if MESSAGE_STARTED.is_some_and(|started| now.wrapping_sub(started) >= 2000) {
        view(View::Closed);
    }
    if let Some(ui) = OVERLAY.as_mut() {
        ui.show(SHOT_STARTED.is_none());
    }
    screenshot(now);
    if let Some(ui) = OVERLAY.as_mut() {
        if SHOT_STARTED.is_some() {
            ui.show(false);
            return;
        }
        ui.show(true);
        text(ui, now);
    }
    paint(now);
}

#[cfg(test)]
pub(crate) unsafe fn reset_for_test() {
    CONFIG = Config::DEFAULT;
    LOADED = false;
    SAVE_PENDING = false;
    SAVE_FAILED = false;
    OVERLAY = None;
    POSITION = CONFIG.anchor();
    GESTURE = Gesture::NEW;
    VIEW = View::Closed;
    DIRTY = true;
    FAN = Tween::ZERO;
    SNAP_X = Tween::ZERO;
    SNAP_Y = Tween::ZERO;
    PANEL = Tween::ZERO;
    ANIMATING = false;
    CLOCK = *b"--:--\0";
    CLOCK_VALID = false;
    LAST_CLOCK = 0;
    LAST_TEXT = 0;
    PENDING = None;
    HOME_STARTED = None;
    HOME_LAST = 0;
    HOME_DEPTH = 0;
    PENDING_BRIGHTNESS = None;
    PENDING_DURATION = None;
    SLIDER_DRAG = false;
    SLIDER_FINE = false;
    FINE_POINT = Point { x: 0, y: 0 };
    FINE_VALUE = 600;
    SLIDER_VALUE = 600;
    KEEP_STARTED = 0;
    KEEP_DURATION = 0;
    TIMEOUT_LEASE = None;
    LAST_KEEP = 0;
    SHOT_STARTED = None;
    SHOT_QUEUED = false;
    MESSAGE = b"\0";
    MESSAGE_STARTED = None;
    BATTERY = None;
    LAST_BATTERY = None;
    SLIDER_COORD = 0;
    SLIDER_PROGRESS = 0;
    LAST_BRIGHTNESS = 0;
}

#[cfg(test)]
#[path = "../tests/floating_ball_cases.rs"]
mod tests;
