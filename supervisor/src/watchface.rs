// 表盘列表 / 选择 / 摇一摇切表盘

use crate::*;
use crate::mem::{rd16, rd32, rd8};

// 表盘名: 内联 name(+0x48) 为空时回退 name_translation
// (+0xB4=条数, +0xB8=数组; 每条 8B: [0]=language(u8), [4]=char* 字符串) 0xCA7E2DA/0xCA7E3B2 静态核对
// 系统内置表盘在 watchface_list.json 里没有内联 name, 名字只在 name_translation
pub(crate) unsafe fn wf_copy_name(node: u32, dst: *mut u8) {
    let nmp = (node + 0x48) as *const u8;
    let mut k = 0usize;
    while k < 63 {
        let c = read_volatile(nmp.add(k));
        if c == 0 { break; }
        write_volatile(dst.add(k), c);
        k += 1;
    }
    if k > 0 { write_volatile(dst.add(k), 0u8); return; }
    let cnt = rd32((node + 0xB4) as *const u32) as usize;
    let arr = rd32((node + 0xB8) as *const u32);
    if cnt == 0 || arr == 0 { write_volatile(dst.add(0), 0u8); return; }
    let zh = st_rd!(LANG) != 0;
    let mut best: u32 = 0;
    let mut best_sc: u32 = 0;
    let mut i = 0usize;
    while i < cnt && i < 16 {
        let tp = rd32((arr + (i as u32) * 8 + 4) as *const u32);
        if tp != 0 {
            let mut sc = 1u32;
            if zh {
                let p = tp as *const u8;
                let mut q = 0usize;
                while q < 32 {
                    let c = read_volatile(p.add(q));
                    if c == 0 { break; }
                    if c >= 0x80 { sc = 2; break; }
                    q += 1;
                }
            }
            if sc > best_sc { best_sc = sc; best = tp; }
        }
        i += 1;
    }
    if best == 0 { write_volatile(dst.add(0), 0u8); return; }
    let bp = best as *const u8;
    k = 0;
    while k < 63 {
        let c = read_volatile(bp.add(k));
        if c == 0 { break; }
        write_volatile(dst.add(k), c);
        k += 1;
    }
    write_volatile(dst.add(k), 0u8);
}

// 表盘枚举(下面的 load_watchfaces): 两遍链表, 先取引擎当前面(*0x20119770)的 type 当主表盘类型
// (自校准), 第二遍收市场表盘(type 1) 与按开关纳入的内置表盘(type 0), AOD 息屏面/deco 被滤掉。
// 节点字段偏移(序列化器 0xCA7E59C 的键 ↔ 偏移核对):
// id@+8 name@+0x48 type@+0x88 version@+0x8C in_use@+0x90 is_delete@+0x91 support_AOD@+0x97
// (WF_STATE 存 type, WF_PEND 存 in_use)
pub(crate) unsafe fn desel_has(node: u32) -> bool {
    let a = core::ptr::addr_of!(WF_DESEL) as *const u32;
    let n = st_rd!(WF_DESEL_CNT) as usize;
    let mut i = 0usize;
    while i < n { if read_volatile(a.add(i)) == node { return true; } i += 1; }
    false
}

pub(crate) unsafe fn desel_set(node: u32, off: bool) {
    let a = core::ptr::addr_of_mut!(WF_DESEL) as *mut u32;
    let mut n = st_rd!(WF_DESEL_CNT) as usize;
    let mut i = 0usize;
    let mut found = usize::MAX;
    while i < n { if read_volatile(a.add(i)) == node { found = i; break; } i += 1; }
    if off {
        if found == usize::MAX && n < 24 { write_volatile(a.add(n), node); n += 1; }
    } else if found != usize::MAX {
        while found + 1 < n {
            write_volatile(a.add(found), read_volatile(a.add(found + 1)));
            found += 1;
        }
        n -= 1;
    }
    st_wr!(WF_DESEL_CNT, n as u32);
}

// show_builtin: 本页列表是否包含内置表盘(type 0) 两页各自的开关独立控制
pub(crate) unsafe fn load_watchfaces(show_builtin: u32) {
    st_wr!(WF_COUNT, 0);
    st_wr!(WF_WIN, 0);
    st_wr!(WF_ERR, 0);
    st_wr!(WF_TOTAL, 0);
    let mgr = rd32(WF_MGR_PTR as *const u32);
    if mgr == 0 || (mgr & 0xE000_0000) != 0x2000_0000 {
        st_wr!(WF_ERR, 1);
        return;
    }
    let c1 = rd32((mgr + 0x48) as *const u32);
    let c2 = rd32((mgr + 0x4C) as *const u32);
    st_wr!(WF_CUR1, c1);
    st_wr!(WF_CUR2, c2);
    let c0 = rd32(WF_ENGINE_CUR as *const u32);
    st_wr!(WF_CUR0, c0);
    let sentinel = mgr + 0x70;
    // pass1: 主表盘 type = 引擎当前面节点的 type
    let mut main_type: i32 = -1;
    let mut node = rd32((mgr + 0x74) as *const u32);
    let mut iter = 0usize;
    while node != 0 && node != sentinel && iter < 200 {
        iter += 1;
        if (node & 0xE000_0000) != 0x2000_0000 { break; }
        if c0 != 0 && node == c0 {
            main_type = rd8((node + 0x88) as *const u8) as i32;
            break;
        }
        node = rd32((node + 4) as *const u32);
    }
    st_wr!(WF_MAINTYPE, main_type);
    // pass2: 过滤收集
    let ids = core::ptr::addr_of_mut!(WF_IDS) as *mut u8;
    let nms = core::ptr::addr_of_mut!(WF_NAMES) as *mut u8;
    let sts = core::ptr::addr_of_mut!(WF_STATE) as *mut u8;   // = type
    let pds = core::ptr::addr_of_mut!(WF_PEND) as *mut u8;     // = in_use
    let nds = core::ptr::addr_of_mut!(WF_NODES) as *mut u32;
    let mut cnt = 0usize;
    let mut total = 0usize;
    node = rd32((mgr + 0x74) as *const u32);
    iter = 0;
    while node != 0 && node != sentinel && iter < 200 {
        iter += 1;
        if (node & 0xE000_0000) != 0x2000_0000 { break; }
        total += 1;
        let ty = rd8((node + 0x88) as *const u8);
        // type 取值(0xCA7CAA2 用 [node+0x88] 索引 0x2013FE4C 的目录数组):
        // 0=/data/app/watchface/builtin/(内置) 1=.../market/(市场) 2=.../aod/(息屏)
        // 固件找"当前面"时跳过 type==2; 切面核心拒 type==3。
        // 主表盘 = type 0 + 1。注意判据不能写成"与当前面同 type", 那样内置表盘会被全滤掉。
        // 内置表盘(type 0)是否纳入本页列表 = 调用方传入的开关(两页独立)
        if (ty == 1 || (ty == 0 && show_builtin != 0)) && cnt < 24 {
            let idp = (node + 8) as *const u8;
            let dst = ids.add(cnt * 16);
            let mut k = 0usize;
            while k < 15 {
                let c = read_volatile(idp.add(k));
                if c == 0 { break; }
                write_volatile(dst.add(k), c);
                k += 1;
            }
            write_volatile(dst.add(k), 0u8);
            // 名称: 内联 +0x48 为空时回退 name_translation(内置表盘属于这种)
            let ndst = nms.add(cnt * 64);
            wf_copy_name(node, ndst);
            write_volatile(sts.add(cnt), ty);
            write_volatile(pds.add(cnt), rd8((node + 0x90) as *const u8));
            write_volatile(nds.add(cnt), node);
            cnt += 1;
        }
        node = rd32((node + 4) as *const u32);
    }
    st_wr!(WF_TOTAL, total as u32);
    st_wr!(WF_COUNT, cnt as u32);
}

// 表盘页行文本: 行0="显示内置表盘"开关, 行1-8=窗内条目(主标签=名称, 副标签=ID), 行9="更多", 行10="返回"
pub(crate) unsafe fn refresh_watchface_lines() {
    // 双排: 主标签 = 表盘名称 / 副标签 = 表盘 ID
    let base = core::ptr::addr_of_mut!(WATCH_LINES) as *mut u8;
    let sub = core::ptr::addr_of_mut!(WATCH_SUB) as *mut u8;
    let mut i = 0usize;
    while i < 11 * 88 { write_volatile(base.add(i), 0); write_volatile(sub.add(i), 0); i += 1; }
    let cnt = st_rd!(WF_COUNT) as usize;
    let err = st_rd!(WF_ERR);
    let wins = if cnt == 0 { 1usize } else { (cnt + 7) / 8 };
    let mut win = st_rd!(WF_WIN) as usize;
    if win >= wins { win = 0; st_wr!(WF_WIN, 0); }
    if err != 0 {
        let mut x = W::new(base, 88);
        x.s("管理器未就绪".as_bytes());
        x.end();
    } else if cnt == 0 {
        let mut x = W::new(base, 88);
        x.s("无表盘".as_bytes());
        x.end();
    }
    let ids = core::ptr::addr_of!(WF_IDS) as *const u8;
    let nms = core::ptr::addr_of!(WF_NAMES) as *const u8;
    let mut r = 0usize;
    while r < 8 {
        let e = win * 8 + r;
        // 条目行下移到 slot1..8(slot0 留给"显示内置表盘" switch)
        let dp = base.add((r + 1) * 88);      // 主标签(名称)
        let sp = sub.add((r + 1) * 88);       // 副标签(ID)
        if e < cnt {
            let nm = nms.add(e * 64);
            // 主标签 = 名称(读不出用"未命名表盘"占位), 副标签 = ID
            let mut x = W::new(dp, 88);
            let mut k = 0usize;
            while k < 60 {
                let c = read_volatile(nm.add(k));
                if c == 0 { break; }
                x.c(c);
                k += 1;
            }
            if k == 0 { x.s("未命名表盘".as_bytes()); }
            x.end();
            let src = ids.add(e * 16);
            let mut x = W::new(sp, 88);
            let mut k = 0usize;
            while k < 15 {
                let c = read_volatile(src.add(k));
                if c == 0 { break; }
                x.c(c);
                k += 1;
            }
            x.end();
        } else {
            write_volatile(dp.add(0), 0u8);   // 不用 '-' 占位(该行已隐藏)
        }
        r += 1;
    }
    // slot0 = "显示内置表盘" switch(第1行)
    let mut x = W::new(base, 88);
    x.s("显示内置表盘".as_bytes());
    x.end();
    // slot9 = 更多, slot10 = 返回
    let mut x = W::new(base.add(9 * 88), 88);
    if (win + 1) * 8 < cnt { x.s("更多".as_bytes()); }
    x.end();
    let mut x = W::new(base.add(10 * 88), 88);
    x.s("返回".as_bytes());
    x.end();
}

// 表盘页勾选态"原地"更新: 只调 row_update 的第 6 参(非 0 → set_state(trailing,3) 打勾),
// 不销毁/重建任何对象; 用于首次建行后补写勾选、切面后把勾选挪到新表盘。
// 注意: row_update 不止改文字(标签已存在时照样 set_text + 给 trailing 做 set_state, 后者连带
// 子树样式失效), 逐字说明见 font_apply::apply_finish; 调用方必须先过 built + page_is_live 两道门。
pub(crate) unsafe fn wf_apply_selection(pid: usize) {
    let upd: unsafe extern "C" fn(u32, *const u8, *const u8, *const u8, i32, u8) -> i32 = fw_api::row_update;
    let rows = page_rows_ptr(pid);
    let win = st_rd!(WF_WIN) as usize;
    let cnt = st_rd!(WF_COUNT) as usize;
    let c0 = st_rd!(WF_CUR0);
    let nds = core::ptr::addr_of!(WF_NODES) as *const u32;
    let pl = core::ptr::addr_of!(WATCH_LINES) as *const u8;
    let ps = core::ptr::addr_of!(WATCH_SUB) as *const u8;
    let mut i = 0usize;
    while i < 8 {
        let op = read_volatile(rows.add(i + 1));   // 条目在 slot1..8
        if op != 0 {
            let mut sel = 0u8;
            let e = win * 8 + i;
            if e < cnt {
                if pid == 8 {
                    if c0 != 0 && read_volatile(nds.add(e)) == c0 { sel = 1; }   // 勾选=引擎当前面
                } else {
                    // 轮换页: 勾选 = 该节点未被取消参与
                    let nd = read_volatile(nds.add(e));
                    if !desel_has(nd) { sel = 1; }
                }
            }
            upd(op, core::ptr::null(), pl.add((i + 1) * 88), ps.add((i + 1) * 88), 0, sel);
        }
        i += 1;
    }
    // slot0 = "显示内置表盘" switch: 初态 (表盘页看 WF_SHOW_BUILTIN / 轮换页看 WF_SEL_BUILTIN)
    let r0 = read_volatile(rows.add(0));
    if r0 != 0 {
        let v = if pid == 8 {
            st_rd!(WF_SHOW_BUILTIN)
        } else {
            st_rd!(WF_SEL_BUILTIN)
        };
        upd(r0, core::ptr::null(), pl.add(0), core::ptr::null(), 0,
            if v != 0 { 1u8 } else { 0u8 });
    }
}

// ===== 摇一摇切表盘(quick_guesture eventbus + lv_timer UI线程延迟执行) =====
// eventbus 回调跑在派发线程: 只置旗标/计数, 重活全部交给 lv_timer(UI 线程) —— 不在派发线程上动对象树
#[no_mangle]
pub(crate) unsafe extern "C" fn chaos_shake_cb(event: u32, _udata: u32) -> u32 {
    if event == 0 { return 0; }
    // 手势码 = payload+8 u16 (event[+0x18]=payload 指针, 与固件自带 system handler 的读法一致)
    let pay = rd32((event + 0x18) as *const u32);
    if pay == 0 || (pay & 0xE000_0000) != 0x2000_0000 { return 0; }
    let code = rd16((pay + 8) as *const u16) as u32;
    if code != GC_SHAKE {
        // 抬腕亮屏(0x1C3)与其它码: 直接丢弃 不置旗标, 不切表盘, 不上冷却
        return 0;
    }
    if st_rd!(SHAKE_EN) == 0 { return 0; }
    // 三条门控(都已实测):
    //   1. 屏幕亮着  —— 挡住息屏与 AOD
    //   2. 最上层页属 home 应用 —— 挡住设置(22)与其它应用(实测: 表盘/launcher 均 top=16:0)
    //   3. 固件的"表盘在显示"标志非 0 —— 挡住 launcher(实测: 表盘页 on=1, launcher on=0)
    if !fw_api::screen_is_on() || fw_api::top_app_id() != fw_api::HOME_APP_ID { return 0; }
    if fw_api::watchface_is_on() == 0 { return 0; }
    st_wr!(SHAKE_PENDING, 1);
    0
}

// lv_timer(UI线程): 框架级维护 + 页面 tick 分发
//
// 节拍自适应:
//   **亮屏期间一律 50ms** —— 亮屏就是有人在操作, 手感不能退(50ms 是为压末段延迟从 200ms 收窄来的);
//   **息屏且完全空闲 = 1000ms**。手表绝大部分时间息屏, 恒定的 20 拍/秒里绝大多数拍只是
//   读几个静态量就早退, 但它是永不停脚的唤醒源; 加上信息页那 0.5 秒一次的 /proc 读与重绘
//   (息屏时同样不需要), 这两项就是息屏功耗里本模块贡献的那一份。
//   "完全空闲" = 下面 tick_busy() 里那些旗标全为 0 —— 任何倒计时/跑批/待重建/提示挂着
//   都算有活, 所以**所有以"拍"为单位的等待量都还在按 50ms 走, 不会因为降频被拉长**。
const TICK_FAST_MS: u32 = 50;
const TICK_IDLE_MS: u32 = 1000;

/// 有没有活在推进(或有人在用)。为 true 就保持快节拍。
unsafe fn tick_busy() -> bool {
    fw_api::screen_is_on()                        // 亮屏 = 随时可能有操作, 不降频
    || st_rd!(RENDER_REQ) != RENDER_NONE          // 有一次重建排队
    || st_rd!(SHAKE_COOL) != 0                    // 摇一摇冷却按拍计
    || st_rd!(HEAL_TRYS) != 0                     // 自愈重试的痕迹: 保持快节拍到本轮结束
    || font_apply::pending()
    || font_tree::backfill_running()
    || icon_apply::pending()
    || font_list::pending()
    || confirm_pop::pending()
}

/// 按忙闲改本定时器的周期(只在句柄验过之后才动; 变了才写, 不每拍写)。
unsafe fn tick_adapt() {
    if st_rd!(TICK_ADAPT) == 0 { return; }
    let want = if tick_busy() { TICK_FAST_MS } else { TICK_IDLE_MS };
    if want == st_rd!(TICK_PERIOD_MS) { return; }
    let t = st_rd!(SHAKE_TIMER);
    fw_api::timer_set_period(t, want);
    // 写完读回核对: 读不回来我们要的值, 说明这个句柄不是**这条 LVGL 定时器链表**上的
    // 对象(建它的是 0x0C587ED1 那层 veneer, 它落到哪一份实现未经实测确认)。
    // 那种情况下**永久放弃自适应**, 也不再写第二次 —— 最坏退回恒 50ms 的原节拍。
    if fw_api::timer_period_of(t) != want {
        st_wr!(TICK_ADAPT, 0);
        st_wr!(TICK_PERIOD_MS, TICK_FAST_MS);
        return;
    }
    st_wr!(TICK_PERIOD_MS, want);
}

#[no_mangle]
pub(crate) unsafe extern "C" fn chaos_shake_timer(_t: u32) {
    // 0. 先按上一拍的忙闲把节拍定好(放在最前: 后面的分支里有提前 return)
    tick_adapt();
    // 1. 自愈: 页句柄还在但内容树没建(built == 0, 而 on_resume 没来, 例如息屏唤醒)。
    //    **只在亮屏时做**: 息屏时把它重建起来等于在关闭的屏幕上凭空画一整页,
    //    而且信息页会因此继续 0.5 秒读写一遍 —— 纯粹多耗功耗。
    {
        let fgp = st_rd!(FG_PAGE) as usize;
        if fgp <= MAX_PID
            && fw_api::screen_is_on()
            && PAGES[fgp].root != 0
            && PAGES[fgp].built == 0
            && page_is_live(fgp)
            && st_rd!(HEAL_TRYS) < 3
            && render_page(fgp) != 0
        {
            st_wr!(HEAL_TRYS, st_rd!(HEAL_TRYS).wrapping_add(1));
        }
    }
    // 2. 页面 tick 分发: 只调前台页(存活门在各页 tick 内部再查一次)
    {
        let fgp = st_rd!(FG_PAGE) as usize;
        if fgp <= MAX_PID && PAGES[fgp].built != 0 && page_is_live(fgp) {
            let page_ref = PAGE_TABLE[fgp];
            if page_ref.needs_tick() {
                page_ref.tick();
            }
        }
    }
    // 3. 消费重建请求(lv_timer 上下文销毁/重建对象树是安全的)
    let rq = st_rd!(RENDER_REQ);
    if rq != RENDER_NONE {
        st_wr!(RENDER_REQ, RENDER_NONE);
        if page_is_live(rq as usize) { render_page(rq as usize); }
    }
    // 4. 字体: 先消费样式级写回请求(含开机自动应用), 再跑逐对象补写。
    //    放定时器而不是页面渲染路径, 是因为**目标页不是我们的页** —— 系统页
    //    没有任何我们能挂的渲染钩子(生命周期槽位只属于我们注册的页, eventbus 也
    //    没有"页面切换"主题)。逐对象补写那条的风险由三条消解: 句柄不缓存(每轮现取
    //    页根)、指针全过门、息屏直接跳过。详见 font_tree.rs 头部纪律。
    font_apply::tick();
    font_tree::tick();
    icon_apply::tick();
    // 字体清单页(更换字体)的删除流水线 + 提示超时。与上面几项跑批同一个位置:
    // 快节拍 50ms 一拍, 息屏空闲时降到 1000ms(页面不在前台时 render_req 也只是置位, 由存活门挡住)。
    font_list::tick();
    // 5. 摇一摇: 冷却 + 消费旗标 + 切表盘
    let cool = st_rd!(SHAKE_COOL);
    if cool > 0 {
        st_wr!(SHAKE_COOL, cool - 1);
        return;
    }
    if st_rd!(SHAKE_PENDING) == 0 { return; }
    st_wr!(SHAKE_PENDING, 0);
    if st_rd!(SHAKE_EN) == 0 { return; }
    // 保住表盘页窗位: load_watchfaces() 会把 WF_WIN 重置为 0; 若当时正停在
    // 表盘页的"更多"屏, 行下标会与窗位错位(点错表盘) → 先存后还原
    let win_save = st_rd!(WF_WIN);
    // 若正停在列表页则不改动其列表(否则会与该页的过滤不一致); 否则用全量(含内置)以便定位当前面
    let fg_now = st_rd!(FG_PAGE);
    if fg_now != 8 && fg_now != 9 {
        load_watchfaces(1);
    }
    st_wr!(WF_WIN, win_save);
    let cnt = st_rd!(WF_COUNT) as usize;
    if cnt < 2 { return; }
    let c0 = st_rd!(WF_CUR0);
    let nds = core::ptr::addr_of!(WF_NODES) as *const u32;
    let mut cur = 0usize;
    let mut found = false;
    let mut i = 0usize;
    while i < cnt {
        if read_volatile(nds.add(i)) == c0 { cur = i; found = true; break; }
        i += 1;
    }
    if !found { cur = 0; }
    // 参与者 = 未被取消参与(WF_DESEL) 且 (市场表盘 或 内置表盘开关为开)
    let tps2 = core::ptr::addr_of!(WF_STATE) as *const u8;
    let builtin_on = st_rd!(WF_SEL_BUILTIN) != 0;
    let mut nx = cur;
    let mut ok = false;
    let mut k = 1usize;
    while k <= cnt {
        let j = (cur + k) % cnt;
        let nd = read_volatile(nds.add(j));
        let ty = read_volatile(tps2.add(j));
        if (ty != 0 || builtin_on) && !desel_has(nd) { nx = j; ok = true; break; }
        k += 1;
    }
    if !ok {
        st_wr!(SHAKE_COOL, 20);
        return;
    }
    let sw: unsafe extern "C" fn(*const u8) -> i32 = core::mem::transmute(FW_SET_WATCHFACE as usize);
    let rc = sw((core::ptr::addr_of!(WF_IDS) as *const u8).add(nx * 16));
    st_wr!(WF_SW_RC, rc);
    if rc == 0 {
        let rf: unsafe extern "C" fn(u32, u32) -> i32 = core::mem::transmute(FW_REFRESH_CUR_WF as usize);
        let node = read_volatile(nds.add(nx));
        let rfc = rf(1, node);
        st_wr!(WF_RF_RC, rfc);
    }
    // 若此刻正停在表盘页(8)或轮换页(9), 原地刷新勾选(不重建整页)
    let fg = st_rd!(FG_PAGE) as usize;
    if fg == 8 || fg == 9 {
        if PAGES[fg].built != 0 && page_is_live(fg) {
            wf_apply_selection(fg);
        }
    }
    st_wr!(SHAKE_COOL, 20);   // 20 拍冷却(快节拍 50ms 时约 1s)防连切
}

// 武装(首次 on_resume = UI线程): 订阅 quick_guesture + 建 lv_timer; 常驻到重启(模块不卸载)
//
// 重要: 句柄一旦建起来就**永不丢弃**。"疑似定时器已随息屏销毁"的判断是错的 ——
// 实测那个定时器一直活着。而**只清句柄不等于删除对象**: 它仍挂在 LVGL 定时器链表里继续跑,
// 每清零重建一次就多一个并发实例(表现为 CPU 数值刷新越来越快, 经历的息屏轮次越多越快)。
// 要做"先删后建"必须先确认 lv_timer_delete 的地址, 在那之前只建一次。
pub(crate) unsafe fn shake_arm() {
    if st_rd!(SHAKE_SUB) == 0 {
        let cb: unsafe extern "C" fn(u32, u32) -> u32 = chaos_shake_cb;
        let h = fw_api::eventbus_subscribe(fw_api::TOPIC_QUICK_GUESTURE, 0, cb as u32, 0);
        st_wr!(SHAKE_SUB, h);
    }
    if st_rd!(SHAKE_TIMER) == 0 {
        let tm: unsafe extern "C" fn(u32) = chaos_shake_timer;
        let t = fw_api::timer_create(tm as u32, TICK_FAST_MS, 0);
        st_wr!(SHAKE_TIMER, t);
        st_wr!(TIMER_CREATE_N, st_rd!(TIMER_CREATE_N).wrapping_add(1));
        // 允许自适应之前先验句柄: create 返回的就是 lv_timer_t*, 周期在 +0x00,
        // 读回来必须正好是我们刚传的 50。读不上来就永远不改(保持恒 50ms),
        // 绝不往一个没验证的指针上写。
        st_wr!(TICK_PERIOD_MS, TICK_FAST_MS);
        st_wr!(TICK_ADAPT, if fw_api::timer_period_of(t) == TICK_FAST_MS { 1 } else { 0 });
    }
}

// 信息页行3 开关文本
pub(crate) unsafe fn shake_toggle_text(dp: *mut u8) {
    // 开/关状态由行右侧 switch 组件表达, 行内只留名称
    let mut x = W::new(dp, 88);
    x.s("摇一摇".as_bytes());
    x.end();
}

