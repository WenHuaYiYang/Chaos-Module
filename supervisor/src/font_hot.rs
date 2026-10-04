// 字体管理器探针 + wrapper 原地热替换 —— 融合 Corona 字体方案的核心一招(2026-10-04)。
//
// ============================ 探针 v1(只读) ============================
// 背景: 补写(font_tree 逐对象直写 local style)诟病已久 + 快应用文字一直不生效。
// Corona 的做法是在管理器中间层原地热替换: 记录的 font 指针与 wrapper 节点的
// lv_font_t 主体(0..28 字节)原地改写, 已建对象下一帧重绘自动跟上。
//
// 布局(v1 探针真机 st=1 实证 + Corona revalidate/COMMIT 互证):
//   fm = rd32(0x20103174)            fm 单例
//   pm = rd32(fm + 0x1C)             路径管理器(Corona 的 mgr)
//   pm+0/4      记录链头(节点 48B): +0 font(=&d.font), +4 族名指针(**内嵌**于 +12,
//               Corona resources() 的 rd(p+4)!=p+12 门实证), +8 尺寸|样式<<16,
//               +44 wrapper 引用数; next@+52(双链, 前驱@+48)
//   pm+12/16    wrapper 链头(节点 40B): 0..28 = font 结构体主体, +28 fallback,
//               +32 user, +36 回指记录; next@+44
//   pm+24       注册表链头(节点 8B): {族名指针, **带 / 的文件路径**}; next@+12
//   描述符: d+0="__FT", font 结构体内联于 d+4..d+32(d+28 槽=自指针 d),
//           d+40=字号, d+44=key, d+48=ctx, d+52=face, d+56=缓存条目, d+60=路径
//
// ============================ 热替换 v3 ============================
// Corona COMMIT(font_reload.c 691-697)的轻量版:
//   - 不新建描述符/不销毁旧描述符/不接管注册表(那些重活我们不干);
//   - 对每个目标记录, 把**我们同字号脸**的 font 结构体(28 字节 = 描述符 d+4..d+32)
//     原地写进两处: ① *record.font(他们描述符里的 font 结构体, = rd32(rec));
//     ② 该记录全部 wrapper 节点的 0..28。
//   为什么两处都要写: Corona revalidate(633 行)验的固件不变量
//     "wrapper 主体 == *record.font" —— 只写一处就是破坏它; 两处同写则保持。
//   回调上下文: font 结构体 +24 槽是描述符自指针, 拷贝自动带上**我们的**自指针
//     → 回调落到我们的描述符/face, 字形与两级缓存全部走我们的; 他们的 MiSans
//     face 与缓存闲置但完好 —— 不销毁 = 无双重所有权。
//   可逆: 首次替换一条记录前, 把 *record.font 原 28 字节(7 字)存进恢复表;
//     revert_all() 原样写回两处。**删除在用字体前必须先调它**。
//   fail-closed: 每步全量校验, 任一不过零写入, FH_ERR 记失败码; 写目标一律过
//     safe_ptr(RAM 门) —— plausible 门允许的 flash 别名域写了就是 hardfault。
//   换代自愈: 台账随 commit/revert 换代清零, sweep 每轮用 cch_face_for_size
//     现查当前字体的脸; *rec.font 与新脸不等就走未换路径重写(恢复表里**首次**
//     的备份保留不动), 换字体/重建脸自动跟上。
//
// ---------- 目标集合(v3.2 定案: 活实例精确集合) ----------
// 演进: v2 前缀匹配族名(真机 m=0 证伪: rec+4 是族名非路径) → v3 捕获开机原字体
// 三向匹配+同名兄弟(真机 C96/M48 双撞上限, 同名扩展被挤死 = 快应用不跟随) →
// v3.2 候选 = **wrapper 链回指的记录集合(去重)**:
//   有 wrapper = 有对象在用 = 该换 —— 这是活实例的精确集合, 捕获/三向/同名三套
//   机制全部退役: 开机捕获的 wrapper 与快应用运行期新建的 wrapper 在同一条链上,
//   下一轮 sweep 自动入列; 我们自己的记录也在链上, swap 的已换判定(body_eq)
//   对它天然幂等零写入。(font_apply 捕获代码保留不调, 证据在案。)
// 每个候选记录过"字号脸"门: 台账有该字号的脸直接用; 没有就**按需现建**
//   (font_apply::ensure_face_for_size, 用台账字体名+该字号, 预算 2/轮) ——
//   快应用运行期字号千奇百怪, 不现建就永远没有拷贝源(v3.1 真机判明)。
//
// ---------- 台账指针形态(v3.1 修正, 真机 e=20 实证 + 反汇编 0x0C85F6CC 定案) ----------
// fw_mfont_create_named(=vg_font_create_core 0x0C8603B0) 的返回值**不是管理器记录**,
// 而是 wrapper(0x28=40B 块): vg_font_create(0x0C85F6CC) 命中/新建路径都会新建一块,
// 0..36 拷贝 *rec.font 36B(lv_font_t 实长 36B, 非 Corona 参考的 28B), +36 回指记录,
// 然后把这个 wrapper 返回给调用者写进 style —— **style text_font 存的就是 wrapper**。
// 故台账(FA_CCH_FACE)存的是 wrapper; src_gate 双形态: 先 wrapper+36→记录, 再直解兜底。
// 换替拷贝仍按 Corona 语义只写 0..28(不变量范围), 各 wrapper 的 28..36 快照残值无碍
// (自指针+24 在 28 内, 回调上下文走它; 固件 revalidate 同步的也是这个语义)。
//
// ---------- 容量与覆盖(v3.2, 真机 v3.1 读数 C96 M48/16 换16/23 判明) ----------
// 真机数字三处撞上限: C96(捕获满) M48(候选满→同名扩展被挤死→快应用字号的
// 记录进不了候选) 换16/e23(恢复表满→第 17 条起拒绝)。且被换 16 条 = 开机捕获
// 命中的全部活跃实例(全局/表盘应已跟随), 快应用不跟随 = 它运行期新建的
// (MiSans, size) 记录既不在捕获表、字号脸又不在台账 —— 双门都进不去。
// 修复: 候选改 wrapper 回指精确集合(三套匹配机制退役) / SW 16→96 / CAND 64 /
// 按需现建字号脸(预算 2/轮, sweep 开头重置; swap_record 内 wrapper 收集先于
// 查脸/建脸, 无 wrapper 的记录不白耗预算)。
//
// 【时机】font_hot::tick() 挂全局 50ms 拍(watchface), 20 拍一轮 + quiet 门,
//   同线程(lv_timer 协作式)与固件的字体创建/渲染无并发窗口。
//
// 读数(字体页最后一行, 拆两个标签避开 32B 副标签预算):
//   主标签: C<台账脸数, >0=应用过> M<恢复表条数 = 累计覆盖记录>
//   副标签: 热<st> 换<本轮写的wrapper>/<失败码>
//     st 同 v1: 1 结构全对 / 2 fm坏 / 3 pm坏 / 4 记录链坏 / 5 wrapper链坏 / 6 注册表头坏
//     失败码: 10 fm坏 11 pm坏 12 记录链坏 13 wrapper链坏
//             20 台账项不像 wrapper(+36 不可解析)/rec 不像指针
//             25 记录.font 不可写 RAM 26 自指针或描述符魔数错 27 ascent 为 0
//             21 目标记录结构坏 22 不变量破 23 恢复表满 (24 已随收集式退役)

use crate::font_apply;
use crate::mem::{rd32, rd8};
use crate::*;

const FM_SLOT: u32 = 0x2010_3174;
const PM_OFF: u32 = 0x1C;
/// 工厂/自建描述符共用的魔数 "__FT"(Corona descriptor() 同款判定)
const DESC_MAGIC: u32 = 0x5F5F_4654;
/// font 结构体字节数(wrapper 0..28 = 描述符 d+4..d+32; Corona 拷贝同宽)
const BODY: u32 = 28;
const BODY_WORDS: usize = (BODY / 4) as usize;
/// 恢复表容量(按记录分组, 一个字号一条; 超出即停, 不挤掉已有的)。
/// v3.5: 96 → 224 —— 真机 v3.4 M96 撞满(活记录 128+, 表满后新记录一律
/// e=23 拒换, 且拒的恰是链尾的快应用高频字号 = 感知"覆盖更少")。
/// 表位需求 = 换过 *rec.font 的记录数(= 有 wrapper 的活记录), 224 对观测值
/// 128 留 75% 余量。bss +9KB(SW_BAK 6.3KB + 三数组 2.7KB)。
/// fail-closed 保留: 表满仍停手(无备份就不换, revert 可逆性优先于覆盖面)。
const SW_CAP: usize = 224;

// ===== 探针结果(RAM 无痕, 每次进字体页重跑) =====
static mut FH_ST: u32 = 0;
static mut FH_REC: u32 = 0;
static mut FH_WRAP: u32 = 0;
// ===== 热替换状态 =====
static mut FH_SW: u32 = 0;      // 恢复表条数 = 已热替换的记录数
static mut FH_ERR: u32 = 0;     // 最近一次失败码(0 = 无)
static mut FH_MT: u32 = 0;      // 最近一轮匹配到的记录数
static mut FH_MW: u32 = 0;      // 最近一轮匹配记录的 wrapper 总数
static mut FH_TICK: u32 = 0;    // 拍计数(20 拍一轮)
// 恢复表(按记录分组): 记录指针 / 字号 / 当前源脸 / *rec.font 的原 7 字备份
static mut SW_REC: [u32; SW_CAP] = [0; SW_CAP];
static mut SW_SZ: [u32; SW_CAP] = [0; SW_CAP];
static mut SW_SRC: [u32; SW_CAP] = [0; SW_CAP];
static mut SW_BAK: [[u32; BODY_WORDS]; SW_CAP] = [[0; BODY_WORDS]; SW_CAP];

/// 串前缀比较(带 plausible 门, 假指针当不匹配)
unsafe fn str_eq_prefix(p: u32, s: &[u8]) -> bool {
    if !plausible_ptr(p) { return false; }
    let mut i = 0usize;
    while i < s.len() {
        if rd8((p + i as u32) as *const u8) != s[i] { return false; }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 热替底层原语(全部只碰过门的真实 RAM 地址)
// ---------------------------------------------------------------------------

#[inline(always)]
unsafe fn wr32(p: u32, v: u32) { core::ptr::write_volatile(p as *mut u32, v); }

/// 28 字节逐字比较(不变量校验/已换判定共用)
#[inline(always)]
unsafe fn body_eq(a: u32, b: u32) -> bool {
    let mut k = 0u32;
    while k < BODY {
        if rd32((a + k) as *const u32) != rd32((b + k) as *const u32) { return false; }
        k += 4;
    }
    true
}

/// 28 字节拷贝(dst 必须已过 RAM 门)
#[inline(always)]
unsafe fn body_copy(dst: u32, src: u32) {
    let mut k = 0u32;
    while k < BODY {
        wr32(dst + k, rd32((src + k) as *const u32));
        k += 4;
    }
}

/// 管理器门: 返回 pm(合格)或 0。err: 10 fm坏 / 11 pm坏。
unsafe fn mgr_gate() -> u32 {
    let fm = rd32(FM_SLOT as *const u32);
    if !plausible_ptr(fm) { return 0; }
    let pm = rd32((fm + PM_OFF) as *const u32);
    if !plausible_ptr(pm) { return 0; }
    pm
}

/// 单条记录的拷贝源门: rec → *rec(= 描述符内联 font = d+4) → 自指针/魔数/度量。
/// 记录布局(vg_font_create 慢路径 0x0C85F772-0x0C85F786 实证):
///   rec+0 = &d.font, rec+4 = 内嵌名字指针(rec+0xC), rec+8 = {size:u16, flag:u16}。
/// 返回 (src, 失败码): 20 rec 不像指针 / 25 *rec 不可写 RAM / 26 自指针或魔数错 /
/// 27 ascent 为 0。真机 v3.2 有 3/64 条走到失败, 细分码一次定位。
unsafe fn gate_from_rec(rec: u32) -> (u32, u32) {
    if !plausible_ptr(rec) { return (0, 20); }
    let src = rd32(rec as *const u32);
    if !safe_ptr(src) { return (0, 25); }
    let d = rd32((src + 24) as *const u32);
    if !safe_ptr(d) || src != d.wrapping_add(4) || rd32(d as *const u32) != DESC_MAGIC {
        return (0, 26);
    }
    if rd32((src + 12) as *const u32) == 0 { return (0, 27); }  // 度量槽(描述符 d+16)
    (src, 0)
}

/// 我们脸的门。台账存的是 **wrapper**(vg_font_create 命中/新建路径都返回一个
/// 0x28 块: 0..36 拷贝 *rec.font 36B, +36 回指记录, 见 0x0C85F716-0x0C85F728),
/// 不是管理器记录本体 —— v3 初版把 wrapper 当记录解, src==d+4 必挂 → e=20。
/// wrapper(+36 可解析)只走记录路径(直解 wrapper 是 v3.1 的 bug 路径);
/// +36 不可解析才按记录直解兜底。err: 20/25/26/27(见 gate_from_rec)。
unsafe fn src_gate(our: u32) -> u32 {
    if !plausible_ptr(our) { st_wr!(FH_ERR, 20); return 0; }
    let w_rec = rd32((our + 36) as *const u32);
    if plausible_ptr(w_rec) {
        let (s, why) = gate_from_rec(w_rec);
        if s != 0 { return s; }
        st_wr!(FH_ERR, why);
        return 0;
    }
    let (s, _why) = gate_from_rec(our);
    if s != 0 { return s; }
    st_wr!(FH_ERR, 20);
    0
}

// ---------------------------------------------------------------------------
// 热替换主流程(v3.4 单遍流式)
// ---------------------------------------------------------------------------

/// 沿 wrapper 链遇到一个 wrapper(回指 rec2)时调用。
/// 记录已换(表命中): body_eq 判幂等, 只补齐这个 wrapper; 换代后内容不等则
/// 保留原备份重写(换代自愈, SW_SRC 更新为当前脸)。
/// 新记录: 登记恢复表(备份 *rec.font 原字节) + 两处同写(*rec.font 与本 wrapper)。
/// 同记录其余 wrapper 由链上后续节点自然补齐 —— 无收集数组、无数量上限
/// (收集式在真机被证伪: 固件每次 style_set 新建 wrapper 不回收, M128 撞候选
/// 上限 / e24 撞收集上限, 数量无上界)。
/// 返回写的 wrapper 数(0/1)。
/// err: 20/25/26/27 我们脸坏 / 21 目标记录坏 / 22 不变量破 / 23 恢复表满。
unsafe fn swap_one(rec: u32, wp: u32) -> u32 {
    let size = rd32((rec + 8) as *const u32) & 0xFFFF;
    // 目标一: *rec.font(他们描述符里的 font 结构体) —— 必须是可写 RAM
    let dstf = rd32(rec as *const u32);
    if !safe_ptr(dstf) { st_wr!(FH_ERR, 21); return 0; }
    // 单点不变量: 这个 wrapper 主体 == *rec.font。其余 wrapper 若不一致只会被
    // 固件 revalidate 双向同步(RAM 内自愈, 无崩溃面), 不预检 —— 收集式全体验证
    // 在数量无上界的现实下既贵又挡路。
    if !body_eq(wp, dstf) { st_wr!(FH_ERR, 22); return 0; }
    // 当前在用字体在这个字号的脸(换代即清的台账 → 永远是"现在的"脸)
    let mut our = font_apply::cch_face_for_size(size);
    if our == 0 {
        // 该字号我们没建脸: 按需现建(预算 2/轮, 见 font_apply::ensure_face_for_size)。
        // 快应用运行期会用各种字号 —— 不现建就永远没有拷贝源 = 快应用永不跟随。
        our = font_apply::ensure_face_for_size(size);
        if our == 0 { return 0; }
    }
    let src = src_gate(our);                                 // 失败码由 src_gate 内部置
    if src == 0 { return 0; }
    // 恢复表定位/登记: 这条记录首次替换时备份 *rec.font 原字节(换代重写复用旧槽, 备份不覆盖)
    let mut e: usize = SW_CAP;
    let mut j = 0usize;
    while j < SW_CAP {
        if st_rd!(SW_REC[j]) == rec { e = j; break; }
        j += 1;
    }
    if e == SW_CAP {
        j = 0;
        while j < SW_CAP {
            if st_rd!(SW_REC[j]) == 0 { e = j; break; }
            j += 1;
        }
        if e == SW_CAP { st_wr!(FH_ERR, 23); return 0; }     // 表满: 保已有的, 不挤占
        let mut k = 0usize;
        while k < BODY_WORDS {
            st_wr!(SW_BAK[e][k], rd32((dstf + (k as u32) * 4) as *const u32));
            k += 1;
        }
        st_wr!(SW_REC[e], rec);
        st_wr!(SW_SZ[e], size);
        st_wr!(FH_SW, st_rd!(FH_SW) + 1);
    }
    if body_eq(dstf, src) {
        // 内容已是我们(幂等 / 换代后同内容): 只同步这个 wrapper
        if !body_eq(wp, src) { body_copy(wp, src); return 1; }
        return 0;
    }
    st_wr!(SW_SRC[e], our);
    // 两处同写(wrapper 主体 + *rec.font), 固件不变量在写完的瞬间仍然成立
    body_copy(dstf, src);
    body_copy(wp, src);
    1
}

/// 一轮热替换扫描(v3.4 单遍流式)。fail-closed: 全部校验前置, 任一不过零写入。
unsafe fn sweep() {
    st_wr!(FH_MT, 0);
    st_wr!(FH_MW, 0);
    if font_apply::cch_n() == 0 { return; }                  // 没有在用字体(恢复走 revert_all)
    font_apply::hot_build_reset();                           // 本轮按需建脸预算(2/轮)
    let pm = mgr_gate();
    if pm == 0 { st_wr!(FH_ERR, 10); return; }
    if rd32(pm as *const u32) != 48 { st_wr!(FH_ERR, 12); return; }        // 记录链头
    if rd32((pm + 12) as *const u32) != 40 { st_wr!(FH_ERR, 13); return; } // wrapper 链头
    // 沿 wrapper 链单遍处理: 每个 wrapper 按回指记录分流(swap_one 统一换替/
    // 补齐/换代)。链长无上界(运行期新建不回收), 4096 门只防链环。
    let mut wp = rd32((pm + 16) as *const u32);
    let mut n = 0u32;
    let mut mw = 0u32;   // 本轮写的 wrapper 数
    while wp != 0 {
        n += 1;
        if n > 4096 { st_wr!(FH_ERR, 13); return; }
        if safe_ptr(wp) {
            let rec2 = rd32((wp + 36) as *const u32);
            if safe_ptr(rec2) {
                mw += swap_one(rec2, wp);
            }
        }
        wp = rd32((wp + 44) as *const u32);
    }
    st_wr!(FH_MT, mw);                                       // M = 本轮写的 wrapper 数
    st_wr!(FH_MW, n);                                        // 链上 wrapper 总数(链增长诊断)
}

/// 全局 50ms 拍(watchface 调): 20 拍一轮热替换扫描, quiet 门(分拍建脸不跑才动管理器)。
pub(crate) unsafe fn tick() {
    let t = st_rd!(FH_TICK).wrapping_add(1);
    st_wr!(FH_TICK, t);
    if t % 20 != 0 { return; }
    if !font_apply::quiet() { return; }
    sweep();
}

/// 恢复全部热替换(删"在用中的字体"前必调, 与 font_apply::request_revert 同拍;
/// 之后台账清零, sweep 自然不再换)。恢复后, 之后任何字体切换/重应用由 sweep
/// 按新台账重新换。
pub(crate) unsafe fn revert_all() {
    let n = st_rd!(FH_SW) as usize;
    if n == 0 { return; }
    let pm = mgr_gate();
    let mut i = 0usize;
    while i < n {
        let rec = st_rd!(SW_REC[i]);
        if plausible_ptr(rec) && pm != 0 && rd32(pm as *const u32) == 48 {
            // 记录仍活着就恢复(不再验族名 —— 换过的记录就是我们记录下来的那条)
            let dstf = rd32(rec as *const u32);
            if safe_ptr(dstf) {
                let mut k = 0usize;
                while k < BODY_WORDS {
                    wr32(dstf + (k as u32) * 4, st_rd!(SW_BAK[i][k]));
                    k += 1;
                }
            }
            // wrapper 链上该记录的全部节点
            if rd32((pm + 12) as *const u32) == 40 {
                let mut p = rd32((pm + 16) as *const u32);
                let mut cnt = 0u32;
                while p != 0 && cnt <= 4096 {
                    cnt += 1;
                    if safe_ptr(p) && rd32((p + 36) as *const u32) == rec {
                        let mut k = 0usize;
                        while k < BODY_WORDS {
                            wr32(p + (k as u32) * 4, st_rd!(SW_BAK[i][k]));
                            k += 1;
                        }
                    }
                    p = rd32((p + 44) as *const u32);
                }
            }
        }
        st_wr!(SW_REC[i], 0);
        i += 1;
    }
    st_wr!(FH_SW, 0);
}

// ---------------------------------------------------------------------------
// 探针(只读; v3.6 从字体页 UI 撤下, 函数留档 —— 排障时一行调用即可恢复读数)
// ---------------------------------------------------------------------------

/// 只读探针: 验证管理器三链布局(v1 已真机 st=1)。失败码: 2 fm坏 / 3 pm坏 /
/// 4 记录链坏 / 5 wrapper链坏 / 6 注册表头坏。
#[allow(dead_code)]
pub(crate) unsafe fn probe_run() {
    st_wr!(FH_ST, 0);
    st_wr!(FH_REC, 0);
    st_wr!(FH_WRAP, 0);
    let fm = rd32(FM_SLOT as *const u32);
    if !plausible_ptr(fm) { st_wr!(FH_ST, 2); return; }
    let pm = rd32((fm + PM_OFF) as *const u32);
    if !plausible_ptr(pm) { st_wr!(FH_ST, 3); return; }
    // ---- 记录链(节点 48B, next@+52) ----
    if rd32(pm as *const u32) != 48 { st_wr!(FH_ST, 4); return; }
    let mut p = rd32((pm + 4) as *const u32);
    let mut n = 0u32;
    while p != 0 {
        n += 1;
        if n > 4096 { st_wr!(FH_ST, 4); return; }
        p = rd32((p + 52) as *const u32);
    }
    st_wr!(FH_REC, n);
    // ---- wrapper 链(节点 40B, next@+44) ----
    if rd32((pm + 12) as *const u32) != 40 { st_wr!(FH_ST, 5); return; }
    p = rd32((pm + 16) as *const u32);
    n = 0;
    while p != 0 {
        n += 1;
        if n > 4096 { st_wr!(FH_ST, 5); return; }
        p = rd32((p + 44) as *const u32);
    }
    st_wr!(FH_WRAP, n);
    // ---- 注册表链头(节点 8B) 只验魔数 ----
    if rd32((pm + 24) as *const u32) != 8 { st_wr!(FH_ST, 6); return; }
    st_wr!(FH_ST, 1);
}

/// 读数: (结构态, 台账脸数, 本轮写wrapper, 链上wrapper总数, 恢复表条数, 热替失败码)
#[allow(dead_code)]
pub(crate) unsafe fn readout() -> (u32, u32, u32, u32, u32, u32) {
    (st_rd!(FH_ST), font_apply::cch_n(),
     st_rd!(FH_MT), st_rd!(FH_MW),
     st_rd!(FH_SW), st_rd!(FH_ERR))
}
