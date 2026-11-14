//! 主界面背景生命周期；退出前撤图，结构变化由已有 UI 调度消费。
use crate::background_scene::Backdrop;
use crate::fw_api;
use crate::mem::{rd16, rd32};
use crate::{st_rd, st_wr};

static mut SCENE: Backdrop<4> = Backdrop::new();
static mut MODE: u32 = 0;
/// 背景档位那一页(系统模糊)的 page_id: 页号只在这里写一次 ——
/// 状态变化要重画的是这一页(见 tick 里的 DIRTY 分支), 美化菜单页13 从本页跳过来。
pub(crate) const BLUR_PID: u32 = 4;
static mut REQUEST: u32 = 0;
static mut HOOKED: u32 = 0;
static mut ATTACHED: u32 = 0;
static mut SOURCE_MODE: u32 = 0;
static mut GENERATE: u32 = 0;
static mut STATUS: u32 = 0;
static mut DIRTY: u32 = 0;
static mut REFRESH: u32 = 0;
static mut DELETED: [(u32, u32); 4] = [(0, 0); 4];
static mut WATCHED_ROOT: u32 = 0;
static mut WATCHED_ROOTS: [u32; 4] = [0; 4];
static mut MUTATING: u32 = 0;
static mut PERSIST_LOADED: u32 = 0;
// 实时背景模糊(跟随表盘): 表盘 identity 变化时自动重生成静态模糊, 免手动点"生成"。
// 注意语义: 这是"事件触发的自动刷新", 不是逐帧 GPU 模糊 —— 固件 GPU(VGLite)没有
// 可从模块安全调用的模糊入口(取证见 Ghidra/blur-gpu-feasibility.md), 逐帧方案被否。
static mut RT_LOADED: u32 = 0;
static mut RT_ON: u32 = 0;
static mut RT_WF: u32 = 0;    // 上次见到的表盘 identity(基线)
static mut RT_VALID: u32 = 0; // 基线是否已校准(打开开关后第一次见到表盘时校准)
// 0 = legacy ownership, 1 = Corona committed, 2 = Corona deferred cleanup.
// No legacy mutations are allowed in states 1/2. ATTACHED and SCENE remain
// exclusively owned by the legacy path; Corona never lends them an image.
static mut CORONA_BG_ACTIVE: u32 = 0;
// Last successfully committed shared region mask; a busy commit must not
// consume a newer selection. Zero is valid: Corona owns no blurred regions.
static mut CORONA_BG_MASK: u32 = 0;
// 所有权看门狗：连续几拍发现 corona 已经不再持有原生 owner（被 DELETE/手势
// 摘掉但没人重新 commit）时，把状态归零让下一拍重新 commit。丢了所有权还继续
// 认定 CORONA_BG_ACTIVE==1，表现就是"模糊不动了，换什么档位都是透明"。
static mut CORONA_OWN_LOST: u32 = 0;
// commit 连续失败计数；到阈值就先把 corona 完整下架、让旧引擎接管一段时间，
// 到点再自动重试一次（否则界面会一直停在"看着像坏了"的状态）。
static mut CORONA_FAIL: u32 = 0;
static mut CORONA_HOLD: u32 = 0;      // >0 = 暂停拍数，期间旧引擎接管
static mut CORONA_HOLD_ARM: u32 = 0;  // 下架序列走完后进入暂停
static mut CORONA_COOLDOWN: u32 = 0;  // 丢所有权后歇几拍再重装（防抖）

/// 连续失败多少拍才降级（tick 50ms → 100 拍 ≈ 5s）。
const CORONA_FAIL_LIMIT: u32 = 100;
/// 降级持续多少拍（≈ 20s），到期重试一次。
const CORONA_HOLD_TICKS: u32 = 400;
/// 所有权连续丢失多少拍才认定真的丢了（避免手势过程中的瞬态误判）。
const CORONA_OWN_LOST_LIMIT: u32 = 3;
/// 重装冷却（≈ 2s）：丢所有权后的重试间隔，避免 commit/丢失高频抖动。
const CORONA_RETRY_TICKS: u32 = 40;
// 模块注入后给其他模块留出启动窗口，约 50 * 50ms = 2.5 秒。
static mut BOOT_DELAY: u32 = 0;

// ===== launcher 滑动震动(两个独立开关) =====
// 逻辑借鉴 11(q66u 4.100.071) 的 persist.vibrate_launcher_state /
// persist.vibrate_back_state 两个设置(那代框架 10 Pro 没有, 触发自己做)。
// 震动走 fw_api::vibrate(0xD) = 固件官方翻页效果(0x0C4C56CC, 内部跟随系统震动开关)。
//
// * 滑动列表震动: launcher 根上的**任意事件**(v1 原始方案, 真机实证会震 —— 滚动时
//   磁贴回收/滚动事件都会到这; 而 SCROLL_END(10) 经实测根本不来, 码过滤全是死路),
//   冷却 200ms 限频: 持续滑动 = 每 200ms 震一次(跟手节奏), 单次操作 = 震一次,
//   停手即停(真机 2026-10-16 三轮迭代定稿)。
// * 右滑返回震动: 前台 app 切回 HOME(右滑返回/退出应用的落点都是进 launcher)震一次。
// 持久化 /data/chaos/swipe_vib.cfg 2 字节: [滑动列表, 右滑返回], 默认全关。
static mut SV_LOADED: u32 = 0;
static mut SV_LIST_ON: u32 = 0;
static mut SV_BACK_ON: u32 = 0;
static mut SV_REQ: u32 = 0;          // bit0=列表翻越 bit1=右滑返回/进 launcher
static mut SV_TICK: u32 = 0;         // 50ms 计数
static mut SV_LAST_VIB: u32 = 0;     // 上次震动时刻(SV_TICK)
static mut SV_LAST_VALID: u32 = 0;
static mut SV_LAST_TOP: u32 = 0;     // 上一 tick 前台 app id
static mut SV_TOP_VALID: u32 = 0;
static mut VIB_TEST_REQ: u32 = 0;

const SV_CFG: &[u8] = b"/data/chaos/swipe_vib.cfg\0";
/// 冷却: 两次震动最小间隔。tick 粒度 50ms, 80ms 不可整除 → 取 2 tick = 100ms
/// (最接近的档位; 要精确 80ms 得引固件 ms 级时钟源, 暂不必)
const SV_COOLDOWN: u32 = 2;

unsafe fn load_sv() {
    if SV_LOADED != 0 { return; }
    SV_LOADED = 1;
    let fd = fw_api::open(SV_CFG.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd >= 0 {
        // 兼容旧版 1 字节文件(只有滑动列表): 读到几个算几个, 缺的位保持默认关
        let mut v = [0u8; 2];
        let n = fw_api::read(fd, v.as_mut_ptr(), 2);
        if n >= 1 && v[0] <= 1 { st_wr!(SV_LIST_ON, v[0] as u32); }
        if n >= 2 && v[1] <= 1 { st_wr!(SV_BACK_ON, v[1] as u32); }
        fw_api::close(fd);
    }
}

pub(crate) unsafe fn swipe_list_on() -> u32 { st_rd!(SV_LIST_ON) }
pub(crate) unsafe fn swipe_back_on() -> u32 { st_rd!(SV_BACK_ON) }

pub(crate) unsafe fn set_swipe_vib(list: bool, on: u32) {
    if list { st_wr!(SV_LIST_ON, on); } else { st_wr!(SV_BACK_ON, on); }
    let v = [st_rd!(SV_LIST_ON) as u8, st_rd!(SV_BACK_ON) as u8];
    let fd = fw_api::open(SV_CFG.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd >= 0 { let _ = fw_api::write(fd, v.as_ptr(), 2); fw_api::close(fd); }
}

pub(crate) unsafe fn vib_test_req() { st_wr!(VIB_TEST_REQ, 1); }



/// tick: 右滑返回/进 launcher = 前台 app 切回 HOME
unsafe fn sv_back_tick() {
    if st_rd!(SV_BACK_ON) == 0 { return; }
    let top = fw_api::top_app_id();
    if st_rd!(SV_TOP_VALID) == 0 {
        st_wr!(SV_TOP_VALID, 1);
        st_wr!(SV_LAST_TOP, top);
        return;
    }
    if top != st_rd!(SV_LAST_TOP) {
        let prev = st_rd!(SV_LAST_TOP);
        st_wr!(SV_LAST_TOP, top);
        if top == fw_api::HOME_APP_ID && prev != fw_api::HOME_APP_ID {
            st_wr!(SV_REQ, st_rd!(SV_REQ) | 2);
        }
    }
}

/// structure_changed 里调: launcher 根上的任意事件 + 冷却。
/// 真机 2026-10-16 定稿: 按事件码过滤(9/10/11)会让滑动完全不震 —— 滚动事件
/// 就藏在 PRESSING/其它码里, 分不出; "按住不滑连震"是已知代价, 用户接受。
pub(crate) unsafe fn swipe_scroll_evt(current: u32, target: u32) {
    if st_rd!(SV_LIST_ON) == 0 { return; }
    let roots = st_rd!(WATCHED_ROOTS);
    let launcher = roots[0];
    if launcher == 0 || (current != launcher && target != launcher) { return; }
    let t = st_rd!(SV_TICK);
    let last = st_rd!(SV_LAST_VIB);
    if st_rd!(SV_LAST_VALID) == 0 || t.wrapping_sub(last) >= SV_COOLDOWN {
        st_wr!(SV_REQ, st_rd!(SV_REQ) | 1);
        st_wr!(SV_LAST_VIB, t);
        st_wr!(SV_LAST_VALID, 1);
    }
}

unsafe fn vib_tick() {
    st_wr!(SV_TICK, st_rd!(SV_TICK).wrapping_add(1));
    sv_back_tick();
    let req = st_rd!(SV_REQ);
    if req != 0 {
        st_wr!(SV_REQ, 0);
        fw_api::vibrate(0xD);
    }
    if st_rd!(VIB_TEST_REQ) != 0 {
        st_wr!(VIB_TEST_REQ, 0);
        fw_api::vibrate(0xD);
    }
}

unsafe fn persist_mode(mode: u32) {
    let path = b"/data/chaos/background.cfg\0";
    let fd = fw_api::open(path.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd >= 0 { let v = mode as u8; let _ = fw_api::write(fd, &v as *const u8, 1); fw_api::close(fd); }
}

unsafe fn load_mode() {
    if st_rd!(PERSIST_LOADED) != 0 { return; }
    st_wr!(PERSIST_LOADED, 1);
    // 模糊区域掩码必须在这条 once 路径里一起装: 重启注入后第一次 attach 走的就是
    // masked_targets(), 不在这读文件的话 MASK 永远是静态默认 0x0F(全开),
    // 表现为"关了区域重启后又被全局模糊"(2026-10-16 真机踩坑)。
    crate::blur_zone::ensure_loaded();
    load_rt();
    load_sv();
    let fd = fw_api::open(b"/data/chaos/background.cfg\0".as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 { return; }
    let mut v = 0u8;
    // 只认 0/1/3; 设备上存的旧值 2(已删的固定图片背景)按"系统背景"处理
    if fw_api::read(fd, &mut v as *mut u8, 1) == 1 && (v == 0 || v == 1 || v == 3) {
        st_wr!(MODE, v as u32);
        if v == 1 { st_wr!(REQUEST, 1); st_wr!(GENERATE, 1); }
        else if v == 3 { st_wr!(REQUEST, 1); }
    }
    fw_api::close(fd);
}

/// 子对象创建和删除会向父链冒泡；回调只登记请求。
unsafe extern "C" fn structure_changed(event: u32) {
    if event == 0 { return; }
    let current = rd32(event as *const u32);
    let target = rd32((event + 4) as *const u32);
    // launcher 滑动震动先判: 回调挂在 launcher 根上, 子对象的事件靠冒泡送进来,
    // 而下面那道"是不是我们盯的根"的过滤只看 current —— 子对象来的事件会被它整条丢掉,
    // 表现就是滑动列表震动时有时无。swipe_scroll_evt 自己会校验事件确实来自 launcher。
    swipe_scroll_evt(current, target);
    let mut watched = current == st_rd!(WATCHED_ROOT);
    for root in st_rd!(WATCHED_ROOTS) {
        if root != 0 && root == current { watched = true; }
    }
    if !watched { return; }
    let code = rd16((event + 8) as *const u16) & 0x7FFF;
    if code == 0x24 && current == target {
        if current == st_rd!(WATCHED_ROOT) { st_wr!(WATCHED_ROOT, 0); }
        let mut roots = st_rd!(WATCHED_ROOTS);
        for root in &mut roots { if *root == current { *root = 0; } }
        st_wr!(WATCHED_ROOTS, roots);
    } else if (code == 0x26 || code == 0x27)
        && st_rd!(MODE) != 0 && st_rd!(MUTATING) == 0 {
        st_wr!(REQUEST, 1);
        crate::background_surfaces::rescan();
    }
}

unsafe fn watch_structure() -> bool {
    let root = rd32((fw_api::BACKGROUND_HOME_DESC + 0x30) as *const u32);
    let Some(targets) = fw_api::background_home_targets() else { return false; };
    if root != st_rd!(WATCHED_ROOT) {
        if fw_api::obj_add_event(root, structure_changed as *const () as u32, 0, root) == 0 {
            return false;
        }
        st_wr!(WATCHED_ROOT, root);
    }
    let mut watched = st_rd!(WATCHED_ROOTS);
    for target in targets {
        if target == 0 || watched.contains(&target) { continue; }
        if fw_api::obj_add_event(target, structure_changed as *const () as u32, 0, target) == 0 {
            return false;
        }
        if let Some(slot) = watched.iter_mut().find(|value| **value == 0) {
            *slot = target;
        } else {
            return false;
        }
    }
    st_wr!(WATCHED_ROOTS, watched);
    true
}

fn callbacks() -> [u32; 4] {
    [home_signal as *const () as u32, home_create as *const () as u32,
     home_resume as *const () as u32, home_delete as *const () as u32]
}

unsafe fn install() -> bool {
    if !fw_api::background_home_matches() { return false; }
    let callbacks = callbacks();
    if st_rd!(HOOKED) != 0 {
        crate::background_pages::install();
        return (0..4).all(|i| rd32((fw_api::BACKGROUND_HOME_DESC +
            fw_api::BACKGROUND_HOME_SLOTS[i]) as *const u32) == callbacks[i]);
    }
    for i in 0..4 {
        let address = fw_api::BACKGROUND_HOME_DESC + fw_api::BACKGROUND_HOME_SLOTS[i];
        if callbacks[i] & 1 == 0
            || rd32(address as *const u32) != fw_api::BACKGROUND_HOME_ORIGINALS[i] {
            return false;
        }
    }
    for i in 0..4 {
        core::ptr::write_volatile((fw_api::BACKGROUND_HOME_DESC +
            fw_api::BACKGROUND_HOME_SLOTS[i]) as *mut u32, callbacks[i]);
    }
    st_wr!(HOOKED, 1);
    // 通知聚合页和详情页拥有独立页面生命周期；单页缺失不阻挡 home 背景。
    crate::background_pages::install();
    true
}

unsafe fn uninstall() {
    if st_rd!(HOOKED) == 0 { return; }
    let callbacks = callbacks();
    for i in 0..4 {
        let address = fw_api::BACKGROUND_HOME_DESC + fw_api::BACKGROUND_HOME_SLOTS[i];
        if rd32(address as *const u32) == callbacks[i] {
            core::ptr::write_volatile(address as *mut u32, fw_api::BACKGROUND_HOME_ORIGINALS[i]);
        }
    }
    st_wr!(HOOKED, 0);
}

/// 独立台账避免 DELETE 回调重入场景的可变引用。
unsafe extern "C" fn image_deleted(event: u32) {
    if event == 0 { return; }
    let target = rd32(event as *const u32);
    let current = rd32((event + 4) as *const u32);
    let tag = rd32((event + 12) as *const u32);
    let slot = (tag & 3) as usize;
    if target == current && rd16((event + 8) as *const u16) & 0x7FFF == 0x24 && slot < 4 {
        st_wr!(DELETED[slot], (tag, target));
    }
}

unsafe fn consume_deleted() {
    for i in 0..4 {
        let (tag, target) = st_rd!(DELETED[i]);
        st_wr!(DELETED[i], (0, 0));
        if target != 0 && SCENE.deleted(tag, target) {
            st_wr!(ATTACHED, 0);
            if st_rd!(MODE) != 0 { st_wr!(REQUEST, 1); }
        }
    }
}

unsafe fn unmount() -> bool {
    st_wr!(MUTATING, 1);
    consume_deleted();
    let parents = fw_api::background_home_targets();
    if (parents.is_none() && !SCENE.unmounted())
        || !crate::background_surfaces::restore(parents) {
        st_wr!(MUTATING, 0);
        return false;
    }
    if let Some(parents) = parents { SCENE.restore_deleted(parents); }
    crate::background_pages::uninstall();
    crate::background_assets::reset_battery();
    SCENE.detach();
    consume_deleted();
    st_wr!(ATTACHED, 0);
    st_wr!(MUTATING, 0);
    SCENE.unmounted()
}

unsafe fn original(index: usize, a: u32, b: u32, c: u32, d: u32) -> u32 {
    fw_api::background_invoke(fw_api::BACKGROUND_HOME_ORIGINALS[index], a, b, c, d)
}

/// Corona（或 v2，同一套引擎）正在接管场景时，旧引擎一律不许动这些根。
/// 暂停期（CORONA_HOLD）例外：那时 corona 已完整下架，旧引擎必须能干活。
unsafe fn corona_owns_scene() -> bool {
    st_rd!(CORONA_HOLD) == 0
        && (crate::corona_blur::active() || st_rd!(CORONA_BG_ACTIVE) != 0)
}

unsafe extern "C" fn home_signal(a: u32, b: u32, c: u32, d: u32) -> u32 {
    // corona 接管期间的 unmount 是旧引擎的破坏性动作（会 restore 旧台账、
    // 卸掉 background_pages 钩子），状态 1/2 下必须禁止 —— 交给 corona_tick 管。
    if a == fw_api::BACKGROUND_HOME_DESC && b == 0x12 && !corona_owns_scene() { unmount(); }
    let result = original(0, a, b, c, d);
    if a == fw_api::BACKGROUND_HOME_DESC && b == 0x12 { st_wr!(REQUEST, 1); }
    result
}

unsafe extern "C" fn home_create(a: u32, b: u32, c: u32, d: u32) -> u32 {
    if a == fw_api::BACKGROUND_HOME_DESC && !corona_owns_scene() { unmount(); }
    let result = original(1, a, b, c, d);
    if a == fw_api::BACKGROUND_HOME_DESC { st_wr!(REQUEST, 1); }
    result
}

unsafe extern "C" fn home_resume(a: u32, b: u32, c: u32, d: u32) -> u32 {
    let result = original(2, a, b, c, d);
    if a == fw_api::BACKGROUND_HOME_DESC { st_wr!(REQUEST, 1); }
    result
}

unsafe extern "C" fn home_delete(a: u32, b: u32, c: u32, d: u32) -> u32 {
    if a == fw_api::BACKGROUND_HOME_DESC && !corona_owns_scene() { unmount(); }
    let result = original(3, a, b, c, d);
    if a == fw_api::BACKGROUND_HOME_DESC { st_wr!(REQUEST, 1); }
    result
}

pub(crate) unsafe fn request(mode: u32) {
    // 档位只有 0(系统背景) / 1(复用静态背景) / 3(完全透明)。
    // 原来的 2(固定图片背景)已删: 那条链只读 /data/chaos/background.bin,
    // 而全项目没有任何一处写这个文件, 选中它永远只会得到"背景不可用"。
    if mode != 0 && mode != 1 && mode != 3 { return; }
    st_wr!(MODE, mode);
    if mode == 1 { st_wr!(GENERATE, 1); }
    st_wr!(REQUEST, 1);
    st_wr!(DIRTY, 1);
    persist_mode(mode);
}

/// 手动更新只登记请求，返回表盘后由已有 UI 调度捕获一次。
pub(crate) unsafe fn generate() {
    st_wr!(MODE, 1);
    st_wr!(GENERATE, 1);
    st_wr!(REQUEST, 1);
    st_wr!(DIRTY, 1);
}

pub(crate) unsafe fn mode() -> u32 { st_rd!(MODE) }

/// 调试页只读快照：(接管态, 已生效区域掩码, 暂停拍数, 失败计数, 丢所有权计数)。
pub(crate) unsafe fn corona_debug() -> (u32, u32, u32, u32, u32) {
    (st_rd!(CORONA_BG_ACTIVE), st_rd!(CORONA_BG_MASK),
     st_rd!(CORONA_HOLD), st_rd!(CORONA_FAIL), st_rd!(CORONA_OWN_LOST))
}

/// 按模糊区域掩码过滤目标面: 关闭的区域槽位置 0 —— SCENE/enable 全链都把
/// parent==0 当"跳过", 于是该区域保持官方模糊画面(见 blur_zone.rs 槽位表)。
/// 刷新快照的调用点**必须用全量 roots**(refresh_dirty_snapshots 内部会拿
/// background_home_targets() 原样比对, 掩码后的数组比对不上会静默失效)。
unsafe fn masked_targets(full: [u32; 4]) -> [u32; 4] {
    [if crate::blur_zone::on(0) { full[0] } else { 0 },
     if crate::blur_zone::on(1) { full[1] } else { 0 },
     if crate::blur_zone::on(2) { full[2] } else { 0 },
     if crate::blur_zone::on(3) { full[3] } else { 0 }]
}

/// 模糊区域或引擎开关变化只登记请求；UI owner 在下一拍切换。
/// 模式 0 也登记，保证 Corona 从系统背景启用/关闭时能推进并刷新页面。
pub(crate) unsafe fn remount() {
    st_wr!(REQUEST, 1);
    st_wr!(DIRTY, 1);
}

// ===== 实时背景模糊(跟随表盘) =====

const RT_CFG: &[u8] = b"/data/chaos/bg_realtime.cfg\0";

/// once 加载开关(load_mode 的 once 路径里调, 重启后保持)
unsafe fn load_rt() {
    if RT_LOADED != 0 { return; }
    RT_LOADED = 1;
    let fd = fw_api::open(RT_CFG.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd >= 0 {
        let mut v = 0u8;
        if fw_api::read(fd, &mut v as *mut u8, 1) == 1 && v <= 1 { st_wr!(RT_ON, v as u32); }
        fw_api::close(fd);
    }
}

pub(crate) unsafe fn realtime_on() -> u32 { st_rd!(RT_ON) }

/// 翻开关并持久化; 打开时清基线 —— 翻开关本身不该立刻触发生成,
/// 而是等下一次表盘 identity 真的变化(或先校准基线)。
pub(crate) unsafe fn set_realtime(on: u32) {
    st_wr!(RT_ON, on);
    if on != 0 { st_wr!(RT_VALID, 0); }
    let v = on as u8;
    let fd = fw_api::open(RT_CFG.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd >= 0 { let _ = fw_api::write(fd, &v as *const u8, 1); fw_api::close(fd); }
    st_wr!(DIRTY, 1);
}

/// 表盘跟随检测: 只在静态背景(mode 1)且人在表盘上时看 identity。
/// identity 变了 → 登记生成请求, 由既有"返回表盘后生效"链路重生成。
/// 生成是分批的(表盘像素抓取 + 模糊), 一次触发只跑一轮, 不会连续烧 CPU。
unsafe fn realtime_follow() {
    if st_rd!(RT_ON) == 0 { return; }
    if fw_api::watchface_is_on() == 0 { return; }
    let id = fw_api::background_watchface_identity();
    if id == 0 { return; }
    if st_rd!(RT_VALID) == 0 {
        st_wr!(RT_VALID, 1);
        st_wr!(RT_WF, id);
        return;
    }
    if id != st_rd!(RT_WF) {
        st_wr!(RT_WF, id);
        st_wr!(GENERATE, 1);
        st_wr!(REQUEST, 1);
    }
}

pub(crate) unsafe fn status() -> &'static [u8] {
    // 降级期间说人话：这段时间是系统背景在生效，不是"正在切模糊"。
    if st_rd!(CORONA_HOLD) != 0 {
        return "模糊引擎已暂停，系统背景生效中\0".as_bytes();
    }
    if crate::corona_blur::active() {
        return if st_rd!(CORONA_BG_ACTIVE) == 1 {
            if st_rd!(CORONA_BG_MASK) != crate::blur_zone::mask() {
                "等待空闲后更新 Corona 区域\0".as_bytes()
            } else if st_rd!(CORONA_BG_MASK) == 0 {
                "模糊：未选择区域\0".as_bytes()
            } else if crate::corona_blur::v2_enabled() {
                "模糊v2模式\0".as_bytes()
            } else {
                "Corona模糊模式\0".as_bytes()
            }
        } else {
            "等待空闲后切换模糊\0".as_bytes()
        };
    }
    if st_rd!(CORONA_BG_ACTIVE) != 0 {
        return "正在安全退出 Corona\0".as_bytes();
    }
    if st_rd!(REQUEST) != 0 && st_rd!(MODE) != 0
        && fw_api::top_app_id() != fw_api::HOME_APP_ID {
        return "返回表盘后生效\0".as_bytes();
    }
    match st_rd!(STATUS) {
        1 => "正在生成背景\0".as_bytes(),
        2 => "已生成，复用静态背景\0".as_bytes(),
        6 => "完全透明背景（实验性）\0".as_bytes(),
        4 => "背景不可用，保留系统背景\0".as_bytes(),
        5 => "接口不匹配，保留系统背景\0".as_bytes(),
        _ => "系统背景\0".as_bytes(),
    }
}

unsafe fn set_status(value: u32) {
    if value != st_rd!(STATUS) { st_wr!(STATUS, value); st_wr!(DIRTY, 1); }
}

/// 记一次 corona 失败。到阈值就安排“先下架、再暂停”，返回 true 表示已安排。
unsafe fn corona_failure(step: u32) -> bool {
    let fails = st_rd!(CORONA_FAIL).saturating_add(step);
    if fails < CORONA_FAIL_LIMIT {
        st_wr!(CORONA_FAIL, fails);
        return false;
    }
    st_wr!(CORONA_FAIL, 0);
    if st_rd!(CORONA_BG_ACTIVE) == 1 {
        // 已接管：先走正常下架序列（commit(false) → poll 到干净），完成后进暂停。
        st_wr!(CORONA_BG_ACTIVE, 2);
        st_wr!(CORONA_HOLD_ARM, 1);
    } else {
        // 从来没接上：直接让旧引擎接管。
        st_wr!(CORONA_BG_ACTIVE, 0);
        st_wr!(CORONA_HOLD, CORONA_HOLD_TICKS);
    }
    st_wr!(DIRTY, 1);
    true
}

unsafe fn corona_tick() -> bool {
    // 暂停期：corona 已完整下架，这段时间完全交给旧引擎；到点再试一次。
    if st_rd!(CORONA_HOLD) != 0 {
        st_wr!(CORONA_HOLD, st_rd!(CORONA_HOLD) - 1);
        if st_rd!(CORONA_HOLD) != 0 { return false; }
    }
    let engine_active = crate::corona_blur::active();
    let v2_active = crate::corona_blur::v2_enabled();
    if engine_active {
        let region_mask = crate::blur_zone::mask();
        if st_rd!(CORONA_BG_ACTIVE) == 0 {
            if !fw_api::screen_is_on() { return true; }
            // 等 native 空闲期间也继续服务 Corona：滚动标记的自愈就在 poll 里，
            // 不 poll 的话“标记卡住 → 永远等不到空闲”会互相锁死。
            if !crate::corona_blur::backgrounds_idle() {
                crate::corona_blur::backgrounds_poll();
                return true;
            }
            if st_rd!(CORONA_COOLDOWN) != 0 {
                // 刚丢过所有权：歇几拍再重装，避免 commit/丢 高频抖动。
                st_wr!(CORONA_COOLDOWN, st_rd!(CORONA_COOLDOWN) - 1);
                crate::corona_blur::backgrounds_poll();
                return true;
            }
            if (st_rd!(ATTACHED) != 0 || !SCENE.unmounted() || crate::background_pages::hooked())
                && fw_api::background_home_targets().is_none() { return true; }
            if !unmount() { return true; }
            crate::background_surfaces::clear_background();
            if !SCENE.release() { return true; }
            st_wr!(ATTACHED, 0);
            st_wr!(SOURCE_MODE, 0);
            st_wr!(REFRESH, 0);
            uninstall();
        }
        if st_rd!(CORONA_BG_ACTIVE) != 1 || st_rd!(CORONA_BG_MASK) != region_mask {
            if crate::corona_blur::backgrounds_commit(true, region_mask) != 0 {
                // Keep servicing the applied configuration while a live mask
                // update or a quick off/on waits for native retirement.
                if st_rd!(CORONA_BG_ACTIVE) != 0 { crate::corona_blur::backgrounds_poll(); }
                let _ = corona_failure(1);
                return true;
            }
            st_wr!(CORONA_FAIL, 0);
            st_wr!(CORONA_BG_MASK, region_mask);
            st_wr!(CORONA_BG_ACTIVE, 1);
            st_wr!(DIRTY, 1);
        }
        // 所有权看门狗：原生 owner 可能被它自己的 DELETE/手势处理摘掉而不经
        // commit。继续认定 CORONA_BG_ACTIVE==1 就会"模糊不动、换什么档位都透明"。
        // 只在主表盘上判定（前台应用的过渡里 owner 本来就可能为空）。
        if fw_api::top_app_id() == fw_api::HOME_APP_ID
            && !crate::corona_blur::backgrounds_owned() {
            let lost = st_rd!(CORONA_OWN_LOST) + 1;
            st_wr!(CORONA_OWN_LOST, lost);
            if lost >= CORONA_OWN_LOST_LIMIT {
                st_wr!(CORONA_OWN_LOST, 0);
                st_wr!(CORONA_BG_ACTIVE, 0);
                st_wr!(CORONA_COOLDOWN, CORONA_RETRY_TICKS);
                st_wr!(DIRTY, 1);
                crate::corona_blur::backgrounds_poll();
                return true;
            }
        } else {
            st_wr!(CORONA_OWN_LOST, 0);
        }
        // v2 adds only the old no-message image replacement. Never restart
        // background_pages/background_surfaces while Corona owns the scene.
        // Use the successfully applied mask, not a still-busy desired mask.
        let icon_wanted = v2_active && st_rd!(CORONA_BG_MASK) & 8 != 0;
        if (icon_wanted || crate::notification_icon::pending())
            && fw_api::screen_is_on() && crate::corona_blur::backgrounds_idle() {
            let _ = crate::notification_icon::tick(icon_wanted);
        }
        // The vendored Corona adapter owns selected regions' drawing/scroll/
        // delete lifecycles and restores system backgrounds for deselected
        // ones. Even mask 0 must not fall through to a legacy background mode.
        crate::corona_blur::backgrounds_poll();
        return true;
    }

    // Restore v2-owned icons before any handoff to the legacy engine. Missing
    // native idle keeps both providers polled; it must not drop cleanup work.
    if crate::notification_icon::pending() {
        if !fw_api::screen_is_on() || !crate::corona_blur::backgrounds_idle()
            || !crate::notification_icon::tick(false) {
            if st_rd!(CORONA_BG_ACTIVE) != 0 { crate::corona_blur::backgrounds_poll(); }
            return true;
        }
    }
    if st_rd!(CORONA_BG_ACTIVE) == 1 {
        // A busy commit changes nothing. Keep servicing the installed hooks
        // until the native swipe ends; never reset/retire the provider alone.
        if crate::corona_blur::backgrounds_commit(false, 0) != 0 {
            crate::corona_blur::backgrounds_poll();
            return true;
        }
        st_wr!(CORONA_BG_ACTIVE, 2);
        st_wr!(DIRTY, 1);
    }
    if st_rd!(CORONA_BG_ACTIVE) == 2 {
        // Off is not a one-shot watch(0): retain owner polling until display
        // observers, GPU-retired images and battery buffers are all released.
        crate::corona_blur::backgrounds_poll();
        if crate::corona_blur::backgrounds_pending() { return true; }
        st_wr!(CORONA_BG_ACTIVE, 0);
        st_wr!(CORONA_BG_MASK, 0);
        st_wr!(REQUEST, 1);
        st_wr!(GENERATE, if st_rd!(MODE) == 1 { 1 } else { 0 });
        st_wr!(REFRESH, 0);
        set_status(0);
        st_wr!(DIRTY, 1);
        // 降级：这次下架是“放弃重试”触发的，旧引擎接管一段后再自动重试。
        if st_rd!(CORONA_HOLD_ARM) != 0 {
            st_wr!(CORONA_HOLD_ARM, 0);
            st_wr!(CORONA_HOLD, CORONA_HOLD_TICKS);
        }
    }
    false
}

pub(crate) unsafe fn pending() -> bool {
    if st_rd!(BOOT_DELAY) != 0 { return true; }
    for i in 0..4 {
        if st_rd!(DELETED[i]).1 != 0 { return true; }
    }
    // Sleeping displays retain Corona's cleanup state for the next wake;
    // they must not force the existing 1s idle timer back to a 50ms spin.
    fw_api::screen_is_on() && (crate::corona_blur::active() || st_rd!(CORONA_BG_ACTIVE) != 0
        || crate::notification_icon::pending() || st_rd!(REQUEST) != 0 || SCENE.pending()
        || st_rd!(REFRESH) != 0 || crate::background_surfaces::pending())
}

pub(crate) unsafe fn tick() {
    if st_rd!(BOOT_DELAY) != 0 {
        st_wr!(BOOT_DELAY, st_rd!(BOOT_DELAY) - 1);
        return;
    }
    load_mode();
    consume_deleted();
    vib_tick();   // 滑动震动 + 震动测试
    if !crate::font_apply::quiet() { return; }
    // 档位或状态一变就排一次整页重建: 回调里(事件派发链上)只登记请求, 控件由这里
    // 在安静拍上重建 —— 与"更换字体""桌面图标"两页同一套做法。
    if st_rd!(DIRTY) != 0 {
        st_wr!(DIRTY, 0);
        if crate::page_is_live(BLUR_PID as usize) { crate::render_req(BLUR_PID); }
        if crate::page_is_live(crate::page::BLUR_LEGACY_PID as usize) {
            crate::render_req(crate::page::BLUR_LEGACY_PID);
        }
    }
    // Give Corona exclusive ownership before ANY legacy surface worker,
    // snapshot refresh or scene attachment. UI-only redraws above still run.
    if corona_tick() { return; }
    if fw_api::screen_is_on() && crate::background_surfaces::pending() {
        let Some(full) = fw_api::background_home_targets() else { return; };
        let changed = crate::background_surfaces::tick(Some(masked_targets(full)));
        if changed != 0 && st_rd!(ATTACHED) != 0 {
            st_wr!(REFRESH, st_rd!(REFRESH) | changed);
        }
    }
    let mode = st_rd!(MODE);
    if mode == 0 {
        if st_rd!(REQUEST) != 0 && unmount() {
            if let Some(roots) = fw_api::background_home_targets() {
                fw_api::background_home_refresh_snapshots(roots);
            }
            st_wr!(REFRESH, 0);
            // 已生成的静态图保留，重新打开开关仍使用同一张。
            if SCENE.pending() {
                if !SCENE.release() { return; }
                st_wr!(SOURCE_MODE, 0);
            }
            st_wr!(REQUEST, 0);
            st_wr!(GENERATE, 0);
            uninstall();
            set_status(0);
        }
        return;
    }
    if !install() { st_wr!(REQUEST, 0); set_status(5); return; }
    if !fw_api::screen_is_on() || fw_api::top_app_id() != fw_api::HOME_APP_ID { return; }
    let Some(full) = fw_api::background_home_targets() else { return; };
    // parents = 掩码后的数组(SCENE/enable/transparent 全链用); full 只喂给
    // refresh_dirty_snapshots(内部按全量 targets 原样比对, 见 masked_targets 注释)。
    let parents = masked_targets(full);
    if !watch_structure() { set_status(4); return; }
    if mode == 3 {
        if st_rd!(REQUEST) != 0 {
            if !unmount() { return; }
            st_wr!(REQUEST, 0);
            crate::background_surfaces::enable_transparent(parents);
            // enable_transparent 必须先看到当前模糊背景，才能为电池
            // 生成与模糊模式完全相同的局部 patch；patch 挂载完成后
            // 再清除背景描述符，页面其余区域仍保持完全透明。
            crate::background_surfaces::set_background(0);
            st_wr!(ATTACHED, 1);
            set_status(6);
        }
        if crate::background_surfaces::pending() { let _ = crate::background_surfaces::tick(Some(parents)); }
        return;
    }
    // 实时背景模糊: 必须在算 generate **之前**看表盘有没有换 —— GENERATE 置位
    // 晚一拍的话本 tick 会走"重挂旧图"分支, 新表盘的模糊永远出不来。
    realtime_follow();
    let generate = st_rd!(GENERATE) != 0 || st_rd!(SOURCE_MODE) != mode
        || SCENE.descriptor() == 0;
    // 只有生成需要表盘可见；已有静态图在其他主界面页面也可立即重挂。
    if generate && fw_api::watchface_is_on() == 0 { return; }
    if st_rd!(REQUEST) != 0 {
        if !generate && st_rd!(ATTACHED) != 0 && SCENE.matches(parents) {
            st_wr!(REQUEST, 0);
        } else {
            if !unmount() { return; }
            st_wr!(REQUEST, 0);
            if generate {
                if !SCENE.release() { return; }
                st_wr!(GENERATE, 0);
                st_wr!(SOURCE_MODE, mode);
                let loaded = SCENE.capture();
                if !loaded { set_status(4); return; }
                set_status(1);
            }
        }
    }
    if SCENE.pending() && !SCENE.step() { return; }
    if st_rd!(ATTACHED) == 0 && SCENE.descriptor() != 0 {
        st_wr!(MUTATING, 1);
        let attached = SCENE.attach(parents, image_deleted as *const () as u32);
        st_wr!(MUTATING, 0);
        consume_deleted();
        if attached {
            crate::background_surfaces::set_background(SCENE.descriptor());
            crate::background_surfaces::enable(parents);
            st_wr!(ATTACHED, 1);
            st_wr!(REFRESH, 0xE);
            set_status(2);
        } else {
            unmount();
            set_status(4);
        }
    }
    if st_rd!(REFRESH) != 0 && st_rd!(ATTACHED) != 0
        && !crate::background_surfaces::pending() {
        fw_api::background_home_refresh_dirty_snapshots(full, st_rd!(REFRESH));
        st_wr!(REFRESH, 0);
    }
}
