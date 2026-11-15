//! 视觉 token: 配色 / 间距 / 圆角 / 字号。
//!
//! 数值只收录真机上已经出现过的那批, 不引入新的 HEX —— 页面代码引用一律走
//! `T::*`, 禁止在页面里写裸色值; 要新颜色先来这里加一个 token。
//! 旧代码里的 C_* 常量是这里的别名(见 page.rs), 逐步把调用点改过来。

pub(crate) mod T {
    // ===== 配色(与旧 C_* 同值) =====
    pub const SECONDARY: u32 = 0x8E_8E93;
    pub const ACCENT: u32 = 0x2D_7CF6;
    pub const SUCCESS: u32 = 0x30_D158;
    pub const WARNING: u32 = 0xFF_9F0A;
    pub const DANGER: u32 = 0xFF_453A;
    pub const HIDDEN: u32 = WARNING;
    pub const HIDDEN_TINT: u32 = 0x4A_3008;
    pub const VISIBLE_TINT: u32 = 0x10_284D;
    pub const VISIBLE: u32 = ACCENT;

    // ===== 间距(px) =====
    #[allow(dead_code)] pub const SP_1: i32 = 4;
    pub const SP_2: i32 = 8;
    pub const SP_3: i32 = 12;
    #[allow(dead_code)] pub const SP_4: i32 = 16;
    /// 行与行的纵向间距(与 build_row 的 align dy=8 一致)
    pub const ROW_GAP: i32 = 8;
    /// 内容底部安全区(不压手势区)
    pub const SAFE_BOTTOM: i32 = 32;

    // ===== 圆角 =====
    pub const R_PILL: u32 = 100;
    #[allow(dead_code)] pub const R_TILE: u32 = 32;

    // ===== 字号(只保留真机已验的两档) =====
    #[allow(dead_code)] pub const FONT_BASE: u32 = 24;
    #[allow(dead_code)] pub const FONT_TITLE: u32 = 28;
}
