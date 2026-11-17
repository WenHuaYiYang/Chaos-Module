//! Firmware-independent configuration, nearest-edge geometry and hold-to-drag input.

pub const WIDTH: i32 = 336;
pub const HEIGHT: i32 = 480;
pub const MARGIN: i32 = 12;
pub const MIN_SIZE: u8 = 32;
pub const MAX_SIZE: u8 = 72;
pub const HOLD_MS: u32 = 450;
pub const CONFIG_LEN: usize = 16;
pub const SIZES: [u8; 6] = [32, 40, 48, 56, 64, 72];
pub const KEEP_MINUTES: [u32; 3] = [5, 10, 15];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BandStyle {
    pub corner: Corner,
    pub size: u8,
    pub translucent: bool,
    pub brightness: Option<u16>,
    pub highlight: Option<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Action {
    Time,
    Brightness,
    Screenshot,
    KeepAwake,
    Watchface,
    Vibration,
    AutoBrightness,
    Recording,
    Rotate,
}
impl Action {
    pub const ALL: [Self; 9] = [
        Self::Time,
        Self::Brightness,
        Self::Screenshot,
        Self::KeepAwake,
        Self::Watchface,
        Self::Vibration,
        Self::AutoBrightness,
        Self::Recording,
        Self::Rotate,
    ];
    pub fn name(self) -> &'static [u8] {
        match self {
            Self::Time => "时间\0".as_bytes(),
            Self::Brightness => "亮度\0".as_bytes(),
            Self::Screenshot => "截屏\0".as_bytes(),
            Self::KeepAwake => "常亮\0".as_bytes(),
            Self::Watchface => "回到表盘\0".as_bytes(),
            Self::Vibration => "震动开关\0".as_bytes(),
            Self::AutoBrightness => "自动亮度\0".as_bytes(),
            Self::Recording => "录屏开关\0".as_bytes(),
            Self::Rotate => "屏幕翻转\0".as_bytes(),
        }
    }
    pub fn short_name(self) -> &'static [u8] {
        match self {
            Self::Watchface => "表盘\0".as_bytes(),
            Self::Vibration => "震动\0".as_bytes(),
            Self::AutoBrightness => "自动\0".as_bytes(),
            Self::Recording => "录屏\0".as_bytes(),
            Self::Rotate => "翻转\0".as_bytes(),
            _ => self.name(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}
impl Corner {
    pub const ALL: [Self; 4] = [
        Self::TopLeft,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomRight,
    ];
    pub fn name(self) -> &'static [u8] {
        match self {
            Self::TopLeft => "左上角\0".as_bytes(),
            Self::TopRight => "右上角\0".as_bytes(),
            Self::BottomLeft => "左下角\0".as_bytes(),
            Self::BottomRight => "右下角\0".as_bytes(),
        }
    }
    pub fn right(self) -> bool {
        matches!(self, Self::TopRight | Self::BottomRight)
    }
    pub fn bottom(self) -> bool {
        matches!(self, Self::BottomLeft | Self::BottomRight)
    }
    pub fn anchor(self, size: u8) -> Point {
        Point {
            x: if self.right() {
                WIDTH - MARGIN - size as i32
            } else {
                MARGIN
            },
            y: if self.bottom() {
                HEIGHT - MARGIN - size as i32
            } else {
                MARGIN
            },
        }
    }
    pub fn nearest(center: Point) -> Self {
        match (center.x >= WIDTH / 2, center.y >= HEIGHT / 2) {
            (false, false) => Self::TopLeft,
            (true, false) => Self::TopRight,
            (false, true) => Self::BottomLeft,
            (true, true) => Self::BottomRight,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}
impl Edge {
    pub const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Top, Self::Bottom];
    pub fn name(self) -> &'static [u8] {
        match self {
            Self::Left => "左侧\0".as_bytes(),
            Self::Right => "右侧\0".as_bytes(),
            Self::Top => "顶部\0".as_bytes(),
            Self::Bottom => "底部\0".as_bytes(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dock {
    pub edge: Edge,
    pub offset: u16,
}
impl Dock {
    pub const DEFAULT: Self = Self {
        edge: Edge::Right,
        offset: 0,
    };
    pub fn from_corner(corner: Corner) -> Self {
        Self {
            edge: if corner.right() {
                Edge::Right
            } else {
                Edge::Left
            },
            offset: if corner.bottom() { 1000 } else { 0 },
        }
    }
    pub fn anchor(self, size: u8) -> Point {
        let w = WIDTH - 2 * MARGIN - size as i32;
        let h = HEIGHT - 2 * MARGIN - size as i32;
        let along = self.offset.min(1000) as i32;
        match self.edge {
            Edge::Left => Point {
                x: MARGIN,
                y: MARGIN + (h * along + 500) / 1000,
            },
            Edge::Right => Point {
                x: MARGIN + w,
                y: MARGIN + (h * along + 500) / 1000,
            },
            Edge::Top => Point {
                x: MARGIN + (w * along + 500) / 1000,
                y: MARGIN,
            },
            Edge::Bottom => Point {
                x: MARGIN + (w * along + 500) / 1000,
                y: MARGIN + h,
            },
        }
    }
    pub fn nearest(position: Point, size: u8) -> Self {
        let w = WIDTH - 2 * MARGIN - size as i32;
        let h = HEIGHT - 2 * MARGIN - size as i32;
        let x = (position.x - MARGIN).clamp(0, w);
        let y = (position.y - MARGIN).clamp(0, h);
        let distances = [x, w - x, y, h - y];
        let mut index = 0;
        for i in 1..4 {
            if distances[i] < distances[index] {
                index = i;
            }
        }
        let edge = Edge::ALL[index];
        let offset = match edge {
            Edge::Left | Edge::Right => (y * 1000 + h / 2) / h,
            Edge::Top | Edge::Bottom => (x * 1000 + w / 2) / w,
        } as u16;
        Self { edge, offset }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    pub size: u8,
    pub translucent: bool,
    pub dock: Dock,
    pub slots: [Action; 3],
}
impl Config {
    // The new global layer is opt-in; enabling it starts in the upper right.
    pub const DEFAULT: Self = Self {
        enabled: false,
        size: 48,
        translucent: true,
        dock: Dock::DEFAULT,
        slots: [Action::Time, Action::KeepAwake, Action::Brightness],
    };
    pub fn valid(self) -> bool {
        (MIN_SIZE..=MAX_SIZE).contains(&self.size)
            && self.dock.offset <= 1000
            && self.slots[0] != self.slots[1]
            && self.slots[0] != self.slots[2]
            && self.slots[1] != self.slots[2]
    }
    pub fn assign(&mut self, slot: usize, action: Action) -> bool {
        if slot >= self.slots.len() {
            return false;
        }
        let previous = self.slots[slot];
        if let Some(other) = self.slots.iter().position(|&a| a == action) {
            self.slots[other] = previous;
        }
        self.slots[slot] = action;
        true
    }
    pub fn anchor(self) -> Point {
        self.dock.anchor(self.size)
    }
    pub fn corner(self) -> Corner {
        let p = self.anchor();
        Corner::nearest(Point {
            x: p.x + self.size as i32 / 2,
            y: p.y + self.size as i32 / 2,
        })
    }
    pub fn inward_point(self, u: i32, v: i32) -> Point {
        let corner = self.corner();
        let fixed = corner.anchor(self.size);
        let dock = self.anchor();
        let p = inward_point(corner, self.size, u, v);
        Point {
            x: p.x + dock.x - fixed.x,
            y: p.y + dock.y - fixed.y,
        }
    }
    fn fixed_point(self, point: Point) -> Point {
        let fixed = self.corner().anchor(self.size);
        let dock = self.anchor();
        Point {
            x: point.x - dock.x + fixed.x,
            y: point.y - dock.y + fixed.y,
        }
    }
    pub fn band_coordinates(self, point: Point) -> Point {
        band_coordinates(self.corner(), self.size, self.fixed_point(point))
    }
    pub fn band_sector(self, point: Point) -> Option<usize> {
        band_sector(self.corner(), self.size, self.fixed_point(point))
    }
    pub fn band_progress(self, point: Point) -> u32 {
        band_progress(self.corner(), self.size, self.fixed_point(point))
    }
    pub fn fan_centers(self) -> [Point; 3] {
        let mut points = fan_centers(self.corner(), self.size, 1000);
        let fixed = self.corner().anchor(self.size);
        let dock = self.anchor();
        for p in &mut points {
            p.x += dock.x - fixed.x;
            p.y += dock.y - fixed.y;
        }
        points
    }
    pub fn encode(self) -> [u8; CONFIG_LEN] {
        let mut bytes = [
            b'D',
            b'O',
            b'T',
            b'2',
            self.enabled as u8,
            self.size,
            self.translucent as u8,
            self.dock.edge as u8,
            self.slots[0] as u8,
            self.slots[1] as u8,
            self.slots[2] as u8,
            self.dock.offset as u8,
            (self.dock.offset >> 8) as u8,
            0,
            0,
            0,
        ];
        bytes[15] = bytes[..15].iter().fold(0xa5, |sum, b| sum ^ b);
        bytes
    }
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let legacy = bytes.len() == 12 && &bytes[..4] == b"DOT1";
        if !(legacy || bytes.len() == CONFIG_LEN && &bytes[..4] == b"DOT2")
            || bytes[4] > 1
            || bytes[6] > 1
            || bytes[..bytes.len() - 1].iter().fold(0xa5, |sum, b| sum ^ b)
                != bytes[bytes.len() - 1]
        {
            return None;
        }
        let dock = if legacy {
            Dock::from_corner(*Corner::ALL.get(bytes[7] as usize)?)
        } else {
            if bytes[13] != 0 || bytes[14] != 0 {
                return None;
            }
            Dock {
                edge: *Edge::ALL.get(bytes[7] as usize)?,
                offset: u16::from_le_bytes([bytes[11], bytes[12]]),
            }
        };
        let config = Self {
            enabled: bytes[4] != 0,
            size: bytes[5],
            translucent: bytes[6] != 0,
            dock,
            slots: [
                *Action::ALL.get(bytes[8] as usize)?,
                *Action::ALL.get(bytes[9] as usize)?,
                *Action::ALL.get(bytes[10] as usize)?,
            ],
        };
        config.valid().then_some(config)
    }
}

pub fn clamp_position(point: Point, size: u8) -> Point {
    Point {
        x: point.x.clamp(0, WIDTH - size as i32),
        y: point.y.clamp(0, HEIGHT - size as i32),
    }
}

pub fn band_radii(size: u8) -> (i32, i32) {
    let middle = size as i32 / 2 + 74;
    (middle - 32, middle + 32)
}
pub fn inward_point(corner: Corner, size: u8, u: i32, v: i32) -> Point {
    let origin = corner.anchor(size);
    Point {
        x: origin.x + size as i32 / 2 + if corner.right() { -u } else { u },
        y: origin.y + size as i32 / 2 + if corner.bottom() { -v } else { v },
    }
}
pub fn band_coordinates(corner: Corner, size: u8, point: Point) -> Point {
    let c = inward_point(corner, size, 0, 0);
    Point {
        x: if corner.right() {
            c.x - point.x
        } else {
            point.x - c.x
        },
        y: if corner.bottom() {
            c.y - point.y
        } else {
            point.y - c.y
        },
    }
}
pub fn band_sector(corner: Corner, size: u8, point: Point) -> Option<usize> {
    let p = band_coordinates(corner, size, point);
    if p.x < 0 || p.y < 0 || p.x > 256 || p.y > 256 {
        return None;
    }
    let (inner, outer) = band_radii(size);
    let d = p.x * p.x + p.y * p.y;
    if d < inner * inner || d > outer * outer {
        return None;
    }
    if p.x < 10 || p.y < 10 {
        let (mut low, mut high) = (0, 256);
        while low < high {
            let middle = (low + high + 1) / 2;
            if middle * middle <= d {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        let radial = if low < inner + 10 {
            inner + 10 - low
        } else if low > outer - 10 {
            low - (outer - 10)
        } else {
            0
        };
        let u = (10 - p.x).max(0);
        let v = (10 - p.y).max(0);
        if radial * radial + u * u + v * v > 100 {
            return None;
        }
    }
    Some(if p.y * 1000 <= p.x * 577 {
        0
    } else if p.x * 1000 <= p.y * 577 {
        2
    } else {
        1
    })
}
/// A monotonic quarter-arc coordinate, shared by input and the progress raster.
pub fn band_progress(corner: Corner, size: u8, point: Point) -> u32 {
    let p = band_coordinates(corner, size, point);
    let u = p.x.clamp(0, 4096) as u32;
    let v = p.y.clamp(0, 4096) as u32;
    if u + v == 0 {
        0
    } else {
        v * 1000 / (u + v)
    }
}
pub fn slider_brightness(progress: u32) -> u32 {
    1 + (progress.min(1000) * 1999 + 500) / 1000
}
pub fn brightness_progress(nit: u32) -> u16 {
    ((nit.clamp(1, 2000) - 1) * 1000 / 1999) as u16
}

/// Three centers at 15/45/75 degrees, mirrored into the screen at every corner.
pub fn fan_centers(corner: Corner, size: u8, progress: u32) -> [Point; 3] {
    let anchor = corner.anchor(size);
    let center = Point {
        x: anchor.x + size as i32 / 2,
        y: anchor.y + size as i32 / 2,
    };
    // Continuous sectors retain a >=44px tap span even at the smallest size.
    let radius = size as i32 / 2 + 74;
    let sign_x = if corner.right() { -1 } else { 1 };
    let sign_y = if corner.bottom() { -1 } else { 1 };
    let mut output = [center; 3];
    for (point, (x, y)) in output.iter_mut().zip([(966, 259), (707, 707), (259, 966)]) {
        point.x += sign_x * radius * x / 1000 * progress.min(1000) as i32 / 1000;
        point.y += sign_y * radius * y / 1000 * progress.min(1000) as i32 / 1000;
    }
    output
}

#[derive(Clone, Copy)]
pub struct Gesture {
    pressed: bool,
    held: bool,
    moved: bool,
    cancelled: bool,
    suppress: bool,
    started: u32,
    touch: Point,
    origin: Point,
}
impl Gesture {
    pub const NEW: Self = Self {
        pressed: false,
        held: false,
        moved: false,
        cancelled: false,
        suppress: false,
        started: 0,
        touch: Point { x: 0, y: 0 },
        origin: Point { x: 0, y: 0 },
    };
    pub fn press(&mut self, point: Point, origin: Point, now: u32) {
        *self = Self {
            pressed: true,
            touch: point,
            origin,
            started: now,
            ..Self::NEW
        };
    }
    pub fn hold(&mut self) {
        if self.pressed && !self.cancelled {
            self.held = true;
            self.suppress = true;
        }
    }
    pub fn motion(&mut self, point: Point, now: u32, size: u8) -> Option<Point> {
        if !self.pressed || self.cancelled {
            return None;
        }
        let dx = point.x.saturating_sub(self.touch.x);
        let dy = point.y.saturating_sub(self.touch.y);
        if now.wrapping_sub(self.started) >= HOLD_MS {
            self.hold();
        }
        if !self.held {
            if dx.unsigned_abs() > 6 || dy.unsigned_abs() > 6 {
                self.cancel();
            }
            return None;
        }
        self.moved |= dx != 0 || dy != 0;
        Some(clamp_position(
            Point {
                x: self.origin.x.saturating_add(dx),
                y: self.origin.y.saturating_add(dy),
            },
            size,
        ))
    }
    pub fn release(&mut self, point: Point, now: u32, size: u8) -> Option<Dock> {
        let position = self.motion(point, now, size);
        self.pressed = false;
        if self.held && self.moved && !self.cancelled {
            let p = position?;
            Some(Dock::nearest(p, size))
        } else {
            None
        }
    }
    pub fn cancel(&mut self) {
        self.pressed = false;
        self.cancelled = true;
        self.suppress = true;
    }
    pub fn can_click(&self) -> bool {
        !self.suppress
    }
    pub fn dragging(&self) -> bool {
        self.pressed && self.held
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Closed,
    Fan,
    Duration,
    Brightness,
    Message,
}
impl View {
    pub fn band(self) -> bool {
        matches!(self, Self::Fan | Self::Duration | Self::Brightness)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Tween {
    pub from: i32,
    pub to: i32,
    pub started: u32,
    pub duration: u32,
}
impl Tween {
    pub const ZERO: Self = Self {
        from: 0,
        to: 0,
        started: 0,
        duration: 0,
    };
    pub fn active(self, now: u32) -> bool {
        self.from != self.to && now.wrapping_sub(self.started) < self.duration
    }
    pub fn value(self, now: u32) -> i32 {
        if !self.active(now) {
            return self.to;
        }
        let p = now.wrapping_sub(self.started) * 1000 / self.duration;
        let remaining = 1000 - p;
        let ease = 1000 - remaining * remaining * remaining / 1_000_000;
        self.from + (self.to - self.from) * ease as i32 / 1000
    }
}

pub fn adjusted_brightness(current: u32, delta: i32) -> u32 {
    (current.min(2000) as i32 + delta.clamp(-2000, 2000)).clamp(1, 2000) as u32
}
