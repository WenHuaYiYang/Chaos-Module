//! Exact p67/.043 launcher and GIF frame-image boundary; UI owner only.
use super::{fwaddr as a, *};
use crate::{dynamic_icon::Icon, honeycomb_layout::MAX_ICONS, mem::{rd16, rd32, rd8, safe_ptr}};
use crate::dynamic_icon_frames::{Animation, WIDTH, HEIGHT, PALETTE_BYTES, PIXELS};
use core::mem::transmute;

const DATA_BYTES: usize = PALETTE_BYTES + PIXELS;
// Only two playback buffers; Original uses the installer's existing file.
// Storage lasts for the module lifetime, including deferred-delete objects.
static mut BUFFERS: [[u8; DATA_BYTES + 63]; 2] = [[0; DATA_BYTES + 63]; 2];
static mut DESCRIPTORS: [[u32; 7]; 2] = [[0; 7]; 2];
static mut READY: [bool; 2] = [false; 2];
static mut FRAME_TIMER: u32 = 0;
static mut FRAME_PERIOD: u32 = 1000;
static mut OWN_PERIOD: u32 = 0;
static mut PACK_PERIOD: u32 = 0;
#[repr(align(64))]
struct StaticPixels([u8; WIDTH * HEIGHT * 4]);
static mut STATIC_PIXELS: [StaticPixels; 2] = [StaticPixels([0; WIDTH * HEIGHT * 4]), StaticPixels([0; WIDTH * HEIGHT * 4])];
#[repr(align(64))]
struct StaticDescriptor([u32; 7]);
static mut STATIC_DESCRIPTORS: [StaticDescriptor; 2] = [StaticDescriptor([0; 7]), StaticDescriptor([0; 7])];
static mut STATIC_READY: [bool; 2] = [false; 2];
static STATIC_ASSETS: [&[u8]; 2] = [include_bytes!("../../assets/app-icons/dark-potato.bin"),
    include_bytes!("../../assets/app-icons/dark-icon.bin")];

unsafe fn static_descriptor(slot: usize) -> u32 { STATIC_DESCRIPTORS[slot].0.as_ptr() as u32 }

pub(crate) unsafe fn show_static(icon: Icon, choice: u8) -> Option<u32> {
    let asset = *STATIC_ASSETS.get(choice.checked_sub(2)? as usize)?;
    if asset.len() != 12 + WIDTH * HEIGHT * 4 { return None; }
    let slot = if icon.source == static_descriptor(0) { 1 } else { 0 };
    let source = static_descriptor(slot);
    if STATIC_READY[slot] { fw_img_free_by_path(source); }
    STATIC_PIXELS[slot].0.copy_from_slice(&asset[12..]);
    super::s541_clean_pixels(STATIC_PIXELS[slot].0.as_ptr(), (WIDTH * HEIGHT * 4) as u32);
    STATIC_DESCRIPTORS[slot].0 = [0x1019, (HEIGHT as u32) << 16 | WIDTH as u32,
        WIDTH as u32 * 4, (WIDTH * HEIGHT * 4) as u32, STATIC_PIXELS[slot].0.as_ptr() as u32, 0, 0];
    super::s541_clean_pixels(STATIC_DESCRIPTORS[slot].0.as_ptr().cast(), 64);
    STATIC_READY[slot] = true;
    publish(icon, source);
    Some(source)
}

unsafe fn descriptor(slot: usize) -> u32 { core::ptr::addr_of!(DESCRIPTORS[slot]) as u32 }

pub(crate) unsafe fn owns(source: u32) -> bool {
    (0..2).any(|slot| (READY[slot] && descriptor(slot) == source)
        || (STATIC_READY[slot] && static_descriptor(slot) == source))
}

unsafe fn prepare(slot: usize, asset: &Animation<'_>, frame: usize) -> Option<u32> {
    let source = descriptor(slot);
    // Drop only a recycled INACTIVE playback key before changing its pixels.
    if READY[slot] { fw_img_free_by_path(source); }
    let raw = core::ptr::addr_of_mut!(BUFFERS[slot]) as usize;
    let data = ((raw + 63) & !63) as *mut u8;
    let pixels = core::slice::from_raw_parts_mut(data, DATA_BYTES);
    pixels[..PALETTE_BYTES].copy_from_slice(asset.palette());
    if !asset.decode_frame(frame, &mut pixels[PALETTE_BYTES..]) { return None; }
    // lv_image_dsc_t: 12-byte header, data_size, data, two reserved words.
    DESCRIPTORS[slot] = [0x0000_0A19, (HEIGHT as u32) << 16 | WIDTH as u32,
        WIDTH as u32, DATA_BYTES as u32, data as u32, 0, 0];
    READY[slot] = true;
    Some(source)
}

unsafe fn publish(icon: Icon, source: u32) {
    background_image_set_source(icon.object, source);
    // Setter can dispatch DELETE: no instance access follows. Descriptor and
    // pixels remain valid; retire only the old owned key's decoded cache.
    if owns(icon.source) && icon.source != source { fw_img_free_by_path(icon.source); }
}

pub(crate) unsafe fn show_frame(icon: Icon, asset: &Animation<'_>, frame: usize) -> Option<u32> {
    let slot = if icon.source == descriptor(0) { 1 } else { 0 };
    let source = prepare(slot, asset, frame)?;
    publish(icon, source);
    Some(source)
}

pub(crate) unsafe fn restore_original(icon: Icon) {
    // Native setter duplicates this permanent path and can fail on OOM.
    // Leave playback cache keys alone until a later lookup confirms success.
    background_image_set_source(icon.object, crate::state::ICON_PATH.as_ptr() as u32);
}

/// Called once after a fresh live lookup confirms Original is bound.
pub(crate) unsafe fn retire_frames() {
    for slot in 0..2 {
        if READY[slot] { fw_img_free_by_path(descriptor(slot)); }
        if STATIC_READY[slot] { fw_img_free_by_path(static_descriptor(slot)); }
    }
}

/// Same audited timer layout used by the honeycomb frame scheduler.
pub(crate) unsafe fn animation_active(active: bool, period: u32) -> bool {
    OWN_PERIOD = if active { period.clamp(10,50) } else { 0 };
    update_timer()
}

pub(crate) unsafe fn pack_animation_active(active: bool, period: u32) -> bool {
    PACK_PERIOD = if active { period.clamp(10,50) } else { 0 };
    update_timer()
}

unsafe fn update_timer() -> bool {
    let period = match (OWN_PERIOD,PACK_PERIOD) { (0,0)=>1000,(0,p)|(p,0)=>p,(a,b)=>a.min(b) };
    if FRAME_TIMER == 0 {
        if period == 1000 { return true; }
        FRAME_TIMER = timer_create(crate::dynamic_icon::animation_tick as *const () as u32 | 1, 1000, 0);
        FRAME_PERIOD = 1000;
    }
    if FRAME_TIMER == 0 || timer_period_of(FRAME_TIMER) != FRAME_PERIOD { return false; }
    let want = period;
    if want != FRAME_PERIOD { timer_set_period(FRAME_TIMER, want); FRAME_PERIOD = want; }
    timer_period_of(FRAME_TIMER) == FRAME_PERIOD
}

unsafe fn original_path(source: u32) -> bool {
    for (offset, expected) in crate::state::ICON_PATH.iter().enumerate() {
        let Some(at) = source.checked_add(offset as u32) else { return false; };
        if !crate::mem::plausible_ptr(at) || rd8(at as *const u8) != *expected { return false; }
    }
    true
}

pub(crate) unsafe fn launcher_transition_active() -> bool {
    rd8(a::LAUNCHER_TRANSITION_SLOT as *const u8) != 0
        || rd32(a::LAUNCHER_SNAPSHOT_SLOT as *const u32) != 0
}

// ===== 桌面滑动检测(只读, 不碰原生状态) =====
// 与 desktop_scale 同一份地址勘误: 容器 = *0x201102EC, 滚动量 = 容器 +0x8AC(i32)。
// 那边是"写表"用的, 这里只读同一个数判断"是不是正在滑"。
const GRID_GLOBAL: u32 = 0x2011_02EC;
const GRID_SCROLL_OFF: u32 = 0x8AC;
/// 与模糊交互门(P67_BLUR_INTERACTION_BUSY)同一口径: 最后一次位移后 160ms 内算滑动。
const SCROLL_QUIET_MS: u32 = 160;
static mut SCROLL_SEEN: i32 = i32::MIN;
static mut SCROLL_AT: u32 = 0;

/// 桌面正在滑动, 或离最后一次位移不到 160ms。
///
/// 为什么要有这道门: 动态图标每帧要 lseek+read 压缩块、解码 9216 字节、memcpy
/// 调色板、再把描述符交给 GPU(还可能 free 上一帧)。这些全在 LVGL 的 UI 线程上,
/// 和滑动手势抢同一帧时间 —— 桌面一滑就掉帧。滑动期间保留定时器与周期(不重排
/// 播放相位), 只跳过解码与上传; 停稳 160ms 后自动接上。
pub(crate) unsafe fn launcher_scrolling() -> bool {
    let cont = rd32(GRID_GLOBAL as *const u32);
    if cont == 0 || !safe_ptr(cont) { return false; }
    let pos = rd32((cont + GRID_SCROLL_OFF) as *const u32) as i32;
    if pos != SCROLL_SEEN {
        SCROLL_SEEN = pos;
        SCROLL_AT = capture_now_ms();
        return true;
    }
    capture_now_ms().wrapping_sub(SCROLL_AT) < SCROLL_QUIET_MS
}

pub(crate) unsafe fn icon_pressed(icon: Icon) -> bool {
    // Press feedback starts before the launcher transition/snapshot flags.
    // This is interaction state, not visibility (PRESSED / PRESS_LOST).
    rd16((icon.object + 0x30) as *const u16) & 0x60 != 0
}

pub(crate) unsafe fn launcher_active() -> bool {
    if top_app_id() != HOME_APP_ID || watchface_is_on() != 0 { return false; }
    let grid = honeycomb::launcher();
    grid != 0 && (honeycomb::supported(grid) || honeycomb::s541_supported(grid)) && honeycomb::visible(grid)
        && rd32((grid + 0x2C) as *const u32) & 1 == 0
        && !launcher_transition_active()
}

/// Native app-id lookup matches the ORIGINAL click callback even when its
/// dispatch filter is disarmed by honeycomb. Never replace click callbacks.
/// Resolve the current child table on every call, including restoration.
pub(crate) unsafe fn current(app_id: u32) -> Option<Icon> {
    if app_id == 0 { return None; }
    let grid = honeycomb::launcher();
    if grid == 0 { return None; }
    let n = obj_child_count(grid);
    if n < 0 || n as usize > MAX_ICONS + 8 { return None; }
    let lookup: unsafe extern "C" fn(u32) -> u32 = transmute(a::LAUNCHER_ICON_APP_ID as usize);
    for index in 0..n as u32 {
        let object = obj_get_child(grid, index);
        if !safe_ptr(object) || rd32(object as *const u32) != a::IMAGE_CLASS
            || lookup(object) != app_id { continue; }
        let x1 = rd32((object + 0x1C) as *const u32) as i32;
        let y1 = rd32((object + 0x20) as *const u32) as i32;
        let x2 = rd32((object + 0x24) as *const u32) as i32;
        let y2 = rd32((object + 0x28) as *const u32) as i32;
        let source = rd32((object + 0x34) as *const u32);
        return Some(Icon {
            object,
            source,
            original: original_path(source),
            visible: rd32((object + 0x2C) as *const u32) & 1 == 0
                && x1 < 336 && y1 < 480 && x2 >= 0 && y2 >= 0,
        });
    }
    None
}
