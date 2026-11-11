//! Application-icon selection, saved independently of native launcher geometry.
use crate::{fw_api as fw, log};
use crate::dynamic_icon_frames::{Animation, PACKAGED};
use fw::dynamic_icon as port;

pub const ORIGINAL: u8 = 0;
pub const DYNAMIC: u8 = 1;
pub const NAMES: [&[u8]; 2] = ["原版\0".as_bytes(), "动态\0".as_bytes()];
const CONFIG: &[u8] = b"/data/chaos/app-icon.cfg\0";
const TEMP: &[u8] = b"/data/chaos/app-icon.cfg.tmp\0";

#[derive(Clone, Copy)]
pub(crate) struct Icon { pub object: u32, pub source: u32, pub original: bool, pub visible: bool }

static mut LOADED: bool = false;
static mut SELECTED: u8 = DYNAMIC;
static mut DIRTY: bool = false;
static mut PENDING: bool = true;
static mut FORCE: bool = false;
static mut SAVE_RETRY: Option<u32> = None;
static mut OWNER: u32 = 0;
static mut LAST_SOURCE: u32 = 0;
static mut LAST_FRAME: usize = usize::MAX;
static mut START: Option<u32> = None;
static mut ASSET_CHECKED: bool = false;
static mut ANIMATION: Option<Animation<'static>> = None;
static mut TIMER_WARNED: bool = false;

pub fn name(style: u8) -> &'static [u8] { NAMES.get(style as usize).copied().unwrap_or(NAMES[0]) }

unsafe fn read_config(path: &[u8]) -> Option<[u8; 5]> {
    let fd = fw::open(path.as_ptr(), fw::oflag::RDONLY, 0);
    if fd < 0 { return None; }
    let mut bytes = [0; 5]; let mut used = 0;
    for _ in 0..5 {
        if used == bytes.len() { break; }
        let n = fw::read(fd, bytes.as_mut_ptr().add(used), (bytes.len() - used) as u32);
        if n <= 0 || n as usize > bytes.len() - used { break; }
        used += n as usize;
    }
    let mut extra = 0;
    let exact = used == bytes.len() && fw::read(fd, &mut extra, 1) == 0;
    let closed = fw::close(fd) == 0;
    if exact && closed { Some(bytes) } else { None }
}

pub(crate) unsafe fn selected() -> u8 {
    if !LOADED {
        LOADED = true;
        if let Some(bytes) = read_config(CONFIG).filter(|b| &b[..4] == b"AIC2" && b[4] < NAMES.len() as u8) {
            SELECTED = bytes[4];
        } else {
            // First install or upgrade from the five-choice/switch versions:
            // select Dynamic once, then preserve future AIC2 user choices.
            SELECTED = DYNAMIC; DIRTY = true; FORCE = true;
        }
    }
    SELECTED
}

/// UI event: only select/enqueue. Disk and image mutations stay on UI timers.
pub(crate) unsafe fn select(style: u8) -> bool {
    if style as usize >= NAMES.len() { return false; }
    if selected() != style { SELECTED = style; DIRTY = true; SAVE_RETRY = None; }
    PENDING = true; FORCE = true; START = None; LAST_FRAME = usize::MAX;
    true
}

unsafe fn save(now: u32) {
    if !DIRTY || SAVE_RETRY.is_some_and(|last| now.wrapping_sub(last) < 1000) { return; }
    let bytes = [b'A', b'I', b'C', b'2', SELECTED];
    let fd = fw::open(TEMP.as_ptr(), fw::oflag::WRONLY | fw::oflag::CREAT | 0x20, 0o640);
    let mut ok = false;
    if fd >= 0 {
        let mut used = 0;
        for _ in 0..5 {
            if used == bytes.len() { break; }
            let n = fw::write(fd, bytes.as_ptr().add(used), (bytes.len() - used) as u32);
            if n <= 0 || n as usize > bytes.len() - used { break; }
            used += n as usize;
        }
        ok = fw::close(fd) == 0 && used == bytes.len();
    }
    ok = ok && read_config(TEMP) == Some(bytes);
    ok = ok && fw::capture_rename(TEMP.as_ptr(), CONFIG.as_ptr()) == 0;
    if ok { DIRTY = false; SAVE_RETRY = None; }
    else {
        fw::fs_remove(TEMP.as_ptr());
        if SAVE_RETRY.is_none() { log::line("应用图标: 选择保存失败，将重试"); }
        SAVE_RETRY = Some(now);
    }
}

unsafe fn animation() -> Option<Animation<'static>> {
    if !ASSET_CHECKED {
        ASSET_CHECKED = true; ANIMATION = Animation::parse(PACKAGED);
        if ANIMATION.is_none() { log::line("应用图标: GIF 帧资源无效"); }
    }
    ANIMATION
}

unsafe fn forget() { OWNER = 0; LAST_SOURCE = 0; LAST_FRAME = usize::MAX; START = None; }

unsafe fn drive() {
    let choice = selected();
    if !fw::screen_is_on() {
        port::animation_active(false, 20); START = None; LAST_FRAME = usize::MAX;
        return;
    }
    let active = port::launcher_active();
    if !active && !PENDING && OWNER == 0 { port::animation_active(false, 20); return; }
    let Some(icon) = port::current(crate::st_rd!(crate::state::APP_ID)) else {
        port::animation_active(false, 20); forget(); return;
    };
    if !icon.original && !port::owns(icon.source) && !FORCE {
        port::animation_active(false, 20); PENDING = false; forget(); return;
    }
    if choice == ORIGINAL || !active || !icon.visible {
        port::animation_active(false, 20);
        if !icon.original {
            // File assignment can fail on OOM or dispatch DELETE. Confirm it
            // through the next live lookup, never read the instance here.
            PENDING = true; forget(); port::restore_original(icon); return;
        }
        if PENDING { port::retire_frames(); }
        PENDING = false; FORCE = false; forget(); return;
    }
    let Some(asset) = animation() else { port::animation_active(false, 20); forget(); return; };
    if !port::animation_active(true, asset.period_ms) && !TIMER_WARNED {
        TIMER_WARNED = true; log::line("应用图标: 动画定时器不可用，使用维护节拍");
    }
    if OWNER != icon.object || icon.source != LAST_SOURCE {
        OWNER = icon.object; LAST_SOURCE = icon.source; LAST_FRAME = usize::MAX; START = None;
    }
    let now = fw::capture_now_ms(); let start = *START.get_or_insert(now);
    let frame = asset.frame_at(now.wrapping_sub(start));
    if LAST_FRAME != frame {
        if let Some(source) = port::show_frame(icon, &asset, frame) {
            LAST_FRAME = frame; LAST_SOURCE = source; PENDING = false; FORCE = false;
            // Source setter may dispatch DELETE; no instance access follows.
        }
    }
}

/// The maintenance timer loads/migrates/persists selection and handles rebuilds.
pub(crate) unsafe fn tick() {
    selected(); if DIRTY { save(fw::capture_now_ms()); } drive();
}

/// Dedicated UI timer: 20ms for the supplied GIF, 1000ms idle/asleep.
pub(crate) unsafe extern "C" fn animation_tick(_: u32) {
    drive(); crate::icon_pack_animation::animation_tick();
}

#[cfg(test)]
pub(crate) unsafe fn reset_for_test() {
    LOADED = false; SELECTED = DYNAMIC; DIRTY = false; PENDING = true; FORCE = false;
    SAVE_RETRY = None; ASSET_CHECKED = false; ANIMATION = None; TIMER_WARNED = false; forget();
}
