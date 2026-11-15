//! UI-owned settings with atomic snapshots for workers and verified rename saves.
use core::sync::atomic::{AtomicU32, Ordering};
use crate::{fw_api as fw, capture_format::{Storage, VideoFormat}};
const FILE: &[u8] = b"/data/chaos/capture.cfg\0";
const TEMP: &[u8] = b"/data/chaos/capture.cfg.tmp\0";
const LOADED: u32 = 1 << 31;
static VALUE: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings { pub format: VideoFormat, pub recording: Storage, pub screenshot: Storage }
impl Settings {
    pub const DEFAULT: Self = Self { format: VideoFormat::Gif, recording: Storage::OfflineLog, screenshot: Storage::OfflineLog };
    pub fn pack(self) -> u32 { self.format as u32 | ((self.recording as u32) << 1) | ((self.screenshot as u32) << 2) }
    pub fn unpack(bits: u32) -> Self { Self {
        format: if bits & 1 == 0 { VideoFormat::Gif } else { VideoFormat::Apng },
        recording: if bits & 2 == 0 { Storage::OfflineLog } else { Storage::Media },
        screenshot: if bits & 4 == 0 { Storage::OfflineLog } else { Storage::Media },
    } }
    fn bytes(self) -> [u8; 8] {
        [b'C', b'A', b'P', b'2', self.format as u8, self.recording as u8, self.screenshot as u8,
            self.format as u8 ^ self.recording as u8 ^ self.screenshot as u8 ^ 0x5a]
    }
}
fn parse(bytes: &[u8; 8]) -> Option<Settings> {
    if &bytes[..4] != b"CAP2" || bytes[4..7].iter().any(|&v| v > 1)
        || bytes[7] != bytes[4] ^ bytes[5] ^ bytes[6] ^ 0x5a { return None; }
    Some(Settings::unpack(bytes[4] as u32 | (bytes[5] as u32) << 1 | (bytes[6] as u32) << 2))
}
unsafe fn read(path: *const u8) -> Result<[u8; 8], i32> {
    let fd = fw::capture_result(fw::open(path, fw::oflag::RDONLY, 0));
    if fd < 0 { return Err(fd); }
    let mut bytes = [0; 8]; let mut used = 0; let mut result = Ok(()); let mut interrupted = 0;
    while used < bytes.len() {
        let n = fw::nsh_read(fd, &mut bytes[used..]);
        if n == -4 && interrupted < 8 { interrupted += 1; continue; }
        if n <= 0 { result = Err(if n < 0 { n } else { -5 }); break; }
        if n as usize > bytes.len() - used { result = Err(-5); break; }
        used += n as usize;
    }
    if result.is_ok() { let mut extra = [0]; if fw::nsh_read(fd, &mut extra) != 0 { result = Err(-5); } }
    let close = fw::capture_result(fw::close(fd));
    if close < 0 { return Err(close); } result.map(|_| bytes)
}
pub fn get() -> Settings {
    let current = VALUE.load(Ordering::Acquire);
    if current & LOADED != 0 { return Settings::unpack(current); }
    let setting = unsafe { read(FILE.as_ptr()).ok().and_then(|b| parse(&b)) }.unwrap_or(Settings::DEFAULT);
    VALUE.store(setting.pack() | LOADED, Ordering::Release); setting
}
pub fn set(setting: Settings) -> Result<(), i32> { unsafe {
    let rc = fw::capture_mkdir_path(b"/data/chaos\0".as_ptr());
    if rc < 0 && rc != -17 { return Err(rc); }
    let bytes = setting.bytes();
    // NuttX O_TRUNC=0x20; rewrite only our temporary config, never the live one.
    let fd = fw::capture_result(fw::open(TEMP.as_ptr(), fw::oflag::WRONLY | fw::oflag::CREAT | 0x20, 0o640));
    if fd < 0 { return Err(fd); }
    let mut used = 0; let mut result = Ok(()); let mut interrupted = 0;
    while used < bytes.len() {
        let n = fw::nsh_write(fd, &bytes[used..]);
        if n == -4 && interrupted < 8 { interrupted += 1; continue; }
        if n <= 0 { result = Err(if n < 0 { n } else { -5 }); break; }
        if n as usize > bytes.len() - used { result = Err(-5); break; }
        used += n as usize;
    }
    let close = fw::capture_result(fw::close(fd));
    if close < 0 { result = Err(close); }
    if result.is_ok() { result = match read(TEMP.as_ptr()) { Ok(found) if found == bytes => Ok(()), Ok(_) => Err(-5), Err(e) => Err(e) }; }
    if result.is_ok() {
        let rc = fw::capture_rename(TEMP.as_ptr(), FILE.as_ptr());
        if rc < 0 { result = Err(rc); }
    }
    if result.is_err() { fw::fs_remove(TEMP.as_ptr()); }
    result?; VALUE.store(setting.pack() | LOADED, Ordering::Release); Ok(())
} }
#[cfg(test)]
pub fn reset_for_test() { VALUE.store(0, Ordering::Release); }
