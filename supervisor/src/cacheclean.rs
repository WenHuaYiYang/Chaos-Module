// 缓存清理(Shell++ shellpp-ii 核心逻辑移植: 遍历根目录 -> 计文件数/体积 -> 二次确认后删除)
//
// 清理范围(只删这些目录**里面**的内容, 根目录本身保留):
//   /data/cache           系统缓存(Chaos 原有清理项, 真机验证过)
//   /data/quickapp/cache  快应用缓存(同上)
//   /data/log             系统日志(shellpp 043 同固件验证: 恒可清)
//   /data/offlinelog      离线日志(同上)
// 红线: 继续不碰 /data/fitness/cache —— 那里是等待同步到手机的健康数据。
//
// 删除规则(与 shellpp walk_directory 一致): 普通文件 unlink; 子目录递归清空后
// rmdir; 链接/设备节点等未知类型一律跳过, 不打开不跟随。深度上限 8。
// 体积 = 每个普通文件 open+lseek(END)+close 取大小(lseek 已验证), 饱和累加。
// 长操作在 tick 里执行(不在事件回调做文件 IO)。

use crate::*;
use crate::mem::rd8;
use core::ptr::{read_volatile, write_volatile};

static mut CC_MSG: [u8; 96] = [0; 96];
pub(crate) static mut CC_ARM: u32 = 0;   // 二次确认: 1=已武装, 再点才执行
pub(crate) static mut CC_REQ: u32 = 0;   // 1=请求执行(tick 消费)
pub(crate) static mut CC_HAS_RESULT: u32 = 0; // 1=CC_MSG 是刚出的清理结果(渲染消费一次)

static mut CC_WORK: [u8; 176] = [0; 176];
static mut CC_FILES: u32 = 0;
static mut CC_FAIL: u32 = 0;
static mut CC_BYTES: u32 = 0;

// 按根目录的统计(卡片展示用): 文件数 / 字节数 / 格式化文案
static mut CC_ROOT_FILES: [u32; 4] = [0; 4];
static mut CC_ROOT_BYTES: [u32; 4] = [0; 4];
static mut CC_ROOT_MSG: [[u8; 48]; 4] = [[0; 48]; 4];

const ROOTS: [&[u8]; 4] = [
    b"/data/cache\0",
    b"/data/quickapp/cache\0",
    b"/data/log\0",
    b"/data/offlinelog\0",
];
const DEPTH_MAX: u32 = 8;

unsafe fn cstr_len(s: *const u8) -> usize {
    let mut n = 0usize;
    while read_volatile(s.add(n)) != 0 { n += 1; }
    n
}

pub(crate) unsafe fn msg() -> &'static [u8] {
    let n = cstr_len(core::ptr::addr_of!(CC_MSG) as *const u8);
    core::slice::from_raw_parts(core::ptr::addr_of!(CC_MSG) as *const u8, n + 1)
}

unsafe fn add_sat(v: &mut u32, amount: u32) {
    if 0xFFFF_FFFF - *v < amount { *v = 0xFFFF_FFFF; } else { *v += amount; }
}

/// 文件大小: open + lseek(END) + close(不 read 内容, 不碰非普通文件)。
unsafe fn file_size(path: *const u8) -> u32 {
    let fd = fw_api::open(path, fw_api::oflag::RDONLY, 0);
    if fd < 0 { return 0; }
    let sz = fw_api::lseek(fd, 0, fw_api::seek::END);
    fw_api::close(fd);
    if sz < 0 { 0 } else { sz as u32 }
}

/// 递归走 CC_WORK[0..plen] 这个目录。clean=false 只统计; true 删除文件 + 清空的子目录。
unsafe fn walk(plen: usize, depth: u32, clean: bool) {
    let od: unsafe extern "C" fn(*const u8) -> u32 = core::mem::transmute(FW_OPENDIR as usize);
    let rd: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(FW_READDIR as usize);
    let cd: unsafe extern "C" fn(u32) -> i32 = core::mem::transmute(FW_CLOSEDIR as usize);
    let dirp = od(CC_WORK.as_ptr());
    if dirp == 0 { return; }
    let mut iter = 0usize;
    loop {
        if iter >= 400 { break; }   // 单目录条目上限(防异常文件系统死循环)
        iter += 1;
        let de = rd(dirp);
        if de == 0 { break; }
        let dt = rd8(de as *const u8);
        let nm = (de + 1) as *const u8;
        let n1 = read_volatile(nm);
        let n2 = read_volatile(nm.add(1));
        let n3 = read_volatile(nm.add(2));
        if n1 == b'.' && (n2 == 0 || (n2 == b'.' && n3 == 0)) { continue; }
        // 拼子路径: CC_WORK[0..plen] + '/' + name
        let mut o = plen;
        if o > 0 && read_volatile(CC_WORK.as_ptr().add(o - 1)) != b'/' {
            if o + 1 >= CC_WORK.len() { continue; }
            write_volatile(CC_WORK.as_mut_ptr().add(o), b'/'); o += 1;
        }
        let mut k = 0usize;
        while o + 1 < CC_WORK.len() {
            let c = read_volatile(nm.add(k));
            if c == 0 { break; }
            write_volatile(CC_WORK.as_mut_ptr().add(o), c);
            o += 1; k += 1;
        }
        write_volatile(CC_WORK.as_mut_ptr().add(o), 0);
        if dt == DT_REG {
            add_sat(&mut CC_BYTES, file_size(CC_WORK.as_ptr()));
            CC_FILES += 1;
            if clean && fw_api::fs_remove(CC_WORK.as_ptr()) < 0 { CC_FAIL += 1; }
        } else if dt == DT_DIR {
            if depth < DEPTH_MAX { walk(o, depth + 1, clean); }
            // 清空后的空目录一并删除(根目录本身不在删除范围)
            if clean && fw_api::fs_remove(CC_WORK.as_ptr()) < 0 { CC_FAIL += 1; }
        }
        // 其它类型(链接/设备/管道): 跳过, 不打开不跟随
        // 截回前缀
        write_volatile(CC_WORK.as_mut_ptr().add(plen), 0);
    }
    cd(dirp);
}

/// 遍历全部根目录。clean=false 统计, true 删除。计数留在 CC_FILES/CC_FAIL/CC_BYTES,
/// 每个根目录各自的文件数/字节留在 CC_ROOT_FILES/CC_ROOT_BYTES(卡片展示用)。
unsafe fn walk_roots(clean: bool) {
    CC_FILES = 0;
    CC_FAIL = 0;
    CC_BYTES = 0;
    let mut r = 0usize;
    while r < ROOTS.len() {
        let root = ROOTS[r];
        let mut i = 0usize;
        while i < root.len() { write_volatile(CC_WORK.as_mut_ptr().add(i), root[i]); i += 1; }
        let f0 = CC_FILES;
        let b0 = CC_BYTES;
        walk(root.len() - 1, 0, clean);   // plen 不含结尾 \0
        CC_ROOT_FILES[r] = CC_FILES - f0;
        CC_ROOT_BYTES[r] = CC_BYTES - b0;
        r += 1;
    }
}

/// 把每个根目录的占用写进 CC_ROOT_MSG(卡片副标签): "N 个文件 / X" 或 "空"(X 自动用 B/KB/MB/GB)。
unsafe fn scan_roots() {
    let mut r = 0usize;
    while r < ROOTS.len() {
        let mut w = W::new((core::ptr::addr_of_mut!(CC_ROOT_MSG) as *mut u8).add(r * 48), 48);
        if CC_ROOT_FILES[r] == 0 {
            w.s("空\0".as_bytes());
        } else {
            w.n(CC_ROOT_FILES[r]).s(" 个文件 / ".as_bytes())
             .sz(CC_ROOT_BYTES[r]).s("\0".as_bytes());
        }
        w.end();
        r += 1;
    }
}

/// 卡片副标签(带结尾 \0 的切片): 根目录 r 的占用文案
pub(crate) unsafe fn root_usage(r: usize) -> &'static [u8] {
    if r >= ROOTS.len() { return &[]; }
    let p = (core::ptr::addr_of!(CC_ROOT_MSG) as *const u8).add(r * 48);
    core::slice::from_raw_parts(p, cstr_len(p) + 1)
}

/// 根目录路径文本(卡片主标签, ROOTS 本就是 \0 结尾的静态串)
pub(crate) unsafe fn root_path(r: usize) -> &'static [u8] {
    if r >= ROOTS.len() { return &[]; }
    ROOTS[r]
}

pub(crate) const ROOT_N: usize = 4;

/// 进页/刷新: 只读扫描, 文案写 CC_MSG, 卡片占用写 CC_ROOT_MSG。
pub(crate) unsafe fn scan_msg() {
    walk_roots(false);
    scan_roots();
    let mut w = W::new(core::ptr::addr_of_mut!(CC_MSG) as *mut u8, 96);
    if CC_FILES == 0 {
        w.s("无可清理缓存\0".as_bytes());
    } else {
        w.s("缓存: ".as_bytes()).n(CC_FILES).s(" 个文件 / ".as_bytes())
         .sz(CC_BYTES);
    }
    w.end();
}

/// tick: 执行清理(此前必须已走二次确认), 结果写 CC_MSG。
pub(crate) unsafe fn clean() {
    walk_roots(true);
    // 先用"本次已删量"出结果文案(下面的重扫会覆盖计数器, 必须先取)
    let mut w = W::new(core::ptr::addr_of_mut!(CC_MSG) as *mut u8, 96);
    w.s("已清理 ".as_bytes()).n(CC_FILES).s(" 个(失败 ".as_bytes())
     .n(CC_FAIL).s("), 释放 ".as_bytes()).sz(CC_BYTES);
    w.end();
    // 只读重扫一遍剩余量: walk_roots(true) 期间 CC_ROOT_* 记的是"已删计数",
    // 不重扫的话卡片显示的占用和清理前一模一样(2026-10-15 用户报告)。
    walk_roots(false);
    scan_roots();
    CC_ARM = 0;
}
