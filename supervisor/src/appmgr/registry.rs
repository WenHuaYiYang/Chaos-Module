//! `apps.json` / `apps.json_hide` 的载入、扫描与条目元数据解析。

use super::*;

// ===== 载入与解析 =====

/// 把 path 读进 REG_BUF。false = 文件不存在(合法: 隐藏注册表可选)。
pub(crate) unsafe fn load_file(path: *const u8) -> bool {
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
pub(crate) unsafe fn scan_object(p: &mut usize, src: u8) -> u8 {
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
pub(crate) unsafe fn parse_registry(src: u8) -> u8 {
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
