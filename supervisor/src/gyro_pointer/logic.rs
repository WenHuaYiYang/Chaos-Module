//! Gyroscope cursor and dwell gestures, independent of firmware and LVGL.

pub const WIDTH: i32 = 336;
pub const HEIGHT: i32 = 480;
pub const DWELL_MS: u32 = 900;
pub const SENSITIVITY_DEFAULT: u32 = 100;
pub const SENSITIVITY_MIN: u32 = 25;
pub const SENSITIVITY_MAX: u32 = 300;
pub const CONFIRM_MIN_MS: u32 = 300;
pub const CONFIRM_MAX_MS: u32 = 5000;
pub const TOLERANCE_DEFAULT_PX: u32 = 6;
pub const TOLERANCE_MIN_PX: u32 = 1;
pub const TOLERANCE_MAX_PX: u32 = 30;
pub const CURSOR_SIZE: u32 = 48;
pub const DOT_SIZE: u32 = 20;
pub const SAMPLE_TIMEOUT_MS: u32 = 160;
pub const PAGE_PAUSE_DEFAULT_S: u32 = 2;
pub const PAGE_PAUSE_MIN_S: u32 = 0;
pub const PAGE_PAUSE_MAX_S: u32 = 10;
const CALIBRATION_MS: u32 = 450;
const CALIBRATION_TIMEOUT_MS: u32 = 2000;
const GAIN: f32 = 420.0; // pixels per radian
const DEAD_ZONE: f32 = 0.025;
pub const MARGIN: f32 = CURSOR_SIZE as f32 / 2.0;
const EDGE: i32 = MARGIN as i32;
const REARM_DISTANCE: f32 = 14.0;
const COOLDOWN_MS: u32 = 700;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point { pub x: i32, pub y: i32 }

#[derive(Clone, Copy, Debug)]
pub struct Motion {
    pub timestamp_us: u64,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Motion {
    fn valid(self) -> bool {
        [self.x, self.y, self.z].iter().all(|v| v.is_finite() && v.abs() <= 40.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Touch { pub point: Point, pub pressed: bool }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub cursor: Point,
    pub progress: u32,
    pub touch: Touch,
    pub calibrated: bool,
    /// Cancel the LVGL press before delivering release, so cancellation cannot click.
    pub reset_input: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Zone { Center, Top, Bottom, Left, Right }

fn zone(p: Point) -> Zone {
    let vertical = p.y.min(HEIGHT - 1 - p.y);
    let horizontal = p.x.min(WIDTH - 1 - p.x);
    if vertical <= EDGE && vertical <= horizontal {
        if p.y < HEIGHT / 2 { Zone::Top } else { Zone::Bottom }
    } else if horizontal <= EDGE {
        if p.x < WIDTH / 2 { Zone::Left } else { Zone::Right }
    } else { Zone::Center }
}

fn distance_squared(a: Point, b: Point) -> i32 {
    let dx = a.x - b.x; let dy = a.y - b.y;
    dx * dx + dy * dy
}

#[derive(Clone, Copy)]
struct Dwell { start: u32, x: f32, y: f32 }

#[derive(Clone, Copy)]
struct Gesture { start: u32, from: Point, to: Point, duration: u32, zone: Zone }

impl Gesture {
    fn new(now: u32, p: Point, at: Zone) -> Self {
        let (from, to) = match at {
            Zone::Top => (Point { x: p.x, y: HEIGHT * 3 / 4 }, Point { x: p.x, y: HEIGHT / 4 }),
            Zone::Bottom => (Point { x: p.x, y: HEIGHT / 4 }, Point { x: p.x, y: HEIGHT * 3 / 4 }),
            Zone::Left => (Point { x: WIDTH * 3 / 4, y: p.y }, Point { x: WIDTH / 4, y: p.y }),
            Zone::Right => (Point { x: WIDTH / 4, y: p.y }, Point { x: WIDTH * 3 / 4, y: p.y }),
            Zone::Center => (p, p),
        };
        Self { start: now, from, to, duration: if at == Zone::Center { 60 } else { 300 }, zone: at }
    }
    fn touch(self, now: u32) -> Touch {
        let elapsed = now.wrapping_sub(self.start).min(self.duration);
        let point = Point {
            x: self.from.x + (self.to.x - self.from.x) * elapsed as i32 / self.duration as i32,
            y: self.from.y + (self.to.y - self.from.y) * elapsed as i32 / self.duration as i32,
        };
        Touch { point, pressed: elapsed < self.duration }
    }
}

pub struct Controller {
    x: f32,
    y: f32,
    last_timestamp: Option<u64>,
    last_sample: Option<u32>,
    calibration_since: Option<u32>,
    calibration_start: Option<u32>,
    calibration_count: u32,
    calibration_sum: [f32; 3],
    calibration_first: [f32; 3],
    bias: [f32; 3],
    calibrated: bool,
    page_pause: Option<u32>,
    rate: [f32; 2],
    edge_zone: Zone,
    dwell: Option<Dwell>,
    gesture: Option<Gesture>,
    last_action: Option<u32>,
    latch: Option<Point>,
    touch: Touch,
    flipped: Option<bool>,
    sensitivity: u32,
    confirm_ms: u32,
    tolerance_px: u32,
    page_pause_s: u32,
}

impl Controller {
    pub const fn new() -> Self {
        let center = Point { x: WIDTH / 2, y: HEIGHT / 2 };
        Self {
            x: center.x as f32, y: center.y as f32,
            last_timestamp: None, last_sample: None, calibration_since: None,
            calibration_start: None, calibration_count: 0, calibration_sum: [0.0; 3],
            calibration_first: [0.0; 3], bias: [0.0; 3], calibrated: false,
            page_pause: None, rate: [0.0; 2], edge_zone: Zone::Center, dwell: None, gesture: None,
            last_action: None, latch: None, touch: Touch { point: center, pressed: false },
            flipped: None,
            sensitivity: SENSITIVITY_DEFAULT, confirm_ms: DWELL_MS,
            tolerance_px: TOLERANCE_DEFAULT_PX,
            page_pause_s: PAGE_PAUSE_DEFAULT_S,
        }
    }

    fn point(&self) -> Point { Point { x: (self.x + 0.5) as i32, y: (self.y + 0.5) as i32 } }

    /// Configuration changes start a fresh dwell and cancel any old press first.
    pub fn configure(&mut self, sensitivity: u32, confirm_ms: u32, tolerance_px: u32,
                     page_pause_s: u32, now: u32) -> Option<Frame> {
        let sensitivity = sensitivity.clamp(SENSITIVITY_MIN, SENSITIVITY_MAX);
        let confirm_ms = confirm_ms.clamp(CONFIRM_MIN_MS, CONFIRM_MAX_MS);
        let tolerance_px = tolerance_px.clamp(TOLERANCE_MIN_PX, TOLERANCE_MAX_PX);
        let page_pause_s = page_pause_s.clamp(PAGE_PAUSE_MIN_S, PAGE_PAUSE_MAX_S);
        if self.sensitivity == sensitivity && self.confirm_ms == confirm_ms
            && self.tolerance_px == tolerance_px && self.page_pause_s == page_pause_s { return None; }
        self.sensitivity = sensitivity; self.confirm_ms = confirm_ms; self.tolerance_px = tolerance_px;
        self.page_pause_s = page_pause_s;
        Some(self.cancel(now))
    }

    fn frame(&self, progress: u32, reset_input: bool) -> Frame {
        Frame { cursor: self.point(), progress, touch: self.touch,
                calibrated: self.calibrated && self.page_pause.is_none(), reset_input }
    }

    fn cancel(&mut self, now: u32) -> Frame {
        let was_pressed = self.touch.pressed;
        self.touch.pressed = false;
        self.dwell = None; self.gesture = None; self.last_action = Some(now);
        self.edge_zone = Zone::Center;
        self.rate = [0.0; 2];
        self.frame(0, was_pressed)
    }

    /// Screen off, disabled, or failed sensor. Preparation is bounded; retain good bias.
    pub fn suspend(&mut self) -> Frame {
        let previous = self.touch;
        let settings = (self.sensitivity, self.confirm_ms, self.tolerance_px, self.page_pause_s);
        let bias = self.bias;
        *self = Self::new();
        self.sensitivity = settings.0; self.confirm_ms = settings.1;
        self.tolerance_px = settings.2; self.page_pause_s = settings.3; self.bias = bias;
        self.touch = Touch { pressed: false, ..previous };
        self.frame(0, previous.pressed)
    }

    /// Discard the old page's target and wait the configured time, regardless of motion.
    pub fn reset_page(&mut self, now: u32) -> Frame {
        self.cancel(now);
        self.x = WIDTH as f32 / 2.0; self.y = HEIGHT as f32 / 2.0;
        self.touch = Touch { point: self.point(), pressed: false };
        self.latch = None; self.last_sample = None;
        self.page_pause = Some(now);
        // Keep the sensor timestamp and learned bias; motion during a page animation
        // is neither cursor displacement nor a new zero-rate calibration sample.
        self.frame(0, true)
    }

    fn calibrate(&mut self, now: u32, m: Motion) {
        let since = *self.calibration_since.get_or_insert(now);
        let axes = [m.x, m.y, m.z];
        // Quiet samples may improve bias, but movement never blocks recovery forever.
        if axes.iter().any(|v| v.abs() > 0.25) {
            self.calibration_start = None; self.calibration_count = 0;
            self.calibration_sum = [0.0; 3];
        } else {
            if self.calibration_start.is_none()
                || axes.iter().zip(self.calibration_first).any(|(v, first)| (*v - first).abs() > 0.035) {
                self.calibration_start = Some(now); self.calibration_count = 0;
                self.calibration_sum = [0.0; 3]; self.calibration_first = axes;
            }
            self.calibration_count += 1;
            for (sum, v) in self.calibration_sum.iter_mut().zip(axes) { *sum += v; }
            if self.calibration_count >= 20
                && now.wrapping_sub(self.calibration_start.unwrap_or(now)) >= CALIBRATION_MS {
                for i in 0..3 { self.bias[i] = self.calibration_sum[i] / self.calibration_count as f32; }
                self.calibrated = true; self.dwell = None;
            }
        }
        if now.wrapping_sub(since) >= CALIBRATION_TIMEOUT_MS {
            // Retain the previous bias (zero on first use), never learn a turn as zero.
            self.calibrated = true; self.dwell = None;
        }
    }

    fn sample(&mut self, now: u32, m: Motion, flipped: bool) {
        // Duplicate, backwards and non-finite samples are never a freshness signal.
        if !m.valid() || self.last_timestamp.is_some_and(|t| m.timestamp_us <= t) { return; }
        let dt = self.last_timestamp.map_or(0, |t| m.timestamp_us - t);
        self.last_timestamp = Some(m.timestamp_us); self.last_sample = Some(now);
        if self.page_pause.is_some() { return; }
        if !self.calibrated { self.calibrate(now, m); return; }
        if dt == 0 || dt > 100_000 { self.dwell = None; self.rate = [0.0; 2]; return; }
        let axes = [m.y - self.bias[1], m.x - self.bias[0]];
        let sign = if flipped { -1.0 } else { 1.0 };
        for (filtered, value) in self.rate.iter_mut().zip(axes) {
            let value = if value.abs() < DEAD_ZONE { 0.0 } else { value };
            *filtered += (value - *filtered) * 0.35;
        }
        let gain = GAIN * self.sensitivity as f32 / 100.0;
        self.x = (self.x + self.rate[0] * sign * gain * dt as f32 / 1_000_000.0)
            .clamp(MARGIN, WIDTH as f32 - 1.0 - MARGIN);
        self.y = (self.y + self.rate[1] * sign * gain * dt as f32 / 1_000_000.0)
            .clamp(MARGIN, HEIGHT as f32 - 1.0 - MARGIN);
    }

    /// One call per input frame, using the latest sensor packet (if any).
    pub fn update(&mut self, now: u32, motion: Option<Motion>, physical_touch: bool,
                  flipped: bool) -> Frame {
        let invalid = motion.is_some_and(|m| !m.valid());
        let gap = self.last_sample.is_some_and(|t| now.wrapping_sub(t) > SAMPLE_TIMEOUT_MS);
        if invalid || gap {
            self.calibrated = false; self.calibration_since = None; self.calibration_start = None;
            self.calibration_count = 0; self.calibration_sum = [0.0; 3];
            let reset = self.cancel(now).reset_input;
            if !invalid {
                if let Some(m) = motion { self.sample(now, m, flipped); }
            }
            return self.frame(0, reset);
        }
        if self.flipped.is_some_and(|old| old != flipped) {
            self.flipped = Some(flipped);
            return self.cancel(now);
        }
        self.flipped = Some(flipped);
        if let Some(m) = motion { self.sample(now, m, flipped); }
        if physical_touch { return self.cancel(now); }
        let fresh = self.last_sample.is_some_and(|t| now.wrapping_sub(t) <= SAMPLE_TIMEOUT_MS);
        if !fresh {
            // Reconnection must collect a new calibration window, rather than finish
            // a dwell started with old data. Preserve the timestamp to reject replay.
            self.calibrated = false; self.calibration_since = None; self.calibration_start = None;
            self.calibration_count = 0; self.calibration_sum = [0.0; 3];
            return self.cancel(now);
        }
        if let Some(start) = self.page_pause {
            if now.wrapping_sub(start) < self.page_pause_s * 1000 { return self.frame(0, false); }
            self.page_pause = None; self.calibrated = true;
            self.calibration_since = None; self.calibration_start = None;
            self.calibration_count = 0; self.calibration_sum = [0.0; 3];
            self.last_action = None; self.rate = [0.0; 2];
        }
        if !self.calibrated { return self.frame(0, false); }
        if let Some(g) = self.gesture {
            self.touch = g.touch(now);
            if !self.touch.pressed {
                self.gesture = None; self.last_action = Some(now); self.dwell = None;
            }
            return self.frame(if self.touch.pressed && g.zone == Zone::Center { 100 } else { 0 }, false);
        }
        self.touch = Touch { point: self.point(), pressed: false };
        let p = self.point(); let at = zone(p);
        let entered_edge = at != self.edge_zone;
        self.edge_zone = at;
        if at != Zone::Center {
            self.dwell = None;
            // First contact is immediate, including while turning against the edge.
            // Repeating on the same edge waits only for release + cooldown, never dwell.
            if !entered_edge && self.last_action.is_some_and(|t| now.wrapping_sub(t) < COOLDOWN_MS) {
                return self.frame(0, false);
            }
            let gesture = Gesture::new(now, p, at);
            self.touch = gesture.touch(now); self.gesture = Some(gesture);
            return self.frame(0, false);
        }
        if self.last_action.is_some_and(|t| now.wrapping_sub(t) < COOLDOWN_MS) {
            self.dwell = None; return self.frame(0, false);
        }
        if let Some(previous) = self.latch {
            let rearm = REARM_DISTANCE.max(self.tolerance_px as f32);
            if distance_squared(previous, p) >= (rearm * rearm) as i32 {
                self.latch = None;
            } else if at == Zone::Center { self.dwell = None; return self.frame(0, false); }
        }
        let tolerance = self.tolerance_px as f32;
        let restart = self.dwell.is_none_or(|d|
            (self.x-d.x)*(self.x-d.x) + (self.y-d.y)*(self.y-d.y) > tolerance*tolerance);
        if restart { self.dwell = Some(Dwell { start: now, x: self.x, y: self.y }); }
        let elapsed = now.wrapping_sub(self.dwell.unwrap().start);
        if elapsed < self.confirm_ms { return self.frame(elapsed * 100 / self.confirm_ms, false); }
        let gesture = Gesture::new(now, p, at);
        self.touch = gesture.touch(now); self.gesture = Some(gesture); self.dwell = None;
        if at == Zone::Center { self.latch = Some(p); }
        self.frame(100, false)
    }
}

/// LVGL rotates raw input together with the display; the cursor uses visible coordinates.
pub fn raw_point(point: Point, flipped: bool) -> Point {
    if flipped { Point { x: WIDTH - 1 - point.x, y: HEIGHT - 1 - point.y } } else { point }
}
