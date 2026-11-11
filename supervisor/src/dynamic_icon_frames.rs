//! Bounded metadata and allocation-free decoder for native GIF frames (DGF1).
pub const WIDTH: usize = 96;
pub const HEIGHT: usize = 96;
pub const PIXELS: usize = WIDTH * HEIGHT;
pub const PALETTE_BYTES: usize = 1024;
pub const HEADER_BYTES: usize = 60;
pub const MAX_FRAMES: usize = 256;
pub const MAX_METADATA: usize = HEADER_BYTES + PALETTE_BYTES + MAX_FRAMES * 12;
pub const MAX_ENCODED: usize = PIXELS * 2;
pub const MAX_FILE_BYTES: usize = 1024 * 1024;
pub static PACKAGED: &[u8] = include_bytes!("../assets/dynamic-icon.frames");

#[derive(Clone, Copy)]
pub struct FrameIndex<'a> {
    data: &'a [u8],
    pub count: usize,
    pub cycle_ms: u32,
    pub plays: u32,
    pub period_ms: u32,
    table: usize,
}
fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at.checked_add(4)?)?.try_into().ok()?))
}
pub fn metadata_len(header: &[u8]) -> Option<usize> {
    if header.get(..8)? != [b'D',b'G',b'F',b'1',96,0,96,0] || header.get(10..12)? != [0,0] { return None; }
    let count = u16::from_le_bytes(header.get(8..10)?.try_into().ok()?) as usize;
    if !(1..=MAX_FRAMES).contains(&count) { return None; }
    Some(HEADER_BYTES + PALETTE_BYTES + count * 12)
}
impl<'a> FrameIndex<'a> {
    /// Metadata includes the palette and complete frame table, never payloads.
    pub fn parse(data: &'a [u8], file_bytes: usize) -> Option<Self> {
        let payload = metadata_len(data)?;
        if data.len() != payload || file_bytes > MAX_FILE_BYTES { return None; }
        let count = u16::from_le_bytes(data[8..10].try_into().ok()?) as usize;
        let table = HEADER_BYTES + PALETTE_BYTES;
        if u32_at(data,20)? as usize != table || u32_at(data,24)? as usize != payload { return None; }
        let mut end = payload;
        let mut total = 0u32;
        let mut minimum = 60_000;
        for index in 0..count {
            let record = table + index * 12;
            let delay = u32_at(data,record)?;
            let start = u32_at(data,record+4)? as usize;
            let size = u32_at(data,record+8)? as usize;
            if !(1..=60_000).contains(&delay) || start != end || !(1..=MAX_ENCODED).contains(&size) { return None; }
            end = start.checked_add(size)?;
            if end > file_bytes { return None; }
            total = total.checked_add(delay)?; minimum = minimum.min(delay);
        }
        if end != file_bytes || total != u32_at(data,12)? { return None; }
        Some(Self { data,count,cycle_ms:total,plays:u32_at(data,16)?,period_ms:minimum.clamp(10,50),table })
    }
    pub fn palette(&self) -> &'a [u8] { &self.data[HEADER_BYTES..HEADER_BYTES+PALETTE_BYTES] }
    pub fn frame_at(&self, elapsed: u32) -> usize {
        if self.plays != 0 && elapsed as u64 >= self.cycle_ms as u64 * self.plays as u64 { return self.count-1; }
        let mut within = elapsed % self.cycle_ms;
        for index in 0..self.count {
            let delay = u32_at(self.data,self.table+index*12).unwrap_or(1);
            if within < delay { return index; } within -= delay;
        }
        self.count-1
    }
    pub fn block(&self, frame: usize) -> Option<(usize,usize)> {
        if frame >= self.count { return None; }
        Some((u32_at(self.data,self.table+frame*12+4)? as usize,u32_at(self.data,self.table+frame*12+8)? as usize))
    }
}
#[derive(Clone, Copy)]
pub struct Animation<'a> {
    data: &'a [u8], index: FrameIndex<'a>,
    pub count: usize, pub cycle_ms: u32, pub plays: u32, pub period_ms: u32,
}
impl<'a> Animation<'a> {
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        let index = FrameIndex::parse(data.get(..metadata_len(data)?)?,data.len())?;
        Some(Self { data,index,count:index.count,cycle_ms:index.cycle_ms,plays:index.plays,period_ms:index.period_ms })
    }
    pub fn palette(&self) -> &'a [u8] { self.index.palette() }
    pub fn frame_at(&self, elapsed: u32) -> usize { self.index.frame_at(elapsed) }
    pub fn decode_frame(&self, frame: usize, pixels: &mut [u8]) -> bool {
        let Some((start,size)) = self.index.block(frame) else { return false; };
        decode(&self.data[start..start+size],pixels)
    }
}
pub fn decode(encoded: &[u8], pixels: &mut [u8]) -> bool {
    if pixels.len() != PIXELS { return false; }
    let (mut read,mut written) = (0,0);
    while read < encoded.len() {
        let tag = encoded[read]; read+=1;
        let n = (tag & 127) as usize + 1;
        let Some(target) = pixels.get_mut(written..written+n) else { return false; };
        if tag & 128 != 0 {
            let Some(&value) = encoded.get(read) else { return false; }; read+=1; target.fill(value);
        } else {
            let Some(literal) = encoded.get(read..read+n) else { return false; }; read+=n; target.copy_from_slice(literal);
        }
        written+=n;
    }
    written == pixels.len()
}
