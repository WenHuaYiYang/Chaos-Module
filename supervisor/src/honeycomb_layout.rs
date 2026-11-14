//! Integer-only hexagonal launcher geometry and pointer state. No firmware I/O.

pub(crate) const MAX_ICONS: usize = 128;
pub(crate) const WIDTH: i32 = 336;
pub(crate) const HEIGHT: i32 = 480;
const ONE: i32 = 256;
const DRAG_THRESHOLD: i32 = 10;
pub(crate) const ZOOM_MIN: i32 = 128;
pub(crate) const ZOOM_MAX: i32 = 384;
pub(crate) const ZOOM_EDGE: i32 = 28;
pub(crate) const SPACING_MIN: i32 = 224;
pub(crate) const SPACING_MAX: i32 = 512;
const EXIT_THRESHOLD: i32 = 24;

/// 控制条渲染还会上移16px；这里为最后一个原生内容保留最终16px间距。
pub(crate) fn selector_anchor(stock: i32, content_bottom: Option<i32>) -> i32 {
    content_bottom.map_or(stock, |bottom| stock.min(bottom.saturating_add(32)))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct Point {
    pub x: i32,
    pub y: i32,
}

/// Stable app order, filling complete hexagonal rings around the first app.
pub(crate) fn position(index: usize) -> Point {
    let mut left = index.min(MAX_ICONS - 1);
    if left == 0 {
        return Point::default();
    }
    left -= 1;
    let mut ring = 1;
    while left >= 6 * ring {
        left -= 6 * ring;
        ring += 1;
    }
    let mut q = -(ring as i32);
    let mut r = ring as i32;
    for (dq, dr) in [(0, -1), (1, -1), (1, 0), (0, 1), (-1, 1), (-1, 0)] {
        let steps = left.min(ring);
        q += dq * steps as i32;
        r += dr * steps as i32;
        left -= steps;
        if left == 0 {
            break;
        }
    }
    Point {
        x: q * 96 + r * 48,
        y: r * 83,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Frame {
    pub x: i32,
    pub y: i32,
    pub diameter: i32,
    pub visible: bool,
}

pub(crate) const RETURN_DURATION_MS: u32 = 240;
const PROGRESS: i32 = 1024;

/// Geometry only: current native bounds, never object handles or image buffers.
pub(crate) struct ReturnAnimation {
    from: [Frame; MAX_ICONS],
    count: usize,
    started: u32,
    amount: i32,
}

impl ReturnAnimation {
    pub(crate) const fn new() -> Self {
        Self {
            from: [Frame {
                x: 0,
                y: 0,
                diameter: 1,
                visible: false,
            }; MAX_ICONS],
            count: 0,
            started: 0,
            amount: 0,
        }
    }
    pub(crate) fn set_start(&mut self, index: usize, frame: Frame) {
        if index < MAX_ICONS {
            self.from[index] = frame;
        }
    }
    pub(crate) fn begin(&mut self, now: u32, count: usize) {
        self.count = count.min(MAX_ICONS);
        self.started = now;
        self.amount = 0;
    }
    pub(crate) fn active(&self) -> bool {
        self.count != 0 && self.amount < PROGRESS
    }
    pub(crate) fn cancel(&mut self) {
        self.count = 0;
    }
    pub(crate) fn tick(&mut self, now: u32) -> bool {
        if !self.active() {
            return false;
        }
        let elapsed = now.wrapping_sub(self.started).min(RETURN_DURATION_MS);
        let remaining = PROGRESS - (elapsed * PROGRESS as u32 / RETURN_DURATION_MS) as i32;
        // Cubic ease-out, evaluated once per timer turn, not once per icon.
        let amount = PROGRESS - remaining * remaining * remaining / (PROGRESS * PROGRESS);
        let amount = if elapsed < RETURN_DURATION_MS {
            amount.min(PROGRESS - 1)
        } else {
            PROGRESS
        };
        let changed = amount != self.amount;
        self.amount = amount;
        changed
    }
    pub(crate) fn frame(&self, index: usize, target: Frame) -> Frame {
        if !self.active() || index >= self.count {
            return target;
        }
        let mut from = self.from[index];
        if !from.visible {
            // Newly exposed icons grow in place; never reveal a stale hidden
            // native position for one frame while crossing into the viewport.
            from = Frame {
                diameter: 1,
                visible: false,
                ..target
            };
        }
        if self.amount == 0 {
            return from;
        }
        let mix = |a, b| a + (b - a) * self.amount / PROGRESS;
        let x = mix(from.x, target.x);
        let y = mix(from.y, target.y);
        let diameter = mix(from.diameter, target.diameter).max(1);
        Frame {
            x,
            y,
            diameter,
            visible: (from.visible || target.visible)
                && x > -diameter / 2
                && x < WIDTH + diameter / 2
                && y > -diameter / 2
                && y < HEIGHT + diameter / 2,
        }
    }
}

pub(crate) fn frame(index: usize, pan: Point) -> Frame {
    frame_zoom(index, pan, ONE)
}

pub(crate) fn frame_zoom(index: usize, pan: Point, zoom: i32) -> Frame {
    frame_spaced(index, pan, zoom, ONE)
}

pub(crate) fn frame_spaced(index: usize, pan: Point, zoom: i32, spacing: i32) -> Frame {
    let p = position(index);
    let zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
    let pitch = zoom * spacing.clamp(SPACING_MIN, SPACING_MAX) / ONE;
    let dx = p.x * pitch / ONE + pan.x;
    let dy = p.y * pitch / ONE + pan.y;
    let nx = dx as i64 * 256 / (WIDTH / 2) as i64;
    let ny = dy as i64 * 256 / (HEIGHT / 2) as i64;
    let radial = (nx * nx + ny * ny).min(65536);
    Frame {
        x: WIDTH / 2 + dx,
        y: HEIGHT / 2 + dy,
        diameter: ((84 - (56 * radial / 65536) as i32) * zoom / ONE).max(1),
        visible: dx.abs() < WIDTH / 2 + 56 && dy.abs() < HEIGHT / 2 + 56,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Route {
    #[default]
    Cloud,
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gesture {
    Pan,
    Zoom,
    LeftEdge,
    Spacing,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Camera {
    pan: Point,
    velocity: Point,
    pending: Point,
    pressed: bool,
    dragged: bool,
    stop_tap: bool,
    travel: i32,
    zoom: i32,
    spacing: i32,
    gesture: Gesture,
    overflow: Point,
    route: Route,
    bounds_count: usize,
    min: Point,
    max: Point,
}

impl Default for Camera {
    fn default() -> Self {
        Self::new()
    }
}

impl Camera {
    pub(crate) const fn new() -> Self {
        Self {
            pan: Point { x: 0, y: 0 },
            velocity: Point { x: 0, y: 0 },
            pending: Point { x: 0, y: 0 },
            pressed: false,
            dragged: false,
            stop_tap: false,
            travel: 0,
            zoom: ONE,
            spacing: ONE,
            gesture: Gesture::Pan,
            overflow: Point { x: 0, y: 0 },
            route: Route::Cloud,
            bounds_count: usize::MAX,
            min: Point { x: 0, y: 0 },
            max: Point { x: 0, y: 0 },
        }
    }
    pub(crate) fn pan(&self) -> Point {
        Point {
            x: self.pan.x / ONE,
            y: self.pan.y / ONE,
        }
    }
    /// 原生纵向列表复用同一惯性/点击保护，不启用蜂窝边缘缩放。
    pub(crate) fn linear_bounds(&mut self, count: usize, range: i32) {
        self.zoom = ONE; self.spacing = ONE;
        self.min = Point::default(); self.max = Point { x: 0, y: range.max(0) };
        self.bounds_count = count.min(MAX_ICONS); self.clamp(count);
    }
    pub(crate) fn linear_end(&mut self, count: usize, range: i32) {
        self.pan = Point { x: 0, y: -range.max(0) * ONE };
        self.linear_bounds(count, range);
    }
    pub(crate) fn press_linear(&mut self, point: Point) {
        self.press_at(point); self.gesture = Gesture::Pan;
    }
    pub(crate) fn zoom(&self) -> i32 {
        self.zoom
    }
    pub(crate) fn spacing(&self) -> i32 {
        self.spacing
    }
    pub(crate) fn frame(&self, index: usize) -> Frame {
        frame_spaced(index, self.pan(), self.zoom, self.spacing)
    }
    fn pitch(&self) -> i32 {
        self.zoom * self.spacing / ONE
    }
    pub(crate) fn route(&self) -> Route {
        self.route
    }
    pub(crate) fn moving(&self) -> bool {
        self.pressed || self.velocity.x != 0 || self.velocity.y != 0
    }
    pub(crate) fn press(&mut self) {
        self.press_at(Point {
            x: WIDTH / 2,
            y: HEIGHT / 2,
        });
    }
    pub(crate) fn press_at(&mut self, point: Point) {
        // A tap which catches a moving cloud stops it without launching an app.
        self.stop_tap = self.velocity.x != 0 || self.velocity.y != 0;
        self.velocity = Point::default();
        self.pending = Point::default();
        self.pressed = true;
        self.dragged = false;
        self.travel = 0;
        self.gesture = if point.y < 0 || point.y >= HEIGHT {
            Gesture::Pan
        } else if point.x >= WIDTH - ZOOM_EDGE && point.x < WIDTH {
            Gesture::Zoom
        } else if point.x >= 0 && point.x < ZOOM_EDGE {
            Gesture::LeftEdge
        } else {
            Gesture::Pan
        };
        self.stop_tap |= self.gesture == Gesture::Zoom;
        self.overflow = Point::default();
        self.route = Route::Cloud;
    }
    pub(crate) fn drag(&mut self, delta: Point, count: usize) -> bool {
        if !self.pressed || self.route != Route::Cloud {
            return false;
        }
        let d = Point {
            x: delta.x.clamp(-128, 128),
            y: delta.y.clamp(-128, 128),
        };
        self.travel = (self.travel
            + if self.gesture == Gesture::Zoom {
                d.y.abs()
            } else {
                d.x.abs() + d.y.abs()
            })
        .min(1024);
        self.pending.x += d.x;
        self.pending.y += d.y;
        if !self.dragged && self.travel < DRAG_THRESHOLD {
            return false;
        }
        self.dragged = true;
        if self.gesture == Gesture::LeftEdge {
            // Lock the first deliberate direction. Horizontal swipes still
            // pan and can reach the firmware's page-navigation boundary.
            self.gesture = if self.pending.y.abs() > self.pending.x.abs() {
                Gesture::Spacing
            } else {
                Gesture::Pan
            };
        }
        if matches!(self.gesture, Gesture::Zoom | Gesture::Spacing) {
            let old_pitch = self.pitch();
            let value = if self.gesture == Gesture::Zoom {
                &mut self.zoom
            } else {
                &mut self.spacing
            };
            let (min, max, direction) = if self.gesture == Gesture::Zoom {
                (ZOOM_MIN, ZOOM_MAX, -1)
            } else {
                (SPACING_MIN, SPACING_MAX, 1)
            };
            let next = (*value + self.pending.y * 2 * direction).clamp(min, max);
            self.pending = Point::default();
            if next == *value {
                return false;
            }
            *value = next;
            // Scale about the viewport centre, retaining the app under it.
            let pitch = self.pitch();
            self.pan.x = self.pan.x * pitch / old_pitch;
            self.pan.y = self.pan.y * pitch / old_pitch;
            self.clamp(count);
            return true;
        }
        let old = self.pan;
        self.pan.x += self.pending.x * ONE;
        self.pan.y += self.pending.y * ONE;
        self.pending = Point::default();
        self.velocity = Point {
            x: d.x * ONE,
            y: d.y * ONE,
        };
        let requested = self.pan;
        self.clamp(count);
        // At the left/top content boundary, outward motion belongs to the
        // firmware's page navigation. Never hand off inertia or edge adjustment.
        let dx = (requested.x - self.pan.x) / ONE;
        let dy = (requested.y - self.pan.y) / ONE;
        self.overflow.x = if dx > 0 && d.x > d.y.abs() {
            self.overflow.x + dx
        } else {
            0
        };
        self.overflow.y = if dy > 0 && d.y > d.x.abs() {
            self.overflow.y + dy
        } else {
            0
        };
        if self.overflow.x >= EXIT_THRESHOLD {
            self.route = Route::Horizontal;
        } else if self.overflow.y >= EXIT_THRESHOLD {
            self.route = Route::Vertical;
        }
        if self.route != Route::Cloud {
            self.velocity = Point::default();
        }
        self.pan != old
    }
    pub(crate) fn release(&mut self) {
        self.pressed = false;
        // Keep the drag latch until the NEXT press, including SHORT_CLICKED.
        if !self.dragged || self.gesture != Gesture::Pan || self.route != Route::Cloud {
            self.velocity = Point::default();
        }
    }
    pub(crate) fn cancel(&mut self) {
        self.release();
        self.velocity = Point::default();
        self.stop_tap = true;
    }
    pub(crate) fn can_click(&self) -> bool {
        !self.dragged && !self.stop_tap
    }
    pub(crate) fn tick(&mut self, count: usize) -> bool {
        if self.pressed || (self.velocity.x == 0 && self.velocity.y == 0) {
            return false;
        }
        self.pan.x += self.velocity.x;
        self.pan.y += self.velocity.y;
        self.clamp(count);
        self.velocity.x = self.velocity.x * 3 / 4;
        self.velocity.y = self.velocity.y * 3 / 4;
        if self.velocity.x.abs() < 64 {
            self.velocity.x = 0;
        }
        if self.velocity.y.abs() < 64 {
            self.velocity.y = 0;
        }
        true
    }
    fn clamp(&mut self, count: usize) {
        let count = count.min(MAX_ICONS);
        if self.bounds_count != count {
            self.min = Point::default();
            self.max = Point::default();
            for i in 0..count {
                let p = position(i);
                self.min.x = self.min.x.min(p.x);
                self.min.y = self.min.y.min(p.y);
                self.max.x = self.max.x.max(p.x);
                self.max.y = self.max.y.max(p.y);
            }
            self.bounds_count = count;
        }
        let pitch = self.pitch();
        let x = self.pan.x.clamp(-self.max.x * pitch, -self.min.x * pitch);
        let y = self.pan.y.clamp(-self.max.y * pitch, -self.min.y * pitch);
        if x != self.pan.x {
            self.velocity.x = 0;
        }
        if y != self.pan.y {
            self.velocity.y = 0;
        }
        self.pan = Point { x, y };
    }
}
