//! 调试信息汇总（整屏纯文本）。
//!
//! 入口：主页 → 关于（页18）连点 5 次进入调试模式 → “③ 调试信息”。
//! 以后任何"需要肉眼确认裸数值"的测试都往 [`refresh`] 里加一行，页面不用动。
//!
//! 这里只做只读采集：不写固件结构、不改配置，放在调试页里给人看。

use crate::*;

/// 整页文本缓冲。按 24 行 × 48 字节估算，超出会被 W 截断。
static mut TEXT: [u8; 4096] = [0; 4096];
static mut BLUR_TEXT: [u8; 2048] = [0; 2048];

pub(crate) unsafe fn blur_refresh() -> *const u8 {
    let p = core::ptr::addr_of_mut!(BLUR_TEXT) as *mut u8;
    let mut w = crate::text::W::new(p, 2048);
    w.s(b"-- blur --\n");
    let (active, mask, hold, fail, lost) = crate::background_home::corona_debug();
    w.s(b"mode=").n(crate::background_home::mode()).c(b'\n');
    w.s(b"cov=").n(active).s(b" mask=").n(mask).c(b'\n');
    w.s(b"hold=").n(hold).s(b" fail=").n(fail).s(b" lost=").n(lost).c(b'\n');
    let mut diag = [0u8; 1536];
    let n = crate::corona_blur::diagnostics(&mut diag);
    if n != 0 { w.s(&diag[..n]); } else { w.s(b"diagnostics unavailable\n"); }
    w.s("\n返回再进入可刷新数据。\0".as_bytes()); w.end(); p
}

/// 十六进制打印一个字节（两位，大写）。
unsafe fn hex_byte(w: &mut crate::text::W, value: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    w.c(HEX[(value >> 4) as usize]).c(HEX[(value & 0xF) as usize]);
}

/// 重新生成整页文本，返回静态缓冲指针（带终止符）。
pub(crate) unsafe fn refresh() -> *const u8 {
    let ptr = core::ptr::addr_of_mut!(TEXT) as *mut u8;
    let mut w = crate::text::W::new(ptr, TEXT.len());

    // ---- 显示 ----
    w.s(b"-- display --").c(b'\n');
    let (disp, bytes) = crate::screen_rotation::probe();
    if disp != 0 {
    }
    w.s(b"disp=").n(disp).c(b'\n');
    w.s(b"flip=").n(if crate::screen_rotation::enabled() { 1 } else { 0 }).c(b'\n');
    w.s(b"scrn=").n(crate::fw_api::screen_state()).c(b' ');
    // screen_state = SCREEN_MGR+0x86A 原始字节: 亮屏/AOD/息屏各看一次,
    // 把 AOD 归到哪一档定下来(若 AOD 也是 2, 息屏空闲档在 AOD 里不生效)。
    // disp 结构 0x20..0x5F：render_mode(疑 +0x39：0=partial 1=direct 2=full)
    // 与 rotation 都在这段里。翻转前后各看一次即可把两者分开。
    for (i, byte) in bytes.iter().enumerate() {
        if i % 8 == 0 {
            if i != 0 {
                w.c(b'\n');
            }
            w.s(b" +");
            w.n((0x20 + i) as u32);
            w.c(b':');
        }
        w.c(b' ');
        hex_byte(&mut w, *byte);
    }
    w.c(b'\n');

    // 翻转前那一刻的同一段字节：和上面逐字节对比，变了的才是 rotation 相关字段。
    if let Some(prior) = crate::screen_rotation::before() {
        w.s(b"-- before flip --").c(b'\n');
        for (i, byte) in prior.iter().enumerate() {
            if i % 8 == 0 {
                if i != 0 {
                    w.c(b'\n');
                }
                w.s(b" +");
                w.n((0x20 + i) as u32);
                w.c(b':');
            }
            w.c(b' ');
            hex_byte(&mut w, *byte);
        }
        w.c(b'\n');
    }

    // ---- 模糊引擎 ----
    // 现场数据：接管态 / 区域掩码 / 暂停与失败计数 + C 侧 Corona 状态机快照。
    // "模糊莫名其妙失效"就看这里：owned=0 表示原生 owner 被摘掉，
    // latch=1 表示固件的滚动手势标记没清（上游 poll 会在门口直接 return）。
    w.s(b"-- blur --").c(b'\n');
    w.s(b"mode ").n(crate::background_home::mode())
        .s(b" engine ").n(if crate::corona_blur::active() { 1 } else { 0 })
        .s(b" v2 ").n(if crate::corona_blur::v2_enabled() { 1 } else { 0 }).c(b'\n');
    let (active, mask, hold, fail, lost) = crate::background_home::corona_debug();
    w.s(b"cov ").n(active).s(b" mask ").n(mask)
        .s(b" reg ").n(crate::blur_zone::mask()).c(b'\n');
    w.s(b"hold ").n(hold).s(b" fail ").n(fail).s(b" lost ").n(lost).c(b'\n');
    let mut diag = [0u8; 1536];
    let n = crate::corona_blur::diagnostics(&mut diag);
    if n > 0 {
        w.s(&diag[..n]);
    } else {
        w.s(b"(diag unavailable)").c(b'\n');
    }

    // ---- 系统 ----
    w.s(b"-- system --").c(b'\n');
    let (app, page) = fw_api::top_page_ids();
    w.s(b"top ").n(app).c(b':').n(page).c(b'\n');
    w.s(b"pages ").n(crate::state::PAGE_COUNT as u32).c(b'\n');

    // ---- 以后在这里继续追加 ----

    w.end();
    ptr as *const u8
}
