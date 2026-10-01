//! 文件系统：NuttX VFS 封装
//!
//! 本文件是 `fw_api` 的子模块，调用方式仍是 `fw_api::名字`（父模块已 `pub use` 转出）。

use core::mem::transmute;
use super::*;

// ===========================================================================
// 6. 文件系统（NuttX VFS）
// ===========================================================================

/// `open(path, oflag, mode)` 只读请用 `oflag::RDONLY`（=1！） 已验证
pub unsafe extern "C" fn open(path: Cp, flags: u32, mode: u32) -> i32 {
    let f: unsafe extern "C" fn(Cp, u32, u32) -> i32 = transmute(0x0C1D_0A29usize);
    f(path, flags, mode)
}

pub unsafe extern "C" fn close(fd: i32) -> i32 {
    let f: unsafe extern "C" fn(i32) -> i32 = transmute(0x0C1B_9D81usize);
    f(fd)
}

/// `lseek(fd, off, whence)` -> 新位置(64 位); <0 = 失败。`whence` 取值见 `seek`。
///
/// 依据: 固件自己的 log_service 就是这么用的 —— 0x0C73D550 `lseek(tmp_fd, 0, SEEK_END)`
/// 把 tmp.log 定位到文件尾(追加模式)。本体 0x0C1D0524 反汇编: whence 取栈上第 3 参
/// (int64 占 r2:r3), 1 走 SEEK_CUR 分支、2 走"读文件大小"分支、其余 EINVAL(0x16)。
pub unsafe extern "C" fn lseek(fd: i32, off: i64, whence: u32) -> i64 {
    let f: unsafe extern "C" fn(i32, i64, u32) -> i64 = transmute(0x0C1D_0525usize);
    f(fd, off, whence)
}

/// `lseek` 的 whence(与固件 0x0C1D0524 的分支逐个对应)
pub mod seek {
    pub const SET: u32 = 0;
    pub const CUR: u32 = 1;
    pub const END: u32 = 2;
}

pub unsafe extern "C" fn read(fd: i32, buf: *mut u8, len: u32) -> i32 {
    let f: unsafe extern "C" fn(i32, *mut u8, u32) -> i32 = transmute(0x0C1D_129Dusize);
    f(fd, buf, len)
}

pub unsafe extern "C" fn write(fd: i32, buf: *const u8, len: u32) -> i32 {
    let f: unsafe extern "C" fn(i32, *const u8, u32) -> i32 = transmute(0x0C1D_2641usize);
    f(fd, buf, len)
}

/// `opendir(path)` **目录不能用 open()（返回 -6 ENXIX）** 已验证
pub unsafe extern "C" fn opendir(path: Cp) -> Obj {
    let f: unsafe extern "C" fn(Cp) -> Obj = transmute(0x0C1E_4529usize);
    f(path)
}

/// `readdir(dir)` → `dirent*`，`{u8 d_type@0, char d_name@1}`；返回 0 = 已读完 已验证
pub unsafe extern "C" fn readdir(dir: Obj) -> Obj {
    let f: unsafe extern "C" fn(Obj) -> Obj = transmute(0x0C1E_4591usize);
    f(dir)
}

pub unsafe extern "C" fn closedir(dir: Obj) -> i32 {
    let f: unsafe extern "C" fn(Obj) -> i32 = transmute(0x0C1E_4565usize);
    f(dir)
}

/// `remove(path)` 删文件；对目录自动退到 rmdir(只能删空目录) 静态核对
///
/// 判据(`dfx_rmdir_recursive` 0x0C3D36DC 反汇编实证)：内部先走 VFS unlink，
/// errno==0x15(EISDIR) 时改调 `rmdir`(0x0C1D1648) —— NuttX `remove()` 的标准形态；
/// 与 open/opendir 同 libc 区，全盘 182 处 BL 调用点。返回 0=成功, -1=errno。
pub unsafe extern "C" fn fs_remove(path: Cp) -> i32 {
    let f: unsafe extern "C" fn(Cp) -> i32 = transmute(0x0C1E_ABE1usize);
    f(path)
}

