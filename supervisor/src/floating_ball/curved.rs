//! Curved per-glyph typography in native indexed-alpha images. No new firmware
//! transforms or guessed style properties: glyphs rotate into a cached mask.
use super::model::{self, Action, Corner};
use super::pixels::BAND_SIDE;
pub const PALETTE_BYTES: usize = 1024;
pub const BYTES: usize = PALETTE_BYTES + BAND_SIDE * BAND_SIDE;
static ATLAS: &[u8] = include_bytes!("../../assets/floating-ball/glyphs.bin");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Main,
    Duration,
    Brightness,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub corner: Corner,
    pub size: u8,
    pub kind: Kind,
    pub slots: [Action; 3],
    pub clock: [u8; 6],
    pub brightness: u32,
}
#[derive(Clone, Copy)]
struct Glyph {
    offset: usize,
    advance: i32,
}
impl Glyph {
    const EMPTY: Self = Self {
        offset: 0,
        advance: 0,
    };
}
fn u16_at(at: usize) -> u16 {
    u16::from_le_bytes([ATLAS[at], ATLAS[at + 1]])
}
fn glyph(c: char) -> Option<Glyph> {
    if ATLAS.get(..4) != Some(b"GAF1") {
        return None;
    }
    let count = u16_at(4) as usize;
    let side = ATLAS[6] as usize;
    if count > 64 || side != 28 || ATLAS.len() != 8 + count * 8 + count * side * side {
        return None;
    }
    for i in 0..count {
        let at = 8 + i * 8;
        let code = u32::from_le_bytes(ATLAS[at..at + 4].try_into().ok()?);
        if code == c as u32 {
            return Some(Glyph {
                offset: 8 + count * 8 + i * side * side,
                advance: u16_at(at + 4) as i32,
            });
        }
    }
    None
}
// Bhaskara's bounded integer sine approximation, angle in degrees / 256.
pub(super) fn sine(angle: i32) -> i32 {
    let x = angle.clamp(0, 90 * 256) as i64;
    let p = x * (180 * 256 - x);
    (4 * p * 16384 / (40500 * 256 * 256 - p)) as i32
}
fn sample(glyph: Glyph, x: i32, y: i32) -> u32 {
    if x < 0 || y < 0 || x >= 27 * 256 || y >= 27 * 256 {
        return 0;
    }
    let (ix, iy) = ((x / 256) as usize, (y / 256) as usize);
    let (fx, fy) = ((x & 255) as u32, (y & 255) as u32);
    let at = glyph.offset + iy * 28 + ix;
    let a = ATLAS[at] as u32 * (256 - fx) + ATLAS[at + 1] as u32 * fx;
    let b = ATLAS[at + 28] as u32 * (256 - fx) + ATLAS[at + 29] as u32 * fx;
    (a * (256 - fy) + b * fy + 32768) / 65536
}
fn draw(out: &mut [u8], style: Style, text: &str, angle: i32) {
    let mut letters = [Glyph::EMPTY; 8];
    let mut count = 0;
    let mut total = 0;
    for c in text.chars().take(8) {
        if let Some(g) = glyph(c) {
            letters[count] = g;
            count += 1;
            total += g.advance;
        }
    }
    let (inner, outer) = model::band_radii(style.size);
    let radius = (inner + outer) / 2;
    let direction = if style.corner.right() { 1 } else { -1 };
    let down = style.corner.right() != style.corner.bottom();
    let mut cursor = 0;
    for g in letters.iter().take(count) {
        let delta = (cursor + g.advance / 2 - total / 2) as i64 * 14668 / (radius as i64 * 256);
        let theta = angle * 256 + direction * delta as i32;
        let si = sine(theta);
        let co = sine(90 * 256 - theta);
        let u = radius * co;
        let v = radius * si;
        let x = if style.corner.right() {
            outer * 16384 - u
        } else {
            u
        };
        let y = if style.corner.bottom() {
            outer * 16384 - v
        } else {
            v
        };
        let cx = x as i64 * BAND_SIDE as i64 * 256 / (outer as i64 * 16384);
        let cy = y as i64 * BAND_SIDE as i64 * 256 / (outer as i64 * 16384);
        let cos = si;
        let sin = if down { co } else { -co };
        let bounds = 22 * BAND_SIDE as i32 / outer + 2;
        for py in ((cy as i32 / 256 - bounds).max(0))
            ..((cy as i32 / 256 + bounds + 1).min(BAND_SIDE as i32))
        {
            for px in ((cx as i32 / 256 - bounds).max(0))
                ..((cx as i32 / 256 + bounds + 1).min(BAND_SIDE as i32))
            {
                let dx = ((px * 256 + 128) as i64 - cx) * outer as i64 / BAND_SIDE as i64;
                let dy = ((py * 256 + 128) as i64 - cy) * outer as i64 / BAND_SIDE as i64;
                let gx = 14 * 256 + (dx * cos as i64 + dy * sin as i64) / 16384;
                let gy = 14 * 256 + (-dx * sin as i64 + dy * cos as i64) / 16384;
                let a = sample(*g, gx as i32, gy as i32);
                if a == 0 {
                    continue;
                }
                let at = py as usize * BAND_SIDE + px as usize;
                let old = (out[at] & 127) as u32 * 255 / 127;
                let alpha = a + old * (255 - a) / 255;
                let u = if style.corner.right() {
                    BAND_SIDE as i32 * 2 - 2 * px - 1
                } else {
                    2 * px + 1
                };
                let v = if style.corner.bottom() {
                    BAND_SIDE as i32 * 2 - 2 * py - 1
                } else {
                    2 * py + 1
                };
                let dark = style.kind == Kind::Brightness
                    && v as u32 * 1000
                        <= model::brightness_progress(style.brightness) as u32 * (u + v) as u32;
                out[at] = (alpha * 127 / 255) as u8 + if dark { 128 } else { 0 };
            }
        }
        cursor += g.advance;
    }
}
pub fn render(out: &mut [u8; BYTES], style: Style, coverage: &[u8]) {
    for i in 0..256 {
        let alpha = (i & 127) * 255 / 127;
        let c = if i < 128 { 250 } else { 22 };
        out[i * 4..i * 4 + 4].copy_from_slice(&[c, c, c, alpha as u8]);
    }
    let pixels = &mut out[PALETTE_BYTES..];
    pixels.fill(0);
    match style.kind {
        Kind::Main => {
            for i in 0..3 {
                let text = if style.slots[i] == Action::Time {
                    core::str::from_utf8(&style.clock[..5]).unwrap_or("--:--")
                } else {
                    let n = style.slots[i].short_name();
                    core::str::from_utf8(&n[..n.len() - 1]).unwrap_or("")
                };
                draw(pixels, style, text, [15, 45, 75][i]);
            }
        }
        Kind::Duration => {
            for (i, text) in ["5min", "10min", "15min"].iter().enumerate() {
                draw(pixels, style, text, [15, 45, 75][i]);
            }
        }
        Kind::Brightness => {
            let mut digits = [0u8; 12];
            let mut value = style.brightness.clamp(1, 2000);
            let mut count = 0;
            loop {
                digits[count] = b'0' + (value % 10) as u8;
                count += 1;
                value /= 10;
                if value == 0 {
                    break;
                }
            }
            digits[..count].reverse();
            digits[count..count + 3].copy_from_slice(b"nit");
            if let Ok(text) = core::str::from_utf8(&digits[..count + 3]) {
                draw(pixels, style, text, 45);
            }
        }
    }
    // Clip rotated glyphs to the same rounded silhouette as the black band.
    if coverage.len() != BAND_SIDE * BAND_SIDE {
        pixels.fill(0);
        return;
    }
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
            let at = y * BAND_SIDE + x;
            let value = pixels[at];
            let alpha = (value & 127) as u32 * coverage[cy * BAND_SIDE + cx] as u32 / 255;
            pixels[at] = if alpha == 0 {
                0
            } else {
                (value & 128) | alpha as u8
            };
        }
    }
}
