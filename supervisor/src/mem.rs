//! 内存访问器：把散落各处的 `read_volatile`/`write_volatile` 冗长写法收敛成一行。
//!
//! ## 为什么要有这一层
//!
//! 裸 `read_volatile`/`write_volatile` 在业务代码里出现极多，
//! 每次都要写 `core::ptr::` + 强制转换，读起来噪声比信息多，也容易在类型转换上写错。
//! 这里给两组工具：
//!
//! - `st_rd!(X)` / `st_wr!(X, v)`：读写**静态量**，类型自动推断，不需要任何转换。
//! - `rd8/rd16/rd32`：按**裸地址**读，显式声明位宽。
//!
//! 写路径统一走 `st_wr!`（静态量）或直接 `write_volatile`（缓冲区扫描循环里 u8 更直观），
//! 所以这里不再提供 `wr*`，需要时再加。
//!
//! 两者都只是 `core::ptr::read_volatile` / `write_volatile` 的薄封装，
//! 加 `#[inline(always)]` 后生成的代码与手写完全一致（构建产物体积可以核对）。
//!
//! 注意：`volatile` 只保证不被编译器优化掉，**不保证原子性**。
//! 多字段状态（如页管理器 `结构 + 页数 + 索引`）要一次读完再判断，不要边读边比。

/// 读一个静态量（u8/u16/u32 等任意 `Copy` 类型），类型由静态量本身决定
#[macro_export]
macro_rules! st_rd {
    ($v:expr) => {
        core::ptr::read_volatile(core::ptr::addr_of!($v))
    };
}

/// 写一个静态量
#[macro_export]
macro_rules! st_wr {
    ($v:expr, $val:expr) => {
        core::ptr::write_volatile(core::ptr::addr_of_mut!($v), $val)
    };
}

/// 读 8 位
#[inline(always)]
pub(crate) unsafe fn rd8(p: *const u8) -> u8 {
    core::ptr::read_volatile(p)
}

/// 读 16 位
#[inline(always)]
pub(crate) unsafe fn rd16(p: *const u16) -> u16 {
    core::ptr::read_volatile(p)
}

/// 读 32 位
#[inline(always)]
pub(crate) unsafe fn rd32(p: *const u32) -> u32 {
    core::ptr::read_volatile(p)
}

// ---------------------------------------------------------------------------
// 指针门
// ---------------------------------------------------------------------------
// 手上只有裸地址, 解引用前必须判断"这个值像不像一个真指针"。两道门分工:
// plausible_ptr = "像固件里的任何东西"(代码/只读常量/别名域/RAM/堆), 读内容用;
// safe_ptr      = "像可写的 RAM/堆对象", 拿去当 LVGL 对象/链表节点遍历用。
// 整个固件镜像有 MPU 空洞与未映射间隙(实测: 扫错区间一进页就崩), 所以宁可判假也不能放开。

/// 宽松门: flash 代码段 / 0x2C 只读别名域 / 段B(0x1C) / RAM / 堆。
pub(crate) unsafe fn plausible_ptr(v: u32) -> bool {
    (v >= 0x0C00_0000 && v <= 0x0DFF_FFFF)
        || (v >= 0x2C00_0000 && v <= 0x2DFF_FFFF)
        || (v >= 0x1C00_0000 && v <= 0x1DFF_FFFF)
        || (v >= 0x2000_0000 && v <= 0x2400_FFFF)
        || (v >= 0x3000_0000 && v <= 0x3FFF_FFFF)
}

/// 严格门: 4 字节对齐的 RAM 或堆 —— 能当对象指针遍历的只有这一类。
/// 0x0C/0x1C/0x2C 是代码与只读常量, 拿它们当链表节点会读到垃圾并一路走下去。
pub(crate) unsafe fn safe_ptr(v: u32) -> bool {
    (v & 3) == 0
        && ((v >= 0x2000_0000 && v <= 0x2400_FFFF)
            || (v >= 0x3000_0000 && v <= 0x3FFF_FFFF))
}
