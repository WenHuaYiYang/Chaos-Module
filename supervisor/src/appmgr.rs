//! 应用管理服务：共享状态、持久化常量与对外 API。
//!
//! 具体逻辑按职责拆在 `appmgr/` 子模块；外部仍通过 `crate::appmgr::*` 使用。
//
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

mod ops;
mod json;
mod registry;
mod excl;
mod mode;
mod rewrite;
mod detail;

pub(crate) use ops::*;
pub(crate) use json::*;
pub(crate) use registry::*;
pub(crate) use excl::*;
pub(crate) use mode::*;
pub(crate) use rewrite::*;
pub(crate) use detail::*;


pub(crate) const VIS_PATH: &[u8] = b"/data/apps.json\0";
pub(crate) const HID_PATH: &[u8] = b"/data/apps.json_hide\0";

pub(crate) const REG_CAP: usize = 64 * 1024; // 单个注册表文件字节上限；64 KB 覆盖约 64 个常见应用对象
pub(crate) const APP_MAX: usize = 64;    // 条目上限(两份注册表合计)
const OBJ_CAP: usize = 1536;             // 单个被搬运对象的字节上限
const PKG_CAP: usize = 44;               // 包名槽宽(E_PKG / BATCH_PKG / EXCL_SLOT 同宽)

// ===== 批量操作排除名单 =====
// 独立于两份注册表: 影响"一键隐藏/显示全部", 且排除项在页1 列表里不显示;
// 格式: magic "EX02" + APP_MAX 槽 × 44 字节包名 + 4 字节校验和; 槽首字节 0 = 空槽, 读到空槽即止。
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
const KEEP_PATH: &[u8] = b"/data/chaos/appmgr_keep.bin\0";
const KEEP_MAGIC: [u8; 4] = *b"KH01";
const KEEP_TOTAL: usize = 4 + APP_MAX * PKG_CAP;
pub(crate) const APPLY_CFG: &[u8] = b"/data/chaos/appmgr_apply.cfg\0";
static mut APPLY_LABEL: [u8; 48] = [0; 48];
static mut MIG_BUF: [[u8; PKG_CAP]; APP_MAX] = [[0; PKG_CAP]; APP_MAX];
static mut MIG_N: usize = 0;
static mut REQ_MODE: u32 = 0;
static mut MODE_BEFORE: u32 = 0; // 模式迁移失败时恢复用
static mut APPLY_MODE: u32 = 0;
static mut APPLY_LOADED: u32 = 0;
static mut KEEP_BUF: [u8; KEEP_TOTAL] = [0; KEEP_TOTAL];
static mut KEEP_N: usize = 0;
// 切换隐藏方式时的包名快照: 文件会被逐条重写, 不能边遍历 E_PKG 边搬。
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

// ===== 解析工作区 =====
static mut REG_BUF: [u8; REG_CAP] = [0; REG_CAP];
static mut REG_N: usize = 0;         // 本次载入字节数
static mut HAVE_FILE: u32 = 0;       // 0=文件不存在(允许合成最小结构)
static mut OVERSIZE: u32 = 0;        // 1=文件超过 REG_CAP
static mut TRUNC: u32 = 0;           // 1=存在无法管理的对象(拒绝改写)
static mut APP_OVER: u32 = 0;        // 1=应用条目数达到 APP_MAX
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
