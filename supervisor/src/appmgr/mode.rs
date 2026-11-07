//! 隐藏模式配置、keep 名单、开机补偿与运行期看门狗。

use super::*;

// ===== 生效模式: 配置/隐藏名单/开机补偿 =====

pub(crate) unsafe fn cfg_mode_read() -> u32 {
    let fd = fw_api::open(APPLY_CFG.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 { return 2; }
    let mut b = [0u8; 2];
    let n = fw_api::read(fd, b.as_mut_ptr(), 2);
    fw_api::close(fd);
    // 新版: "2" + ("0"=深度 / "1"=普通)。旧版单字节 "0"/"1" 仍兼容，避免升级后模式漂移。
    if n == 2 && b[0] == b'2' { return if b[1] == b'1' { 1 } else { 0 }; }
    if n >= 1 && (b[0] == b'0' || b[0] == b'1') { return if b[0] == b'1' { 1 } else { 0 }; }
    2

}

pub(crate) unsafe fn cfg_mode_save() -> bool {
    let fd = fw_api::open(APPLY_CFG.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd < 0 { return false; }
    let b = [b'2', if APPLY_MODE == 0 { b'0' } else { b'1' }];
    let n = fw_api::write(fd, b.as_ptr(), 2);
    fw_api::close(fd);
    n == 2
}

/// 读隐藏名单(固定 4+40*44 字节)。文件缺失/坏 magic 视为空名单。
pub(crate) unsafe fn keep_load() {
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

pub(crate) unsafe fn keep_save() -> bool {
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

pub(crate) unsafe fn keep_find(pkg: *const u8) -> usize {
    let mut i = 0usize;
    while i < KEEP_N {
        let p = KEEP_BUF.as_ptr().add(4 + i * PKG_CAP);
        if cstr_eq(p, pkg) { return i; }
        i += 1;
    }
    usize::MAX
}

pub(crate) unsafe fn keep_has(pkg: *const u8) -> bool { keep_find(pkg) != usize::MAX }

pub(crate) unsafe fn keep_add(pkg: *const u8) -> bool {
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

pub(crate) unsafe fn keep_del(pkg: *const u8) -> bool {
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
pub(crate) unsafe fn keep_apply_all() -> usize {
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
    // 读取持久化模式: 0 = 深度隐藏, 1 = 普通隐藏。
    // 配置缺失/损坏/旧格式时保持深度隐藏默认值，避免升级后隐藏状态意外变化。
    APPLY_MODE = if cfg_mode_read() == 1 { 1 } else { 0 };
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
    let old = APPLY_MODE;
    MODE_BEFORE = old;
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

/// 迁移失败时恢复到切换前的模式，并让下一次开机也看到旧模式。
unsafe fn restore_mode_before_failure() {
    APPLY_MODE = MODE_BEFORE;
    let _ = cfg_mode_save();
}

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
                restore_mode_before_failure();
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
        // 先按深度模式读两份注册表，避免 reload() 把"可见且命中 keep"的条目误标成隐藏。
        let target = APPLY_MODE;
        APPLY_MODE = 0;
        reload();
        APPLY_MODE = target;
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
                restore_mode_before_failure();
                reload();
                wmsg_keep("切换失败, 已回滚; 请重试\0");
                return;
            }
            if !keep_add(p) {
                // 当前条目已经从 hide 搬回 vis，先搬回去，再回滚之前成功的条目。
                if move_vis_to_hide(p) { apply_pkg(true, p); }
                let mut j = 0usize;
                while j < moved {
                    let q = (core::ptr::addr_of!(MIG_BUF) as *const u8).add(j * PKG_CAP);
                    if move_vis_to_hide(q) { apply_pkg(true, q); }
                    j += 1;
                }
                KEEP_N = keep_before;
                let _ = keep_save();
                restore_mode_before_failure();
                reload();
                wmsg_keep("切换失败, 已回滚; 请重试\0");
                return;
            }
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
pub(crate) unsafe fn deep_apply_all() -> (usize, usize, usize) {
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

    // 普通隐藏: 条目一直在 apps.json，节点由固件开机注册；只需把 keep 名单
    // 重新置位并摘图标，不搬文件、不补节点，也不会要求重启。
    if apply_mode_is_keep() {
        let found = keep_apply_all();
        let n = KEEP_N;
        let d = core::ptr::addr_of_mut!(APP_MSG) as *mut u8;
        let mut w = put_lit(d, "已重新应用隐藏 ".as_bytes());
        w += put_u32(d.add(w), found as u32);
        w += put_lit(d.add(w), "/".as_bytes());
        w += put_u32(d.add(w), n as u32);
        write_volatile(d.add(w), 0);
        MSG_KEEP = 1;
        crate::log::kv("重新应用隐藏, 命中 ", found as u32);
        return;
    }

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
    BOOT_WAIT = BOOT_WAIT.wrapping_add(1);
    // 注册表/应用表可能比模块晚就绪: 每 40 拍试一次, 最多 5 分钟。
    if BOOT_WAIT % 40 != 0 { return; }
    BOOT_TRIES = BOOT_TRIES.wrapping_add(1);
    if apply_mode_is_keep() {
        // 普通隐藏: 条目在 apps.json，等固件把节点建出来后逐项置位、摘图标。
        // 升级迁移后的第一次开机，固件已经读完 apps.json；这里按固件的
        // “重同步”路径补缺节点，让旧深度隐藏的应用也能立即显示。
        // proxyquickapp 的已装表可能比模块晚就绪，所以每 5 次补偿重试一次，
        // 而不是只在第 5 次试一次。
        if BOOT_TRIES % 5 == 0 {
            let (total, rebuilt) = fw_api::quickapp_resync_missing();
            if rebuilt > 0 {
                crate::log::kv("开机补偿: 补建节点, 应用表 ", total);
                crate::log::kv("开机补偿: 补建节点, 本次 ", rebuilt);
            }
        }
        let found = keep_apply_all();
        let n = KEEP_N;
        if n == 0 || found >= n || BOOT_TRIES >= 300 {
            BOOT_APPLIED = 1;
            crate::log::kv("开机补偿: 隐藏条目 ", n as u32);
            crate::log::kv("开机补偿: 命中 ", found as u32);
        }
        return;
    }
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
pub(crate) unsafe fn reg_sig_rt() -> u32 {
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
    // 开关请求不能只依赖应用管理页的 tick：用户拨完开关立刻离开页面时，
    // 维护定时器仍要把迁移做完，避免"配置已改、文件没搬"的跨重启不一致。
    if REQ_MODE != 0 { do_mode_apply(); }
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
        if apply_mode_is_keep() {
            let _ = keep_apply_all();
        } else {
            let _ = fw_api::quickapp_resync_missing();
            let _ = deep_apply_all();
        }
    } else if WATCH_WAIT % 240 == 0 {
        // 兜底: 集合没变但节点被重建/bit0 被清, 每分钟(慢拍 4 分钟)整表重放一次。
        // Periodic fallback is intentionally silent: it runs every minute and
        // used to add identical lines forever. Only actual registry changes
        // (the branch above) are worth a log line.
        if apply_mode_is_keep() {
            let _ = keep_apply_all();
        } else {
            let _ = deep_apply_all();
        }
    }
    WATCH_BUSY = 0;
}
