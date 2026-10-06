//! 页面根使用静态背景，保留卡片和原控件样式。
use crate::fw_api;
use crate::mem::{rd8, rd16, rd32, safe_ptr, plausible_ptr};
use crate::{st_rd, st_wr};

const LIMIT: usize = 128;
const BG_IMAGE: u32 = 40;
const BG_IMAGE_OPA: u32 = 41;
const BG_OPA: u32 = 0x1D;
const BATTERY_SOURCE: u32 = 0xFFFF_FFFD;
const BATTERY_CLASS: u32 = 0x2CD1_98DC;
const IMAGE_SOURCE: u32 = 0xFFFF_FFFE;
const IMAGE_OPA: u32 = 0x44;
const OBJ_CLASS: u32 = 0x2CCE_0308;
const IMAGE_CLASS: u32 = 0x2CCE_61EC;
const BUTTON_CLASS: u32 = 0x2CDB_9D6C;

#[derive(Clone, Copy)]
struct Surface { object: u32, root: u32, prop: u32, original: u32, desired: u32, tag: u32 }
const EMPTY: Surface = Surface { object: 0, root: 0, prop: 0, original: 0, desired: 0, tag: 0 };
static mut SURFACES: [Surface; LIMIT] = [EMPTY; LIMIT];
static mut EVENTS: [(u32, u32, u32); LIMIT] = [(0, 0, 0); LIMIT];
static mut EVENT_MASK: u128 = 0;
static mut ACTIVE: u32 = 0;
static mut NEXT_TAG: u32 = 1;
static mut WORK: u128 = 0;
static mut SCAN: u32 = 0;
static mut BACKGROUND: u32 = 0;
static mut PAGE_ROOTS: [u32; 2] = [0; 2];
static mut PAGE_IMAGES: [u32; 2] = [0; 2];
static mut PAGE_WATCHED: [u32; 2] = [0; 2];
static mut PAGE_SCAN: u32 = 0;

pub(crate) unsafe fn set_background(descriptor: u32) { st_wr!(BACKGROUND, descriptor); }

pub(crate) unsafe fn rescan() {
    if st_rd!(ACTIVE) != 0 { st_wr!(SCAN, 1); }
}

unsafe extern "C" fn page_changed(event: u32) {
    if event == 0 { return; }
    let target = rd32(event as *const u32);
    let current = rd32((event + 4) as *const u32);
    let code = rd16((event + 8) as *const u16) & 0x7FFF;
    for slot in 0..2 {
        let root = st_rd!(PAGE_ROOTS[slot]);
        if root == 0 || (target != root && current != root) { continue; }
        if code == 0x24 && target == current {
            st_wr!(PAGE_ROOTS[slot], 0);
            st_wr!(PAGE_WATCHED[slot], 0);
        } else if code == 0x24 || code == 0x26 || code == 0x27 || code == 0x2D {
            st_wr!(PAGE_SCAN, 1);
        }
    }
}

/// 页面生命周期回调在原始 create 返回后登记真实根；后续卡片/清除按钮晚建时只置位。
pub(crate) unsafe fn set_page_root(slot: usize, root: u32) {
    if slot >= 2 || !safe_ptr(root) { return; }
    let previous = st_rd!(PAGE_ROOTS[slot]);
    if previous != 0 && previous != root {
        let image = st_rd!(PAGE_IMAGES[slot]);
        if image != 0 && fw_api::obj_is_child_of(previous, image) {
            fw_api::obj_delete(image);
        }
        st_wr!(PAGE_IMAGES[slot], 0);
    }
    if st_rd!(PAGE_ROOTS[slot]) != root {
        st_wr!(PAGE_ROOTS[slot], root);
        st_wr!(PAGE_SCAN, 1);
    }
    if st_rd!(PAGE_WATCHED[slot]) != root {
        if fw_api::obj_add_event(root, page_changed as *const () as u32, 0, root) != 0 {
            st_wr!(PAGE_WATCHED[slot], root);
        }
    }
}

pub(crate) unsafe fn clear_page_roots() {
    for slot in 0..2 {
        let root = st_rd!(PAGE_ROOTS[slot]);
        let image = st_rd!(PAGE_IMAGES[slot]);
        if root != 0 && image != 0 && fw_api::obj_is_child_of(root, image) {
            fw_api::obj_delete(image);
        }
    }
    st_wr!(PAGE_IMAGES, [0; 2]);
    st_wr!(PAGE_ROOTS, [0; 2]);
    st_wr!(PAGE_WATCHED, [0; 2]);
    st_wr!(PAGE_SCAN, 0);
}

unsafe fn page_sync() {
    let scan = st_rd!(PAGE_SCAN) != 0;
    if !scan { return; }
    st_wr!(PAGE_SCAN, 0);
    for slot in 0..2 {
        let root = st_rd!(PAGE_ROOTS[slot]);
        if root == 0 { continue; }
        page_background(root);
        scan_children(root, root, 8);
    }
}

unsafe fn owns_root(root: u32, roots: Option<[u32; 4]>) -> bool {
    st_rd!(PAGE_ROOTS).iter().any(|page| *page != 0 && *page == root)
        || roots.map(|items| items.contains(&root)).unwrap_or(false)
}

unsafe extern "C" fn changed(event: u32) {
    if event == 0 { return; }
    let target = rd32(event as *const u32);
    let tag = rd32((event + 12) as *const u32);
    let slot = (tag & 127) as usize;
    let code = rd16((event + 8) as *const u16) & 0x7FFF;
    if slot >= LIMIT || target != rd32((event + 4) as *const u32) { return; }
    if code == 0x24 || code == 0x2D {
        let flags = if code == 0x24 { 2 } else { 1 };
        let previous = st_rd!(EVENTS[slot]);
        st_wr!(EVENTS[slot], (tag, target,
            flags | if previous.0 == tag && previous.1 == target { previous.2 } else { 0 }));
        st_wr!(EVENT_MASK, st_rd!(EVENT_MASK) | (1u128 << slot));
    }
}

unsafe fn consume() {
    let mask = st_rd!(EVENT_MASK);
    st_wr!(EVENT_MASK, 0);
    for slot in 0..LIMIT {
        if mask & (1u128 << slot) == 0 { continue; }
        let (tag, object, flags) = st_rd!(EVENTS[slot]);
        st_wr!(EVENTS[slot], (0, 0, 0));
        let entry = st_rd!(SURFACES[slot]);
        if object != 0 && tag == entry.tag && object == entry.object {
            if flags & 2 != 0 {
                st_wr!(SURFACES[slot], EMPTY);
                if st_rd!(ACTIVE) != 0 { st_wr!(SCAN, 1); }
            }
            else if st_rd!(ACTIVE) != 0 { st_wr!(WORK, st_rd!(WORK) | (1u128 << slot)); }
        }
    }
}

unsafe fn belongs(object: u32, root: u32) -> bool {
    let mut object = object;
    for _ in 0..8 {
        if object == root { return true; }
        if !safe_ptr(object) { return false; }
        object = rd32((object + 4) as *const u32);
    }
    false
}

unsafe fn value(object: u32, prop: u32) -> u32 {
    if prop == IMAGE_SOURCE || prop == BATTERY_SOURCE { rd32((object + 0x34) as *const u32) }
    else { fw_api::fw_style_get_prop(object, 0, prop) }
}

unsafe fn write(object: u32, prop: u32, value: u32) {
    if prop == IMAGE_SOURCE { fw_api::background_image_set_source(object, value); }
    else if prop == BATTERY_SOURCE {
        core::ptr::write_volatile((object + 0x34) as *mut u32, value);
    }
    else if prop == BG_IMAGE || prop == BG_IMAGE_OPA {
        fw_api::background_card_set_prop(object, prop, value);
    }
    else { fw_api::background_surface_set_prop(object, prop, value); }
}

unsafe fn original_value(object: u32, prop: u32) -> u32 {
    if prop == IMAGE_SOURCE { crate::background_assets::EMPTY_PATH.as_ptr() as u32 }
    else { value(object, prop) }
}

unsafe fn add(object: u32, root: u32, prop: u32, desired: u32) {
    if !safe_ptr(object) || !belongs(object, root) { return; }
    let mut free = LIMIT;
    for slot in 0..LIMIT {
        let mut entry = st_rd!(SURFACES[slot]);
        if entry.object == object && entry.prop == prop {
            entry.root = root;
            if st_rd!(ACTIVE) == 0 {
                entry.original = original_value(object, prop);
            }
            entry.desired = desired;
            st_wr!(SURFACES[slot], entry);
            st_wr!(WORK, st_rd!(WORK) | (1u128 << slot));
            return;
        }
        if entry.object == 0 && free == LIMIT { free = slot; }
    }
    let generation = st_rd!(NEXT_TAG);
    if free == LIMIT || generation >= 0x01FF_FFFF { return; }
    let tag = generation << 7 | free as u32;
    st_wr!(NEXT_TAG, generation + 1);
    let callback = changed as *const () as u32;
    // 一个 ALL 回调兼顾删除和样式通知，重新开启时复用登记。
    if fw_api::obj_add_event(object, callback, 0, tag) == 0 { return; }
    st_wr!(SURFACES[free], Surface { object, root, prop,
        original: original_value(object, prop), desired, tag });
    st_wr!(WORK, st_rd!(WORK) | (1u128 << free));
}

/// 整页底图；页根本身不画自己的背景，真正出像素的是挂在页根底部那张全屏图像。
unsafe fn page_background(root: u32) {
    let background = st_rd!(BACKGROUND);
    if root == 0 { return; }
    for slot in 0..2 {
        if background == 0 {
            add(root, root, BG_OPA, 0);
            continue;
        }
        if st_rd!(PAGE_ROOTS[slot]) != root { continue; }
        let mut image = st_rd!(PAGE_IMAGES[slot]);
        if image == 0 {
            image = fw_api::background_image_create(root);
            if image == 0 { continue; }
            fw_api::background_image_set_floating(image);
            fw_api::background_image_set_scale(image, 256, 256);
            fw_api::background_image_set_pivot(image, 0, 0);
            fw_api::obj_align(image, 9, 0, 0);
            fw_api::background_clear_flags(image, 0x12);
            fw_api::background_image_move_back(image);
            st_wr!(PAGE_IMAGES[slot], image);
        }
        fw_api::background_image_set_source(image, background);
    }
    if background == 0 {
        fw_api::background_set_opa(root, 0);
        scan_children(root, root, 8);
        return;
    }
    add(root, root, BG_IMAGE, background);
    add(root, root, BG_IMAGE_OPA, 255);
    // 底色透明度为零时原绘制会整块跳过背景，底图跟着一起被跳过，因此必须保持非零；
    // 底下的黑色填充被整页底图完全盖住。原值由背景所有权在关闭时还原，这里不强占台账。
    fw_api::background_set_opa(root, 255);
    let count = fw_api::obj_child_count(root);
    if count > 8 { return; }
    // 页根类的事件回调不处理绘制、也不转交基类，挂在页根上的底图永远不会出像素；
    // 底部这张图像子对象才是整页底图的来源，必须保持可见。
    for index in 0..count {
        let child = fw_api::obj_get_child(root, index as u32);
        if safe_ptr(child) && rd32(child as *const u32) == IMAGE_CLASS
            && rd32((child + 0x34) as *const u32) == background {
            add(child, root, IMAGE_OPA, 255);
        }
    }
}

unsafe fn path_is(image: u32, expected: &[u8]) -> bool {
    let path = rd32((image + 0x34) as *const u32);
    if !plausible_ptr(path) { return false; }
    for (i, value) in expected.iter().enumerate() {
        if rd8((path + i as u32) as *const u8) != *value { return false; }
    }
    true
}

unsafe fn scan_children(root: u32, object: u32, depth: u32) {
    if depth == 0 || !safe_ptr(object) { return; }
    let count = fw_api::obj_child_count(object);
    if count > 16 { return; }
    for index in 0..count {
        let child = fw_api::obj_get_child(object, index as u32);
        if !safe_ptr(child) { continue; }
        let class = rd32(child as *const u32);
        add(child, root, BG_OPA, if class == OBJ_CLASS && object == root { 150 } else { 0 });
        if class == IMAGE_CLASS {
            if rd32((child + 0x34) as *const u32) == st_rd!(BACKGROUND) {
                add(child, root, IMAGE_OPA, 255);
            } else {
                if st_rd!(PAGE_ROOTS).contains(&root) {
                    fw_api::background_notification_cache_refresh(root, child);
                }
                if path_is(child, crate::background_assets::EMPTY_PATH) {
                let source = crate::background_assets::empty_image();
                if source != 0 { add(child, root, IMAGE_SOURCE, source); }
                }
            }
        }
        scan_children(root, child, depth - 1);
    }
}

pub(crate) unsafe fn enable(roots: [u32; 4]) {
    consume();
    let control = roots[1];
    let count = fw_api::obj_child_count(control);
    if count <= 8 {
        for index in 0..count {
            let footer = fw_api::obj_get_child(control, index as u32);
            if safe_ptr(footer) && rd32(footer as *const u32) == OBJ_CLASS {
                let count = fw_api::obj_child_count(footer);
                if count <= 8 {
                    for child in 0..count {
                        let battery = fw_api::obj_get_child(footer, child as u32);
                        if safe_ptr(battery) && rd32(battery as *const u32) == BATTERY_CLASS
                            && path_is(battery, crate::background_assets::BATTERY_PATH) {
                            let source = crate::background_assets::battery_mask(st_rd!(BACKGROUND), control, battery);
                            if source != 0 { add(battery, control, BATTERY_SOURCE, source); }
                        }
                    }
                }
            }
            if safe_ptr(footer) && rd32(footer as *const u32) == OBJ_CLASS
                && fw_api::obj_child_count(footer) == 1 {
                let button = fw_api::obj_get_child(footer, 0);
                if safe_ptr(button) && rd32(button as *const u32) == BUTTON_CLASS {
                    add(footer, control, BG_OPA, 0);
                }
            }
        }
    }
    page_background(roots[2]);
    page_background(roots[3]);
    if roots[2] != 0 { scan_children(roots[2], roots[2], 8); }
    if roots[3] != 0 { scan_children(roots[3], roots[3], 8); }
    let messages = roots[3];
    if messages != 0 {
        let count = fw_api::obj_child_count(messages);
        if count <= 8 {
            for index in 0..count {
                let list = fw_api::obj_get_child(messages, index as u32);
                if safe_ptr(list) && rd32(list as *const u32) == IMAGE_CLASS
                    && path_is(list, crate::background_assets::EMPTY_PATH) {
                    let source = crate::background_assets::empty_image();
                    if source != 0 { add(list, messages, IMAGE_SOURCE, source); }
                }
            }
        }
    }
    st_wr!(SCAN, 0);
    st_wr!(ACTIVE, 1);
}

pub(crate) unsafe fn enable_transparent(roots: [u32; 4]) {
    consume();
    for root in roots {
        if root != 0 { add(root, root, BG_OPA, 0); scan_children(root, root, 8); }
    }
    st_wr!(SCAN, 0);
    st_wr!(ACTIVE, 1);
}

pub(crate) unsafe fn restore(roots: Option<[u32; 4]>) {
    consume();
    let active = st_rd!(ACTIVE);
    st_wr!(ACTIVE, 0);
    st_wr!(WORK, 0);
    st_wr!(SCAN, 0);
    if active == 0 { return; }
    if let Some(roots) = roots {
        for slot in 0..LIMIT {
            let entry = st_rd!(SURFACES[slot]);
            if entry.object != 0 && owns_root(entry.root, Some(roots)) && belongs(entry.object, entry.root) {
                if value(entry.object, entry.prop) != entry.original {
                    write(entry.object, entry.prop, entry.original);
                }
            }
        }
    }
    // 删除通知返回后再撤销旧句柄。
    consume();
}

pub(crate) unsafe fn pending() -> bool {
    if st_rd!(ACTIVE) == 0 { return false; }
    st_rd!(WORK) != 0 || st_rd!(SCAN) != 0 || st_rd!(PAGE_SCAN) != 0 || st_rd!(EVENT_MASK) != 0
}

pub(crate) unsafe fn tick(roots: Option<[u32; 4]>) -> u32 {
    consume();
    if st_rd!(ACTIVE) == 0 { return 0; }
    page_sync();
    if roots.is_none() && !st_rd!(PAGE_ROOTS).iter().any(|root| *root != 0) { return 0; }
    let mut changed = 0;
    if st_rd!(SCAN) != 0 {
        if let Some(home_roots) = roots { enable(home_roots); }
        else { st_wr!(SCAN, 0); }
    }
    let mut processed = 0;
    for slot in 0..LIMIT {
        let bit = 1u128 << slot;
        if st_rd!(WORK) & bit == 0 { continue; }
        st_wr!(WORK, st_rd!(WORK) & !bit);
        let entry = st_rd!(SURFACES[slot]);
        if entry.object != 0 && owns_root(entry.root, roots) && belongs(entry.object, entry.root)
            && value(entry.object, entry.prop) != entry.desired {
            write(entry.object, entry.prop, entry.desired);
            if let Some(slot) = roots.and_then(|items| items.iter().position(|root| *root == entry.root)) {
                changed |= 1u32 << slot;
            }
        }
        processed += 1;
        if processed == 8 { break; }
    }
    changed
}
