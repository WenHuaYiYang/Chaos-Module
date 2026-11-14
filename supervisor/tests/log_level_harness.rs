#![allow(dead_code, static_mut_refs)]
mod fw_api {
    pub mod oflag { pub const RDONLY: u32 = 0; pub const WRONLY: u32 = 1; pub const CREAT: u32 = 2; }
    pub static mut MASK: u8 = 0x7f;
    pub static mut FILE: Option<[u8; 6]> = None;
    pub static mut TEMP: [u8; 6] = [0; 6];
    pub static mut FAIL: bool = false;
    pub unsafe fn system_log_mask() -> u8 { MASK }
    pub unsafe fn system_log_mask_set(v: u8) { MASK = v; }
    pub unsafe fn open(_: *const u8, flags: u32, _: u32) -> i32 {
        if FAIL || (flags == 0 && FILE.is_none()) { -1 } else { 3 }
    }
    pub unsafe fn read(_: i32, p: *mut u8, _: u32) -> i32 {
        if let Some(b) = FILE { core::ptr::copy_nonoverlapping(b.as_ptr(), p, 6); 6 } else { -1 }
    }
    pub unsafe fn write(_: i32, p: *const u8, _: u32) -> i32 {
        core::ptr::copy_nonoverlapping(p, TEMP.as_mut_ptr(), 6); 6
    }
    pub unsafe fn close(_: i32) {}
    pub unsafe fn capture_mkdir_path(_: *const u8) -> i32 { 0 }
    pub unsafe fn capture_rename(_: *const u8, _: *const u8) -> i32 {
        if FAIL { -1 } else { FILE = Some(TEMP); 0 }
    }
}
#[path = "../src/log_level.rs"]
mod log_level;
