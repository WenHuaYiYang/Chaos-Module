//! 桌面排版律：把 RW6 桌面的作者参数写进 10P launcher 容器自己的仿射表。
//!
//! ## 为什么是写表，不是逐帧写对象
//!
//! 043 的桌面网格是**表驱动**的：容器对象里存着三张仿射表（每项 8 字节 = {斜率, 截距}），
//! 排版函数 `0x004552D0` 每帧拿当前滚动量查这三张表，算出每个可见位的位置与缩放分子，
//! 再由固件自己调缩放。链式三发（次序逐字对着反编译读的）：
//!
//! ```text
//! L    = (序号/3 + self[0x6C]) * 73 + 滚动量     // 夹在 [0, 73*(排数-1)]
//! 项号 = (L/73)*3 + (序号%3) + K0                // 排号取整 => 用的是哪一格
//! x    = lerp(self + 项号*8 + 0x000, L)
//! y    = lerp(self + 项号*8 + 0x0A8, L)
//! 分子 = lerp(self + 项号*8 + 0x150, y)          // 第三发的自变量是 y
//! 缩放 = clamp(分子 * 112 / 盒宽, 0, 300)
//! lerp(p, i) = p[4] + i * p[0]                   // 截距 + 斜率*输入
//! ```
//!
//! 所以**位置与缩放的动画本来就是固件自己在做**。模块侧再逐帧写图像对象的缩放字段，
//! 等于塞进第二个写入者：两个写入者按不同节奏改同一个字段，屏上就是跳动、不连续、
//! 松手之后还在变大变小；而模块只能按"类型 + 尺寸"认对象，会把快应用里同样尺寸的
//! 图标一起写坏。那条路整层删了，现在只写表 —— 一次 63 对浮点，之后不再碰对象树。
//!
//! ## 表首在哪（不带任何模块的一台机器上回读 + 反编译逐条对上）
//!
//! 全局 `0x201102EC` 给出容器，排版函数收到的 self 就是这个容器指针。模式 1 的项号
//! `k = 排*3 + 列 + 0x10`，三张表落在 `self + k*8 + 0 / +0xA8 / +0x150`，所以表首是
//! 容器 +0x80 / +0x128 / +0x1D0，每项 8 字节 `{斜率, 截距}`（lerp 读的是
//! `p[1] + 输入 * p[0]`），21 项首尾相接。默认值：
//!
//! ```text
//! x 表   斜率 0，截距按列 54 / 168 / 282      // 三列，列距 114
//! y 表   斜率 1.534247，截距 -46              // = 112/73，等距排距
//! 分子表 斜率 0，截距 256                     // 恒 256 => 原生桌面不缩放
//! ```
//!
//! 注意: 容器 +0x84 那一格是 x 表第一项的**截距**（54），不是表首。按 +4 起写会把
//! 斜率与截距整对调，而按同一口径读回的抽查仍然自洽 —— 这类假绿只能靠表首现算防住。
//!
//! 10P 带着与 RW6 同一套机制，只是参数填成平的。把 RW6 的原值写进去就是"完全一样"。
//!
//! ## 三个开关为什么必须配着算
//!
//! 缩放那一发的自变量是 y，所以折线的拐点要按**当前真正在用的一组 y 锚点**算。
//! 排距开关选锚点组（RW6 的两头挤 / 固件等距 112），缩放开关决定用不用这组锚点画折线，
//! 内缩开关决定 x 表在缩放的四档上收不收 12px。三条都按同一套段号落表，
//! 单独改一条就会让 x 与 y 的拐点错位（图标在段中间斜着漂）。
//!
//! ## RW6 原值搬过来的三条成分
//!
//! 1. 缩放折线：`[0, 170.667, 256.667, 256.667, 256.667, 170.667, 0]`（外列）
//!    与中列的 202.667 档。256 = 一倍；最外两档降到 0，图标滚到那一档就缩没。
//! 2. 排距不均匀：七档排间距 44/88/105/105/88/39，中间松、两头紧。
//! 3. 外列内缩：RW6 在缩放的两档（首末各两档）上把外列往中间收（外列 15px、内列 3px），
//!    中间三档回到原值 —— 也就是"缩小的那一排更紧凑"。10P 三列（列中心 54/168/282）
//!    只有两列外列，中列压在中轴上不动，外列各收 12px（15 × 336/432 ≈ 11.7）：
//!    内缩档 66/168/270，满档 54/168/282。这里只搬"往中间收"，不搬 RW6 中间三排
//!    向外鼓的那 15/3/-3/-14 —— 10P 满尺寸盒子 112 宽、左沿已经在 -2，往外挪就是切边。
//!
//! ## 读数（运行日志页）
//!
//! `law ` 写进去的项数；`law c` 容器地址；`law m` 模式字（不是 1 也不是 0x10001 时不写表）。
//! 开关选择持久化在 `/data/chaos/applist.cfg`（1 字节位图，缺失=全开），
//! 开机第一次 tick 读回并自动写入表；运行日志里 `ds cfg <位图>` 是读到的值。

use crate::fw_api;
use crate::mem::{rd32, safe_ptr};
use crate::{st_rd, st_wr};

/// 网格容器全局指针字。
const GRID_GLOBAL: u32 = 0x2011_02EC;
/// 排版函数收到的 self 就是容器指针本身（模式字 +0x89C、滚动量 +0x8AC、表首 +0x80
/// 三条都与反编译同址对过）。早前记的"差 4"是把 x 表第一项的截距当成了斜率。
const SELF_OFF: u32 = 0;
/// 项号起点由模式字决定：1 => 0x10，0x10001 => 0x4F。两者都是三列七排的这一族表。
const MODE_OFF: u32 = 0x89C;
const MODE_A: u32 = 0x0000_0001;
const MODE_B: u32 = 0x0001_0001;
const K0_A: usize = 0x10;
const K0_B: usize = 0x4F;
/// 滚动量在 self+0x8AC。
const SCROLL_OFF: u32 = 0x8AC;
/// 项数：三列七排。排版函数里那个乘数写死成 3，所以列数不是可变量。
const ROWS: usize = 7;
const COLS: usize = 3;
/// 三张表相对项首的偏移。
const OFF_X: u32 = 0;
const OFF_Y: u32 = 0x0A8;
const OFF_S: u32 = 0x150;
/// 逻辑排距：位置自变量 L 每排走这么多（模式字不是 2 的那一档）。
const STEP: f32 = 73.0;

/// x 表三列的默认截距（固件原值斜率 0）：列中心 54 / 168 / 282，列距 114。
const X_COL: [f32; 3] = [54.0, 168.0, 282.0];
/// 内缩档的外列位移。RW6 外列在缩放档往中间收 15px（432 屏），336 屏折算 ≈ 12px。
const X_INSET: f32 = 12.0;

/// RW6 七档 y 锚点（相对中间那一排）与排间距 44/88/105/105/88/39，**原样照搬不换算**。
/// 理由：两台机器的分子刻度是同一个（`缩放 = 分子*112/盒宽`，满档 256 就画成 112 像素），
/// 所以锚点之间的像素关系本身就带着同一套留白 —— RW6 的满档盒子与排距也重叠 7 像素，
/// 那是图标素材自带透明边的结果，不是我们引入的错。换算反而会把这套比例打散。
/// 外列与中列不同：中列的第二、五排各再往外 5 像素，那是那圈弧的竖向分量。
const RW6_Y_OUT: [f32; 7] = [-237.0, -193.0, -105.0, 0.0, 105.0, 193.0, 232.0];
const RW6_Y_MID: [f32; 7] = [-237.0, -198.0, -105.0, 0.0, 105.0, 198.0, 237.0];
/// 同一批锚点上的缩放分子（256 = 一倍）。满档 256.667 是 RW6 原值照抄，不是笔误。
/// 最外两档降到 0：图标滚到最外那一档就缩没，这与 RW6 一致。
const RW6_S_OUT: [f32; 7] = [0.0, 170.667, 256.667, 256.667, 256.667, 170.667, 0.0];
const RW6_S_MID: [f32; 7] = [0.0, 202.667, 256.667, 256.667, 256.667, 202.667, 0.0];

/// RW6 把中间那一排摆在自己的屏幕正中（原表 y 就是屏幕像素：七档 20/64/152/257/362/450/489
/// 落在 514 高的屏上，正中 257 那档写的就是 0+257）。10P 照同一口径：表里的 y 直接就是
/// 屏幕像素，屏幕正中 240 落回表里就是 240（实测不带模块时表给的 66/178/290/402 就画在
/// 66/178/290/402，两者相等）。
const Y_CENTER: f32 = 240.0;
/// 固件默认的等距档：斜率 112/73、截距 -46，中间排在表里 290。
const DEF_Y_A: f32 = 112.0 / 73.0;
const DEF_Y_B: f32 = -46.0;
const DEF_Y_MID: f32 = 290.0;
/// 固件默认的缩放分子恒 256（= 不缩放）。
const DEF_S_B: f32 = 256.0;

/// 三个开关都默认开：这一层的存在理由就是"桌面看起来与 RW6 一样"。
static mut LENS_SW: u32 = 1;
static mut PITCH_SW: u32 = 1;
static mut INSET_SW: u32 = 1;
/// 开关选择的持久化: /data/chaos/applist.cfg, 1 字节位图
/// (bit0 缩放 / bit1 排距 / bit2 内缩)。文件缺失或越界 = 0b111(全开)。
/// 回调里只改内存并置脏, 真正落盘与写表都由 tick 做 —— 与"点击只登记"同一条例。
const CFG: &[u8] = b"/data/chaos/applist.cfg\0";
static mut CFG_LOADED: u32 = 0;
static mut CFG_DIRTY: u32 = 0;
/// 上一次写表时的 (容器, 三个开关) 签名。
static mut LAW_SIG: u32 = 0;
static mut LAW_CONT: u32 = 0;
static mut LAW_LOGGED: u32 = 0;
static mut SCROLL_PREV: i32 = 0;

/// 开机第一次用到开关时读一次配置; 之后只读内存。
unsafe fn ensure_loaded() {
    if st_rd!(CFG_LOADED) != 0 {
        return;
    }
    st_wr!(CFG_LOADED, 1);
    let fd = fw_api::open(CFG.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 {
        return;
    }
    let mut v = 0u8;
    let n = fw_api::read(fd, &mut v as *mut u8, 1);
    fw_api::close(fd);
    if n == 1 && v <= 0b111 {
        st_wr!(LENS_SW, (v & 1) as u32);
        st_wr!(PITCH_SW, ((v >> 1) & 1) as u32);
        st_wr!(INSET_SW, ((v >> 2) & 1) as u32);
        crate::log::kv("ds cfg", v as u32);
    }
}

/// tick 里把回调登记的选择落盘(每次最多一次写)。
unsafe fn save_deferred() {
    if st_rd!(CFG_DIRTY) == 0 {
        return;
    }
    st_wr!(CFG_DIRTY, 0);
    let v = ((st_rd!(LENS_SW) & 1)
        | ((st_rd!(PITCH_SW) & 1) << 1)
        | ((st_rd!(INSET_SW) & 1) << 2)) as u8;
    let fd = fw_api::open(CFG.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd >= 0 {
        let _ = fw_api::write(fd, &v as *const u8, 1);
        fw_api::close(fd);
    }
}

pub(crate) unsafe fn lens_on() -> bool {
    ensure_loaded();
    st_rd!(LENS_SW) != 0
}

pub(crate) unsafe fn set_lens(on: bool) {
    ensure_loaded();
    let v = if on { 1 } else { 0 };
    if st_rd!(LENS_SW) == v {
        return;
    }
    st_wr!(LENS_SW, v);
    st_wr!(CFG_DIRTY, 1);
}

pub(crate) unsafe fn pitch_on() -> bool {
    ensure_loaded();
    st_rd!(PITCH_SW) != 0
}

pub(crate) unsafe fn set_pitch(on: bool) {
    ensure_loaded();
    let v = if on { 1 } else { 0 };
    if st_rd!(PITCH_SW) == v {
        return;
    }
    st_wr!(PITCH_SW, v);
    st_wr!(CFG_DIRTY, 1);
}

pub(crate) unsafe fn inset_on() -> bool {
    ensure_loaded();
    st_rd!(INSET_SW) != 0
}

pub(crate) unsafe fn set_inset(on: bool) {
    ensure_loaded();
    let v = if on { 1 } else { 0 };
    if st_rd!(INSET_SW) == v {
        return;
    }
    st_wr!(INSET_SW, v);
    st_wr!(CFG_DIRTY, 1);
}

/// 桌面这一轮有没有在动：读容器里的滚动量与上一拍比。
pub(crate) unsafe fn scrolling() -> bool {
    let cont = rd32(GRID_GLOBAL as *const u32);
    if cont == 0 || !safe_ptr(cont) {
        return false;
    }
    let now = rd32((cont + SELF_OFF + SCROLL_OFF) as *const u32) as i32;
    let moved = st_rd!(SCROLL_PREV) != now;
    st_wr!(SCROLL_PREV, now);
    moved
}

unsafe fn put(item: u32, a: f32, b: f32) {
    core::ptr::write_volatile(item as *mut f32, a);
    core::ptr::write_volatile((item + 4) as *mut f32, b);
}

unsafe fn get(item: u32) -> f32 {
    core::ptr::read_volatile(item as *const f32)
}

/// 当前在用的七档 y 锚点（表里口径）。
unsafe fn knots(out: &mut [f32; 7], mid: &mut [f32; 7]) {
    if st_rd!(PITCH_SW) != 0 {
        let mut i = 0;
        while i < ROWS {
            out[i] = RW6_Y_OUT[i] + Y_CENTER;
            mid[i] = RW6_Y_MID[i] + Y_CENTER;
            i += 1;
        }
    } else {
        let mut i = 0;
        while i < ROWS {
            out[i] = DEF_Y_MID + (i as i32 - 3) as f32 * 112.0;
            mid[i] = out[i];
            i += 1;
        }
    }
}

/// 第 q 段（L 从 q*STEP 到 (q+1)*STEP）的 {斜率, 截距}，端点是给定的锚点。
/// 最后一档没有右端点：沿用前一段的斜率（那一档本来就被夹在 L 的上限上）。
fn seg(ys: &[f32; 7], q: usize) -> (f32, f32) {
    let y0 = ys[q];
    let y1 = if q + 1 < ROWS {
        ys[q + 1]
    } else {
        y0 + (y0 - ys[q - 1])
    };
    let a = (y1 - y0) / STEP;
    (a, y0 - a * (q as f32 * STEP))
}

/// 外列在当前开关下的七档 x 锚点：缩放的四档（0/1/5/6）向中间收 X_INSET，
/// 中间三档回固件原值；中列压在中轴上不动。内缩开关关掉就整列回原值。
unsafe fn x_knots(col: usize, out: &mut [f32; 7]) {
    let base = X_COL[col];
    let inward = if col == 0 {
        base + X_INSET
    } else if col == COLS - 1 {
        base - X_INSET
    } else {
        base
    };
    let inset = st_rd!(INSET_SW) != 0;
    let mut i = 0;
    while i < ROWS {
        let zoom_row = i <= 1 || i >= ROWS - 2;
        out[i] = if inset && zoom_row { inward } else { base };
        i += 1;
    }
}

/// 第 q 段的缩放分子：过 (ys[q], ss[q]) 与 (ys[q+1], ss[q+1]) 的直线。
/// 两点定线正是 RW6 那支护函数（`0x0072F21C`）的算法，这里按同一口径自己算。
fn s_seg(ys: &[f32; 7], ss: &[f32; 7], q: usize) -> (f32, f32) {
    let s0 = ss[q];
    if q + 1 >= ROWS {
        return (0.0, s0);
    }
    let dy = ys[q + 1] - ys[q];
    let a = if dy == 0.0 { 0.0 } else { (ss[q + 1] - s0) / dy };
    (a, s0 - a * ys[q])
}

/// 这一视图用的项号起点；不是这两档就返回 0（表结构不同，不能照写）。
fn k0_of(mode: u32) -> usize {
    if mode == MODE_A {
        K0_A
    } else if mode == MODE_B {
        K0_B
    } else {
        0
    }
}

/// 把律写进容器：三张表（x / y / 缩放分子）都按当前开关重算一遍。
unsafe fn law_write(me: u32, k0: usize) {
    let mut y_out = [0.0f32; 7];
    let mut y_mid = [0.0f32; 7];
    knots(&mut y_out, &mut y_mid);
    let rw6_pitch = st_rd!(PITCH_SW) != 0;
    let rw6_lens = st_rd!(LENS_SW) != 0;
    let rw6_inset = st_rd!(INSET_SW) != 0;
    let mut j = 0usize;
    while j < ROWS * COLS {
        let q = j / COLS;
        let col = j % COLS;
        let item = me + ((k0 + j) as u32) * 8;
        let ys: &[f32; 7] = if col == 1 { &y_mid } else { &y_out };
        if rw6_inset {
            let mut xs = [0.0f32; 7];
            x_knots(col, &mut xs);
            let (a, b) = seg(&xs, q);
            put(item + OFF_X, a, b);
        } else {
            put(item + OFF_X, 0.0f32, X_COL[col]);
        }
        if rw6_pitch {
            let (a, b) = seg(ys, q);
            put(item + OFF_Y, a, b);
        } else {
            put(item + OFF_Y, DEF_Y_A, DEF_Y_B);
        }
        if rw6_lens {
            let ss: &[f32; 7] = if col == 1 { &RW6_S_MID } else { &RW6_S_OUT };
            let (a, b) = s_seg(ys, ss, q);
            put(item + OFF_S, a, b);
        } else {
            put(item + OFF_S, 0.0f32, DEF_S_B);
        }
        j += 1;
    }
}

/// 写入与读出同路径的浮点比对。容差给 float 往返，够小到能分辨"内缩 12px"这一档。
fn near(x: f32, y: f32) -> bool {
    let d = x - y;
    d < 0.0002 && d > -0.0002
}

/// 抽查某一档某一列的三项（x / y / 缩放分子）是否等于当前开关应写出的值。
unsafe fn item_matches(me: u32, k0: usize, q: usize, col: usize,
                       y_out: &[f32; 7], y_mid: &[f32; 7]) -> bool {
    let item = me + ((k0 + q * COLS + col) as u32) * 8;
    let (xa, xb) = if st_rd!(INSET_SW) != 0 {
        let mut xs = [0.0f32; 7];
        x_knots(col, &mut xs);
        seg(&xs, q)
    } else {
        (0.0f32, X_COL[col])
    };
    let ys: &[f32; 7] = if col == 1 { y_mid } else { y_out };
    let (ya, yb) = if st_rd!(PITCH_SW) != 0 {
        seg(ys, q)
    } else {
        (DEF_Y_A, DEF_Y_B)
    };
    let (sa, sb) = if st_rd!(LENS_SW) != 0 {
        let ss: &[f32; 7] = if col == 1 { &RW6_S_MID } else { &RW6_S_OUT };
        s_seg(ys, ss, q)
    } else {
        (0.0f32, DEF_S_B)
    };
    near(get(item + OFF_X), xa)
        && near(get(item + OFF_X + 4), xb)
        && near(get(item + OFF_Y), ya)
        && near(get(item + OFF_Y + 4), yb)
        && near(get(item + OFF_S), sa)
        && near(get(item + OFF_S + 4), sb)
}

/// 抽查三格，判表还在不在我们写的状态：q=1 是内缩档（x 与满档不同），q=3 是满档，
/// 中列单列一类锚点。期望值按当前开关现算，所以三个开关都关时"表就是默认值"也算没漂。
unsafe fn law_matches(me: u32, k0: usize) -> bool {
    let mut y_out = [0.0f32; 7];
    let mut y_mid = [0.0f32; 7];
    knots(&mut y_out, &mut y_mid);
    item_matches(me, k0, 1, 0, &y_out, &y_mid)
        && item_matches(me, k0, 1, 1, &y_out, &y_mid)
        && item_matches(me, k0, 3, 0, &y_out, &y_mid)
}

pub(crate) unsafe fn tick() {
    ensure_loaded();
    save_deferred();
    let cont = rd32(GRID_GLOBAL as *const u32);
    if cont == 0 || !safe_ptr(cont) {
        st_wr!(LAW_SIG, 0);
        st_wr!(LAW_CONT, 0);
        return;
    }
    let me = cont + SELF_OFF;
    if !safe_ptr(me) {
        return;
    }
    let mode = rd32((me + MODE_OFF) as *const u32);
    let k0 = k0_of(mode);
    if k0 == 0 {
        st_wr!(LAW_SIG, 0);
        return;
    }
    let sig = 1 + (st_rd!(LENS_SW) & 1) + ((st_rd!(PITCH_SW) & 1) << 1)
        + ((st_rd!(INSET_SW) & 1) << 2);
    let changed = st_rd!(LAW_SIG) != sig || st_rd!(LAW_CONT) != cont;
    if changed || !law_matches(me, k0) {
        law_write(me, k0);
        st_wr!(LAW_SIG, sig);
        st_wr!(LAW_CONT, cont);
        if st_rd!(LAW_LOGGED) == 0 {
            st_wr!(LAW_LOGGED, 1);
            crate::log::kv("law ", (ROWS * COLS) as u32);
            crate::log::kv("law c", cont);
            crate::log::kv("law m", mode);
        }
    }
}
