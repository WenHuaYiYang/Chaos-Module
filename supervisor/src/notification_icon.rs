//! 模糊 v2 独立的 no_message.bin 图源 owner（p67 / 3.101.043）。
//! 只用旧版 empty_image 的纯黑 palette → alpha=0 处理；不启动旧版背景 worker。
//! 所有图源写入都发生在 background_home 的亮屏、native-idle UI tick 中。

use crate::background_assets;
use crate::fw_api;
use crate::mem::{plausible_ptr, rd16, rd32, rd8, safe_ptr};
use crate::{st_rd, st_wr};

const LIMIT: usize = 128;
const CHILD_CAP: i32 = 64;
const SCAN_BUDGET: u32 = 1024;
const IMAGE_CLASS: u32 = 0x2CCE_61EC;
const DELETE: u16 = 0x24;

#[derive(Clone, Copy)]
struct Entry { object: u32, tag: u32, owned: bool }
const EMPTY: Entry = Entry { object: 0, tag: 0, owned: false };
static mut ENTRIES: [Entry; LIMIT] = [EMPTY; LIMIT];
static mut NEXT_TAG: u32 = 1;
static mut ACTIVE: bool = false;
static mut SOURCE: u32 = 0;
static mut RETRY_TICKS: u32 = 0;

unsafe fn image_source(object: u32) -> u32 { rd32((object + 0x34) as *const u32) }

unsafe fn path_is_no_message(source: u32) -> bool {
    if !plausible_ptr(source) { return false; }
    // 包含结尾 NUL，不能匹配 no_message.bin.bak；首字节不符便停止，
    // 避免把变量图像 descriptor 当作长路径读取。
    for (i, byte) in background_assets::EMPTY_PATH.iter().enumerate() {
        let Some(address) = source.checked_add(i as u32) else { return false; };
        if !plausible_ptr(address) || rd8(address as *const u8) != *byte { return false; }
    }
    true
}

/// DELETE 只清台账，不读 object、不分配、不写 UI。generation 防止旧事件
/// 清掉复用地址的新对象；STYLE_CHANGED 不是删除，不能丢掉恢复记录。
unsafe extern "C" fn changed(event: u32) {
    if event == 0 { return; }
    let current = rd32(event as *const u32);
    let target = rd32((event + 4) as *const u32);
    let code = rd16((event + 8) as *const u16) & 0x7FFF;
    if target != current || code != DELETE { return; }
    let tag = rd32((event + 12) as *const u32);
    let slot = (tag & 127) as usize;
    let entry = st_rd!(ENTRIES[slot]);
    if entry.object == target && entry.tag == tag { st_wr!(ENTRIES[slot], EMPTY); }
}

unsafe fn register(object: u32) -> Option<usize> {
    let mut free = None;
    for slot in 0..LIMIT {
        let entry = st_rd!(ENTRIES[slot]);
        if entry.object == object { return Some(slot); }
        if entry.object == 0 && free.is_none() { free = Some(slot); }
    }
    let slot = free?;
    let generation = st_rd!(NEXT_TAG);
    if generation >= 0x01FF_FFFF { return None; }
    let tag = (generation << 7) | slot as u32;
    if fw_api::obj_add_event(object, changed as *const () as u32 | 1, DELETE as u32, tag) == 0 {
        return None;
    }
    st_wr!(NEXT_TAG, generation + 1);
    st_wr!(ENTRIES[slot], Entry { object, tag, owned: false });
    Some(slot)
}

unsafe fn update_image(object: u32, wanted: bool, mutated: &mut bool) -> bool {
    if rd32(object as *const u32) != IMAGE_CLASS { return true; }
    let source = image_source(object);
    let replacement = st_rd!(SOURCE);
    if wanted {
        if !path_is_no_message(source) { return true; }
        let Some(slot) = register(object) else { return false; };
        let mut entry = st_rd!(ENTRIES[slot]);
        entry.owned = true;
        st_wr!(ENTRIES[slot], entry);
        *mutated = true;
        fw_api::background_image_set_source(object, replacement);
        // Setter 可能派发事件；DELETE 已清项时不能再解引用旧 object。
        if st_rd!(ENTRIES[slot]).tag != entry.tag { return true; }
        let applied = image_source(object) == replacement;
        entry.owned = applied;
        st_wr!(ENTRIES[slot], entry);
        return applied;
    }
    for slot in 0..LIMIT {
        let mut entry = st_rd!(ENTRIES[slot]);
        if entry.object != object || !entry.owned { continue; }
        if source == replacement {
            *mutated = true;
            fw_api::background_image_set_source(object, background_assets::EMPTY_PATH.as_ptr() as u32);
            if st_rd!(ENTRIES[slot]).tag != entry.tag { return true; }
            if image_source(object) == replacement { return false; }
        }
        // 第三方换了图源时只放弃所有权；同一活对象反复开关复用删除监听，
        // 不在固件里持续累积回调。直到 DELETE 才真正释放台账槽。
        entry.owned = false;
        st_wr!(ENTRIES[slot], entry);
        break;
    }
    true
}

/// 只沿当前可信 root 的 child 表访问活对象，绝不对台账中可能脱离树的
/// object 盲读 parent/source；缺失 root 时等待恢复或 DELETE 事件。
unsafe fn scan(object: u32, depth: u32, budget: &mut u32, wanted: bool, mutated: &mut bool) -> bool {
    if depth == 0 || *budget == 0 || !safe_ptr(object) { return false; }
    *budget -= 1;
    let mut complete = true;
    let count = fw_api::obj_child_count(object);
    if !(0..=CHILD_CAP).contains(&count) { return false; }
    for i in 0..count {
        if *budget == 0 { return false; }
        let child = fw_api::obj_get_child(object, i as u32);
        if !safe_ptr(child) { complete = false; continue; }
        complete = scan(child, depth - 1, budget, wanted, mutated) && complete;
        // 一次 setter 可能通过其它监听者删除父容器乃至整页。写后立即
        // 逐层返回，不能再读 ancestor/sibling 或开始扫描另一个旧 root。
        if *mutated { return complete; }
    }
    // 图源 setter 可能触发本对象 DELETE；在它返回后不再遍历该对象。
    update_image(object, wanted, mutated) && complete
}

unsafe fn owns_images() -> bool {
    (0..LIMIT).any(|slot| st_rd!(ENTRIES[slot]).owned)
}

pub(crate) unsafe fn pending() -> bool { st_rd!(ACTIVE) || owns_images() }

pub(crate) unsafe fn tick(wanted: bool) -> bool {
    if !wanted && !owns_images() {
        st_wr!(ACTIVE, false);
        st_wr!(RETRY_TICKS, 0);
        return true;
    }
    st_wr!(ACTIVE, true);
    if wanted && st_rd!(SOURCE) == 0 {
        if st_rd!(RETRY_TICKS) != 0 {
            st_wr!(RETRY_TICKS, st_rd!(RETRY_TICKS) - 1);
            return false;
        }
        st_wr!(SOURCE, background_assets::empty_image());
        if st_rd!(SOURCE) == 0 {
            // UI owner 为 50ms 一拍；图标读取/分配失败至少等约 1s 再试。
            st_wr!(RETRY_TICKS, 20);
            return false;
        }
    }
    let roots = notification_roots();
    let mut found = false;
    let mut complete = true;
    let mut mutated = false;
    for root in roots {
        if !safe_ptr(root) { continue; }
        found = true;
        let mut budget = SCAN_BUDGET;
        complete = scan(root, 8, &mut budget, wanted, &mut mutated) && complete;
        if mutated { break; }
    }
    if !wanted {
        if owns_images() { return false; }
        st_wr!(ACTIVE, false);
        return true;
    }
    // 每拍至多写一个图源，其余活对象下一拍重新获取 root 后处理。
    // 有界重扫也覆盖清空消息后晚建的占位图标。
    found && complete
}

unsafe fn notification_roots() -> [u32; 3] {
    // 每拍重新获取当前根：home 的通知根、通知聚合页、通知详情页。
    // 详情页不接管背景（见 background_pages），但无通知时的占位图标在详情页，
    // 必须单独覆盖，否则它会保留 no_message 的黑色背景。
    let home = fw_api::background_home_targets().map(|roots| roots[3]).unwrap_or(0);
    let page = fw_api::background_notification_page_root();
    let detail = fw_api::background_notification_detail_root();
    [home,
     if page != home { page } else { 0 },
     if detail != home && detail != page { detail } else { 0 }]
}
