//! 批量操作排除名单与可见视图构建。

use super::*;

// ===== 排除名单(不参与一键显示/隐藏) =====
//
// 文件 /data/chaos/appmgr_excl.bin, 定长:
//   magic[4] = "EX01"
//   条目 0..39, 每条 44 B: 包名(43 B, 第 44 字节恒 0)
// 列表按文件顺序排列, 写满即止。读失败/魔数不对 = 空名单(绝不因此禁止应用管理)。
// 排除是 Chaos 自己的名单, 不进 apps.json —— 固件不认识它, 也不该认识。

/// 标记合并列表里的重复包名: 同一包名只保留第一次出现(可见优先, 因为先解析)。
pub(crate) unsafe fn mark_dups() {
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

pub(crate) unsafe fn build_view() {
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

pub(crate) unsafe fn excl_index(pkg: *const u8) -> usize {
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
pub(crate) unsafe fn excl_checksum(b: *const u8, sum_off: usize) -> u32 {
    let mut s = 0u32;
    let mut i = EXCL_HDR;
    while i < sum_off { s = s.wrapping_add(read_volatile(b.add(i)) as u32); i += 1; }
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
    let mut legacy_len = false;
    if new_ok {
        if n < EXCL_HDR + EXCL_SLOT + 4 { return; }
        // 旧版 40 槽文件: 校验和就在实际长度末尾，按实际长度校验后迁移到新容量。
        let sum_off = if n >= EXCL_TOTAL { EXCL_SUM_OFF } else { n - 4 };
        let want = read_volatile(b.add(sum_off) as *const u32);
        if want != excl_checksum(b, sum_off) { return; }   // 损坏 -> 空名单
        legacy_len = sum_off != EXCL_SUM_OFF;
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
    let limit = if is_old { n } else if legacy_len { n - 4 } else { EXCL_SUM_OFF };
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
    if is_old || legacy_len { let _ = excl_save(); }
}

pub(crate) unsafe fn excl_save() -> bool {
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
    let sum = excl_checksum(b, EXCL_SUM_OFF);
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
pub(crate) unsafe fn excl_toggle(pkg: *const u8) -> bool {
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

pub(crate) unsafe fn eset(s: &str) {
    put_str(core::ptr::addr_of_mut!(EXCL_MSG) as *mut u8, s.as_bytes());
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

/// (重)读两份注册表, 重建展示列表。
pub(crate) unsafe fn reload() {
    APP_N = 0;
    TRUNC = 0;
    PARSE_BAD = 0;
    OVERSIZE = 0;
    APP_OVER = 0;
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
    } else if APP_OVER != 0 {
        let d = core::ptr::addr_of_mut!(APP_MSG) as *mut u8;
        let mut n = put_lit(d, "应用过多, 只读(上限 ".as_bytes());
        n += put_u32(d.add(n), APP_MAX as u32);
        n += put_lit(d.add(n), b")");
        write_volatile(d.add(n), 0);
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
