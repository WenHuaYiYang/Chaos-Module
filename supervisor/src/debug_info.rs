//! 调试信息汇总（整屏纯文本）。
//!
//! 入口：主页 → 关于（页18）连点 5 次进入调试模式 → “③ 调试信息”。
//! 以后任何"需要肉眼确认裸数值"的测试都往 [`refresh`] 里加一行，页面不用动。
//!
//! 这里只做只读采集：不写固件结构、不改配置，放在调试页里给人看。

use crate::*;

/// 整页文本缓冲。按 24 行 × 48 字节估算，超出会被 W 截断。
static mut TEXT: [u8; 1536] = [0; 1536];

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
    w.s(b"disp=").n(disp).c(b'\n');
    w.s(b"flip=").n(if crate::screen_rotation::enabled() { 1 } else { 0 }).c(b'\n');
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

    // ---- 系统 ----
    w.s(b"-- system --").c(b'\n');
    let (app, page) = fw_api::top_page_ids();
    w.s(b"top ").n(app).c(b':').n(page).c(b'\n');
    w.s(b"pages ").n(crate::state::PAGE_COUNT as u32).c(b'\n');

    // ---- 以后在这里继续追加 ----

    w.end();
    ptr as *const u8
}
