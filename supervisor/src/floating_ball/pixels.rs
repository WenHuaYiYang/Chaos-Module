//! Flat premultiplied ball materials and antialiased rounded arc coverage.
pub const SIDE: usize = 64;
pub const BYTES: usize = SIDE * SIDE * 4;
pub const BAND_SIDE: usize = 128;
pub const BAND_BYTES: usize = BAND_SIDE * BAND_SIDE * 4;
const N: usize = BAND_SIDE * BAND_SIDE;
static SPHERE: &[u8; BYTES] = include_bytes!("../../assets/floating-ball/sphere.bgra");
static BATTERY_STROKE: &[u8; SIDE * SIDE] =
    include_bytes!("../../assets/floating-ball/battery-stroke.a8");
static MASKS: &[u8] = include_bytes!("../../assets/floating-ball/arc-masks.bin");

const BACKGROUND: [u32; 3] = [20, 23, 28];
// RGB material colours; antialiasing affects coverage only, never lighting.
const ACTIVE: [u32; 3] = [228, 231, 237];
const REMAINING: [u32; 3] = [82, 88, 98];

pub fn render(output: &mut [u8; BYTES], translucent: bool) {
    render_battery(output, translucent, Some(100));
}
/// Flat battery progress on one permanently closed broad circular track. Two solid material
/// colours distinguish the charged and remaining portions. The disk and glyph
/// have no highlights, bevel, glow or shadow. The hollow end marker moves
/// with the shrinking progress endpoint. Only shape edges are antialiased.
pub fn render_battery(output: &mut [u8; BYTES], translucent: bool, percent: Option<u8>) {
    let value = percent.unwrap_or(0).min(100) as i32;
    let sweep = value * 360 * 256 / 100;
    let quarter = sweep / (90 * 256);
    let angle = sweep % (90 * 256);
    let sin = super::curved::sine(angle);
    let cos = super::curved::sine(90 * 256 - angle);
    let along = 24 * 8;
    let cap_radius = 25 * 2;
    let opacity = if translucent { 212 } else { 255 };
    let (ex, ey) = match quarter % 4 {
        0 => (-along * sin / 16384, along * cos / 16384),
        1 => (-along * cos / 16384, -along * sin / 16384),
        2 => (along * sin / 16384, -along * cos / 16384),
        _ => (along * cos / 16384, along * sin / 16384),
    };
    for y in 0..SIDE {
        for x in 0..SIDE {
            let coverage = BATTERY_STROKE[y * SIDE + x] as u32;
            let mut filled = if value == 100 { 16 } else { 0 };
            let mut hole = 0u32;
            if coverage != 0 && percent.is_some() {
                for sy in 0..4 {
                    for sx in 0..4 {
                        let dx = x as i32 * 8 + 1 + sx * 2 - 256;
                        let dy = y as i32 * 8 + 1 + sy * 2 - 256;
                        if (dx - ex) * (dx - ex) + (dy - ey) * (dy - ey) <= 22 * 22 {
                            hole += 1;
                        }
                        if value == 0 || value == 100 {
                            continue;
                        }
                        let (nx, ny) = (-dx, -dy);
                        let (q, u, v) = match (nx >= 0, ny >= 0) {
                            (true, false) => (0, -ny, nx),
                            (true, true) => (1, nx, ny),
                            (false, true) => (2, ny, -nx),
                            (false, false) => (3, -nx, -ny),
                        };
                        let cap = dx * dx + (dy - along) * (dy - along) <= cap_radius * cap_radius
                            || (dx - ex) * (dx - ex) + (dy - ey) * (dy - ey)
                                <= cap_radius * cap_radius;
                        if q < quarter || q == quarter && v * cos <= u * sin || cap {
                            filled += 1;
                        }
                    }
                }
            }
            let at = (y * SIDE + x) * 4;
            // Compose the solid stroke over the flat premultiplied disk/glyph.
            for c in 0..3 {
                let rgb = 2 - c;
                let ink = (ACTIVE[rgb] * filled + REMAINING[rgb] * (16 - filled)) / 16;
                let ink = (ink * (16 - hole) + BACKGROUND[rgb] * hole) / 16;
                let channel = ink * coverage / 255 + SPHERE[at + c] as u32 * (255 - coverage) / 255;
                output[at + c] = (channel * opacity / 255) as u8;
            }
            let alpha = coverage + SPHERE[at + 3] as u32 * (255 - coverage) / 255;
            output[at + 3] = (alpha * opacity / 255) as u8;
        }
    }
}

pub fn decode(data: &[u8], out: &mut [u8]) -> bool {
    let mut at = 0;
    let mut used = 0;
    while at < data.len() {
        let tag = data[at];
        at += 1;
        let count = (tag & 127) as usize + 1;
        let Some(target) = out.get_mut(used..used + count) else {
            return false;
        };
        if tag & 128 != 0 {
            let Some(&value) = data.get(at) else {
                return false;
            };
            at += 1;
            target.fill(value);
        } else {
            let Some(literal) = data.get(at..at + count) else {
                return false;
            };
            at += count;
            target.copy_from_slice(literal);
        }
        used += count;
    }
    used == out.len()
}
fn word(data: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?) as usize)
}
pub struct BandPainter {
    size: u8,
    coverage: [u8; N],
    separators: [u8; N],
}
impl BandPainter {
    pub const NEW: Self = Self {
        size: 0,
        coverage: [0; N],
        separators: [0; N],
    };
    pub fn mask(&mut self, size: u8) -> Option<&[u8]> {
        if self.prepare(size) {
            Some(&self.coverage)
        } else {
            None
        }
    }
    fn prepare(&mut self, size: u8) -> bool {
        if self.size == size {
            return true;
        }
        if !(32..=72).contains(&size) || MASKS.get(..4) != Some(b"ARB2") {
            return false;
        }
        let at = 8 + (size - 32) as usize * 16;
        for (offset, dst) in [(0, &mut self.coverage), (8, &mut self.separators)] {
            let (Some(start), Some(len)) = (word(MASKS, at + offset), word(MASKS, at + offset + 4))
            else {
                return false;
            };
            let Some(data) = MASKS.get(start..start + len) else {
                return false;
            };
            if !decode(data, dst) {
                return false;
            }
        }
        self.size = size;
        true
    }
    pub fn render(&mut self, output: &mut [u8; BAND_BYTES], style: super::model::BandStyle) {
        if !self.prepare(style.size) {
            output.fill(0);
            return;
        }
        let (inner, outer) = super::model::band_radii(style.size);
        for y in 0..BAND_SIDE {
            for x in 0..BAND_SIDE {
                let cx = if style.corner.right() {
                    BAND_SIDE - 1 - x
                } else {
                    x
                };
                let cy = if style.corner.bottom() {
                    BAND_SIDE - 1 - y
                } else {
                    y
                };
                let source = cy * BAND_SIDE + cx;
                let at = (y * BAND_SIDE + x) * 4;
                let coverage = self.coverage[source] as u32;
                if coverage == 0 {
                    output[at..at + 4].fill(0);
                    continue;
                }
                let u = (cx * 2 + 1) as u32;
                let v = (cy * 2 + 1) as u32;
                let filled = style
                    .brightness
                    .is_some_and(|p| v * 1000 <= p as u32 * (u + v));
                let mut alpha = if filled {
                    245
                } else if style.translucent {
                    158
                } else {
                    255
                };
                let mut channel = if filled { 239 * alpha / 255 } else { 0 };
                let separator = if style.brightness.is_none() {
                    self.separators[source] as u32 * 195 / 255
                } else {
                    0
                };
                if separator != 0 {
                    channel = 132 * separator / 255 + channel * (255 - separator) / 255;
                    alpha = separator + alpha * (255 - separator) / 255;
                }
                if let Some(index) = style.highlight.filter(|&i| i < 3) {
                    let (co, si) = [(966, 259), (707, 707), (259, 966)][index as usize];
                    let du = (inner + 8) * co * (BAND_SIDE as i32) * 2 / (outer * 1000);
                    let dv = (inner + 8) * si * (BAND_SIDE as i32) * 2 / (outer * 1000);
                    let dx = u as i32 - du;
                    let dy = v as i32 - dv;
                    let r = 5 * BAND_SIDE as i32 / outer;
                    let distance = dx * dx + dy * dy;
                    if distance < r * r {
                        let a = ((r * r - distance) * 210 / (r * r)) as u32;
                        channel = 175 * a / 255 + channel * (255 - a) / 255;
                        alpha = a + alpha * (255 - a) / 255;
                    }
                }
                let color = (channel * coverage / 255) as u8;
                output[at..at + 3].fill(color);
                output[at + 3] = (alpha * coverage / 255) as u8;
            }
        }
    }
}
/// Host rendering convenience; the native UI owns a persistent painter.
pub fn render_band(output: &mut [u8; BAND_BYTES], style: super::model::BandStyle) {
    let mut painter = BandPainter::NEW;
    painter.render(output, style);
}
