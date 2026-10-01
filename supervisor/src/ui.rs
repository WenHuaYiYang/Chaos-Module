// 页面框架(建页/行/事件分发/生命周期回调)

use crate::*;
use crate::mem::{rd16, rd32};

// ===== content 容器的固有几何(约束: root 永不自建, 控件一律挂 content) =====
// 336x424 摆在 y=56 起的地方, 底部再留 32 不压手势区。56 不是随手取的:
// 它就是 lvx_page_title 的类固有高度(0x38 = 56, 见 fw_api::page 的注释),
// 标题栏与内容区首尾相接、**不重叠** —— "新 content 盖住标题栏"这类解释
// 按这条几何就能直接排除。
const CONTENT_W: i32 = 336;
const CONTENT_H: i32 = 424;
const CONTENT_TOP: i32 = 56;
const CONTENT_PAD_BOTTOM: i32 = 32;

pub(crate) unsafe extern "C" fn chaos_on_signal() -> i32 {
    core::arch::asm!("nop");
    0
}

#[no_mangle]
pub(crate) unsafe extern "C" fn chaos_on_create(_page: u32, _p1: u32, _p2: u32) -> i32 {
    // 多页架构: 固件每页 on_create(page_desc, root, start_data) 传各自 root
    // page_id = 描述符 +0x14 (u16, 注册时 ipc::build_structures 写成 app_id<<16|页号)
    // 存该页 root/start_data, built=0 → on_resume 在新 root 上重建对象树
    if _page == 0 { return 0; }
    let pid = rd16((_page + 0x14) as *const u16) as usize;
    if pid <= MAX_PID {
        // 旧对象树的句柄在这里统一作废, 且**只丢句柄不删对象**: 固件把新页交给我们时,
        // 上一个实例的对象树已经随旧页销毁了。反过来不能在 on_destroy 里清 —— 息屏时固件
        // 也会回调 on_destroy, 但对象树和页面都还在, 清了会让信息页的实时刷新永久停摆。
        PAGES[pid].content = 0;
        PAGES[pid].label = 0;
        PAGES[pid].title = 0;
        {
            let rws = page_rows_ptr(pid);
            let mut q = 0usize;
            while q < fw_api::MAX_ROWS_PER_PAGE { write_volatile(rws.add(q), 0); q += 1; }
        }
        PAGES[pid].desc = _page;
        PAGES[pid].root = _p1;
        PAGES[pid].built = 0;
        // 页5/页6 的确认框挂在**页根**上(状态机见 confirm_pop.rs), 新实例 = 旧根连同旧框已被
        // 固件销毁: 框句柄状态必须在这里归零, 否则后面的删除会打到复用同一块内存的新对象上。
        crate::font_list::popup_forget(pid);
        crate::icon_apply::popup_forget(pid);
        st_wr!(HEAL_TRYS, 0);
    st_wr!(APP_FG, 1);   // 我们的应用在前台(逐对象补写那类跑批按这道门过滤)
        if pid == 7 {
            // 查看页每次进入 = 新文件: 块归零 + 强制重读(VIEW_PATH 已由目录页写好)
            st_wr!(VIEW_BLK, 0);
            st_wr!(VIEW_LOADED, 0);
        }
    }
    0
}

#[inline(always)]
pub(crate) unsafe fn page_rows_ptr(pid: usize) -> *mut u32 {
    PAGES[pid].rows.as_mut_ptr()
}

// 页面对象树是否还活着: 读固件描述符 +0x30 的 root_object。
// 静态核对: 固件的 destroy 包装递归删除 root 之后会把这一项清零, 所以
//   非 0 = 对象树仍在, 可以安全写行/内容; 0 = 已经删干净, 什么都别碰(否则 UAF 崩)。
// 这是固件自己维护的标志, 比我们缓存的句柄可靠 —— 退出应用时只有它会立刻反映出来。
pub(crate) unsafe fn page_is_live(pid: usize) -> bool {
    if pid > MAX_PID { return false; }
    let dsc = PAGES[pid].desc;
    dsc != 0 && rd32((dsc + 0x30) as *const u32) != 0
}

// 写一段不带结尾 0 的字面量并补 0
pub(crate) unsafe fn put_str(dst: *mut u8, s: &[u8]) -> usize {
    let mut o = 0usize;
    while o < s.len() { write_volatile(dst.add(o), s[o]); o += 1; }
    write_volatile(dst.add(o), 0u8);
    o
}

// 亮度页: 填充状态行文本 "NNNnit [A]" (A=自动开)
pub(crate) unsafe fn bright_fill_status() {
    let buf = core::ptr::addr_of_mut!(BRIGHT_STATUS) as *mut u8;
    let v = fw_api::brightness_get_target();
    let auto = fw_api::brightness_get_auto_adjustment();
    // 手动写十进制(nit 值最大 4 位: 2500)
    let mut o = 0usize;
    let mut n = v;
    if n >= 1000 { write_volatile(buf.add(o), b'0' + (n / 1000) as u8); o += 1; n %= 1000; }
    if v >= 100 { write_volatile(buf.add(o), b'0' + (n / 100) as u8); o += 1; n %= 100; }
    if v >= 10 { write_volatile(buf.add(o), b'0' + (n / 10) as u8); o += 1; n %= 10; }
    write_volatile(buf.add(o), b'0' + n as u8); o += 1;
    // "nit"
    for ch in b"nit" { write_volatile(buf.add(o), *ch); o += 1; }
    if auto != 0 { for ch in b" [A]" { write_volatile(buf.add(o), *ch); o += 1; } }
    write_volatile(buf.add(o), 0u8);
}

// 行显隐(通过 trait 分发, 不写 if-else 长链)
pub(crate) unsafe fn apply_row_visibility(pid: usize) {
    if pid <= MAX_PID {
        PAGE_TABLE[pid].apply_visibility();
    }
}

// 登记"重建某页"请求: 点击回调里不能直接销毁自身对象树(事件派发中 → use-after-free),
// 交给常驻 lv_timer(UI线程, 不在事件派发上下文)真正重建
pub(crate) unsafe fn render_req(pid: usize) {
    st_wr!(RENDER_REQ, pid as u32);
}

// ===== 动态建行(固件核对: 无批量建行 API, 由调用方按需循环建) =====
// rows_sync: 只为 slots 里的槽建行控件(未列出的槽不建=不占位); 已建的按 slots 顺序重排对齐链
// 表盘页(8/9)的 slot0 = 名称行 + switch(trailing=1), slot1..8 = 名称+ID 双排 + 复选框(trailing=2);
// 其余条目与导航行 = 单排无 trailing。行右端部件由 row_create 第 4 参选, 见 fw_api::trailing。
pub(crate) unsafe fn rows_sync(pid: usize, slots: &[usize]) {
    let c = PAGES[pid].content;
    let t = PAGES[pid].title;
    if c == 0 { return; }
    let rc: unsafe extern "C" fn(u32, *const u8, *const u8, u32) -> u32 = fw_api::row_create;
    let ea: unsafe extern "C" fn(u32, u32, u32, u32) -> u32 = fw_api::obj_add_event;
    let at: unsafe extern "C" fn(u32, u32, u32, i32, i32) -> u32 = fw_api::obj_align_to;
    let sh: unsafe extern "C" fn(u32, u32) -> u32 = fw_api::obj_set_hidden;
    let rows = page_rows_ptr(pid);
    let wf_page = pid == 8 || pid == 9;      // 只有这两个页用表盘列表缓冲与复选框
    let mut prev = if t != 0 { t } else { c };
    let mut k = 0usize;
    while k < slots.len() {
        let slot = slots[k];
        let mut row = read_volatile(rows.add(slot));
        if row == 0 {
            let p = if wf_page {
                (core::ptr::addr_of!(WATCH_LINES) as *const u8).add(slot * 88)
            } else {
                (core::ptr::addr_of!(FILE_LINES) as *const u8).add(slot * 88)
            };
            let sub: *const u8 = if wf_page && slot >= 1 && slot <= 8 {
                (core::ptr::addr_of!(WATCH_SUB) as *const u8).add(slot * 88)
            } else { core::ptr::null() };
            // slot0 = switch(1); slot1..8 = 复选框(2); 其余 = 无
            let tr: u32 = if wf_page && slot == 0 { fw_api::trailing::SWITCH }
                          else if wf_page && slot >= 1 && slot <= 8 { fw_api::trailing::CHECKBOX }
                          else { fw_api::trailing::NONE };
            row = rc(c, p, sub, tr);
            if row != 0 {
                write_volatile(rows.add(slot), row);
                if wf_page && slot == 0 {
                    // switch 行: 事件挂在 trailing 对象上、事件码 0(=LV_EVENT_ALL),
                    // 取不到 trailing 时退回挂整行(事件码 7=CLICKED)
                    let gtr: unsafe extern "C" fn(u32) -> u32 = fw_api::row_trailing;
                    let tobj = gtr(row);
                    if tobj != 0 { ea(tobj, row_ev_fn(slot), 0, 0); }
                    else { ea(row, row_ev_fn(slot), 7, 0); }
                } else {
                    ea(row, row_ev_fn(slot), 7, 0);
                }
            }
        }
        // 注意: 这里不做统一的字号写回 —— 本路径服务目录页与表盘列表,
        // 按 28px 统一写回会把这两类列表拉成大字(实测)。标签字号各自收窄, 见 page.rs。
        if row != 0 { at(row, prev, 14, 0, 8); prev = row; }
        k += 1;
    }
    // 与固件自带页面的做法一致: 未列入 slots 的已建行全部隐藏
    // 关键是它们**不在对齐链里**, 所以不会把后面的行顶下去留出空白
    let mut j = 0usize;
    while j < MAX_ROWS_PER_PAGE {
        let mut used = false;
        let mut q = 0usize;
        while q < slots.len() { if slots[q] == j { used = true; break; } q += 1; }
        if !used {
            let r = read_volatile(rows.add(j));
            if r != 0 { sh(r, 1); }
        }
        j += 1;
    }
}

// rows_layout: 本页需要哪些槽 = [条目 0..min(8,cnt)-1] + [更多(slot8, 还有下一页时)] + [返回/上级(slot9)]
// cnt==0 时留一个 slot0 显示提示语(空目录/无表盘/管理器未就绪)
pub(crate) unsafe fn rows_layout(pid: usize) {
    let (cnt, win) = if pid == 8 || pid == 9 {
        (st_rd!(WF_COUNT) as usize,
         st_rd!(WF_WIN) as usize)
    } else {
        (st_rd!(DIR_COUNT) as usize,
         st_rd!(DIR_WIN) as usize)
    };
    // 只算「本窗内」的条目数: 若按全量 cnt 算, 第二屏会多建空行,
    // 并把"返回"行链到隐藏行下面 → 中间留一大片空白
    let win_base = win * 8;
    let ent = if cnt > win_base {
        let left = cnt - win_base;
        if left < 8 { left } else { 8 }
    } else { 0 };
    // 槽位上限: 8 条目 + 更多 + switch + 返回 = 11 → 数组按 12 留; 少一格就是栈越界写,
    // 越界落在相邻局部量上, 表现为表盘页卡死
    let mut slots = [0usize; 12];
    let mut n = 0usize;
    let mut i = 0usize;
    if pid == 8 || pid == 9 {
        // 表盘页/轮换页: 0=显示内置表盘 switch, 1..8=条目, 9=更多, 10=返回
        slots[n] = 0; n += 1;
        while i < ent { slots[n] = i + 1; n += 1; i += 1; }
        if ent == 0 { slots[n] = 1; n += 1; }    // 空列表: 用 slot1 显示提示语
        if (win + 1) * 8 < cnt { slots[n] = 9; n += 1; }
        slots[n] = 10; n += 1;
    } else {
        // 目录页: 0..7=条目, 8=更多, 9=返回/上级, 10=缓存清理入口(仅根目录页 pid1 建这一行)
        while i < ent { slots[n] = i; n += 1; i += 1; }
        if ent == 0 { slots[n] = 0; n += 1; }
        if (win + 1) * 8 < cnt { slots[n] = 8; n += 1; }
        slots[n] = 9; n += 1;
        if pid == 1 { slots[n] = 10; n += 1; }
    }
    rows_sync(pid, &slots[..n]);
}

// 统一渲染: 需要重建时先删旧 content/title → content 三件套 → title → 页面专属控件挂 content
pub(crate) unsafe fn render_page(pid: usize) -> i32 {
    if pid > MAX_PID { return -1; }
    let root = PAGES[pid].root;
    if root == 0 { return -1; }
    // 重建式渲染: 先销毁上次的 content(行/label 是其子对象, 一并销毁) → 再走与首次
    // 完全相同的 build 路径。刷新旧对象会让 align_to 读到脏几何(错位/空白/重叠的根源)。
    // 标题栏**跟着一起重做**(只建一次并保留会把上一代的 local 脸留在活对象上,
    // 切完字体整条标题栏不画且救不回来, 原因见下面删除标题那段)。
    if PAGES[pid].built != 0 {
        let oc = PAGES[pid].content;
        if oc != 0 { fw_api::obj_delete(oc); }
        // 标题栏跟着一起重做(旧对象删掉, 后面按首次建页那条路重建)。
        // 实测: 页5 切一次字体后**整条标题栏不再画**, 重新应用/上下滚动都
        // 救不回来, 只有退出重进才好。原因是标题标签上那份"固件建行时直烘的
        // local text_font"指着上一代**已被回收的脸**:
        //   * 样式级写回(font_apply)改的是共享样式表, 盖不住对象身上的 local 覆盖;
        //   * 唯一能改写 local 覆盖的逐对象补写, 被 font_apply 的 `APP_FG == 0` 门锁住
        //     —— 我们的页在前台时一拍都不跑(边重建边写对象就是那条崩溃轴)。
        // 而"退出重进"做的恰好就是"删掉旧标题 + 重新建一个", 这里把它自动化。
        // 顺序仍然是 content 先建、title 后建 => 标题照旧压在 content 之上(frontmost)。
        let ot = PAGES[pid].title;
        if ot != 0 { fw_api::obj_delete(ot); }
        PAGES[pid].content = 0;
        PAGES[pid].label = 0;
        PAGES[pid].title = 0;
        // 必须清满全部槽位: 少清会留下悬空句柄 → 重建时复用已删对象 = UAF
        // 上限引用 fw_api::MAX_ROWS_PER_PAGE(单一事实来源, 避免"改了容量忘了改循环")
        let rws = page_rows_ptr(pid);
        let mut q = 0usize;
        while q < fw_api::MAX_ROWS_PER_PAGE { write_volatile(rws.add(q), 0); q += 1; }
        PAGES[pid].built = 0;
    }
    if PAGES[pid].built == 0 {
        // content 三件套: 创建 → 定尺寸 → 顶部对齐 → 底部留白
        let cc: unsafe extern "C" fn(u32) -> u32 = fw_api::content_create;
        let c = cc(root);
        if c == 0 { return -1; }
        let ss: unsafe extern "C" fn(u32, i32, i32) -> u32 = fw_api::obj_set_size;
        ss(c, CONTENT_W, CONTENT_H);
        let al: unsafe extern "C" fn(u32, u32, i32, i32) -> u32 = fw_api::obj_align;
        al(c, fw_api::align::TOP_MID, 0, CONTENT_TOP);
        let pb: unsafe extern "C" fn(u32, i32, u32) -> u32 = fw_api::content_pad_bottom;
        pb(c, CONTENT_PAD_BOTTOM, 0);
        PAGES[pid].content = c;
        // title(content 后创建→frontmost; mode 由各页 title_mode() 给出, 1=带返回键 → 动画返回上级)
        let tc: unsafe extern "C" fn(u32, *const u8, u32, u32, u32) -> u32 =
            fw_api::page_title_create;
        // 标题 = 进入本页时点的那个行控件名; 目录页/查看页是动态的, 写进本页的标题缓冲。
        // cb=NULL → 固件装默认 back 回调(0x0C4CA6A9→0x0CA76FB5 动画pop) = 系统标题返回行为
        let tcb: u32 = 0;
        let tud: u32 = 0;
        let tb = (core::ptr::addr_of_mut!(TITLE_TXT) as *mut u8).add(pid * 48);
        let page_ref = PAGE_TABLE[pid];
        // 所有页(含页0)一律走 fill_title 写进本页标题缓冲。
        // 根因: 在这一行特判页0 直接给硬编码标题, 页面自己的 fill_title 就根本不被调用。
        page_ref.fill_title(tb);
        let tname: *const u8 = tb;
        let mode: u32 = page_ref.title_mode();
        // 标题栏: 上面重建分支已经把旧对象删掉并把句柄归零, 所以每次重建都会
        // 重走一遍创建(与首次建页同一条路、同一个顺序: content 之后建标题)。
        let mut t = PAGES[pid].title;
        if t == 0 {
            t = tc(root, tname, mode, tcb, tud);
            if t == 0 { return -1; }
            let sh: unsafe extern "C" fn(u32, u32) -> u32 = fw_api::obj_set_hidden;
            sh(t, 0);
            PAGES[pid].title = t;
        }
        // 页面专属控件(通过 trait 分发, 不写 if-else 长链)
        let ctx = PageCtx {
            content: PAGES[pid].content,
            title: t,
            rows: page_rows_ptr(pid),
        };
        let _ = page_ref.render(&ctx);  // 失败时页面部分渲染, 不阻塞
        PAGES[pid].built = 1;
    }
    apply_row_visibility(pid);
    0
}

// 导航: page_goto 压栈 / page_finish 出栈, 转场动画由固件提供
pub(crate) unsafe fn nav_goto(target_pid: u32, start_data: u32) {
    let key = (st_rd!(APP_ID) << 16) | target_pid;
    fw_api::page_goto(key, start_data);
}

pub(crate) unsafe fn nav_back() {
    // 系统动画返回(与固件默认 title back 同款); page_finish(desc) 无动画, 实测已证伪
    fw_api::page_back();
}

// 行事件分发(通过 trait 查表, 不写 if-else 长链)
pub(crate) unsafe fn chaos_row_dispatch(idx: usize, event: u32) -> u32 {
    if event == 0 { return 0; }
    let pid = st_rd!(FG_PAGE) as usize;
    if pid > MAX_PID { return 0; }
    let gc: unsafe extern "C" fn(u32) -> u32 = fw_api::event_get_code;
    let code = gc(event);
    let page_ref = PAGE_TABLE[pid];
    // switch 行只认 VALUE_CHANGED, 其余只认 CLICKED
    if page_ref.is_switch_row(idx) {
        if code != fw_api::ev::VALUE_CHANGED { return 0; }
        page_ref.on_switch(idx);
    } else {
        if code != fw_api::ev::CLICKED { return 0; }
        page_ref.on_click(idx);
    }
    0
}

/// 行回调的取址: 函数项要先落到函数指针再转整数(直接 `fn as u32` 会被编译器警告)
#[inline]
fn cb_u32(f: unsafe extern "C" fn(u32) -> u32) -> u32 {
    f as u32
}

pub(crate) unsafe fn row_ev_fn(i: usize) -> u32 {
    match i {
        0 => cb_u32(chaos_row_ev0), 1 => cb_u32(chaos_row_ev1), 2 => cb_u32(chaos_row_ev2),
        3 => cb_u32(chaos_row_ev3), 4 => cb_u32(chaos_row_ev4), 5 => cb_u32(chaos_row_ev5),
        6 => cb_u32(chaos_row_ev6), 7 => cb_u32(chaos_row_ev7), 8 => cb_u32(chaos_row_ev8),
        9 => cb_u32(chaos_row_ev9),
        // 注意: 每个槽位都要有自己的回调; 用 catch-all 兜住 slot10/11 会把"返回"当"更多"派发
        10 => cb_u32(chaos_row_ev10),
        _ => cb_u32(chaos_row_ev11),
    }
}

// ===== 页面生命周期(对应固件的 manager_page_on_* 一组回调; 描述符 +0x14 = page_id) =====
#[no_mangle]
pub(crate) unsafe extern "C" fn chaos_on_resume(page: u32) -> i32 {
    st_wr!(HEAL_TRYS, 0);
    // 这里不做「定时器存活检测」: 判定 timer 已死就把 SHAKE_TIMER 清零、再让 shake_arm
    // 重建 —— 但**只清句柄不等于删除对象**: 旧 timer 仍挂在 LVGL 定时器链表里继续跑,
    // 于是每命中一次就多一个并发定时器; 而 STAT_TICK 是每拍 tick 递增一次,
    // 定时器变成 N 个 -> STAT_TICK 快 N 倍 -> 信息页的 % 10 刷新门槛被命中 N 倍
    // => 表现就是「刷新越来越快」(进查看页/多次 resume 后尤其明显, 实测)。
    // 结论: shake_arm 全生命周期只建一次; 要「先删后建」必须先拿到并验证 lv_timer_delete。
    // 描述符可能不是本页的(唤醒时固件换了编码) → 解析不出就退回当前前台页, 保证仍能渲染
    let mut pid = if page == 0 { FG_PAGE as usize }
                  else { rd16((page + 0x14) as *const u16) as usize };
    if pid > MAX_PID { pid = st_rd!(FG_PAGE) as usize; }
    if pid > MAX_PID { return 0; }
    st_wr!(FG_PAGE, pid as u32);
    shake_arm();   // 首次进入(UI线程)订阅摇一摇+建timer, 幂等
    if pid >= 1 && pid <= 4 {
        // 目录页: depth=pid-1, 恢复该级窗位, 重新加载目录(数据永远新鲜)。
        // 目录页对应 pid 1-4: 更深的层级没有分配 page_id, 这里的上界跟着那条分配走
        st_wr!(DIR_DEPTH, (pid - 1) as u32);
        let w = read_volatile((core::ptr::addr_of!(DIR_WIN_SAVE) as *const u32).add(pid - 1));
        st_wr!(DIR_WIN, w);
        load_dir();
    }
    if pid == 8 || pid == 9 {
        // 两页各自独立过滤: 表盘页看 WF_SHOW_BUILTIN, 轮换页看 WF_SEL_BUILTIN
        let f = if pid == 8 {
            st_rd!(WF_SHOW_BUILTIN)
        } else {
            st_rd!(WF_SEL_BUILTIN)
        };
        load_watchfaces(f);
    }
    if pid == 6 {
        // 桌面图标页(系统美化的二级页): 进页重读图标包清单 —— 刚侧载投递完的包
        // 不用重启就该出现在列表里。列表行的原地刷新在 render 里做。
        icon_apply::refresh_packs();
    }
    if pid == 5 {
        // 更换字体页(系统美化的二级页): 进页重读字体清单, 同理 —— 刚投递进去的字体
        // 不用重启就该出现在列表里。
        font_list::refresh();
    }
    render_page(pid);
    0
}

#[no_mangle]
pub(crate) unsafe extern "C" fn chaos_on_pause() -> i32 {
    st_wr!(APP_FG, 0);
    // 息屏/退后台: 不做任何事。在这里置旗标让 on_resume 重建 lv_timer 是不行的 —— 实测
    // 定时器在息屏期间存活, 重建只会多出一个并发实例(见 watchface.rs shake_arm 注释)
    0
}

#[no_mangle]
pub(crate) unsafe extern "C" fn chaos_on_destroy(_page: u32) -> i32 {
    // 这里**不清任何句柄**。实测: 息屏时固件也会回调 on_destroy, 而页面对象树并没有销毁
    // (唤醒后行控件照样能点、内容照样显示 —— 事件回调就挂在我们建的行对象上)。
    // 一旦在这里清零 built/root, 信息页的刷新会永久停摆, 而点击却正常(点击不看这些句柄)。
    // 句柄作废统一放到 on_create: 那时固件给的是新页, 旧对象树确实已经销毁。
    0
}

#[no_mangle]
pub(crate) unsafe extern "C" fn chaos_on_ui_destroy(page: u32) -> i32 {
    // 固件递归删除 root_object **之前**回调这里(静态核对固件页面类型说明: 这是删除前的
    // 可选回调)。把该页 LVGL 句柄全部作废 —— 只丢句柄不删对象, 因为固件马上就要把
    // 整棵树删掉。
    // 这一步是"退出应用不崩"的关键: 定时器比页面活得久, 句柄不清零它就会往已释放的对象上写。
    if page == 0 { return 0; }
    let pid = rd16((page + 0x14) as *const u16) as usize;
    if pid > MAX_PID { return 0; }
    PAGES[pid].desc = 0;
    PAGES[pid].root = 0;
    PAGES[pid].content = 0;
    PAGES[pid].label = 0;
    PAGES[pid].title = 0;
    PAGES[pid].built = 0;
    let rows = page_rows_ptr(pid);
    let mut i = 0usize;
    while i < fw_api::MAX_ROWS_PER_PAGE { write_volatile(rows.add(i), 0); i += 1; }
    0
}

pub(crate) unsafe extern "C" fn chaos_display_name() -> *const u8 {
    // 应用名中英文统一为 Chaos。
    // 两条分支都返回 "Chaos"; 保留分支结构是因为 LANG 还被别的文案用着。
    if st_rd!(LANG) != 0 {
        NAME_ZH.as_ptr()
    } else {
        DISPLAY_NAME.as_ptr()
    }
}

