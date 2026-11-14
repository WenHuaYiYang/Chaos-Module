//! chaos_sup 完整 supervisor（/dev/chaos + 1CHS + 软浮点 + register_driver + cmd_install）
//! 关键：register_app 内部 malloc/memset 是 thunk→段B(LVGL)，需 fops.write 上下文。

#![no_std]
#![no_main]
#![allow(static_mut_refs)]

mod state;
pub(crate) use state::*;

mod font_apply;
mod beautify_restore;
mod font_list;
mod font_hot;
mod icon_apply;
mod res_hook;
mod time_set;
mod desktop_scale;
mod honeycomb_layout;
mod honeycomb;
mod settings_entry;
mod system_update_entry;
mod launcher_entry;
mod system_version;
mod system_text;
mod device_name;
mod device_model;
mod keyboard;
mod nsh;
mod terminal_ui;
mod appmgr;
mod log;
mod cacheclean;
mod confirm_pop;
#[allow(dead_code)]
mod background_pixels;
#[allow(dead_code)]
mod background_scene;
mod background_home;
mod background_pages;
mod background_surfaces;
mod background_assets;
mod blur_zone;
mod corona_blur;
mod notification_icon;
mod web_nsh;
mod btpan;

mod text;
mod watchface;
mod page;
mod ui;
mod ipc;
mod bridge_cfg;
mod screen_rotation;
mod touch_palm;
mod dynamic_icon;
mod dynamic_icon_frames;
mod icon_pack_animation;
mod capture_format;
mod capture_apng;
mod capture_settings;
mod capture;
mod debug_info;

/// Called by the C bridge only for one-shot state transitions, never per
/// frame. Keeping it here means the bridge uses Chaos' log file and the
/// "运行日志" page instead of a second logging path.
#[no_mangle]
pub extern "C" fn chaos_bridge_log(message: *const u8) {
    if message.is_null() {
        return;
    }
    unsafe {
        let mut n = 0usize;
        while n < 160 && *message.add(n) != 0 {
            n += 1;
        }
        if let Ok(text) = core::str::from_utf8(core::slice::from_raw_parts(message, n)) {
            crate::log::line(text);
        }
    }
}
pub(crate) use text::*;
pub(crate) use watchface::*;
pub(crate) use page::*;
pub(crate) use ui::*;
pub(crate) use ipc::*;

// 已验证固件 API 封装层(新界面一律走这里, 不再写裸 transmute)
mod fw_api;
#[macro_use]
mod mem;
pub(crate) use mem::{plausible_ptr, safe_ptr};

use core::ptr::{read_volatile, write_volatile};



type FnLookup = unsafe extern "C" fn(u32) -> *mut u8;
type FnInstall = unsafe extern "C" fn(*const u32, *const u32, u32) -> i32;
type FnInitBuffer = unsafe extern "C" fn(u32) -> *mut u8;























































// 行事件回调只有一个(chaos_row_any, 在 ui.rs): 行号注册时写进 obj_add_event 的
// user_data, 派发时从 event_user_data 读回 —— 新增行不再需要在这里补回调。
// 旧的一行一回调(chaos_row_ev0..75)已随该方案删除。

// 模块加载即执行的构造(instream: Vela 的 insmod 会对 init_array 调一次):
// 铺 fops 四件套(open/close/read/write) + 版本字, 然后向固件注册设备节点。
// register_driver 的四参(path, fops, mode, priv)是这套内核对**所有**设备驱动
// 的通行入口约定, 0x1B6 = 0666 权限位 —— 固件自带模块全部这么传。
#[no_mangle]
unsafe extern "C" fn chaos_ctor() {
    if st_rd!(CTOR_DONE) != 0 {
        return;
    }
    st_wr!(CTOR_DONE, 1);

    let fp = core::ptr::addr_of_mut!(FOPS) as *mut u32;
    write_volatile(fp, chaos_open as *const () as u32);
    write_volatile(fp.add(1), chaos_close as *const () as u32);
    write_volatile(fp.add(2), chaos_read as *const () as u32);
    write_volatile(fp.add(3), chaos_write as *const () as u32);
    fops_wr(FOPS_VER, STAT_VERSION);
    let register: unsafe extern "C" fn(*const u8, *const u8, u32, u32) -> u32 =
        core::mem::transmute(FW_REGISTER_DRIVER as usize);
    let registered = register(
        core::ptr::addr_of!(DEV_PATH) as *const u8,
        fp as *const u8,
        0x1B6,
        0,
    );
    // 返回 0 才算注册成功, 与固件自带模块的"已注册"标志同一套语义
    if registered == 0 {
        st_wr!(DRIVER_ON, 1);
        // 注入完成后立即启动模块维护定时器。此前定时器只在 Chaos 页面
        // on_resume 时创建，导致已保存的背景/透明模式要打开一次 Chaos 才生效。
        // 定时器回调会等待固件桌面对象树就绪，再消费 background.cfg 并刷新缓存；
        // shake_arm 本身幂等，页面首次打开时不会重复创建定时器。
        // 这里只建立延后执行的维护定时器；真正的订阅、hook 和设置应用
        // 由定时器在启动窗口结束后统一开始。
        // 模拟器安全模式也可以在 insmod 之前用文件打开: /data/chaos/simlite.cfg
        // 存在且首字节非 0 就开。走文件是因为**时间窗**: watchface::BOOT_DELAY 只有
        // 50 拍 × 50ms = 2.5 秒, 命令帧(0x51)要走 NSH 往返, 往往赶不上第一拍;
        // 文件则能在 insmod 之前就摆好, ctor 里直接读。
        {
            const SIMLITE_PATH: &[u8] = b"/data/chaos/simlite.cfg\0";
            let fd = fw_api::open(SIMLITE_PATH.as_ptr(), fw_api::oflag::RDONLY, 0);
            if fd >= 0 {
                let mut v = 0u8;
                if fw_api::read(fd, &mut v, 1) == 1 && v != 0 {
                    st_wr!(SIM_LITE, 1);
                }
                fw_api::close(fd);
            }
        }
        crate::watchface::maintenance_arm();
        // 定时器句柄: 0 说明 timer_create 没成, 那样 chaos_shake_timer 永远不会被调,
        // 依赖它的一切(页面 tick / desktop_scale)都不会跑。
        crate::log::kv("timer h ", st_rd!(SHAKE_TIMER));
        crate::log::line("模块加载: /dev/chaos 注册成功");
        // The bridge is armed from chaos_shake_timer, not here: the owner
        // loop / LVGL timer source is not ready while insmod is running.
    }
}

// 析构必须幂等: Vela 的 insmod 会对 fini_array 立即调一次, 卸载时再调一次。
// 只有真正注册过(DRIVER_ON==1)才做反注册并清标志。
#[no_mangle]
unsafe extern "C" fn chaos_dtor() {
    if st_rd!(DRIVER_ON) != 1 {
        return;
    }
    let unregister: unsafe extern "C" fn(*const u8) -> i32 =
        core::mem::transmute(FW_UNREGISTER_DRIVER as usize);
    unregister(core::ptr::addr_of!(DEV_PATH) as *const u8);
    st_wr!(DRIVER_ON, 0);
}



#[no_mangle]
pub extern "C" fn module_main() -> i32 {
    unsafe { chaos_ctor(); }
    0
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
