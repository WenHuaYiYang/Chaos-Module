// 系统桌面图标更换 —— 多图标包共存。
//
// 机制(逐字反汇编，静态核对)：
//   1. 应用注册表是带哨兵的循环链表：head = *(0x200EB640)，节点靠 +0x04 成环，
//      节点 +0x0C = 图标路径串、+0x10 u16 = app_id(app_lookup 0x0CA69934 逐字)。
//   2. 桌面记录(0x200D0CBC 链)的 +0x0C 是建行时**从注册表节点抄过去的裸指针**
//      (get_ctx_cb 0x0C513748 `ldr r6,[r4,#0xc]` -> 0x0C513770 `str r6,[r5,#0xc]`)，
//      最终由 0x0C5177CE `ldr r1,[r5,#0xc]` -> `bl 0x0C587FA0` 喂给图像控件。
//      所以改注册表节点才是持久的：桌面每次重建记录都会重读到我们的路径。
//   3. 改完调 launcher_refresh_app(app_id)(0x0C513BC0)：它重读 desc+0x0C 进记录、
//      释放旧图片对象重新加载、最后 rebuild_all，桌面立即可见。
//
// 多图标包: 素材按包分目录放, 清单文件列出有哪些包。
//   /data/chaos/icons/index.txt   恒 128 字节 = 8 行 x 16 字节, 每行 "<2位包号> <12字节短名>"
//   /data/chaos/icons/<包号>/<stem>.bin
// 清单只有 8 个位置(界面每页 3 个, 三页刚好用完); 包号 0 保留给"系统原图标"。
// **刻意不遍历目录**: 实测 procfs 目录遍历会死锁, 所以清单是唯一来源。
// 定长记录是为了"改写清单"永远是整块写 128 字节, 不需要先删文件也不需要 O_TRUNC,
// 写失败时旧文件也还是完整的 —— 固件自带模块用的同样是这种固定记录写法。
//
// 三条硬约束：
//   A. 节点 +0x0C 归注册表所有：app_install 用 str_dup_x 分配(0x0CA6A388..390)，
//      注销路径再 free(0x0CA6A48C..490)。所以我们写进去的指针必须是 fw_str_dup
//      的堆指针 —— 写 .rodata/.bss 地址等于让固件将来对只读段调 free = 堆损坏。
//   B. 换指针前先 fw_img_free_by_path(旧路径)：LVGL 图像缓存按路径常驻，不释放
//      就是 38 张 x 约 50KB 的旧位图留在堆里。顺序照抄固件自己那处
//      (hidden_and_show 0x0C51421E 读当前值 -> 0x0C514222 释放，再改)。
//   C. 不缓存节点指针。队列里只存 (stem 槽号, app_id)，每 tick 重新遍历注册表现取，
//      并二次核对 app_id 与路径 —— 批量跨多个 tick，期间万一有应用被卸载，
//      缓存的节点地址就是悬空指针。
//
// 多包的第四条(由多包切换引入):
//   D. 节点 +0x0C 上的堆串只有两种来源, 靠**路径前缀**分辨: `/data/chaos/icons` 开头的是
//      我们自己 strdup 的(切包时要 free), 其余是固件注册表自己的(绝不能 free)。
//      原路径(IC_SAVED)只在第一次换指针时记一次 —— 多包之间来回切时, 第二次写进去的
//      "旧路径"其实是我们自己上一包留下的堆串, 拿它覆盖 IC_SAVED 就再也回不去系统图标。

use crate::mem::{rd16, rd32, rd8};
use crate::*;

/// 素材 stem 表，与固件桌面 launcher 应用图标去重后的全集一一对应(38 个 stem)。
/// 在位素材来自哪个包由投递包决定: 素材池 assets/delta_lvgl 是 38 张全在位的那一套,
/// assets/fluent_lvgl 是存档(33 张, 该池没有运动/血氧/女性健康/米家 的图形
/// => 换过去会有 5 张读不到)。每张 112x112、cf=0x10、50188B。
/// 日历是**已知不覆盖**的一张, 不是漏配: 它的磁贴 = 固件自绘日期 + 一张**白色圆盘底**
/// (`/resource/app/perpetual_calendar/calendar_background_icon.bin`; 实测均值
/// R236 G241 B245、逐行不透明宽度 35..101..29 对称 => 圆盘, 内容框 101x100 四边 6px),
/// 而这条路径是 launcher 代码里的 flash 字面量(0x0CB941A4), **不走注册表节点 +0x0C**
/// => 本机制改不到它。曾按别名补过第 39 张试, 实测日历仍没换, 已撤。
/// 注意: 日期两枚文字是画在这张白底上的(所以字色按浅底配), 换深色底图会让日期读不出来。
const ICON_STEMS: [&[u8]; IC_STEM_N] = [
    b"activities", b"aivs", b"alarm", b"alipay",
    b"breath", b"calendar", b"camera", b"card",
    b"chronograph", b"compass", b"dealt", b"findphone",
    b"flashlight", b"heartrate", b"innovation_research", b"interconnect",
    b"mijia", b"music", b"mute", b"oxygen",
    b"perpetual_calendar", b"pressure", b"recorder", b"settings",
    b"share", b"sleep", b"sports", b"sports_course",
    b"sports_record", b"sports_status", b"timer", b"todo",
    b"tomato_clock", b"vitality", b"weather", b"womenhealth",
    b"worldclock", b"wxpay",
];

/// 素材条数(= 固件桌面 launcher 应用图标全集)
const IC_STEM_N: usize = 38;

/// 图标投递根目录(**不带**结尾斜杠: 目录名与路径前缀两种拼法都要用)
const ICON_DIR: &[u8] = b"/data/chaos/icons";
/// 包清单(定长记录)
const ICON_INDEX: &[u8] = b"/data/chaos/icons/index.txt\0";
/// 清单一行的字节数
const IC_LINE: usize = 16;          // 定长记录: [2 位包号][空格][12 字节短名][换行]
/// 短名在记录里的起点与字段宽度(与字体清单 `font_list::fl_write` 逐字节同一套格式)
const IC_NAME_AT: usize = 3;
const IC_NAME_BYTES: usize = 12;
/// 清单最多几个包(界面每页 3 个, 三页用完)
const IC_PACK_MAX: usize = 8;
/// 清单文件恒长
const IC_INDEX_SIZE: usize = IC_LINE * IC_PACK_MAX;
/// 单条路径槽字节数：目录 18 + 包号 2 + '/' 1 + 最长 stem 19 + ".bin" 4 + NUL = 45, 留到 56
const ICON_SLOT: usize = 56;
/// 行文本缓冲容量(界面行: 前缀 4 + 短名 12 + "再点一次删除 " 21 + 余量)
const IC_ROW_CAP: usize = 48;

/// 拼好的完整路径(.bss 常驻，是 fw_str_dup 的源)。
static mut ICON_FILES: [[u8; ICON_SLOT]; IC_STEM_N] = [[0; ICON_SLOT]; IC_STEM_N];

// ===== 包清单(只有 index.txt 一个来源, 不遍历目录) =====
/// 包号, 与 index.txt 每行前两位十进制一致
static mut IC_PACK_ID: [u32; IC_PACK_MAX] = [0; IC_PACK_MAX];
/// 短名(ASCII, 最多 12 字节 + NUL)
static mut IC_PACKS: [[u8; 16]; IC_PACK_MAX] = [[0; 16]; IC_PACK_MAX];
/// 清单里有几个包
static mut IC_PACK_N: u32 = 0;
/// 1 = 清单已读过(界面上每次进页会强制重读, 见 refresh_packs)
static mut IC_LOADED: u32 = 0;

// ===== 状态(RAM 无痕，重启即回原图标) =====
static mut IC_BUILT: u32 = 0;       // 1 = 路径串已按 IC_PACK 拼好
static mut IC_PACK: u32 = 0;        // 当前选中的包号, 0 = 系统原图标
static mut IC_REQ: u32 = 0;         // 0=无 1=应用 IC_PACK 2=恢复原图标
/// 0=原图标 1=已应用 2=正在应用 3=正在恢复
static mut IC_STATE: u32 = 0;
static mut IC_PLAN: [u32; IC_STEM_N] = [0; IC_STEM_N];      // 待处理 stem 槽号
static mut IC_PLAN_ID: [u32; IC_STEM_N] = [0; IC_STEM_N];   // 对应的 app_id
static mut IC_N: u32 = 0;
static mut IC_CURSOR: u32 = 0;
static mut IC_SAVED: [u32; IC_STEM_N] = [0; IC_STEM_N];     // 原路径指针(本次开机内可恢复)

// ===== 删除流水线(每拍一步, 删当前包前先把注册表放回系统原图标) =====
static mut IC_DEL: u32 = 0;         // 待删除的包号, 0 = 无
static mut IC_DEL_ST: u32 = 0;      // 0=空闲 1=可能要恢复 2=等恢复跑完 3=改清单 4=删文件
static mut IC_DEL_K: u32 = 0;       // 删除进度(第几个 stem; == IC_STEM_N 时删空目录)
static mut IC_DELCONF: u32 = 0;     // 界面上"再点一次删除"的包号, 0 = 无

// ===== 删除的真系统确认框: 状态机在 confirm_pop, 这里只接本页的差异 =====
// 四条已验证的规矩(parent = 页根 / 点击回调当场建 / 关框真删对象 / 存活门走页根子对象表)
// 只有一份实现, 依据见 `confirm_pop.rs` 文件头。
// 本页留下两件事, 因为它们本来就是各页自己的东西:
//   * **关过框必须排一次整页重建**: obj_delete 只失效框自己那一片, 被它盖住的
//     标题栏那一层不在重画范围里, 不重建就冻成一片黑;
//   * **什么时候才允许动对象树**: 本页的热窗口是 38 张逐拍换的跑批(IC_STATE 2|3),
//     与字体页的 `window_is_quiet()` 不是同一个判据。
static mut IP_POP_DIRT: u32 = 0;    // 1 = 关过框, 等一拍没跑批的时机排整页重建

// 注意: IC_STATE / IC_SAVED / IC_PLAN 这些是驱动跑批的**逻辑量**, 不用于界面显示。

// ===== 界面列表(动态建行, 与表盘切换页同一套做法) =====
/// 图标列表最多几条。条目 = 1(系统原图标) + 最多 8 个包(清单只有 8 行位置) = 9,
/// 给 10 个槽位 => **永远一屏装得下**, 不需要翻页, 也就没有"更多"行。
const IC_ENTRY_MAX: usize = 10;

/// 桌面图标页(系统美化的二级页)的 page_id。删除流水线改完清单要重建这一页,
/// 它是独立注册页(page.rs / ipc.rs 里的 page6), 有自己的 page_goto/page_back。
pub(crate) const ICON_PID: u32 = 6;
/// 条目行文本缓冲: 主标签(包名 / "再点一次删除 X") 与副标签(包号 / 说明)。
/// 静态缓冲的原因与表盘页 WATCH_LINES/WATCH_SUB 一样: 行控件的文本指针必须一直有效。
static mut IC_LINES: [[u8; IC_ROW_CAP]; IC_ENTRY_MAX] = [[0; IC_ROW_CAP]; IC_ENTRY_MAX];
static mut IC_SUBS: [[u8; 32]; IC_ENTRY_MAX] = [[0; 32]; IC_ENTRY_MAX];
/// 本页条目行的句柄数组首地址(ctx.rows)与起始槽位/行数 —— 原地刷新勾选态要用。
/// 只在 render 里绑定(那时页面确定活着), 定时器侧一律走 render_req, 不直接碰对象。
static mut IC_ROWS: *mut u32 = core::ptr::null_mut();
static mut IC_ROWS_BASE: usize = 0;
static mut IC_ROWS_N: usize = 0;

/// 注册表遍历上限(内置应用几十个，留足余量；触顶即停，绝不无界走)。
const IC_WALK_MAX: u32 = 512;


/// 桌面记录链此刻在不在位。不在位时 `launcher_refresh_app` 会静默早退(0x0C513BD0),
/// 所以我们照样换指针、但不假装刷新成功: 指针已经换到我们的素材, 下一次进桌面重建
/// 记录时会读到新路径, 只是当前这一屏不会立刻变。
unsafe fn desktop_live() -> bool { fw_api::fw_list_head(fw_api::LAUNCHER_DESKTOP_LIST) != 0 }

// ---------------------------------------------------------------------------
// 包清单: 读 / 改 / 写(只碰 index.txt 一个文件, 绝不遍历目录)
// ---------------------------------------------------------------------------

/// 解析 128 字节定长清单。坏行(包号不是 1..8 或短名为空)直接跳过, 不猜。
unsafe fn icon_packs_parse(buf: &[u8; IC_INDEX_SIZE], got: usize) {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < IC_PACK_MAX {
        let off = i * IC_LINE;
        if off + IC_LINE > got { break; }
        let d0 = buf[off];
        let d1 = buf[off + 1];
        i += 1;
        if !d0.is_ascii_digit() || !d1.is_ascii_digit() { continue; }
        let id = ((d0 - b'0') as u32) * 10 + (d1 - b'0') as u32;
        if id < 1 || id > IC_PACK_MAX as u32 { continue; }
        // 短名: 第 4..15 字节, 到第一个非可打印字符为止
        let mut name = [0u8; 16];
        let mut ln = 0usize;
        let mut k = 3usize;
        while k < IC_LINE && ln < 15 {
            let c = buf[off + k];
            if c < 0x21 || c >= 0x7f { break; }
            name[ln] = c;
            ln += 1;
            k += 1;
        }
        if ln == 0 { continue; }
        st_wr!(IC_PACK_ID[n], id);
        let dst = core::ptr::addr_of_mut!(IC_PACKS[n]) as *mut u8;
        let mut j = 0usize;
        while j < ln { write_volatile(dst.add(j), name[j]); j += 1; }
        write_volatile(dst.add(ln), 0);
        n += 1;
    }
    st_wr!(IC_PACK_N, n as u32);
}

/// 读清单。已读过就直接返回(开机后第一次进页/第一次点按钮时读)。
unsafe fn icon_packs_load() {
    if st_rd!(IC_LOADED) != 0 { return; }
    st_wr!(IC_LOADED, 1);
    st_wr!(IC_PACK_N, 0);
    let fd = fw_api::open(ICON_INDEX.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 { return; }
    let mut buf = [0u8; IC_INDEX_SIZE];
    let mut got = 0usize;
    while got < IC_INDEX_SIZE {
        let n = fw_api::read(fd, buf.as_mut_ptr().add(got), (IC_INDEX_SIZE - got) as u32);
        if n <= 0 { break; }
        got += n as usize;
    }
    fw_api::close(fd);
    icon_packs_parse(&buf, got);
}

/// 强制重读清单。投递包刚写进去的包必须**不用重启**就能在界面上看到, 所以每次
/// 进美化页都重读一次(128 字节, 一次 open/read/close; index.txt 是普通文件,
/// 不是设备节点 —— 只有设备节点的 read 会在 UI 线程上阻塞到看门狗复位)。
pub(crate) unsafe fn refresh_packs() {
    st_wr!(IC_LOADED, 0);
    icon_packs_load();
}

/// 从内存清单里去掉一个包, 返回是否真找到并去掉了。
unsafe fn icon_index_drop(pack: u32) -> bool {
    let n = st_rd!(IC_PACK_N) as usize;
    let mut out = 0usize;
    let mut found = false;
    let mut i = 0usize;
    while i < n {
        let id = st_rd!(IC_PACK_ID[i]);
        if id == pack { found = true; i += 1; continue; }
        if out != i {
            st_wr!(IC_PACK_ID[out], id);
            let src = core::ptr::addr_of!(IC_PACKS[i]) as *const u8;
            let dst = core::ptr::addr_of_mut!(IC_PACKS[out]) as *mut u8;
            let mut k = 0usize;
            while k < 16 { write_volatile(dst.add(k), rd8(src.add(k) as *const u8)); k += 1; }
        }
        out += 1;
        i += 1;
    }
    // 清尾巴: 不清的话, 下次"整块写回"会把已经删掉的那几行原样写回去
    let mut j = out;
    while j < IC_PACK_MAX {
        st_wr!(IC_PACK_ID[j], 0);
        let dst = core::ptr::addr_of_mut!(IC_PACKS[j]) as *mut u8;
        write_volatile(dst, 0);
        j += 1;
    }
    st_wr!(IC_PACK_N, out as u32);
    found
}

/// 把内存清单整块写回(恒 128 字节, 空槽 = 16 个空格)。
/// 为什么定长: 这样不需要 O_TRUNC、也不需要"先删文件再建" —— 写失败时旧文件还是完整的。
unsafe fn icon_index_write() -> bool {
    let mut buf = [0x20u8; IC_INDEX_SIZE];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < st_rd!(IC_PACK_N) as usize {
        let off = n * IC_LINE;
        let id = st_rd!(IC_PACK_ID[i]);
        buf[off] = b'0' + ((id / 10) % 10) as u8;
        buf[off + 1] = b'0' + (id % 10) as u8;
        buf[off + 2] = b' ';
        let src = core::ptr::addr_of!(IC_PACKS[i]) as *const u8;
        let mut k = 0usize;
        while k < IC_NAME_BYTES {
            let c = rd8(src.add(k) as *const u8);
            if c == 0 { break; }
            buf[off + IC_NAME_AT + k] = c;
            k += 1;
        }
        buf[off + IC_LINE - 1] = b'\n';
        n += 1;
        i += 1;
    }
    // O_WRONLY|O_CREAT = 6(与固件自带模块的写组合一致; 内核层 O_RDONLY=1 不是 POSIX)
    let fd = fw_api::open(ICON_INDEX.as_ptr(),
                          fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd < 0 { return false; }
    let mut wrote = 0usize;
    let mut ok = true;
    while wrote < IC_INDEX_SIZE {
        let w = fw_api::write(fd, buf.as_ptr().add(wrote), (IC_INDEX_SIZE - wrote) as u32);
        if w <= 0 { ok = false; break; }
        wrote += w as usize;
    }
    fw_api::close(fd);
    ok
}

/// 第 pack 个包的短名指针(0 = 没有这个包)
unsafe fn pack_name_ptr(pack: u32) -> u32 {
    let mut i = 0usize;
    while i < st_rd!(IC_PACK_N) as usize {
        if st_rd!(IC_PACK_ID[i]) == pack {
            return core::ptr::addr_of!(IC_PACKS[i]) as u32;
        }
        i += 1;
    }
    0
}

// ---------------------------------------------------------------------------
// 路径串与字符串工具
// ---------------------------------------------------------------------------

/// 写 `/data/chaos/icons/<NN>` 到 dst, `slash` = 是否再补一个 '/'。返回写入长度。
unsafe fn icon_dir_of(dst: *mut u8, pack: u32, slash: bool) -> usize {
    let mut o = 0usize;
    let mut i = 0usize;
    while i < ICON_DIR.len() { write_volatile(dst.add(o), ICON_DIR[i]); o += 1; i += 1; }
    write_volatile(dst.add(o), b'/'); o += 1;
    write_volatile(dst.add(o), b'0' + ((pack / 10) % 10) as u8); o += 1;
    write_volatile(dst.add(o), b'0' + (pack % 10) as u8); o += 1;
    if slash { write_volatile(dst.add(o), b'/'); o += 1; }
    o
}

/// 拼 `/data/chaos/icons/<NN>/<stem>.bin` 到 dst(dst 至少 ICON_SLOT 字节)
unsafe fn path_build(dst: *mut u8, pack: u32, slot: usize) {
    let mut o = icon_dir_of(dst, pack, true);
    let s = ICON_STEMS[slot];
    let mut i = 0usize;
    while i < s.len() { write_volatile(dst.add(o), s[i]); o += 1; i += 1; }
    for c in b".bin" {
        if o < ICON_SLOT - 1 { write_volatile(dst.add(o), *c); o += 1; }
    }
    write_volatile(dst.add(o), 0);
}

unsafe fn icon_paths_build() {
    if st_rd!(IC_BUILT) != 0 { return; }
    st_wr!(IC_BUILT, 1);
    let pack = st_rd!(IC_PACK);
    let mut k = 0usize;
    while k < IC_STEM_N {
        let dst = core::ptr::addr_of_mut!(ICON_FILES[k]) as *mut u8;
        // 未选包(系统原图标)时槽位内容为空串: 任何用到路径的入口都会先看 IC_PACK,
        // 这里留空是为了让"忘了看"的代价是打不开文件, 而不是指向别的包的图。
        if pack < 1 || pack > IC_PACK_MAX as u32 {
            write_volatile(dst, 0);
        } else {
            path_build(dst, pack, k);
        }
        k += 1;
    }
}

/// 节点当前路径是否已经就是我们**这一包**的。按**字符串**比, 不比指针 —— 我们写进节点的
/// 是堆上副本(约束 A), 与 .bss 里的 ICON_FILES 地址永远不同, 比指针会把"已应用"判成
/// "未应用", 于是每次点按钮都会再 strdup 一份(慢性泄漏)。
unsafe fn icon_path_is_ours(path: u32, slot: usize) -> bool {
    let a = path as *const u8;
    let b = core::ptr::addr_of!(ICON_FILES[slot]) as *const u8;
    let mut i = 0usize;
    while i < ICON_SLOT {
        let ca = rd8(a.add(i) as *const u8);
        if ca != rd8(b.add(i) as *const u8) { return false; }
        if ca == 0 { return true; }
        i += 1;
    }
    false
}

/// 路径是不是**我们任何一包**留下的(约束 D)。只看前缀, 所以切换包之后照样认得出
/// 上一包写进去的堆串。系统原路径 `/resource/...` 永远不匹配 => 绝不会误 free 固件的串。
unsafe fn icon_path_is_any_ours(path: u32) -> bool {
    if !plausible_ptr(path) { return false; }
    let a = path as *const u8;
    let mut i = 0usize;
    while i < ICON_DIR.len() {
        if rd8(a.add(i) as *const u8) != ICON_DIR[i] { return false; }
        i += 1;
    }
    true
}

unsafe fn icon_file_ok(slot: usize) -> bool {
    let p = core::ptr::addr_of!(ICON_FILES[slot]) as *const u8;
    let fd = fw_api::open(p, fw_api::oflag::RDONLY, 0);
    if fd < 0 { return false; }
    fw_api::close(fd);
    true
}

/// 第 pack 包在不在位(只查第一张 activities.bin)。界面用它把整包没投递的项在副标签上
/// 标出来, 每页只多做 3 次 open/close —— 38 张 x 8 包全查一遍在 UI 线程是不合适的。
unsafe fn pack_present(pack: u32) -> bool {
    if pack < 1 || pack > IC_PACK_MAX as u32 { return false; }
    let mut p = [0u8; ICON_SLOT];
    path_build(p.as_mut_ptr(), pack, 0);
    let fd = fw_api::open(p.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 { return false; }
    fw_api::close(fd);
    true
}

/// 路径里可当 stem 的两段: 文件名(去 ".bin") 与它的上一级目录名。
///
/// 为什么两段: 固件桌面图标路径有两种形态(覆盖调研已去重确认), 同一个应用可能
/// 走任一条 —— `/resource/app/launcher/<名>.bin` 与
/// `/resource/app/<名>/launcher.bin`(calendar / perpetual_calendar / weather /
/// sleep / breath / innovation_research 实测是后者)。只看文件名会漏掉这 6 个。
unsafe fn icon_stem_eq(path: *const u8, slot: usize) -> bool {
    let want = ICON_STEMS[slot];
    let mut buf = [0u8; 96];
    let mut n = 0usize;
    while n < 95 {
        let c = rd8((path as u32 + n as u32) as *const u8);
        if c == 0 { break; }
        if c < 0x20 || c >= 0x7f { return false; }
        buf[n] = c;
        n += 1;
    }
    if n == 0 { return false; }
    // 逐段切 '/'，取"最后一段"(去 .bin)与"倒数第二段"比较
    let mut seg1_len = 0usize;      // 最后一段长度
    let mut seg1_off = 0usize;
    let mut seg2_off = 0usize;      // 倒数第二段
    let mut seg2_len = 0usize;
    let mut start = 0usize;
    let mut i = 0usize;
    while i <= n {
        if i == n || buf[i] == b'/' {
            if i > start {
                seg2_off = seg1_off; seg2_len = seg1_len;
                seg1_off = start;    seg1_len = i - start;
            }
            start = i + 1;
        }
        i += 1;
    }
    if seg1_len > 4 && buf[seg1_off + seg1_len - 4] == b'.'
        && buf[seg1_off + seg1_len - 3] == b'b'
        && buf[seg1_off + seg1_len - 2] == b'i'
        && buf[seg1_off + seg1_len - 1] == b'n' { seg1_len -= 4; }
    (seg1_len == want.len() && icon_seg_is(buf.as_ptr(), seg1_off, seg1_len, want))
        || (seg2_len == want.len() && icon_seg_is(buf.as_ptr(), seg2_off, seg2_len, want))
}

fn icon_seg_is(buf: *const u8, off: usize, len: usize, want: &[u8]) -> bool {
    let mut i = 0usize;
    while i < len {
        if unsafe { *buf.add(off + i) } != want[i] { return false; }
        i += 1;
    }
    true
}

/// 遍历注册表找第 slot 个 stem 对应的节点，返回 (节点, app_id, 当前路径指针)。
/// 每次从头走，不缓存节点地址(文件头约束 C)。
unsafe fn icon_find(slot: usize) -> (u32, u32, u32) {
    let head = rd32(fw_api::APP_REGISTRY as *const u32);
    if !safe_ptr(head) { return (0, 0, 0); }
    let mut node = rd32((head + 4) as *const u32);
    let mut guard = 0u32;
    while safe_ptr(node) && node != head && guard < IC_WALK_MAX {
        guard += 1;
        let path = rd32((node + fw_api::APPN_ICON) as *const u32);
        if safe_ptr(path) && icon_stem_eq(path as *const u8, slot) {
            let id = rd16((node + fw_api::APPN_ID) as *const u16) as u32;
            return (node, id, path);
        }
        node = rd32((node + 4) as *const u32);
    }
    (0, 0, 0)
}

// ---------------------------------------------------------------------------
// 计划与执行(每 tick 推进一个应用：refresh 内部会 rebuild 桌面，不能连做)
// ---------------------------------------------------------------------------

unsafe fn plan_apply() {
    // 没选包(系统原图标)时没有可应用的素材: 状态留 0, 一个字节都不动
    if st_rd!(IC_PACK) == 0 {
        st_wr!(IC_STATE, 0);
        return;
    }
    icon_paths_build();
    st_wr!(IC_N, 0);
    st_wr!(IC_CURSOR, 0);
    let mut k = 0usize;
    let mut n = 0usize;
    while k < ICON_STEMS.len() {
        let (node, id, _) = icon_find(k);
        // 注册表里没有这个应用(无表) / 我们的文件不在位(缺件) => 都不动它。
        // 两类原因不分开计数, 但**都必须跳过**, 不能去指一个打不开的路径。
        if node != 0 && icon_file_ok(k) {
            st_wr!(IC_PLAN[n], k as u32);
            st_wr!(IC_PLAN_ID[n], id);
            n += 1;
        }
        k += 1;
    }
    st_wr!(IC_N, n as u32);
    // 重要: 一个都没排上(素材没投递, 38 条全跳过)时状态必须留在 0, 否则空跑一遍
    // 就把状态报成"已应用", 而注册表其实一个字节都没动 —— 界面会自己说谎。
    st_wr!(IC_STATE, if n > 0 { 2 } else { 0 });
    // 这里不再重建本页: 勾选态在 click_entry 里已经原地写过了, 跑批期间界面不需要动
    // (重建式渲染会顺手把整页的字体样式重抄一遍, 那一下整页字体会跳变)。
}

unsafe fn step_apply() {
    let c = st_rd!(IC_CURSOR) as usize;
    if c >= st_rd!(IC_N) as usize {
        st_wr!(IC_STATE, 1);
        return;
    }
    st_wr!(IC_CURSOR, (c + 1) as u32);
    let slot = st_rd!(IC_PLAN[c]) as usize;
    let (node, id, _path) = icon_find(slot);
    // 二次核对：节点还在、app_id 还是计划里那个(路径可信性交给 point_to_ours 现查)
    if node == 0 || id != st_rd!(IC_PLAN_ID[c]) {
        return;
    }
    point_to_ours(slot);
}

/// 把第 slot 个 stem 的注册表节点指向我们这一包的素材文件, 并让桌面立刻重读。
/// 幂等: 已经指向我们这一份就什么都不做(也不 strdup, 否则每次点都慢性漏一块堆)。
/// 返回 true = 这一次真换了指针。
unsafe fn point_to_ours(slot: usize) -> bool {
    let (node, id, path) = icon_find(slot);
    if node == 0 || path == 0 { return false; }
    if icon_path_is_ours(path, slot) { return false; }       // 已是这一包(幂等)
    let saved = st_rd!(IC_SAVED[slot]);
    let old_ours = icon_path_is_any_ours(path);
    if saved == 0 {
        // 节点指着我们的目录、却没有原路径记录 => 状态不可信(改回原路径也没得改)。
        // 正常流程不可能走到这里: 换指针时一定同时记下原路径。
        if old_ours { return false; }
        st_wr!(IC_SAVED[slot], path);       // 原路径只在第一次记(约束 D)
    }
    let ours = core::ptr::addr_of!(ICON_FILES[slot]) as u32;
    let newp = fw_api::fw_str_dup(ours as *const u8);
    if newp == 0 || !plausible_ptr(newp) {
        if saved == 0 { st_wr!(IC_SAVED[slot], 0); }
        return false;
    }
    fw_api::fw_img_free_by_path(path);                // 约束 B：先释放旧路径缓存
    write_volatile((node + fw_api::APPN_ICON) as *mut u32, newp);
    if rd32((node + fw_api::APPN_ICON) as *const u32) != newp {
        fw_api::fw_free(newp);
        if saved == 0 { st_wr!(IC_SAVED[slot], 0); }
        return false;
    }
    // 桌面记录不在位时不假装刷新成功: 指针已经换了(下次进桌面会重建记录读到新路径),
    // 只是当前界面不会变 —— 这就是"有时候点了没反应"的成因。
    if desktop_live() { fw_api::launcher_refresh_app(id); }
    // 回收上一包留下的堆串: 顺序必须在 refresh **之后** —— 在那之前桌面记录
    // (+0x0C)还引用着它(与 step_restore 同一条顺序)。
    if old_ours && path != st_rd!(IC_SAVED[slot]) { fw_api::fw_free(path); }
    true
}

unsafe fn plan_restore() {
    st_wr!(IC_N, 0);
    st_wr!(IC_CURSOR, 0);
    let mut k = 0usize;
    let mut n = 0usize;
    while k < ICON_STEMS.len() {
        if st_rd!(IC_SAVED[k]) != 0 {
            let (node, id, _) = icon_find(k);
            if node != 0 {
                st_wr!(IC_PLAN[n], k as u32);
                st_wr!(IC_PLAN_ID[n], id);
                n += 1;
            }
        }
        k += 1;
    }
    st_wr!(IC_N, n as u32);
    st_wr!(IC_STATE, if n > 0 { 3 } else { 0 });
}

unsafe fn step_restore() {
    let c = st_rd!(IC_CURSOR) as usize;
    if c >= st_rd!(IC_N) as usize {
        st_wr!(IC_STATE, 0);
        return;
    }
    st_wr!(IC_CURSOR, (c + 1) as u32);
    let slot = st_rd!(IC_PLAN[c]) as usize;
    let old = st_rd!(IC_SAVED[slot]);
    let (node, id, path) = icon_find(slot);
    if node == 0 || id != st_rd!(IC_PLAN_ID[c]) || old == 0 { return; }
    write_volatile((node + fw_api::APPN_ICON) as *mut u32, old);
    st_wr!(IC_SAVED[slot], 0);
    // 先刷新再回收：refresh 会把节点当前值(已是原路径)抄进记录并重建桌面, 我们那份
    // 到这一步才真正不被任何人引用 —— 顺序反过来会让桌面拿着已释放的字符串画图标。
    if desktop_live() { fw_api::launcher_refresh_app(id); }
    // 判定用**前缀**而不是"等于当前包的路径": 恢复时 IC_PACK 已经被置 0, 路径槽是空的,
    // 按当前包比会把上一包的堆串判成"不是我们的" => 每次恢复都漏 38 块堆。
    if path != 0 && path != old && plausible_ptr(path) && icon_path_is_any_ours(path) {
        fw_api::fw_img_free_by_path(path);
        fw_api::fw_free(path);
    }
}

// ---------------------------------------------------------------------------
// 删除一个包(界面两步确认之后才会走到这里)
// ---------------------------------------------------------------------------

/// 界面请求删除: 只置状态位, 真正的活在 tick 里逐拍做。
pub(crate) unsafe fn request_delete(pack: u32) {
    if pack < 1 || pack > IC_PACK_MAX as u32 { return; }
    st_wr!(IC_DELCONF, 0);
    st_wr!(IC_DEL, pack);
    st_wr!(IC_DEL_ST, 1);
}

/// 删除流水线(每拍一步)。四步:
///   1 删的是当前正在用的那一包 => 先把注册表放回系统原图标(否则桌面会拿着
///     马上要删掉的文件的路径去画图标);
///   2 等恢复跑完(38 拍);
///   3 清单里去掉这一行, 整块写回 —— **写失败就中止**, 不能出现"清单里没了、文件还在"
///     或反过来"文件删了、清单还列着"的半截状态;
///   4 逐拍删 38 个文件, 最后删只剩空壳的目录(不需要改名腾位置, 直接 remove 空目录)。
unsafe fn delete_tick() {
    match st_rd!(IC_DEL_ST) {
        1 => {
            if st_rd!(IC_PACK) == st_rd!(IC_DEL) {
                st_wr!(IC_PACK, 0);
                st_wr!(IC_BUILT, 0);
                plan_restore();
                st_wr!(IC_DEL_ST, 2);
            } else {
                st_wr!(IC_DEL_ST, 3);
            }
        }
        2 => {
            // 恢复批次跑完(或本来就没排上东西)才继续
            let s = st_rd!(IC_STATE);
            if s == 0 || s == 1 { st_wr!(IC_DEL_ST, 3); }
        }
        3 => {
            refresh_packs();                       // 以磁盘上的清单为准(投递包可能刚改过)
            icon_index_drop(st_rd!(IC_DEL));
            if !icon_index_write() {
                // 写不进去就把内存清单作废, 下次重读磁盘 —— 内存与磁盘必须一致。
                // 清单没改成就**不删文件**: 否则会出现"文件没了、清单还列着"的半截状态。
                st_wr!(IC_LOADED, 0);
                st_wr!(IC_DEL_ST, 0);
                st_wr!(IC_DEL, 0);
                render_req(ICON_PID as usize);
                return;
            }
            st_wr!(IC_LOADED, 1);                  // 内存清单 == 刚写下去的磁盘内容
            st_wr!(IC_DEL_K, 0);
            st_wr!(IC_DEL_ST, 4);
            render_req(ICON_PID as usize);                        // 列表立刻少一项
        }
        4 => {
            let k = st_rd!(IC_DEL_K) as usize;
            let pack = st_rd!(IC_DEL);
            let mut p = [0u8; ICON_SLOT];
            if k < IC_STEM_N {
                path_build(p.as_mut_ptr(), pack, k);
                fw_api::fs_remove(p.as_ptr());
                st_wr!(IC_DEL_K, (k + 1) as u32);
                return;
            }
            if k == IC_STEM_N {
                let n = icon_dir_of(p.as_mut_ptr(), pack, false);
                write_volatile(p.as_mut_ptr().add(n), 0);
                fw_api::fs_remove(p.as_ptr());     // 目录: remove() 对空目录退到 rmdir
                st_wr!(IC_DEL_K, (k + 1) as u32);
                return;
            }
            st_wr!(IC_DEL_ST, 0);
            st_wr!(IC_DEL, 0);
            render_req(ICON_PID as usize);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// 对外接口(美化页列表用)：与"表盘切换"页同一套做法
//   1. render: lines_fill() 填文本 -> 按需**动态建行**(有几条建几行) -> bind_rows + refresh_rows
//   2. 之后任何状态变化(切换选中、两步删除的确认态)只走 refresh_rows(): 重填文本 +
//      用 row_update 的第 6 参原地写勾选态, **不销毁/重建任何对象**(表盘页 wf_apply_selection 同款)
//   3. 定时器侧(跑批、删除流水线)一概不直接碰对象, 只置 render_req(ICON_PID as usize) 让框架重建
// ---------------------------------------------------------------------------

/// 条目总数 = 1(系统原图标) + 包数
pub(crate) unsafe fn entry_count() -> u32 { 1 + st_rd!(IC_PACK_N) }

/// 第 e 项的包号(0 = 系统原图标; 越界也返回 0)
pub(crate) unsafe fn entry_pack(e: u32) -> u32 {
    if e == 0 { return 0; }
    let i = (e - 1) as usize;
    if i >= st_rd!(IC_PACK_N) as usize { return 0; }
    st_rd!(IC_PACK_ID[i])
}

/// 第 e 项是否被选中(选中 = 注册表正指向它 / 它就是系统原图标)
pub(crate) unsafe fn entry_selected(e: u32) -> bool { st_rd!(IC_PACK) == entry_pack(e) }

/// 短名的真实长度(定长 16 字节 .bss 缓冲, 到 NUL 或第一个非可打印字节为止)。
/// 返回 0 = 不可信。名字本来自设备上的清单文件, 不当可信输入。
unsafe fn pack_name_len(p: *const u8) -> usize {
    if p.is_null() { return 0; }
    let mut n = 0usize;
    while n < 15 {
        let c = rd8(p.add(n));
        if c == 0 { break; }
        if c < 0x21 || c >= 0x7f { return 0; }
        n += 1;
    }
    n
}

/// 本页要建几条图标行(= 条目总数, 最多 IC_ENTRY_MAX)。
/// 条目 = 1(系统原图标) + 最多 8 个包 = 9 ≤ IC_ENTRY_MAX, 所以**永远一屏装得下**,
/// 不需要翻页(清单格式本身也只有 8 行位置)。
pub(crate) unsafe fn entry_shown() -> u32 {
    let total = entry_count();
    if total > IC_ENTRY_MAX as u32 { IC_ENTRY_MAX as u32 } else { total }
}

/// 填条目行的两排文本。先整体清零再按需写(与 refresh_watchface_lines 同款):
/// 上一屏的长名字不会被下一屏读出来。
/// 主标签 = 包名(两步删除的确认态则是"再点一次删除 X"), 副标签 = 包号 + 是否已投递。
pub(crate) unsafe fn lines_fill() {
    let mut i = 0usize;
    while i < IC_ENTRY_MAX {
        write_volatile(core::ptr::addr_of_mut!(IC_LINES[i]) as *mut u8, 0);
        write_volatile(core::ptr::addr_of_mut!(IC_SUBS[i]) as *mut u8, 0);
        i += 1;
    }
    let total = entry_count();
    let mut row = 0usize;
    while row < IC_ENTRY_MAX {
        let e = row as u32;
        if e >= total { break; }
        let pack = entry_pack(e);
        let p = pack_name_ptr(pack) as *const u8;
        let n = pack_name_len(p);
        let lp = core::ptr::addr_of_mut!(IC_LINES[row]) as *mut u8;
        let mut w = W::new(lp, IC_ROW_CAP);
        if pack != 0 && entry_selected(e) && st_rd!(IC_DELCONF) == pack {
            w.s("再点一次删除 ".as_bytes());
        }
        if pack == 0 { w.s("系统原图标".as_bytes()); }
        else if n > 0 { w.s(core::slice::from_raw_parts(p, n)); }
        w.end();
        let sp = core::ptr::addr_of_mut!(IC_SUBS[row]) as *mut u8;
        let mut w = W::new(sp, 32);
        if pack == 0 {
            w.s("固件原图标".as_bytes());
        } else {
            w.s("包 ".as_bytes());
            w.c(b'0' + ((pack / 10) % 10) as u8);
            w.c(b'0' + (pack % 10) as u8);
            // 整包没投递时直接在副标签上说清楚(点下去不会有反应, 但这个状态必须写在界面上)
            if !pack_present(pack) { w.s(" 未投递".as_bytes()); }
        }
        w.end();
        row += 1;
    }
}

/// 第 row 行的主标签指针(静态缓冲, 行控件会一直读它)
pub(crate) unsafe fn line_primary(row: usize) -> *const u8 {
    core::ptr::addr_of!(IC_LINES[row]) as *const u8
}

/// 第 row 行的副标签指针(非 NULL => 行是双排, 与表盘页的"名称 + ID"同形)
pub(crate) unsafe fn line_secondary(row: usize) -> *const u8 {
    core::ptr::addr_of!(IC_SUBS[row]) as *const u8
}

/// render 建完行之后把行句柄数组交给本模块(原地刷新要用)。
/// 只在页面上下文里调用 —— 页面被销毁后这些行句柄就作废了(碰对象前先过 page_is_live)。
pub(crate) unsafe fn bind_rows(rows: *mut u32, base: usize, n: usize) {
    st_wr!(IC_ROWS, rows);
    st_wr!(IC_ROWS_BASE, base);
    st_wr!(IC_ROWS_N, n);
}

/// 原地写勾选态(表盘页 wf_apply_selection 同款): 只调 row_update 的第 6 参,
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
                core::ptr::addr_of!(IC_LINES[i]) as *const u8,
                core::ptr::addr_of!(IC_SUBS[i]) as *const u8, 0, sel);
        }
        i += 1;
    }
}

/// 原地刷新列表: 重填文本 + 重写勾选态。不销毁/重建任何对象, 所以不会把这一页的
/// 字体样式重新抄一遍(重建式渲染会)。
pub(crate) unsafe fn refresh_rows() {
    if IC_ROWS.is_null() || IC_ROWS_N == 0 { return; }
    lines_fill();
    apply_checkboxes(IC_ROWS, IC_ROWS_BASE, IC_ROWS_N);
}

/// 点第 row 行(0..IC_ENTRY_MAX-1) —— 界面槽位在 page.rs 里映射过来
pub(crate) unsafe fn click_row(row: usize) {
    if row >= IC_ENTRY_MAX { return; }
    click_entry(row as u32);
}

/// 第三击(确认态已开): **当场**弹"删除「短名」?"。短名直接用行上那一份
/// (`pack_name_len` 已经挡过右填充与不可打印字节)。建不出来就把确认态收回 ——
/// 图标页没有消息行可显示分级码, 红线是绝不因为建框失败就静默删包。
unsafe fn popup_ask(pack: u32) {
    let np = pack_name_ptr(pack) as *const u8;
    let n = pack_name_len(np);
    if confirm_pop::build(confirm_pop::SLOT_ICON, pack, np, n,
                          chaos_ipop_no as *const () as u32,
                          chaos_ipop_ok as *const () as u32) != 0 {
        st_wr!(IC_DELCONF, 0);
    }
}

/// 叉/勾共同的处理: 关框交给 `confirm_pop`(它按页根子对象表判框还在不在, 门没过转兜底)。
/// 非 0 结局都要清确认态 + **排一次整页重建**(只 obj_delete 不重建, 被全屏框盖住的
/// 标题栏那一层就冻着不动), 勾才进删除流水线。行上的勾选态/确认态仍然原地跟上。
unsafe fn popup_clicked(event: u32, ok: bool) {
    if confirm_pop::click(confirm_pop::SLOT_ICON, event) == 0 { return; }
    st_wr!(IC_DELCONF, 0);
    if ok { request_delete(confirm_pop::target(confirm_pop::SLOT_ICON)); }
    st_wr!(IP_POP_DIRT, 1);
    refresh_rows();
}

/// 消息框按钮的事件入口(勾 = 确认删除 / 叉 = 取消; 事件码过滤同行控件回调)。
/// 符号名不能与字体页的 `chaos_pop_*` 撞 —— 两个 #[no_mangle] 同名会直接链接失败。
#[no_mangle]
pub(crate) unsafe extern "C" fn chaos_ipop_ok(event: u32) -> u32 { popup_clicked(event, true); 0 }
#[no_mangle]
pub(crate) unsafe extern "C" fn chaos_ipop_no(event: u32) -> u32 { popup_clicked(event, false); 0 }

/// 建页开头(render 开头)调用: 兜掉上一次建页时漏删的框 + 状态归零(confirm_pop 的约定之一)。
pub(crate) unsafe fn popup_reset() {
    confirm_pop::reset(confirm_pop::SLOT_ICON);
    st_wr!(IP_POP_DIRT, 0);
}

/// 新页实例边界(ui.rs 的 on_create)调用: 只丢状态不碰对象 —— 息屏时 on_destroy 也会被
/// 走到, 那里对象树还在但状态不该动, 句柄作废一律登记在这个边界上。
pub(crate) unsafe fn popup_forget(pid: usize) {
    if pid != ICON_PID as usize { return; }
    confirm_pop::forget(confirm_pop::SLOT_ICON);
    st_wr!(IP_POP_DIRT, 0);
}

/// 点某一条目。语义(与行上的文字一致):
///   未选中的项   -> 切过去(应用这一包 / 回系统原图标)
///   已选中的包   -> 第一次点变成"再点一次删除", 第二次点弹真系统确认框, 勾才删
///   系统原图标   -> 已经是它, 无事可做
/// 跑批期间不接受换方向(半途换会留下"部分换部分没换"), 只把确认态清掉。
unsafe fn click_entry(e: u32) {
    if e >= entry_count() { return; }
    let pack = entry_pack(e);
    if entry_selected(e) {
        if pack == 0 { return; }
        if st_rd!(IC_DELCONF) == pack {
            // 第二次点已选中的包 = 当场弹真系统确认框, 勾才进删除流水线。
            popup_ask(pack);
        } else {
            st_wr!(IC_DELCONF, pack);
        }
        refresh_rows();
        return;
    }
    st_wr!(IC_DELCONF, 0);
    let s = st_rd!(IC_STATE);
    if s == 2 || s == 3 { refresh_rows(); return; }
    st_wr!(IC_PACK, pack);
    st_wr!(IC_BUILT, 0);
    st_wr!(IC_REQ, if pack == 0 { 2 } else { 1 });
    // 勾选立刻挪到点的那一条, 不等 38 拍跑完(跑批在后台把桌面图标换过去)
    refresh_rows();
}

/// 有没有我们这一路的活在推进(供节拍自适应用): 跑批、待消费的应用/恢复请求、
/// 删除流水线、关框后欠着的那一次整页重建。
pub(crate) unsafe fn pending() -> bool {
    st_rd!(IC_STATE) != 0 || st_rd!(IC_REQ) != 0 || st_rd!(IC_DEL_ST) != 0
        || st_rd!(IP_POP_DIRT) != 0
}

/// 50ms UI tick 里调用：一次只推进一个应用。
/// 注册表与 launcher 的写入都发生在 UI 线程，与固件自己的 app_install 同线程，
/// 不存在并发修改；但一次 refresh 内含 rebuild，所以 38 张摊到 38 拍(约 2 秒)。
pub(crate) unsafe fn tick() {
    // 息屏/AOD 不做：refresh 内部会 rebuild 整个桌面, 那时固件正在销毁/重建界面。
    // 删除流水线同样停在这里, 屏幕亮了自然接着跑(状态都在 RAM 里)。
    if !fw_api::screen_is_on() { return; }
    // 一批正在跑时不接受新请求: 半途换方向会留下"部分换部分没换"的状态,
    // 而界面只显示一个勾选态。click_entry 在跑批期间也会吞掉切换。
    let running = st_rd!(IC_STATE) == 2 || st_rd!(IC_STATE) == 3;
    if running && st_rd!(IC_REQ) != 0 { st_wr!(IC_REQ, 0); }
    // 框的兜底销毁: 关框那一下现场门没过才会转到这里。跑批(38 张逐拍换)期间不碰对象,
    // 亮屏 + 没跑批的拍上补一刀; 补完照样要排那一次整页重建。
    if !running && page_is_live(ICON_PID as usize)
        && confirm_pop::close_deferred(confirm_pop::SLOT_ICON) {
        st_wr!(IP_POP_DIRT, 1);
    }
    // 关框后那一次整页重建(勾与叉都要, 依据见 IP_POP_DIRT 的说明)。跑批期间不排,
    // 批过去自然跟上; 页不在前台了就不用排(下次 on_create 会把旗标清掉)。
    if st_rd!(IP_POP_DIRT) != 0 && !running && page_is_live(ICON_PID as usize) {
        st_wr!(IP_POP_DIRT, 0);
        render_req(ICON_PID as usize);
    }
    if !running {
        match st_rd!(IC_REQ) {
            1 => { st_wr!(IC_REQ, 0); plan_apply(); }
            2 => { st_wr!(IC_REQ, 0); plan_restore(); }
            _ => {}
        }
    }
    match st_rd!(IC_STATE) {
        2 => step_apply(),
        3 => step_restore(),
        // 批量空闲才走删除流水线(它自己会在删当前包时先起一次恢复批次)
        _ => delete_tick(),
    }
}
