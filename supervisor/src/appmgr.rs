// 应用管理(核心逻辑移植自 Shell++ shellpp-ii, 精简版)
//
// 注册表(同固件 p67tc 3.101.043, 与 shellpp 043 target 一致):
//   /data/apps.json       可见应用
//   /data/apps.json_hide  隐藏应用(可选, 不存在 = 没有隐藏过)
// 结构: 顶层 object, "InstalledApps": [ {..}, .. ]; 条目字段:
//   "package"  包名(身份字段)
//   "names"    显示名数组: [ { "lang": "defaults", "value": "Corona" }, .. ]
//              (lang=="defaults" 优先, 否则取数组里第一个能读到的 value)
//   "name" | "appName" | "label" | "title"  显示名(兼容字段, 优先级高于 names)
//   "hideFlag" bool / "locked" bool
//
// 隐藏 = 对象从可见注册表搬到隐藏注册表(hideFlag 重建为 true);
// 显示 = 反向(搬回并摘除 hideFlag)。其余字段原样保留, 未知顶层键原样复制。
//
// 即时生效(2026-10-15, Ghidra 取证见 fw_api::app_launcher_del/add 注释):
// 注册表写回成功后, 按包名在注册表(APP_REGISTRY 链)里找到节点, 先把节点 +0x3C
// bit0(固件的"应隐藏"标志)同步成目标态, 再调 app_launcher_del(app_id) /
// app_launcher_add(app_id) 让桌面立刻摘/挂图标。
// **显示必须先清位再 add**: add 的桌面建记录函数 0x0C513700 在入口按这一位拒绝
// ("The app need hidden", 串 0x2CB99DA0), 只搬 JSON 不改位 = 重启后显示失败。
// 固件的"隐藏应用"命令(cmd 0x1a)就是同一条 del; 重启后的持久态由注册表文件决定。
// 找不到注册表节点(应用此刻不在册)时只写文件, 不调固件调用 —— 不猜。
//
// 写回策略: 内存重组后 open(WRONLY|CREAT) 原位写回; 新内容比原文短时用空格
// 填充到原长度(尾部空白是合法 JSON)。**不用 truncate/rename** —— 固件侧没有
// 拿到实证地址, 这里只允许已真机验证过的 open/read/write/close/remove。
//
// 安全红线:
//   * 对象溢出/缺 package/条目超上限 => 列表标 truncated, 拒绝一切改写
//     (改写只保留有记录的对象, 未记录的对象会被丢掉, 所以截断时绝不能写)。
//   * 单个被搬运对象 > OBJ_CAP => 拒绝该次操作。
//   * 操作在 tick 里执行(不在事件回调做文件 IO), 与缓存清理同一套模式。

use crate::*;
use core::ptr::{read_volatile, write_volatile};

pub(crate) const VIS_PATH: &[u8] = b"/data/apps.json\0";
pub(crate) const HID_PATH: &[u8] = b"/data/apps.json_hide\0";

pub(crate) const REG_CAP: usize = 32768; // 单个注册表文件字节上限(应用多时 10KB 不够)
pub(crate) const APP_MAX: usize = 40;    // 条目上限(两份注册表合计)
const OBJ_CAP: usize = 1536;             // 单个被搬运对象的字节上限
const PKG_CAP: usize = 44;               // 包名槽宽(E_PKG / BATCH_PKG / EXCL_SLOT 同宽)

// ===== 批量操作排除名单 =====
// 独立于两份注册表: 影响"一键隐藏/显示全部", 且排除项在页1 列表里不显示;
// 页7 可逐项取消排除。格式: magic "EX01" + 40 槽 × 44 字节包名; 槽首字节 0 = 空槽, 读到空槽即止。
// 包名匹配而非下标匹配, 重启后注册表顺序变化不会串项。
pub(crate) const EXCL_PATH: &[u8] = b"/data/chaos/appmgr_excl.bin\0";
const EXCL_MAGIC: [u8; 4] = *b"EX01";
const EXCL_HDR: usize = 4;
const EXCL_SLOT: usize = 44;
const EXCL_MAX: usize = APP_MAX;
const EXCL_TOTAL: usize = EXCL_HDR + EXCL_MAX * EXCL_SLOT;

// ===== 生效模式开关 =====
// 0 = 深度隐藏(搬 apps.json / apps.json_hide, 开机不注册; 显示需重启)
// 1 = 普通隐藏(保留 apps.json 条目, 只同步注册表隐藏位 + 桌面图标;
//     隐藏名单另存 appmgr_keep.bin, 开机由维护定时器补隐藏, 会闪一下)
// 开关只迁移“隐藏状态的存储方式”, 不改变哪些应用在逻辑上是隐藏的。
pub(crate) const APPLY_CFG: &[u8] = b"/data/chaos/appmgr_apply.cfg\0";
const KEEP_PATH: &[u8] = b"/data/chaos/appmgr_keep.bin\0";
const KEEP_MAGIC: [u8; 4] = *b"KH01";
const KEEP_TOTAL: usize = 4 + APP_MAX * PKG_CAP;
static mut APPLY_MODE: u32 = 0;
static mut APPLY_LOADED: u32 = 0;
static mut APPLY_LABEL: [u8; 48] = [0; 48];
static mut KEEP_BUF: [u8; KEEP_TOTAL] = [0; KEEP_TOTAL];
static mut KEEP_N: usize = 0;
// 切换隐藏方式时的包名快照: 文件会被逐条重写, 不能边遍历 E_PKG 边搬。
static mut MIG_BUF: [[u8; PKG_CAP]; APP_MAX] = [[0; PKG_CAP]; APP_MAX];
static mut MIG_N: usize = 0;
static mut REQ_MODE: u32 = 0;
static mut BOOT_APPLIED: u32 = 0;
static mut BOOT_TRIES: u32 = 0;
static mut BOOT_WAIT: u32 = 0;

// ===== 条目元数据(两份注册表合并; E_SRC 区分来源) =====
pub(crate) static mut E_PKG: [[u8; 44]; APP_MAX] = [[0; 44]; APP_MAX];
pub(crate) static mut E_NAME: [[u8; 32]; APP_MAX] = [[0; 32]; APP_MAX];
static mut E_OFF: [u32; APP_MAX] = [0; APP_MAX]; // 对象在本次载入的 REG_BUF 内的起点
static mut E_LEN: [u32; APP_MAX] = [0; APP_MAX];
static mut E_SRC: [u8; APP_MAX] = [0; APP_MAX];  // 0=可见 1=隐藏
static mut E_LOCK: [u8; APP_MAX] = [0; APP_MAX];
// 同一包名在合并列表里重复出现(同文件重复, 或可见/隐藏各一份)时, 后面的标记为 1。
// 页1/批量都跳过它们: 固件对重复条目会注册出两个桌面记录, 退出快应用时二次释放 -> 黑屏。
static mut E_DUP: [u8; APP_MAX] = [0; APP_MAX];
static mut DUP_N: usize = 0;
pub(crate) static mut APP_N: usize = 0;
static mut APP_LOADED: u32 = 0;
pub(crate) static mut APP_MSG: [u8; 96] = [0; 96];

// ===== 行句柄(页1 全量条目行; 批量按钮/状态行走 ctx.rows 槽0/1/2) =====
// 条目行**不走 ctx.rows**(PAGES[pid].rows 只有 12 格, build_row 拒绝更高槽位),
// 句柄自管; 事件槽号 = 条目下标 + 12(12..=51, 回调见 lib.rs chaos_row_ev12..51)。
// 0..11 必须留给 ctx.rows —— 条目复用会让点条目被派发成点按钮(槽位冲突, 已修)。
// 重建式渲染开始时统一清零(对象树已随旧 content 销毁, 只丢句柄不删对象)。
const APP_ROWS_N: usize = APP_MAX;
static mut APP_ROWS: [u32; APP_ROWS_N] = [0; APP_ROWS_N];

// ===== 排除名单状态 =====
static mut EXCL_PKG: [[u8; EXCL_SLOT]; EXCL_MAX] = [[0; EXCL_SLOT]; EXCL_MAX];
static mut EXCL_N: usize = 0;
static mut EXCL_LOADED: u32 = 0;
static mut EXCL_IO: [u8; EXCL_TOTAL] = [0; EXCL_TOTAL];
static mut EXCL_LABEL: [u8; 48] = [0; 48];   // 页1 行3 文案
static mut EXCL_HINT: [u8; 64] = [0; 64];    // 页7 行1 文案
// 当前列表每项是否被排除(按完整列表下标), 批量循环直接查它。
static mut E_EXCL: [u8; APP_MAX] = [0; APP_MAX];
// 页1 过滤视图: VIEW_IDX[v] = 完整列表下标; 已排除的条目不出现在页1。
static mut VIEW_IDX: [u8; APP_MAX] = [0; APP_MAX];
static mut VIEW_N: usize = 0;

// 页7 的展示顺序: 排除项优先, 其余保持原顺序。
static mut EXCL_VIEW: [u8; APP_MAX] = [0; APP_MAX];
static mut EXCL_VIEW_N: usize = 0;

pub(crate) unsafe fn rows_reset() {
    let p = core::ptr::addr_of_mut!(APP_ROWS) as *mut u32;
    let mut i = 0usize;
    while i < APP_ROWS_N { write_volatile(p.add(i), 0); i += 1; }
}

pub(crate) unsafe fn rows_ptr() -> *mut u32 { core::ptr::addr_of_mut!(APP_ROWS) as *mut u32 }

/// 条目行主标签(显示名)指针 —— E_NAME 是静态缓冲, 行控件可直接持有
pub(crate) unsafe fn entry_primary(i: usize) -> *const u8 {
    let p = core::ptr::addr_of!(E_NAME) as *const u8;
    p.add(i * 32)
}

// ===== 批量操作队列(一键隐藏/显示全部; tick 消费) =====
static mut REQ_BATCH: u32 = 0;                  // 0=无 1=全部隐藏 2=全部显示
static mut BATCH_PKG: [[u8; 44]; APP_MAX] = [[0; 44]; APP_MAX];
static mut BATCH_LOCK: [u8; APP_MAX] = [0; APP_MAX];  // 入队时的 locked 快照
static mut BATCH_N: usize = 0;
static mut BATCH_CURSOR: usize = 0;

// ===== 请求(tick 消费) =====
static mut REQ_OP: u32 = 0;      // 0=无 1=隐藏 2=显示
static mut REQ_PKG: [u8; 44] = [0; 44];
static mut REQ_NAME: [u8; 32] = [0; 32];
// 排除项切换请求(同样由 tick 消费, 不在事件回调里做文件 IO)
static mut REQ_EXCL: u32 = 0;
static mut REQ_EXCL_PKG: [u8; 44] = [0; 44];
static mut EXCL_MSG: [u8; 48] = [0; 48];
static mut EXCL_MODE: u32 = 0;              // 0=管理排除项 1=管理显示/隐藏
static mut MODE_LABEL: [u8; 48] = [0; 48];

// ===== 解析工作区 =====
static mut REG_BUF: [u8; REG_CAP] = [0; REG_CAP];
static mut REG_N: usize = 0;         // 本次载入字节数
static mut HAVE_FILE: u32 = 0;       // 0=文件不存在(允许合成最小结构)
static mut OVERSIZE: u32 = 0;        // 1=文件超过 REG_CAP
static mut TRUNC: u32 = 0;           // 1=存在无法管理的对象(拒绝改写)
static mut PARSE_BAD: u32 = 0;       // 1=本次 parse 失败
// 失败现场(界面直接报出来): 设备上的注册表拿不到, 这一行是定位格式差异的唯一入口。
static mut PARSE_AT: u32 = 0;        // 失败位置(字节偏移)
static mut PARSE_C: u8 = 0;          // 失败位置上的字节
static mut PARSE_LEN: u32 = 0;       // 失败的那份文件长度
static mut PARSE_FILE: u8 = 0;       // 0=可见注册表 1=隐藏注册表
// 顶层 key-value 跨度(键首引号到值结尾, 不含逗号)
static mut TL_S: [u32; 8] = [0; 8];
static mut TL_E: [u32; 8] = [0; 8];
static mut TL_ISARR: [u8; 8] = [0; 8]; // 1=值是 InstalledApps 数组
static mut TL_N: usize = 0;
// 本次 parse 中要跳过的对象(改写时删除)
static mut SKIP_OFF: usize = usize::MAX;
static mut SKIP_LEN: usize = 0;
// 被搬运对象的原始字节
static mut OBJ_BUF: [u8; OBJ_CAP] = [0; OBJ_CAP];
static mut OBJ_N: usize = 0;

// ===== 小工具 =====

unsafe fn cstr_len(s: *const u8) -> usize {
    let mut n = 0usize;
    while read_volatile(s.add(n)) != 0 { n += 1; }
    n
}

pub(crate) unsafe fn count() -> usize { APP_N }

pub(crate) unsafe fn msg() -> &'static [u8] {
    let n = cstr_len(core::ptr::addr_of!(APP_MSG) as *const u8);
    core::slice::from_raw_parts(core::ptr::addr_of!(APP_MSG) as *const u8, n + 1)
}

pub(crate) unsafe fn pkg_ptr(i: usize) -> *const u8 {
    let p = core::ptr::addr_of!(E_PKG) as *const u8;
    p.add(i * 44)
}

/// 条目当前是否可见(复选框态: 勾 = 可见)
pub(crate) unsafe fn visible(i: usize) -> bool {
    if i >= APP_N { return false; }
    read_volatile((core::ptr::addr_of!(E_SRC) as *const u8).add(i)) == 0
}

pub(crate) unsafe fn invalidate() { APP_LOADED = 0; }

pub(crate) unsafe fn has_req() -> bool { REQ_OP != 0 }

/// 列表被截断时禁止一切改写
unsafe fn write_blocked() -> bool { TRUNC != 0 || OVERSIZE != 0 || PARSE_BAD != 0 }

// ===== 页面事件 =====

/// 点了条目 e: 可见 => 请求隐藏; 隐藏 => 请求显示
pub(crate) unsafe fn click(e: usize) {
    if REQ_OP != 0 || REQ_BATCH != 0 || APP_LOADED == 0 || e >= APP_N { return; }
    if write_blocked() { return; }
    if read_volatile((core::ptr::addr_of!(E_LOCK) as *const u8).add(e)) != 0 {
        wmsg("该应用已锁定\0"); return;
    }
    let src = read_volatile((core::ptr::addr_of!(E_SRC) as *const u8).add(e));
    // 拷贝身份与显示名(操作与结果提示都用它)
    let mut k = 0usize;
    let pp = core::ptr::addr_of!(E_PKG) as *const u8;
    while k < 43 { let c = read_volatile(pp.add(e * 44 + k)); write_volatile(REQ_PKG.as_mut_ptr().add(k), c); if c == 0 { break; } k += 1; }
    write_volatile(REQ_PKG.as_mut_ptr().add(43), 0);
    k = 0;
    let np = core::ptr::addr_of!(E_NAME) as *const u8;
    while k < 31 { let c = read_volatile(np.add(e * 32 + k)); write_volatile(REQ_NAME.as_mut_ptr().add(k), c); if c == 0 { break; } k += 1; }
    write_volatile(REQ_NAME.as_mut_ptr().add(31), 0);
    REQ_OP = if src == 0 { 1 } else { 2 };
}

/// 在固件应用注册表(APP_REGISTRY 链, 同 app_lookup/icon_apply 一条链)里按包名
/// 找节点。节点: +0x04 链表 / +0x08 包名(堆串) / +0x10 u16 app_id。
/// 返回 0 = 没找到(应用此刻不在册, 调用方不得拿 0 去调固件)。
unsafe fn find_node(pkg: *const u8) -> u32 {
    let head = crate::mem::rd32(fw_api::APP_REGISTRY as *const u32);
    if head == 0 || crate::mem::safe_ptr(head) == false { return 0; }
    let mut node = crate::mem::rd32((head + 4) as *const u32);
    let mut guard = 0u32;
    while crate::mem::safe_ptr(node) && node != head && guard < 512 {
        guard += 1;
        let np = crate::mem::rd32((node + 0x08) as *const u32);
        if crate::mem::safe_ptr(np) && cstr_eq(np as *const u8, pkg) {
            return node;
        }
        node = crate::mem::rd32((node + 4) as *const u32);
    }
    0
}

/// 同步注册表节点的隐藏标志(node+0x3C bit0)。
///
/// 固件重建桌面(0x0C513700 `ldrb.w r2,[r0,#0x3c]; lsls r1,r2,#0x1f; bmi ...`)按这一位
/// 决定"跳过/拒绝"某个应用: app_launcher_add 看到它为 1 时打 "The app need hidden"
/// (串地址 0x2CB99DA0) 并返回 0。Chaos 只搬 JSON 不改这一位时, 重启后显示会失败
/// (必须再重启让 data_init 按新 JSON 重建节点); 隐藏则留下"桌面重建又冒出来"的隐患。
/// 只读改写 bit0, 其它位原样保留。
unsafe fn set_node_hidden(node: u32, hide: bool) {
    if node == 0 || crate::mem::safe_ptr(node) == false { return; }
    let p = (node + fw_api::APPN_FLAGS) as *mut u8;
    let v = read_volatile(p);
    let nv = if hide { v | fw_api::APPN_HIDDEN } else { v & !fw_api::APPN_HIDDEN };
    if nv != v { write_volatile(p, nv); }
}

/// 注册表写回成功后的即时生效: 先同步内存隐藏位, 再摘/挂桌面图标。
/// 节点不在册(重启后条目被搬走)时只返回 false, 由文件改动在重启后生效。
unsafe fn apply_pkg(hide: bool, pkg: *const u8) -> bool {
    let node = find_node(pkg);
    if node == 0 { return false; }
    let id = crate::mem::rd16((node + fw_api::APPN_ID) as *const u16) as u32;
    if id == 0 { return false; }
    if hide {
        set_node_hidden(node, true);
        fw_api::app_launcher_del(id);
    } else {
        // 必须先清位再 add: add 在入口就按这一位拒绝
        set_node_hidden(node, false);
        fw_api::app_launcher_add(id);
    }
    true
}

unsafe fn apply_immediate(hide: bool) -> bool {
    apply_pkg(hide, core::ptr::addr_of!(REQ_PKG) as *const u8)
}

/// tick: 执行挂起的隐藏/显示, 完成后重读列表(调用方随后 render_req)
pub(crate) unsafe fn do_op() {
    if REQ_OP == 0 { return; }
    let hide = REQ_OP == 1;
    REQ_OP = 0;
    let ok = if hide { op_hide() } else { op_show() };
    if ok {
        let live = apply_immediate(hide);
        let mut w = W::new(core::ptr::addr_of_mut!(APP_MSG) as *mut u8, 96);
        if live {
            w.s(if hide { "已隐藏 ".as_bytes() } else { "已显示 ".as_bytes() });
        } else if hide {
            w.s("已写入, 重启后隐藏 ".as_bytes());
        } else {
            w.s("已写入, 重启后显示 ".as_bytes());
        }
        w.s(cstr_slice(core::ptr::addr_of!(REQ_NAME) as *const u8))
         .s("\0".as_bytes());
        w.end();
    } else {
        wmsg("操作失败, 注册表未改动\0");
    }
    reload();
}

/// 一键批量: 把当前批的方向对应的全部条目拷进队列(快照), tick 里逐个执行。
pub(crate) unsafe fn request_batch(hide_all: bool) {
    if REQ_BATCH != 0 || REQ_OP != 0 || APP_LOADED == 0 { return; }
    if write_blocked() { wmsg("列表不完整, 禁止改写\0"); return; }
    BATCH_N = 0;
    BATCH_CURSOR = 0;
    let mut i = 0usize;
    while i < APP_N && BATCH_N < APP_MAX {
        let src = read_volatile((core::ptr::addr_of!(E_SRC) as *const u8).add(i));
        let want = if hide_all { 0u8 } else { 1u8 };
        // 重复包名只操作一次: 固件会为重复条目建多个桌面记录, 退出时二次释放 -> 黑屏。
        if read_volatile((core::ptr::addr_of!(E_DUP) as *const u8).add(i)) != 0 {
            i += 1; continue;
        }
        if src == want {
            // 排除名单不参与任何一键操作
            if read_volatile((core::ptr::addr_of!(E_EXCL) as *const u8).add(i)) != 0 {
                i += 1; continue;
            }
            let pp = core::ptr::addr_of!(E_PKG) as *const u8;
            let dp = (core::ptr::addr_of_mut!(BATCH_PKG) as *mut u8).add(BATCH_N * 44);
            let mut k = 0usize;
            while k < 43 { let c = read_volatile(pp.add(i * 44 + k)); write_volatile(dp.add(k), c); if c == 0 { break; } k += 1; }
            write_volatile(dp.add(43), 0);
            // 锁定位在入队时快照(批量循环里 capture 会重置 E_*, 不能再回头查)
            write_volatile((core::ptr::addr_of_mut!(BATCH_LOCK) as *mut u8).add(BATCH_N),
                read_volatile((core::ptr::addr_of!(E_LOCK) as *const u8).add(i)));
            BATCH_N += 1;
        }
        i += 1;
    }
    if BATCH_N == 0 {
        wmsg("没有可操作应用\0");
        return;
    }
    REQ_BATCH = if hide_all { 1 } else { 2 };
}

pub(crate) unsafe fn batch_pending() -> bool { REQ_BATCH != 0 }

/// tick: 执行批量队列。每个条目一次 capture+写回(单条 op 的完整流程),
/// 全部跑完或遇到失败即收尾。
pub(crate) unsafe fn do_batch() {
    if REQ_BATCH == 0 { return; }
    let hide = REQ_BATCH == 1;
    let mut done = 0usize;
    let mut failed = 0usize;
    while BATCH_CURSOR < BATCH_N {
        let bp = (core::ptr::addr_of!(BATCH_PKG) as *const u8).add(BATCH_CURSOR * 44);
        if read_volatile((core::ptr::addr_of!(BATCH_LOCK) as *const u8).add(BATCH_CURSOR)) != 0 {
            BATCH_CURSOR += 1; continue;   // 锁定的应用批量不碰(与单点同一语义)
        }
        let ok = if hide { op_hide_pkg(bp) } else { op_show_pkg(bp) };
        BATCH_CURSOR += 1;
        if ok {
            done += 1;
            // 即时生效(队列包名不在 REQ_PKG, 直接按包名找)
            apply_pkg(hide, bp);
        } else {
            failed += 1;
            break;   // 文件态不可信, 停止批量(与单条操作同一保守策略)
        }
    }
    REQ_BATCH = 0;
    let mut w = W::new(core::ptr::addr_of_mut!(APP_MSG) as *mut u8, 96);
    w.s(if hide { "已隐藏 ".as_bytes() } else { "已显示 ".as_bytes() })
     .n(done as u32).s(" 个".as_bytes());
    if failed != 0 { w.s("(中途失败)\0".as_bytes()); } else { w.s("\0".as_bytes()); }
    w.end();
    reload();
}

unsafe fn cstr_slice(p: *const u8) -> &'static [u8] {
    core::slice::from_raw_parts(p, cstr_len(p) + 1)
}

unsafe fn wmsg(s: &str) {
    put_str(core::ptr::addr_of_mut!(APP_MSG) as *mut u8, s.as_bytes());
}

/// 记下失败现场并返回"坏"(2)。界面会把位置、该字节与文件长度报出来。
unsafe fn bad(p: usize) -> u8 {
    PARSE_BAD = 1;
    PARSE_AT = p as u32;
    PARSE_LEN = REG_N as u32;
    PARSE_C = if p < REG_N { read_volatile(REG_BUF.as_ptr().add(p)) } else { 0 };
    2
}

unsafe fn put_u32(dst: *mut u8, mut v: u32) -> usize {
    let mut tmp = [0u8; 10];
    let mut n = 0usize;
    loop {
        tmp[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
        if v == 0 { break; }
    }
    let mut i = 0usize;
    while i < n { write_volatile(dst.add(i), tmp[n - 1 - i]); i += 1; }
    n
}

unsafe fn put_hex2(dst: *mut u8, v: u8) -> usize {
    const H: &[u8; 16] = b"0123456789abcdef";
    write_volatile(dst, H[(v >> 4) as usize]);
    write_volatile(dst.add(1), H[(v & 15) as usize]);
    2
}

unsafe fn put_lit(dst: *mut u8, s: &[u8]) -> usize {
    let mut i = 0usize;
    while i < s.len() { write_volatile(dst.add(i), s[i]); i += 1; }
    s.len()
}

// ===== JSON 扫描(base/len 参数化, REG_BUF 与 OBJ_BUF 共用) =====

#[inline(always)]
unsafe fn s_peek(base: *const u8, len: usize, p: usize) -> u8 {
    if p < len { read_volatile(base.add(p)) } else { 0 }
}

#[inline(always)]
unsafe fn s_ws(base: *const u8, len: usize, p: &mut usize) {
    while *p < len {
        let c = read_volatile(base.add(*p));
        if c != b' ' && c != b'\t' && c != b'\n' && c != b'\r' { break; }
        *p += 1;
    }
}

/// 跳过一个字符串(*p 在开引号)。返回 false = 坏。
unsafe fn s_string_skip(base: *const u8, len: usize, p: &mut usize) -> bool {
    if s_peek(base, len, *p) != b'"' { return false; }
    *p += 1;
    while *p < len {
        let c = read_volatile(base.add(*p)); *p += 1;
        if c == b'\\' { if *p < len { *p += 1; continue; } return false; }
        if c == b'"' { return true; }
    }
    false
}

/// 跳过任意 JSON 值(深度计数, 字符串感知)。返回 false = 坏。
unsafe fn s_value_skip(base: *const u8, len: usize, p: &mut usize) -> bool {
    s_ws(base, len, p);
    let c = s_peek(base, len, *p);
    if c == b'"' { return s_string_skip(base, len, p); }
    if c == b'{' || c == b'[' {
        let mut depth = 0u32;
        loop {
            if *p >= len { return false; }
            let ch = read_volatile(base.add(*p)); *p += 1;
            if ch == b'"' {
                *p -= 1;
                if !s_string_skip(base, len, p) { return false; }
                continue;
            }
            if ch == b'{' || ch == b'[' { depth += 1; }
            else if ch == b'}' || ch == b']' {
                depth -= 1;
                if depth == 0 { return true; }
            }
        }
    }
    // true / false / null / 数字: 吃到分隔符为止
    while *p < len {
        let ch = read_volatile(base.add(*p));
        if ch == b',' || ch == b'}' || ch == b']' || ch == b' ' || ch == b'\t' || ch == b'\n' || ch == b'\r' { break; }
        *p += 1;
    }
    true
}

/// 读字符串到 dst(带最小 unescape)。0=ok 1=溢出(串本身已跳过) 2=坏。
unsafe fn s_string_read(base: *const u8, len: usize, dst: *mut u8, cap: usize, p: &mut usize) -> u8 {
    if s_peek(base, len, *p) != b'"' { return 2; }
    *p += 1;
    let mut o = 0usize;
    let mut over = false;
    loop {
        if *p >= len { return 2; }
        let c = read_volatile(base.add(*p)); *p += 1;
        if c == b'"' { break; }
        let mut ch = c;
        if c == b'\\' {
            if *p >= len { return 2; }
            let e = read_volatile(base.add(*p)); *p += 1;
            ch = match e {
                b'"' | b'\\' | b'/' => e,
                b'n' => 0x0A, b't' => 0x09, b'r' => 0x0D, b'b' => 0x08, b'f' => 0x0C,
                b'u' => { if *p + 4 > len { return 2; } *p += 4; 0x20 }
                _ => return 2,
            };
        }
        if o + 1 < cap { write_volatile(dst.add(o), ch); o += 1; } else { over = true; }
    }
    write_volatile(dst.add(o), 0);
    if over { 1 } else { 0 }
}

unsafe fn key_is(key: *const u8, lit: &[u8]) -> bool {
    let n = cstr_len(key);
    if n + 1 != lit.len() { return false; }
    let mut i = 0usize;
    while i < n { if read_volatile(key.add(i)) != lit[i] { return false; } i += 1; }
    true
}

// ===== 载入与解析 =====

/// 把 path 读进 REG_BUF。false = 文件不存在(合法: 隐藏注册表可选)。
unsafe fn load_file(path: *const u8) -> bool {
    HAVE_FILE = 0;
    OVERSIZE = 0;
    let fd = fw_api::open(path, fw_api::oflag::RDONLY, 0);
    if fd < 0 { REG_N = 0; return false; }
    let mut n = 0usize;
    while n < REG_CAP {
        let got = fw_api::read(fd, REG_BUF.as_mut_ptr().add(n), (REG_CAP - n) as u32);
        if got <= 0 { break; }
        n += got as usize;
    }
    if n == REG_CAP {
        // 读满上限: 无论后面还有没有字节, 都不能保证解析完整, 一律按超限只读处理。
        OVERSIZE = 1;
    }
    fw_api::close(fd);
    REG_N = n;
    HAVE_FILE = 1;
    true
}

/// 解析一个应用对象(*p 在 '{'), 抽字段并登记条目。返回 0=ok(到达 '}' 之后)。
unsafe fn scan_object(p: &mut usize, src: u8) -> u8 {
    let base = REG_BUF.as_ptr();
    let len = REG_N;
    let start = *p;
    let mut pkg = [0u8; 44];
    let mut name = [0u8; 32];
    let mut name_pri = 9u8;
    let mut has_pkg = false;
    let mut pkg_over = false;
    let mut hidden = 0u8;
    let mut locked = 0u8;
    let mut key = [0u8; 20];
    *p += 1; // '{'
    loop {
        s_ws(base, len, p);
        let c = s_peek(base, len, *p);
        if c == b'}' { *p += 1; break; }
        if c == b',' { *p += 1; continue; }
        let kr = s_string_read(base, len, key.as_mut_ptr(), 20, p);
        if kr == 2 { return 2; }
        s_ws(base, len, p);
        if s_peek(base, len, *p) != b':' { return 2; }
        *p += 1;
        s_ws(base, len, p);
        let known = kr == 0;
        if known && key_is(key.as_ptr(), b"package\0") {
            let r = s_string_read(base, len, pkg.as_mut_ptr(), 44, p);
            if r == 2 { return 2; }
            if r == 0 { has_pkg = true; } else { pkg_over = true; }
        } else if known && (key_is(key.as_ptr(), b"name\0") || key_is(key.as_ptr(), b"appName\0")
            || key_is(key.as_ptr(), b"label\0") || key_is(key.as_ptr(), b"title\0")) {
            // 优先级: name(0) < appName(1) < label(2) < title(3); 重复键先到先得
            let pri = if key_is(key.as_ptr(), b"name\0") { 0u8 }
                else if key_is(key.as_ptr(), b"appName\0") { 1u8 }
                else if key_is(key.as_ptr(), b"label\0") { 2u8 } else { 3u8 };
            let mut tmp = [0u8; 32];
            let r = s_string_read(base, len, tmp.as_mut_ptr(), 32, p);
            if r == 0 && pri < name_pri {
                let mut k = 0usize;
                while k < 32 { name[k] = tmp[k]; if tmp[k] == 0 { break; } k += 1; }
                name_pri = pri;
            }
        } else if known && key_is(key.as_ptr(), b"names\0") {
            // names 有三种形态。本机固件写的是**对象**(固件里没有 lang/defaults 两个键,
            // 它的 names 是"语言 -> 名字"的映射), 旧假设的数组形态也留着兼容:
            //   { "default": "Corona", "zh-CN": "..." }      <- 本机
            //   [ { "lang": "defaults", "value": "..." } ]   <- 旧假设
            //   "Corona"                                     <- 单字符串
            // 档位: default/defaults = 4, 其余语言键 = 5; name/appName/label/title(0..3) 仍优先。
            let shape = s_peek(base, len, *p);
            if shape == b'{' {
                *p += 1;
                let mut ofirst = true;
                loop {
                    s_ws(base, len, p);
                    let oc = s_peek(base, len, *p);
                    if oc == b'}' { *p += 1; break; }
                    if oc == 0 { return 2; }
                    if !ofirst {
                        if oc != b',' { return 2; }
                        *p += 1;
                        s_ws(base, len, p);
                    }
                    ofirst = false;
                    let mut lk = [0u8; 20];
                    let kr3 = s_string_read(base, len, lk.as_mut_ptr(), 20, p);
                    if kr3 == 2 { return 2; }
                    s_ws(base, len, p);
                    if s_peek(base, len, *p) != b':' { return 2; }
                    *p += 1;
                    s_ws(base, len, p);
                    if s_peek(base, len, *p) != b'"' {
                        if !s_value_skip(base, len, p) { return 2; }
                        continue;
                    }
                    let mut tv = [0u8; 32];
                    let r3 = s_string_read(base, len, tv.as_mut_ptr(), 32, p);
                    if r3 == 2 { return 2; }
                    if r3 != 0 { continue; }
                    let pri = if kr3 == 0 && (key_is(lk.as_ptr(), b"default\0")
                        || key_is(lk.as_ptr(), b"defaults\0")) { 4u8 } else { 5u8 };
                    if pri < name_pri {
                        let mut k = 0usize;
                        while k < 32 { name[k] = tv[k]; if tv[k] == 0 { break; } k += 1; }
                        name_pri = pri;
                    }
                }
            } else if shape == b'[' {
                *p += 1;
                let mut nfirst = true;
                loop {
                    s_ws(base, len, p);
                    let nc = s_peek(base, len, *p);
                    if nc == b']' { *p += 1; break; }
                    if nc == 0 { return 2; }
                    if !nfirst {
                        if nc != b',' { return 2; }
                        *p += 1;
                        s_ws(base, len, p);
                    }
                    nfirst = false;
                    s_ws(base, len, p);
                    if s_peek(base, len, *p) != b'{' {
                        // 非对象条目(容错): 整个跳过
                        if !s_value_skip(base, len, p) { return 2; }
                        continue;
                    }
                    *p += 1;
                    let mut is_def = false;
                    let mut val_ok = false;
                    let mut tmp = [0u8; 32];
                    let mut k2 = [0u8; 20];
                    loop {
                        s_ws(base, len, p);
                        let c2 = s_peek(base, len, *p);
                        if c2 == b'}' { *p += 1; break; }
                        if c2 == b',' { *p += 1; continue; }
                        if c2 == 0 { return 2; }
                        let kr2 = s_string_read(base, len, k2.as_mut_ptr(), 20, p);
                        if kr2 == 2 { return 2; }
                        s_ws(base, len, p);
                        if s_peek(base, len, *p) != b':' { return 2; }
                        *p += 1;
                        s_ws(base, len, p);
                        if kr2 == 0 && key_is(k2.as_ptr(), b"lang\0") {
                            let mut lg = [0u8; 16];
                            let r = s_string_read(base, len, lg.as_mut_ptr(), 16, p);
                            if r == 2 { return 2; }
                            if r == 0 && (cstr_eq(lg.as_ptr(), b"defaults\0".as_ptr()) || cstr_eq(lg.as_ptr(), b"default\0".as_ptr())) {
                                is_def = true;
                            }
                        } else if kr2 == 0 && key_is(k2.as_ptr(), b"value\0") {
                            let r = s_string_read(base, len, tmp.as_mut_ptr(), 32, p);
                            if r == 0 { val_ok = true; } else if r == 2 { return 2; }
                        } else {
                            if !s_value_skip(base, len, p) { return 2; }
                        }
                    }
                    if val_ok && (is_def || name_pri >= 5) {
                        let mut k = 0usize;
                        while k < 32 { name[k] = tmp[k]; if tmp[k] == 0 { break; } k += 1; }
                        name_pri = if is_def { 4 } else { 5 };
                    }
                }
            } else if shape == b'"' {
                let mut tv = [0u8; 32];
                let r3 = s_string_read(base, len, tv.as_mut_ptr(), 32, p);
                if r3 == 2 { return 2; }
                if r3 == 0 && name_pri > 4 {
                    let mut k = 0usize;
                    while k < 32 { name[k] = tv[k]; if tv[k] == 0 { break; } k += 1; }
                    name_pri = 4;
                }
            } else if !s_value_skip(base, len, p) {
                return 2;
            }
        } else if known && key_is(key.as_ptr(), b"hideFlag\0") {
            hidden = if s_peek(base, len, *p) == b't' { 1 } else { 0 };
            if !s_value_skip(base, len, p) { return 2; }
        } else if known && key_is(key.as_ptr(), b"locked\0") {
            locked = if s_peek(base, len, *p) == b't' { 1 } else { 0 };
            if !s_value_skip(base, len, p) { return 2; }
        } else {
            if !s_value_skip(base, len, p) { return 2; }
        }
    }
    let end = *p;
    // 登记(超出上限 / 缺身份 / 身份溢出 => 截断, 拒绝改写)
    if APP_N >= APP_MAX || !has_pkg || pkg_over {
        TRUNC = 1;
        return 0;
    }
    let i = APP_N;
    let mut k = 0usize;
    while k < 44 { E_PKG[i][k] = pkg[k]; if pkg[k] == 0 { break; } k += 1; }
    // 显示名缺省回退 = 包名; 锁定条目在名字尾部标注
    if name[0] == 0 {
        let mut k = 0usize;
        while k < 31 { name[k] = pkg[k]; if pkg[k] == 0 { break; } k += 1; }
        name[31] = 0;
    } else if locked != 0 {
        let n0 = cstr_len(name.as_ptr());
        let tag: &[u8] = " [锁]".as_bytes();
        let mut o = n0;
        let mut t = 0usize;
        while o < 30 && t < tag.len() { name[o] = tag[t]; o += 1; t += 1; }
        name[o] = 0;
    }
    let mut k = 0usize;
    while k < 32 { E_NAME[i][k] = name[k]; k += 1; }
    E_OFF[i] = start as u32;
    E_LEN[i] = (end - start) as u32;
    E_SRC[i] = src;
    E_LOCK[i] = locked;
    // 隐藏注册表里的对象视作已隐藏(hideFlag 仅为固件约定, 展示态以所在文件为准)
    let _ = hidden;
    APP_N += 1;
    0
}

/// 解析 REG_BUF 里的注册表(顶层 object)。0=ok 2=坏。
/// 条目写入 E_*; 顶层 kv 跨度写入 TL_*(改写用)。
unsafe fn parse_registry(src: u8) -> u8 {
    TL_N = 0;
    PARSE_BAD = 0;
    if HAVE_FILE == 0 || REG_N == 0 {
        // 文件不存在或空文件 = 空注册表(043 宽松读取语义)
        if REG_N == 0 { HAVE_FILE = 0; }
        return 0;
    }
    if OVERSIZE != 0 { return bad(REG_N); }
    let base = REG_BUF.as_ptr();
    let len = REG_N;
    let mut p = 0usize;
    s_ws(base, len, &mut p);
    if s_peek(base, len, p) != b'{' { return bad(p); }
    p += 1;
    let mut first = true;
    let mut found = false;
    loop {
        s_ws(base, len, &mut p);
        let c = s_peek(base, len, p);
        if c == b'}' { break; }
        if c == 0 { return bad(p); }
        if !first {
            if c != b',' { return bad(p); }
            p += 1;
            s_ws(base, len, &mut p);
        }
        first = false;
        let ks = p;
        let mut key = [0u8; 20];
        let kr = s_string_read(base, len, key.as_mut_ptr(), 20, &mut p);
        if kr == 2 { return bad(p); }
        s_ws(base, len, &mut p);
        if s_peek(base, len, p) != b':' { return bad(p); }
        p += 1;
        s_ws(base, len, &mut p);
        let _vs = p;
        let mut is_arr = false;
        if kr == 0 && key_is(key.as_ptr(), b"InstalledApps\0") {
            if found { return bad(p); }   // 重复键 = 坏
            found = true;
            let shape = s_peek(base, len, p);
            if shape == b'{' {
                // 本机隐藏表里出现过 {"InstalledApps":{}}(空对象, 20 字节): 当成空列表;
                // 改写时由 serialize 统一写回数组形态。非空对象只读(拒绝改写)。
                let mut q = p;
                if !s_value_skip(base, len, &mut q) { return bad(p); }
                let mut k = p + 1;
                s_ws(base, len, &mut k);
                if s_peek(base, len, k) != b'}' { TRUNC = 1; }
                p = q;
                is_arr = true;
            } else if shape != b'[' {
                return bad(p);
            } else {
                p += 1;
                is_arr = true;
                let mut efirst = true;
                loop {
                    s_ws(base, len, &mut p);
                    let ec = s_peek(base, len, p);
                    if ec == b']' { p += 1; break; }
                    if ec == 0 { return bad(p); }
                    if !efirst {
                        if ec != b',' { return bad(p); }
                        p += 1;
                        s_ws(base, len, &mut p);
                    }
                    efirst = false;
                    s_ws(base, len, &mut p);
                    // 条目不是对象: 跳过并拒绝改写(条目的形态我们看不懂)
                    if s_peek(base, len, p) != b'{' {
                        TRUNC = 1;
                        if !s_value_skip(base, len, &mut p) { return bad(p); }
                        continue;
                    }
                    let rc = scan_object(&mut p, src);
                    if rc != 0 { return bad(p); }
                }
            }
        } else {
            if !s_value_skip(base, len, &mut p) { return bad(p); }
        }
        let ve = p;
        // 顶层键超过台账容量: 只读可用, 改写拒绝(拼接会丢键)。
        if TL_N >= 8 { TRUNC = 1; continue; }
        TL_S[TL_N] = ks as u32;
        TL_E[TL_N] = ve as u32;
        TL_ISARR[TL_N] = is_arr as u8;
        TL_N += 1;
    }
    if p > len { return bad(p); }
    0
}

// ===== 排除名单(不参与一键显示/隐藏) =====
//
// 文件 /data/chaos/appmgr_excl.bin, 定长:
//   magic[4] = "EX01"
//   条目 0..39, 每条 44 B: 包名(43 B, 第 44 字节恒 0)
// 列表按文件顺序排列, 写满即止。读失败/魔数不对 = 空名单(绝不因此禁止应用管理)。
// 排除是 Chaos 自己的名单, 不进 apps.json —— 固件不认识它, 也不该认识。

/// 标记合并列表里的重复包名: 同一包名只保留第一次出现(可见优先, 因为先解析)。
unsafe fn mark_dups() {
    DUP_N = 0;
    let base = core::ptr::addr_of_mut!(E_DUP) as *mut u8;
    let pkgs = core::ptr::addr_of!(E_PKG) as *const u8;
    let mut i = 0usize;
    while i < APP_N {
        let mut dup = 0u8;
        let mut j = 0usize;
        while j < i {
            if cstr_eq(pkgs.add(i * 44), pkgs.add(j * 44)) { dup = 1; break; }
            j += 1;
        }
        write_volatile(base.add(i), dup);
        if dup != 0 { DUP_N += 1; }
        i += 1;
    }
}

unsafe fn build_view() {
    VIEW_N = 0;
    let mut i = 0usize;
    while i < APP_N && i < APP_MAX {
        let du = read_volatile((core::ptr::addr_of!(E_DUP) as *const u8).add(i));
        let ex = if excl_index(pkg_ptr(i)) != usize::MAX { 1u8 } else { 0u8 };
        write_volatile((core::ptr::addr_of_mut!(E_EXCL) as *mut u8).add(i), ex);
        if ex == 0 && du == 0 && VIEW_N < APP_MAX {
            write_volatile((core::ptr::addr_of_mut!(VIEW_IDX) as *mut u8).add(VIEW_N), i as u8);
            VIEW_N += 1;
        }
        i += 1;
    }
}

unsafe fn excl_index(pkg: *const u8) -> usize {
    let base = core::ptr::addr_of!(EXCL_PKG) as *const u8;
    let mut i = 0usize;
    while i < EXCL_N {
        if cstr_eq(base.add(i * EXCL_SLOT), pkg) { return i; }
        i += 1;
    }
    usize::MAX
}

pub(crate) unsafe fn excl_on(i: usize) -> bool {
    i < APP_N && read_volatile((core::ptr::addr_of!(E_EXCL) as *const u8).add(i)) != 0
}

pub(crate) unsafe fn view_count() -> usize { VIEW_N }

/// 第 v 个可见条目在 APP_* 里的原始下标(排除项不会出现在这里)。
pub(crate) unsafe fn view_at(v: usize) -> usize {
    if v >= VIEW_N { return usize::MAX; }
    read_volatile((core::ptr::addr_of!(VIEW_IDX) as *const u8).add(v)) as usize
}

/// 页1 顺序: 勾选(可见)在前, 隐藏在后; 稳定排序。
pub(crate) unsafe fn sort_view_visible_first() {
    let mut i = 1usize;
    while i < VIEW_N {
        let cur = read_volatile((core::ptr::addr_of!(VIEW_IDX) as *const u8).add(i));
        let mut j = i;
        while j > 0 {
            let prev = read_volatile((core::ptr::addr_of!(VIEW_IDX) as *const u8).add(j - 1));
            if (visible(prev as usize) as u8) >= (visible(cur as usize) as u8) { break; }
            write_volatile((core::ptr::addr_of_mut!(VIEW_IDX) as *mut u8).add(j), prev);
            j -= 1;
        }
        write_volatile((core::ptr::addr_of_mut!(VIEW_IDX) as *mut u8).add(j), cur);
        i += 1;
    }
}

/// 页7 顺序: 排除项在前; 稳定排序。
pub(crate) unsafe fn build_excl_view() {
    EXCL_VIEW_N = 0;
    let mut i = 0usize;
    while i < APP_N && i < APP_MAX {
        if !is_dup(i) {
            write_volatile((core::ptr::addr_of_mut!(EXCL_VIEW) as *mut u8).add(EXCL_VIEW_N), i as u8);
            EXCL_VIEW_N += 1;
        }
        i += 1;
    }
    let mut a = 1usize;
    while a < EXCL_VIEW_N {
        let cur = read_volatile((core::ptr::addr_of!(EXCL_VIEW) as *const u8).add(a));
        let mut b = a;
        while b > 0 {
            let prev = read_volatile((core::ptr::addr_of!(EXCL_VIEW) as *const u8).add(b - 1));
            if (excl_on(prev as usize) as u8) >= (excl_on(cur as usize) as u8) { break; }
            write_volatile((core::ptr::addr_of_mut!(EXCL_VIEW) as *mut u8).add(b), prev);
            b -= 1;
        }
        write_volatile((core::ptr::addr_of_mut!(EXCL_VIEW) as *mut u8).add(b), cur);
        a += 1;
    }
}

pub(crate) unsafe fn is_dup(i: usize) -> bool {
    i < APP_N && read_volatile((core::ptr::addr_of!(E_DUP) as *const u8).add(i)) != 0
}

pub(crate) unsafe fn excl_view_count() -> usize { EXCL_VIEW_N }

pub(crate) unsafe fn excl_view_at(v: usize) -> usize {
    if v >= EXCL_VIEW_N { return usize::MAX; }
    read_volatile((core::ptr::addr_of!(EXCL_VIEW) as *const u8).add(v)) as usize
}

pub(crate) unsafe fn excl_ensure() {
    if EXCL_LOADED != 0 { return; }
    EXCL_LOADED = 1;
    EXCL_N = 0;
    let fd = fw_api::open(EXCL_PATH.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 { return; }
    let b = core::ptr::addr_of_mut!(EXCL_IO) as *mut u8;
    let mut n = 0usize;
    while n < EXCL_TOTAL {
        let got = fw_api::read(fd, b.add(n), (EXCL_TOTAL - n) as u32);
        if got <= 0 { break; }
        n += got as usize;
    }
    fw_api::close(fd);
    if n < EXCL_HDR + EXCL_SLOT { return; }
    let mut k = 0usize;
    while k < EXCL_HDR {
        if read_volatile(b.add(k)) != EXCL_MAGIC[k] { return; }
        k += 1;
    }
    let mut i = 0usize;
    while i < EXCL_MAX {
        let o = EXCL_HDR + i * EXCL_SLOT;
        if o + EXCL_SLOT > n { break; }
        if read_volatile(b.add(o)) == 0 { break; }
        let d = (core::ptr::addr_of_mut!(EXCL_PKG) as *mut u8).add(i * EXCL_SLOT);
        let mut j = 0usize;
        while j < EXCL_SLOT {
            write_volatile(d.add(j), read_volatile(b.add(o + j)));
            j += 1;
        }
        write_volatile(d.add(EXCL_SLOT - 1), 0);
        EXCL_N += 1;
        i += 1;
    }
}

unsafe fn excl_save() -> bool {
    let b = core::ptr::addr_of_mut!(EXCL_IO) as *mut u8;
    let mut i = 0usize;
    while i < EXCL_TOTAL { write_volatile(b.add(i), 0); i += 1; }
    i = 0;
    while i < EXCL_HDR { write_volatile(b.add(i), EXCL_MAGIC[i]); i += 1; }
    i = 0;
    while i < EXCL_N {
        let s = (core::ptr::addr_of!(EXCL_PKG) as *const u8).add(i * EXCL_SLOT);
        let d = b.add(EXCL_HDR + i * EXCL_SLOT);
        let mut j = 0usize;
        while j < EXCL_SLOT { write_volatile(d.add(j), read_volatile(s.add(j))); j += 1; }
        i += 1;
    }
    let fd = fw_api::open(EXCL_PATH.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd < 0 { return false; }
    let mut off = 0usize;
    let mut ok = true;
    while off < EXCL_TOTAL {
        let got = fw_api::write(fd, b.add(off), (EXCL_TOTAL - off) as u32);
        if got <= 0 { ok = false; break; }
        off += got as usize;
    }
    fw_api::close(fd);
    ok
}

/// 只在 tick 里调用(文件 IO)。
unsafe fn excl_toggle(pkg: *const u8) -> bool {
    let idx = excl_index(pkg);
    let base = core::ptr::addr_of_mut!(EXCL_PKG) as *mut u8;
    if idx != usize::MAX {
        let mut j = idx;
        while j + 1 < EXCL_N {
            let s = base.add((j + 1) * EXCL_SLOT) as *const u8;
            let d = base.add(j * EXCL_SLOT);
            let mut k = 0usize;
            while k < EXCL_SLOT { write_volatile(d.add(k), read_volatile(s.add(k))); k += 1; }
            j += 1;
        }
        EXCL_N -= 1;
        let d = base.add(EXCL_N * EXCL_SLOT);
        let mut k = 0usize;
        while k < EXCL_SLOT { write_volatile(d.add(k), 0); k += 1; }
    } else {
        if EXCL_N >= EXCL_MAX { eset("排除项已满\0"); return false; }
        let d = base.add(EXCL_N * EXCL_SLOT);
        let mut k = 0usize;
        while k < EXCL_SLOT { write_volatile(d.add(k), read_volatile(pkg.add(k))); k += 1; }
        write_volatile(d.add(EXCL_SLOT - 1), 0);
        EXCL_N += 1;
    }
    if !excl_save() {
        // 写盘失败: 回到磁盘真值, 不让内存/文件分叉
        EXCL_LOADED = 0;
        excl_ensure();
        eset("保存失败\0");
        return false;
    }
    true
}

unsafe fn eset(s: &str) {
    put_str(core::ptr::addr_of_mut!(EXCL_MSG) as *mut u8, s.as_bytes());
}

pub(crate) unsafe fn excl_msg() -> &'static [u8] {
    core::slice::from_raw_parts(core::ptr::addr_of!(EXCL_MSG) as *const u8, 48)
}

/// 事件回调里只登记请求, 真正读写由 tick 做。
pub(crate) unsafe fn request_excl(i: usize) {
    if REQ_EXCL != 0 || APP_LOADED == 0 || i >= APP_N { return; }
    let dst = core::ptr::addr_of_mut!(REQ_EXCL_PKG) as *mut u8;
    let src = (core::ptr::addr_of!(E_PKG) as *const u8).add(i * PKG_CAP);
    let mut k = 0usize;
    while k < PKG_CAP { write_volatile(dst.add(k), read_volatile(src.add(k))); k += 1; }
    write_volatile(dst.add(PKG_CAP - 1), 0);
    REQ_EXCL = 1;
}

pub(crate) unsafe fn excl_has_req() -> bool { REQ_EXCL != 0 }

pub(crate) unsafe fn do_excl() {
    if REQ_EXCL == 0 { return; }
    REQ_EXCL = 0;
    if excl_toggle(core::ptr::addr_of!(REQ_EXCL_PKG) as *const u8) {
        eset("已保存\0");
    }
    build_view();
}

pub(crate) unsafe fn excl_label() -> &'static [u8] {
    let d = core::ptr::addr_of_mut!(EXCL_LABEL) as *mut u8;
    let mut n = put_lit(d, "排除项管理 (".as_bytes());
    n += put_u32(d.add(n), EXCL_N as u32);
    n += put_lit(d.add(n), ")\0".as_bytes());
    core::slice::from_raw_parts(d, n)
}

pub(crate) unsafe fn excl_hint() -> &'static [u8] {
    let d = core::ptr::addr_of_mut!(EXCL_HINT) as *mut u8;
    if EXCL_MODE != 0 {
        return {
            let mut n = put_lit(d, "点一行 = 隐藏/显示该应用\0".as_bytes());
            core::slice::from_raw_parts(d, n)
        };
    }
    let mut n = put_lit(d, "勾选 = 不参与一键 (已排除 ".as_bytes());
    n += put_u32(d.add(n), EXCL_N as u32);
    n += put_lit(d.add(n), ")\0".as_bytes());
    core::slice::from_raw_parts(d, n)
}

pub(crate) unsafe fn excl_mode() -> u32 { EXCL_MODE }

pub(crate) unsafe fn toggle_excl_mode() { EXCL_MODE = if EXCL_MODE == 0 { 1 } else { 0 }; }

pub(crate) unsafe fn mode_label() -> &'static [u8] {
    let d = core::ptr::addr_of_mut!(MODE_LABEL) as *mut u8;
    let s = if EXCL_MODE == 0 { "模式: 管理排除项\0" } else { "模式: 管理显隐\0" };
    let n = put_lit(d, s.as_bytes());
    core::slice::from_raw_parts(d, n)
}


/// (重)读两份注册表, 重建展示列表。
pub(crate) unsafe fn reload() {
    APP_N = 0;
    TRUNC = 0;
    PARSE_BAD = 0;
    OVERSIZE = 0;
    excl_ensure();
    apply_mode_ensure();
    let mut vis_bad = 0u32;
    let mut hid_bad = 0u32;
    // 可见注册表(应存在; 缺失 = 空列表, 043 宽松语义)
    if load_file(VIS_PATH.as_ptr()) {
        PARSE_FILE = 0;
        if parse_registry(0) != 0 { vis_bad = 1; }
    }
    // 隐藏注册表(可选; 一份坏不丢另一份已读到的条目)
    if load_file(HID_PATH.as_ptr()) {
        PARSE_FILE = 1;
        if parse_registry(1) != 0 { hid_bad = 1; }
    }
    // 免重启模式: 条目还在 apps.json 里, 隐藏状态由 Chaos 名单表达。
    // 只把"可见来源且名单里命中"的条目标成隐藏, 界面的勾选/点击方向随之切换。
    if APPLY_MODE != 0 {
        let mut i = 0usize;
        while i < APP_N {
            if read_volatile((core::ptr::addr_of!(E_SRC) as *const u8).add(i)) == 0 {
                let p = (core::ptr::addr_of!(E_PKG) as *const u8).add(i * 44);
                if keep_has(p) {
                    write_volatile((core::ptr::addr_of_mut!(E_SRC) as *mut u8).add(i), 1);
                }
            }
            i += 1;
        }
    }
    mark_dups();
    if OVERSIZE != 0 {
        wmsg("注册表过大, 只读\0");
    } else if vis_bad != 0 {
        // 失败现场报到界面上: 文件/偏移/该字节/长度。设备上的注册表拿不到,
        // 这一行就是定位格式差异的唯一入口。
        let d = core::ptr::addr_of_mut!(APP_MSG) as *mut u8;
        let mut n = put_lit(d, "解析失败 ".as_bytes());
        n += put_lit(d.add(n), if PARSE_FILE == 0 { "可见".as_bytes() } else { "隐藏".as_bytes() });
        n += put_lit(d.add(n), b"@");
        n += put_u32(d.add(n), PARSE_AT);
        n += put_lit(d.add(n), b" b=0x");
        n += put_hex2(d.add(n), PARSE_C);
        n += put_lit(d.add(n), b" len=");
        n += put_u32(d.add(n), PARSE_LEN);
        write_volatile(d.add(n), 0);
    } else if hid_bad != 0 {
        // 可见列表照常显示; 隐藏表看不懂就只读, 不拖累整页。
        wmsg("隐藏表格式不识别, 已忽略\0");
        TRUNC = 1;
    } else if TRUNC != 0 {
        wmsg("列表已截断, 禁止改写\0");
    } else if DUP_N != 0 {
        // 不自动改盘: 用户点一次隐藏/显示就会把该包名的所有副本一起收敛。
        wmsg("发现重复条目, 已只显示一份\0");
    } else if APP_N == 0 {
        wmsg("没有可管理的应用\0");
    } else {
        wmsg("点应用切换显示/隐藏\0");
    }
    build_view();
    if read_volatile(core::ptr::addr_of!(EXCL_MSG) as *const u8) == 0 {
        eset("点一行切换排除状态\0");
    }
    APP_LOADED = 1;
}

pub(crate) unsafe fn ensure() {
    apply_mode_ensure();
    if APP_LOADED == 0 { reload(); }
}

// ===== 生效模式: 配置/隐藏名单/开机补偿 =====

unsafe fn cfg_mode_read() -> u32 {
    let fd = fw_api::open(APPLY_CFG.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 { return 0; }
    let mut b = [0u8; 1];
    let n = fw_api::read(fd, b.as_mut_ptr(), 1);
    fw_api::close(fd);
    if n == 1 && b[0] == b'1' { 1 } else { 0 }
}

unsafe fn cfg_mode_save() -> bool {
    let fd = fw_api::open(APPLY_CFG.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd < 0 { return false; }
    let b = if APPLY_MODE == 0 { b'0' } else { b'1' };
    let n = fw_api::write(fd, &b as *const u8, 1);
    fw_api::close(fd);
    n == 1
}

/// 读隐藏名单(固定 4+40*44 字节)。文件缺失/坏 magic 视为空名单。
unsafe fn keep_load() {
    KEEP_N = 0;
    KEEP_BUF[0] = KEEP_MAGIC[0]; KEEP_BUF[1] = KEEP_MAGIC[1];
    KEEP_BUF[2] = KEEP_MAGIC[2]; KEEP_BUF[3] = KEEP_MAGIC[3];
    let fd = fw_api::open(KEEP_PATH.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 { return; }
    let mut n = 0usize;
    while n < KEEP_TOTAL {
        let got = fw_api::read(fd, KEEP_BUF.as_mut_ptr().add(n), (KEEP_TOTAL - n) as u32);
        if got <= 0 { break; }
        n += got as usize;
    }
    fw_api::close(fd);
    if n < 4 { return; }
    if KEEP_BUF[0] != KEEP_MAGIC[0] || KEEP_BUF[1] != KEEP_MAGIC[1]
        || KEEP_BUF[2] != KEEP_MAGIC[2] || KEEP_BUF[3] != KEEP_MAGIC[3] { return; }
    let mut i = 0usize;
    while i < APP_MAX && 4 + i * PKG_CAP < n {
        if read_volatile(KEEP_BUF.as_ptr().add(4 + i * PKG_CAP)) == 0 { break; }
        i += 1;
    }
    KEEP_N = i;
}

unsafe fn keep_save() -> bool {
    KEEP_BUF[0] = KEEP_MAGIC[0]; KEEP_BUF[1] = KEEP_MAGIC[1];
    KEEP_BUF[2] = KEEP_MAGIC[2]; KEEP_BUF[3] = KEEP_MAGIC[3];
    let len = 4 + KEEP_N * PKG_CAP;
    let fd = fw_api::open(KEEP_PATH.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd < 0 { return false; }
    let mut off = 0usize;
    let mut ok = true;
    while off < len {
        let got = fw_api::write(fd, KEEP_BUF.as_ptr().add(off), (len - off) as u32);
        if got <= 0 { ok = false; break; }
        off += got as usize;
    }
    fw_api::close(fd);
    ok
}

unsafe fn keep_find(pkg: *const u8) -> usize {
    let mut i = 0usize;
    while i < KEEP_N {
        let p = KEEP_BUF.as_ptr().add(4 + i * PKG_CAP);
        if cstr_eq(p, pkg) { return i; }
        i += 1;
    }
    usize::MAX
}

unsafe fn keep_has(pkg: *const u8) -> bool { keep_find(pkg) != usize::MAX }

unsafe fn keep_add(pkg: *const u8) -> bool {
    if keep_has(pkg) { return true; }
    if KEEP_N >= APP_MAX { return false; }
    let dst = KEEP_BUF.as_mut_ptr().add(4 + KEEP_N * PKG_CAP);
    let mut k = 0usize;
    while k < PKG_CAP - 1 {
        let c = read_volatile(pkg.add(k));
        write_volatile(dst.add(k), c);
        if c == 0 { break; }
        k += 1;
    }
    write_volatile(dst.add(PKG_CAP - 1), 0);
    KEEP_N += 1;
    keep_save()
}

unsafe fn keep_del(pkg: *const u8) -> bool {
    let i = keep_find(pkg);
    if i == usize::MAX { return true; }
    let mut j = i;
    while j + 1 < KEEP_N {
        let src = KEEP_BUF.as_ptr().add(4 + (j + 1) * PKG_CAP);
        let dst = KEEP_BUF.as_mut_ptr().add(4 + j * PKG_CAP);
        let mut k = 0usize;
        while k < PKG_CAP {
            write_volatile(dst.add(k), read_volatile(src.add(k)));
            k += 1;
        }
        j += 1;
    }
    KEEP_N -= 1;
    keep_save()
}

/// 把名单里的应用逐个补成"隐藏": 置 node+0x3C bit0, 再摘桌面图标。
unsafe fn keep_apply_all() -> usize {
    let mut found = 0usize;
    let mut i = 0usize;
    while i < KEEP_N {
        let p = KEEP_BUF.as_ptr().add(4 + i * PKG_CAP);
        if excl_index(p) != usize::MAX { found += 1; i += 1; continue; }
        let node = find_node(p);
        if node != 0 {
            let id = crate::mem::rd16((node + fw_api::APPN_ID) as *const u16) as u32;
            if id != 0 {
                set_node_hidden(node, true);
                fw_api::app_launcher_del(id);
                found += 1;
            }
        }
        i += 1;
    }
    found
}

pub(crate) unsafe fn apply_mode_ensure() {
    if APPLY_LOADED != 0 { return; }
    APPLY_MODE = cfg_mode_read();
    keep_load();
    APPLY_LOADED = 1;
}

pub(crate) unsafe fn apply_mode_is_keep() -> bool {
    apply_mode_ensure();
    APPLY_MODE != 0
}

/// 开关态: true = 深度隐藏(条目不在 apps.json)。
pub(crate) unsafe fn deep_on() -> bool {
    apply_mode_ensure();
    APPLY_MODE == 0
}

/// 页1 开关行文案: 只写名称, 开关态由行右侧部件表达。
pub(crate) unsafe fn apply_mode_label() -> &'static [u8] {
    apply_mode_ensure();
    let d = core::ptr::addr_of_mut!(APPLY_LABEL) as *mut u8;
    let n = put_lit(d, "深度隐藏\0".as_bytes());
    core::slice::from_raw_parts(d, n)
}

/// 开关拨动: 只改模式与配置, 真正的补隐藏/重读放到 tick。
pub(crate) unsafe fn toggle_apply_mode() {
    apply_mode_ensure();
    APPLY_MODE = if APPLY_MODE == 0 { 1 } else { 0 };
    cfg_mode_save();
    REQ_MODE = 1;
}

pub(crate) unsafe fn mode_pending() -> bool { REQ_MODE != 0 }

pub(crate) unsafe fn do_mode_apply() {
    if REQ_MODE == 0 { return; }
    REQ_MODE = 0;
    if APPLY_MODE == 0 {
        // 打开深度隐藏: 普通隐藏(keep)的条目搬进 apps.json_hide。
        // 逻辑上仍然隐藏, 只是存储方式从"保留条目+名单"换成"移出注册表"。
        keep_load();
        let mut i = 0usize;
        while i < KEEP_N {
            let p = KEEP_BUF.as_ptr().add(4 + i * PKG_CAP);
            if move_vis_to_hide(p) {
                apply_pkg(true, p);
            }
            i += 1;
        }
        KEEP_N = 0;
        let _ = keep_save();
        reload();
        wmsg("深度隐藏已开启: 隐藏应用不再出现在开机桌面\0");
    } else {
        // 关闭深度隐藏: 把 apps.json_hide 的条目全部搬回 apps.json,
        // 但**保持原来的隐藏状态** —— 同时记入 keep 名单, 开机补偿再隐藏。
        // 这样所有应用都会重新注册(有节点), 之后普通显示立即生效。
        reload();
        MIG_N = 0;
        let mut i = 0usize;
        while i < APP_N && MIG_N < APP_MAX {
            if read_volatile((core::ptr::addr_of!(E_SRC) as *const u8).add(i)) == 1 {
                let dst = (core::ptr::addr_of_mut!(MIG_BUF) as *mut u8).add(MIG_N * PKG_CAP);
                let src = (core::ptr::addr_of!(E_PKG) as *const u8).add(i * PKG_CAP);
                let mut k = 0usize;
                while k < PKG_CAP {
                    write_volatile(dst.add(k), read_volatile(src.add(k)));
                    k += 1;
                }
                MIG_N += 1;
            }
            i += 1;
        }
        let mut m = 0usize;
        while m < MIG_N {
            let p = (core::ptr::addr_of!(MIG_BUF) as *const u8).add(m * PKG_CAP);
            if move_hide_to_vis(p) {
                keep_add(p);
            }
            m += 1;
        }
        let _ = keep_save();
        reload();
        wmsg("深度隐藏已关闭: 全部应用已写入 apps.json, 原隐藏状态保留\0");
    }
}

/// 开机补偿: 模块加载后由维护定时器(UI 线程)调用。只对保留条目模式生效,
/// 把 Chaos 隐藏名单里的应用重新置位并摘图标; 找不到节点时有限次重试。
pub(crate) unsafe fn boot_apply() {
    if BOOT_APPLIED != 0 { return; }
    apply_mode_ensure();
    excl_ensure();
    if APPLY_MODE == 0 { BOOT_APPLIED = 1; return; }
    if KEEP_N == 0 { BOOT_APPLIED = 1; return; }
    // 注册表可能比模块晚就绪: 每秒试一次, 最多 5 分钟。
    // 不能只试 120 拍就放弃, 否则关机前的隐藏名单会因节点未就绪而失效。
    BOOT_WAIT = BOOT_WAIT.wrapping_add(1);
    if BOOT_WAIT % 40 != 0 { return; }
    let found = keep_apply_all();
    BOOT_TRIES = BOOT_TRIES.wrapping_add(1);
    if found >= KEEP_N || BOOT_TRIES >= 300 { BOOT_APPLIED = 1; }
}

// ===== 改写 =====

/// 把当前 REG_BUF(已 parse) 重组进自身前部并返回新长度。
/// skip: 跳过的对象跨度; append: 0=不追加 1=追加原样(摘 hideFlag) 2=追加并写 hideFlag:true。
/// 不变量: 写指针永远 <= 读指针(只删不增, 追加字节来自 OBJ_BUF), 原位覆盖安全。
unsafe fn serialize(skip_off: usize, skip_len: usize, append: u8) -> usize {
    let old = REG_N;
    let src = REG_BUF.as_ptr();
    let mut w = 0usize;
    macro_rules! putb {
        ($b:expr) => {{
            if w + 1 >= REG_CAP { return 0; }
            REG_BUF[w] = $b; w += 1;
        }};
    }
    macro_rules! copy {
        ($from:expr, $n:expr) => {{
            let n = $n;
            if w + n >= REG_CAP { return 0; }
            let mut i = 0usize;
            while i < n { REG_BUF[w] = read_volatile(src.add($from + i)); w += 1; i += 1; }
        }};
    }
    macro_rules! lit {
        ($s:expr) => {{
            let s: &[u8] = $s;
            if w + s.len() >= REG_CAP { return 0; }
            let mut i = 0usize;
            while i < s.len() { REG_BUF[w] = s[i]; w += 1; i += 1; }
        }};
    }
    putb!(b'{');
    if TL_N == 0 && HAVE_FILE == 0 {
        // 隐藏注册表不存在: 合成最小结构
        lit!(b"\"InstalledApps\":[".as_slice());
        if append != 0 { if !write_obj(&mut w, append == 2) { return 0; } }
        lit!(b"]}".as_slice());
    } else {
        let mut first = true;
        let mut k = 0usize;
        while k < TL_N {
            if !first { putb!(b','); }
            first = false;
            if TL_ISARR[k] != 0 {
                lit!(b"\"InstalledApps\":[".as_slice());
                let mut ifirst = true;
                let mut i = 0usize;
                while i < APP_N {
                    let o = E_OFF[i] as usize;
                    let l = E_LEN[i] as usize;
                    if !(o == skip_off && l == skip_len) {
                        if !ifirst { putb!(b','); }
                        ifirst = false;
                        copy!(o, l);
                    }
                    i += 1;
                }
                if append != 0 {
                    if !ifirst { putb!(b','); }
                    if !write_obj(&mut w, append == 2) { return 0; }
                }
                lit!(b"]".as_slice());
            } else {
                copy!(TL_S[k] as usize, (TL_E[k] as usize) - (TL_S[k] as usize));
            }
            k += 1;
        }
        putb!(b'}');
    }
    // 原位写回: 短于原文的部分用空格补齐(尾部空白是合法 JSON)
    while w < old { REG_BUF[w] = b' '; w += 1; }
    w
}

/// 把 OBJ_BUF 里的对象写进 REG_BUF[w..], 摘除 hideFlag; need_flag=true 时补 "hideFlag":true。
unsafe fn write_obj(w: &mut usize, need_flag: bool) -> bool {
    let base = OBJ_BUF.as_ptr();
    let len = OBJ_N;
    if len == 0 || s_peek(base, len, 0) != b'{' { return false; }
    macro_rules! putb {
        ($b:expr) => {{
            if *w + 1 >= REG_CAP { return false; }
            REG_BUF[*w] = $b; *w += 1;
        }};
    }
    macro_rules! copyo {
        ($from:expr, $n:expr) => {{
            let n = $n;
            if *w + n >= REG_CAP { return false; }
            let mut i = 0usize;
            while i < n { REG_BUF[*w] = read_volatile(base.add($from + i)); *w += 1; i += 1; }
        }};
    }
    putb!(b'{');
    let mut p = 1usize;
    let mut first = true;
    let mut key = [0u8; 20];
    loop {
        s_ws(base, len, &mut p);
        let c = s_peek(base, len, p);
        if c == b'}' { break; }
        if c == b',' { p += 1; continue; }
        if c == 0 { return false; }
        let ks = p;
        let kr = s_string_read(base, len, key.as_mut_ptr(), 20, &mut p);
        if kr == 2 { return false; }
        s_ws(base, len, &mut p);
        if s_peek(base, len, p) != b':' { return false; }
        p += 1;
        s_ws(base, len, &mut p);
        let _vs = p;
        if !s_value_skip(base, len, &mut p) { return false; }
        let ve = p;
        // hideFlag 一律摘除, 由 need_flag 决定是否重写
        if kr == 0 && key_is(key.as_ptr(), b"hideFlag\0") { continue; }
        if !first { putb!(b','); }
        first = false;
        copyo!(ks, ve - ks);
    }
    if need_flag {
        if !first { putb!(b','); }
        let s: &[u8] = b"\"hideFlag\":true";
        let mut i = 0usize;
        while i < s.len() { putb!(s[i]); i += 1; }
    }
    putb!(b'}');
    true
}

/// 把 path 用 REG_BUF[0..n] 原位覆盖(WRONLY|CREAT, 与全模块写组合一致)。
unsafe fn write_file(path: *const u8, n: usize) -> bool {
    if n == 0 { return false; }
    let fd = fw_api::open(path, fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd < 0 { return false; }
    let mut off = 0usize;
    let mut ok = true;
    while off < n {
        let got = fw_api::write(fd, REG_BUF.as_ptr().add(off), (n - off) as u32);
        if got <= 0 { ok = false; break; }
        off += got as usize;
    }
    fw_api::close(fd);
    ok
}

/// 从 src 注册表(0=可见 1=隐藏)里按包名找到对象, 原始字节拷进 OBJ_BUF,
/// 并记下它的跨度(Skip)。成功返回 true(REG_BUF 保持该文件已 parse 状态)。
/// 注意: 这里清零 APP_N —— capture 的 parse 必须是"单文件隔离"的,
/// serialize 只允许遍历当前 parse 的对象跨度。
unsafe fn capture(pkg: *const u8, src: u8) -> bool {
    let path = if src == 0 { VIS_PATH.as_ptr() } else { HID_PATH.as_ptr() };
    APP_N = 0;
    if !load_file(path) { return false; }
    if parse_registry(src) != 0 { return false; }
    let mut i = 0usize;
    while i < APP_N {
        if E_SRC[i] == src {
            let pp = core::ptr::addr_of!(E_PKG) as *const u8;
            if cstr_eq(pp.add(i * 44), pkg) {
                let o = E_OFF[i] as usize;
                let l = E_LEN[i] as usize;
                if l == 0 || l > OBJ_CAP { return false; }
                let mut k = 0usize;
                while k < l { OBJ_BUF[k] = read_volatile(REG_BUF.as_ptr().add(o + k)); k += 1; }
                OBJ_N = l;
                SKIP_OFF = o;
                SKIP_LEN = l;
                return true;
            }
        }
        i += 1;
    }
    false
}

unsafe fn cstr_eq(a: *const u8, b: *const u8) -> bool {
    let mut i = 0usize;
    loop {
        let ca = read_volatile(a.add(i));
        let cb = read_volatile(b.add(i));
        if ca != cb { return false; }
        if ca == 0 { return true; }
        i += 1;
    }
}

/// 当前已 parse 的文件里是否已有该包名(必须在 parse_registry 之后调用)。
unsafe fn file_has_pkg(pkg: *const u8) -> bool {
    let base = core::ptr::addr_of!(E_PKG) as *const u8;
    let mut i = 0usize;
    while i < APP_N {
        if cstr_eq(base.add(i * 44), pkg) { return true; }
        i += 1;
    }
    false
}

/// 从 src 文件里删除该包名的**所有**副本。重复条目会让固件注册多个桌面记录,
/// 退出应用时二次释放 -> 黑屏, 所以收敛时必须一次删干净。
unsafe fn remove_all_pkg(pkg: *const u8, src: u8) -> bool {
    let path = if src == 0 { VIS_PATH.as_ptr() } else { HID_PATH.as_ptr() };
    let mut guard = 0usize;
    loop {
        if !capture(pkg, src) { return true; }
        let n = serialize(SKIP_OFF, SKIP_LEN, 0);
        if n == 0 { return false; }
        if !write_file(path, n) { return false; }
        guard += 1;
        if guard >= 8 { return false; }
    }
}

/// 只做文件搬运: 可见 -> 隐藏(不读模式开关, 供单点/批量/模式迁移共用)。
unsafe fn move_vis_to_hide(pkg: *const u8) -> bool {
    if write_blocked() { return false; }
    if !capture(pkg, 0) { return false; }
    // 1) 隐藏注册表: 只有还没有该包名时才追加(重复追加 = 复制体)
    load_file(HID_PATH.as_ptr());
    APP_N = 0;
    if parse_registry(1) != 0 { return false; }
    if !file_has_pkg(pkg) {
        let n = serialize(usize::MAX, 0, 2);
        if n == 0 || !write_file(HID_PATH.as_ptr(), n) { return false; }
    }
    // 2) 可见注册表: 删掉该包名的所有副本
    if !remove_all_pkg(pkg, 0) { return false; }
    true
}

/// 只做文件搬运: 隐藏 -> 可见(不读模式开关)。
unsafe fn move_hide_to_vis(pkg: *const u8) -> bool {
    if write_blocked() { return false; }
    if !capture(pkg, 1) { return false; }
    // 1) 可见注册表: 只有还没有该包名时才追加(重复追加 = 复制体)
    if !load_file(VIS_PATH.as_ptr()) { return false; }
    APP_N = 0;
    if parse_registry(0) != 0 { return false; }
    if !file_has_pkg(pkg) {
        let n = serialize(usize::MAX, 0, 1);
        if n == 0 || !write_file(VIS_PATH.as_ptr(), n) { return false; }
    }
    // 2) 隐藏注册表: 删掉该包名的所有副本
    if !remove_all_pkg(pkg, 1) { return false; }
    true
}

/// 隐藏: 对象 可见 -> 隐藏(hideFlag:true)。先写隐藏注册表, 再从可见注册表删除。
unsafe fn op_hide() -> bool { op_hide_pkg(core::ptr::addr_of!(REQ_PKG) as *const u8) }

unsafe fn op_hide_pkg(pkg: *const u8) -> bool {
    if write_blocked() { return false; }
    // 普通隐藏: 条目留在 apps.json, 只记入 Chaos 隐藏名单; 即时生效由 apply_pkg 完成。
    if apply_mode_is_keep() {
        return keep_add(pkg);
    }
    move_vis_to_hide(pkg)
}

/// 显示: 对象 隐藏 -> 可见(摘 hideFlag)。先写可见注册表, 再从隐藏注册表删除。
unsafe fn op_show() -> bool { op_show_pkg(core::ptr::addr_of!(REQ_PKG) as *const u8) }

unsafe fn op_show_pkg(pkg: *const u8) -> bool {
    if write_blocked() { return false; }
    // 普通隐藏: 条目本来就在 apps.json, 只需从 Chaos 隐藏名单里摘掉。
    if apply_mode_is_keep() && keep_has(pkg) {
        return keep_del(pkg);
    }
    move_hide_to_vis(pkg)
}
