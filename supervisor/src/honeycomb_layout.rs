//! Integer-only hexagonal launcher geometry and pointer state. No firmware I/O.

pub(crate) const MAX_ICONS: usize = 128;
pub(crate) const WIDTH: i32 = 336;
pub(crate) const HEIGHT: i32 = 480;
const ONE: i32 = 256;
const DRAG_THRESHOLD: i32 = 10;
pub(crate) const ZOOM_MIN: i32 = 128;
pub(crate) const ZOOM_MAX: i32 = 384;
pub(crate) const ZOOM_EDGE: i32 = 28;
const EXIT_THRESHOLD: i32 = 24;

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

#[derive(Clone, Copy, Debug)]
pub(crate) struct Frame {
    pub x: i32,
    pub y: i32,
    pub diameter: i32,
    pub visible: bool,
}

pub(crate) fn frame(index: usize, pan: Point) -> Frame {
    frame_zoom(index, pan, ONE)
}

pub(crate) fn frame_zoom(index: usize, pan: Point, zoom: i32) -> Frame {
    let p = position(index);
    let zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
    let dx = p.x * zoom / ONE + pan.x;
    let dy = p.y * zoom / ONE + pan.y;
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
    zooming: bool,
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
            zooming: false,
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
    pub(crate) fn zoom(&self) -> i32 {
        self.zoom
    }
    pub(crate) fn frame(&self, index: usize) -> Frame {
        frame_zoom(index, self.pan(), self.zoom)
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
        self.zooming =
            point.x >= WIDTH - ZOOM_EDGE && point.x < WIDTH && point.y >= 0 && point.y < HEIGHT;
        self.stop_tap |= self.zooming;
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
            + if self.zooming {
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
        if self.zooming {
            let next = (self.zoom - self.pending.y * 2).clamp(ZOOM_MIN, ZOOM_MAX);
            self.pending = Point::default();
            if next == self.zoom {
                return false;
            }
            // Scale about the viewport centre, retaining the app under it.
            self.pan.x = self.pan.x * next / self.zoom;
            self.pan.y = self.pan.y * next / self.zoom;
            self.zoom = next;
            self.bounds_count = usize::MAX;
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
        // firmware's page navigation. Never hand off inertia or a zoom drag.
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
        if !self.dragged || self.zooming || self.route != Route::Cloud {
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
        let x = self
            .pan
            .x
            .clamp(-self.max.x * self.zoom, -self.min.x * self.zoom);
        let y = self
            .pan
            .y
            .clamp(-self.max.y * self.zoom, -self.min.y * self.zoom);
        if x != self.pan.x {
            self.velocity.x = 0;
        }
        if y != self.pan.y {
            self.velocity.y = 0;
        }
        self.pan = Point { x, y };
    }
}
