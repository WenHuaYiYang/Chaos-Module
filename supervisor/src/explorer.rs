// 文件管理器(目录浏览 + 文件查看)

use crate::*;
use crate::mem::{rd8};

#[inline(always)]
pub(crate) unsafe fn file_read(fd: i32, buf: *mut u8, len: u32) -> i32 {
    let f: unsafe extern "C" fn(i32, *mut u8, u32) -> i32 = core::mem::transmute(FW_FILE_READ as usize);
    f(fd, buf, len)
}

// 路径拼接: dst = base + '/'(base 非/结尾时) + name, 158 截断 + \0
pub(crate) unsafe fn path_join(dst: *mut u8, base: *const u8, name: *const u8) {
    let mut o = 0usize;
    while o < 150 { let c = read_volatile(base.add(o)); if c == 0 { break; } write_volatile(dst.add(o), c); o += 1; }
    if o > 0 && read_volatile(dst.add(o - 1)) != b'/' && o < 150 { write_volatile(dst.add(o), b'/'); o += 1; }
    let mut k = 0usize;
    while o < 158 { let c = read_volatile(name.add(k)); if c == 0 { break; } write_volatile(dst.add(o), c); o += 1; k += 1; }
    write_volatile(dst.add(o), 0u8);
}

// 取路径最后一段(忽略结尾的 '/'): "/data/app/foo" -> "foo"; 根/空 -> 返回 0
pub(crate) unsafe fn path_basename(src: *const u8, dst: *mut u8, cap: usize) -> usize {
    let mut n = 0usize;
    while n + 1 < cap { let c = read_volatile(src.add(n)); if c == 0 { break; } n += 1; }
    let mut e = n;
    while e > 0 && read_volatile(src.add(e - 1)) == b'/' { e -= 1; }
    let mut s = e;
    while s > 0 && read_volatile(src.add(s - 1)) != b'/' { s -= 1; }
    let mut o = 0usize;
    let mut k = s;
    while k < e && o + 1 < cap { write_volatile(dst.add(o), read_volatile(src.add(k))); o += 1; k += 1; }
    write_volatile(dst.add(o), 0u8);
    o
}

// 查看页读文件: 读 VIEW_PATH 的第 VIEW_BLK 块; 跳块靠顺序读丢弃, 每轮 512B 直到 2047 上限
pub(crate) unsafe fn file_read_view() -> i32 {
    let path = core::ptr::addr_of!(VIEW_PATH) as *const u8;
    let fd = file_open_ro(path);            // 内核层 O_RDONLY=1, 不是 POSIX 的 0
    if fd < 0 {
        st_wr!(FILE_VIEW_N, fd);
        return -1;
    }
    let v = core::ptr::addr_of_mut!(FILE_VIEW) as *mut u8;
    let mut skip = st_rd!(VIEW_BLK) * 2047;
    while skip > 0 {
        let chunk = if skip > 512 { 512u32 } else { skip };
        let got = file_read(fd, v, chunk);
        if got <= 0 {
            file_close(fd);
            st_wr!(FILE_VIEW_N, 0);  // EOF → refresh 层回卷块0
            return 0;
        }
        skip -= got as u32;
    }
    let mut used = 0i32;
    let mut got = 0i32;
    while used < 2047 {
        got = file_read(fd, v.add(used as usize), 512);
        if got <= 0 { break; }
        used += got;
    }
    file_close(fd);
    if used == 0 {
        st_wr!(FILE_VIEW_N, got);
        return if got < 0 { -1 } else { 0 };
    }
    write_volatile(v.add(used as usize), 0);
    st_wr!(FILE_VIEW_N, used);
    0
}

// 目录遍历(实测): opendir → readdir 循环(过滤 ./..) → closedir; 目录排前插入排序
pub(crate) unsafe fn load_dir() {
    st_wr!(DIR_COUNT, 0);
    st_wr!(DIR_ERR, 0);
    let depth = st_rd!(DIR_DEPTH) as usize;
    if depth > 7 { return; }
    let p = (core::ptr::addr_of!(PATH_STACK) as *const u8).add(depth * 160);
    let od: unsafe extern "C" fn(*const u8) -> u32 = core::mem::transmute(FW_OPENDIR as usize);
    let dirp = od(p);
    if dirp == 0 {
        let ep: unsafe extern "C" fn() -> *mut i32 = core::mem::transmute(FW_ERRNO_LOCATION as usize);
        let eptr = ep();
        let a = eptr as u32;
        let e = if a != 0 && (a & 0xE000_0000) == 0x2000_0000 { read_volatile(eptr as *const i32) } else { -1 };
        st_wr!(DIR_ERR, e);
        return;
    }
    let rd: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(FW_READDIR as usize);
    let names = core::ptr::addr_of_mut!(DIR_NAMES) as *mut u8;
    let types = core::ptr::addr_of_mut!(DIR_TYPES) as *mut u8;
    let mut cnt = 0usize;
    let mut iter = 0usize;
    while cnt < 24 && iter < 200 {
        iter += 1;
        let de = rd(dirp);
        if de == 0 { break; }
        let dt = rd8(de as *const u8);
        let nm = (de + 1) as *const u8;
        let n1 = read_volatile(nm);
        let n2 = read_volatile(nm.add(1));
        let n3 = read_volatile(nm.add(2));
        if n1 == b'.' && (n2 == 0 || (n2 == b'.' && n3 == 0)) { continue; }
        let dst = names.add(cnt * 64);
        let mut k = 0usize;
        while k < 63 {
            let c = read_volatile(nm.add(k));
            if c == 0 { break; }
            write_volatile(dst.add(k), c);
            k += 1;
        }
        write_volatile(dst.add(k), 0u8);
        write_volatile(types.add(cnt), dt);
        cnt += 1;
    }
    let cd: unsafe extern "C" fn(u32) -> i32 = core::mem::transmute(FW_CLOSEDIR as usize);
    cd(dirp);
    let mut i = 1usize;
    while i < cnt {
        let ti = read_volatile(types.add(i));
        let ri: u8 = if ti == DT_DIR { 0 } else { 1 };
        let mut tmp: [u8; 64] = [0; 64];
        let src = names.add(i * 64);
        let mut k = 0usize; while k < 64 { tmp[k] = read_volatile(src.add(k)); k += 1; }
        let mut j = i;
        while j > 0 {
            let tj = read_volatile(types.add(j - 1));
            let rj: u8 = if tj == DT_DIR { 0 } else { 1 };
            if rj <= ri { break; }
            let d = names.add(j * 64); let s = names.add((j - 1) * 64);
            let mut k = 0usize; while k < 64 { write_volatile(d.add(k), read_volatile(s.add(k))); k += 1; }
            write_volatile(types.add(j), tj);
            j -= 1;
        }
        let d = names.add(j * 64);
        let mut k = 0usize; while k < 64 { write_volatile(d.add(k), tmp[k]); k += 1; }
        write_volatile(types.add(j), ti);
        i += 1;
    }
    st_wr!(DIR_COUNT, cnt as u32);
}

// 目录页行文本: 行0-7=窗内条目(目录名后加 '/', 非普通文件加 '*'), 行8="更多",
// 行9="上级"/"返回", 行10="缓存清理"(仅根目录页建行)
pub(crate) unsafe fn refresh_dir_lines() {
    let base = core::ptr::addr_of_mut!(FILE_LINES) as *mut u8;
    let mut i = 0usize; while i < 11 * 88 { write_volatile(base.add(i), 0); i += 1; }
    let cnt = st_rd!(DIR_COUNT) as usize;
    let err = st_rd!(DIR_ERR);
    let wins = if cnt == 0 { 1usize } else { (cnt + 7) / 8 };
    let mut win = st_rd!(DIR_WIN) as usize;
    if win >= wins { win = 0; st_wr!(DIR_WIN, 0); }
    if err != 0 {
        let mut w = W::new(base, 88);
        w.s(b"ERR ").x(err as u32);
        w.end();
    } else if cnt == 0 {
        let mut x = W::new(base, 88);
        x.s("空目录".as_bytes());
        x.end();
    }
    let names = core::ptr::addr_of!(DIR_NAMES) as *const u8;
    let types = core::ptr::addr_of!(DIR_TYPES) as *const u8;
    let mut r = 0usize;
    while r < 8 {
        let e = win * 8 + r;
        let dp = base.add(r * 88);
        if e < cnt {
            let dt = read_volatile(types.add(e));
            let src = names.add(e * 64);
            // 目录靠名字后的 '/' 区分, 不加 [D]/[F] 前缀
            // 设备/管道/套接字等非普通文件标 '*' —— 点进去不读内容(驱动 read 会阻塞)
            let mut x = W::new(dp, 88);
            let mut k = 0usize;
            while k < 63 {
                let c = read_volatile(src.add(k));
                if c == 0 { break; }
                x.c(c);
                k += 1;
            }
            if dt == DT_DIR { x.c(b'/'); } else if dt != DT_REG { x.c(b'*'); }
            x.end();
        } else {
            write_volatile(dp.add(0), 0u8);   // 不用 '-' 占位(该行已隐藏)
        }
        r += 1;
    }
    let mut x = W::new(base.add(8 * 88), 88);
    if (win + 1) * 8 < cnt { x.s("更多".as_bytes()); }
    x.end();
    // 目录页: 返回/上级 在 slot9
    let mut x = W::new(base.add(9 * 88), 88);
    x.s(if st_rd!(DIR_DEPTH) > 0 { "上级".as_bytes() } else { "返回".as_bytes() });
    x.end();
    // slot10 = 缓存清理入口(仅根目录页 pid1 建行, 见 ui.rs rows_layout)
    let mut x = W::new(base.add(10 * 88), 88);
    x.s("缓存清理".as_bytes());
    x.end();
}

// ===== 缓存清理 =====
//
// 清理范围(固件字符串核对): 只清 /data/cache 与 /data/quickapp/cache 两个目录下的
// 普通文件(含一层子目录)。**不碰 /data/fitness/cache** —— 那里的 *.db 是等待同步到
// 手机的健康数据缓存, 删了会丢记录。
// 删除走 fw_api::fs_remove(libc remove, 0x0C1EABE0, 固件里广泛调用的标准入口)。

const CACHE_DIR1: &[u8] = b"/data/cache\0";
const CACHE_DIR2: &[u8] = b"/data/quickapp/cache\0";

// 统计并(可选)删除一个目录里的普通文件。depth=0 时对子目录再走一层。
// 计数: 文件数 files / 删除失败 fail。返回 0=目录打不开(不算失败, 可能不存在)。
unsafe fn cache_walk_dir(dir: *const u8, depth: u32, clean: bool,
                         files: *mut u32, fail: *mut u32) -> u32 {
    let od: unsafe extern "C" fn(*const u8) -> u32 = core::mem::transmute(FW_OPENDIR as usize);
    let dirp = od(dir);
    if dirp == 0 { return 0; }
    let rd: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(FW_READDIR as usize);
    let cd: unsafe extern "C" fn(u32) -> i32 = core::mem::transmute(FW_CLOSEDIR as usize);
    let mut cnt_here = 0u32;
    let mut iter = 0usize;
    let mut sub: [u8; 160] = [0; 160];
    while iter < 200 {
        iter += 1;
        let de = rd(dirp);
        if de == 0 { break; }
        let dt = rd8(de as *const u8);
        let nm = (de + 1) as *const u8;
        let n1 = read_volatile(nm);
        let n2 = read_volatile(nm.add(1));
        let n3 = read_volatile(nm.add(2));
        if n1 == b'.' && (n2 == 0 || (n2 == b'.' && n3 == 0)) { continue; }
        path_join(sub.as_mut_ptr(), dir, nm);
        if dt == DT_REG {
            write_volatile(files, read_volatile(files) + 1);
            cnt_here += 1;
            if clean && fw_api::fs_remove(sub.as_ptr()) < 0 {
                write_volatile(fail, read_volatile(fail) + 1);
            }
        } else if dt == DT_DIR && depth < 1 {
            cnt_here += cache_walk_dir(sub.as_ptr(), depth + 1, clean, files, fail);
        }
    }
    cd(dirp);
    cnt_here
}

// 扫描/清理两条缓存目录。clean=false 只数; clean=true 删除并返回结果计数。
pub(crate) unsafe fn cache_walk(clean: bool, files: *mut u32, fail: *mut u32) {
    write_volatile(files, 0);
    write_volatile(fail, 0);
    cache_walk_dir(CACHE_DIR1.as_ptr(), 0, clean, files, fail);
    cache_walk_dir(CACHE_DIR2.as_ptr(), 0, clean, files, fail);
}

// 把"N 个文件"样式的状态文本写入 CACHE_MSG。
// scan:  "缓存: N 个文件";  done:  "已清理 N 个(失败 M), 释放 X MB"
pub(crate) unsafe fn cache_fmt_msg(mode: u32, n: u32, fail: u32, freed_mb: u32) {
    let mut w = W::new(core::ptr::addr_of_mut!(CACHE_MSG) as *mut u8, 64);
    match mode {
        0 => { w.s("缓存: ".as_bytes()).n(n).s(" 个文件".as_bytes()); }
        1 => { w.s("已清理 ".as_bytes()).n(n).s(" 个(失败 ".as_bytes()).n(fail)
               .s("), 释放 ".as_bytes()).n(freed_mb).s(" MB".as_bytes()); }
        _ => { w.s("无可清理缓存".as_bytes()); }
    }
    w.end();
}

// 页7 label 文本: 首刷读块(VIEW_LOADED) → 文本sanitize / 二进制strings提取 → VIEW_TEXT + Back行文本
pub(crate) unsafe fn refresh_view_text() {
    // 非普通文件一律不 open 不 read: /dev 下大量字符/块设备节点的驱动 read() 会阻塞,
    // 把 UI 线程锁死到看门狗复位(3-4 分钟)。标记 FILE_VIEW_N=-2 让下面的取数逻辑跳过,
    // 直接走"未读取"提示分支。判据来自 readdir 的 dirent.d_type, 不额外做 stat。
    if st_rd!(VIEW_LOADED) == 0
        && st_rd!(VIEW_DT) != DT_REG {
        st_wr!(FILE_VIEW_N, -2);
        st_wr!(VIEW_LOADED, 1);
    }
    if st_rd!(VIEW_LOADED) == 0 {
        file_read_view();
        if st_rd!(FILE_VIEW_N) <= 0
            && st_rd!(VIEW_BLK) > 0 {
            st_wr!(VIEW_BLK, 0);
            file_read_view();
        }
        st_wr!(VIEW_LOADED, 1);
    }
    let vt = core::ptr::addr_of_mut!(VIEW_TEXT) as *mut u8;
    let view = core::ptr::addr_of!(FILE_VIEW) as *const u8;
    let n = st_rd!(FILE_VIEW_N);
    let mut o = 0usize;
    if n == -2 {
        // 设备节点/管道/套接字: 读它会阻塞, 只提示不读。
        // 带上实际 type 值 -- 万一某文件系统报的 d_type 与预期不同, 一眼能看出来
        let mut w = W::new(vt, 2200);
        w.s("非普通文件(type=".as_bytes()).n(st_rd!(VIEW_DT) as u32).c(b')')
         .s(", 不读取".as_bytes()).c(10).s("(设备节点读取会阻塞)".as_bytes());
        o = w.len();
    } else if n < 0 {
        let mut w = W::at(vt, 2200, o);
        w.s(b"ERR n=").x(n as u32);
        o = w.len();
    } else if n == 0 {
        let mut w = W::at(vt, 2200, o);
        w.s("空文件".as_bytes());
        o = w.len();
    } else {
        let nn = n as usize;
        let mut pc = nn; if pc > 256 { pc = 256; }
        let mut bad = 0usize;
        let mut k = 0usize;
        while k < pc {
            let ch = read_volatile(view.add(k));
            if ch < 0x20 && ch != b'\n' && ch != b'\t' && ch != b'\r' { bad += 1; }  // UTF-8(>=0x80)=文本
            k += 1;
        }
        let binary = if bad * 4 > pc { 1u32 } else { 0u32 };
        st_wr!(VIEW_BINARY, binary);
        if binary == 0 {
            let mut k = 0usize;
            let mut col = 0u32;   // 折行(24单位/行)
            while k < nn && o < 2140 {
                let ch = read_volatile(view.add(k));
                let c = if ch == b'\n' { b'\n' } else if ch == b'\t' { b' ' } else if ch < 0x20 { b'.' } else { ch };
                let wu: u32 = if c == b'\n' { 0 } else if ch < 0x80 { 1 } else if ch < 0xE0 { 1 } else if ch < 0xF0 { 2 } else { 0 };
                if c == b'\n' { col = 0; }
                else if wu > 0 && col + wu > 24 { write_volatile(vt.add(o), b'\n'); o += 1; col = 0; }
                write_volatile(vt.add(o), c); o += 1; col += wu; k += 1;
            }
        } else {
            let mut run = 0usize;
            let mut rs = 0usize;
            let mut k = 0usize;
            while k <= nn && o < 2000 {
                let ch = if k < nn { read_volatile(view.add(k)) } else { 0u8 };
                let pr = ch >= 0x20 && ch <= 0x7E;
                if pr { if run == 0 { rs = k; } run += 1; }
                if (!pr || k == nn) && run > 0 {
                    if run >= 4 {
                        let mut c = 0usize;
                        let mut col = 0u32;
                        while c < run && o < 2140 {
                            if col >= 24 { write_volatile(vt.add(o), b'\n'); o += 1; col = 0; }
                            write_volatile(vt.add(o), read_volatile(view.add(rs + c)));
                            o += 1; c += 1; col += 1;   // strings 全 ASCII = 1单位
                        }
                        write_volatile(vt.add(o), b'\n'); o += 1;
                    }
                    run = 0;
                }
                k += 1;
            }
        }
        if o == 0 {
            let mut w = W::at(vt, 2200, o);
            w.s("无字符串".as_bytes());
            o = w.len();
        }
    }
    write_volatile(vt.add(o), 0u8);
    // 返回行文本(查看页第二行, 点击回目录)
    let mut w = W::new(core::ptr::addr_of_mut!(VIEW_BACKTXT) as *mut u8, 88);
    w.s("返回".as_bytes());
    w.end();
}

