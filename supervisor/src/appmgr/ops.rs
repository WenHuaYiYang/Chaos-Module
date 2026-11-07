//! 页面事件、单条隐藏/显示与批量操作。

use super::*;

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
pub(crate) unsafe fn find_node(pkg: *const u8) -> u32 {
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
pub(crate) unsafe fn set_node_hidden(node: u32, hide: bool) {
    if node == 0 || crate::mem::safe_ptr(node) == false { return; }
    let p = (node + fw_api::APPN_FLAGS) as *mut u8;
    let v = read_volatile(p);
    let nv = if hide { v | fw_api::APPN_HIDDEN } else { v & !fw_api::APPN_HIDDEN };
    if nv != v { write_volatile(p, nv); }
}

/// 注册表写回成功后的即时生效: 先同步内存隐藏位, 再摘/挂桌面图标。
/// 节点不在册(重启后条目被搬走)时只返回 false, 由文件改动在重启后生效。
pub(crate) unsafe fn apply_pkg(hide: bool, pkg: *const u8) -> bool {
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

pub(crate) unsafe fn apply_immediate(hide: bool) -> bool {
    let pkg = core::ptr::addr_of!(REQ_PKG) as *const u8;
    // 深度隐藏重启后节点缺失: 显示前先按包名从 proxyquickapp 已装表补建节点,
    // 否则 app_launcher_add 找不到记录。补建失败时把应用表数量写进日志，
    // 便于区分“表没起来”和“表里没有这个包”。
    if !hide && find_node(pkg) == 0 {
        let n = fw_api::quickapp_app_count();
        crate::log::kv("显示应用-应用表 ", if n > 0 { n as u32 } else { 0 });
        let ok = fw_api::quickapp_register_pkg(pkg);
        crate::log::kv("显示应用-补建节点 ", if ok { 1 } else { 0 });
        if find_node(pkg) == 0 {
            crate::log::line("显示应用-补建后仍无节点");
        }
    }
    apply_pkg(hide, pkg)
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
    sort_view_visible_first();
}

/// 一键批量: 把当前批的方向对应的全部条目拷进队列(快照), tick 里逐个执行。
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
            // 显示时节点可能缺失(深度隐藏重启过): 先按包名补建再摘/加图标。
            if !hide && find_node(bp) == 0 { let _ = fw_api::quickapp_register_pkg(bp); }
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
    sort_view_visible_first();
}

pub(crate) unsafe fn cstr_slice(p: *const u8) -> &'static [u8] {
    core::slice::from_raw_parts(p, cstr_len(p) + 1)
}

pub(crate) unsafe fn wmsg(s: &str) {
    put_str(core::ptr::addr_of_mut!(APP_MSG) as *mut u8, s.as_bytes());
}

/// 写结果提示, 并让紧随其后的 reload() 不要覆盖它。
pub(crate) unsafe fn wmsg_keep(s: &str) {
    wmsg(s);
    MSG_KEEP = 1;
}

/// 记下失败现场并返回"坏"(2)。界面会把位置、该字节与文件长度报出来。
pub(crate) unsafe fn bad(p: usize) -> u8 {
    PARSE_BAD = 1;
    PARSE_AT = p as u32;
    PARSE_LEN = REG_N as u32;
    PARSE_C = if p < REG_N { read_volatile(REG_BUF.as_ptr().add(p)) } else { 0 };
    2
}

pub(crate) unsafe fn put_u32(dst: *mut u8, mut v: u32) -> usize {
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

pub(crate) unsafe fn put_hex2(dst: *mut u8, v: u8) -> usize {
    const H: &[u8; 16] = b"0123456789abcdef";
    write_volatile(dst, H[(v >> 4) as usize]);
    write_volatile(dst.add(1), H[(v & 15) as usize]);
    2
}

pub(crate) unsafe fn put_lit(dst: *mut u8, s: &[u8]) -> usize {
    let mut i = 0usize;
    while i < s.len() { write_volatile(dst.add(i), s[i]); i += 1; }
    s.len()
}
