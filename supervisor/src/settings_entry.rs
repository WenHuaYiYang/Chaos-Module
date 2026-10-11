//! Settings entry, ported from Shell++ II 3.101.043 settings_rows/install.
use crate::*;
use core::mem::transmute;
static mut INSTALLED: bool = false;
static mut CHECKED: bool = false;
static mut REQUEST: bool = false;
static mut ORIGINAL: [u32; 10] = [
    0, 0x0c56634d, 0x0c566b75, 0x0c566429, 0, 0x0c5663a9, 0x0c5665f5, 0x0c566605, 0, 0,
];
pub(crate) unsafe fn request() {
    REQUEST = true;
}
unsafe fn count_original(l: u32, c: u32) -> u32 {
    let f: unsafe extern "C" fn(u32, u32) -> u32 = transmute(ORIGINAL[5] as usize);
    f(l, c)
}
fn valid(n: u32) -> bool {
    n >= 9 && n <= 32
}
unsafe extern "C" fn count(l: u32, c: u32) -> u32 {
    let n = count_original(l, c);
    if valid(n) {
        n + 1
    } else {
        n
    }
}
unsafe extern "C" fn kind(l: u32, i: u32, c: u32) -> u32 {
    let n = count_original(l, c);
    if valid(n) && i == n {
        return 0xfd;
    }
    if i < n {
        let f: unsafe extern "C" fn(u32, u32, u32) -> u32 = transmute(ORIGINAL[1] as usize);
        return f(l, i, c);
    }
    0xff
}
unsafe extern "C" fn click(e: u32) {
    if e != 0 && fw_api::event_get_code(e) == 7 && APP_REGISTERED != 0 {
        let f: unsafe extern "C" fn(u32, u32, u32, u32) = transmute(0x0ca6c359usize);
        f(APP_ID << 16, 0, 0, 0);
    }
}
unsafe extern "C" fn create(l: u32, i: u32, c: u32) -> u32 {
    let n = count_original(l, c);
    if l == 0 {
        return 0;
    }
    if i < n {
        let f: unsafe extern "C" fn(u32, u32, u32) -> u32 = transmute(ORIGINAL[2] as usize);
        return f(l, i, c);
    }
    if !valid(n) || i != n {
        return 0;
    }
    let f: unsafe extern "C" fn(u32) -> u32 = transmute(0x0c4c8515usize);
    let row = f(l);
    if row == 0 {
        return 0;
    }
    let config: unsafe extern "C" fn(u32, *const u8, *const u8, *const u8, u32, u8, u8) =
        transmute(0x0c4c8561usize);
    config(
        row,
        SETTINGS_ICON_PATH.as_ptr(),
        DISPLAY_NAME.as_ptr(),
        core::ptr::null(),
        0,
        0,
        10,
    );
    fw_api::obj_add_event(row, click as *const () as u32, 7, 0);
    row
}
unsafe extern "C" fn bind(l: u32, r: u32, i: u32, c: u32) {
    let n = count_original(l, c);
    if l == 0 || r == 0 {
        return;
    }
    if i < n {
        let f: unsafe extern "C" fn(u32, u32, u32, u32) = transmute(ORIGINAL[3] as usize);
        f(l, r, i, c);
    } else if valid(n) && i == n {
        fw_api::row_update(
            r,
            SETTINGS_ICON_PATH.as_ptr(),
            DISPLAY_NAME.as_ptr(),
            core::ptr::null(),
            0,
            1,
        );
    }
}
unsafe fn matches(t: u32, a: &[u32; 10]) -> bool {
    (0..10).all(|i| read_volatile((t + i as u32 * 4) as *const u32) == a[i])
}
unsafe fn install() -> i32 {
    if APP_REGISTERED == 0 {
        return -102;
    }
    let table = 0x200d3514u32;
    if !INSTALLED {
        let mut current = [0u32; 10];
        for i in 0..10 {
            current[i] = read_volatile((table + i as u32 * 4) as *const u32);
        }
        for i in [1usize, 2, 3, 5] {
            if current[i] == 0 || current[i] & 1 == 0 {
                return -301;
            }
        }
        ORIGINAL = current;
    }
    let mut replacement = ORIGINAL;
    replacement[1] = kind as *const () as u32 | 1;
    replacement[2] = create as *const () as u32 | 1;
    replacement[3] = bind as *const () as u32 | 1;
    replacement[5] = count as *const () as u32 | 1;
    let expected = if INSTALLED { &replacement } else { &ORIGINAL };
    if !matches(table, expected) {
        return -301;
    }
    let list = read_volatile(0x201130a8 as *const u32);
    let second = read_volatile(0x201130a0 as *const u32);
    if list != second {
        return -303;
    }
    let mut live = 0;
    if list != 0 {
        if list & 3 != 0 || list >> 24 != 0x20 || list & 0xffffff > 0xfff000 {
            return -303;
        }
        let t = list + 0x5c;
        if matches(t, &replacement) {
            if !INSTALLED {
                return -303;
            }
        } else if matches(t, &ORIGINAL) {
            live = t;
        } else {
            return -303;
        }
    }
    for i in [1usize, 2, 3, 5] {
        write_volatile((table + i as u32 * 4) as *mut u32, replacement[i]);
        if live != 0 {
            write_volatile((live + i as u32 * 4) as *mut u32, replacement[i]);
        }
    }
    INSTALLED = true;
    if live != 0 {
        let f: unsafe extern "C" fn(u32) = transmute(0x0ca5bcc9usize);
        f(list);
    }
    0
}
pub(crate) unsafe fn tick() {
    if APP_REGISTERED == 0 {
        return;
    }
    if !CHECKED {
        CHECKED = true;
        let fd = fw_api::open(
            b"/data/chaos/settings.cfg\0".as_ptr(),
            fw_api::oflag::RDONLY,
            0,
        );
        if fd >= 0 {
            let mut v = 0u8;
            if fw_api::read(fd, &mut v, 1) == 1 && v == 1 {
                REQUEST = true;
            }
            fw_api::close(fd);
        }
    }
    if REQUEST {
        let result = install();
        if result != -303 && result != -102 {
            REQUEST = false;
        }
        fops_wr(FOPS_DBG1, result as u32);
    }
}
