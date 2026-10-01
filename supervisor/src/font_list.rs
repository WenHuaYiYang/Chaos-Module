// 字体清单层 —— "更换字体"页的数据面: 有哪些字体可选 / 点一下切换 / 两步删除。
//
// 为什么要有这一层:
//   这一层之前, 字体页只有两个按钮(重新应用字体 / 补写一遍), 要的是**和"桌面图标切换"
//   一样的复选框列表**: 想用哪个字体就点哪个。于是:
//     1) 池位共 8 个(见 font_apply::FONT_PATHS), 一个字体永远占同一个槽位, 不轮转;
//     2) 槽位归属由 /data/chaos/font/index.txt 这份**定长清单**决定 —— 格式与图标清单
//        (`/data/chaos/icons/index.txt`)**逐字节同一套**: 恒 128 字节 = 8 行 x 16 字节,
//        行 = `[2 位槽号][空格][12 字节短名][换行]`, 空槽 = 16 个空格。两处代码各自写一遍
//        (刻意不抽公共模块: 图标那条链已经跑通, 不去动它), 两侧的常量与行格式必须逐条对上。
//     3) 切换 = font_apply::commit(n) + font_apply::request() —— 这两句正是 0x30 门里
//        一直在做的事(已验证), 所以切换这条路的机制风险最低。
//
// 清单是唯一来源, **绝不遍历目录**(procfs 目录遍历在表盘 Lua 回调里必死锁)。
// 清单文件本身是普通文件(不是设备节点), UI 线程读它不会阻塞 —— 会阻塞到看门狗复位的是设备节点。
//
// 删除的两条红线(与图标那边同理):
//   1) 绝不删当前在用的那一份 —— 固件建出的 face 会按需回读文件。清单页上"点已选中的条目"
//      只会给出"先切走再删"的提示, 真正的删除门在 request_delete 里再判一次(界面提示不是门)。
//   2) 绝不删"可能仍被读"的文件 —— 所以先改清单(写失败就中止, 不出现半截状态), 再逐拍删文件。
//
// 界面做法与"桌面图标切换"页同一套(见 page.rs 页6): 动态建行 + 真复选框(trailing::CHECKBOX)。
// 差别在"什么时候画": 本文件**只有 render() 那一处**原地写行(页面上下文, 句柄刚建好必然有效);
// 其余任何状态变化(切换选中、删除确认态、提示、探测完成)都只登记 FL_ROWS_REQ, 由 tick 在
// `window_is_quiet()` 那一拍排一次整页重建 —— 定时器侧一概不直接碰对象, 也不在跑批窗口里重建。

use crate::mem::rd8;
use crate::*;

/// 字体目录(**不带**结尾斜杠: 目录名与路径前缀两种拼法都要用)
const FONT_DIR: &[u8] = b"/data/chaos/font";
/// 清单路径(定长记录, 与图标清单同一格式)
const FONT_INDEX: &[u8] = b"/data/chaos/font/index.txt\0";
/// 清单一行的字节数(2 位槽号 + 空格 + 12 字节短名 + 换行)
const FL_LINE: usize = 16;
/// 清单最多几个字体 = 池位上限(font_apply::FONT_SLOT_MAX), 两边必须一致
const FL_MAX: usize = 8;
/// 清单文件恒长(128 字节)
const FL_INDEX_SIZE: usize = FL_LINE * FL_MAX;
/// 短名缓冲字节数: 12 字节可用 + NUL 余量(16 是 .bss 里的对齐形状, 与图标那边同款)
const FL_NAME_CAP: usize = 16;

/// "更换字体"页(系统美化的二级页)的 page_id。删除流水线改完清单要重建这一页,
/// 它是一个独立注册页(page.rs / ipc.rs 里的 page5)。
pub(crate) const FONT_PID: u32 = 5;

/// 槽号(1..8), 与 index.txt 每行前两位十进制一致
static mut FL_ID: [u32; FL_MAX] = [0; FL_MAX];
/// 短名(UTF-8, 最多 12 字节 + NUL)。**允许非 ASCII** —— 短名要能写中文(如"文楷")。
/// 图标那条线是纯 ASCII, 这里是唯一放宽的地方: 名字门只挡控制字符(< 0x20 与 0x7F)。
static mut FL_NAMES: [[u8; FL_NAME_CAP]; FL_MAX] = [[0; FL_NAME_CAP]; FL_MAX];
/// 清单里有几个字体
static mut FL_N: u32 = 0;
/// 1 = 清单已读过(每次进页强制重读, 见 refresh)
static mut FL_LOADED: u32 = 0;
/// 列表刷新用的静态文本缓冲 + 行句柄绑定(与图标页同一套: 行控件会一直读这些指针)
const FL_ROW_CAP: usize = 48;
static mut FL_LINES: [[u8; FL_ROW_CAP]; FL_MAX] = [[0; FL_ROW_CAP]; FL_MAX];
static mut FL_SUBS: [[u8; 32]; FL_MAX] = [[0; 32]; FL_MAX];
static mut FL_ROWS: *mut u32 = core::ptr::null_mut();
static mut FL_ROWS_BASE: usize = 0;
static mut FL_ROWS_N: usize = 0;

// ===== 删除流水线(每拍一步) =====
static mut FL_DEL: u32 = 0;         // 待删除的槽号, 0 = 无
static mut FL_DEL_ST: u32 = 0;      // 0=空闲 1=改清单 2=删文件
static mut FL_DEL_K: u32 = 0;       // 删除进度(0 = 还没删, 1 = 文件已删)
// ===== 两步删除的"确认态" + 第三击的真系统确认框 =====
// 交互(与"桌面图标"页同一套): 点已选中的条目第一次变成"再点一次删除 X",
// 第二次弹系统消息框(lvx_page_msgbox, 与"确认重启？"同款); 勾 = 进删除流水线(在用中的
// 先恢复系统字体再删), 叉 = 取消。
// **框本身的状态机在 `confirm_pop`**(四条已验证的约束只有一份实现); 本页只留名字来源、
// 失败读数, 以及"关掉框要排一次整页重建"这条本页的收尾。
static mut FL_DELCONF: u32 = 0;     // 界面上"再点一次删除"的槽号, 0 = 无
static mut FL_MSG: u32 = 0;         // 提示行: 0=无 3=退出后生效 4=清单没写进 5=文件缺失 6=弹框没建出
static mut FL_MSG_AT: u32 = 0;      // 提示出现的拍号(到时自动清)
static mut FL_MSG_W: u32 = 0;       // 提示附带的读数(6 用的是建框失败分级码)
/// 提示行在屏幕上停留的拍数(50ms 一拍, 30 拍约 1.5 秒)。超时自动清 + 排一次重画。
const FL_MSG_HOLD: u32 = 30;
static mut FL_TICKS: u32 = 0;       // 本模块自己的拍计数(不借 STAT_TICK: 那个只在信息页递增)
/// 0 = 没有待办的界面更新, 1 = 有。点击、提示超时、删除流水线、应用收尾都只置这一位;
/// 由 tick 在 `window_is_quiet()` 那一拍把它变成一次**整页重建**
/// (不再原地 row_update, 也不再绕过窗口门直接 render_req —— 见 tick 第 1 步的说明)。
static mut FL_ROWS_REQ: u32 = 0;
/// 刚点下的槽号(临时勾选态)。live 追上之后它自然让位, 见 entry_selected。
static mut FL_SEL: u32 = 0;
/// 上一次看到的在用池位(变了就重建本页: 勾选态与"使用中"副标签要跟上 live)
static mut FL_LAST: u32 = 0;
/// 上一拍补写任务是否在跑(同上; 补写收工那一拍要把行9 的"补写中"文案换回来)
static mut FL_BF_RUN: u32 = 0;

// ---------------------------------------------------------------------------
// 清单: 读 / 改 / 写(只碰 index.txt 一个文件, 绝不遍历目录)
// ---------------------------------------------------------------------------

/// 名字是**非控制字符**就算合法(允许中文, 见 FL_NAMES 说明)。
#[inline(always)]
fn name_byte_ok(c: u8) -> bool { c >= 0x20 && c != 0x7f }

/// 解析 128 字节定长清单。坏行(槽号不是 1..8 或短名为空)直接跳过, 不猜。
unsafe fn fl_parse(buf: &[u8; FL_INDEX_SIZE], got: usize) {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < FL_MAX {
        let off = i * FL_LINE;
        if off + FL_LINE > got { break; }
        let d0 = buf[off];
        let d1 = buf[off + 1];
        i += 1;
        if !d0.is_ascii_digit() || !d1.is_ascii_digit() { continue; }
        let id = ((d0 - b'0') as u32) * 10 + (d1 - b'0') as u32;
        if id < 1 || id > FL_MAX as u32 { continue; }
        let mut ln = 0usize;
        let mut k = 3usize;
        while k < FL_LINE && ln < FL_NAME_CAP - 1 {
            let c = buf[off + k];
            if !name_byte_ok(c) { break; }
            let dst = core::ptr::addr_of_mut!(FL_NAMES[n]) as *mut u8;
            write_volatile(dst.add(ln), c);
            ln += 1;
            k += 1;
        }
        if ln == 0 { continue; }
        // 尾随空格 = 12 字节定长字段的右填充, 必须剥掉: 列表行宽看不出, 但确认框
        // 的居中文本会把一串空格暴露成大空隙(实测可见)。
        while ln > 0 {
            let dst = core::ptr::addr_of_mut!(FL_NAMES[n]) as *mut u8;
            if rd8(dst.add(ln - 1)) != b' ' { break; }
            ln -= 1;
        }
        if ln == 0 { continue; }
        st_wr!(FL_ID[n], id);
        write_volatile((core::ptr::addr_of_mut!(FL_NAMES[n]) as *mut u8).add(ln), 0);
        n += 1;
    }
    st_wr!(FL_N, n as u32);
}

/// 读清单。已读过就直接返回(开机后第一次进页/第一次点按钮时读)。
unsafe fn fl_load() {
    if st_rd!(FL_LOADED) != 0 { return; }
    st_wr!(FL_LOADED, 1);
    st_wr!(FL_N, 0);
    let fd = fw_api::open(FONT_INDEX.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 { return; }
    let mut buf = [0u8; FL_INDEX_SIZE];
    let mut got = 0usize;
    while got < FL_INDEX_SIZE {
        let n = fw_api::read(fd, buf.as_mut_ptr().add(got), (FL_INDEX_SIZE - got) as u32);
        if n <= 0 { break; }
        got += n as usize;
    }
    fw_api::close(fd);
    fl_parse(&buf, got);
}

/// 强制重读清单。投递包刚写进去的字体必须**不用重启**就能在界面上看到, 所以每次进页
/// 都重读一次(128 字节, 一次 open/read/close)。
pub(crate) unsafe fn refresh() {
    st_wr!(FL_LOADED, 0);
    fl_load();
}

/// 从内存清单里去掉一个槽, 返回是否真找到并去掉了。
unsafe fn fl_drop(slot: u32) -> bool {
    let n = st_rd!(FL_N) as usize;
    let mut out = 0usize;
    let mut found = false;
    let mut i = 0usize;
    while i < n {
        let id = st_rd!(FL_ID[i]);
        if id == slot { found = true; i += 1; continue; }
        if out != i {
            st_wr!(FL_ID[out], id);
            let src = core::ptr::addr_of!(FL_NAMES[i]) as *const u8;
            let dst = core::ptr::addr_of_mut!(FL_NAMES[out]) as *mut u8;
            let mut k = 0usize;
            while k < FL_NAME_CAP { write_volatile(dst.add(k), rd8(src.add(k))); k += 1; }
        }
        out += 1;
        i += 1;
    }
    // 清尾巴: 不清的话, 下次"整块写回"会把已经删掉的那几行原样写回去
    let mut j = out;
    while j < FL_MAX {
        st_wr!(FL_ID[j], 0);
        write_volatile(core::ptr::addr_of_mut!(FL_NAMES[j]) as *mut u8, 0);
        j += 1;
    }
    st_wr!(FL_N, out as u32);
    found
}

/// 把内存清单整块写回(恒 128 字节, 空槽 = 16 个空格)。
/// 为什么定长: 这样不需要 O_TRUNC、也不需要"先删文件再建" —— 写失败时旧文件还是完整的。
unsafe fn fl_write() -> bool {
    let mut buf = [0x20u8; FL_INDEX_SIZE];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < st_rd!(FL_N) as usize {
        let off = n * FL_LINE;
        let id = st_rd!(FL_ID[i]);
        buf[off] = b'0' + ((id / 10) % 10) as u8;
        buf[off + 1] = b'0' + (id % 10) as u8;
        buf[off + 2] = b' ';
        let src = core::ptr::addr_of!(FL_NAMES[i]) as *const u8;
        let mut k = 0usize;
        while k < FL_NAME_CAP - 1 {
            let c = rd8(src.add(k));
            if c == 0 { break; }
            buf[off + 3 + k] = c;
            k += 1;
        }
        buf[off + 15] = b'\n';
        n += 1;
        i += 1;
    }
    // O_WRONLY|O_CREAT = 6(与固件自带模块用的写组合一致; 内核层 O_RDONLY=1, 不是 POSIX)
    let fd = fw_api::open(FONT_INDEX.as_ptr(),
                          fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd < 0 { return false; }
    let mut wrote = 0usize;
    let mut ok = true;
    while wrote < FL_INDEX_SIZE {
        let w = fw_api::write(fd, buf.as_ptr().add(wrote), (FL_INDEX_SIZE - wrote) as u32);
        if w <= 0 { ok = false; break; }
        wrote += w as usize;
    }
    fw_api::close(fd);
    ok
}

/// 拼 `/data/chaos/font/st<n>.ttf` 到 dst。
/// 与 font_apply::FONT_PATHS / ipc.rs 的校验门必须逐字同形("池位号 = 文件名号")。
unsafe fn slot_path(dst: *mut u8, slot: u32) {
    let mut w = W::new(dst, 32);
    w.s(FONT_DIR).s(b"/st").n(slot).s(b".ttf");
    w.end();
}

/// 槽位对应的文件在不在(只 open/close, 不读内容)。
unsafe fn slot_file_ok(slot: u32) -> bool {
    let mut buf = [0u8; 32];
    slot_path(buf.as_mut_ptr(), slot);
    let fd = fw_api::open(buf.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 { false } else { fw_api::close(fd); true }
}

// ---------------------------------------------------------------------------
// 对外接口(更换字体页用): 与"桌面图标"页同一套做法
//   1. render: refresh() -> lines_fill() -> 动态建行 -> bind_rows + refresh_rows
//      (只有这一处**原地**写行: 页面上下文里, 行刚建好, 句柄必然有效)
//   2. 之后任何状态变化(切换选中、两步删除的确认态、提示行读数)都**只登记一次请求**
//      (置 FL_ROWS_REQ), 由 tick 在"没有跑批在动"的那一拍排一次整页重建 ——
//      不再原地 row_update。理由见 tick 里 FL_ROWS_REQ 那段与 font_apply::apply_finish。
//   3. 定时器侧(删除流水线)一概不直接碰对象, 只置 render_req(FONT_PID as usize)。
// ---------------------------------------------------------------------------

/// 清单里有几个字体(即页面上几条条目)
pub(crate) unsafe fn entry_count() -> u32 { st_rd!(FL_N) }

/// 第 e 个条目的槽号(越界返回 0)
pub(crate) unsafe fn entry_slot(e: u32) -> u32 {
    let i = e as usize;
    if i >= st_rd!(FL_N) as usize { return 0; }
    st_rd!(FL_ID[i])
}

/// 第 e 个条目是不是当前勾选的那一个(界面画勾用)。
/// 判据两处: **live**(真正在用的) + **pending**(刚点下、等下一拍落地的)。
/// 两者都要看 —— commit 不在点击回调里做, 点完到落地之间有几十毫秒,
/// 这段时间勾选必须已经跟过去了(否则看起来像没点上)。
pub(crate) unsafe fn entry_selected(e: u32) -> bool {
    let s = entry_slot(e);
    if s == 0 { return false; }
    s == font_apply::live_get() || s == font_apply::pending_get()
}

/// 本页要建几条行(= 清单里的字体数, 最多 FL_MAX)
pub(crate) unsafe fn entry_shown() -> u32 {
    let n = entry_count();
    if n > FL_MAX as u32 { FL_MAX as u32 } else { n }
}

/// 填条目行的两排文本: 主 = 字体短名(确认态则是"再点一次删除 X"), 副 = 槽位与在用状态。
/// 先整体清零再按需写(与图标页同款): 上一屏的长名字不会被下一屏读出来。
pub(crate) unsafe fn lines_fill() {
    let mut i = 0usize;
    while i < FL_MAX {
        write_volatile(core::ptr::addr_of_mut!(FL_LINES[i]) as *mut u8, 0);
        write_volatile(core::ptr::addr_of_mut!(FL_SUBS[i]) as *mut u8, 0);
        i += 1;
    }
    let total = st_rd!(FL_N);
    let live = font_apply::live_get();
    let mut row = 0usize;
    while row < FL_MAX {
        let e = row as u32;
        if e >= total { break; }
        let slot = st_rd!(FL_ID[row]);
        let np = core::ptr::addr_of!(FL_NAMES[row]) as *const u8;
        let mut nl = 0usize;
        while nl < FL_NAME_CAP - 1 && rd8(np.add(nl)) != 0 { nl += 1; }
        let lp = core::ptr::addr_of_mut!(FL_LINES[row]) as *mut u8;
        let mut w = W::new(lp, FL_ROW_CAP);
        // 两步删除的确认态(与图标页"再点一次删除 X"同款)
        if slot != 0 && slot == st_rd!(FL_DELCONF) {
            w.s("再点一次删除 ".as_bytes());
        }
        w.s(core::slice::from_raw_parts(np, nl));
        w.end();
        let sp = core::ptr::addr_of_mut!(FL_SUBS[row]) as *mut u8;
        let mut w = W::new(sp, 32);
        if slot == live {
            w.s("使用中".as_bytes());
        } else if !slot_file_ok(slot) {
            // 文件不在(清单与文件不一致): 点下去不会有效果, 但界面上必须说出来
            w.s("文件缺失".as_bytes());
        } else {
            w.s("槽位 ".as_bytes());
            w.n(slot);
        }
        // 提示行(只在挂着的那几拍显示, 到 FL_MSG_HOLD 自动清)。共用同一个位, 一种结局一句话:
        //   3 = "已选择, 退出应用后生效"(点选切换要等退场后落地, 界面上得说清
        //       为什么勾了却不马上换)
        //   4 = 删除时清单没写进去  5 = 字体文件缺失  6 = 弹框没建出来(带分级码)
        if st_rd!(FL_MSG) != 0 && row == 0 {
            if st_rd!(FL_MSG) == 3 {
                // 注意长度: FL_SUBS 每行只有 32 字节, 这句(含前导空格)28 字节, 不截断。
                // 写超了会被 W 截断成非法 UTF-8(半个汉字), 界面直接出乱码 ——
                // 这句从"退出应用后生效并自动补写"压到现在的长度就是为了避开它。
                w.s("  已选择, 退出后生效".as_bytes());
            } else if st_rd!(FL_MSG) == 4 {
                w.s("  删除失败, 清单写入未成功".as_bytes());
            } else if st_rd!(FL_MSG) == 5 {
                w.s("  字体文件缺失, 无法删除".as_bytes());
            } else if st_rd!(FL_MSG) == 6 {
                w.s("  弹框创建失败(".as_bytes());
                w.n(st_rd!(FL_MSG_W));
                w.s(")".as_bytes());
            }
        }
        w.end();
        row += 1;
    }
}

/// 第 row 行的主标签指针
pub(crate) unsafe fn line_primary(row: usize) -> *const u8 {
    core::ptr::addr_of!(FL_LINES[row]) as *const u8
}

/// 第 row 行的副标签指针(非 NULL => 行是双排, 与表盘页"名称 + ID"同形)
pub(crate) unsafe fn line_secondary(row: usize) -> *const u8 {
    core::ptr::addr_of!(FL_SUBS[row]) as *const u8
}

/// render 建完行之后把行句柄数组交给本模块(原地刷新要用)。
/// 只在页面上下文里调用 —— 页面被销毁后这些句柄就作废了(定时器比页面活得久)。
pub(crate) unsafe fn bind_rows(rows: *mut u32, base: usize, n: usize) {
    st_wr!(FL_ROWS, rows);
    st_wr!(FL_ROWS_BASE, base);
    st_wr!(FL_ROWS_N, n);
}

/// "可以不碰对象树"的窗口 —— 见 font_apply::apply_finish 的说明与固件逐字证据。
/// 两道跑批任何一个在动(分拍应用 / **补写任务**), 原地改行(或重建本页)就落进
/// "引擎正在动这棵树"的那一拍。
unsafe fn window_is_quiet() -> bool {
    font_apply::quiet() && !font_tree::backfill_running()
}

/// 原地写勾选态(表盘切换页 wf_apply_selection 同款): 只调 row_update 的第 6 参,
/// 非 0 = 复选框打勾。行文本一起重传, 免得确认态的文字留在行上。
unsafe fn apply_checkboxes(rows: *mut u32, base: usize, n: usize) {
    let upd: unsafe extern "C" fn(u32, *const u8, *const u8, *const u8, i32, u8) -> i32 =
        fw_api::row_update;
    let mut i = 0usize;
    while i < n {
        let op = read_volatile(rows.add(base + i));
        if op != 0 {
            // 本页只有一屏, 第 i 行就是第 i 个条目
            let sel = if entry_selected(i as u32) { 1u8 } else { 0u8 };
            upd(op, fw_api::NONE_PTR,
                core::ptr::addr_of!(FL_LINES[i]) as *const u8,
                core::ptr::addr_of!(FL_SUBS[i]) as *const u8, 0, sel);
        }
        i += 1;
    }
}

/// 原地刷新列表: 重填文本 + 重写勾选态。不销毁/重建任何对象(重建会把整页字体样式重抄一遍)。
/// **只给页面上下文用**(render 建行之后立刻画一次勾选态)。
/// tick 侧不再走这里 —— 见 mark_rows_dirty / mark_page_dirty 与 window_is_quiet 的说明。
pub(crate) unsafe fn refresh_rows() {
    if FL_ROWS.is_null() || FL_ROWS_N == 0 { return; }
    lines_fill();
    apply_checkboxes(FL_ROWS, FL_ROWS_BASE, FL_ROWS_N);
}

// ===== 第三击的真系统确认框(状态机在 confirm_pop, 这里只接本页的差异) =====

/// 第三击: 弹"删除「短名」?"。**必须在点击回调里当场建**(固件闹钟"连接断开"
/// 的弹框就是事件上下文里直接建的生产先例; 绕到 tick 安静拍去建, 门没开会整个无声吞掉)。
/// 短名从清单条目取(定长 12 字节字段的右填充已在读时剥掉, 否则居中文案里是一段大空隙
/// —— 实测可见)。失败分级码进 FL_MSG_W, 行 0 副标签显示"弹框创建失败(N)"。
unsafe fn popup_ask(slot: u32) {
    let mut np: *const u8 = core::ptr::null();
    let mut n = 0usize;
    let mut i = 0usize;
    while i < st_rd!(FL_N) as usize {
        if st_rd!(FL_ID[i]) == slot {
            np = core::ptr::addr_of!(FL_NAMES[i]) as *const u8;
            while n < FL_NAME_CAP - 1 && rd8(np.add(n)) != 0 { n += 1; }
            break;
        }
        i += 1;
    }
    let rc = confirm_pop::build(confirm_pop::SLOT_FONT, slot, np, n,
                                chaos_pop_no as *const () as u32,
                                chaos_pop_ok as *const () as u32);
    st_wr!(FL_MSG_W, rc);
    if rc != 0 {
        st_wr!(FL_MSG, 6);
        st_wr!(FL_MSG_AT, st_rd!(FL_TICKS));
        st_wr!(FL_DELCONF, 0);           // 没有框可确认就把确认态收回, 绝不静默删
    }
}

/// 叉/勾共同的处理: 关框交给 `confirm_pop`(它按页根子对象表判框还在不在, 门没过转兜底)。
/// 非 0 结局都要清确认态 + **排一次整页重建**: obj_delete 只失效框自己那一片, 被它盖住的
/// 标题栏那一层不在重画范围里, 不重建就冻成一片黑(实测)。勾才进删除流水线。
unsafe fn popup_clicked(event: u32, ok: bool) {
    if confirm_pop::click(confirm_pop::SLOT_FONT, event) == 0 { return; }
    st_wr!(FL_DELCONF, 0);
    if ok { request_delete(confirm_pop::target(confirm_pop::SLOT_FONT)); }
    st_wr!(FL_ROWS_REQ, 1);
}

/// 消息框按钮的事件入口(勾 = 确认删除 / 叉 = 取消; 事件码过滤同 chaos_row_dispatch)
#[no_mangle]
pub(crate) unsafe extern "C" fn chaos_pop_ok(event: u32) -> u32 { popup_clicked(event, true); 0 }
#[no_mangle]
pub(crate) unsafe extern "C" fn chaos_pop_no(event: u32) -> u32 { popup_clicked(event, false); 0 }

/// 新页实例边界(ui.rs 的 on_create)调用: 只丢状态不碰对象(这一路可能拿着上一页的句柄)。
pub(crate) unsafe fn popup_forget(pid: usize) {
    if pid != FONT_PID as usize { return; }
    confirm_pop::forget(confirm_pop::SLOT_FONT);
}

/// 建页开头(render 开头)调用: 兜掉上一轮漏删的框并归零状态(四条规矩只在 confirm_pop 模块头写一份)。
pub(crate) unsafe fn popup_reset() { confirm_pop::reset(confirm_pop::SLOT_FONT); }

/// 点第 row 行 —— 界面槽位在 page.rs 里映射过来
pub(crate) unsafe fn click_row(row: usize) {
    if row >= FL_MAX { return; }
    click_entry(row as u32);
}

/// 点某一条目。语义(与 icon_apply::click_entry 同一套交互):
///   未选中的条目 -> 切过去(commit + request, 与 0x30 门同一套);
///   已选中的条目 -> 第一次点变成"再点一次删除 X", 第二次点走删除流水线;
///   已在用中的那份(它就是 live) -> 只给提示("先切走")。在用的那份绝不删, 也不进入确认态。
/// 判"能不能删"一律以 **live**(font_apply 的当前池位)为准, 不以界面勾选为准 ——
/// 勾选在点完立刻跟上, 而 live 要等 0x30 那一套跑完才跟上, 这段时间里按勾选判
/// 会把"其实还在用"的那份判成可删。request_delete 里还有第二道同样的门。
///
/// 重要: 本函数**在固件的事件派发上下文里**跑,
/// 所以这里一概**不碰 LVGL 对象** —— 只改静态量, 把"界面要更新"记成 FL_ROWS_REQ,
/// 由 font_list::tick 在"没有跑批在动"的那一拍排一次**整页重建**(不再原地 row_update)。
/// (icon_apply 在点击回调里直接 refresh_rows 是因为它那边没有字体脸的回退扫描
/// 0x0CA666D8; 字体行有, 就是那条崩溃轴, 不能照抄这一条。)
unsafe fn click_entry(e: u32) {
    let slot = entry_slot(e);
    if slot == 0 { return; }
    // 已选中的(在用中的那份永远算已选中): 两步删除的确认态(与图标页同款)。
    // 第二击 = 行文字变"再点一次删除 X"; 第三击 = 当场弹系统确认框, 勾才进流水线。
    if entry_selected(e) {
        if st_rd!(FL_DELCONF) == slot {
            // 第三击: **当场**弹系统确认框 —— 闹钟"连接断开"弹框就是事件上下文直接建的
            // 生产先例(0x0C53D58C), 不绕 tick 安静拍(那扇门没开就整个无声吞掉,
            // 表现就是"点了没反应")。建失败给可见提示, 确认态收回。
            st_wr!(FL_MSG, 0);
            popup_ask(slot);
        } else {
            st_wr!(FL_DELCONF, slot);
            st_wr!(FL_MSG, 0);
        }
        click_feedback();
        return;
    }
    // 换了目标: 确认态作废(弹框是模态的, 有框在的时候点不到别的行, 走到这里必然没框)。
    st_wr!(FL_DELCONF, 0);
    st_wr!(FL_MSG, 0);
    // **不在事件派发链里改字体管理器身份** —— 这里只记下选择。
    // 落地时机还要再退一步(实测: 消掉全部原地动作之后点选**仍崩**):
    // 应用在前台时 pending_tick 不落地, 等 nav_back 退出应用之后才 commit + 应用,
    // 整条链与投递包那条从不崩的路完全同形。所以要在这里就说明
    // "为什么勾了却不马上换"(提示行 3)。
    if font_apply::busy() { click_feedback(); return; }   // 上一次还在跑: 不做半途切换
    st_wr!(FL_SEL, slot);                              // 勾选立刻跟过去(原地刷新即可)
    font_apply::set_pending(slot);
    st_wr!(FL_MSG, 3);
    st_wr!(FL_MSG_AT, st_rd!(FL_TICKS));
    click_feedback();
}

/// 点击反馈(图标页同款): 安静拍**原地刷新行**, 不再排整页重建。
/// 根因(症状是"连点条目就退回上一界面"): 每次点击都排一次 render_req 整页重建,
/// 重建期间旧行对象销毁/新行重建, 连续快速点击正好砸进这个窗口 —— 固件在半死对象上
/// 派发事件, 崩溃被页面框架回收 = 表现为"返回"。图标页点击后原地 refresh_rows 从不
/// 重建整页, 所以没有这个症状。字体行的 row_update 危险只在**跑批窗口**
/// (0x0CA666D8 回退扫描撞上建脸/写样式), 安静拍做 = 图标页已验证的同等安全条件;
/// 不安静时照旧延后(登记 FL_ROWS_REQ, tick 慢拍重建)。
/// set_pending 不再自带 mark_rows_dirty(那是每次点选都触发整页重建的另一半)。
unsafe fn click_feedback() {
    if window_is_quiet() {
        refresh_rows();
    } else {
        st_wr!(FL_ROWS_REQ, 1);
    }
}

/// 界面请求删除(两步确认之后才会走到这里): 只置状态位, 真正的活在 tick 里逐拍做。
/// 门(第二道, 界面提示不是门): 槽号合法 / 文件在 / **不是当前在用的那一份**。
/// 删的正好是刚点过切换、还没落地的那份的话, 把未落地的切换请求一并撤掉 —— 文件都删了,
/// 不能在退场时再应用它。
pub(crate) unsafe fn request_delete(slot: u32) {
    if slot < 1 || slot > FL_MAX as u32 { return; }
    if !slot_file_ok(slot) {
        // 文件不在 = 无从删起, 要给可见反馈, 不能无声吞掉
        st_wr!(FL_MSG, 5);
        st_wr!(FL_MSG_AT, st_rd!(FL_TICKS));
        click_feedback();
        return;
    }
    st_wr!(FL_DELCONF, 0);
    if st_rd!(FL_SEL) == slot { st_wr!(FL_SEL, 0); }
    font_apply::cancel_pending(slot);
    st_wr!(FL_DEL, slot);
    // 在用中的那份: 先把样式写回系统字体(face 换回开机原字, 我们的文件
    // 不再被引用), 之后才允许删文件 —— "face 按需回读文件"红线由恢复步骤解除。
    st_wr!(FL_DEL_ST, if slot == font_apply::live_get() { 10 } else { 1 });
}

/// 删除流水线(每拍一步)。两步:
///   1 清单里去掉这一行整块写回 —— **写失败就中止**(清单没改成就绝不删文件,
///     否则会出现"文件没了、清单还列着"或反过来"清单没了、文件还在"的半截状态);
///   2 逐拍删文件(一拍一个)。清单是唯一来源, 所以文件删掉之后界面上它就不存在了。
unsafe fn delete_tick() {
    match st_rd!(FL_DEL_ST) {
        // 10 = 在用中的那份: 第一步先把样式恢复成系统字体(分拍应用队列)。
        10 => {
            font_apply::request_revert();
            st_wr!(FL_DEL_ST, 11);
        }
        // 等恢复队列跑完(font_apply::tick 每拍推进; apply_finish 清 FA_REVERT)。
        11 => {
            if font_apply::revert_busy() != 0 { return; }
            st_wr!(FL_DEL_ST, 1);
        }
        1 => {
            refresh();                             // 以磁盘上的清单为准(投递包可能刚改过)
            fl_drop(st_rd!(FL_DEL));
            if !fl_write() {
                st_wr!(FL_LOADED, 0);              // 内存清单作废, 下次重读磁盘
                st_wr!(FL_DEL_ST, 0);
                st_wr!(FL_DEL, 0);
                st_wr!(FL_MSG, 4);                 // 失败必须可见, 不能无声吞掉
                st_wr!(FL_MSG_AT, st_rd!(FL_TICKS));
                st_wr!(FL_ROWS_REQ, 1);            // 重建统一走 tick 的窗口门
                return;
            }
            st_wr!(FL_LOADED, 1);                  // 内存清单 == 刚写下去的磁盘内容
            st_wr!(FL_DEL_K, 0);
            st_wr!(FL_DEL_ST, 2);
            st_wr!(FL_ROWS_REQ, 1);                // 列表立刻少一项
        }
        2 => {
            if st_rd!(FL_DEL_K) == 0 {
                let mut p = [0u8; 32];
                slot_path(p.as_mut_ptr(), st_rd!(FL_DEL));
                fw_api::fs_remove(p.as_ptr());
                st_wr!(FL_DEL_K, 1);
                return;
            }
            if st_rd!(FL_DEL) == font_apply::live_get() {
                font_apply::clear_live();      // 在用的那份删掉了, 池位归零
            }
            st_wr!(FL_DEL_ST, 0);
            st_wr!(FL_DEL, 0);
            st_wr!(FL_ROWS_REQ, 1);
        }
        _ => {}
    }
}

/// 应用收尾专用(font_apply 在队列跑完那一拍调): 本页要整体重建一次。
/// 与直接置 `FL_ROWS_REQ` 的区别只是这里顺手把"池位变了"记平 —— 不同步 FL_LAST 的话,
/// 下面的 live 变化检测会再登记一次请求, 于是同一秒内重建两遍(观感是闪两下)。
/// 真正排 render_req 仍然只在 tick 的窗口门里发生(分拍应用与补写都在门里让路)。
pub(crate) unsafe fn mark_page_dirty() {
    st_wr!(FL_LAST, font_apply::live_get());
    st_wr!(FL_ROWS_REQ, 1);
}

/// 有没有我们这一路的活在推进(供节拍自适应用): 待重画请求、删除流水线、挂着的提示行。
pub(crate) unsafe fn pending() -> bool {
    st_rd!(FL_ROWS_REQ) != 0 || st_rd!(FL_DEL_ST) != 0 || st_rd!(FL_MSG) != 0
}

/// 50ms UI tick 里调用: 推进删除流水线 + 到点自动清掉那句提示 + 消费"界面要更新"请求。
/// 请求的消费方式是**排一次整页重建**, 不是原地改行(理由见下面 FL_ROWS_REQ 那段)。
/// 与字体应用(font_apply::tick 分拍建脸)是两条独立的事, 各走各的状态位。
pub(crate) unsafe fn tick() {
    st_wr!(FL_TICKS, st_rd!(FL_TICKS).wrapping_add(1));
    // 0) 删除确认框: 弹框在点击回调里当场做, 这里只剩**兜底销毁** ——
    //    勾/叉那一下没过关(页已经把框带走/句柄对不上)才转到这里, 安静拍补一刀。
    if page_is_live(FONT_PID as usize) && window_is_quiet()
        && confirm_pop::close_deferred(confirm_pop::SLOT_FONT) {
        st_wr!(FL_ROWS_REQ, 1);
    }
    // 补写任务"从在跑变成停了"的那一拍: 行9 的文案要换回来("补写中" -> "补写一遍")。
    if st_rd!(FL_BF_RUN) == 0 && font_tree::backfill_running() {
        st_wr!(FL_BF_RUN, 1);
    } else if st_rd!(FL_BF_RUN) != 0 && !font_tree::backfill_running() {
        st_wr!(FL_BF_RUN, 0);
        st_wr!(FL_ROWS_REQ, 1);
    }
    // 点击/收尾/提示超时/删除流水线留下的"界面要更新"请求 —— **不在这里原地 row_update**,
    // 统一只在窗口开着的那一拍排一次整页重建。为什么不能原地改行: 见 font_apply::apply_finish
    // 的说明(row_update 的真实代价), 同一份证据不在这里重抄。
    // 观感代价明确: 点完到勾选跟上 ≈ 应用剩余时长(1~2 秒), 期间不做原地闪烁。
    if st_rd!(FL_ROWS_REQ) != 0 {
        if !page_is_live(FONT_PID as usize) {
            st_wr!(FL_ROWS_REQ, 0);        // 页不在了: 下次进页本来就会重画, 请求没有意义
        } else if confirm_pop::shown(confirm_pop::SLOT_FONT) {
            // 框还挂着(显示中, 或已请求关闭但门没过在等兜底): 等它关掉再重建 ——
            // 重建开头会把没删掉的框兜掉, 那等于把正对着的确认框变没。
            // 框是全屏模态, 这期间能点的只有框上的叉/勾, 不会有别的请求积压。
        } else if window_is_quiet() {
            st_wr!(FL_ROWS_REQ, 0);
            render_req(FONT_PID as usize);
        }
    }
    // 在用的池位变了: 只登记请求(原来是立刻 row_update ×N, 正撞在分拍窗口里)。
    let lv = font_apply::live_get();
    if lv != st_rd!(FL_LAST) {
        st_wr!(FL_LAST, lv);
        if st_rd!(FL_SEL) == lv { st_wr!(FL_SEL, 0); }   // live 追上来了, 临时勾选让位
        st_wr!(FL_ROWS_REQ, 1);
    }
    if st_rd!(FL_DEL_ST) != 0 {
        delete_tick();
        return;
    }
    // 提示只在屏幕上留一会儿(FL_MSG_HOLD 拍): 不清理的话它会一直挂在副标签上,
    // 而且下次进页重读清单也会带着它。重画同样只登记请求, 由窗口门决定哪一拍做。
    if st_rd!(FL_MSG) != 0 {
        let now = st_rd!(FL_TICKS);
        if now.wrapping_sub(st_rd!(FL_MSG_AT)) > FL_MSG_HOLD {
            st_wr!(FL_MSG, 0);
            st_wr!(FL_ROWS_REQ, 1);
        }
    }
}
