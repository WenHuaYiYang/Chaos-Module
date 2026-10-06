//! 空消息图标去黑底；电池蒙版采样静态背景并保留裁切。
use crate::fw_api;
use crate::mem::{rd8, rd16, rd32, safe_ptr};
use crate::{st_rd, st_wr};

pub(crate) const EMPTY_PATH: &[u8] = b"/resource/app/notifications/no_message.bin\0";
pub(crate) const BATTERY_PATH: &[u8] = b"/resource/app/common/icon/charge_cont.bin\0";
static mut EMPTY_IMAGE: u32 = 0;
static mut ATTEMPTED: u32 = 0;
static mut BATTERY_TEMPLATE: u32 = 0;
static mut BATTERY_ATTEMPTED: u32 = 0;
static mut BATTERY_IMAGE: u32 = 0;
static mut BATTERY_BACKGROUND: u32 = 0;
static mut BATTERY_POSITION: (i32, i32) = (-1, -1);

pub(crate) unsafe fn empty_image() -> u32 {
    if st_rd!(ATTEMPTED) != 0 { return st_rd!(EMPTY_IMAGE); }
    st_wr!(ATTEMPTED, 1);
    let image = load(EMPTY_PATH, 96, 96, true);
    st_wr!(EMPTY_IMAGE, image);
    image
}

pub(crate) unsafe fn reset_battery() { st_wr!(BATTERY_BACKGROUND, 0); }

pub(crate) unsafe fn battery_mask(background: u32, root: u32, battery: u32) -> u32 {
    if !safe_ptr(background) || rd8((background + 1) as *const u8) != 15
        || rd16((background + 4) as *const u16) != 336
        || rd16((background + 6) as *const u16) != 480
        || rd16((background + 8) as *const u16) != 1008 { return 0; }
    let data = rd32((background + 16) as *const u32);
    if !safe_ptr(data) { return 0; }
    let x = rd32((battery + 0x1C) as *const u32) as i32
        - rd32((root + 0x1C) as *const u32) as i32
        - fw_api::fw_style_get_prop(battery, 0, 0x68) as i32 - 2;
    let y = rd32((battery + 0x20) as *const u32) as i32
        - rd32((root + 0x20) as *const u32) as i32
        - fw_api::fw_style_get_prop(battery, 0, 0x69) as i32 - 1;
    if x < 0 || y < 0 || x + 64 > 336 || y + 36 > 480 { return 0; }
    if st_rd!(BATTERY_BACKGROUND) == background && st_rd!(BATTERY_POSITION) == (x, y) {
        return st_rd!(BATTERY_IMAGE);
    }
    if st_rd!(BATTERY_ATTEMPTED) == 0 {
        st_wr!(BATTERY_ATTEMPTED, 1);
        st_wr!(BATTERY_TEMPLATE, load(BATTERY_PATH, 64, 36, false));
    }
    let template = st_rd!(BATTERY_TEMPLATE);
    if template == 0 { return 0; }
    let mut image = st_rd!(BATTERY_IMAGE);
    if image == 0 {
        image = fw_api::heap_malloc(28 + 9216 + 63);
        if image == 0 { return 0; }
        st_wr!(BATTERY_IMAGE, image);
    } else { fw_api::fw_img_free_by_path(image); }
    let output = (image + 28 + 63) & !63;
    let palette = rd32((template + 16) as *const u32);
    for row in 0..36u32 {
        for column in 0..64u32 {
            let index = rd8((palette + 1024 + row * 64 + column) as *const u8) as u32;
            let color = palette + index * 4;
            let black = (0..3).all(|channel| rd8((color + channel) as *const u8) == 0);
            let source = if black { data + (y as u32 + row) * 1008 + (x as u32 + column) * 3 }
                         else { color };
            let target = output + (row * 64 + column) * 4;
            for channel in 0..3 {
                core::ptr::write_volatile((target + channel) as *mut u8,
                    rd8((source + channel) as *const u8));
            }
            core::ptr::write_volatile((target + 3) as *mut u8, rd8((color + 3) as *const u8));
        }
    }
    core::ptr::write_bytes(image as *mut u8, 0, 28);
    core::ptr::copy_nonoverlapping(
        [25u8, 16, 0, 0, 64, 0, 36, 0, 0, 1, 0, 0].as_ptr(), image as *mut u8, 12);
    core::ptr::write_volatile((image + 12) as *mut u32, 9216);
    core::ptr::write_volatile((image + 16) as *mut u32, output);
    st_wr!(BATTERY_BACKGROUND, background);
    st_wr!(BATTERY_POSITION, (x, y));
    image
}

unsafe fn load(path: &[u8], width: u8, height: u8, remove_black: bool) -> u32 {
    let payload_bytes = 1024 + width as u32 * height as u32;
    let encoded_bytes = 12 + payload_bytes;
    let fd = fw_api::open(path.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 { return 0; }
    if fw_api::lseek(fd, 0, fw_api::seek::END) != encoded_bytes as i64
        || fw_api::lseek(fd, 0, fw_api::seek::SET) != 0 {
        fw_api::close(fd);
        return 0;
    }
    let owner = fw_api::heap_malloc(28 + payload_bytes + 63);
    if owner == 0 { fw_api::close(fd); return 0; }
    let mut position = 0;
    for _ in 0..3 {
        if position == encoded_bytes { break; }
        let budget = (encoded_bytes - position).min(4096);
        let count = fw_api::read(fd, (owner + 16 + position) as *mut u8, budget);
        if count <= 0 || count as u32 > budget { break; }
        position += count as u32;
    }
    fw_api::close(fd);
    let header = [25, 10, 0, 0, width, 0, height, 0, width, 0, 0, 0];
    if position != encoded_bytes || (0..12).any(|i|
        rd8((owner + 16 + i as u32) as *const u8) != header[i]) {
        fw_api::fw_free(owner);
        return 0;
    }
    let data = (owner + 28 + 63) & !63;
    core::ptr::copy((owner + 28) as *const u8, data as *mut u8, payload_bytes as usize);
    if remove_black {
        for index in 0..256 {
            let color = data + index * 4;
            if (0..3).all(|channel| rd8((color + channel) as *const u8) == 0) {
                core::ptr::write_volatile((color + 3) as *mut u8, 0);
            }
        }
    }
    core::ptr::write_bytes(owner as *mut u8, 0, 28);
    core::ptr::copy_nonoverlapping(header.as_ptr(), owner as *mut u8, 12);
    core::ptr::write_volatile((owner + 12) as *mut u32, payload_bytes);
    core::ptr::write_volatile((owner + 16) as *mut u32, data);
    owner
}
