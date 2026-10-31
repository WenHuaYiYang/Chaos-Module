//! 设置 → 我的设备 → 系统更新：可选 Chaos 入口（3.101.043）。
//!
//! 包装系统更新页的 on_create：先让固件创建原页面，再向它的内容容器
//! 追加一行标准列表项。页面描述符与原始创建函数来自研究资料，创建函数
//! 内部调用 content_create 的位置已对 AP 镜像做静态核对。

use crate::*;

const DESCRIPTOR: u32 = 0x200D_3EF4;
const PAGE_KEY: u32 = 0x0016_0036;
const CREATE_SLOT: u32 = DESCRIPTOR + 0x4C;
const ORIGINAL_CREATE: u32 = 0x0C57_0C41;
const CONFIG_PATH: &[u8] = b"/data/chaos/system_update.cfg\0";

static mut INSTALLED: bool = false;
static mut CHECKED: bool = false;
static mut REQUEST: bool = false;

pub(crate) unsafe fn request() {
    REQUEST = true;
}

unsafe extern "C" fn click(event: u32) {
    if event != 0 && fw_api::event_get_code(event) == fw_api::ev::CLICKED
        && APP_REGISTERED != 0
    {
        fw_api::page_goto(APP_ID << 16, 0);
    }
}

unsafe extern "C" fn create(page: u32, root: u32, start: u32) -> i32 {
    let original: unsafe extern "C" fn(u32, u32, u32) -> i32 =
        core::mem::transmute(ORIGINAL_CREATE as usize);
    let result = original(page, root, start);
    if !safe_ptr(root) {
        return result;
    }

    // 原创建函数先建标题，再建内容容器；最后一个直接子对象即 content。
    // 不再额外创建 content，避免破坏固件自身的更新状态控件布局。
    let count = fw_api::obj_child_count(root);
    if count <= 0 || count > 64 {
        return result;
    }
    let content = fw_api::obj_get_child(root, count as u32 - 1);
    if !safe_ptr(content) {
        return result;
    }
    let previous_count = fw_api::obj_child_count(content);
    if previous_count <= 0 || previous_count > 64 {
        return result;
    }
    let previous = fw_api::obj_get_child(content, previous_count as u32 - 1);
    if !safe_ptr(previous) {
        return result;
    }
    let row = fw_api::row_create(
        content,
        DISPLAY_NAME.as_ptr(),
        core::ptr::null(),
        fw_api::trailing::FORWARD,
    );
    if safe_ptr(row) {
        // 原状态文本宽 292、左边距 38，其中心相对 336 宽内容区右移 16。
        // 纵轴仍紧跟文本，横轴补偿 16，避免按钮跟着文本中心一起偏右。
        fw_api::obj_align_to(row, previous, fw_api::align::OUT_BOTTOM_MID, -16, 8);
        fw_api::obj_add_event(row, click as *const () as u32, fw_api::ev::CLICKED, 0);
    }
    result
}

unsafe fn install() -> i32 {
    if APP_REGISTERED == 0 {
        return -102;
    }
    if read_volatile((DESCRIPTOR + 0x14) as *const u32) != PAGE_KEY {
        return -301;
    }
    let replacement = create as *const () as u32 | 1;
    let current = read_volatile(CREATE_SLOT as *const u32);
    if current == replacement {
        INSTALLED = true;
        return 0;
    }
    // 只接受已核对的原始槽值；其他模块已接管或固件不匹配时保持原样。
    if INSTALLED || current != ORIGINAL_CREATE {
        return -301;
    }
    write_volatile(CREATE_SLOT as *mut u32, replacement);
    INSTALLED = read_volatile(CREATE_SLOT as *const u32) == replacement;
    if INSTALLED { 0 } else { -301 }
}

pub(crate) unsafe fn tick() {
    if APP_REGISTERED == 0 {
        return;
    }
    if !CHECKED {
        CHECKED = true;
        let fd = fw_api::open(CONFIG_PATH.as_ptr(), fw_api::oflag::RDONLY, 0);
        if fd >= 0 {
            let mut value = 0u8;
            if fw_api::read(fd, &mut value, 1) == 1 && value == 1 {
                REQUEST = true;
            }
            fw_api::close(fd);
        }
    }
    if REQUEST {
        let _ = install();
        REQUEST = false;
    }
}
