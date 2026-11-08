//! 页4/5/6/7/13/15/28：字体、图标、模糊与桌面布局美化。

use crate::page::*;
use crate::page::Page;

// ===== 页13: 系统美化(菜单) / 页5: 更换字体 / 页6: 桌面图标 =====
//
// 结构(一个菜单页 + 两个二级页, 二级页要带页面跳转动画):
//   页13 菜单    行0 更换字体 -> nav_goto(5) / 行1 桌面图标 -> nav_goto(6) / 行2 返回
//   页5  字体    行0..7 字体条目(动态建行 + 真复选框) / 行8 重新应用字体 /
//                行9 再补写一遍 / 行11 返回(nav_back)
//   页6  图标    行0..9 图标包条目(动态建行 + 真复选框) / 行11 返回(nav_back)
//
// 为什么两个二级页要占独立 page_id: 固件的页面跳转动画只在真正的 page_goto/page_back
// 里有(页面注册见 ipc.rs)。页内切层级(只 render_req)没有动画, 整页内容原地换一批,
// 观感上就不是"进了另一个页面"。文件管理每级目录各占一个 page_id 也是同一个原因。
// 代价: 原生应用最多注册 14 页, 再多会在安装过程中黑屏自重启(机制未验证, 按硬上限处理);
// 所以文件管理从 6 级压到 3 级腾出三个 page_id(页4/5/6) —— 注册总数仍是 14, 只是重新分配。
//
// 行位分配都受 MAX_ROWS_PER_PAGE(=12) 封顶: 条目行从行 0 起排, 页5/页6 的行 11 固定"返回"。

/// 图标列表的第 0 个行槽(页6: 行0..9 共 10 行, 行11 返回)
const ICON_ROW0: usize = 0;

/// 静态 C 串缓冲 -> 带结尾 NUL 的切片。行控件读的是 C 串, 传切片是给 RowSpec 用。
unsafe fn cbuf(p: *const u8) -> &'static [u8] {
    let n = cstr_len(p);
    if n == 0 { return &[]; }
    core::slice::from_raw_parts(p, n + 1)
}

// ===== 页13: 系统美化(菜单, 四条进二级页) =====

/// 页28: 系统美化 -> 应用列表设置(二级)。桌面图标三条排版律的开关都放在那里。
pub(crate) const APP_LIST_PID: u32 = 28;

pub(crate) struct BeautifyPage;

impl Page for BeautifyPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "系统美化\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::clickable("更换字体\0".as_bytes())),
                (1, RowSpec::clickable("桌面图标\0".as_bytes())),
                (2, RowSpec::clickable("系统模糊\0".as_bytes())),
                (3, RowSpec::switch("重启恢复字体和图标\0".as_bytes())),
                (4, RowSpec::switch("重启恢复模糊\0".as_bytes())),
                (5, RowSpec::clickable("应用列表设置\0".as_bytes())),
                (6, RowSpec::clickable("返回\0".as_bytes()))])?;
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(3)),
                fw_api::NONE_PTR, fw_api::NONE_PTR, crate::beautify_restore::status(), 0,
                crate::beautify_restore::enabled() as u8);
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(4)),
                fw_api::NONE_PTR, fw_api::NONE_PTR, crate::res_hook::status(), 0,
                crate::res_hook::held() as u8);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            // 二级页一律 nav_goto(真页面跳转 => 固件带压栈动画, 返回键也是动画 pop);
            // 页内切层级(只 render_req)没有动画, 不许再用这种做法。
            match idx {
                0 => nav_goto(5, 0),
                1 => nav_goto(6, 0),
                2 => nav_goto(crate::background_home::BLUR_PID, 0),
                5 => nav_goto(APP_LIST_PID, 0),
                6 => nav_back(),
                _ => {}
            }
        }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx == 3 || idx == 4 }

    fn on_switch(&self, idx: usize) {
        match idx {
            3 => unsafe { crate::beautify_restore::request_toggle(); },
            // 关掉 = 让出 LVGL 资源钩子。Corona 那类模块要求槽里仍是出厂回调才肯装,
            // 先让位、让它装好、再打开这一格, 两条重定向就能串在一起。
            4 => unsafe { crate::res_hook::request_toggle(); },
            _ => {}
        }
    }
}

// ===== 页28: 系统美化 -> 应用列表设置(二级) =====
//
// 桌面图标的三条排版参数都在 desktop_scale 那张写表层: 行0 = 缩放折线(同 RW6),
// 行1 = 排距锚点(同 RW6), 行2 = 外列内缩(同 RW6)。三个开关都默认开, 关掉就把
// 对应的表写回固件原值 —— 实现路径只有一条, 开关只改参数。
pub(crate) struct AppListPage;

impl Page for AppListPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "应用列表设置\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::switch("缩放同 RW6\0".as_bytes())),
                (1, RowSpec::switch("排距同 RW6\0".as_bytes())),
                (2, RowSpec::switch("内缩同 RW6\0".as_bytes())),
                (11, RowSpec::clickable("返回\0".as_bytes()))])?;
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(0)),
                fw_api::NONE_PTR, fw_api::NONE_PTR, fw_api::NONE_PTR, 0,
                crate::desktop_scale::lens_on() as u8);
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(1)),
                fw_api::NONE_PTR, fw_api::NONE_PTR, fw_api::NONE_PTR, 0,
                crate::desktop_scale::pitch_on() as u8);
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(2)),
                fw_api::NONE_PTR, fw_api::NONE_PTR, fw_api::NONE_PTR, 0,
                crate::desktop_scale::inset_on() as u8);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe { if idx == 11 { nav_back(); } }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx < 3 }

    fn on_switch(&self, idx: usize) {
        unsafe {
            if idx == 0 {
                crate::desktop_scale::set_lens(!crate::desktop_scale::lens_on());
            } else if idx == 1 {
                crate::desktop_scale::set_pitch(!crate::desktop_scale::pitch_on());
            } else if idx == 2 {
                crate::desktop_scale::set_inset(!crate::desktop_scale::inset_on());
            } else {
                return;
            }
            crate::render_req(APP_LIST_PID as usize);
        }
    }
}

// ===== 页4: 系统美化 -> 系统模糊(二级) =====
//
// 背景档位与状态文案都在 background_home(0 关 / 1 复用静态背景 / 3 完全透明),
// 这里只建行与派发点击。返回固定走行 11 —— 与"更换字体""桌面图标"两页同一套。
// 旧版静态背景/实时截取设置移到页7，主页面保留系统背景、模糊v2、
// 旧版选项和模糊区域。corona模糊那一行（行1）已按要求注释停用，行号留空位。
// 行号固定为 0=系统 / (1=corona 停用) / 2=v2 / 3=状态 / 4=旧版 / 5=区域 / 11=返回。
pub(crate) struct BlurPage;

pub(crate) const BLUR_LEGACY_PID: u32 = 7;

impl Page for BlurPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "系统模糊\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            let prev = build_rows(ctx, ctx.title, &[
                (0, RowSpec::check("系统背景\0".as_bytes(), core::ptr::null())),
                // 行1 = corona模糊开关：按要求停用（保留空位，不重排其它行号）。
                // 引擎本身没删：模糊v2 是同一套 Corona 引擎，corona_blur::active()
                // 只要 v2 打开就成立；旧配置若还是 1，点一下“系统背景”即可关掉它
                // （on_click 分支仍然调用 corona_blur::set(false)）。
                // (1, RowSpec::switch("corona模糊\0".as_bytes())),
                (2, RowSpec::switch("模糊v2\0".as_bytes())),
                (3, RowSpec::display(crate::background_home::status(), core::ptr::null())),
                (4, RowSpec::tap("旧版选项\0".as_bytes(), "关闭上面的模糊开关后生效\0".as_bytes().as_ptr())),
                (5, RowSpec::tap("模糊区域\0".as_bytes(), "适用于所有模糊选项（含通知图标去黑底）\0".as_bytes().as_ptr())),
                (11, RowSpec::clickable("返回\0".as_bytes()))])?;
            // 长说明不放入会裁切的行副标题。与许可全文相同：标签只限宽，
            // 高度随换行增长；挂在原生可滚动 content 上，完整 URL 不含 Markdown 标记。
            let label = fw_api::label_create(ctx.content);
            if label == 0 { return Err(PageError::RowFailed(6)); }
            fw_api::style_apply(label, crate::FW_STYLE_HR_NAME, 255, 0);
            fw_api::obj_set_width(label, 320);
            fw_api::label_set_text(label,
                "说明：“corona模糊”功能源码来自https://github.com/AstralSightStudios/Canopus-Module-Corona，修改后源码可见：https://github.com/WenHuaYiYang/Chaos-Module/tree/master/supervisor。\0".as_bytes().as_ptr());
            fw_api::obj_align_to(label, prev, fw_api::align::OUT_BOTTOM_MID, 0, 8);
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(0)), fw_api::NONE_PTR,
                fw_api::NONE_PTR, fw_api::NONE_PTR, 0,
                (crate::background_home::mode() == 0 && !crate::corona_blur::active()) as u8);
            // 行1（corona模糊）已停用，不再刷新勾选态。
            // fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(1)), fw_api::NONE_PTR,
            //     fw_api::NONE_PTR, fw_api::NONE_PTR, 0,
            //     crate::corona_blur::enabled() as u8);
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(2)), fw_api::NONE_PTR,
                fw_api::NONE_PTR, fw_api::NONE_PTR, 0,
                crate::corona_blur::v2_enabled() as u8);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            match idx {
                0 => {
                    // 选回官方背景须关闭两开关；引擎和 v2 图标都由背景生命周期安全退役。
                    crate::corona_blur::set(false);
                    crate::corona_blur::set_v2_enabled(false);
                    crate::background_home::request(0);
                },
                4 => nav_goto(BLUR_LEGACY_PID, 0),
                5 => nav_goto(crate::blur_zone::PAGE_PID, 0),
                11 => nav_back(),
                _ => {}
            }
        }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx == 2 }

    fn on_switch(&self, idx: usize) {
        unsafe {
            // 行1（corona模糊）已停用：不接开关事件，避免它被“看不见的手”翻开。
            // if idx == 1 {
            //     crate::corona_blur::set(!crate::corona_blur::enabled());
            // } else
            if idx == 2 {
                crate::corona_blur::set_v2_enabled(!crate::corona_blur::v2_enabled());
            }
        }
    }
}

// ===== 页7: 系统模糊 -> 旧版选项 =====
//
// 这是原系统模糊页的兼容入口。系统背景(主页面)与模糊区域(页15)不在这里重复，
// 其余旧档位/实时截取设置全部集中到本页；corona 开启时保留设置但不应用。
pub(crate) struct BlurLegacyPage;

impl Page for BlurLegacyPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "旧版选项\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::switch("静态背景图(表盘)\0".as_bytes())),
                (1, RowSpec::switch("自动截取替换图片\0".as_bytes())),
                (2, RowSpec::clickable("手动生成或更新表盘背景\0".as_bytes())),
                (3, RowSpec::switch("完全透明背景（实验性）\0".as_bytes())),
                (4, RowSpec::display(crate::background_home::status(), core::ptr::null())),
                (5, RowSpec::display("关闭 corona模糊后生效\0".as_bytes(), core::ptr::null())),
                (11, RowSpec::clickable("返回\0".as_bytes()))])?;
            let mode = crate::background_home::mode();
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(0)), fw_api::NONE_PTR,
                fw_api::NONE_PTR, fw_api::NONE_PTR, 0, (mode == 1) as u8);
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(1)), fw_api::NONE_PTR,
                fw_api::NONE_PTR, fw_api::NONE_PTR, 0,
                crate::background_home::realtime_on() as u8);
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(3)), fw_api::NONE_PTR,
                fw_api::NONE_PTR, fw_api::NONE_PTR, 0, (mode == 3) as u8);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            match idx {
                2 => crate::background_home::generate(),
                11 => nav_back(),
                _ => {}
            }
        }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx == 0 || idx == 1 || idx == 3 }

    fn on_switch(&self, idx: usize) {
        unsafe {
            match idx {
                0 => crate::background_home::request(
                    if crate::background_home::mode() == 1 { 0 } else { 1 }),
                1 => crate::background_home::set_realtime(
                    if crate::background_home::realtime_on() != 0 { 0 } else { 1 }),
                3 => crate::background_home::request(
                    if crate::background_home::mode() == 3 { 0 } else { 3 }),
                _ => {}
            }
        }
    }
}

// ===== 页5: 系统美化 -> 更换字体(二级) =====
//
// 页面形态: 复选框列表(与"桌面图标"页同款) —— 清单里有几条字体就列几条,
// 点哪条就切到哪条; 已选中的那条再点走两步删除。
// 数据面全在 font_list.rs(清单 / 切换 / 两步删除), 这里只负责建行与派发点击。
//
// 行位: 0..7 = 字体条目(动态, 有几条建几条), 8 = 重新应用字体, 9 = 补写一遍,
// 11 = 返回。条目行数由 font_list::entry_shown() 截断, 与 0..7 这 8 个槽位一致。
// 删除字体的确认浮层叠在本页 content 上, 不占行位。
pub(crate) struct FontListPage;

/// 字体条目行的起始槽位(与 action/返回 隔开: 8 条占 0..7)
const FONT_ROW0: usize = 0;
/// "重新应用字体"那一行的槽位
/// 行 0..7 = 字体条目槽位(FONT_ROW0 起); (v3.6) 原"重新应用字体"(行8)与
/// "补写一遍"(行9)退役: 热替换接管后重应用冗余、逐对象补写不再需要。

impl Page for FontListPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe {
            put_str(buf, "更换字体\0".as_bytes());
        }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            // 确认框挂在页根上, 重建只销毁 content 带不走它 —— 先把它的状态归零。
            font_list::popup_reset();
            font_list::refresh();          // 新投递的字体不用重启就该出现在列表里
            let ent = font_list::entry_shown();
            font_list::lines_fill();
            // 动态建行: 有几条建几行。条目行带真复选框(trailing::CHECKBOX),
            // 勾选态靠 row_update 原地写 —— 与"桌面图标"页、"表盘切换"页同一套。
            let mut last = ctx.title;
            let mut i = 0usize;
            while i < ent as usize {
                let spec = RowSpec::check(cbuf(font_list::line_primary(i)),
                                          font_list::line_secondary(i));
                let r = build_row(ctx, last, FONT_ROW0 + i, &spec);
                if r != 0 { last = r; }
                i += 1;
            }
            // 字体一个都没有时不新增行(复用行 0 那个槽位), 只给一句说明:
            // 空列表页什么都不说会像是界面坏了。副标签指着那句静态文本。
            if ent == 0 {
                let r = build_row(ctx, last, FONT_ROW0, &RowSpec::display(
                    "还没有投递过字体\0".as_bytes(), FONT_EMPTY_SUB.as_ptr()));
                if r != 0 { last = r; }
            }
            build_rows(ctx, last, &[
                (9, RowSpec::clickable("返回\0".as_bytes()))])?;
            // 建完行立刻原地补写勾选态, 并把行句柄交给 font_list: 切换与两步删除
            // 的反馈都走原地刷新(不重建整页 —— 重建会把整页字体样式重抄一遍)。
            font_list::bind_rows(ctx.rows, FONT_ROW0, ent as usize);
            font_list::refresh_rows();
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            match idx {
                // 行0固定为官方字体，行1..8对应八个自定义字体槽位。
                0..=8 => font_list::click_row(idx),
                9 => nav_back(),
                _ => {}
            }
        }
    }
}
static FONT_EMPTY_SUB: &[u8] = b"\xe5\x85\x88\xe4\xbe\xa7\xe8\xbd\xbd\xe4\xb8\x80\xe4\xb8\xaa\xe5\xad\x97\xe4\xbd\x93\xe6\x8a\x95\xe9\x80\x92\xe5\x8c\x85\0";

// ===== 页6: 系统美化 -> 桌面图标(二级) =====

pub(crate) struct IconSubPage;

impl Page for IconSubPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "桌面图标\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            // 确认框挂在本页根对象上: 重建只销毁 content, 带不走它 —— 开头先兜住
            // 前一次没关掉的框, 再把状态归零(与"更换字体"页同款)。
            icon_apply::popup_reset();
            icon_apply::refresh_packs();   // 新投递的包不用重启就该出现在列表里
            let ent = icon_apply::entry_shown();
            icon_apply::lines_fill();
            // 动态建行: 有几条建几行。条目行带真复选框(trailing::CHECKBOX),
            // 勾选态靠 row_update 原地写 —— 与"表盘切换"页同一套做法。
            // 列表第一项是"系统原图标": 这一页的交互就是图标切换(复选框列表), 不是
            // "应用/恢复"两个动作按钮; 做成第 0 项之后切换与还原是同一个动作。
            let mut last = ctx.title;
            let mut i = 0usize;
            while i < ent as usize {
                let spec = RowSpec::check(cbuf(icon_apply::line_primary(i)),
                                          icon_apply::line_secondary(i));
                let r = build_row(ctx, last, ICON_ROW0 + i, &spec);
                if r != 0 { last = r; }
                i += 1;
            }
            build_rows(ctx, last, &[(11, RowSpec::clickable("返回\0".as_bytes()))])?;
            // 建完行立刻原地补写勾选态, 并把行句柄交给 icon_apply: 之后点一下切换、
            // 两次确认删除都只走原地刷新(不重建整页 —— 重建会把整页字体样式重抄一遍)。
            icon_apply::bind_rows(ctx.rows, ICON_ROW0, ent as usize);
            icon_apply::refresh_rows();
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            match idx {
                // 0..9 = 图标包条目。选中语义全在 icon_apply::click_row 里:
                // 点未选中的 = 切过去; 点已选中的包 = 先变成"再点一次删除", 再点才删。
                0..=9 => icon_apply::click_row(idx),
                11 => nav_back(),
                _ => {}
            }
        }
    }
}


// ===== 页15: 系统模糊 -> 模糊区域(三级) =====
//
// 四个背景替换区域的开关(数据与持久化全在 blur_zone.rs, 这里只建行与派发):
//   行0=启动器 行1=控制中心 行2=小部件 行3=通知中心 行11=返回。
// 开 = 正常替换(默认); 关 = 该区域保持官方模糊画面。
// 对 Corona 开关生效；关闭通知区域也恢复原图标和去黑底 hook。
// 翻位后 blur_zone::set 持久化 + background_home::remount() 置 REQUEST,
// 由背景 tick 的拆/装流程按新掩码统一重建(页面退出前就能看到拆装, 不用重启)。
// 开关初态: 单排行, row_update 第 6 参原地补写(secondary 传 NONE 是安全的 ——
// 这些行没有副标签, 详见 fw_api::row_update 注释)。
pub(crate) struct BlurZonePage;

impl Page for BlurZonePage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "模糊区域\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            crate::blur_zone::ensure_loaded();
            build_rows(ctx, ctx.title, &[
                (4, RowSpec::display("适用于 Corona 模糊\0".as_bytes(),
                    "包括通知图标去黑底\0".as_bytes().as_ptr())),
                (0, RowSpec::switch("启动器\0".as_bytes())),
                (1, RowSpec::switch("控制中心\0".as_bytes())),
                (2, RowSpec::switch("小部件\0".as_bytes())),
                (3, RowSpec::switch("通知中心\0".as_bytes())),
                (5, RowSpec::display("关闭通知区域恢复原图标\0".as_bytes(),
                    "同时撤销通知图标去黑底\0".as_bytes().as_ptr())),
                (11, RowSpec::clickable("返回\0".as_bytes()))])?;
            for i in 0..crate::blur_zone::REGION_N {
                fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(i)),
                    fw_api::NONE_PTR, fw_api::NONE_PTR, fw_api::NONE_PTR,
                    0, crate::blur_zone::on(i) as u8);
            }
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        if idx == 11 { unsafe { nav_back(); } }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx < crate::blur_zone::REGION_N }

    fn on_switch(&self, idx: usize) {
        unsafe {
            if idx >= crate::blur_zone::REGION_N { return; }
            if crate::blur_zone::set(idx, !crate::blur_zone::on(idx)) {
                crate::background_home::remount();
            }
        }
    }
}


pub(super) static PAGE_BEAUTIFY: BeautifyPage = BeautifyPage;
pub(super) static PAGE_APPLIST: AppListPage = AppListPage;
pub(super) static PAGE_BLURP: BlurPage = BlurPage;
pub(super) static PAGE_ZONEP: BlurZonePage = BlurZonePage;
pub(super) static PAGE_FONTLIST: FontListPage = FontListPage;
pub(super) static PAGE_ICONSUB: IconSubPage = IconSubPage;
pub(super) static PAGE_BLUR_LEGACYP: BlurLegacyPage = BlurLegacyPage;
