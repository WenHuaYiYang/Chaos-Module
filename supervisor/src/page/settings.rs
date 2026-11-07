//! 页11/12/14/16/19：亮度、缓存、设置、震动与 fetch 恢复。

use crate::page::*;
use crate::page::Page;
use super::time::{time_edit_enter, TIME_PID, DATE_MD_PID};

use core::sync::atomic::{AtomicU32, Ordering};

pub(crate) struct SettingsPage;

// ===== 页11: 亮度 =====

pub(crate) struct BrightnessPage {
    pub req: AtomicU32,
    pub req_val: AtomicU32,
}

impl Page for BrightnessPage {

    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "亮度控制".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            let cur = fw_api::brightness_get_target();
            st_wr!(BRIGHT_VAL, cur);
            let auto_on = fw_api::brightness_get_auto_adjustment();
            bright_fill_status();
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::display("当前亮度\0".as_bytes(), core::ptr::addr_of!(BRIGHT_STATUS) as *const u8)),
                (1, RowSpec::clickable(b"600nit\0")),
                (2, RowSpec::clickable(b"2000nit\0")),
                (3, RowSpec::switch("自动亮度\0".as_bytes())),
            ])?;
            // 补写开关初态
            let rows = page_rows_ptr(11);
            let r3 = read_volatile(rows.add(3));
            if r3 != 0 {
                let upd = fw_api::row_update;
                upd(r3, core::ptr::null(), "自动亮度\0".as_bytes().as_ptr(),
                    core::ptr::null(), 0, auto_on as u8);
            }
            Ok(())
        }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx == 3 }

    fn on_click(&self, idx: usize) {
        match idx {
            1 => {
                self.req_val.store(600, Ordering::Relaxed);
                self.req.store(4, Ordering::Relaxed);
                unsafe {
                    st_wr!(BRIGHT_REQ_VAL, 600);
                    st_wr!(BRIGHT_REQ, 4);
                }
            }
            2 => {
                self.req_val.store(2000, Ordering::Relaxed);
                self.req.store(4, Ordering::Relaxed);
                unsafe {
                    st_wr!(BRIGHT_REQ_VAL, 2000);
                    st_wr!(BRIGHT_REQ, 4);
                }
            }
            _ => {}
        }
    }

    fn on_switch(&self, _idx: usize) {
        self.req.store(2, Ordering::Relaxed);
        unsafe { st_wr!(BRIGHT_REQ, 2); }
    }

    fn needs_tick(&self) -> bool {
        self.req.load(Ordering::Relaxed) != 0
    }

    fn tick(&self) {
        unsafe {
            let req = st_rd!(BRIGHT_REQ);
            if req == 0 { return; }
            st_wr!(BRIGHT_REQ, 0);
            if req == 1 {
                let v = st_rd!(BRIGHT_REQ_VAL);
                fw_api::brightness_set_value(v);
            } else if req == 2 {
                let cur = fw_api::brightness_get_auto_adjustment();
                let nv = if cur != 0 { 0u32 } else { 1u32 };
                // 关自动前记住当前亮度: 固件关自动时会 apply(state[+0x0C]=手动值),
                //   我们从没写过手动值 → 0 → 息屏。先记下 target, 关完立刻补上。
                let target = fw_api::brightness_get_target();
                fw_api::brightness_set_auto_adjustment(nv);
                if nv == 0 && target > 0 {
                    fw_api::brightness_apply_raw(target);
                }
            } else if req == 4 {
                let v = st_rd!(BRIGHT_REQ_VAL);
                fw_api::brightness_apply_raw(v);
            }
            // 刷新亮度页(存活门)
            if PAGES[11].built == 0 || !page_is_live(11) { return; }
            bright_fill_status();
            let rows = page_rows_ptr(11);
            let upd: unsafe extern "C" fn(u32, *const u8, *const u8, *const u8, i32, u8) -> i32 =
                fw_api::row_update;
            let r0 = read_volatile(rows.add(0));
            if r0 != 0 {
                upd(r0, core::ptr::null(), "当前亮度\0".as_bytes().as_ptr(),
                    core::ptr::addr_of!(BRIGHT_STATUS) as *const u8, 0, 0);
            }
            let auto_on = fw_api::brightness_get_auto_adjustment() as u8;
            let r3 = read_volatile(rows.add(3));
            if r3 != 0 {
                upd(r3, core::ptr::null(), "自动亮度\0".as_bytes().as_ptr(),
                    core::ptr::null(), 0, auto_on);
            }
        }
    }
}

// ===== 页12: 缓存清理(核心逻辑移植自 Shell++, 见 cacheclean.rs) =====
//
// (2026-10-15 二版) 页面加卡片: 每个根目录一张(主标签=路径, 副标签=占用),
// 与 shellpp 的清理页同形态。行0=总量状态, 行1..4=四张卡, 行5=清理按钮。
// 布局走 ctx.rows 槽 0..5(<= MAX_ROWS_PER_PAGE, 一屏装得下)。

pub(crate) struct CachePage;

impl Page for CachePage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "缓存清理".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            // 首行文本: 刚出清理结果就展示结果(消费一次), 否则现场扫描
            if st_rd!(crate::cacheclean::CC_HAS_RESULT) == 0 {
                crate::cacheclean::scan_msg();
            } else {
                st_wr!(crate::cacheclean::CC_HAS_RESULT, 0);
            }
            let msg = crate::cacheclean::msg();
            let r0 = build_row(ctx, ctx.title, 0, &RowSpec::display(msg, core::ptr::null()));
            let mut last = if r0 != 0 { r0 } else { ctx.title };
            // 四张路径卡片(不可点: 主标签=路径, 副标签=占用)
            let mut r = 1usize;
            while r <= crate::cacheclean::ROOT_N {
                let spec = RowSpec::display(crate::cacheclean::root_path(r - 1),
                                            crate::cacheclean::root_usage(r - 1).as_ptr());
                let card = build_row(ctx, last, r, &spec);
                if card != 0 { last = card; }
                r += 1;
            }
            let label: &[u8] = if st_rd!(crate::cacheclean::CC_ARM) != 0 {
                "再次点击确认清理\0".as_bytes()
            } else {
                "开始清理\0".as_bytes()
            };
            let r5 = build_row(ctx, last, 5, &RowSpec::clickable(label));
            if r5 == 0 { return Err(PageError::RowFailed(5)); }
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if idx != 5 { return; }
            if st_rd!(crate::cacheclean::CC_ARM) == 0 {
                // 第一次点: 武装确认(只改行文本, 不在事件回调做长 FS 操作)
                st_wr!(crate::cacheclean::CC_ARM, 1);
                let rows = page_rows_ptr(12);
                let r1 = read_volatile(rows.add(5));   // 行5 = 清理按钮(1..4 是路径卡片)
                if r1 != 0 {
                    fw_api::row_update(r1, fw_api::NONE_PTR,
                        "再次点击确认清理\0".as_bytes().as_ptr(), fw_api::NONE_PTR, 0, 0);
                }
            } else {
                st_wr!(crate::cacheclean::CC_REQ, 1);   // 清理由 tick 执行
            }
        }
    }

    fn needs_tick(&self) -> bool {
        unsafe { st_rd!(crate::cacheclean::CC_REQ) != 0 }
    }

    fn tick(&self) {
        unsafe {
            if st_rd!(crate::cacheclean::CC_REQ) == 0 { return; }
            st_wr!(crate::cacheclean::CC_REQ, 0);
            crate::cacheclean::clean();
            st_wr!(crate::cacheclean::CC_HAS_RESULT, 1);
            render_req(12);
        }
    }
}


// ===== 页16: 主页 -> 震动 =====
//
// 行0 = "滑动列表震动"(launcher 根 SCROLL_END + 拖拽距离过滤, 见 background_home.rs);
// 行1 = "右滑返回震动"(前台 app 切回 HOME 震一次); 行2 = "测试震动";
// 行11 = 返回。两个开关独立持久化 /data/chaos/swipe_vib.cfg, 默认全关。
pub(crate) struct VibPage;

impl Page for VibPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "震动\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::switch("滑动列表震动\0".as_bytes())),
                (1, RowSpec::switch("右滑返回震动\0".as_bytes())),
                (2, RowSpec::clickable("测试震动\0".as_bytes())),
                (11, RowSpec::clickable("返回\0".as_bytes()))])?;
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(0)),
                fw_api::NONE_PTR, fw_api::NONE_PTR, fw_api::NONE_PTR,
                0, crate::background_home::swipe_list_on() as u8);
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(1)),
                fw_api::NONE_PTR, fw_api::NONE_PTR, fw_api::NONE_PTR,
                0, crate::background_home::swipe_back_on() as u8);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if idx == 2 { crate::background_home::vib_test_req(); }
            else if idx == 11 { nav_back(); }
        }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx == 0 || idx == 1 }

    fn on_switch(&self, idx: usize) {
        unsafe {
            if idx == 0 {
                crate::background_home::set_swipe_vib(true,
                    if crate::background_home::swipe_list_on() != 0 { 0 } else { 1 });
            } else if idx == 1 {
                crate::background_home::set_swipe_vib(false,
                    if crate::background_home::swipe_back_on() != 0 { 0 } else { 1 });
            }
        }
    }
}

impl Page for SettingsPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "设置\0".as_bytes()); }
    }
    /// 页14 原来叫"关于"，只放版本与开发者。2026-10-18 重排：主页只留四个一级入口
    /// (应用管理 / 表盘管理 / 系统美化 / 设置)，常用的小开关与时间日期都收进这里，
    /// 版本信息压到底部当展示行 —— 用户要"清楚"，就不要在主页堆十个平级行。
    /// 页名字符串沿用 `about`(state/ipc 没动)：它只是 page_goto 的解析键，与标题无关。
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            let rows: &[(usize, RowSpec<'_>)] = &[
                (0, RowSpec::clickable("时间设置\0".as_bytes())),
                (1, RowSpec::clickable("日期设置\0".as_bytes())),
                (2, RowSpec::clickable("亮度\0".as_bytes())),
                (3, RowSpec::clickable("震动\0".as_bytes())),
                (4, RowSpec::clickable("缓存清理\0".as_bytes())),
                (5, RowSpec::clickable("运行日志\0".as_bytes())),
                (6, RowSpec::clickable("恢复fetch支持\0".as_bytes())),
                (7, RowSpec::clickable("系统关于页面修改\0".as_bytes())),
                (8, RowSpec::switch("文件桥\0".as_bytes())),
            ];
            build_rows(ctx, ctx.title, rows)?;
            fw_api::row_update(
                core::ptr::read_volatile(ctx.rows.add(8)),
                fw_api::NONE_PTR, fw_api::NONE_PTR, fw_api::NONE_PTR,
                0, crate::bridge_cfg::enabled() as u8,
            );
            Ok(())
        }
    }
    fn on_click(&self, idx: usize) {
        unsafe {
            match idx {
                0 => time_edit_enter(TIME_PID as u32),
                1 => time_edit_enter(DATE_MD_PID as u32),
                2 => nav_goto(11, 0),   // 亮度
                3 => nav_goto(16, 0),   // 震动
                4 => nav_goto(12, 0),   // 缓存清理
                5 => nav_goto(17, 0),   // 运行日志
                6 => nav_goto(19, 0),   // 恢复 fetch 支持
                7 => nav_goto(24, 0),   // 系统关于页面修改
                _ => {}
            }
        }
    }
    fn is_switch_row(&self, idx: usize) -> bool {
        idx == 8
    }
    fn on_switch(&self, idx: usize) {
        if idx == 8 {
            unsafe {
                let on = crate::bridge_cfg::toggle();
                crate::log::kv("文件桥开关 ", if on { 1 } else { 0 });
            }
        }
    }
}


// ===== 页19: 设置 -> 恢复 fetch 支持 =====
pub(crate) struct FetchPage;
pub(super) static PAGE_FETCHP: FetchPage = FetchPage;
static FETCH_HELP: &[u8] = "使用：\n1.前提条件：安装了autorun（如果没安装请在AstroBox上搜索或者加群1018886448获取autorun最新版本）\n2.开启方式：在Chaos的注入器表盘中点击注入设置->注册自启动，然后在autorun表盘中按照流程开启自启动，重启即可。\n3.注意事项：此功能只保证在小米运动健康中正常使用。\0".as_bytes();

impl Page for FetchPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "恢复fetch支持\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            let label = fw_api::label_create(ctx.content);
            if label != 0 {
                fw_api::style_apply(label, crate::FW_STYLE_HR_NAME, 255, 0);
                fw_api::label_set_text(label, FETCH_HELP.as_ptr());
                // 说明页没有开关；完整文本超出首屏后由 content 原生滚动。
                fw_api::obj_set_size_wh(label, 320, 640);
                fw_api::obj_align(label, fw_api::align::TOP_MID, 0, 12);
            }
            Ok(())
        }
    }

    fn on_click(&self, _idx: usize) {}
}


pub(super) static PAGE_BRIGHT: BrightnessPage = BrightnessPage { req: AtomicU32::new(0), req_val: AtomicU32::new(0) };
pub(super) static PAGE_CACHEP: CachePage = CachePage;
pub(super) static PAGE_VIBP: VibPage = VibPage;
pub(super) static PAGE_SETTINGS: SettingsPage = SettingsPage;
