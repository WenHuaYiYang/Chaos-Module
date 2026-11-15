//! Streaming GIF89a/BMP encoders. Input is the p67 RGB888 framebuffer (B,G,R).
//! No allocation, firmware calls or UI handles in this module.

pub const WIDTH: usize = 336;
pub const HEIGHT: usize = 480;
pub const FRAME_BYTES: usize = WIDTH * HEIGHT * 3;
pub const NAME_BYTES: usize = 40;
pub const ROOT: &[u8] = b"/data/offlinelog\0";
pub const MEDIA_ROOT: &[u8] = b"/data/chaos/media\0";
/// Preserve the fast RGB332 index, but make its eight neutral cells neutral.
/// Independent 3/3/2-bit channel expansion otherwise turns gray cards yellow
/// (e.g. input 48/48/48 becomes 36/36/0) or blue. Only six entries change.
const fn make_palette() -> [u8; 768] {
    let mut palette = [0; 768]; let mut i = 0;
    while i < 256 {
        let r = (i >> 5) & 7; let g = (i >> 2) & 7; let b = i & 3;
        palette[i * 3] = (r * 255 / 7) as u8;
        palette[i * 3 + 1] = (g * 255 / 7) as u8;
        palette[i * 3 + 2] = if r == g && b == r >> 1 { (r * 255 / 7) as u8 } else { (b * 255 / 3) as u8 };
        i += 1;
    }
    palette
}
pub const PALETTE: [u8; 768] = make_palette();
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Storage { OfflineLog = 0, Media = 1 }
impl Storage {
    pub const ALL: [Self; 2] = [Self::OfflineLog, Self::Media];
    pub const fn root(self) -> &'static [u8] { match self { Self::OfflineLog => ROOT, Self::Media => MEDIA_ROOT } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum VideoFormat { Gif = 0, Apng = 1 }
impl VideoFormat {
    pub const fn label(self) -> &'static [u8] { match self { Self::Gif => b"GIF", Self::Apng => b"APNG" } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { Recording, Screenshot }

pub fn kind(name: &[u8]) -> Option<Kind> {
    let (prefix, suffix, result) = if name.starts_with(b"chaos-rec-") {
        (b"chaos-rec-".as_slice(), if name.ends_with(b".apng") { b".apng".as_slice() } else { b".gif".as_slice() }, Kind::Recording)
    } else {
        (b"chaos-shot-".as_slice(), b".bmp".as_slice(), Kind::Screenshot)
    };
    let stem = prefix.len();
    if name.len() != stem + 17 + suffix.len() || !name.starts_with(prefix)
        || !name.ends_with(suffix) || name[stem + 8] != b'-' { return None; }
    if !name[stem..stem + 8].iter().chain(&name[stem + 9..stem + 17])
        .all(u8::is_ascii_hexdigit) { return None; }
    Some(result)
}

/// Cache cleaning preserves our completed files and in-flight transactions.
pub fn owned(name: &[u8]) -> bool {
    file_kind(name).is_some()
}
pub fn file_kind(name: &[u8]) -> Option<Kind> { kind(name.strip_suffix(b".part").unwrap_or(name)) }

pub fn name(out: &mut [u8; NAME_BYTES], kind: Kind, seconds: u32, sequence: u32) {
    name_for(out, kind, VideoFormat::Gif, seconds, sequence)
}
pub fn name_for(out: &mut [u8; NAME_BYTES], kind: Kind, format: VideoFormat, seconds: u32, sequence: u32) {
    out.fill(0);
    let prefix = if kind == Kind::Recording { b"chaos-rec-".as_slice() } else { b"chaos-shot-".as_slice() };
    out[..prefix.len()].copy_from_slice(prefix);
    let mut n = prefix.len();
    for (i, value) in [seconds, sequence].iter().enumerate() {
        if i != 0 { out[n] = b'-'; n += 1; }
        for shift in (0..8).rev() {
            out[n] = b"0123456789abcdef"[((value >> (shift * 4)) & 15) as usize]; n += 1;
        }
    }
    let extension = if kind == Kind::Screenshot { b".bmp".as_slice() }
        else if format == VideoFormat::Gif { b".gif".as_slice() } else { b".apng".as_slice() };
    out[n..n + extension.len()].copy_from_slice(extension);
}

pub fn path(name: &[u8], temporary: bool, out: &mut [u8; 96]) -> Result<(), i32> {
    path_at(name, temporary, Storage::OfflineLog, out)
}
pub fn path_at(name: &[u8], temporary: bool, storage: Storage, out: &mut [u8; 96]) -> Result<(), i32> {
    let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    if file_kind(&name[..len]).is_none() || (temporary && kind(&name[..len]).is_none()) { return Err(-22); }
    out.fill(0);
    let root = storage.root();
    let prefix = &root[..root.len() - 1];
    out[..prefix.len()].copy_from_slice(prefix);
    out[prefix.len()] = b'/';
    let end = prefix.len() + 1 + len;
    out[prefix.len() + 1..end].copy_from_slice(&name[..len]);
    if temporary { out[end..end + 5].copy_from_slice(b".part"); }
    Ok(())
}

pub trait Sink { fn put(&mut self, bytes: &[u8]) -> Result<(), i32>; }

/// /proc/fs/usage reports mount, total, used, free (B/K/M/G/T), mount.
pub fn data_free(report: &[u8]) -> Option<u64> {
    for line in report.split(|&b| b == b'\n') {
        let mut fields = line.split(u8::is_ascii_whitespace).filter(|s| !s.is_empty());
        let Some(mount) = fields.next() else { continue; };
        let (Some(_total), Some(_used), Some(token)) = (fields.next(), fields.next(), fields.next()) else { continue; };
        if mount != b"/data" && fields.next() != Some(b"/data".as_slice()) { continue; }
        let (&unit, digits) = token.split_last()?;
        let (digits, shift) = match unit.to_ascii_uppercase() {
            b'B' => (digits, 0), b'K' => (digits, 10), b'M' => (digits, 20),
            b'G' => (digits, 30), b'T' => (digits, 40), b'0'..=b'9' => (token, 0), _ => return None,
        };
        let mut value = 0u64;
        if digits.is_empty() { return None; }
        for &digit in digits {
            if !digit.is_ascii_digit() { return None; }
            value = value.checked_mul(10)?.checked_add((digit - b'0') as u64)?;
        }
        return value.checked_mul(1u64 << shift);
    }
    None
}

pub fn bmp(out: &mut impl Sink, bgr: &[u8]) -> Result<(), i32> {
    if bgr.len() != FRAME_BYTES { return Err(-22); }
    // Top-down, uncompressed BGR24. 336*3 is already a multiple of four.
    let mut h = [0u8; 54];
    h[..2].copy_from_slice(b"BM");
    h[2..6].copy_from_slice(&((54 + FRAME_BYTES) as u32).to_le_bytes());
    h[10..14].copy_from_slice(&54u32.to_le_bytes());
    h[14..18].copy_from_slice(&40u32.to_le_bytes());
    h[18..22].copy_from_slice(&(WIDTH as i32).to_le_bytes());
    h[22..26].copy_from_slice(&(-(HEIGHT as i32)).to_le_bytes());
    h[26..28].copy_from_slice(&1u16.to_le_bytes());
    h[28..30].copy_from_slice(&24u16.to_le_bytes());
    h[34..38].copy_from_slice(&(FRAME_BYTES as u32).to_le_bytes());
    out.put(&h)?; out.put(bgr)
}

pub fn gif_header(out: &mut impl Sink) -> Result<(), i32> {
    out.put(b"GIF89a")?;
    out.put(&(WIDTH as u16).to_le_bytes())?;
    out.put(&(HEIGHT as u16).to_le_bytes())?;
    out.put(&[0xf7, 0, 0])?;
    out.put(&PALETTE)?;
    out.put(b"\x21\xff\x0bNETSCAPE2.0\x03\x01\x00\x00\x00")
}

/// Heap-allocated by the worker, never constructed as a 48 KiB task-stack value.
pub struct Dictionary { keys: [u32; 8192], values: [u16; 8192] }
impl Dictionary {
    fn reset(&mut self) { self.keys.fill(u32::MAX); }
    #[inline(always)]
    fn slot(&self, key: u32) -> usize {
        let mut i = (key.wrapping_mul(0x9e3779b1) >> 19) as usize;
        while self.keys[i] != key && self.keys[i] != u32::MAX { i = (i + 1) & 8191; }
        i
    }
}

struct Codes<'a, S> { out: &'a mut S, block: [u8; 255], len: usize, bits: u32, used: u32 }
impl<S: Sink> Codes<'_, S> {
    #[inline(always)]
    fn byte(&mut self, byte: u8) -> Result<(), i32> {
        self.block[self.len] = byte; self.len += 1;
        if self.len == 255 { self.flush()?; }
        Ok(())
    }
    fn flush(&mut self) -> Result<(), i32> {
        if self.len != 0 { self.out.put(&[self.len as u8])?; self.out.put(&self.block[..self.len])?; self.len = 0; }
        Ok(())
    }
    #[inline(always)]
    fn code(&mut self, value: u16, width: u32) -> Result<(), i32> {
        self.bits |= (value as u32) << self.used; self.used += width;
        while self.used >= 8 { self.byte(self.bits as u8)?; self.bits >>= 8; self.used -= 8; }
        Ok(())
    }
    fn finish(mut self) -> Result<(), i32> {
        if self.used != 0 { self.byte(self.bits as u8)?; }
        self.flush()?; self.out.put(&[0])
    }
}

pub const PIXELS: usize = WIDTH * HEIGHT;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect { pub x: usize, pub y: usize, pub width: usize, pub height: usize }
impl Rect { pub const FULL: Self = Self { x: 0, y: 0, width: WIDTH, height: HEIGHT }; }
#[inline(always)]
fn index(bgr: &[u8]) -> u8 { (bgr[2] & 0xe0) | ((bgr[1] >> 3) & 0x1c) | (bgr[0] >> 6) }

/// Old direct-read path, retained for the host comparison benchmark only.
pub unsafe fn quantize_framebuffer(source: *const u8, indexed: &mut [u8]) {
    for (i, out) in indexed.iter_mut().enumerate() {
        let p = source.add(i * 3);
        *out = (core::ptr::read_volatile(p.add(2)) & 0xe0)
            | ((core::ptr::read_volatile(p.add(1)) >> 3) & 0x1c)
            | (core::ptr::read_volatile(p) >> 6);
    }
}
pub fn indexed_changed_rect(current: &[u8], previous: &[u8]) -> Option<Rect> {
    let (mut left, mut top, mut right, mut bottom) = (WIDTH, HEIGHT, 0, 0);
    for (y, (row, old)) in current.chunks_exact(WIDTH).zip(previous.chunks_exact(WIDTH)).enumerate() {
        if row == old { continue; }
        let first = row.iter().zip(old).position(|(a, b)| a != b).unwrap();
        let last = row.iter().zip(old).rposition(|(a, b)| a != b).unwrap();
        left = left.min(first); right = right.max(last); top = top.min(y); bottom = y;
    }
    if left == WIDTH { None } else { Some(Rect { x: left, y: top, width: right - left + 1, height: bottom - top + 1 }) }
}

pub fn quantize(bgr: &[u8], indexed: &mut [u8]) {
    for (pixel, out) in bgr.chunks_exact(3).zip(indexed.iter_mut()) { *out = index(pixel); }
}

/// Compare palette indices, so invisible RGB quantization noise costs no frame.
pub fn changed_rect(bgr: &[u8], previous: &[u8]) -> Option<Rect> {
    let (mut left, mut top, mut right, mut bottom) = (WIDTH, HEIGHT, 0, 0);
    for (i, (pixel, &old)) in bgr.chunks_exact(3).zip(previous.iter()).enumerate() {
        if index(pixel) == old { continue; }
        let x = i % WIDTH; let y = i / WIDTH;
        left = left.min(x); right = right.max(x); top = top.min(y); bottom = y;
    }
    if left == WIDTH { None } else { Some(Rect { x: left, y: top, width: right - left + 1, height: bottom - top + 1 }) }
}

pub fn gif_frame(out: &mut impl Sink, indexed: &[u8], rect: Rect, delay_cs: u16, dict: &mut Dictionary) -> Result<(), i32> {
    if indexed.len() != PIXELS || rect.width == 0 || rect.height == 0
        || rect.x >= WIDTH || rect.y >= HEIGHT || rect.width > WIDTH - rect.x || rect.height > HEIGHT - rect.y { return Err(-22); }
    let delay = delay_cs.max(1).to_le_bytes();
    out.put(&[0x21, 0xf9, 4, 4, delay[0], delay[1], 0, 0])?;
    out.put(&[0x2c])?;
    out.put(&(rect.x as u16).to_le_bytes())?; out.put(&(rect.y as u16).to_le_bytes())?;
    out.put(&(rect.width as u16).to_le_bytes())?; out.put(&(rect.height as u16).to_le_bytes())?;
    out.put(&[0, 8])?;
    let mut codes = Codes { out, block: [0; 255], len: 0, bits: 0, used: 0 };
    dict.reset();
    let mut next = 258u16; let mut width = 9;
    codes.code(256, width)?;
    let mut pixels = (rect.y..rect.y + rect.height).flat_map(|y| indexed[y * WIDTH + rect.x..y * WIDTH + rect.x + rect.width].iter().copied());
    let mut prefix = pixels.next().unwrap() as u16;
    for pixel in pixels {
        let k = pixel as u16;
        let key = ((prefix as u32) << 8) | k as u32;
        let slot = dict.slot(key);
        if dict.keys[slot] == key { prefix = dict.values[slot]; continue; }
        codes.code(prefix, width)?;
        if next < 4096 {
            dict.keys[slot] = key; dict.values[slot] = next; next += 1;
            // Encoder's dictionary is one entry ahead of the GIF decoder.
            if next > (1 << width) && width < 12 { width += 1; }
        } else {
            codes.code(256, width)?; dict.reset(); next = 258; width = 9;
        }
        prefix = k;
    }
    codes.code(prefix, width)?; codes.code(257, width)?; codes.finish()
}
