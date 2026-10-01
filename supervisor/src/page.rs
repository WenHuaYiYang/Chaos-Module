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
                (3, RowSpec::clickable(b"\xe6\x96\x87\xe4\xbb\xb6\xe7\xae\xa1\xe7\x90\x86\0")), // 文件管理
                (4, RowSpec::clickable(b"\xe8\xa1\xa8\xe7\x9b\x98\xe7\xae\xa1\xe7\x90\x86\0")), // 表盘管理
                (5, RowSpec::clickable(b"\xe4\xba\xae\xe5\xba\xa6\xe6\x8e\xa7\xe5\x88\xb6\0")), // 亮度
                (6, RowSpec::clickable(b"\xe7\xb3\xbb\xe7\xbb\x9f\xe7\xbe\x8e\xe5\x8c\x96\0")), // 系统美化
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
                3 => {
                    let rp = core::ptr::addr_of_mut!(PATH_STACK) as *mut u8;
                    write_volatile(rp.add(0), b'/');
                    write_volatile(rp.add(1), 0u8);
                    write_volatile(core::ptr::addr_of_mut!(DIR_WIN_SAVE).cast::<u32>(), 0);
                    nav_goto(1, 0);
                }
                4 => nav_goto(10, 0),
                5 => nav_goto(11, 0),
                6 => nav_goto(13, 0),
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
                        if d < 3 {
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
                nav_goto(12, 0);   // 根目录专属: 清理的是固定两条缓存目录, 与当前目录无关
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

pub(crate) struct WfMgmtPage {
    /// 摇一摇开关状态(AtomicU32 内部可变, 不再用全局 static mut)
    pub shake_en: AtomicU32,
}

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
                let en = self.shake_en.load(Ordering::Relaxed);
                upd(s0, core::ptr::null(), tbuf.as_ptr(), core::ptr::null(), 0, en as u8);
            }
            let _ = prev;
            Ok(())
        }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx == 0 }

    fn on_switch(&self, _idx: usize) {
        let en = self.shake_en.load(Ordering::Relaxed);
        self.shake_en.store(if en != 0 { 0 } else { 1 }, Ordering::Relaxed);
        // 同步到全局(摇一摇定时器读的是全局)
        unsafe {
            st_wr!(SHAKE_EN, self.shake_en.load(Ordering::Relaxed));
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

// ===== 页12: 缓存清理(从文件管理根目录进入) =====

pub(crate) struct CachePage;

impl Page for CachePage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "缓存清理".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            // 首行文本: 刚出清理结果就展示结果(消费一次), 否则现场扫描当前缓存文件数
            if st_rd!(CACHE_HAS_RESULT) == 0 {
                let mut files = 0u32;
                let mut fail = 0u32;
                cache_walk(false, &mut files, &mut fail);
                st_wr!(CACHE_FILES, files);
                cache_fmt_msg(if files == 0 { 2 } else { 0 }, files, 0, 0);
            } else {
                st_wr!(CACHE_HAS_RESULT, 0);
            }
            let msgp = core::ptr::addr_of!(CACHE_MSG) as *const u8;
            let msg = core::slice::from_raw_parts(msgp, cstr_len(msgp) + 1);
            let r0 = build_row(ctx, ctx.title, 0, &RowSpec::display(msg, core::ptr::null()));
            let anchor = if r0 != 0 { r0 } else { ctx.title };
            let r1 = build_row(ctx, anchor, 1,
                &RowSpec::clickable("开始清理\0".as_bytes()));
            if r1 == 0 { return Err(PageError::RowFailed(1)); }
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if idx == 1 { st_wr!(CLEAN_REQ, 1); }   // 清理由 tick 执行(不在事件回调里做长 FS 操作)
        }
    }

    fn needs_tick(&self) -> bool {
        unsafe { st_rd!(CLEAN_REQ) != 0 }
    }

    fn tick(&self) {
        unsafe {
            if st_rd!(CLEAN_REQ) == 0 { return; }
            st_wr!(CLEAN_REQ, 0);
            // 释放量 = 数据分区"已用"的前后差(/proc/fs/usage, 最大容量那行)
            let before = fs_used_mb();
            let mut files = 0u32;
            let mut fail = 0u32;
            cache_walk(true, &mut files, &mut fail);
            let after = fs_used_mb();
            let freed = if before == u32::MAX || after == u32::MAX || after >= before {
                0
            } else { before - after };
            cache_fmt_msg(1, files, fail, freed);
            st_wr!(CACHE_HAS_RESULT, 1);
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
// 所以文件管理从 6 级压到 4 级腾出两个 page_id —— 注册总数仍是 14, 只是重新分配。
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

// ===== 页13: 系统美化(只有菜单, 两条进二级页) =====

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
                (2, RowSpec::clickable("返回\0".as_bytes()))])?;
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            match idx {
                // 进二级页走 nav_goto(真页面跳转 => 固件带压栈动画, 返回键也是动画 pop)
                0 => nav_goto(5, 0),
                1 => nav_goto(6, 0),
                2 => nav_back(),
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
const FONT_ROW_REAPPLY: usize = 8;
/// "补写一遍"那一行的槽位。逐对象补写(font_tree)是一次性任务: 点一下排一趟, 再点 = 中断。
/// 样式级写回盖不到的对象(小部件屏 / 桌面"布局切换"的旧脸对象)只有这条路够得到。
const FONT_ROW_BFILL: usize = 9;
/// 行9 副标签的静态文本缓冲(行控件一直读这些指针, 必须是 static)
static mut FT_BFILL_SUB: [u8; 48] = [0; 48];

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
            let bf_on = font_tree::enabled();
            // 行9 副标签 = 任务读数(跑: 进度; 完: 生效/没生效/导航中断/树变丢弃四个数)。
            // 判据可分辨: 每种结局一个独立计数, 一次读数就能区分全部结局。
            {
                let (ok, nw, nav, ch, rounds, pages) = font_tree::backfill_stats();
                let mut w = W::new(FT_BFILL_SUB.as_mut_ptr(), FT_BFILL_SUB.len());
                if bf_on {
                    w.s("跑到第 ".as_bytes()); w.n(rounds);
                    w.s("/".as_bytes()); w.n(pages); w.s(" 页".as_bytes());
                } else if ok != 0 || nw != 0 || nav != 0 || ch != 0 {
                    w.s("补 ".as_bytes()); w.n(ok); w.s(" 处".as_bytes());
                    if nw != 0 { w.s(" 败 ".as_bytes()); w.n(nw); }
                    if ch != 0 { w.s(" 树变 ".as_bytes()); w.n(ch); }
                    if nav != 0 { w.s(" 断 ".as_bytes()); w.n(nav); }
                } else {
                    w.s("按一次, 全页栈补一遍".as_bytes());
                }
                w.end();
            }
            build_rows(ctx, last, &[
                (FONT_ROW_REAPPLY, RowSpec::clickable("重新应用字体\0".as_bytes())),
                // 样式级写回(含 23 条派生样式)盖不住小部件屏与桌面"布局切换"的旧脸
                // 对象 —— 它们的 face 不经过样式级写回触及的任何样式。
                // 逐对象直写是唯一够得到的路, 所以做成带门控的一次性任务(见 font_tree)。
                // 这一行用 tap 而不是 switch/check: 点它只是排一趟任务或中断, 行右端不放部件。
                (FONT_ROW_BFILL, RowSpec::tap(if bf_on {
                    "补写中, 再点中断\0".as_bytes() } else { "补写一遍(修小部件/桌面)\0".as_bytes() },
                    FT_BFILL_SUB.as_ptr())),
                (11, RowSpec::clickable("返回\0".as_bytes()))])?;
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
                // 8 = 请求重跑样式级写回。不在这里直接跑: 点击回调是固件的事件派发
                //     上下文, 建 132 张 face 是秒级重活, 交给 UI tick 消费(见 font_apply::tick)。
                8 => font_apply::request(),
                // 9 = 一次性补写任务: 只翻静态量, 遍历与写入分拍在常驻 tick 里跑。
                //     任务在跑时再点 = 中断(见 font_tree::toggle)。
                9 => font_tree::toggle(),
                11 => nav_back(),
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

// ===== 分发表 =====

static PAGE_INFO: InfoPage = InfoPage;
static PAGE_DIR: DirPage = DirPage;
static PAGE_VIEWER: ViewerPage = ViewerPage;
static PAGE_WF_LIST_8: WatchfaceListPage = WatchfaceListPage { pid: 8 };
static PAGE_WF_LIST_9: WatchfaceListPage = WatchfaceListPage { pid: 9 };
static PAGE_WF_MGMT: WfMgmtPage = WfMgmtPage { shake_en: AtomicU32::new(1) };
static PAGE_BRIGHT: BrightnessPage = BrightnessPage { req: AtomicU32::new(0), req_val: AtomicU32::new(0) };
static PAGE_CACHEP: CachePage = CachePage;
static PAGE_BEAUTIFY: BeautifyPage = BeautifyPage;
static PAGE_FONTLIST: FontListPage = FontListPage;
static PAGE_ICONSUB: IconSubPage = IconSubPage;

pub(crate) static PAGE_TABLE: [&dyn Page; 14] = [
    &PAGE_INFO,       // 0
    &PAGE_DIR,        // 1  目录 1 级(/)
    &PAGE_DIR,        // 2  目录 2 级
    &PAGE_DIR,        // 3  目录 3 级
    &PAGE_DIR,        // 4  目录 4 级(文件管理最深一级)
    &PAGE_FONTLIST,   // 5  系统美化 -> 更换字体(字体清单 + 自由切换)
    &PAGE_ICONSUB,    // 6  系统美化 -> 桌面图标
    &PAGE_VIEWER,     // 7
    &PAGE_WF_LIST_8,  // 8
    &PAGE_WF_LIST_9,  // 9
    &PAGE_WF_MGMT,    // 10
    &PAGE_BRIGHT,     // 11
    &PAGE_CACHEP,     // 12
    &PAGE_BEAUTIFY,     // 13
];
