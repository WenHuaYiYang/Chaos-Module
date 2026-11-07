//! 应用详情、卸载目录树与批量选择/删除。

use super::*;

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

pub(crate) unsafe fn mk_path3(root: &[u8], mid: &[u8], pkg: *const u8, suffix: &[u8]) -> usize {
    let buf = core::ptr::addr_of_mut!(D_WORK) as *mut u8;
    let cap = 300usize;
    let mut o = 0usize;
    let mut i = 0usize;
    // 整段拷贝 root(包括结尾的 /), 之前少拷最后一个字节, 拼出来是 "/data/appcom.x"
    while i < root.len() && o + 1 < cap { write_volatile(buf.add(o), root[i]); o += 1; i += 1; }
    i = 0;
    while i < mid.len() && o + 1 < cap { write_volatile(buf.add(o), mid[i]); o += 1; i += 1; }
    if o > 0 && read_volatile(buf.add(o - 1)) != b'/' && o + 1 < cap {
        write_volatile(buf.add(o), b'/'); o += 1;
    }
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

pub(crate) unsafe fn path_exists(path: *const u8) -> bool {
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

pub(crate) unsafe fn cpy_n(dst: *mut u8, src: *const u8, cap: usize) {
    let mut k = 0usize;
    while k < cap {
        let c = read_volatile(src.add(k));
        write_volatile(dst.add(k), c);
        if c == 0 { return; }
        k += 1;
    }
    write_volatile(dst.add(cap - 1), 0);
}

pub(crate) unsafe fn file_bytes(path: *const u8) -> u32 {
    let fd = fw_api::open(path, fw_api::oflag::RDONLY, 0);
    if fd < 0 { return 0; }
    let sz = fw_api::lseek(fd, 0, fw_api::seek::END);
    fw_api::close(fd);
    if sz < 0 { 0 } else { sz as u32 }
}

pub(crate) unsafe fn calc_walk(plen: usize, depth: u32) {
    let dp = fw_api::opendir(core::ptr::addr_of!(D_WORK) as *const u8);
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

pub(crate) unsafe fn del_walk(plen: usize, depth: u32) {
    let dp = fw_api::opendir(core::ptr::addr_of!(D_WORK) as *const u8);
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

pub(crate) unsafe fn scan_size(pkg: *const u8) {
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

pub(crate) unsafe fn rm_app_dirs(pkg: *const u8) {
    let mut i = 0usize;
    while i < APP_DIRS.len() {
        let plen = mk_path3(APP_DIRS[i], b"", pkg, b"");
        del_walk(plen, 0);
        let _ = fw_api::fs_remove(core::ptr::addr_of!(D_WORK) as *const u8);
        i += 1;
    }
    let mut j = 0usize;
    while j < SUB_DIRS.len() {
        let plen = mk_path3(SUB_DIRS[j], b"", pkg, b"");
        del_walk(plen, 0);
        let _ = fw_api::fs_remove(core::ptr::addr_of!(D_WORK) as *const u8);
        j += 1;
    }
    let olds: [&[u8]; 3] = [b"/data/app/", b"/data/quickapp/system/", b"/data/app/system/"];
    let mut k = 0usize;
    while k < olds.len() {
        let plen = mk_path3(olds[k], b"", pkg, b".old");
        del_walk(plen, 0);
        let _ = fw_api::fs_remove(core::ptr::addr_of!(D_WORK) as *const u8);
        k += 1;
    }
}

/// 从 rpk_info.json 里取该应用的 versionName(取不到就空)。
pub(crate) unsafe fn scan_version(pkg: *const u8) {
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

/// 详情页「打开应用」: 按包名调 `quickapp_launch_app(pkg, NULL)`。
/// 深度隐藏只影响 launcher 的注册表/图标, 应用本身仍装着, 已装表里能查到。
/// 返回 true = 已发起启动。
pub(crate) unsafe fn detail_launch() -> bool {
    if !detail_valid() { return false; }
    let pkg = core::ptr::addr_of!(D_PKG) as *const u8;
    // 表没初始化（真机上 0x20111B90 常为 NULL）时先拉起 proxyquickapp 一次。
    let mut n = fw_api::quickapp_app_count();
    if n <= 0 {
        let booted = fw_api::quickapp_proxy_init();
        n = fw_api::quickapp_app_count();
        crate::log::kv("打开应用-拉起proxy ", if booted { 1 } else { 0 });
    }
    crate::log::kv("打开应用-应用表数量 ", if n > 0 { n as u32 } else { 0 });
    // 深度隐藏重启后注册表没有节点: 先按包名补建(固件页面管理器按节点找应用),
    // 补完若原本是隐藏态, 再把图标摘掉, 保持隐藏。
    if find_node(pkg) == 0 {
        let ok = fw_api::quickapp_register_pkg(pkg);
        crate::log::kv("打开应用-补建节点 ", if ok { 1 } else { 0 });
        if ok && D_HIDDEN != 0 { let _ = apply_pkg(true, pkg); }
    }
    let rc = fw_api::quickapp_launch_app(pkg);
    if rc < 0 {
        wmsg_keep("启动失败: 应用表未就绪或未安装\0");
        return false;
    }
    true
}

/// 真正卸载: 摘图标 → 删六个目录 → 从两份注册表摘条目 → reload。
pub(crate) unsafe fn uninstall_pkg(pkg: *const u8, name: *const u8) -> bool {
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
pub(crate) unsafe fn excluded(i: usize) -> bool {
    i < APP_MAX && read_volatile((core::ptr::addr_of!(E_EXCL) as *const u8).add(i)) != 0
}

/// 批量页只算未被排除的应用
pub(crate) unsafe fn batch_total() -> usize {
    let n = view_count();
    let mut v = 0usize;
    let mut c = 0usize;
    while v < n {
        let i = view_at(v);
        if i != usize::MAX && !excluded(i) { c += 1; }
        v += 1;
    }
    c
}

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
    // 顺便把快应用代理的“已装应用表”数量显示出来: 0 表示 proxyquickapp 没起来,
    // 补节点/打开都会失败。这样不用看日志也能判断。
    let n = fw_api::quickapp_app_count();
    w.s(" · 表".as_bytes());
    w.n(if n > 0 { n as u32 } else { 0 });
    w.s("\0".as_bytes());
    w.end();
    d
}
