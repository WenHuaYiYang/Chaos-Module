//! 注册表原位改写、隐藏/显示搬运与去重。

use super::*;

// ===== 改写 =====

/// 把当前 REG_BUF(已 parse) 重组进自身前部并返回新长度。
/// skip: 跳过的对象跨度; append: 0=不追加 1=追加原样(摘 hideFlag) 2=追加并写 hideFlag:true。
/// 不变量: 写指针永远 <= 读指针(只删不增, 追加字节来自 OBJ_BUF), 原位覆盖安全。
pub(crate) unsafe fn serialize(skip_off: usize, skip_len: usize, append: u8) -> usize {
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
pub(crate) unsafe fn write_obj(w: &mut usize, need_flag: bool) -> bool {
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
pub(crate) unsafe fn write_file(path: *const u8, n: usize) -> bool {
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
pub(crate) unsafe fn capture(pkg: *const u8, src: u8) -> bool {
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

pub(crate) unsafe fn cstr_eq(a: *const u8, b: *const u8) -> bool {
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
pub(crate) unsafe fn file_has_pkg(pkg: *const u8) -> bool {
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
pub(crate) unsafe fn remove_all_pkg(pkg: *const u8, src: u8) -> bool {
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
pub(crate) unsafe fn move_vis_to_hide(pkg: *const u8) -> bool {
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
pub(crate) unsafe fn move_hide_to_vis(pkg: *const u8) -> bool {
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
pub(crate) unsafe fn op_hide() -> bool { op_hide_pkg(core::ptr::addr_of!(REQ_PKG) as *const u8) }

pub(crate) unsafe fn op_hide_pkg(pkg: *const u8) -> bool {
    if write_blocked() { return false; }
    // 普通隐藏: 条目留在 apps.json, 只记入 Chaos 隐藏名单; 即时生效由 apply_pkg 完成。
    if apply_mode_is_keep() {
        return keep_add(pkg);
    }
    move_vis_to_hide(pkg)
}

/// 显示: 对象 隐藏 -> 可见(摘 hideFlag)。先写可见注册表, 再从隐藏注册表删除。
pub(crate) unsafe fn op_show() -> bool { op_show_pkg(core::ptr::addr_of!(REQ_PKG) as *const u8) }

pub(crate) unsafe fn op_show_pkg(pkg: *const u8) -> bool {
    if write_blocked() { return false; }
    // 普通隐藏: 条目本来就在 apps.json, 只需从 Chaos 隐藏名单里摘掉。
    if apply_mode_is_keep() && keep_has(pkg) {
        return keep_del(pkg);
    }
    move_hide_to_vis(pkg)
}

/// 旧版本可能留下重复包名: 读到内存后按包名收敛，只保留第一份。
pub(crate) unsafe fn keep_dedupe() {
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

pub(crate) unsafe fn excl_dedupe() {
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
