//! BTPAN and HTTP pages. Firmware calls stay in fw_api and the audited C port.
use crate::*;
use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

pub(crate) const PAGE_BTPAN_PID: u32 = 32;
pub(crate) const PAGE_WEBSRV_PID: u32 = 33;
pub(crate) const PAGE_NETINFO_PID: u32 = 34;
pub(crate) const PAGE_EXP_PID: u32 = 35;
pub(crate) const PAGE_DIAG_PID: u32 = 36;
const PAN_ROWS: usize = 12;
const WEB_ROWS: usize = 9; /* 0开关 1状态 2地址 3用户名 4密码 5设置密码 6btpan跳转 7统计 8诊断 */
const DEV_FIRST: usize = 6; /* 设备行 6..11, 扫到几台显示几台, 其余隐藏 */
const DEV_SLOTS: usize = 6;
const DEV_LAST: usize = DEV_FIRST + DEV_SLOTS - 1;
const ROW_CAP: usize = fw_api::MAX_ROWS_PER_PAGE;
const WEB_SET_PASSWORD: usize = 5;
const WEB_BTPAN: usize = 6;
const INFO_ROWS: usize = 10;
const DIAG_ROWS: usize = 10;
const _: () = assert!(PAN_ROWS <= ROW_CAP && WEB_ROWS <= ROW_CAP && INFO_ROWS <= ROW_CAP && DIAG_ROWS <= ROW_CAP);

#[repr(C)]
#[derive(Clone, Copy)]
struct Device { address: [u8; 6], rssi: i8, name: [u8; 21] }
#[repr(C)]
#[derive(Clone, Copy)]
struct Peer { device: Device, probe_index: u32 }
#[repr(C)]
struct NetworkView {
    phase: u32, count: u32, checked: u32, candidates: u32, selected: u32,
    passkey: u32, error_serial: u32, error: i32, error_stage: u32,
    cid: u16, mtu: u16, connected_name: [u8; 32], address: [u8; 6],
    peers: [Peer; 16], enabled: u32,
}
#[repr(C)]
struct IpInfo { ip: [u8; 4], mask: [u8; 4], gateway: [u8; 4], dns: [u8; 4], mac: [u8; 6], mtu: u16, ready: u32 }
#[repr(C)]
struct HttpView { enabled: u32, listening: u32, clients: u32, requests: u32, failures: u32, refused: u32, last_error: i32 }
#[repr(C)]
struct BtDiagnostics {
    adapter_state: u32, scan_stage: u32, scan_events: u32, scan_seen_start: u32, command: u32,
    ipc_result: i32, service_result: i32, sys_errno: i32, client_state: u32, client_fd: i32,
    l2_event: u32, l2_events: u32, error: i32, error_stage: u32,
    /* bond 回调枚举(固件字符串表实证): 0=BOND_NONE 1=BONDING 2=BONDED。 */
    bond_attempts: u32, bond_pending: u32, bonding: u32, bond_events: u32,
    bond_value: u32, bond_transport: u32, bond_order: u32, phase: u32,
    pair_reply_result: u32,
    device_configured: u32,
    result_cod: i32, result_iocap: i32, result_paircfg: i32,
    result_connect: i32, result_bond: i32, result_confirm: i32, result_remove: i32,
}
const _: () = assert!(core::mem::size_of::<Device>() == 28);
const _: () = assert!(core::mem::size_of::<Peer>() == 32);
const _: () = assert!(core::mem::size_of::<NetworkView>() == 596);
const _: () = assert!(core::mem::size_of::<IpInfo>() == 28);
const _: () = assert!(core::mem::size_of::<HttpView>() == 28);
const _: () = assert!(core::mem::size_of::<BtDiagnostics>() == 124);

unsafe extern "C" {
    fn chaos_pan_enable(on: i32);
    fn chaos_pan_scan();
    fn chaos_pan_connect(index: u32);
    fn chaos_pan_disconnect();
    fn chaos_pan_pair_confirm();
    fn network_service_view(out: *mut NetworkView);
    fn pan_ip_info(out: *mut IpInfo);
    fn chaos_pan_bt_poll();
    fn chaos_pan_net_poll(now_ms: u32);
    fn chaos_net_set_credentials(user: *const u8, pass: *const u8) -> i32;
    fn chaos_web_credentials(user: *mut u8, ucap: u32, pass: *mut u8, pcap: u32) -> i32;
    fn chaos_httpd_set_enable(on: i32);
    fn chaos_httpd_enabled() -> i32;
    fn chaos_httpd_snapshot(out: *mut HttpView);
    fn bt_probe_diagnostics(out: *mut BtDiagnostics);
    fn bt_probe_event_text(out: *mut u8, capacity: u32) -> u32;
}
static WORKER_STARTED: AtomicBool = AtomicBool::new(false);
static WORKER_ERROR: AtomicI32 = AtomicI32::new(0);
unsafe extern "C" fn net_worker_entry(_argc: i32, _argv: *mut *mut u8) -> i32 {
    let mut last_bt = fw_api::network_monotonic_ms().wrapping_sub(250);
    loop {
        let now = fw_api::network_monotonic_ms();
        chaos_pan_net_poll(now);
        if now.wrapping_sub(last_bt) >= 250 { last_bt = now; chaos_pan_bt_poll(); }
        let mut view: NetworkView = core::mem::zeroed(); network_service_view(&mut view);
        fw_api::nsh_usleep(if view.enabled != 0 || chaos_httpd_enabled() != 0 { 20_000 } else { 100_000 });
    }
}
/// Lazy resident worker. Services start disabled; default account chaos/chaos
/// (RAM-only, cleared on reboot — 设置密码可覆盖, 5～31 位)。
pub(crate) fn ensure_worker() {
    if WORKER_STARTED.swap(true, Ordering::AcqRel) { return; }
    unsafe { chaos_net_set_credentials(b"chaos\0".as_ptr(), b"chaos\0".as_ptr()); }
    let pid = unsafe { fw_api::nsh_spawn(b"chaos.net\0".as_ptr(), net_worker_entry, core::ptr::null_mut()) };
    WORKER_ERROR.store(if pid > 0 { 0 } else { pid.min(-1) }, Ordering::Release);
    if pid <= 0 { WORKER_STARTED.store(false, Ordering::Release); }
}
unsafe fn snapshot() -> (NetworkView, IpInfo) {
    let mut v = core::mem::zeroed(); let mut ip = core::mem::zeroed();
    network_service_view(&mut v); pan_ip_info(&mut ip); (v, ip)
}
unsafe fn visible(pid: u32) -> bool { FG_PAGE == pid && PAGES[pid as usize].built != 0 && page_is_live(pid as usize) && fw_api::top_page_ids() == (APP_ID, pid) }
unsafe fn interactive_row(pid: u32, slot: usize) -> bool {
    slot < ROW_CAP && visible(pid) && fw_api::screen_is_on() && PAGES[pid as usize].rows[slot] != 0
}
// Only LVGL owns these caches. Reset on render; ticks use live row handles.
struct RowCache { main: [[u8; 64]; ROW_CAP], sub: [[u8; 128]; ROW_CAP], selected: [u8; ROW_CAP], handles: [u32; ROW_CAP], valid: [bool; ROW_CAP] }
impl RowCache { const EMPTY: Self = Self { main: [[0;64];ROW_CAP], sub: [[0;128];ROW_CAP], selected: [0;ROW_CAP], handles: [0;ROW_CAP], valid: [false;ROW_CAP] }; }
static mut PAN_CACHE: RowCache = RowCache::EMPTY;
static mut WEB_CACHE: RowCache = RowCache::EMPTY;
static mut INFO_CACHE: RowCache = RowCache::EMPTY;
static mut DIAG_CACHE: RowCache = RowCache::EMPTY;
static mut DIAG_LABEL: u32 = 0;
static mut DIAG_CONTENT: u32 = 0;
static mut DIAG_ANCHOR: u32 = 0;
static mut DIAG_TEXT: [u8; 1024] = [0; 1024];
// The growing diagnostic label belongs to this content/last-row generation.
// Never update it just because a stale numeric LVGL handle still looks valid.
static mut INFO_DIAG_LABEL: u32 = 0;
static mut INFO_DIAG_CONTENT: u32 = 0;
static mut INFO_DIAG_ANCHOR: u32 = 0;
static mut INFO_DIAG_TEXT: [u8; 1024] = [0; 1024];
static mut UPDATING: bool = false;
static mut SHOWN: [Option<(u32, [u8;6])>; DEV_SLOTS] = [None;DEV_SLOTS];
static mut DEV_BUILT: usize = 0;      /* 当前已创建的设备行数(动态创建/删除) */
static mut DEV_SIG: [[u8;6]; DEV_SLOTS] = [[0;6];DEV_SLOTS];
static mut WEB_HINT: &[u8] = b"\0";
// UI-only target. published.enabled may lag several taps/worker polls. Keep
// the latest target even after matching a snapshot: older queued enable
// commands may still publish intermediate states. A new backend error or
// worker failure rolls the switch back to the actual published state.
static mut PAN_TARGET: Option<bool> = None;
static mut PAN_TARGET_ERROR_SERIAL: u32 = 0;
fn cbytes(bytes: &[u8]) -> &[u8] { &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())] }
fn label_bytes<const N: usize>(bytes: &[u8]) -> [u8; N] {
    let bytes = cbytes(bytes); let mut out = [0u8; N];
    let limit = bytes.len().min(N.saturating_sub(1));
    let count = match core::str::from_utf8(&bytes[..limit]) { Ok(_) => limit, Err(e) => e.valid_up_to() };
    out[..count].copy_from_slice(&bytes[..count]); out
}
unsafe fn pan_selected(v: &NetworkView) -> bool {
    if WORKER_ERROR.load(Ordering::Acquire) != 0 || (v.error != 0 && v.error_serial != PAN_TARGET_ERROR_SERIAL) { PAN_TARGET = None; }
    PAN_TARGET.unwrap_or(v.enabled != 0)
}
unsafe fn request_pan(on: bool, v: &NetworkView) {
    ensure_worker();
    if WORKER_STARTED.load(Ordering::Acquire) {
        PAN_TARGET = Some(on); PAN_TARGET_ERROR_SERIAL = v.error_serial; chaos_pan_enable(on as i32);
    } else { PAN_TARGET = None; }
    // The native switch already moved before VALUE_CHANGED; cached old
    // text/state must not suppress correction after a rejected request.
    PAN_CACHE.valid[0] = false;
}
unsafe fn set_row(pid: u32, slot: usize, main: &[u8], sub: &[u8], selected: bool) {
    if slot >= ROW_CAP { return; }
    let cache = match pid { PAGE_BTPAN_PID => &mut *core::ptr::addr_of_mut!(PAN_CACHE), PAGE_WEBSRV_PID => &mut *core::ptr::addr_of_mut!(WEB_CACHE), PAGE_NETINFO_PID => &mut *core::ptr::addr_of_mut!(INFO_CACHE), PAGE_DIAG_PID => &mut *core::ptr::addr_of_mut!(DIAG_CACHE), _ => return };
    let a = label_bytes::<64>(main); let b = label_bytes::<128>(sub);
    let row = PAGES[pid as usize].rows[slot]; if row == 0 { return; }
    if cache.valid[slot] && cache.handles[slot] == row && cache.main[slot] == a && cache.sub[slot] == b && cache.selected[slot] == selected as u8 { return; }
    cache.main[slot] = a; cache.sub[slot] = b; cache.selected[slot] = selected as u8; cache.handles[slot] = row; cache.valid[slot] = true;
    UPDATING = true;
    fw_api::row_update(row, fw_api::NONE_PTR, a.as_ptr(), if slot == 0 && !matches!(pid, PAGE_NETINFO_PID | PAGE_DIAG_PID) { fw_api::NONE_PTR } else { b.as_ptr() }, 0, selected as u8);
    UPDATING = false;
}
unsafe fn ip_text(ip: &[u8;4], out: &mut [u8]) {
    let mut w = crate::text::W::new(out.as_mut_ptr(), out.len());
    for (i, &b) in ip.iter().enumerate() { if i != 0 { w.c(b'.'); } w.n(b as u32); } w.end();
}
unsafe fn mac_text(mac: &[u8;6], reverse: bool, w: &mut crate::text::W) {
    const HEX: &[u8;16] = b"0123456789ABCDEF";
    for i in 0..6 { if i != 0 { w.c(b':'); } let b = mac[if reverse {5-i} else {i}]; w.c(HEX[(b>>4) as usize]).c(HEX[(b&15) as usize]); }
}
unsafe fn signed(w: &mut crate::text::W, value: i32) { if value < 0 { w.c(b'-'); } w.n(value.unsigned_abs()); }
fn error_stage(stage: u32) -> &'static [u8] {
    match stage { 1 => "扫描".as_bytes(), 2 => "连接请求".as_bytes(), 3 => "NAP筛选".as_bytes(),
        4 => "配对/L2CAP".as_bytes(), 5 => b"BNEP", 6 => b"DHCP", 7 => "协议栈".as_bytes(), _ => "网络".as_bytes() }
}
fn phase(v: &NetworkView) -> &'static [u8] {
    if v.enabled == 0 { return "未启用\0".as_bytes(); }
    match v.phase { 0 => "空闲\0".as_bytes(), 1 => "正在扫描\0".as_bytes(), 2 => "筛选网络共享设备\0".as_bytes(), 3 => "正在连接\0".as_bytes(), 4 => "等待确认配对\0".as_bytes(), 5 => "协商网络连接\0".as_bytes(), 6 => "正在获取IP\0".as_bytes(), 7 => "已连接\0".as_bytes(), _ => "未知状态\0".as_bytes() }
}
unsafe fn status_text(v: &NetworkView, out: &mut [u8]) {
    let mut w = crate::text::W::new(out.as_mut_ptr(), out.len()); let err = WORKER_ERROR.load(Ordering::Acquire);
    if err != 0 { w.s("网络任务启动失败 ".as_bytes()); signed(&mut w, err); }
    else if pan_selected(v) != (v.enabled != 0) { w.s(if pan_selected(v) { "正在启用btpan…".as_bytes() } else { "正在停用btpan…".as_bytes() }); }
    else { w.s(cbytes(phase(v))); if v.phase == 2 { w.c(b' ').n(v.checked).c(b'/').n(v.candidates); }
        if v.error != 0 {
            // 配对阶段(bt_probe 自己的 error_stage)不走 network_service 的阶段名,
            // 否则 -611 会被标成"DHCP", 误导排查方向。
            if v.error == -611 { w.s(" · 对端未完成配对，请在电脑上确认配对请求 ".as_bytes()); }
            else if v.error == -614 { w.s(" · 配对超时，请重试 ".as_bytes()); }
            else if v.error == -16 { w.s(" · 上次连接未结束，请先点断开 ".as_bytes()); }
            else {
                w.s(" · ".as_bytes()).s(error_stage(v.error_stage));
                let mut d: BtDiagnostics = core::mem::zeroed(); bt_probe_diagnostics(&mut d);
                if v.error_stage == 1 && d.ipc_result != 0 { w.s("IPC失败 ".as_bytes()); }
                else if v.error_stage == 1 && d.service_result > 0 { w.s("服务拒绝 ".as_bytes()); }
                else { w.s("错误 ".as_bytes()); }
            }
            signed(&mut w, v.error);
        } } w.end();
}
/// 行删除: 设备行不占位、不隐藏 —— 扫到几台创建几行, 设备没了整行删掉。
/// (HIDDEN flag 对 obj_align_to 手动定位的行不会引起重排, 必须真删。)
unsafe fn delete_device_rows() {
    let page = &mut PAGES[PAGE_BTPAN_PID as usize];
    for slot in DEV_FIRST..=DEV_LAST {
        let row = page.rows[slot];
        if row != 0 {
            fw_api::obj_delete(row);
            page.rows[slot] = 0;
            PAN_CACHE.valid[slot] = false;
        }
    }
    DEV_BUILT = 0;
}

unsafe fn refresh_pan() {
    let (v, _) = snapshot(); let mut text = [0u8;128];
    set_row(PAGE_BTPAN_PID,0,"启用btpan\0".as_bytes(),b"\0",pan_selected(&v));
    status_text(&v, &mut text); set_row(PAGE_BTPAN_PID,1,"连接状态\0".as_bytes(),&text,false);
    set_row(PAGE_BTPAN_PID,2,"扫描设备\0".as_bytes(),"手机需开启蓝牙网络共享\0".as_bytes(),false);
    set_row(PAGE_BTPAN_PID,3,"断开连接\0".as_bytes(),&v.connected_name,false);
    let mut w = crate::text::W::new(text.as_mut_ptr(),text.len());
    if v.phase == 4 && v.passkey != 0 {
        // SSP 数字比较: 手环和对端显示同一个 6 位数, 两边都要确认。
        w.s("配对码 ".as_bytes()); let mut div = 100_000; while div != 0 { w.c(b'0' + ((v.passkey / div) % 10) as u8); div /= 10; } w.s(" · 点击确认".as_bytes());
    } else if v.phase == 4 {
        // Just Works: 没有配对码, 确认动作发生在对端(电脑/手机)的弹窗上。
        w.s("配对中，请在电脑/手机上确认配对".as_bytes());
    } else { w.s("收到请求后核对配对码".as_bytes()); }
    w.end();
    set_row(PAGE_BTPAN_PID,4,"确认配对\0".as_bytes(),&text,false);
    set_row(PAGE_BTPAN_PID,5,"连接信息\0".as_bytes(),"IP、网关、DNS、MAC\0".as_bytes(),false);
    /* 设备列表: 一台设备一行, 动态创建/删除 —— 没有设备就没有卡片, 不留空位。 */
    let count = (v.count as usize).min(16);
    let shown = count.min(DEV_SLOTS);
    let mut sig = [[0u8;6]; DEV_SLOTS];
    for i in 0..shown { sig[i] = v.peers[i].device.address; }
    let stale = DEV_BUILT != shown || (0..shown).any(|i| DEV_SIG[i] != sig[i]);
    if stale {
        delete_device_rows();
        if shown > 0 {
            let ctx = PageCtx {
                content: PAGES[PAGE_BTPAN_PID as usize].content,
                title: PAGES[PAGE_BTPAN_PID as usize].title,
                rows: core::ptr::addr_of_mut!(PAGES[PAGE_BTPAN_PID as usize].rows) as *mut u32,
            };
            let mut prev = PAGES[PAGE_BTPAN_PID as usize].rows[DEV_FIRST - 1];
            for i in 0..shown {
                let r = build_row(&ctx, prev, DEV_FIRST + i, &RowSpec::tap(b" \0", b" \0".as_ptr()));
                if r == 0 { break; }
                prev = r; DEV_BUILT = i + 1;
            }
        }
        DEV_SIG = sig;
    }
    for i in 0..DEV_SLOTS {
        if i < DEV_BUILT {
            let peer = &v.peers[i]; SHOWN[i] = Some((i as u32, peer.device.address));
            let mut w = crate::text::W::new(text.as_mut_ptr(),text.len()); mac_text(&peer.device.address,true,&mut w); w.c(b' '); signed(&mut w,peer.device.rssi as i32); w.s(b" dBm"); w.end();
            let name = if peer.device.name[0] == 0 { "未命名设备\0".as_bytes() } else { &peer.device.name }; set_row(PAGE_BTPAN_PID,DEV_FIRST+i,name,&text,false);
        } else { SHOWN[i] = None; }
    }
}
pub(crate) struct BtpanPage;
pub(crate) static PAGE_BTPAN_P: BtpanPage = BtpanPage;
impl crate::page::Page for BtpanPage {
    fn fill_title(&self, buf: *mut u8) { unsafe { put_str(buf, "btpan联网\0".as_bytes()); } }
    fn render(&self, ctx: &PageCtx) -> PageResult { unsafe {
        ensure_worker(); PAN_CACHE = RowCache::EMPTY; SHOWN = [None;DEV_SLOTS];
        DEV_BUILT = 0; DEV_SIG = [[0;6];DEV_SLOTS]; let mut prev = ctx.title;
        for slot in 0..DEV_FIRST { let spec = if slot == 0 { RowSpec::switch("启用btpan\0".as_bytes()) } else if slot == 1 { RowSpec::display(b" \0", b" \0".as_ptr()) } else { RowSpec::tap(b" \0", b" \0".as_ptr()) };
            prev = build_row(ctx,prev,slot,&spec); if prev == 0 { return Err(PageError::RowFailed(slot)); } } refresh_pan(); Ok(())
    } }
    fn is_switch_row(&self, idx: usize) -> bool { idx == 0 }
    fn on_switch(&self, idx: usize) { unsafe {
        if idx != 0 || UPDATING || !interactive_row(PAGE_BTPAN_PID,idx) { return; }
        let (v,_) = snapshot(); request_pan(!pan_selected(&v),&v); refresh_pan();
    } }
    fn on_click(&self, idx: usize) { unsafe {
        if UPDATING || !interactive_row(PAGE_BTPAN_PID,idx) { return; } let (v,_) = snapshot();
        match idx {
            2 => { if !pan_selected(&v) { request_pan(true,&v); } else if WORKER_STARTED.load(Ordering::Acquire) { chaos_pan_scan(); } }
            3 => chaos_pan_disconnect(), 4 if v.phase == 4 => chaos_pan_pair_confirm(),
            5 => { nav_goto(PAGE_NETINFO_PID,0); return; }
            DEV_FIRST..=DEV_LAST if pan_selected(&v) && WORKER_STARTED.load(Ordering::Acquire) => { let row = idx - DEV_FIRST; if let Some((index,address)) = SHOWN[row] { if index < v.count.min(16) && v.peers[index as usize].device.address == address { chaos_pan_connect(index); } } }
            _ => {}
        } refresh_pan();
    } }
    fn needs_tick(&self) -> bool { true }
    fn tick(&self) { unsafe { if visible(PAGE_BTPAN_PID) && fw_api::screen_is_on() { refresh_pan(); } } }
}
pub(crate) struct NetInfoPage;
pub(crate) static PAGE_NETINFO_P: NetInfoPage = NetInfoPage;
unsafe fn refresh_info() {
    let (v, ip) = snapshot(); let mut text = [0u8;128]; status_text(&v,&mut text); set_row(PAGE_NETINFO_PID,0,"连接状态\0".as_bytes(),&text,false);
    set_row(PAGE_NETINFO_PID,1,"设备\0".as_bytes(),if v.cid != 0 { &v.connected_name } else { "未连接\0".as_bytes() },false);
    let mut w = crate::text::W::new(text.as_mut_ptr(),text.len()); if v.cid != 0 { mac_text(&v.address,true,&mut w); } else { w.s(b"--"); } w.end(); set_row(PAGE_NETINFO_PID,2,"远端MAC\0".as_bytes(),&text,false);
    for (slot,name,bytes) in [(3,b"IP\0".as_slice(),&ip.ip),(4,"子网掩码\0".as_bytes(),&ip.mask),(5,"网关\0".as_bytes(),&ip.gateway),(6,b"DNS\0".as_slice(),&ip.dns)] { ip_text(bytes,&mut text); set_row(PAGE_NETINFO_PID,slot,name,if ip.ready != 0 { &text } else { b"--\0" },false); }
    let mut w = crate::text::W::new(text.as_mut_ptr(),text.len()); mac_text(&ip.mac,false,&mut w); w.end(); set_row(PAGE_NETINFO_PID,7,"本地MAC\0".as_bytes(),if ip.ready != 0 { &text } else { b"--\0" },false);
    let mut w = crate::text::W::new(text.as_mut_ptr(),text.len()); w.n(ip.mtu as u32); w.end(); set_row(PAGE_NETINFO_PID,8,b"IP MTU\0",&text,false);
    let mut w = crate::text::W::new(text.as_mut_ptr(),text.len()); w.n(v.cid as u32).s(b" / ").n(v.mtu as u32); w.end(); set_row(PAGE_NETINFO_PID,9,b"L2CAP CID / MTU\0",&text,false);
    refresh_diagnostics();
}
unsafe fn refresh_diagnostics() {
    let page = &PAGES[PAGE_NETINFO_PID as usize];
    if !safe_ptr(INFO_DIAG_LABEL) || INFO_DIAG_CONTENT != page.content || INFO_DIAG_ANCHOR != page.rows[INFO_ROWS-1] { return; }
    let mut d: BtDiagnostics = core::mem::zeroed(); bt_probe_diagnostics(&mut d);
    let mut text = [0u8;1024]; let mut w=crate::text::W::new(text.as_mut_ptr(),text.len());
    w.s("蓝牙诊断\n客户端：".as_bytes()).s(match d.client_state {
        0 => "未建立".as_bytes(), 1 => "建立中".as_bytes(), 2 => "就绪".as_bytes(),
        3 => "创建失败".as_bytes(), 4 => "回调注册失败".as_bytes(), _ => "未知".as_bytes() });
    w.s("\n蓝牙状态：".as_bytes()).n(d.adapter_state).s(" / fd：".as_bytes()); signed(&mut w,d.client_fd);
    w.s("\n扫描阶段：".as_bytes()).n(d.scan_stage).s(" / 事件：".as_bytes()).n(d.scan_events);
    w.s("\n扫描已启动：".as_bytes()).s(if d.scan_seen_start != 0 {"是".as_bytes()} else {"否".as_bytes()});
    w.s("\nIPC命令：".as_bytes()).n(d.command).s(" / 返回：".as_bytes()); signed(&mut w,d.ipc_result);
    w.s("\n服务回复：".as_bytes()); if d.service_result < 0 {w.s("未收到有效回复".as_bytes());} else {signed(&mut w,d.service_result);}
    w.s("\n系统errno：".as_bytes());signed(&mut w,d.sys_errno);
    w.s("\nL2CAP事件：".as_bytes()).n(d.l2_event).s(" / 总数：".as_bytes()).n(d.l2_events);
    w.s("\n底层错误：".as_bytes());signed(&mut w,d.error);w.s(" / 阶段：".as_bytes()).n(d.error_stage);
    w.s("\nIPC非0表示通信失败；有效服务回复非0表示蓝牙服务拒绝。\0".as_bytes());w.end();
    if INFO_DIAG_TEXT != text {INFO_DIAG_TEXT=text;fw_api::label_set_text(INFO_DIAG_LABEL,core::ptr::addr_of!(INFO_DIAG_TEXT).cast::<u8>());}
}
impl crate::page::Page for NetInfoPage {
    fn fill_title(&self,buf:*mut u8) { unsafe { put_str(buf,"连接信息\0".as_bytes()); } }
    fn render(&self,ctx:&PageCtx)->PageResult { unsafe { INFO_CACHE = RowCache::EMPTY; INFO_DIAG_LABEL=0; INFO_DIAG_CONTENT=0; INFO_DIAG_ANCHOR=0; INFO_DIAG_TEXT=[0;1024]; let mut prev=ctx.title;
        for slot in 0..INFO_ROWS { prev=build_row(ctx,prev,slot,&RowSpec::display(b" \0",b" \0".as_ptr())); if prev==0 { return Err(PageError::RowFailed(slot)); } }
        let label=fw_api::label_create(ctx.content); if label==0 {return Err(PageError::RowFailed(INFO_ROWS));}
        fw_api::style_apply(label,FW_STYLE_HR_NAME,255,0);fw_api::obj_set_width(label,320);
        fw_api::obj_align_to(label,prev,fw_api::align::OUT_BOTTOM_LEFT,0,8);
        INFO_DIAG_LABEL=label; INFO_DIAG_CONTENT=ctx.content; INFO_DIAG_ANCHOR=prev;
        refresh_info(); Ok(())
    } }
    fn needs_tick(&self)->bool {true}
    fn on_click(&self, _idx: usize) {}
    fn tick(&self) { unsafe { if visible(PAGE_NETINFO_PID) && fw_api::screen_is_on() { refresh_info(); } } }
}
/// 页36: 蓝牙诊断(实验性功能 -> 蓝牙诊断)。把回调/命令/配对状态原样摊开,
/// 现场就不必再靠猜。数据全部来自 bt_probe_diagnostics + bt_probe_event_text。
pub(crate) struct BtDiagPage;
pub(crate) static PAGE_DIAG_P: BtDiagPage = BtDiagPage;
unsafe fn refresh_diag() {
    let mut d: BtDiagnostics = core::mem::zeroed(); bt_probe_diagnostics(&mut d);
    let mut text = [0u8; 128];
    let mut w = crate::text::W::new(text.as_mut_ptr(), text.len());
    w.s(match d.client_state { 0 => "未建立".as_bytes(), 1 => "建立中".as_bytes(), 2 => "就绪".as_bytes(),
        3 => "创建失败".as_bytes(), 4 => "回调注册失败".as_bytes(), _ => "未知".as_bytes() });
    w.s(" · fd ".as_bytes()); signed(&mut w, d.client_fd); w.end();
    set_row(PAGE_DIAG_PID, 0, "蓝牙客户端\0".as_bytes(), &text, false);
    let mut w = crate::text::W::new(text.as_mut_ptr(), text.len());
    w.s("BT ".as_bytes()).n(d.adapter_state).s(" · 阶段 ".as_bytes()).n(d.scan_stage)
        .s(" · 配置 ".as_bytes()).s(if d.device_configured != 0 { "已下发".as_bytes() } else { "未下发".as_bytes() }).end();
    set_row(PAGE_DIAG_PID, 1, "适配器\0".as_bytes(), &text, false);
    let mut w = crate::text::W::new(text.as_mut_ptr(), text.len());
    w.s("cmd ".as_bytes()).n(d.command).s(" · 返回 ".as_bytes()); signed(&mut w, d.ipc_result);
    w.s(" · 服务 ".as_bytes()); signed(&mut w, d.service_result); w.s(" · errno ".as_bytes()); signed(&mut w, d.sys_errno); w.end();
    set_row(PAGE_DIAG_PID, 2, "最近IPC\0".as_bytes(), &text, false);
    let bond = if d.bonding != 0 { "BONDING 配对中".as_bytes() }
        else { match d.bond_value { 0 => "BOND_NONE 未绑定".as_bytes(), 1 => "BONDING 配对中".as_bytes(),
            2 => "BONDED 已绑定".as_bytes(), _ => "无回调".as_bytes() } };
    let mut w = crate::text::W::new(text.as_mut_ptr(), text.len());
    w.s(bond).s(" · 尝试 ".as_bytes()).n(d.bond_attempts).s("/8".as_bytes());
    if d.bond_pending != 0 { w.s(" · 等回调".as_bytes()); } w.end();
    set_row(PAGE_DIAG_PID, 3, "配对状态\0".as_bytes(), &text, false);
    let mut w = crate::text::W::new(text.as_mut_ptr(), text.len());
    w.s("回调 ".as_bytes()).n(d.bond_events).s(" 次 · transport ".as_bytes());
    if d.bond_transport == 0xffff { w.s(b"--"); } else { w.n(d.bond_transport); }
    w.s(" · 接受回复 ".as_bytes()); signed(&mut w, d.pair_reply_result as i32); w.end();
    set_row(PAGE_DIAG_PID, 4, "配对回调\0".as_bytes(), &text, false);
    let mut w = crate::text::W::new(text.as_mut_ptr(), text.len());
    w.s("事件 ".as_bytes()).n(d.l2_event).s(" · 累计 ".as_bytes()).n(d.l2_events); w.end();
    set_row(PAGE_DIAG_PID, 5, "L2CAP\0".as_bytes(), &text, false);
    let mut w = crate::text::W::new(text.as_mut_ptr(), text.len());
    signed(&mut w, d.error); w.s(" · 阶段 ".as_bytes()).n(d.error_stage).s(" · phase ".as_bytes()).n(d.phase); w.end();
    set_row(PAGE_DIAG_PID, 6, "底层错误\0".as_bytes(), &text, false);
    let mut w = crate::text::W::new(text.as_mut_ptr(), text.len());
    w.s("CoD ".as_bytes()); signed(&mut w, d.result_cod);
    w.s(" IO ".as_bytes()); signed(&mut w, d.result_iocap);
    w.s(" 3D ".as_bytes()); signed(&mut w, d.result_paircfg); w.end();
    set_row(PAGE_DIAG_PID, 7, "设备配置返回\0".as_bytes(), &text, false);
    let mut w = crate::text::W::new(text.as_mut_ptr(), text.len());
    w.s("Conn ".as_bytes()); signed(&mut w, d.result_connect);
    w.s(" Bond ".as_bytes()); signed(&mut w, d.result_bond);
    w.s(" Rm ".as_bytes()); signed(&mut w, d.result_remove);
    w.s(" Cfm ".as_bytes()); signed(&mut w, d.result_confirm); w.end();
    set_row(PAGE_DIAG_PID, 8, "配对命令返回\0".as_bytes(), &text, false);
    set_row(PAGE_DIAG_PID, 9, "最近回调\0".as_bytes(), "见下方事件列表\0".as_bytes(), false);
    update_diag_log();
}
unsafe fn update_diag_log() {
    let page = &PAGES[PAGE_DIAG_PID as usize];
    if !safe_ptr(DIAG_LABEL) || DIAG_CONTENT != page.content || DIAG_ANCHOR != page.rows[DIAG_ROWS-1] { return; }
    let mut scratch = [0u8; 640];
    let size = bt_probe_event_text(scratch.as_mut_ptr(), scratch.len() as u32) as usize;
    if size + 1 > scratch.len() { return; }
    scratch[size] = 0;
    if DIAG_TEXT[..=size] != scratch[..=size] {
        DIAG_TEXT[..=size].copy_from_slice(&scratch[..=size]);
        fw_api::label_set_text(DIAG_LABEL, core::ptr::addr_of!(DIAG_TEXT).cast::<u8>());
    }
}
impl crate::page::Page for BtDiagPage {
    fn fill_title(&self, buf: *mut u8) { unsafe { put_str(buf, "蓝牙诊断\0".as_bytes()); } }
    fn render(&self, ctx: &PageCtx) -> PageResult { unsafe {
        DIAG_CACHE = RowCache::EMPTY; DIAG_LABEL = 0; DIAG_CONTENT = 0; DIAG_ANCHOR = 0; DIAG_TEXT = [0; 1024];
        let mut prev = ctx.title;
        for slot in 0..DIAG_ROWS {
            prev = build_row(ctx, prev, slot, &RowSpec::display(b" \0", b" \0".as_ptr()));
            if prev == 0 { return Err(PageError::RowFailed(slot)); }
        }
        let label = fw_api::label_create(ctx.content); if label == 0 { return Err(PageError::RowFailed(DIAG_ROWS)); }
        fw_api::style_apply(label, FW_STYLE_HR_NAME, 255, 0); fw_api::obj_set_width(label, 320);
        fw_api::obj_align_to(label, prev, fw_api::align::OUT_BOTTOM_LEFT, 0, 8);
        DIAG_LABEL = label; DIAG_CONTENT = ctx.content; DIAG_ANCHOR = prev;
        refresh_diag(); Ok(())
    } }
    fn needs_tick(&self) -> bool { true }
    fn on_click(&self, _idx: usize) {}
    fn tick(&self) { unsafe { if visible(PAGE_DIAG_PID) && fw_api::screen_is_on() { refresh_diag(); } } }
}
/// The shared keyboard calls this before returning. Credentials stay in RAM.
pub(crate) unsafe fn save_web_password(password:*const u8)->bool {
    if password.is_null() {return false;} let mut n=0;
    while n<=31 { let c=read_volatile(password.add(n)); if c==0 {break;} if !(0x20..=0x7e).contains(&c) {return false;} n+=1; }
    if !(5..=31).contains(&n) {return false;}
    let mut user=[0u8;32]; let mut previous=[0u8;65];
    if chaos_web_credentials(user.as_mut_ptr(),32,previous.as_mut_ptr(),65)==0
        && cbytes(&previous)==core::slice::from_raw_parts(password,n) {
        // An unchanged save must neither stop HTTP nor leave a misleading
        // "re-enable" hint over an already running service.
        WEB_HINT=b"\0"; return true;
    }
    let ok=chaos_net_set_credentials(b"chaos\0".as_ptr(),password)==0;
    WEB_HINT=if ok {"密码已更新，请重新开启服务器\0".as_bytes()} else {"密码保存失败，请重试\0".as_bytes()}; ok
}
unsafe fn refresh_web() {
    let (_,ip)=snapshot(); let mut http:HttpView=core::mem::zeroed(); chaos_httpd_snapshot(&mut http);
    let mut user=[0u8;32];let mut password=[0u8;65];let has_password=chaos_web_credentials(user.as_mut_ptr(),32,password.as_mut_ptr(),65)==0; let enabled=chaos_httpd_enabled()!=0;
    set_row(PAGE_WEBSRV_PID,0,"网页服务器\0".as_bytes(),b"\0",enabled); let err=WORKER_ERROR.load(Ordering::Acquire);
    let status=if err!=0 {"网络任务启动失败\0".as_bytes()} else if WEB_HINT[0]!=0 {WEB_HINT} else if !enabled {"已关闭\0".as_bytes()} else if ip.ready==0 {"等待btpan获取IP\0".as_bytes()} else if http.listening!=0 {"运行中\0".as_bytes()} else {"正在启动\0".as_bytes()};
    set_row(PAGE_WEBSRV_PID,1,"状态\0".as_bytes(),status,false); let mut text=[0u8;128]; let mut addr=[0u8;20];ip_text(&ip.ip,&mut addr);
    let mut w=crate::text::W::new(text.as_mut_ptr(),text.len()); if ip.ready!=0 {w.s(b"http://").s(cbytes(&addr)).c(b'/');} else {w.s("连接btpan后显示地址".as_bytes());}w.end();
    set_row(PAGE_WEBSRV_PID,2,"访问地址\0".as_bytes(),&text,false);
    let username=if has_password {cbytes(&user)} else {"chaos\0".as_bytes()};
    set_row(PAGE_WEBSRV_PID,3,"用户名\0".as_bytes(),username,false);
    /* 密码单行整显(单行标签放不下长密码时会截断, 重进页面/重新输入可再看)。 */
    set_row(PAGE_WEBSRV_PID,4,"密码\0".as_bytes(),if has_password {cbytes(&password)} else {"chaos\0".as_bytes()},false);
    set_row(PAGE_WEBSRV_PID,WEB_SET_PASSWORD,"设置密码\0".as_bytes(),"5～31位英文、数字或符号\0".as_bytes(),false); set_row(PAGE_WEBSRV_PID,WEB_BTPAN,"btpan联网\0".as_bytes(),"连接或查看网络信息\0".as_bytes(),false);
    let mut w=crate::text::W::new(text.as_mut_ptr(),text.len());w.n(http.clients).s(" 个连接 · ".as_bytes()).n(http.requests).s(" 个请求".as_bytes());w.end(); set_row(PAGE_WEBSRV_PID,7,"访问统计\0".as_bytes(),&text,false);
    let mut w=crate::text::W::new(text.as_mut_ptr(),text.len());signed(&mut w,http.last_error);w.s(" · 失败 ".as_bytes()).n(http.failures).s(" · 拒绝 ".as_bytes()).n(http.refused);w.end(); set_row(PAGE_WEBSRV_PID,8,"服务诊断\0".as_bytes(),&text,false);
}
pub(crate) struct WebSrvPage;
pub(crate) static PAGE_WEBSRV_P:WebSrvPage=WebSrvPage;
impl crate::page::Page for WebSrvPage {
    fn fill_title(&self,buf:*mut u8) {unsafe{put_str(buf,"网页服务器\0".as_bytes());}}
    fn render(&self,ctx:&PageCtx)->PageResult {unsafe{ ensure_worker();WEB_CACHE=RowCache::EMPTY;let mut prev=ctx.title;
        for slot in 0..WEB_ROWS { let spec=if slot==0 {RowSpec::switch("网页服务器\0".as_bytes())} else if slot==WEB_SET_PASSWORD || slot==WEB_BTPAN {RowSpec::tap(b" \0",b" \0".as_ptr())} else {RowSpec::display(b" \0",b" \0".as_ptr())}; prev=build_row(ctx,prev,slot,&spec);if prev==0{return Err(PageError::RowFailed(slot));} }
        let help=fw_api::label_create(ctx.content); if help!=0 {fw_api::style_apply(help,FW_STYLE_HR_NAME,255,0);fw_api::obj_set_width(help,320);fw_api::label_set_text(help,"先在手机开启蓝牙网络共享，再连接btpan。设置密码后开启服务器，在同一蓝牙网络访问上方地址。\n网页使用HTTP，请在可信设备间使用。密码重启后清除，离开页面继续运行。\0".as_ptr());fw_api::obj_align_to(help,prev,fw_api::align::OUT_BOTTOM_LEFT,0,8);}
        refresh_web();Ok(())
    }}
    fn is_switch_row(&self,idx:usize)->bool {idx==0}
    fn on_switch(&self,idx:usize) {unsafe{ if idx!=0 || UPDATING || !interactive_row(PAGE_WEBSRV_PID,idx) {return;} WEB_HINT=b"\0";
        if chaos_httpd_enabled()!=0 {chaos_httpd_set_enable(0); if chaos_httpd_enabled()!=0 {WEB_HINT="停止请求未生效，请重试\0".as_bytes();}} else {
            let mut user=[0u8;32];let mut pass=[0u8;65];
            if chaos_web_credentials(user.as_mut_ptr(),32,pass.as_mut_ptr(),65)!=0 {WEB_HINT="请先设置5～31位密码\0".as_bytes();}
            else {ensure_worker(); if WORKER_STARTED.load(Ordering::Acquire) {chaos_httpd_set_enable(1);if chaos_httpd_enabled()==0 {WEB_HINT="启动请求未生效，请重试\0".as_bytes();}}}
        }
        WEB_CACHE.valid[0]=false;refresh_web();
    }}
    fn on_click(&self,idx:usize) {unsafe{if UPDATING || !interactive_row(PAGE_WEBSRV_PID,idx) {return;} match idx {WEB_SET_PASSWORD=>crate::keyboard::open(core::ptr::null(),crate::keyboard::Completion::WebPassword),WEB_BTPAN=>nav_goto(PAGE_BTPAN_PID,0),_=>{}}}}
    fn needs_tick(&self)->bool {true}
    fn tick(&self) {unsafe{if visible(PAGE_WEBSRV_PID) && fw_api::screen_is_on(){refresh_web();}}}
}

/// 主页 -> 实验性功能(页35)。收拢未稳定入口: btpan联网 / 网页服务器 / 连接信息。
pub(crate) struct ExpPage;
pub(crate) static PAGE_EXP_P: ExpPage = ExpPage;
impl crate::page::Page for ExpPage {
    fn fill_title(&self, buf: *mut u8) { unsafe { put_str(buf, "实验性功能\0".as_bytes()); } }
    fn render(&self, ctx: &PageCtx) -> PageResult { unsafe {
        build_rows(ctx, ctx.title, &[
            (0, RowSpec::tap("btpan联网\0".as_bytes(), "蓝牙网络共享实验功能\0".as_ptr())),
            (1, RowSpec::tap("网页服务器\0".as_bytes(), "局域网控制台(实验)\0".as_ptr())),
            (2, RowSpec::tap("蓝牙诊断\0".as_bytes(), "配对回调与 IPC 现场\0".as_ptr())),
        ])?; Ok(())
    } }
    fn on_click(&self, idx: usize) { unsafe {
        match idx {
            0 => nav_goto(PAGE_BTPAN_PID, 0),
            1 => nav_goto(PAGE_WEBSRV_PID, 0),
            2 => nav_goto(PAGE_DIAG_PID, 0),
            _ => {}
        }
    } }
    fn needs_tick(&self) -> bool { false }
}

#[cfg(test)]
#[path = "../tests/btpan_ui_cases.rs"]
mod tests;
