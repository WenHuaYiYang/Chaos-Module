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
// 页7 可逐项取消排除。格式: magic "EX02" + 40 槽 × 44 字节包名 + 4 字节校验和; 槽首字节 0 = 空槽, 读到空槽即止。
// 包名匹配而非下标匹配, 重启后注册表顺序变化不会串项。
pub(crate) const EXCL_PATH: &[u8] = b"/data/chaos/appmgr_excl.bin\0";
const EXCL_MAGIC: [u8; 4] = *b"EX02";
const EXCL_MAGIC_OLD: [u8; 4] = *b"EX01";
const EXCL_HDR: usize = 4;
const EXCL_SLOT: usize = 44;
const EXCL_MAX: usize = APP_MAX;
const EXCL_SUM_OFF: usize = EXCL_HDR + EXCL_MAX * EXCL_SLOT;
const EXCL_TOTAL: usize = EXCL_SUM_OFF + 4;

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
// 运行期看门狗: 换绑/重连、手机重发 rpk_info、新装应用都会触发固件重注册,
// 普通隐藏的隐藏位只在运行期(新节点 bit0 默认 0), 需要有人持续补回来。
static mut WATCH_WAIT: u32 = 0;
static mut WATCH_SIG: u32 = 0;
static mut WATCH_SEEN: u32 = 0;
static mut WATCH_BUSY: u32 = 0;
static mut WATCH_CHG: u32 = 0;      // 注册表刚变过, 等安静一拍再统一重放

// ===== 条目元数据(两份注册表合并; E_SRC 区分来源) =====
pub(crate) static mut E_PKG: [[u8; 44]; APP_MAX] = [[0; 44]; APP_MAX];
pub(crate) static mut E_NAME: [[u8; 32]; APP_MAX] = [[0; 32]; APP_MAX];
static mut E_OFF: [u32; APP_MAX] = [0; APP_MAX]; // 对象在本次载入的 REG_BUF 内的起点
static mut E_LEN: [u32; APP_MAX] = [0; APP_MAX];
static mut E_SRC: [u8; APP_MAX] = [0; APP_MAX];  // 0=可见 1=隐藏
static mut E_VER: [[u8; 24]; APP_MAX] = [[0; 24]; APP_MAX];   // 条目里的 versionName
static mut E_ICON: [[u8; 32]; APP_MAX] = [[0; 32]; APP_MAX];  // 条目里的 icon 文件名
static mut E_LOCK: [u8; APP_MAX] = [0; APP_MAX];
// 同一包名在合并列表里重复出现(同文件重复, 或可见/隐藏各一份)时, 后面的标记为 1。
// 页1/批量都跳过它们: 固件对重复条目会注册出两个桌面记录, 退出快应用时二次释放 -> 黑屏。
static mut E_DUP: [u8; APP_MAX] = [0; APP_MAX];
static mut DUP_N: usize = 0;
pub(crate) static mut APP_N: usize = 0;
static mut APP_LOADED: u32 = 0;
pub(crate) static mut APP_MSG: [u8; 96] = [0; 96];
// do_op/do_batch 的结果提示要跨 reload 保留: reload 默认会把状态行改回操作提示。
static mut MSG_KEEP: u32 = 0;

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
// 页1/页30 上次建出来的行数。行数没变就逐行原地刷新, 不整页重建
// (重建会销毁行对象, 列表滚动位置也会跳回顶部)。
static mut UI_ROWS: u32 = 0;

pub(crate) unsafe fn set_ui_rows(n: u32) { UI_ROWS = n; }
pub(crate) unsafe fn ui_rows() -> u32 { UI_ROWS }

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
static mut BATCH_DONE: usize = 0;
static mut BATCH_FAIL: usize = 0;
static mut BATCH_NOREG: usize = 0;

// ===== 请求(tick 消费) =====
static mut REQ_OP: u32 = 0;      // 0=无 1=隐藏 2=显示
static mut REQ_PKG: [u8; 44] = [0; 44];
static mut REQ_NAME: [u8; 32] = [0; 32];
// 排除项切换请求(同样由 tick 消费, 不在事件回调里做文件 IO)
static mut REQ_EXCL: u32 = 0;
static mut REQ_EXCL_PKG: [u8; 44] = [0; 44];
// 重放隐藏名单: 注册表有变化(设备上新装了应用)或用户点"重新应用隐藏"时置位, tick 消费
static mut REQ_REAPPLY: u32 = 0;
static mut REG_SIG: u32 = 0;        // 上次注册表的包名指纹(与顺序无关)
static mut REG_SEEN: u32 = 0;       // 是否已建立基线
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

pub(crate) static mut HDR_BUF: [u8; 48] = [0; 48];

/// 列表上方一行分组说明: "快应用 N · 已隐藏 M" (随 tick 原地刷新)
pub(crate) unsafe fn header_text() -> &'static [u8] {
    let n = view_count();
    let mut hid = 0usize;
    let mut v = 0usize;
    while v < n {
        let i = view_at(v);
        if i != usize::MAX && !visible(i) { hid += 1; }
        v += 1;
    }
    let mut w = W::new(core::ptr::addr_of_mut!(HDR_BUF) as *mut u8, 48);
    w.s("快应用 ".as_bytes()).n(n as u32).s(" / 已隐藏 ".as_bytes()).n(hid as u32).end();
    let len = cstr_len(core::ptr::addr_of!(HDR_BUF) as *const u8);
    core::slice::from_raw_parts(core::ptr::addr_of!(HDR_BUF) as *const u8, len + 1)
}

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

pub(crate) unsafe fn reapply_pending() -> bool { REQ_REAPPLY != 0 }

/// 请求重放隐藏名单(tick 里做; 事件回调里不做文件/桌面操作)
pub(crate) unsafe fn request_reapply() { REQ_REAPPLY = 1; }

/// 包名集合指纹: 与顺序无关、且不含显示/隐藏来源 —— 只有"装/卸了应用"才会变,
/// Chaos 自己改隐藏状态(同一条目换注册表文件)不会触发。
unsafe fn reg_sig() -> u32 {
    let mut acc = 0u32;
    let mut i = 0usize;
    while i < APP_N {
        let p = (core::ptr::addr_of!(E_PKG) as *const u8).add(i * 44);
        let mut h = 2166136261u32;   // FNV-1a
        let mut k = 0usize;
        while k < 43 {
            let c = read_volatile(p.add(k));
            if c == 0 { break; }
            h = (h ^ c as u32).wrapping_mul(16777619);
            k += 1;
        }
        acc = acc.wrapping_add(h.rotate_left(7));
        i += 1;
    }
    // 条目数也计入(同包名重复出现时集合指纹不会变, 但数量会变)
    acc ^ (APP_N as u32).wrapping_mul(2654435761)
}


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
        // 先 del 再 add: 应用可能已经在桌面上(排除项/状态错位), 直接 add 会多建一条记录
        set_node_hidden(node, false);
        fw_api::app_launcher_del(id);
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
        MSG_KEEP = 1;
    } else {
        wmsg_keep("操作失败, 注册表未改动\0");
    }
    crate::log::cstr(core::ptr::addr_of!(APP_MSG) as *const u8);
    reload();
}

/// 一键批量: 把当前批的方向对应的全部条目拷进队列(快照), tick 里逐个执行。
pub(crate) unsafe fn request_batch(hide_all: bool) {
    if REQ_BATCH != 0 || REQ_OP != 0 || APP_LOADED == 0 { return; }
    if write_blocked() { wmsg("列表不完整, 禁止改写\0"); return; }
    BATCH_N = 0;
    BATCH_CURSOR = 0;
    BATCH_DONE = 0;
    BATCH_FAIL = 0;
    BATCH_NOREG = 0;
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
    crate::log::kv(if hide_all { "批量隐藏开始, 队列 " } else { "批量显示开始, 队列 " }, BATCH_N as u32);
}

pub(crate) unsafe fn batch_pending() -> bool { REQ_BATCH != 0 }

/// tick: 执行批量队列。每个条目一次 capture+写回(单条 op 的完整流程),
/// 全部跑完或遇到失败即收尾。
pub(crate) unsafe fn do_batch() {
    if REQ_BATCH == 0 { return; }
    let hide = REQ_BATCH == 1;
    // 一拍只做一个: 每次 op 都要整文件解析/序列化/写盘, 40 个连做会长期占用 UI 线程。
    while BATCH_CURSOR < BATCH_N {
        let bp = (core::ptr::addr_of!(BATCH_PKG) as *const u8).add(BATCH_CURSOR * 44);
        if read_volatile((core::ptr::addr_of!(BATCH_LOCK) as *const u8).add(BATCH_CURSOR)) != 0 {
            BATCH_CURSOR += 1; continue;   // 锁定的应用批量不碰(与单点同一语义)
        }
        let ok = if hide { op_hide_pkg(bp) } else { op_show_pkg(bp) };
        BATCH_CURSOR += 1;
        if ok {
            BATCH_DONE += 1;
            // 文件已改; 节点不存在时只能等重启, 单独计数并明确告诉用户。
            if !apply_pkg(hide, bp) { BATCH_NOREG += 1; }
        } else {
            BATCH_FAIL += 1;
        }
        break;
    }
    if BATCH_CURSOR < BATCH_N && BATCH_FAIL == 0 { return; }
    REQ_BATCH = 0;
    let mut w = W::new(core::ptr::addr_of_mut!(APP_MSG) as *mut u8, 96);
    w.s(if hide { "已隐藏 ".as_bytes() } else { "已显示 ".as_bytes() })
     .n(BATCH_DONE as u32).s(" 个".as_bytes());
    if BATCH_NOREG != 0 {
        w.s(", ".as_bytes()).n(BATCH_NOREG as u32).s(" 个需重启\0".as_bytes());
    } else if BATCH_FAIL != 0 {
        w.s("(中途失败)\0".as_bytes());
    } else {
        w.s("\0".as_bytes());
    }
    w.end();
    MSG_KEEP = 1;
    crate::log::kv("批量完成, 成功 ", BATCH_DONE as u32);
    if BATCH_NOREG != 0 { crate::log::kv("批量完成, 需重启 ", BATCH_NOREG as u32); }
    if BATCH_FAIL != 0 { crate::log::kv("批量完成, 失败 ", BATCH_FAIL as u32); }
    sel_clear();
    reload();
}

unsafe fn cstr_slice(p: *const u8) -> &'static [u8] {
    core::slice::from_raw_parts(p, cstr_len(p) + 1)
}

unsafe fn wmsg(s: &str) {
    put_str(core::ptr::addr_of_mut!(APP_MSG) as *mut u8, s.as_bytes());
}

/// 写结果提示, 并让紧随其后的 reload() 不要覆盖它。
unsafe fn wmsg_keep(s: &str) {
    wmsg(s);
    MSG_KEEP = 1;
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
    let mut ver = [0u8; 24];
    let mut icon = [0u8; 32];
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
        } else if known && (key_is(key.as_ptr(), b"versionName\0")
            || key_is(key.as_ptr(), b"version\0") || key_is(key.as_ptr(), b"ver\0")) {
            let r = s_string_read(base, len, ver.as_mut_ptr(), 24, p);
            if r == 2 { return 2; }
        } else if known && key_is(key.as_ptr(), b"icon\0") {
            let r = s_string_read(base, len, icon.as_mut_ptr(), 32, p);
            if r == 2 { return 2; }
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
    let mut kv = 0usize;
    while kv < 24 { E_VER[i][kv] = ver[kv]; kv += 1; }
    let mut ki = 0usize;
    while ki < 32 { E_ICON[i][ki] = icon[ki]; ki += 1; }
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
        if du == 0 && VIEW_N < APP_MAX {
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

unsafe fn excl_checksum(b: *const u8) -> u32 {
    let mut s = 0u32;
    let mut i = EXCL_HDR;
    while i < EXCL_SUM_OFF { s = s.wrapping_add(read_volatile(b.add(i)) as u32); i += 1; }
    s
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
    let mut is_old = false;
    let mut k = 0usize;
    // 新格式必须整份长度 + 校验和都对; 旧格式(EX01)按结构校验后迁移。
    let mut new_ok = true;
    while k < EXCL_HDR {
        if read_volatile(b.add(k)) != EXCL_MAGIC[k] { new_ok = false; }
        k += 1;
    }
    if new_ok {
        if n < EXCL_TOTAL { return; }
        let want = read_volatile(b.add(EXCL_SUM_OFF) as *const u32);
        if want != excl_checksum(b) { return; }   // 损坏 -> 空名单 -> 开机补偿把 keep 全部隐藏
    } else {
        let mut old_ok = true;
        k = 0;
        while k < EXCL_HDR {
            if read_volatile(b.add(k)) != EXCL_MAGIC_OLD[k] { old_ok = false; }
            k += 1;
        }
        if !old_ok { return; }
        is_old = true;
    }
    let limit = if is_old { n } else { EXCL_SUM_OFF };
    let mut i = 0usize;
    while i < EXCL_MAX {
        let o = EXCL_HDR + i * EXCL_SLOT;
        if o + EXCL_SLOT > limit { break; }
        if read_volatile(b.add(o)) == 0 { break; }
        // 槽内必须有 NUL, 否则视为损坏(半截写入)
        let mut j = 0usize;
        while j < EXCL_SLOT && read_volatile(b.add(o + j)) != 0 { j += 1; }
        if j >= EXCL_SLOT {
            EXCL_N = 0;
            return;
        }
        let d = (core::ptr::addr_of_mut!(EXCL_PKG) as *mut u8).add(i * EXCL_SLOT);
        j = 0;
        while j < EXCL_SLOT {
            write_volatile(d.add(j), read_volatile(b.add(o + j)));
            j += 1;
        }
        write_volatile(d.add(EXCL_SLOT - 1), 0);
        EXCL_N += 1;
        i += 1;
    }
    excl_dedupe();
    if is_old { let _ = excl_save(); }
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
    let sum = excl_checksum(b);
    write_volatile(b.add(EXCL_SUM_OFF) as *mut u32, sum);
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
    crate::log::pkg(if idx == usize::MAX { "排除 " } else { "取消排除 " }, pkg);
    if idx == usize::MAX {
        // 排除项不参与批量/开机补偿; 摘掉隐藏名单并确保桌面可见, 避免"显示隐藏但实际显示"
        let _ = keep_del(pkg);
        let _ = apply_pkg(false, pkg);
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
    let keep_msg = read_volatile(core::ptr::addr_of!(MSG_KEEP) as *const u32);
    MSG_KEEP = 0;
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
                if keep_has(p) && excl_index(p) == usize::MAX {
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
    } else if keep_msg == 0 {
        wmsg("点应用切换显示/隐藏\0");
    }
    build_view();
    if read_volatile(core::ptr::addr_of!(EXCL_MSG) as *const u8) == 0 {
        eset("点一行切换排除状态\0");
    }
    // 包名集合变了 = 设备上新装/卸载了应用(Chaos 自己改隐藏状态不会变指纹)。
    // 普通隐藏模式下图标会被新应用注册顺带复原, 这里自动重放一次隐藏名单。
    let sig = reg_sig();
    if REG_SEEN != 0 && sig != REG_SIG {
        REQ_REAPPLY = 1;
    }
    REG_SIG = sig;
    REG_SEEN = 1;
    APP_LOADED = 1;
}

pub(crate) unsafe fn ensure() {
    apply_mode_ensure();
    if APP_LOADED == 0 { reload(); }
}

// ===== 生效模式: 配置/隐藏名单/开机补偿 =====

unsafe fn cfg_mode_read() -> u32 {
    let fd = fw_api::open(APPLY_CFG.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 { return 2; }
    let mut b = [0u8; 2];
    let n = fw_api::read(fd, b.as_mut_ptr(), 2);
    fw_api::close(fd);
    if n != 2 || b[0] != b'2' { return 2; }
    if b[1] == b'1' { 1 } else { 0 }
}

unsafe fn cfg_mode_save() -> bool {
    let fd = fw_api::open(APPLY_CFG.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd < 0 { return false; }
    let b = [b'2', if APPLY_MODE == 0 { b'0' } else { b'1' }];
    let n = fw_api::write(fd, b.as_ptr(), 2);
    fw_api::close(fd);
    n == 2
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
    keep_dedupe();
}

unsafe fn keep_save() -> bool {
    KEEP_BUF[0] = KEEP_MAGIC[0]; KEEP_BUF[1] = KEEP_MAGIC[1];
    KEEP_BUF[2] = KEEP_MAGIC[2]; KEEP_BUF[3] = KEEP_MAGIC[3];
    // 必须写满固定长度并把尾部清零: 只写 4+N*44 时, 文件变短后旧槽仍在,
    // keep_load 会读回已删除的包名, 表现为"取消隐藏/清空名单后重启又回来"。
    let len = KEEP_TOTAL;
    let mut z = 4 + KEEP_N * PKG_CAP;
    while z < len { write_volatile(KEEP_BUF.as_mut_ptr().add(z), 0); z += 1; }
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
                crate::log::pkg("开机补偿隐藏 ", p);
                found += 1;
            }
        }
        i += 1;
    }
    found
}

pub(crate) unsafe fn apply_mode_ensure() {
    if APPLY_LOADED != 0 { return; }
    // 统一为深度隐藏: 隐藏 = 条目在 apps.json_hide + 节点 bit0 + 无图标。
    // 普通/深度两套模式合并后不再读配置; 旧 keep 名单在 deep_apply_all 里迁移。
    APPLY_MODE = 0;
    keep_load();
    APPLY_LOADED = 1;
}

pub(crate) unsafe fn apply_mode_is_keep() -> bool { false }

/// 开关态: true = 深度隐藏(条目不在 apps.json)。
pub(crate) unsafe fn deep_on() -> bool { true }

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
    let old = APPLY_MODE;
    APPLY_MODE = if APPLY_MODE == 0 { 1 } else { 0 };
    if !cfg_mode_save() {
        APPLY_MODE = old;
        wmsg("模式配置写入失败, 未切换\0");
        return;
    }
    REQ_MODE = 1;
    crate::log::line(if APPLY_MODE == 0 { "切换模式: 深度隐藏(需重启)" } else { "切换模式: 普通隐藏" });
}

pub(crate) unsafe fn mode_pending() -> bool { REQ_MODE != 0 }

pub(crate) unsafe fn do_mode_apply() {
    if REQ_MODE == 0 { return; }
    REQ_MODE = 0;
    if APPLY_MODE == 0 {
        // 打开深度隐藏: 普通隐藏(keep)的条目搬进 apps.json_hide。
        // 逻辑上仍然隐藏, 只是存储方式从"保留条目+名单"换成"移出注册表"。
        keep_load();
        let mut moved = 0usize;
        while moved < KEEP_N {
            let p = KEEP_BUF.as_ptr().add(4 + moved * PKG_CAP);
            if !move_vis_to_hide(p) {
                // 部分搬运会留下"一半在 apps.json、一半在 hide"的分裂状态, 必须回滚。
                let mut j = 0usize;
                while j < moved {
                    let q = KEEP_BUF.as_ptr().add(4 + j * PKG_CAP);
                    if move_hide_to_vis(q) { apply_pkg(false, q); }
                    j += 1;
                }
                reload();
                wmsg_keep("切换失败, 已回滚; 请重试\0");
                return;
            }
            apply_pkg(true, p);
            moved += 1;
        }
        KEEP_N = 0;
        let _ = keep_save();
        reload();
        wmsg_keep("深度隐藏已开启, 重启后生效\0");
        crate::log::line("模式迁移: keep -> apps.json_hide 完成");
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
        let keep_before = KEEP_N;
        let mut moved = 0usize;
        while moved < MIG_N {
            let p = (core::ptr::addr_of!(MIG_BUF) as *const u8).add(moved * PKG_CAP);
            if !move_hide_to_vis(p) {
                let mut j = 0usize;
                while j < moved {
                    let q = (core::ptr::addr_of!(MIG_BUF) as *const u8).add(j * PKG_CAP);
                    if move_vis_to_hide(q) { apply_pkg(true, q); }
                    j += 1;
                }
                KEEP_N = keep_before;          // 撤掉失败前追加的 keep 项
                let _ = keep_save();
                reload();
                wmsg_keep("切换失败, 已回滚; 请重试\0");
                return;
            }
            keep_add(p);
            moved += 1;
        }
        let _ = keep_save();
        reload();
        wmsg_keep("深度隐藏已关闭, 存储已迁移; 请重启后生效\0");
        crate::log::line("模式迁移: apps.json_hide -> apps.json 完成");
    }
}

static mut SNAP: [[u8; PKG_CAP]; APP_MAX] = [[0; PKG_CAP]; APP_MAX];

/// 深度隐藏的统一重放: 隐藏表(以及旧版 keep 名单)里的每个应用,
/// ①若还在 apps.json 里就搬进 apps.json_hide; ②把节点 bit0 置 1 并摘图标。
/// 返回 (命中节点数, 迁移条目数, 隐藏表条目数)。
unsafe fn deep_apply_all() -> (usize, usize, usize) {
    let mut n = 0usize;
    APP_N = 0;
    if load_file(HID_PATH.as_ptr()) {
        if parse_registry(1) == 0 {
            let base = core::ptr::addr_of!(E_PKG) as *const u8;
            let mut i = 0usize;
            while i < APP_N && n < APP_MAX {
                let dst = (core::ptr::addr_of_mut!(SNAP) as *mut u8).add(n * PKG_CAP);
                cpy_n(dst, base.add(i * PKG_CAP), PKG_CAP);
                n += 1;
                i += 1;
            }
        }
    }
    keep_load();
    let mut k = 0usize;
    while k < KEEP_N && n < APP_MAX {
        let p = KEEP_BUF.as_ptr().add(4 + k * PKG_CAP);
        let mut dup = false;
        let mut j = 0usize;
        while j < n {
            if cstr_eq((core::ptr::addr_of!(SNAP) as *const u8).add(j * PKG_CAP), p) { dup = true; break; }
            j += 1;
        }
        if !dup {
            let dst = (core::ptr::addr_of_mut!(SNAP) as *mut u8).add(n * PKG_CAP);
            cpy_n(dst, p, PKG_CAP);
            n += 1;
        }
        k += 1;
    }
    let mut found = 0usize;
    let mut moved = 0usize;
    let mut i = 0usize;
    while i < n {
        let p = (core::ptr::addr_of!(SNAP) as *const u8).add(i * PKG_CAP);
        if capture(p, 0) {
            if move_vis_to_hide(p) { moved += 1; }
        }
        let node = find_node(p);
        if node != 0 {
            set_node_hidden(node, true);
            let id = crate::mem::rd16((node + fw_api::APPN_ID) as *const u16) as u32;
            if id != 0 { fw_api::app_launcher_del(id); found += 1; }
        }
        i += 1;
    }
    if KEEP_N != 0 { KEEP_N = 0; let _ = keep_save(); }
    reload();
    (found, moved, n)
}

/// tick: 重放隐藏名单 —— 进页时发现注册表包名集合变化(新装应用)会自动触发,
/// 也可由应用管理页的"重新应用隐藏"按钮触发。
///
/// 先补注册表节点再重放隐藏: 深度隐藏把条目搬进 apps.json_hide 后, 重启时固件
/// 不会为这些应用建注册表节点, 于是既无法启动也无法置隐藏位。这里走固件自己的
/// "手机重同步"路径(proxyquickapp 应用表 → quickapp_register_app)把缺的节点补回来,
/// 只补查不到的, 已在册的应用一律不动。普通模式下通常没有缺节点, 这一步是空转。
pub(crate) unsafe fn do_reapply() {
    if REQ_REAPPLY == 0 { return; }
    REQ_REAPPLY = 0;

    // 深度隐藏: 先把缺的注册表节点补回来, 再把隐藏表里的条目统一搬回隐藏态。
    let (total, rebuilt) = fw_api::quickapp_resync_missing();
    if rebuilt > 0 {
        crate::log::kv("补建快应用节点, 应用表合计 ", total);
        crate::log::kv("补建快应用节点, 本次新建 ", rebuilt);
    }
    let (found, moved, n) = deep_apply_all();
    let d = core::ptr::addr_of_mut!(APP_MSG) as *mut u8;
    let mut w = put_lit(d, "已重新应用隐藏 ".as_bytes());
    w += put_u32(d.add(w), found as u32);
    w += put_lit(d.add(w), "/".as_bytes());
    w += put_u32(d.add(w), n as u32);
    if moved > 0 {
        w += put_lit(d.add(w), " 迁移 ".as_bytes());
        w += put_u32(d.add(w), moved as u32);
    }
    if rebuilt > 0 {
        w += put_lit(d.add(w), " 补建 ".as_bytes());
        w += put_u32(d.add(w), rebuilt);
    }
    write_volatile(d.add(w), 0);
    MSG_KEEP = 1;
    crate::log::kv("重新应用隐藏, 命中 ", found as u32);
    if moved > 0 { crate::log::kv("重新应用隐藏, 迁移 ", moved as u32); }
}

/// 开机补偿: 模块加载后由维护定时器(UI 线程)调用。只对保留条目模式生效,
/// 把 Chaos 隐藏名单里的应用重新置位并摘图标; 找不到节点时有限次重试。
pub(crate) unsafe fn boot_apply() {
    if BOOT_APPLIED != 0 { return; }
    apply_mode_ensure();
    excl_ensure();
    if BOOT_TRIES == 0 { crate::log::line("开机补偿: 深度隐藏重放开始"); }
    BOOT_WAIT = BOOT_WAIT.wrapping_add(1);
    // 注册表/应用表可能比模块晚就绪: 每 40 拍试一次, 最多 5 分钟。
    if BOOT_WAIT % 40 != 0 { return; }
    BOOT_TRIES = BOOT_TRIES.wrapping_add(1);
    let _ = fw_api::quickapp_resync_missing();
    let (found, moved, n) = deep_apply_all();
    if n == 0 || found >= n || BOOT_TRIES >= 300 {
        BOOT_APPLIED = 1;
        crate::log::kv("开机补偿: 隐藏条目 ", n as u32);
        crate::log::kv("开机补偿: 命中 ", found as u32);
        if moved > 0 { crate::log::kv("开机补偿: 迁移 ", moved as u32); }
    }
}

/// 固件运行期注册表的指纹(包名集合 + 节点数 / app_id)。
///
/// 与 reg_sig() 读文件不同, 这里读的是**内存 APP_REGISTRY 链**: 应用注册/退出会改它,
/// Chaos 自己改隐藏位不会。换绑重连、手机重发 rpk_info 后固件重注册应用, 这个指纹必变。
unsafe fn reg_sig_rt() -> u32 {
    let head = crate::mem::rd32(fw_api::APP_REGISTRY as *const u32);
    if head == 0 || crate::mem::safe_ptr(head) == false { return 0; }
    let mut node = crate::mem::rd32((head + 4) as *const u32);
    let mut h = 0u32;
    let mut n = 0u32;
    let mut guard = 0u32;
    while crate::mem::safe_ptr(node) && node != head && guard < 512 {
        guard += 1;
        let np = crate::mem::rd32((node + 0x08) as *const u32) as *const u8;
        if crate::mem::safe_ptr(np as u32) {
            let mut k = 0usize;
            while k < 44 {
                let c = read_volatile(np.add(k));
                if c == 0 { break; }
                h = h.wrapping_mul(131).wrapping_add(c as u32);
                k += 1;
            }
        }
        n = n.wrapping_add(1);
        node = crate::mem::rd32((node + 4) as *const u32);
    }
    h.wrapping_mul(31).wrapping_add(n)
}

/// 运行期隐藏看门狗(维护定时器里调)。
///
/// 为什么需要: 普通隐藏(APPLY_MODE=1)的隐藏状态是**运行期**的 —— 隐藏名单在
/// /data/chaos/appmgr_keep.bin, 生效靠 node+0x3C bit0 + 摘图标。任何一次固件重注册
/// (换绑新手机后手机重下发 rpk_info、重装、升级)都会建出新节点, bit0 默认 0,
/// 图标就被 add 回来了; 开机补偿只在开机跑一次, 页1 的自动重放又要求页面打开。
/// 所以换绑后所有隐藏应用会一起冒出来。
///
/// 这里低频巡检两条: ①注册表包名集合变了 -> 整名单重放(补 bit + 摘图标);
/// ②兜底逐项查 bit0, 防止集合没变但节点被重建的情况。
pub(crate) unsafe fn watch_keep() {
    apply_mode_ensure();
    excl_ensure();
    WATCH_WAIT = WATCH_WAIT.wrapping_add(1);
    // 维护定时器快拍 50ms / 慢拍 1s: 4 拍 = 快时 0.2s、慢时 4s, 足够挡住重注册。
    if WATCH_WAIT % 4 != 0 { return; }
    if WATCH_BUSY != 0 { return; }
    WATCH_BUSY = 1;

    let sig = reg_sig_rt();
    if WATCH_SEEN == 0 {
        WATCH_SEEN = 1;
        WATCH_SIG = sig;
    } else if sig != WATCH_SIG {
        // 重装/换绑时固件是一条条重注册的, 注册期间指纹每拍都在变。
        // 先记下来等这一批注册完(指纹安静一拍)再统一重放, 避免逐条重放打满 UI 线程。
        WATCH_SIG = sig;
        WATCH_CHG = 1;
    } else if WATCH_CHG != 0 {
        WATCH_CHG = 0;
        let _ = fw_api::quickapp_resync_missing();
        let (found, moved, _n) = deep_apply_all();
        if found != 0 || moved != 0 {
            crate::log::kv("看门狗: 注册表变化, 补隐藏 ", found as u32);
            if moved != 0 { crate::log::kv("看门狗: 迁移回隐藏表 ", moved as u32); }
        }
    } else if WATCH_WAIT % 240 == 0 {
        // 兜底: 集合没变但节点被重建/bit0 被清, 每分钟(慢拍 4 分钟)整表重放一次。
        let (found, _moved, _n) = deep_apply_all();
        if found != 0 { crate::log::kv("看门狗: 周期补隐藏 ", found as u32); }
    }
    WATCH_BUSY = 0;
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
    let mut appended = false;
    if !file_has_pkg(pkg) {
        let n = serialize(usize::MAX, 0, 2);
        if n == 0 || !write_file(HID_PATH.as_ptr(), n) { return false; }
        appended = true;
    }
    // 2) 可见注册表: 删掉该包名的所有副本
    if !remove_all_pkg(pkg, 0) {
        if appended { let _ = remove_all_pkg(pkg, 1); }   // 回滚: 别把副本留在隐藏表
        return false;
    }
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
    let mut appended = false;
    if !file_has_pkg(pkg) {
        let n = serialize(usize::MAX, 0, 1);
        if n == 0 || !write_file(VIS_PATH.as_ptr(), n) { return false; }
        appended = true;
    }
    // 2) 隐藏注册表: 删掉该包名的所有副本
    if !remove_all_pkg(pkg, 1) {
        if appended { let _ = remove_all_pkg(pkg, 0); }
        return false;
    }
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

/// 旧版本可能留下重复包名: 读到内存后按包名收敛，只保留第一份。
unsafe fn keep_dedupe() {
    let base = core::ptr::addr_of_mut!(KEEP_BUF) as *mut u8;
    let mut a = 0usize;
    while a < KEEP_N {
        let pa = base.add(4 + a * PKG_CAP);
        let mut dup = false;
        let mut b = 0usize;
        while b < a {
            if cstr_eq(base.add(4 + b * PKG_CAP), pa) { dup = true; break; }
            b += 1;
        }
        if dup {
            let mut k = a;
            while k + 1 < KEEP_N {
                let mut j = 0usize;
                while j < PKG_CAP {
                    write_volatile(base.add(4 + k * PKG_CAP + j),
                        read_volatile(base.add(4 + (k + 1) * PKG_CAP + j)));
                    j += 1;
                }
                k += 1;
            }
            KEEP_N -= 1;
        } else {
            a += 1;
        }
    }
}

unsafe fn excl_dedupe() {
    let base = core::ptr::addr_of_mut!(EXCL_PKG) as *mut u8;
    let mut a = 0usize;
    while a < EXCL_N {
        let pa = base.add(a * EXCL_SLOT);
        let mut dup = false;
        let mut b = 0usize;
        while b < a {
            if cstr_eq(base.add(b * EXCL_SLOT), pa) { dup = true; break; }
            b += 1;
        }
        if dup {
            let mut k = a;
            while k + 1 < EXCL_N {
                let mut j = 0usize;
                while j < EXCL_SLOT {
                    write_volatile(base.add(k * EXCL_SLOT + j),
                        read_volatile(base.add((k + 1) * EXCL_SLOT + j)));
                    j += 1;
                }
                k += 1;
            }
            EXCL_N -= 1;
        } else {
            a += 1;
        }
    }
}

// ===== 应用详情 / 批量选择 / 卸载(2026-11 界面重构) =====
//
// 详情页: 名称/包名/大小/版本/隐藏态 + 隐藏-显示切换 + 卸载(二次确认)。
// 卸载按 SmartHome 的做法: 删六个目录 + 从两份注册表里摘条目 + 摘桌面图标,
// 不走固件的卸载命令, 也不需要重启。
// 批量页: 勾选集合, 支持全选/取消全选、隐藏选中、显示选中、删除选中。


// 快应用在各固件版本里落盘位置不完全一致; 逐个候选求和/删除, 不存在的自然贡献 0。
// 依据: 固件字符串 "/data/app/%s/%s"(图标)、"/data/quickapp/"(config/rpk_info)、
// "data/quickapp/system"、"/data/quickapp/cache"; SmartHome 另外尝试 /data/ 下。
const APP_DIRS: [&[u8]; 4] = [
    b"/data/app/",
    b"/data/quickapp/app/",
    b"/data/app/quickapp/",
    b"/data/quickapp/",
];
const SUB_DIRS: [&[u8]; 8] = [
    b"/data/quickapp/system/",
    b"/data/quickapp/cache/",
    b"/data/quickapp/files/",
    b"/data/quickapp/mass/",
    b"/data/app/system/",
    b"/data/cache/",
    b"/data/files/",
    b"/data/mass/",
];

unsafe fn mk_path3(root: &[u8], mid: &[u8], pkg: *const u8, suffix: &[u8]) -> usize {
    let buf = core::ptr::addr_of_mut!(D_WORK) as *mut u8;
    let cap = 300usize;
    let mut o = 0usize;
    let mut i = 0usize;
    while i < root.len() - 1 && o + 1 < cap { write_volatile(buf.add(o), root[i]); o += 1; i += 1; }
    i = 0;
    while i < mid.len() && o + 1 < cap { write_volatile(buf.add(o), mid[i]); o += 1; i += 1; }
    let mut k = 0usize;
    while o + 1 < cap {
        let c = read_volatile(pkg.add(k));
        if c == 0 { break; }
        write_volatile(buf.add(o), c); o += 1; k += 1;
    }
    let mut j = 0usize;
    while j < suffix.len() && o + 1 < cap { write_volatile(buf.add(o), suffix[j]); o += 1; j += 1; }
    write_volatile(buf.add(o), 0);
    o
}

unsafe fn path_exists(path: *const u8) -> bool {
    let dp = fw_api::opendir(path);
    if dp != 0 { fw_api::closedir(dp); return true; }
    let fd = fw_api::open(path, fw_api::oflag::RDONLY, 0);
    if fd >= 0 { fw_api::close(fd); return true; }
    false
}

static mut D_WORK: [u8; 300] = [0; 300];
static mut D_SIZE: u32 = 0;
static mut D_VERBUF: [u8; 3000] = [0; 3000];

static mut D_IDX: usize = usize::MAX;
static mut D_PKG: [u8; PKG_CAP] = [0; PKG_CAP];
static mut D_NAME: [u8; 32] = [0; 32];
static mut D_HIDDEN: u8 = 0;
static mut D_BYTES: u32 = 0;
static mut HITS: u32 = 0;
static mut D_VER: [u8; 24] = [0; 24];
static mut D_CONFIRM: u32 = 0;
// 0 = 从应用管理进详情(隐藏/显示按钮), 1 = 从排除项页进详情(排除/取消排除按钮)
static mut D_EXCL: u32 = 0;

static mut SEL: [u8; APP_MAX] = [0; APP_MAX];
static mut SEL_N: usize = 0;
static mut SEL_DEL_CONFIRM: u32 = 0;

static mut DEL_PKG: [[u8; PKG_CAP]; APP_MAX] = [[0; PKG_CAP]; APP_MAX];
static mut DEL_N: usize = 0;
static mut DEL_CUR: usize = 0;
static mut DEL_OK: usize = 0;
static mut DEL_FAIL: usize = 0;
static mut REQ_DEL: u32 = 0;

unsafe fn cpy_n(dst: *mut u8, src: *const u8, cap: usize) {
    let mut k = 0usize;
    while k < cap {
        let c = read_volatile(src.add(k));
        write_volatile(dst.add(k), c);
        if c == 0 { return; }
        k += 1;
    }
    write_volatile(dst.add(cap - 1), 0);
}

unsafe fn file_bytes(path: *const u8) -> u32 {
    let fd = fw_api::open(path, fw_api::oflag::RDONLY, 0);
    if fd < 0 { return 0; }
    let sz = fw_api::lseek(fd, 0, fw_api::seek::END);
    fw_api::close(fd);
    if sz < 0 { 0 } else { sz as u32 }
}

unsafe fn calc_walk(plen: usize, depth: u32) {
    let dp = fw_api::opendir((core::ptr::addr_of!(D_WORK) as *const u8));
    if dp == 0 { return; }
    let buf = core::ptr::addr_of_mut!(D_WORK) as *mut u8;
    let mut iter = 0usize;
    loop {
        if iter >= 400 { break; }
        iter += 1;
        let de = fw_api::readdir(dp);
        if de == 0 { break; }
        let dt = crate::mem::rd8(de as *const u8);
        let nm = (de + 1) as *const u8;
        let n1 = read_volatile(nm);
        let n2 = read_volatile(nm.add(1));
        let n3 = read_volatile(nm.add(2));
        if n1 == b'.' && (n2 == 0 || (n2 == b'.' && n3 == 0)) { continue; }
        let mut o = plen;
        if o > 0 && read_volatile(buf.add(o - 1)) != b'/' {
            if o + 1 >= 300 { continue; }
            write_volatile(buf.add(o), b'/'); o += 1;
        }
        let mut k = 0usize;
        while o + 1 < 300 {
            let c = read_volatile(nm.add(k));
            if c == 0 { break; }
            write_volatile(buf.add(o), c); o += 1; k += 1;
        }
        write_volatile(buf.add(o), 0);
        if dt == crate::state::DT_REG {
            D_BYTES = D_BYTES.saturating_add(file_bytes(buf as *const u8));
        } else if depth < 6 {
            // 目录项类型未知(0)时不能只当文件: 先试当目录打开
            let dp2 = fw_api::opendir(buf as *const u8);
            if dp2 != 0 {
                fw_api::closedir(dp2);
                calc_walk(o, depth + 1);
            } else {
                D_BYTES = D_BYTES.saturating_add(file_bytes(buf as *const u8));
            }
        }
        write_volatile(buf.add(plen), 0);
    }
    fw_api::closedir(dp);
}

unsafe fn del_walk(plen: usize, depth: u32) {
    let dp = fw_api::opendir((core::ptr::addr_of!(D_WORK) as *const u8));
    if dp == 0 { return; }
    let buf = core::ptr::addr_of_mut!(D_WORK) as *mut u8;
    let mut iter = 0usize;
    loop {
        if iter >= 400 { break; }
        iter += 1;
        let de = fw_api::readdir(dp);
        if de == 0 { break; }
        let dt = crate::mem::rd8(de as *const u8);
        let nm = (de + 1) as *const u8;
        let n1 = read_volatile(nm);
        let n2 = read_volatile(nm.add(1));
        let n3 = read_volatile(nm.add(2));
        if n1 == b'.' && (n2 == 0 || (n2 == b'.' && n3 == 0)) { continue; }
        let mut o = plen;
        if o > 0 && read_volatile(buf.add(o - 1)) != b'/' {
            if o + 1 >= 300 { continue; }
            write_volatile(buf.add(o), b'/'); o += 1;
        }
        let mut k = 0usize;
        while o + 1 < 300 {
            let c = read_volatile(nm.add(k));
            if c == 0 { break; }
            write_volatile(buf.add(o), c); o += 1; k += 1;
        }
        write_volatile(buf.add(o), 0);
        if dt == crate::state::DT_REG {
            if fw_api::fs_remove(buf as *const u8) < 0 { DEL_FAIL += 1; }
        } else if depth < 6 {
            let dp2 = fw_api::opendir(buf as *const u8);
            if dp2 != 0 {
                fw_api::closedir(dp2);
                del_walk(o, depth + 1);
            }
            if fw_api::fs_remove(buf as *const u8) < 0 { DEL_FAIL += 1; }
        }
        write_volatile(buf.add(plen), 0);
    }
    fw_api::closedir(dp);
}

unsafe fn scan_size(pkg: *const u8) {
    D_BYTES = 0;
    HITS = 0;
    let mut i = 0usize;
    while i < APP_DIRS.len() {
        let plen = mk_path3(APP_DIRS[i], b"", pkg, b"");
        if path_exists(core::ptr::addr_of!(D_WORK) as *const u8) { HITS += 1; calc_walk(plen, 0); }
        i += 1;
    }
    let mut j = 0usize;
    while j < SUB_DIRS.len() {
        let plen = mk_path3(SUB_DIRS[j], b"", pkg, b"");
        if path_exists(core::ptr::addr_of!(D_WORK) as *const u8) { HITS += 1; calc_walk(plen, 0); }
        j += 1;
    }
    // 旧版本目录: 优先 app/ 与 system/ 下的 <pkg>.old
    let olds: [&[u8]; 3] = [b"/data/app/", b"/data/quickapp/system/", b"/data/app/system/"];
    let mut k = 0usize;
    while k < olds.len() {
        let plen = mk_path3(olds[k], b"", pkg, b".old");
        if path_exists(core::ptr::addr_of!(D_WORK) as *const u8) { HITS += 1; calc_walk(plen, 0); }
        k += 1;
    }
    D_SIZE = D_BYTES;
    crate::log::kv("详情大小(字节) ", D_BYTES);
    crate::log::kv("命中的目录数 ", HITS);
}

unsafe fn rm_app_dirs(pkg: *const u8) {
    let mut i = 0usize;
    while i < APP_DIRS.len() {
        let plen = mk_path3(APP_DIRS[i], b"", pkg, b"");
        del_walk(plen, 0);
        let _ = fw_api::fs_remove((core::ptr::addr_of!(D_WORK) as *const u8));
        i += 1;
    }
    let mut j = 0usize;
    while j < SUB_DIRS.len() {
        let plen = mk_path3(SUB_DIRS[j], b"", pkg, b"");
        del_walk(plen, 0);
        let _ = fw_api::fs_remove((core::ptr::addr_of!(D_WORK) as *const u8));
        j += 1;
    }
    let olds: [&[u8]; 3] = [b"/data/app/", b"/data/quickapp/system/", b"/data/app/system/"];
    let mut k = 0usize;
    while k < olds.len() {
        let plen = mk_path3(olds[k], b"", pkg, b".old");
        del_walk(plen, 0);
        let _ = fw_api::fs_remove((core::ptr::addr_of!(D_WORK) as *const u8));
        k += 1;
    }
}

/// 从 rpk_info.json 里取该应用的 versionName(取不到就空)。
unsafe fn scan_version(pkg: *const u8) {
    // 1) 注册表条目里就带 versionName(SmartHome 也是读这个, 不再依赖 rpk_info.json)
    let ebase = core::ptr::addr_of!(E_PKG) as *const u8;
    let mut ei = 0usize;
    while ei < APP_N {
        if cstr_eq(ebase.add(ei * PKG_CAP), pkg) {
            let vp = (core::ptr::addr_of!(E_VER) as *const u8).add(ei * 24);
            if read_volatile(vp) != 0 {
                let dst = core::ptr::addr_of_mut!(D_VER) as *mut u8;
                let mut k = 0usize;
                while k < 23 {
                    let c = read_volatile(vp.add(k));
                    write_volatile(dst.add(k), c);
                    if c == 0 { break; }
                    k += 1;
                }
                write_volatile(dst.add(23), 0);
                return;
            }
            break;
        }
        ei += 1;
    }
    write_volatile((core::ptr::addr_of_mut!(D_VER) as *mut u8).add(0), 0);
    let mut path: *const u8 = b"/data/app/quickapp/rpk_info.json\0".as_ptr();
    let mut fd = fw_api::open(path, fw_api::oflag::RDONLY, 0);
    if fd < 0 {
        path = b"/quickapp/rpk_info.json\0".as_ptr();
        fd = fw_api::open(path, fw_api::oflag::RDONLY, 0);
        if fd < 0 { return; }
    }
    let buf = core::ptr::addr_of_mut!(D_VERBUF) as *mut u8;
    let n = fw_api::read(fd, buf, 3000);
    fw_api::close(fd);
    if n <= 0 { return; }
    let len = n as usize;
    let mut i = 0usize;
    let mut hit = usize::MAX;
    while i + 1 < len {
        if read_volatile(buf.add(i)) == b'"' {
            // 包名串匹配(package / packageName 的值)
            let mut k = 0usize;
            loop {
                let c = read_volatile(pkg.add(k));
                if c == 0 { break; }
                if read_volatile(buf.add(i + 1 + k)) != c { break; }
                k += 1;
            }
            let mut e = i + 1 + k;
            while e < len { let c = read_volatile(buf.add(e)); if c == b' ' { e += 1; } else { break; } }
            if k > 0 && read_volatile(pkg.add(k)) == 0 && read_volatile(buf.add(i + 1 + k)) == b'"' {
                hit = i;
                break;
            }
            let _ = e;
        }
        i += 1;
    }
    if hit == usize::MAX { return; }
    // 在包名之后找 "versionName"
    let lit: &[u8] = b"\"versionName\"";
    let mut j = hit;
    let end = if hit + 512 < len { hit + 512 } else { len };
    while j + lit.len() < end {
        let mut ok = true;
        let mut k = 0usize;
        while k < lit.len() {
            if read_volatile(buf.add(j + k)) != lit[k] { ok = false; break; }
            k += 1;
        }
        if ok {
            let mut q = j + lit.len();
            while q < len {
                let c = read_volatile(buf.add(q));
                if c == b':' { q += 1; break; }
                if c == b'"' { return; }
                q += 1;
            }
            while q < len { let c = read_volatile(buf.add(q)); if c == b'"' { q += 1; break; } q += 1; }
            let out = core::ptr::addr_of_mut!(D_VER) as *mut u8;
            let mut k = 0usize;
            while q < len && k < 23 {
                let c = read_volatile(buf.add(q));
                if c == 0 || c == b'"' { break; }
                write_volatile(out.add(k), c); k += 1; q += 1;
            }
            write_volatile(out.add(k), 0);
            return;
        }
        j += 1;
    }
}

pub(crate) unsafe fn detail_open(i: usize) { detail_open_ctx(i, 0) }

pub(crate) unsafe fn detail_open_ctx(i: usize, excl: u32) {
    D_IDX = usize::MAX;
    D_CONFIRM = 0;
    D_EXCL = excl;
    if i >= APP_N { return; }
    D_IDX = i;
    cpy_n(core::ptr::addr_of_mut!(D_PKG) as *mut u8, pkg_ptr(i), PKG_CAP);
    cpy_n(core::ptr::addr_of_mut!(D_NAME) as *mut u8, entry_primary(i), 32);
    D_HIDDEN = if visible(i) { 0 } else { 1 };
    scan_size(core::ptr::addr_of!(D_PKG) as *const u8);
    scan_version(core::ptr::addr_of!(D_PKG) as *const u8);
}

pub(crate) unsafe fn detail_excl_ctx() -> bool { D_EXCL != 0 }

pub(crate) unsafe fn detail_valid() -> bool { D_IDX != usize::MAX && D_IDX < APP_N }
pub(crate) unsafe fn detail_hidden() -> bool { D_HIDDEN != 0 }
pub(crate) unsafe fn detail_bytes() -> u32 { D_SIZE }
pub(crate) unsafe fn detail_confirm() -> bool { D_CONFIRM != 0 }
pub(crate) unsafe fn detail_name() -> &'static [u8] {
    let p = core::ptr::addr_of!(D_NAME) as *const u8;
    core::slice::from_raw_parts(p, cstr_len(p) + 1)
}
pub(crate) unsafe fn detail_pkg() -> &'static [u8] {
    let p = core::ptr::addr_of!(D_PKG) as *const u8;
    core::slice::from_raw_parts(p, cstr_len(p) + 1)
}
pub(crate) unsafe fn detail_ver() -> &'static [u8] {
    let p = core::ptr::addr_of!(D_VER) as *const u8;
    core::slice::from_raw_parts(p, cstr_len(p) + 1)
}

/// 详情页里切换隐藏/显示: 复用单击流程(REQ_OP), tick 里执行。
pub(crate) unsafe fn detail_excl_on() -> bool { detail_valid() && excl_on(D_IDX) }

pub(crate) unsafe fn detail_excl_label() -> &'static [u8] {
    if detail_excl_on() { "取消排除\0".as_bytes() } else { "加入排除\0".as_bytes() }
}

pub(crate) unsafe fn detail_toggle_excl() {
    if detail_valid() { request_excl(D_IDX); }
}

pub(crate) unsafe fn detail_toggle_hide() { if detail_valid() { click(D_IDX); } }

pub(crate) unsafe fn detail_toggle() {
    if !detail_valid() { return; }
    if D_EXCL != 0 {
        request_excl(D_IDX);
    } else {
        click(D_IDX);
    }
}

/// 详情页主操作按钮文案(随入口变化)
pub(crate) unsafe fn detail_action_label() -> &'static [u8] {
    if D_EXCL != 0 {
        if excl_on(D_IDX) { "移出排除\0".as_bytes() } else { "加入排除\0".as_bytes() }
    } else if D_HIDDEN != 0 {
        "取消隐藏\0".as_bytes()
    } else {
        "隐藏应用\0".as_bytes()
    }
}

/// 详情页操作完成后刷新(列表已 reload, 按包名找回新下标)。
pub(crate) unsafe fn detail_refresh() {
    D_CONFIRM = 0;
    let mut i = 0usize;
    let base = core::ptr::addr_of!(E_PKG) as *const u8;
    while i < APP_N {
        if cstr_eq(base.add(i * PKG_CAP), core::ptr::addr_of!(D_PKG) as *const u8) {
            D_IDX = i;
            D_HIDDEN = if visible(i) { 0 } else { 1 };
            return;
        }
        i += 1;
    }
    D_IDX = usize::MAX;
}

/// 卸载确认: 第一次点击=armed, 第二次才执行。返回 true = 已执行。
pub(crate) unsafe fn detail_uninstall_click() -> bool {
    if D_CONFIRM == 0 { D_CONFIRM = 1; return false; }
    D_CONFIRM = 0;
    uninstall_pkg(core::ptr::addr_of!(D_PKG) as *const u8, core::ptr::addr_of!(D_NAME) as *const u8)
}

/// 真正卸载: 摘图标 → 删六个目录 → 从两份注册表摘条目 → reload。
unsafe fn uninstall_pkg(pkg: *const u8, name: *const u8) -> bool {
    if write_blocked() { wmsg_keep("列表不完整, 禁止卸载\0"); return false; }
    let node = find_node(pkg);
    if node != 0 {
        let id = crate::mem::rd16((node + fw_api::APPN_ID) as *const u16) as u32;
        if id != 0 { fw_api::app_launcher_del(id); }
    }
    DEL_FAIL = 0;
    rm_app_dirs(pkg);
    let _ = remove_all_pkg(pkg, 0);
    let _ = remove_all_pkg(pkg, 1);
    let mut w = W::new(core::ptr::addr_of_mut!(APP_MSG) as *mut u8, 96);
    w.s("已卸载 ".as_bytes()).s(cstr_slice(name));
    if DEL_FAIL != 0 { w.s(", 部分文件删除失败".as_bytes()); }
    w.s("\0".as_bytes());
    w.end();
    MSG_KEEP = 1;
    crate::log::cstr(core::ptr::addr_of!(APP_MSG) as *const u8);
    reload();
    true
}

/// 批量选择: E 下标为键(页面用视图下标换算)。
pub(crate) unsafe fn sel_clear() { let p = core::ptr::addr_of_mut!(SEL) as *mut u8; let mut i=0; while i<APP_MAX { write_volatile(p.add(i),0); i+=1; } SEL_N = 0; }
pub(crate) unsafe fn sel_count() -> usize { SEL_N }
pub(crate) unsafe fn sel_has(i: usize) -> bool { i < APP_MAX && read_volatile((core::ptr::addr_of!(SEL) as *const u8).add(i)) != 0 }
pub(crate) unsafe fn batch_total() -> usize { view_count() }

pub(crate) unsafe fn sel_toggle(i: usize) {
    if i >= APP_N { return; }
    if read_volatile((core::ptr::addr_of!(E_EXCL) as *const u8).add(i)) != 0 { return; }
    if read_volatile((core::ptr::addr_of!(E_LOCK) as *const u8).add(i)) != 0 { return; }
    let p = (core::ptr::addr_of_mut!(SEL) as *mut u8).add(i);
    let v = read_volatile(p);
    if v == 0 { write_volatile(p, 1); SEL_N += 1; } else { write_volatile(p, 0); SEL_N -= 1; }
}

/// 全选/取消全选当前视图(排除锁定与排除项)。
pub(crate) unsafe fn sel_toggle_all() {
    if SEL_N != 0 { sel_clear(); return; }
    let n = view_count();
    let mut v = 0usize;
    while v < n {
        let i = view_at(v);
        if i != usize::MAX { sel_toggle(i); }
        v += 1;
    }
}

/// 把选中项排进批量队列; kind: 1=隐藏 2=显示。
pub(crate) unsafe fn batch_request_sel(hide: bool) -> usize {
    if REQ_BATCH != 0 || REQ_OP != 0 || SEL_N == 0 { return 0; }
    if write_blocked() { wmsg_keep("列表不完整, 禁止改写\0"); return 0; }
    BATCH_N = 0; BATCH_CURSOR = 0; BATCH_DONE = 0; BATCH_FAIL = 0; BATCH_NOREG = 0;
    let n = view_count();
    let mut v = 0usize;
    while v < n && BATCH_N < APP_MAX {
        let i = view_at(v);
        if i != usize::MAX && sel_has(i) {
            let want = if hide { 0u8 } else { 1u8 };
            let src = read_volatile((core::ptr::addr_of!(E_SRC) as *const u8).add(i));
            if src == want {
                let pp = (core::ptr::addr_of!(E_PKG) as *const u8).add(i * PKG_CAP);
                let dp = (core::ptr::addr_of_mut!(BATCH_PKG) as *mut u8).add(BATCH_N * PKG_CAP);
                cpy_n(dp, pp, PKG_CAP);
                write_volatile((core::ptr::addr_of_mut!(BATCH_LOCK) as *mut u8).add(BATCH_N), 0);
                BATCH_N += 1;
            }
        }
        v += 1;
    }
    if BATCH_N == 0 { wmsg_keep("选中的应用已经是目标状态\0"); return 0; }
    REQ_BATCH = if hide { 1 } else { 2 };
    BATCH_N
}

pub(crate) unsafe fn sel_del_confirmed() -> bool { SEL_DEL_CONFIRM != 0 }
pub(crate) unsafe fn sel_del_arm() { SEL_DEL_CONFIRM = 1; }
pub(crate) unsafe fn sel_del_disarm() { SEL_DEL_CONFIRM = 0; }

/// 删除选中: 入队, tick 里逐条卸载(一拍照一个)。
pub(crate) unsafe fn batch_request_del() -> usize {
    if REQ_DEL != 0 || SEL_N == 0 { return 0; }
    if write_blocked() { wmsg_keep("列表不完整, 禁止卸载\0"); return 0; }
    DEL_N = 0; DEL_CUR = 0; DEL_OK = 0; DEL_FAIL = 0;
    let n = view_count();
    let mut v = 0usize;
    while v < n && DEL_N < APP_MAX {
        let i = view_at(v);
        if i != usize::MAX && sel_has(i) {
            if read_volatile((core::ptr::addr_of!(E_LOCK) as *const u8).add(i)) == 0 {
                let pp = (core::ptr::addr_of!(E_PKG) as *const u8).add(i * PKG_CAP);
                cpy_n((core::ptr::addr_of_mut!(DEL_PKG) as *mut u8).add(DEL_N * PKG_CAP), pp, PKG_CAP);
                DEL_N += 1;
            }
        }
        v += 1;
    }
    if DEL_N == 0 { wmsg_keep("没有可卸载的应用\0"); return 0; }
    REQ_DEL = 1;
    SEL_DEL_CONFIRM = 0;
    DEL_N
}

pub(crate) unsafe fn del_pending() -> bool { REQ_DEL != 0 }

pub(crate) unsafe fn do_del_batch() {
    if REQ_DEL == 0 { return; }
    if DEL_CUR < DEL_N {
        let p = (core::ptr::addr_of!(DEL_PKG) as *const u8).add(DEL_CUR * PKG_CAP);
        DEL_CUR += 1;
        let node = find_node(p);
        if node != 0 {
            let id = crate::mem::rd16((node + fw_api::APPN_ID) as *const u16) as u32;
            if id != 0 { fw_api::app_launcher_del(id); }
        }
        rm_app_dirs(p);
        let _ = remove_all_pkg(p, 0);
        let _ = remove_all_pkg(p, 1);
        DEL_OK += 1;
        return;   // 一拍一个
    }
    REQ_DEL = 0;
    let mut w = W::new(core::ptr::addr_of_mut!(APP_MSG) as *mut u8, 96);
    w.s("已卸载 ".as_bytes()).n(DEL_OK as u32).s(" 个\0".as_bytes());
    w.end();
    MSG_KEEP = 1;
    sel_clear();
    crate::log::kv("批量卸载完成 ", DEL_OK as u32);
    reload();
}

static mut D_FMT: [u8; 96] = [0; 96];

/// 详情页第二行: "12.3 MB · v1.2.3"(版本取不到时显示"版本未知")。
pub(crate) unsafe fn detail_fmt() -> *const u8 {
    let d = core::ptr::addr_of_mut!(D_FMT) as *mut u8;
    let mut w = W::new(d, 96);
    w.sz(D_SIZE);
    let vp = core::ptr::addr_of!(D_VER) as *const u8;
    if read_volatile(vp) != 0 {
        w.s(" · v".as_bytes()).s(cstr_slice(vp));
    } else {
        w.s(" · 版本未知".as_bytes());
    }
    w.s("\0".as_bytes());
    w.end();
    d
}
