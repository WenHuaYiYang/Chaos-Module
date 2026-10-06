//! 美化选择记录与启动恢复，沿用现有 UI 调度。

use crate::*;

const NONE: u8 = 255;
const PATHS: [&[u8]; 2] = [b"/data/chaos/beautify-a.dat\0", b"/data/chaos/beautify-b.dat\0"];
static mut LOADED: bool = false;
static mut ENABLED: bool = true;
static mut FONT: u8 = NONE;
static mut ICON: u8 = 0;
static mut GEN: u32 = 0;
static mut FONT_CHANGED: bool = false;
static mut ICON_CHANGED: bool = false;
static mut DIRTY: bool = false;
static mut TRIES: u32 = 0;
static mut TOGGLE: bool = false;
static mut RESTORED: bool = false;
static mut UI_DIRTY: bool = false;
static mut SAVE_FAILED: bool = false;

fn checksum(bytes: &[u8]) -> u32 {
    bytes.iter().fold(2166136261u32, |h, b| (h ^ *b as u32).wrapping_mul(16777619))
}

fn encode(enabled: bool, font: u8, icon: u8, gen: u32) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[..4].copy_from_slice(b"CBR1");
    b[4] = enabled as u8; b[5] = font; b[6] = icon;
    b[8..12].copy_from_slice(&gen.to_le_bytes());
    let check = checksum(&b[..12]);
    b[12..].copy_from_slice(&check.to_le_bytes());
    b
}

fn decode(b: &[u8]) -> Option<(bool, u8, u8, u32)> {
    if b.len() != 16 || &b[..4] != b"CBR1" || b[4] > 1 || b[7] != 0
        || (b[5] != NONE && b[5] > font_apply::FONT_SLOT_MAX as u8) || b[6] > icon_apply::IC_PACK_MAX as u8 { return None; }
    let word = |i| u32::from_le_bytes([b[i], b[i+1], b[i+2], b[i+3]]);
    if checksum(&b[..12]) != word(12) { return None; }
    Some((b[4] != 0, b[5], b[6], word(8)))
}

unsafe fn read_record(path: &[u8]) -> Option<(bool, u8, u8, u32)> {
    let fd = fw_api::open(path.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 { return None; }
    let mut b = [0u8; 17]; let mut n = 0usize;
    while n < b.len() {
        let got = fw_api::read(fd, b.as_mut_ptr().add(n), (b.len()-n) as u32);
        if got < 0 { fw_api::close(fd); return None; }
        if got == 0 { break; }
        if got as usize > b.len()-n { n = 17; break; }
        n += got as usize;
    }
    fw_api::close(fd);
    decode(&b[..n])
}

pub(crate) unsafe fn load() {
    if st_rd!(LOADED) { return; }
    st_wr!(LOADED, true);
    let a = read_record(PATHS[0]); let b = read_record(PATHS[1]);
    let chosen = match (a, b) {
        (Some(a), Some(b)) => Some(if b.3.wrapping_sub(a.3) as i32 > 0 { b } else { a }),
        (a, b) => a.or(b),
    };
    if let Some((enabled, font, icon, gen)) = chosen {
        st_wr!(ENABLED, enabled); st_wr!(GEN, gen);
        if !st_rd!(FONT_CHANGED) { st_wr!(FONT, font); }
        if !st_rd!(ICON_CHANGED) { st_wr!(ICON, icon); }
    }
}

unsafe fn dirty() { st_wr!(DIRTY, true); st_wr!(TRIES, 0); st_wr!(SAVE_FAILED, false); }
pub(crate) unsafe fn remember_font(slot: u32) {
    let font = if slot <= font_apply::FONT_SLOT_MAX { slot as u8 } else { NONE };
    st_wr!(FONT_CHANGED, true);
    if !st_rd!(LOADED) || font != st_rd!(FONT) { st_wr!(FONT, font); dirty(); }
}
pub(crate) unsafe fn remember_icon(pack: u32) {
    if pack > icon_apply::IC_PACK_MAX as u32 { return; }
    st_wr!(ICON_CHANGED, true);
    if !st_rd!(LOADED) || pack as u8 != st_rd!(ICON) { st_wr!(ICON, pack as u8); dirty(); }
}
pub(crate) unsafe fn enabled() -> bool { load(); st_rd!(ENABLED) }
pub(crate) unsafe fn font_auto_allowed(live: u32) -> bool {
    st_rd!(LOADED) && st_rd!(ENABLED) && st_rd!(FONT) != NONE && st_rd!(FONT) as u32 == live
}
pub(crate) unsafe fn request_toggle() { st_wr!(TOGGLE, true); }
pub(crate) unsafe fn status() -> *const u8 {
    if st_rd!(SAVE_FAILED) { "保存失败，请重新切换开关\0".as_ptr() }
    else if st_rd!(DIRTY) || st_rd!(TOGGLE) { "正在保存设置\0".as_ptr() }
    else { "记录选择，下次运行自动应用\0".as_ptr() }
}
pub(crate) unsafe fn pending() -> bool { st_rd!(DIRTY) || st_rd!(TOGGLE) }

unsafe fn save() -> bool {
    let gen = st_rd!(GEN).wrapping_add(1);
    let path = PATHS[(st_rd!(GEN) & 1) as usize];
    let data = encode(st_rd!(ENABLED), st_rd!(FONT), st_rd!(ICON), gen);
    let fd = fw_api::open(path.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd < 0 { return false; }
    let mut n = 0usize;
    while n < data.len() {
        let got = fw_api::write(fd, data.as_ptr().add(n), (data.len()-n) as u32);
        if got <= 0 || got as usize > data.len()-n { break; }
        n += got as usize;
    }
    let closed = fw_api::close(fd);
    if n != data.len() || closed < 0 || read_record(path) != decode(&data) { return false; }
    st_wr!(GEN, gen);
    true
}

pub(crate) unsafe fn tick() {
    load();
    if st_rd!(TOGGLE) {
        st_wr!(TOGGLE, false); st_wr!(ENABLED, !st_rd!(ENABLED)); dirty(); st_wr!(UI_DIRTY, true);
    }
    if st_rd!(DIRTY) && st_rd!(TRIES) < 3 {
        st_wr!(TRIES, st_rd!(TRIES) + 1);
        if save() { st_wr!(DIRTY, false); st_wr!(UI_DIRTY, true); }
        else if st_rd!(TRIES) == 3 {
            st_wr!(DIRTY, false); st_wr!(SAVE_FAILED, true); st_wr!(UI_DIRTY, true);
        }
    }
    if !st_rd!(RESTORED) {
        if !st_rd!(ENABLED) { st_wr!(RESTORED, true); }
        else if fw_api::screen_is_on() && st_rd!(APP_FG) == 0 && font_apply::quiet()
            && !icon_apply::restore_busy() {
            st_wr!(RESTORED, true);
            let font = st_rd!(FONT);
            if font != NONE && !st_rd!(FONT_CHANGED) { font_apply::restore_selection(font as u32); }
            if !st_rd!(ICON_CHANGED) { icon_apply::restore_selection(st_rd!(ICON) as u32); }
        }
    }
    if st_rd!(UI_DIRTY) && font_apply::quiet() && !icon_apply::restore_busy() && page_is_live(13) {
        st_wr!(UI_DIRTY, false); render_req(13);
    }
}
