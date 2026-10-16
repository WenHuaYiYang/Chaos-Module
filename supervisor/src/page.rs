// 页面抽象层: 每个页面是一个有状态的对象, 通过 PAGE_TABLE 分发
//
// 架构原则:
//   1. 页面知道自己的 id —— trait 方法不传 pid
//   2. 页面专属状态存在 struct 里(AtomicU32 内部可变性), 不再用全局 static mut
//   3. render 返回 Result, 失败可追溯
//   4. 新增页面 = 实现 Page + 加进 PAGE_TABLE, 不改 ui.rs

use core::sync::atomic::{AtomicU32, Ordering};
use crate::*;

// ===== 错误类型 =====

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageError {
    /// content_create 返回 0
    ContentFailed,
    /// 某个关键行控件创建失败
    RowFailed(usize),
}

pub(crate) type PageResult = Result<(), PageError>;

// ===== 渲染上下文 =====

/// 框架建好 content/title 后传给页面的渲染句柄
pub(crate) struct PageCtx {
    pub content: u32,
    pub title: u32,
    pub rows: *mut u32,
}

// ===== 行构建器 =====

/// 行控件规格。用 builder 模式配置, 避免带一堆位置参数的构造函数。
pub(crate) struct RowSpec<'a> {
    pub text: &'a [u8],
    pub sub: *const u8,
    pub trailing: u32,
    pub clickable: bool,
}

impl<'a> RowSpec<'a> {
    /// 普通可点击行
    pub fn clickable(text: &'a [u8]) -> Self {
        Self { text, sub: core::ptr::null(), trailing: 0, clickable: true }
    }
    /// 不可点击的展示行
    pub fn display(text: &'a [u8], sub: *const u8) -> Self {
        Self { text, sub, trailing: 0, clickable: false }
    }
    /// 可点击的展示行: 样子与 `display` 一样(主标签 + 可选的副标签), 但它挂事件。
    /// 用在"点这一行要触发一个动作、但这一行不该有开关/勾选框"的场合
    /// (页5 的"补写一遍"行就是: 点它启动或中断任务, 但行右端不放部件)。
    /// 注意: 这种场合若改用 `display`, 它 `clickable: false`, 点上去没有任何反应。
    pub fn tap(text: &'a [u8], sub: *const u8) -> Self {
        Self { text, sub, trailing: 0, clickable: true }
    }
    /// switch 开关行
    pub fn switch(text: &'a [u8]) -> Self {
        Self { text, sub: core::ptr::null(), trailing: fw_api::trailing::SWITCH, clickable: true }
    }
    /// 复选框行(表盘切换列表同款)。`sub` 非 NULL => 双排(主标签 + 副标签), 与表盘页
    /// 的"名称 + ID"同形; 勾选态由调用方用 `row_update` 的第 6 参**原地**写。
    /// 为什么可以用: 复选框是行控件自带的 trailing 部件(`trailing::CHECKBOX` = 2,
    /// 见 ui.rs rows_sync), 内核模块直接调 `row_create` 就能拿到。"原生 checkbox 只有
    /// 原生 ELF 能调"那个限制只针对 Lua 绑定够不到 `lvx_widgets`, 与内核模块无关。
    pub fn check(text: &'a [u8], sub: *const u8) -> Self {
        Self { text, sub, trailing: fw_api::trailing::CHECKBOX, clickable: true }
    }
}

/// 行内副标签(值)的对象偏移。`lvx_list_item_update` 的参数归属已实测核对:
/// primary -> row+0x3c, secondary -> row+0x40(见 fw_api::row_update 注释)。
const ROW_SUB_LB: u32 = 0x40;

/// 按 RowSpec 建一行 + 挂事件 + 对齐。返回行句柄, 0=失败。
/// 重要: slot 必须在 MAX_ROWS_PER_PAGE(=12) 之内 —— ctx.rows 只有 12 个元素,
/// 越界写会破坏相邻静态量, 实测表现为 slot 用到 12/13/14 时点任意一行都崩。
pub(crate) unsafe fn build_row(ctx: &PageCtx, prev: u32, slot: usize, spec: &RowSpec) -> u32 {
    if slot >= fw_api::MAX_ROWS_PER_PAGE { return 0; }
    let r = fw_api::row_create(ctx.content, spec.text.as_ptr(), spec.sub, spec.trailing);
    if r == 0 { return 0; }
    write_volatile(ctx.rows.add(slot), r);
    // 注意: 标签自套不在这里隐式全量执行(28px 统一写会把文件管理/表盘管理等
    // 列表行拉成大字)。改由 InfoPage::render 显式调用, 只作用于主页行。
    if spec.trailing == fw_api::trailing::SWITCH {
        let tobj = fw_api::row_trailing(r);
        if tobj != 0 { fw_api::obj_add_event(tobj, row_ev_fn(slot), 0, 0); }
        else { fw_api::obj_add_event(r, row_ev_fn(slot), 7, 0); }
    } else if spec.clickable {
        fw_api::obj_add_event(r, row_ev_fn(slot), 7, 0);
        // 复选框行: 复选框自己也挂一份, 点到圈上也算点这一行(表盘页只挂行本体,
        // 点到圈上没有反应)。这一行不是 switch 行, 所以复选框发出来的 VALUE_CHANGED
        // 会被 chaos_row_dispatch 丢掉, 只有 CLICKED 会走到 on_click。
        if spec.trailing == fw_api::trailing::CHECKBOX {
            let tobj = fw_api::row_trailing(r);
            if tobj != 0 { fw_api::obj_add_event(tobj, row_ev_fn(slot), 0, 0); }
        }
    }
    fw_api::obj_align_to(r, prev, 14, 0, 8);
    r
}

/// 批量建行, 任意一行失败即返回 Err
pub(crate) unsafe fn build_rows(
    ctx: &PageCtx, mut prev: u32, specs: &[(usize, RowSpec)],
) -> Result<u32, PageError> {
    for &(slot, ref spec) in specs {
        let r = build_row(ctx, prev, slot, spec);
        if r == 0 { return Err(PageError::RowFailed(slot)); }
        prev = r;
    }
    Ok(prev)
}

// ===== Page trait =====

/// 页面行为接口。页面知道自己是谁, 不需要外部传 pid。
pub(crate) trait Page: Sync {
    /// 标题文本写入 buf(48B), 必须以 \0 结尾
    fn fill_title(&self, buf: *mut u8);
    /// 标题模式: 0=无返回键, 1=有返回键
    fn title_mode(&self) -> u32 { 1 }
    /// 建页面专属控件(框架已建好 content + title)
    fn render(&self, ctx: &PageCtx) -> PageResult;
    /// 行点击(CLICKED 事件)
    fn on_click(&self, idx: usize);
    /// 开关翻转(VALUE_CHANGED 事件)
    fn on_switch(&self, _idx: usize) {}
    /// 该行是否是 switch 行(事件过滤用)
    fn is_switch_row(&self, _idx: usize) -> bool { false }
    /// 行显隐(渲染后调用)
    fn apply_visibility(&self) {}
    /// 由常驻 lv_timer 调用(忙 50ms / 息屏全空闲 1000ms), 做延迟请求消费 + UI 刷新。
    /// 只在本页是前台页且对象树存活时才被框架调用。
    fn tick(&self) {}
    /// 本页是否有待处理的 tick 逻辑(框架用, 避免空转)
    fn needs_tick(&self) -> bool { false }
}

// ===== 页0: 信息/主菜单 =====

pub(crate) struct InfoPage;

impl Page for InfoPage {
    fn fill_title(&self, buf: *mut u8) {
        // 标题栏文本 = 应用名 "Chaos"。
        // 缓冲 48B, 5 字节写入无越界
        unsafe { put_str(buf, "Chaos\0".as_bytes()); }
    }
    fn title_mode(&self) -> u32 { 0 }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            // 展示顺序: CPU(标准行) -> 存储 -> 内存 -> 导航。
            // 句柄槽位不变(数据行仍 0/1, CPU 仍 2), 只改悬挂位置。
            stat_fill(2);
            let nm2 = stat_name(2);
            let val2 = (core::ptr::addr_of!(STAT_VAL) as *const u8).add(2 * STAT_STRIDE);
            let cpu = build_row(ctx, ctx.title, 2, &RowSpec::display(
                core::slice::from_raw_parts(nm2, cstr_len(nm2)), val2));
            let anchor = if cpu != 0 { cpu } else { ctx.title };
            // 数据行组件(存储=小米蓝, 内存=绿), 封装在 fw_api::data_row_create 等
            stat_fill(0);
            let d0 = fw_api::data_row_create(ctx.content, stat_name(0),
                fw_api::BAR_FILL_RGB, storage_pct());
            fw_api::data_row_place(&d0, anchor);
            if d0.value != 0 {
                fw_api::label_set_text(d0.value, core::ptr::addr_of!(STAT_VAL) as *const u8);
            }
            stat_fill(1);
            let d1 = fw_api::data_row_create(ctx.content, stat_name(1),
                fw_api::BAR_FILL_GREEN, mem_pct());
            fw_api::data_row_place(&d1, d0.row);
            if d1.value != 0 {
                fw_api::label_set_text(d1.value,
                    (core::ptr::addr_of!(STAT_VAL) as *const u8).add(STAT_STRIDE));
            }
            // 句柄登记(tick 用): [行, 数值标签, 条] x2
            let hp = core::ptr::addr_of_mut!(DR_HANDLES) as *mut u32;
            write_volatile(hp.add(0), d0.row);
            write_volatile(hp.add(1), d0.value);
            write_volatile(hp.add(2), d0.bar);
            write_volatile(hp.add(3), d1.row);
            write_volatile(hp.add(4), d1.value);
            write_volatile(hp.add(5), d1.bar);
            // 导航入口
            build_rows(ctx, d1.row, &[
                (3, RowSpec::clickable(b"\xe5\xba\x94\xe7\x94\xa8\xe7\xae\xa1\xe7\x90\x86\0")), // 应用管理(页1, Shell++ 移植)
                (4, RowSpec::clickable(b"\xe8\xa1\xa8\xe7\x9b\x98\xe7\xae\xa1\xe7\x90\x86\0")), // 表盘管理
                (5, RowSpec::clickable(b"\xe4\xba\xae\xe5\xba\xa6\xe6\x8e\xa7\xe5\x88\xb6\0")), // 亮度
                (6, RowSpec::clickable(b"\xe7\xb3\xbb\xe7\xbb\x9f\xe7\xbe\x8e\xe5\x8c\x96\0")), // 系统美化
                (7, RowSpec::clickable(b"\xe7\xbc\x93\xe5\xad\x98\xe6\xb8\x85\xe7\x90\x86\0")), // 缓存清理(页12, Shell++ 逻辑重写)
                (8, RowSpec::clickable("关于 ".as_bytes())),
            ])?;
            // 主页行的字体不再在这里特殊处理: 逐对象补写(font_tree)会按每个标签
            // 自己的行盒选脸, 覆盖所有页面。这里再无条件写 28px 基准 face 反而
            // 会和它打架(实测: 主页导航行 28px 观感正常, 但同一规则套到列表行就偏大)。
            let _ = cpu;
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            match idx {
                // 3 = 应用管理(页1, Shell++ 移植, 见 appmgr.rs)。
                // (2026-10-15) 原文件管理入口已注释退役:
                //   3 => {
                //       let rp = core::ptr::addr_of_mut!(PATH_STACK) as *mut u8;
                //       write_volatile(rp.add(0), b'/');
                //       write_volatile(rp.add(1), 0u8);
                //       write_volatile(core::ptr::addr_of_mut!(DIR_WIN_SAVE).cast::<u32>(), 0);
                //       nav_goto(1, 0);
                //   }
                3 => nav_goto(1, 0),
                4 => nav_goto(10, 0),
                5 => nav_goto(11, 0),
                6 => nav_goto(13, 0),
                7 => nav_goto(12, 0),
                8 => nav_goto(14, 0),
                _ => {} // 行0-2 是数据行
            }
        }
    }

    fn needs_tick(&self) -> bool { true }

    fn tick(&self) {
        unsafe {
            // 每 10 tick (0.5s) 刷新三行实时数据
            let tk = st_rd!(STAT_TICK).wrapping_add(1);
            st_wr!(STAT_TICK, tk);
            if tk % 10 != 0 { return; }
            // 息屏 / AOD 一律不刷(续航): 这三行数值只有亮屏时才有人看,
            // 而每 0.5 秒一次的刷新里含三次 /proc 读 + 若干 label 重绘。少了这道门,
            // 息屏期间自愈重建还会把本页拉起来 => 整段息屏时间都在 2Hz 读 /proc 并重画。
            if !fw_api::screen_is_on() { return; }
            if PAGES[0].built == 0 || !page_is_live(0) { return; }
            // 值真变了才写: label_set_text / bar_set_pct 每次都要失效重绘, 而存储、内存
            // 这类值绝大多数拍根本没变。STAT_PREV/BAR_PREV 只在本页 tick 里用。
            static mut STAT_PREV: [[u8; STAT_STRIDE]; 3] = [[0; STAT_STRIDE]; 3];
            static mut BAR_PREV: [i32; 2] = [-1; 2];
            // 返回"文本与上一份不同"(并把新值抄进 prev)。0 终止符之外的字节全比。
            unsafe fn stat_changed(row: usize) -> bool {
                let cur = (core::ptr::addr_of!(STAT_VAL) as *const u8).add(row * STAT_STRIDE);
                let prev = core::ptr::addr_of_mut!(STAT_PREV[row]) as *mut u8;
                let mut i = 0usize;
                let mut diff = false;
                while i < STAT_STRIDE {
                    let c = read_volatile(cur.add(i));
                    if read_volatile(prev.add(i)) != c { diff = true; }
                    write_volatile(prev.add(i), c);
                    if c == 0 { break; }
                    i += 1;
                }
                diff
            }
            let rows = page_rows_ptr(0);
            let upd: unsafe extern "C" fn(u32, *const u8, *const u8, *const u8, i32, u8) -> i32 =
                fw_api::row_update;
            // 行2(CPU): 数值只写值标签, 不走 row_update。row_update 每次都会把
            // "行级联当前解析到的字体"重烘进行内值标签的 local style
            // (0x0C4C89D6 读属性 0x5A -> 0x0C4C8A34 `bl 0xc588e78`), 而本行 500ms 刷一次
            // => 烘进去的文楷被盖回行级联字体, 逐对象补写 10s 才追回一次。这正是实测
            // "只有 CPU 这类周期刷新的行闪变, 标题与数据行不闪"的根因。
            // 值标签句柄每次从行现取(行的副标签在 +0x40, 已实测核对), 不缓存。
            let r2 = read_volatile(rows.add(2));
            if r2 != 0 {
                stat_fill(2);
                let ch2 = stat_changed(2);
                let val = (core::ptr::addr_of!(STAT_VAL) as *const u8).add(2 * STAT_STRIDE);
                let vl = read_volatile((r2 + ROW_SUB_LB) as *const u32);
                if vl != 0 {
                    if ch2 { fw_api::label_set_text(vl, val); }
                } else {
                    // 值标签还没建出来: 走一次 row_update, 它会顺手把标签建好
                    upd(r2, core::ptr::null(), stat_name(2), val, 0, 0);
                }
            }
            // 行0/行1(数据行): 数值标签与条
            let hp = core::ptr::addr_of_mut!(DR_HANDLES) as *mut u32;
            let bp = core::ptr::addr_of_mut!(BAR_PREV) as *mut i32;
            let pct = [storage_pct(), mem_pct()];
            for i in 0..2usize {
                stat_fill(i);
                let ch = stat_changed(i);
                let vl = read_volatile(hp.add(i * 3 + 1));
                if vl != 0 && ch {
                    let val = (core::ptr::addr_of!(STAT_VAL) as *const u8).add(i * STAT_STRIDE);
                    fw_api::label_set_text(vl, val);
                }
                let bar = read_volatile(hp.add(i * 3 + 2));
                let p = pct[i] as i32;
                if bar != 0 && read_volatile(bp.add(i)) != p {
                    write_volatile(bp.add(i), p);
                    fw_api::bar_set_pct(bar, pct[i]);
                }
            }

        }
    }
}


/// 算 C 字符串长度(不含 \0)
unsafe fn cstr_len(s: *const u8) -> usize {
    let mut n = 0usize;
    while read_volatile(s.add(n)) != 0 { n += 1; }
    n
}

// ===== 页1-4: 文件目录 =====

pub(crate) struct DirPage;

impl Page for DirPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe {
            let pid = st_rd!(FG_PAGE) as usize;
            if pid == 1 {
                put_str(buf, "文件管理".as_bytes());
            } else {
                let d = pid - 1;
                let pth = (core::ptr::addr_of!(PATH_STACK) as *const u8).add(d * 160);
                if path_basename(pth, buf, 48) == 0 {
                    put_str(buf, "文件管理".as_bytes());
                }
            }
        }
    }

    fn render(&self, _ctx: &PageCtx) -> PageResult {
        unsafe {
            refresh_dir_lines();
            let pid = st_rd!(FG_PAGE) as usize;
            rows_layout(pid);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            let pid = st_rd!(FG_PAGE) as usize;
            let d = pid - 1;
            let cnt = st_rd!(DIR_COUNT) as usize;
            let win = st_rd!(DIR_WIN) as usize;
            if idx <= 7 {
                let e = win * 8 + idx;
                if e < cnt {
                    let names = core::ptr::addr_of!(DIR_NAMES) as *const u8;
                    let types = core::ptr::addr_of!(DIR_TYPES) as *const u8;
                    let dt = read_volatile(types.add(e));
                    let cur = (core::ptr::addr_of!(PATH_STACK) as *const u8).add(d * 160);
                    if dt == DT_DIR {
                        // 目录只占页 1..3(深度 0..2): 再往下一级没有独立 page_id 了,
                        // 不降这个级就无法给"系统模糊"一个真 page_id(见 state.rs 页名表)。
                        if d < 2 {
                            write_volatile((core::ptr::addr_of_mut!(DIR_WIN_SAVE) as *mut u32).add(d), win as u32);
                            let nxt = (core::ptr::addr_of_mut!(PATH_STACK) as *mut u8).add((d + 1) * 160);
                            path_join(nxt, cur, names.add(e * 64));
                            write_volatile((core::ptr::addr_of_mut!(DIR_WIN_SAVE) as *mut u32).add(d + 1), 0);
                            nav_goto((pid + 1) as u32, 0);
                        }
                    } else {
                        let vp = core::ptr::addr_of_mut!(VIEW_PATH) as *mut u8;
                        path_join(vp, cur, names.add(e * 64));
                        st_wr!(VIEW_DT, dt);
                        st_wr!(VIEW_BLK, 0);
                        st_wr!(VIEW_LOADED, 0);
                        nav_goto(7, 0);
                    }
                }
            } else if idx == 8 {
                if (win + 1) * 8 < cnt {
                    st_wr!(DIR_WIN, (win + 1) as u32);
                    render_req(pid);
                }
            } else if idx == 10 && pid == 1 {
                // (2026-10-15) 缓存清理入口已提升到主页行7, 文件管理退役后此分支不可达
                nav_back();
            } else {
                nav_back();
            }
        }
    }

    fn apply_visibility(&self) {
        unsafe {
            let pid = st_rd!(FG_PAGE) as usize;
            let sh = fw_api::obj_set_hidden;
            let rows = page_rows_ptr(pid);
            let cnt = st_rd!(DIR_COUNT) as usize;
            let win = st_rd!(DIR_WIN) as usize;
            for r in 0..8usize {
                let op = read_volatile(rows.add(r));
                if op != 0 {
                    let vis = if cnt == 0 { if r == 0 { 0u32 } else { 1u32 } }
                              else if win * 8 + r < cnt { 0u32 } else { 1u32 };
                    sh(op, vis);
                }
            }
            let op8 = read_volatile(rows.add(8));
            if op8 != 0 { sh(op8, if (win + 1) * 8 < cnt { 0 } else { 1 }); }
            let op9 = read_volatile(rows.add(9));
            if op9 != 0 { sh(op9, 0); }
        }
    }
}

// ===== 页7: 文件查看 =====

pub(crate) struct ViewerPage;

impl Page for ViewerPage {

    fn fill_title(&self, buf: *mut u8) {
        unsafe {
            if path_basename(core::ptr::addr_of!(VIEW_PATH) as *const u8, buf, 48) == 0 {
                put_str(buf, "文件查看".as_bytes());
            }
        }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            refresh_view_text();
            let lb = fw_api::label_create(ctx.content);
            if lb == 0 { return Err(PageError::ContentFailed); }
            fw_api::label_set_text(lb, core::ptr::addr_of!(VIEW_TEXT) as *const u8);
            let sa: unsafe extern "C" fn(u32, u32, u32, u32) -> i32 = fw_api::style_apply;
            sa(lb, FW_STYLE_MISANS_REG_24, 255, 0);
            fw_api::obj_align_to(lb, ctx.title, 14, 0, 4);
            PAGES[7].label = lb;
            let r0 = build_row(ctx, lb, 0, &RowSpec::clickable("下一段\0".as_bytes()));
            let vn = st_rd!(FILE_VIEW_N);
            let vmore = vn >= 0 && vn < 2047;
            let base = if vmore && r0 != 0 { r0 } else { lb };
            let backtxt = core::slice::from_raw_parts(
                core::ptr::addr_of!(VIEW_BACKTXT) as *const u8, cstr_len(core::ptr::addr_of!(VIEW_BACKTXT) as *const u8) + 1);
            build_row(ctx, base, 1, &RowSpec::clickable(backtxt));   // 必须 clickable: display() 不挂事件 -> 点了没反应
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            match idx {
                0 => {
                    st_wr!(VIEW_BLK, st_rd!(VIEW_BLK) + 1);
                    st_wr!(VIEW_LOADED, 0);
                    render_req(7);
                }
                1 => nav_back(),
                _ => {}
            }
        }
    }

    fn apply_visibility(&self) {
        unsafe {
            let sh = fw_api::obj_set_hidden;
            let rows = page_rows_ptr(7);
            let n = st_rd!(FILE_VIEW_N);
            let op0 = read_volatile(rows.add(0));
            if op0 != 0 { sh(op0, if n >= 2047 || n < 0 { 0 } else { 1 }); }
            let op1 = read_volatile(rows.add(1));
            if op1 != 0 { sh(op1, 0); }
        }
    }
}

// ===== 页8/9: 表盘列表(切换/轮换) =====

pub(crate) struct WatchfaceListPage {
    /// 本页对应的 page_id (8 或 9)
    pub pid: usize,
}

impl Page for WatchfaceListPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe {
            if self.pid == 8 { put_str(buf, "表盘切换".as_bytes()); }
            else { put_str(buf, "摇一摇轮换".as_bytes()); }
        }
    }

    fn render(&self, _ctx: &PageCtx) -> PageResult {
        unsafe {
            refresh_watchface_lines();
            rows_layout(self.pid);
            wf_apply_selection(self.pid);
            Ok(())
        }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx == 0 }

    fn on_switch(&self, _idx: usize) {
        unsafe {
            if self.pid == 8 {
                let v = st_rd!(WF_SHOW_BUILTIN);
                st_wr!(WF_SHOW_BUILTIN, if v != 0 { 0 } else { 1 });
                load_watchfaces(st_rd!(WF_SHOW_BUILTIN));
                render_req(8);
            } else {
                let v = st_rd!(WF_SEL_BUILTIN);
                st_wr!(WF_SEL_BUILTIN, if v != 0 { 0 } else { 1 });
                load_watchfaces(st_rd!(WF_SEL_BUILTIN));
                render_req(9);
            }
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if idx == 0 { return; }
            let cnt = st_rd!(WF_COUNT) as usize;
            let win = st_rd!(WF_WIN) as usize;
            if idx <= 8 {
                let e = win * 8 + (idx - 1);
                if e < cnt {
                    if self.pid == 8 {
                        let sw: unsafe extern "C" fn(*const u8) -> i32 =
                            core::mem::transmute(FW_SET_WATCHFACE as usize);
                        let rc = sw((core::ptr::addr_of!(WF_IDS) as *const u8).add(e * 16));
                        st_wr!(WF_SW_RC, rc);
                        if rc == 0 {
                            let node = read_volatile((core::ptr::addr_of!(WF_NODES) as *const u32).add(e));
                            let rf: unsafe extern "C" fn(u32, u32) -> i32 =
                                core::mem::transmute(FW_REFRESH_CUR_WF as usize);
                            let rfc = rf(1, node);
                            st_wr!(WF_RF_RC, rfc);
                        }
                        load_watchfaces(st_rd!(WF_SHOW_BUILTIN));
                        st_wr!(WF_WIN, win as u32);
                        wf_apply_selection(8);
                    } else {
                        let node = read_volatile((core::ptr::addr_of!(WF_NODES) as *const u32).add(e));
                        let off = !desel_has(node);
                        desel_set(node, off);
                        wf_apply_selection(9);
                    }
                }
            } else if idx == 9 {
                if (win + 1) * 8 < cnt {
                    st_wr!(WF_WIN, (win + 1) as u32);
                    render_req(self.pid);
                }
            } else {
                nav_back();
            }
        }
    }

    fn apply_visibility(&self) {
        unsafe {
            let sh = fw_api::obj_set_hidden;
            let rows = page_rows_ptr(self.pid);
            let cnt = st_rd!(WF_COUNT) as usize;
            let win = st_rd!(WF_WIN) as usize;
            let sw0 = read_volatile(rows.add(0));
            if sw0 != 0 { sh(sw0, 0); }
            for r in 0..8usize {
                let op = read_volatile(rows.add(r + 1));
                if op != 0 {
                    let vis = if cnt == 0 { if r == 0 { 0u32 } else { 1u32 } }
                              else if win * 8 + r < cnt { 0u32 } else { 1u32 };
                    sh(op, vis);
                }
            }
            let op9 = read_volatile(rows.add(9));
            if op9 != 0 { sh(op9, if (win + 1) * 8 < cnt { 0 } else { 1 }); }
            let op10 = read_volatile(rows.add(10));
            if op10 != 0 { sh(op10, 0); }
        }
    }
}

// ===== 页10: 表盘管理 =====

pub(crate) struct WfMgmtPage;

impl Page for WfMgmtPage {

    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "表盘管理".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            let mut tbuf: [u8; 88] = [0; 88];
            shake_toggle_text(tbuf.as_mut_ptr());
            let tlen = cstr_len(tbuf.as_ptr());
            let text = core::slice::from_raw_parts(tbuf.as_ptr(), tlen + 1);
            let prev = build_rows(ctx, ctx.title, &[
                (0, RowSpec::switch(text)),
                (1, RowSpec::clickable("摇一摇轮换\0".as_bytes())),
                (2, RowSpec::clickable("表盘切换\0".as_bytes())),
            ])?;
            // 补写开关初态
            let rows = page_rows_ptr(10);
            let s0 = read_volatile(rows.add(0));
            if s0 != 0 {
                let upd = fw_api::row_update;
                let en = st_rd!(SHAKE_EN);   // 开关真源 = 全局(与开机恢复/定时器门控同源)
                upd(s0, core::ptr::null(), tbuf.as_ptr(), core::ptr::null(), 0, en as u8);
            }
            let _ = prev;
            Ok(())
        }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx == 0 }

    fn on_switch(&self, _idx: usize) {
        // 开关唯一真源 = 全局 SHAKE_EN(摇一摇定时器门控与开机恢复都读它)。
        // 页面不再持本地副本 —— 本地副本会让开关初态与持久化状态脱钩
        // (旧实现: 本地默认 1, 重启后永远显示"开", 且第一次拨动写的值与所见相反)。
        unsafe {
            let v = if st_rd!(SHAKE_EN) != 0 { 0u32 } else { 1u32 };
            st_wr!(SHAKE_EN, v);
            crate::watchface::persist_shake_enabled(v);
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            match idx {
                1 => nav_goto(9, 0),
                2 => {
                    load_watchfaces(st_rd!(WF_SHOW_BUILTIN));
                    nav_goto(8, 0);
                }
                _ => {}
            }
        }
    }
}

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

// ===== 页13: 系统美化(只有菜单, 三条进二级页) =====

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
                (5, RowSpec::clickable("返回\0".as_bytes()))])?;
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
                5 => nav_back(),
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

// ===== 页4: 系统美化 -> 系统模糊(二级) =====
//
// 背景档位与状态文案都在 background_home(0 关 / 1 复用静态背景 / 3 完全透明),
// 这里只建行与派发点击。返回固定走行 11 —— 与"更换字体""桌面图标"两页同一套。
// 原"固定图片背景"那一档已删: 它只读 /data/chaos/background.bin, 而这文件从没有生成过。
// 行5 = "模糊区域"入口(页15, blur_zone): 分区域启停背景替换(启动器/控制中心/通知中心/小部件)。
pub(crate) struct BlurPage;

impl Page for BlurPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "系统模糊\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::check("系统背景\0".as_bytes(), core::ptr::null())),
                (1, RowSpec::switch("静态背景图(表盘)\0".as_bytes())),
                (2, RowSpec::switch("自动截取替换图片\0".as_bytes())),
                (3, RowSpec::clickable("手动生成或更新表盘背景\0".as_bytes())),
                (4, RowSpec::switch("完全透明背景（实验性）\0".as_bytes())),
                (5, RowSpec::display(crate::background_home::status(), core::ptr::null())),
                (6, RowSpec::clickable("模糊区域\0".as_bytes())),
                (11, RowSpec::clickable("返回\0".as_bytes()))])?;
            // 勾选态原地写(不重建整页): 行 0/1 是两档单选(档位), 行 2/4 是开关
            let sel = crate::background_home::mode() as usize;
            for i in 0..2 {
                fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(i)), fw_api::NONE_PTR,
                    fw_api::NONE_PTR, fw_api::NONE_PTR, 0, (i == sel) as u8);
            }
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(4)), fw_api::NONE_PTR,
                fw_api::NONE_PTR, fw_api::NONE_PTR, 0, (sel == 3) as u8);
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(2)), fw_api::NONE_PTR,
                fw_api::NONE_PTR, fw_api::NONE_PTR, 0, crate::background_home::realtime_on() as u8);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            match idx {
                0 => crate::background_home::request(0),
                3 => crate::background_home::generate(),
                6 => nav_goto(crate::blur_zone::PAGE_PID, 0),
                11 => nav_back(),
                _ => {}
            }
        }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx == 1 || idx == 2 || idx == 4 }

    fn on_switch(&self, idx: usize) {
        unsafe {
            if idx == 1 {
                crate::background_home::request(
                    if crate::background_home::mode() == 1 { 0 } else { 1 });
            }
            if idx == 2 {
                crate::background_home::set_realtime(
                    if crate::background_home::realtime_on() != 0 { 0 } else { 1 });
            }
            if idx == 4 {
                crate::background_home::request(
                    if crate::background_home::mode() == 3 { 0 } else { 3 });
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
                (8, RowSpec::clickable("返回\0".as_bytes()))])?;
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
                // 0..7 = 字体条目。语义在 font_list::click_row(与"桌面图标"页同一套):
                // 点未选中的 = 切过去; 点已选中的 = 第一次变"再点一次删除", 再点才删;
                // 点当前在用的 = 提示"先切走"(在用的那份绝不删)。
                0..=7 => font_list::click_row(idx),
                8 => nav_back(),
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

// ===== 页1: 应用管理(Shell++ shellpp-ii 核心逻辑移植, 见 appmgr.rs) =====
//
// (2026-10-15 二版) 列表形态改为**全量条目**: 去掉"更多"分页, 注册表里的每个应用
// 一行复选框(勾 = 当前可见), 行数超过一屏靠 content 滚动(lvx_page_content 本就是
// 可滚容器, content_pad_bottom 就是给滚动收尾用的)。
// 行句柄/事件槽位约束: 条目行**不走 ctx.rows**(只有 12 格) —— 句柄自管在
// appmgr::APP_ROWS, 事件槽号 = 条目下标 + 12(12..=51; 0..11 是 ctx.rows 的
// 批量按钮/状态行, 条目复用会被派发成点按钮), 回调是 lib.rs 的 chaos_row_ev12..51。
// 布局: 行0=一键隐藏全部 行1=一键显示全部 行2=状态 —— 放最上面, 列表再长也不怕
// 按钮滚出屏幕; 之后是应用条目。

pub(crate) struct AppMgrPage;

impl Page for AppMgrPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "应用管理\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            crate::appmgr::ensure();
            crate::appmgr::rows_reset();   // 旧对象树已随上次 content 销毁, 丢旧句柄
            // 顶部三行(走 ctx.rows 槽 0..2)
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::clickable(b"\xe4\xb8\x80\xe9\x94\xae\xe9\x9a\x90\xe8\x97\x8f\xe5\x85\xa8\xe9\x83\xa8\0")), // 一键隐藏全部
                (1, RowSpec::clickable(b"\xe4\xb8\x80\xe9\x94\xae\xe6\x98\xbe\xe7\xa4\xba\xe5\x85\xa8\xe9\x83\xa8\0")), // 一键显示全部
                (2, RowSpec::display(crate::appmgr::msg(), core::ptr::null())),
            ])?;
            // 条目行(全量, 事件槽 = 下标; 句柄存 appmgr::APP_ROWS)
            let mut prev = read_volatile(ctx.rows.add(2));
            if prev == 0 { prev = ctx.title; }
            let rows = crate::appmgr::rows_ptr();
            let n = crate::appmgr::count();
            let mut i = 0usize;
            while i < n {
                let r = fw_api::row_create(ctx.content,
                    crate::appmgr::entry_primary(i), crate::appmgr::pkg_ptr(i),
                    fw_api::trailing::CHECKBOX);
                if r == 0 { return Err(PageError::RowFailed(i)); }
                write_volatile(rows.add(i), r);
                fw_api::obj_add_event(r, row_ev_fn(i + 12), 7, 0);   // 事件槽 = 条目下标 + 12
                // 复选框自己也挂一份(点到圈上也算点这一行, 与图标页同款)
                let tobj = fw_api::row_trailing(r);
                if tobj != 0 { fw_api::obj_add_event(tobj, row_ev_fn(i + 12), 0, 0); }
                fw_api::obj_align_to(r, prev, 14, 0, 8);
                prev = r;
                // 勾选态: 勾 = 当前可见(原地补写)
                // secondary 必须原样再传: row_update 对 secondary=NULL 的语义是"清空并隐藏"
                // 已存在的副标签(0x0C4C8904 反编译实证), 传 NONE 会把 row_create 建出的包名洗掉
                fw_api::row_update(r, fw_api::NONE_PTR, fw_api::NONE_PTR,
                    crate::appmgr::pkg_ptr(i), 0, crate::appmgr::visible(i) as u8);
                i += 1;
            }
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            // 槽0/1 = 顶部批量按钮(ctx.rows); 条目事件槽 = 条目下标 + 12
            // (0..11 被 ctx.rows 占用: 不错开的话点条目会被派发成点按钮 —— 2026-10-15 修)
            if idx == 0 {
                crate::appmgr::request_batch(true);
                render_req(1);
            } else if idx == 1 {
                crate::appmgr::request_batch(false);
                render_req(1);
            } else if idx >= 12 {
                let e = idx - 12;
                if e < crate::appmgr::count() { crate::appmgr::click(e); }
            }
        }
    }

    fn needs_tick(&self) -> bool {
        unsafe { crate::appmgr::has_req() || crate::appmgr::batch_pending() }
    }

    fn tick(&self) {
        unsafe {
            if crate::appmgr::batch_pending() {
                crate::appmgr::do_batch();
                render_req(1);
            } else if crate::appmgr::has_req() {
                crate::appmgr::do_op();
                render_req(1);
            }
        }
    }
}

// ===== 分发表 =====

static PAGE_INFO: InfoPage = InfoPage;
static PAGE_DIR: DirPage = DirPage;
static PAGE_APPMGR: AppMgrPage = AppMgrPage;
static PAGE_VIEWER: ViewerPage = ViewerPage;
static PAGE_WF_LIST_8: WatchfaceListPage = WatchfaceListPage { pid: 8 };
static PAGE_WF_LIST_9: WatchfaceListPage = WatchfaceListPage { pid: 9 };
static PAGE_WF_MGMT: WfMgmtPage = WfMgmtPage;
static PAGE_BRIGHT: BrightnessPage = BrightnessPage { req: AtomicU32::new(0), req_val: AtomicU32::new(0) };
static PAGE_CACHEP: CachePage = CachePage;
static PAGE_BEAUTIFY: BeautifyPage = BeautifyPage;
static PAGE_BLURP: BlurPage = BlurPage;
static PAGE_ZONEP: BlurZonePage = BlurZonePage;
static PAGE_FONTLIST: FontListPage = FontListPage;
static PAGE_ICONSUB: IconSubPage = IconSubPage;
static PAGE_ABOUT: AboutPage = AboutPage;

pub(crate) struct AboutPage;
static mut ABOUT_FW: [u8; 32] = [0; 32];
static mut ABOUT_BT: [u8; 40] = [0; 40];

unsafe fn about_firmware() -> &'static [u8] {
    let out = core::ptr::addr_of_mut!(ABOUT_FW) as *mut u8;
    let fd = fw_api::open(b"/etc/build.prop\0".as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd >= 0 {
        let mut b = [0u8; 512];
        let n = fw_api::read(fd, b.as_mut_ptr(), 511);
        fw_api::close(fd);
        let key = b"ro.build.version=";
        let mut i = 0usize;
        while i + key.len() < n.max(0) as usize {
            let mut same = true;
            for j in 0..key.len() { if b[i + j] != key[j] { same = false; break; } }
            if same {
                let start = i + key.len();
                let mut end = start;
                while end < n as usize && b[end] != b'\n' && b[end] != b'\r' && b[end] != 0 { end += 1; }
                let len = core::cmp::min(end - start, 31);
                for j in 0..len { write_volatile(out.add(j), b[start + j]); }
                write_volatile(out.add(len), 0);
                return core::slice::from_raw_parts(out, len + 1);
            }
            i += 1;
        }
    }
    put_str(out, "未知 ".as_bytes());
    core::slice::from_raw_parts(out, 7)
}

unsafe fn about_build_time() -> &'static [u8] {
    let out = core::ptr::addr_of_mut!(ABOUT_BT) as *mut u8;
    let src = option_env!("CHAOS_BUILD_TIME").unwrap_or("unknown").as_bytes();
    let len = core::cmp::min(src.len(), 39);
    for i in 0..len { write_volatile(out.add(i), src[i]); }
    write_volatile(out.add(len), 0);
    core::slice::from_raw_parts(out, len + 1)
}

impl Page for AboutPage {
    fn fill_title(&self, buf: *mut u8) { unsafe { put_str(buf, "关于\0".as_bytes()); } }
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { build_rows(ctx, ctx.title, &[
            (0, RowSpec::display("Chaos\0".as_bytes(), "v1.78.91\0".as_bytes().as_ptr())),
            (1, RowSpec::display(about_build_time(), "当前Chaos编译时间\0".as_bytes().as_ptr())),
            (2, RowSpec::display(about_firmware(), "运行固件\0".as_bytes().as_ptr())),
            (3, RowSpec::display("文华逸洋\0".as_bytes(), "开发者\0".as_bytes().as_ptr())),
            (4, RowSpec::display("IKUN_CXKPRO\0".as_bytes(), "开发者\0".as_bytes().as_ptr())),
            (5, RowSpec::clickable("返回\0".as_bytes())),
        ]).map(|_| ()) }
    }
    fn on_click(&self, idx: usize) { if idx == 5 { unsafe { nav_back(); } } }
}

// ===== 页15: 系统模糊 -> 模糊区域(三级) =====
//
// 四个背景替换区域的开关(数据与持久化全在 blur_zone.rs, 这里只建行与派发):
//   行0=启动器 行1=控制中心 行2=通知中心 行3=小部件 行11=返回。
// 开 = 正常替换(默认); 关 = 该区域保持官方模糊画面。
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
                (0, RowSpec::switch("启动器\0".as_bytes())),
                (1, RowSpec::switch("控制中心\0".as_bytes())),
                (2, RowSpec::switch("小部件\0".as_bytes())),
                (3, RowSpec::switch("通知中心\0".as_bytes())),
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

pub(crate) static PAGE_TABLE: [&dyn Page; 16] = [
    &PAGE_INFO,       // 0
    &PAGE_APPMGR,     // 1  应用管理(Shell++ 移植; 文件管理入口已注释退役)
    &PAGE_DIR,        // 2  目录 2 级(不可达, 保留注册)
    &PAGE_DIR,        // 3  目录 3 级(文件管理最深一级)
    &PAGE_BLURP,      // 4  系统美化 -> 系统模糊(原来是目录 4 级, 为腾独立 page_id 重分配)
    &PAGE_FONTLIST,   // 5  系统美化 -> 更换字体(字体清单 + 自由切换)
    &PAGE_ICONSUB,    // 6  系统美化 -> 桌面图标
    &PAGE_VIEWER,     // 7
    &PAGE_WF_LIST_8,  // 8
    &PAGE_WF_LIST_9,  // 9
    &PAGE_WF_MGMT,    // 10
    &PAGE_BRIGHT,     // 11
    &PAGE_CACHEP,     // 12
    &PAGE_BEAUTIFY,     // 13
    &PAGE_ABOUT,        // 14
    &PAGE_ZONEP,        // 15 系统模糊 -> 模糊区域
];
