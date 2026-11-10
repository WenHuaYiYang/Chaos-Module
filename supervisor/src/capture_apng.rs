//! APNG with millisecond timing and streaming fixed-Huffman/RLE DEFLATE.
//! Indexed RGB332 matches GIF quality; no allocator, dependencies or match table.
use crate::capture_format::{Sink, Rect, WIDTH, HEIGHT, PIXELS, PALETTE};

const fn crc_table() -> [u32; 256] {
    let mut t = [0; 256]; let mut n = 0;
    while n < 256 { let mut v = n as u32; let mut k = 0;
        while k < 8 { v = (v >> 1) ^ (0xedb88320u32 & 0u32.wrapping_sub(v & 1)); k += 1; }
        t[n] = v; n += 1;
    } t
}
const CRC: [u32; 256] = crc_table();
fn crc(mut c: u32, data: &[u8]) -> u32 {
    for &b in data { c = CRC[((c ^ b as u32) & 255) as usize] ^ (c >> 8); } c
}
fn chunk(out: &mut impl Sink, tag: &[u8; 4], data: &[u8]) -> Result<(), i32> {
    out.put(&(data.len() as u32).to_be_bytes())?; out.put(tag)?; out.put(data)?;
    out.put(&(!crc(crc(u32::MAX, tag), data)).to_be_bytes())
}
pub const ACTL_OFFSET: i64 = 813;
pub fn animation_control(out: &mut impl Sink, frames: u32) -> Result<(), i32> {
    let mut data = [0; 8]; data[..4].copy_from_slice(&frames.to_be_bytes()); chunk(out, b"acTL", &data)
}
pub fn header(out: &mut impl Sink) -> Result<(), i32> {
    out.put(b"\x89PNG\r\n\x1a\n")?;
    let mut ihdr = [0; 13]; ihdr[..4].copy_from_slice(&(WIDTH as u32).to_be_bytes());
    ihdr[4..8].copy_from_slice(&(HEIGHT as u32).to_be_bytes()); ihdr[8] = 8; ihdr[9] = 3;
    chunk(out, b"IHDR", &ihdr)?;
    chunk(out, b"PLTE", &PALETTE)?;
    // Patched once at commit, including CRC. Temporary files are never published.
    animation_control(out, 1)
}

pub struct Encoder { pub frames: u32, sequence: u32 }
impl Encoder {
    pub const fn new() -> Self { Self { frames: 0, sequence: 0 } }
    pub fn frame(&mut self, out: &mut impl Sink, indices: &[u8], rect: Rect, milliseconds: u32) -> Result<(), i32> {
        if indices.len() != PIXELS || rect.x >= WIDTH || rect.y >= HEIGHT || rect.width == 0 || rect.height == 0
            || rect.width > WIDTH - rect.x || rect.height > HEIGHT - rect.y { return Err(-22); }
        if self.frames == 0 && rect != Rect::FULL { return Err(-22); }
        let (num, den) = duration(milliseconds);
        let mut fctl = [0; 26]; fctl[..4].copy_from_slice(&self.sequence.to_be_bytes()); self.sequence += 1;
        for (offset, value) in [(4, rect.width), (8, rect.height), (12, rect.x), (16, rect.y)] {
            fctl[offset..offset + 4].copy_from_slice(&(value as u32).to_be_bytes());
        }
        fctl[20..22].copy_from_slice(&num.to_be_bytes()); fctl[22..24].copy_from_slice(&den.to_be_bytes());
        // NONE disposal + SOURCE blend replaces every pixel in the changed rectangle.
        chunk(out, b"fcTL", &fctl)?;
        let mut stream = DataChunks { out, sequence: &mut self.sequence, first: self.frames == 0, data: [0; 4096], len: 0 };
        stream.byte(0x78)?; stream.byte(0x01)?;
        let mut deflate = Rle { output: &mut stream, bits: 0, used: 0, last: 0, run: 0, a: 1, b: 0, adler_count: 0 };
        deflate.bits(3, 3)?; // BFINAL=1, fixed Huffman BTYPE=01.
        for y in rect.y..rect.y + rect.height {
            deflate.byte(0)?; // PNG filter None, one byte per scanline.
            for &value in &indices[y * WIDTH + rect.x..y * WIDTH + rect.x + rect.width] { deflate.byte(value)?; }
        }
        deflate.finish()?; stream.flush()?; self.frames += 1; Ok(())
    }
    pub fn end(&self, out: &mut impl Sink) -> Result<(), i32> { chunk(out, b"IEND", &[]) }
}

/// Exact milliseconds when representable; long holds fall back to 10 ms.
pub fn duration(ms: u32) -> (u16, u16) {
    fn gcd(mut a: u32, mut b: u32) -> u32 { while b != 0 { let n = a % b; a = b; b = n; } a }
    let ms = ms.clamp(1, 600_000); let divisor = gcd(ms, 1000);
    if ms / divisor <= 65535 { ((ms / divisor) as u16, (1000 / divisor) as u16) }
    else { (((ms + 5) / 10) as u16, 100) }
}

struct DataChunks<'a, S> { out: &'a mut S, sequence: &'a mut u32, first: bool, data: [u8; 4096], len: usize }
impl<S: Sink> DataChunks<'_, S> {
    #[inline(always)]
    fn byte(&mut self, byte: u8) -> Result<(), i32> {
        self.data[self.len] = byte; self.len += 1;
        if self.len == self.data.len() { self.flush()?; } Ok(())
    }
    fn flush(&mut self) -> Result<(), i32> {
        if self.len == 0 { return Ok(()); }
        if self.first { chunk(self.out, b"IDAT", &self.data[..self.len])?; }
        else {
            let sequence = self.sequence.to_be_bytes(); *self.sequence += 1;
            self.out.put(&((self.len + 4) as u32).to_be_bytes())?; self.out.put(b"fdAT")?;
            self.out.put(&sequence)?; self.out.put(&self.data[..self.len])?;
            let checksum = !crc(crc(crc(u32::MAX, b"fdAT"), &sequence), &self.data[..self.len]);
            self.out.put(&checksum.to_be_bytes())?;
        }
        self.len = 0; Ok(())
    }
}
struct Rle<'a, 'b, S> {
    output: &'a mut DataChunks<'b, S>, bits: u32, used: u32,
    last: u8, run: usize, a: u32, b: u32, adler_count: u32,
}
impl<S: Sink> Rle<'_, '_, S> {
    #[inline(always)]
    fn bits(&mut self, value: u32, count: u32) -> Result<(), i32> {
        self.bits |= value << self.used; self.used += count;
        while self.used >= 8 { self.output.byte(self.bits as u8)?; self.bits >>= 8; self.used -= 8; } Ok(())
    }
    #[inline(always)]
    fn symbol(&mut self, symbol: u32) -> Result<(), i32> {
        let (code, n) = match symbol { 0..=143 => (symbol + 0x30, 8), 144..=255 => (symbol - 144 + 0x190, 9),
            256..=279 => (symbol - 256, 7), _ => (symbol - 280 + 0xc0, 8) };
        self.bits(code.reverse_bits() >> (32 - n), n)
    }
    #[inline(always)]
    fn byte(&mut self, value: u8) -> Result<(), i32> {
        self.a += value as u32; self.b += self.a; self.adler_count += 1;
        if self.adler_count == 5552 { self.a %= 65521; self.b %= 65521; self.adler_count = 0; }
        if self.run != 0 && self.last != value {
            // Singleton literals dominate high-change regions. Keep this
            // small path inline, avoiding the generic length-code routine.
            if self.run == 1 { self.symbol(self.last as u32)?; self.run = 0; }
            else { self.flush_run()?; }
        }
        self.last = value; self.run += 1;
        // Keep the run bounded and avoid an expensive search for length codes.
        if self.run == 259 { self.flush_run()?; } Ok(())
    }
    fn flush_run(&mut self) -> Result<(), i32> {
        if self.run == 0 { return Ok(()); }
        self.symbol(self.last as u32)?; let mut remaining = self.run - 1;
        if remaining >= 3 {
            const BASE: [usize; 29] = [3,4,5,6,7,8,9,10,11,13,15,17,19,23,27,31,35,43,51,59,67,83,99,115,131,163,195,227,258];
            const EXTRA: [u32; 29] = [0,0,0,0,0,0,0,0,1,1,1,1,2,2,2,2,3,3,3,3,4,4,4,4,5,5,5,5,0];
            let mut code = 28; while BASE[code] > remaining { code -= 1; }
            self.symbol(257 + code as u32)?; self.bits((remaining - BASE[code]) as u32, EXTRA[code])?;
            self.bits(0, 5)?; remaining = 0; // distance=1, no extra bits
        }
        for _ in 0..remaining { self.symbol(self.last as u32)?; }
        self.run = 0; Ok(())
    }
    fn finish(mut self) -> Result<(), i32> {
        self.flush_run()?; self.symbol(256)?;
        if self.used != 0 { self.output.byte(self.bits as u8)?; }
        let checksum = ((self.b % 65521) << 16) | (self.a % 65521);
        for byte in checksum.to_be_bytes() { self.output.byte(byte)?; } Ok(())
    }
}
