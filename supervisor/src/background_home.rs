//! 主界面背景生命周期；退出前撤图，结构变化由已有 UI 调度消费。
use crate::background_scene::Backdrop;
use crate::fw_api;
use crate::mem::{rd16, rd32};
use crate::{st_rd, st_wr};

static mut SCENE: Backdrop<4> = Backdrop::new();
static mut MODE: u32 = 0;
static mut REQUEST: u32 = 0;
static mut HOOKED: u32 = 0;
static mut ATTACHED: u32 = 0;
static mut SOURCE_MODE: u32 = 0;
static mut GENERATE: u32 = 0;
static mut STATUS: u32 = 0;
static mut DIRTY: u32 = 0;
static mut REFRESH: u32 = 0;
static mut DELETED: [(u32, u32); 4] = [(0, 0); 4];
static mut WATCHED_ROOT: u32 = 0;
static mut WATCHED_ROOTS: [u32; 4] = [0; 4];
static mut MUTATING: u32 = 0;

/// 子对象创建和删除会向父链冒泡；回调只登记请求。
unsafe extern "C" fn structure_changed(event: u32) {
    if event == 0 { return; }
    let current = rd32(event as *const u32);
    let mut watched = current == st_rd!(WATCHED_ROOT);
    for root in st_rd!(WATCHED_ROOTS) {
        if root != 0 && root == current { watched = true; }
    }
    if !watched { return; }
    let code = rd16((event + 8) as *const u16) & 0x7FFF;
    if code == 0x24 && current == rd32((event + 4) as *const u32) {
        if current == st_rd!(WATCHED_ROOT) { st_wr!(WATCHED_ROOT, 0); }
        let mut roots = st_rd!(WATCHED_ROOTS);
        for root in &mut roots { if *root == current { *root = 0; } }
        st_wr!(WATCHED_ROOTS, roots);
    } else if (code == 0x26 || code == 0x27)
        && st_rd!(MODE) != 0 && st_rd!(MUTATING) == 0 {
        st_wr!(REQUEST, 1);
        crate::background_surfaces::rescan();
    }
}

unsafe fn watch_structure() -> bool {
    let root = rd32((fw_api::BACKGROUND_HOME_DESC + 0x30) as *const u32);
    let Some(targets) = fw_api::background_home_targets() else { return false; };
    if root != st_rd!(WATCHED_ROOT) {
        if fw_api::obj_add_event(root, structure_changed as *const () as u32, 0, root) == 0 {
            return false;
        }
        st_wr!(WATCHED_ROOT, root);
    }
    let mut watched = st_rd!(WATCHED_ROOTS);
    for target in targets {
        if target == 0 || watched.contains(&target) { continue; }
        if fw_api::obj_add_event(target, structure_changed as *const () as u32, 0, target) == 0 {
            return false;
        }
        if let Some(slot) = watched.iter_mut().find(|value| **value == 0) {
            *slot = target;
        } else {
            return false;
        }
    }
    st_wr!(WATCHED_ROOTS, watched);
    true
}

fn callbacks() -> [u32; 4] {
    [home_signal as *const () as u32, home_create as *const () as u32,
     home_resume as *const () as u32, home_delete as *const () as u32]
}

unsafe fn install() -> bool {
    if !fw_api::background_home_matches() { return false; }
    let callbacks = callbacks();
    if st_rd!(HOOKED) != 0 {
        crate::background_pages::install();
        return (0..4).all(|i| rd32((fw_api::BACKGROUND_HOME_DESC +
            fw_api::BACKGROUND_HOME_SLOTS[i]) as *const u32) == callbacks[i]);
    }
    for i in 0..4 {
        let address = fw_api::BACKGROUND_HOME_DESC + fw_api::BACKGROUND_HOME_SLOTS[i];
        if callbacks[i] & 1 == 0
            || rd32(address as *const u32) != fw_api::BACKGROUND_HOME_ORIGINALS[i] {
            return false;
        }
    }
    for i in 0..4 {
        core::ptr::write_volatile((fw_api::BACKGROUND_HOME_DESC +
            fw_api::BACKGROUND_HOME_SLOTS[i]) as *mut u32, callbacks[i]);
    }
    st_wr!(HOOKED, 1);
    // 通知聚合页和详情页拥有独立页面生命周期；单页缺失不阻挡 home 背景。
    crate::background_pages::install();
    true
}

unsafe fn uninstall() {
    if st_rd!(HOOKED) == 0 { return; }
    let callbacks = callbacks();
    for i in 0..4 {
        let address = fw_api::BACKGROUND_HOME_DESC + fw_api::BACKGROUND_HOME_SLOTS[i];
        if rd32(address as *const u32) == callbacks[i] {
            core::ptr::write_volatile(address as *mut u32, fw_api::BACKGROUND_HOME_ORIGINALS[i]);
        }
    }
    st_wr!(HOOKED, 0);
}

/// 独立台账避免 DELETE 回调重入场景的可变引用。
unsafe extern "C" fn image_deleted(event: u32) {
    if event == 0 { return; }
    let target = rd32(event as *const u32);
    let current = rd32((event + 4) as *const u32);
    let tag = rd32((event + 12) as *const u32);
    let slot = (tag & 3) as usize;
    if target == current && rd16((event + 8) as *const u16) & 0x7FFF == 0x24 && slot < 4 {
        st_wr!(DELETED[slot], (tag, target));
    }
}

unsafe fn consume_deleted() {
    for i in 0..4 {
        let (tag, target) = st_rd!(DELETED[i]);
        st_wr!(DELETED[i], (0, 0));
        if target != 0 && SCENE.deleted(tag, target) {
            st_wr!(ATTACHED, 0);
            if st_rd!(MODE) != 0 { st_wr!(REQUEST, 1); }
        }
    }
}

unsafe fn unmount() -> bool {
    st_wr!(MUTATING, 1);
    consume_deleted();
    if let Some(parents) = fw_api::background_home_targets() { SCENE.restore_deleted(parents); }
    crate::background_surfaces::restore(fw_api::background_home_targets());
    crate::background_pages::uninstall();
    crate::background_assets::reset_battery();
    SCENE.detach();
    consume_deleted();
    st_wr!(ATTACHED, 0);
    st_wr!(MUTATING, 0);
    SCENE.unmounted()
}

unsafe fn original(index: usize, a: u32, b: u32, c: u32, d: u32) -> u32 {
    fw_api::background_invoke(fw_api::BACKGROUND_HOME_ORIGINALS[index], a, b, c, d)
}

unsafe extern "C" fn home_signal(a: u32, b: u32, c: u32, d: u32) -> u32 {
    if a == fw_api::BACKGROUND_HOME_DESC && b == 0x12 { unmount(); }
    let result = original(0, a, b, c, d);
    if a == fw_api::BACKGROUND_HOME_DESC && b == 0x12 { st_wr!(REQUEST, 1); }
    result
}

unsafe extern "C" fn home_create(a: u32, b: u32, c: u32, d: u32) -> u32 {
    if a == fw_api::BACKGROUND_HOME_DESC { unmount(); }
    let result = original(1, a, b, c, d);
    if a == fw_api::BACKGROUND_HOME_DESC { st_wr!(REQUEST, 1); }
    result
}

unsafe extern "C" fn home_resume(a: u32, b: u32, c: u32, d: u32) -> u32 {
    let result = original(2, a, b, c, d);
    if a == fw_api::BACKGROUND_HOME_DESC { st_wr!(REQUEST, 1); }
    result
}

unsafe extern "C" fn home_delete(a: u32, b: u32, c: u32, d: u32) -> u32 {
    if a == fw_api::BACKGROUND_HOME_DESC { unmount(); }
    let result = original(3, a, b, c, d);
    if a == fw_api::BACKGROUND_HOME_DESC { st_wr!(REQUEST, 1); }
    result
}

unsafe fn load_fixed() -> bool {
    const BYTES: u32 = 30252;
    let fd = fw_api::open(b"/data/chaos/background.bin\0".as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 { return false; }
    let length = fw_api::lseek(fd, 0, fw_api::seek::END);
    if length != BYTES as i64 || fw_api::lseek(fd, 0, fw_api::seek::SET) != 0 {
        fw_api::close(fd);
        return false;
    }
    let input = fw_api::heap_malloc(BYTES);
    if input == 0 { fw_api::close(fd); return false; }
    let mut position = 0;
    for _ in 0..8 {
        if position == BYTES { break; }
        let count = fw_api::read(fd, (input + position) as *mut u8, (BYTES - position).min(4096));
        if count <= 0 || count as u32 > (BYTES - position).min(4096) { break; }
        position += count as u32;
    }
    fw_api::close(fd);
    let loaded = position == BYTES && SCENE.load_fixed(
        core::slice::from_raw_parts(input as *const u8, BYTES as usize));
    fw_api::fw_free(input);
    loaded
}

pub(crate) unsafe fn request(mode: u32) {
    if mode > 3 { return; }
    st_wr!(MODE, mode);
    if mode == 2 { st_wr!(GENERATE, 1); }
    st_wr!(REQUEST, 1);
    st_wr!(DIRTY, 1);
}

/// 手动更新只登记请求，返回表盘后由已有 UI 调度捕获一次。
pub(crate) unsafe fn generate() {
    st_wr!(MODE, 1);
    st_wr!(GENERATE, 1);
    st_wr!(REQUEST, 1);
    st_wr!(DIRTY, 1);
}

pub(crate) unsafe fn mode() -> u32 { st_rd!(MODE) }

pub(crate) unsafe fn status() -> &'static [u8] {
    if st_rd!(REQUEST) != 0 && st_rd!(MODE) != 0
        && fw_api::top_app_id() != fw_api::HOME_APP_ID {
        return "返回表盘后生效\0".as_bytes();
    }
    match st_rd!(STATUS) {
        1 => "正在生成背景\0".as_bytes(),
        2 => "已生成，复用静态背景\0".as_bytes(),
        6 => "完全透明背景（实验性）\0".as_bytes(),
        3 => "固定图片背景\0".as_bytes(),
        4 => "背景不可用，保留系统背景\0".as_bytes(),
        5 => "接口不匹配，保留系统背景\0".as_bytes(),
        _ => "系统背景\0".as_bytes(),
    }
}

unsafe fn set_status(value: u32) {
    if value != st_rd!(STATUS) { st_wr!(STATUS, value); st_wr!(DIRTY, 1); }
}

pub(crate) unsafe fn pending() -> bool {
    for i in 0..4 {
        if st_rd!(DELETED[i]).1 != 0 { return true; }
    }
    fw_api::screen_is_on() && (st_rd!(REQUEST) != 0 || SCENE.pending()
        || st_rd!(REFRESH) != 0 || crate::background_surfaces::pending())
}

pub(crate) unsafe fn tick() {
    consume_deleted();
    if !crate::font_apply::quiet() { return; }
    if fw_api::screen_is_on() && crate::background_surfaces::pending() {
        let changed = crate::background_surfaces::tick(fw_api::background_home_targets());
        if changed != 0 && st_rd!(ATTACHED) != 0 {
            st_wr!(REFRESH, st_rd!(REFRESH) | changed);
        }
    }
    if st_rd!(DIRTY) != 0 {
        st_wr!(DIRTY, 0);
        if crate::page_is_live(13) { crate::render_req(13); }
    }
    let mode = st_rd!(MODE);
    if mode == 0 {
        if st_rd!(REQUEST) != 0 && unmount() {
            if let Some(roots) = fw_api::background_home_targets() {
                fw_api::background_home_refresh_snapshots(roots);
            }
            st_wr!(REFRESH, 0);
            // 已生成的静态图保留，重新打开开关仍使用同一张。
            if SCENE.pending() {
                if !SCENE.release() { return; }
                st_wr!(SOURCE_MODE, 0);
            }
            st_wr!(REQUEST, 0);
            st_wr!(GENERATE, 0);
            uninstall();
            set_status(0);
        }
        return;
    }
    if !install() { st_wr!(REQUEST, 0); set_status(5); return; }
    if !fw_api::screen_is_on() || fw_api::top_app_id() != fw_api::HOME_APP_ID { return; }
    let Some(parents) = fw_api::background_home_targets() else { return; };
    if !watch_structure() { set_status(4); return; }
    if mode == 3 {
        if st_rd!(REQUEST) != 0 {
            if !unmount() { return; }
            st_wr!(REQUEST, 0);
            crate::background_surfaces::set_background(0);
            crate::background_surfaces::enable_transparent(parents);
            st_wr!(ATTACHED, 1);
            set_status(6);
        }
        if crate::background_surfaces::pending() { let _ = crate::background_surfaces::tick(Some(parents)); }
        return;
    }
    let generate = st_rd!(GENERATE) != 0 || st_rd!(SOURCE_MODE) != mode
        || SCENE.descriptor() == 0;
    // 只有生成需要表盘可见；已有静态图在其他主界面页面也可立即重挂。
    if generate && fw_api::watchface_is_on() == 0 { return; }
    if st_rd!(REQUEST) != 0 {
        if !generate && st_rd!(ATTACHED) != 0 && SCENE.matches(parents) {
            st_wr!(REQUEST, 0);
        } else {
            if !unmount() { return; }
            st_wr!(REQUEST, 0);
            if generate {
                if !SCENE.release() { return; }
                st_wr!(GENERATE, 0);
                st_wr!(SOURCE_MODE, mode);
                let loaded = if mode == 1 { SCENE.capture() } else { load_fixed() };
                if !loaded { set_status(4); return; }
                set_status(1);
            }
        }
    }
    if SCENE.pending() && !SCENE.step() { return; }
    if st_rd!(ATTACHED) == 0 && SCENE.descriptor() != 0 {
        st_wr!(MUTATING, 1);
        let attached = SCENE.attach(parents, image_deleted as *const () as u32);
        st_wr!(MUTATING, 0);
        consume_deleted();
        if attached {
            crate::background_surfaces::set_background(SCENE.descriptor());
            crate::background_surfaces::enable(parents);
            st_wr!(ATTACHED, 1);
            st_wr!(REFRESH, 0xE);
            set_status(if mode == 1 { 2 } else { 3 });
        } else {
            unmount();
            set_status(4);
        }
    }
    if st_rd!(REFRESH) != 0 && st_rd!(ATTACHED) != 0
        && !crate::background_surfaces::pending() {
        fw_api::background_home_refresh_dirty_snapshots(parents, st_rd!(REFRESH));
        st_wr!(REFRESH, 0);
    }
}
