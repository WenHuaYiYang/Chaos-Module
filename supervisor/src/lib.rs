//! chaos_sup 完整 supervisor（/dev/chaos + 1CHS + 软浮点 + register_driver + cmd_install）
//! 关键：register_app 内部 malloc/memset 是 thunk→段B(LVGL)，需 fops.write 上下文。

#![no_std]
#![feature(arm_target_feature)]
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
mod settings_entry;
mod system_update_entry;
mod launcher_entry;
mod system_version;
mod system_text;
mod device_name;
mod device_model;
mod keyboard;
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

mod text;
mod explorer;
mod watchface;
mod page;
mod ui;
mod ipc;
pub(crate) use text::*;
pub(crate) use explorer::*;
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























































// 与固件自带模块的 ROW_CALLBACK 宏等价: 每行独立回调, 把行号固化进回调本身。
// 新增槽位时必须同时补 `chaos_row_cb!(...)` 与 `row_ev_fn` 的分支,
// 且**禁止在 dispatch 里写 `_ => ev9` 这类 catch-all** —— 槽位布局一变就会静默错配
// (踩过的坑: slot10「返回」拿到 ev9 的回调, 被当成「更多」派发)。
macro_rules! chaos_row_cb {
    ($name:ident, $idx:expr) => {
        #[no_mangle]
        unsafe extern "C" fn $name(event: u32) -> u32 { chaos_row_dispatch($idx, event) }
    };
}
chaos_row_cb!(chaos_row_ev0, 0);
chaos_row_cb!(chaos_row_ev1, 1);
chaos_row_cb!(chaos_row_ev2, 2);
chaos_row_cb!(chaos_row_ev3, 3);
chaos_row_cb!(chaos_row_ev4, 4);
chaos_row_cb!(chaos_row_ev5, 5);
chaos_row_cb!(chaos_row_ev6, 6);
chaos_row_cb!(chaos_row_ev7, 7);
chaos_row_cb!(chaos_row_ev8, 8);
chaos_row_cb!(chaos_row_ev9, 9);
chaos_row_cb!(chaos_row_ev10, 10);   // slot10 = 返回行(表盘页/轮换页)
chaos_row_cb!(chaos_row_ev11, 11);
// 12..=51: 应用管理页的全量条目行(事件槽 = 条目下标 + 12; 0..11 是 ctx.rows
// 顶部按钮/状态行, 条目复用会被派发成点按钮 —— 2026-10-15 修的槽位冲突)。
// 这些槽位**不走 ctx.rows**(PAGES[pid].rows 只有 MAX_ROWS_PER_PAGE=12 个,
// build_row 写它会被拒), 行句柄由 appmgr.rs 自管的 APP_ROWS 持有;
// 槽号只进事件派发(chaos_row_dispatch -> AppMgrPage::on_click)。
chaos_row_cb!(chaos_row_ev12, 12);
chaos_row_cb!(chaos_row_ev13, 13);
chaos_row_cb!(chaos_row_ev14, 14);
chaos_row_cb!(chaos_row_ev15, 15);
chaos_row_cb!(chaos_row_ev16, 16);
chaos_row_cb!(chaos_row_ev17, 17);
chaos_row_cb!(chaos_row_ev18, 18);
chaos_row_cb!(chaos_row_ev19, 19);
chaos_row_cb!(chaos_row_ev20, 20);
chaos_row_cb!(chaos_row_ev21, 21);
chaos_row_cb!(chaos_row_ev22, 22);
chaos_row_cb!(chaos_row_ev23, 23);
chaos_row_cb!(chaos_row_ev24, 24);
chaos_row_cb!(chaos_row_ev25, 25);
chaos_row_cb!(chaos_row_ev26, 26);
chaos_row_cb!(chaos_row_ev27, 27);
chaos_row_cb!(chaos_row_ev28, 28);
chaos_row_cb!(chaos_row_ev29, 29);
chaos_row_cb!(chaos_row_ev30, 30);
chaos_row_cb!(chaos_row_ev31, 31);
chaos_row_cb!(chaos_row_ev32, 32);
chaos_row_cb!(chaos_row_ev33, 33);
chaos_row_cb!(chaos_row_ev34, 34);
chaos_row_cb!(chaos_row_ev35, 35);
chaos_row_cb!(chaos_row_ev36, 36);
chaos_row_cb!(chaos_row_ev37, 37);
chaos_row_cb!(chaos_row_ev38, 38);
chaos_row_cb!(chaos_row_ev39, 39);
chaos_row_cb!(chaos_row_ev40, 40);
chaos_row_cb!(chaos_row_ev41, 41);
chaos_row_cb!(chaos_row_ev42, 42);
chaos_row_cb!(chaos_row_ev43, 43);
chaos_row_cb!(chaos_row_ev44, 44);
chaos_row_cb!(chaos_row_ev45, 45);
chaos_row_cb!(chaos_row_ev46, 46);
chaos_row_cb!(chaos_row_ev47, 47);
chaos_row_cb!(chaos_row_ev48, 48);
chaos_row_cb!(chaos_row_ev49, 49);
chaos_row_cb!(chaos_row_ev50, 50);
chaos_row_cb!(chaos_row_ev51, 51);











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
        crate::watchface::maintenance_arm();
        // 定时器句柄: 0 说明 timer_create 没成, 那样 chaos_shake_timer 永远不会被调,
        // 依赖它的一切(页面 tick / desktop_scale)都不会跑。
        crate::log::kv("timer h ", st_rd!(SHAKE_TIMER));
        crate::log::line("模块加载: /dev/chaos 注册成功");
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
