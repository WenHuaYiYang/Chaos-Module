//! 主界面可写回调槽与背景业务根的存活检查。
use core::mem::transmute;
use crate::mem::{rd8, rd32, safe_ptr};

pub const BACKGROUND_HOME_DESC: u32 = 0x200D_0948;
pub const BACKGROUND_HOME_SLOTS: [u32; 4] = [0x34, 0x4C, 0x50, 0x64];
pub const BACKGROUND_HOME_ORIGINALS: [u32; 4] =
    [0x0C50_FCED, 0x0C50_FB65, 0x0C51_00A5, 0x0C50_FA75];

/// 通知聚合页与详情页的固件描述符。两项来自 043 镜像的页面表，
/// 描述符由启动复制到可写页表区，页面创建时会把真实根写入 +0x30。
pub const BACKGROUND_NOTIFY_DESCS: [u32; 2] = [0x200D_10C0, 0x200D_1134];
pub const BACKGROUND_NOTIFY_KEYS: [u32; 2] = [0x0014_0001, 0x0014_0000];
pub const BACKGROUND_NOTIFY_CREATE_ORIGINALS: [u32; 2] = [0x0C52_5AFD, 0x0C52_613D];
pub const BACKGROUND_NOTIFY_CREATE_SLOT: u32 = 0x4C;

/// 登记时用复合键确认目标，避免改写其他应用。
pub unsafe fn background_home_matches() -> bool {
    rd32((BACKGROUND_HOME_DESC + 0x14) as *const u32) == 0x0010_0000
}

/// 三处业务根必须仍挂在本次主界面的独立内容层中。
pub unsafe fn background_home_roots() -> Option<[u32; 3]> {
    if !background_home_matches() { return None; }
    let root = rd32((BACKGROUND_HOME_DESC + 0x30) as *const u32);
    let state = rd8((BACKGROUND_HOME_DESC + 0x28) as *const u8);
    if !safe_ptr(root) || state == 4
        || rd8((BACKGROUND_HOME_DESC + 0x2A) as *const u8) != 2
        || rd32((BACKGROUND_HOME_DESC + 0x24) as *const u32) != 0 { return None; }
    let fixed = [rd32(0x2011_02F4 as *const u32), rd32(0x2010_FE50 as *const u32),
                 rd32(0x2011_00E0 as *const u32)];
    let records = [0x2011_0154u32, 0x2011_0140, 0x2011_012C];
    let mut roots = [0u32; 3];
    for index in 0..3 {
        let record = records[index];
        let content = rd32((record + 12) as *const u32);
        let panel = rd32(record as *const u32);
        let mut object = fixed[index];
        if !business_root_matches(object, content, panel, root) {
            object = find_business_root(content, panel, root);
        }
        if !business_root_matches(object, content, panel, root) { return None; }
        roots[index] = object;
    }
    Some(roots)
}

unsafe fn business_root_matches(object: u32, content: u32, panel: u32, root: u32) -> bool {
    if !safe_ptr(object) || rd32(object as *const u32) != 0x2CDB_A8B4
        || !safe_ptr(content) || !safe_ptr(panel)
        || rd32((object + 4) as *const u32) != content
        || rd32((content + 4) as *const u32) != panel { return false; }
    // 方向容器逐层嵌套，沿有限父链确认仍属于本次主界面。
    let mut ancestor = panel;
    for _ in 0..5 {
        if ancestor == root { return true; }
        if !safe_ptr(ancestor) || rd32(ancestor as *const u32) != 0x2CCE_0308 {
            return false;
        }
        ancestor = rd32((ancestor + 4) as *const u32);
    }
    ancestor == root
}

unsafe fn find_business_root(content: u32, panel: u32, root: u32) -> u32 {
    if !safe_ptr(content) || !safe_ptr(panel) { return 0; }
    let count = super::obj_child_count(content);
    if count <= 0 || count > 16 { return 0; }
    for index in 0..count as u32 {
        let object = super::obj_get_child(content, index);
        if business_root_matches(object, content, panel, root) { return object; }
    }
    0
}

/// 原回调来自已核对的可写槽，保留全部寄存器参数及返回值。
pub unsafe fn background_invoke(callback: u32, a: u32, b: u32, c: u32, d: u32) -> u32 {
    if callback & 1 == 0 { return 0; }
    let f: unsafe extern "C" fn(u32, u32, u32, u32) -> u32 = transmute(callback as usize);
    f(a, b, c, d)
}

/// 页面 on_create 的三参调用约定：page descriptor、固件传入的 root、启动参数。
pub unsafe fn background_page_invoke(callback: u32, page: u32, root: u32, start: u32) -> i32 {
    if callback & 1 == 0 { return 0; }
    let f: unsafe extern "C" fn(u32, u32, u32) -> i32 = transmute(callback as usize);
    f(page, root, start)
}

pub unsafe fn background_watchface_identity() -> u32 { rd32(0x2011_9770 as *const u32) }

/// 备份 scan_children 中的通知全屏缓存重捕获链。
pub unsafe fn background_notification_cache_refresh(root: u32, image: u32) {
    let buffer = rd32((image + 0x34) as *const u32);
    if !safe_ptr(buffer) || rd8(buffer as *const u8) != 25
        || rd8((buffer + 1) as *const u8) != 15
        || crate::mem::rd16((buffer + 4) as *const u16) != 336
        || crate::mem::rd16((buffer + 6) as *const u16) != 480
        || crate::mem::rd16((buffer + 8) as *const u16) != 1008
        || rd32((buffer + 12) as *const u32) < 483840
        || rd32((buffer + 12) as *const u32) > 1048576
        || !safe_ptr(rd32((buffer + 16) as *const u32)) { return; }
    let f: unsafe extern "C" fn(u32, u32, u32) -> u32 = transmute(0x0CA5_FB41usize);
    if f(root, 15, buffer) == 1 {
        super::fw_img_free_by_path(buffer);
        super::background_image_set_source(image, buffer);
    }
}

/// 已验证的原图像透明度入口；只改变底层图像自身。
pub unsafe fn background_surface_set_prop(object: u32, prop: u32, value: u32) {
    if prop == 0x1D { super::background_set_opa(object, value); }
    else if prop == 0x44 {
        let f: unsafe extern "C" fn(u32, u32, u32) = transmute(0x0C58_7D89usize);
        f(object, value, 0);
    }
}

/// 消息根独立检查，缺失时不阻挡其他主界面页面。
pub unsafe fn background_home_targets() -> Option<[u32; 4]> {
    let roots = background_home_roots()?;
    let mut targets = [roots[0], roots[1], roots[2], 0];
    let content = rd32(0x2011_0174 as *const u32);
    let panel = rd32(0x2011_0168 as *const u32);
    let root = rd32((BACKGROUND_HOME_DESC + 0x30) as *const u32);
    let fixed = rd32(0x2011_0EF4 as *const u32);
    if business_root_matches(fixed, content, panel, root) {
        targets[3] = fixed;
    } else {
        targets[3] = find_business_root(content, panel, root);
    }
    Some(targets)
}

/// 刷新已有转场缓冲，不改变原快照、图像及隐藏态的所有权。
pub unsafe fn background_home_refresh_snapshots(roots: [u32; 4]) -> u32 {
    background_home_refresh_dirty_snapshots(roots, 0xE)
}

/// 仅刷新本次确实改变的页面，其他原缓冲继续复用。
pub unsafe fn background_home_refresh_dirty_snapshots(roots: [u32; 4], dirty: u32) -> u32 {
    if background_home_targets() != Some(roots) { return 0; }
    let mut changed = 0;
    for (slot, buffer_slot, image_slot) in [(1usize, 0x2010_FE68, 0x2010_FE64),
                                            (2usize, 0x2011_00D0, 0x2011_00CC),
                                            (3usize, 0, 0x2011_0ED4)] {
        if dirty & (1u32 << slot) == 0 { continue; }
        let root = roots[slot];
        let image = rd32(image_slot as *const u32);
        if !safe_ptr(root) || !safe_ptr(image) { continue; }
        let buffer = if buffer_slot == 0 { rd32((image + 0x34) as *const u32) }
            else { rd32(buffer_slot as *const u32) };
        if !safe_ptr(buffer) || !safe_ptr(image)
            || rd32(image as *const u32) != 0x2CCE_61EC
            || rd32((image + 4) as *const u32) != rd32((root + 4) as *const u32)
            || rd32((image + 0x34) as *const u32) != buffer
            || rd8(buffer as *const u8) != 25 || rd8((buffer + 1) as *const u8) != 15
            || crate::mem::rd16((buffer + 4) as *const u16) != 336
            || crate::mem::rd16((buffer + 6) as *const u16) != 480
            || crate::mem::rd16((buffer + 8) as *const u16) != 1008
            || rd32((buffer + 12) as *const u32) < 483840
            || rd32((buffer + 12) as *const u32) > 1048576
            || !safe_ptr(rd32((buffer + 16) as *const u32)) { continue; }
        let f: unsafe extern "C" fn(u32, u32, u32) -> u32 = transmute(0x0CA5_FB41usize);
        let marker = if slot == 3 { notification_swipe_marker(root) } else { 0 };
        let opacity = if marker != 0 { super::fw_style_get_prop(marker, 0, 0x44) } else { 0 };
        if marker != 0 { background_surface_set_prop(marker, 0x44, 0); }
        let result = f(root, 15, buffer);
        if marker != 0 { background_surface_set_prop(marker, 0x44, opacity); }
        if result == 1 {
            super::fw_img_free_by_path(buffer);
            super::background_image_set_source(image, buffer);
            changed |= 1u32 << slot;
        }
    }
    changed
}

/// 原通知缓存另带滑动提示，业务根捕获时排除同一素材。
unsafe fn notification_swipe_marker(root: u32) -> u32 {
    let count = super::obj_child_count(root);
    if count > 8 { return 0; }
    for index in 0..count {
        let image = super::obj_get_child(root, index as u32);
        if !safe_ptr(image) || rd32(image as *const u32) != 0x2CCE_61EC { continue; }
        let path = rd32((image + 0x34) as *const u32);
        if !crate::mem::plausible_ptr(path) { continue; }
        if b"/resource/app/common/icon/swipe_none.bin\0".iter().enumerate()
            .all(|(i, byte)| rd8((path + i as u32) as *const u8) == *byte) { return image; }
    }
    0
}

/// 已确认的卡片缓存失效入口，只登记重捕获，不改缓存所有权。
pub unsafe fn background_widget_cache_invalidate(object: u32) -> bool {
    if !safe_ptr(object) || rd32(object as *const u32) != 0x2CDB_FCC0 {
        return false;
    }
    let f: unsafe extern "C" fn(u32) = transmute(0x0CA4_8259usize);
    f(object);
    true
}

/// 背景图片属性使用启动后复制到 SRAM 的原样式入口。
pub unsafe fn background_card_set_prop(object: u32, prop: u32, value: u32) {
    if !safe_ptr(object) || (prop != 40 && prop != 41) { return; }
    let f: unsafe extern "C" fn(u32, u32, u32, u32) -> u32 = transmute(0x002B_D925usize);
    f(object, prop, value, 0);
}

/// 保持首个点值不变，由原入口退役图表自身的绘制缓存。
pub unsafe fn background_chart_cache_invalidate(object: u32) -> bool {
    if !safe_ptr(object) || rd32(object as *const u32) != 0x2CDB_9F10
        || rd8((object + 0xCC) as *const u8) & 1 == 0
        || crate::mem::rd16((object + 0xB2) as *const u16) == 0 { return false; }
    let series = rd32((object + 0x38) as *const u32);
    if !safe_ptr(series) { return false; }
    let points = rd32((series + 4) as *const u32);
    if !safe_ptr(points) { return false; }
    let value = rd32(points as *const u32);
    let f: unsafe extern "C" fn(u32, u32, u32, u32) = transmute(0x0CA4_E371usize);
    f(object, series, 0, value);
    true
}
