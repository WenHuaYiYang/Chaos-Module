//! Small fixed-size preferences: exact reads and verified atomic replacement.
use crate::fw_api as fw;

pub(crate) unsafe fn read<const N: usize>(path: &[u8]) -> Option<[u8; N]> {
    let fd = fw::capture_result(fw::open(path.as_ptr(), fw::oflag::RDONLY, 0));
    if fd < 0 { return None; }
    let mut bytes = [0; N];
    let mut used = 0;
    let mut interrupts = 0;
    let mut valid = true;
    while used < N {
        let n = fw::nsh_read(fd, &mut bytes[used..]);
        if n == -4 && interrupts < 8 { interrupts += 1; continue; }
        if n <= 0 || n as usize > N - used { valid = false; break; }
        used += n as usize;
    }
    if valid {
        let mut extra = [0];
        loop {
            let n = fw::nsh_read(fd, &mut extra);
            if n == -4 && interrupts < 8 { interrupts += 1; continue; }
            valid = n == 0; break;
        }
    }
    if fw::capture_result(fw::close(fd)) < 0 { valid = false; }
    if valid { Some(bytes) } else { None }
}

pub(crate) unsafe fn save<const N: usize>(path: &[u8], temporary: &[u8], bytes: &[u8; N]) -> bool {
    let rc = fw::capture_mkdir_path(b"/data/chaos\0".as_ptr());
    if rc < 0 && rc != -17 { return false; }
    let fd = fw::capture_result(fw::open(temporary.as_ptr(), fw::oflag::WRONLY | fw::oflag::CREAT | 0x20, 0o640));
    if fd < 0 { return false; }
    let mut used = 0;
    let mut interrupts = 0;
    let mut valid = true;
    while used < N {
        let n = fw::nsh_write(fd, &bytes[used..]);
        if n == -4 && interrupts < 8 { interrupts += 1; continue; }
        if n <= 0 || n as usize > N - used { valid = false; break; }
        used += n as usize;
    }
    if fw::capture_result(fw::close(fd)) < 0 { valid = false; }
    if valid { valid = read::<N>(temporary).as_ref() == Some(bytes); }
    if valid { valid = fw::capture_rename(temporary.as_ptr(), path.as_ptr()) == 0; }
    if !valid { fw::fs_remove(temporary.as_ptr()); }
    valid
}
