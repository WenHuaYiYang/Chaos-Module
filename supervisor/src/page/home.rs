//! 页0：主页与实时状态（CPU / 存储 / 内存）。

use crate::page::*;
use crate::page::Page;

pub(crate) struct InfoPage;

/// 读缓存里的占用百分比(0=存储 1=内存)。render 建条时用, 不碰 procfs。
unsafe fn stat_pct(i: usize) -> u32 {
    read_volatile((core::ptr::addr_of!(STAT_PCT) as *const u32).add(i))
}

impl Page for InfoPage {
    fn fill_title(&self, buf: *mut u8) {
        // 标题栏文本 = 应用名 "Chaos"。
        // 缓冲 48B, 5 字节写入无越界
        unsafe { put_str(buf, "Chaos\0".as_bytes()); }
    }
    fn title_mode(&self) -> u32 { 0 }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            DBG_ON = 0; DBG_CLICKS = 0;
            // 展示顺序: CPU(标准行) -> 存储 -> 内存 -> 导航。
            // 句柄槽位不变(数据行仍 0/1, CPU 仍 2), 只改悬挂位置。
            // 进页面不读 procfs: 原先这一屏要 5 次 open/read/close(存储与内存各读两遍 ——
            // stat_fill 一次、storage_pct/mem_pct 又一次), 是"每次进入主页都顿一下"的主要来源。
            // 现在用上一轮的缓存建行: 文本在 STAT_VAL、百分比在 STAT_PCT; 建完把 STAT_TICK
            // 置 19, 下一拍 tick 立刻补一次真值(50ms 后数字就是新的), 进页这一下不碰文件系统。
            let nm2 = stat_name(2);
            let val2 = (core::ptr::addr_of!(STAT_VAL) as *const u8).add(2 * STAT_STRIDE);
            let cpu = build_row(ctx, ctx.title, 2, &RowSpec::display(
                core::slice::from_raw_parts(nm2, cstr_len(nm2)), val2));
            let anchor = if cpu != 0 { cpu } else { ctx.title };
            // 数据行组件(存储=小米蓝, 内存=绿), 封装在 fw_api::data_row_create 等
            let d0 = fw_api::data_row_create(ctx.content, stat_name(0),
                fw_api::BAR_FILL_RGB, stat_pct(0));
            fw_api::data_row_place(&d0, anchor);
            if d0.value != 0 {
                fw_api::label_set_text(d0.value, core::ptr::addr_of!(STAT_VAL) as *const u8);
            }
            let d1 = fw_api::data_row_create(ctx.content, stat_name(1),
                fw_api::BAR_FILL_GREEN, stat_pct(1));
            fw_api::data_row_place(&d1, d0.row);
            if d1.value != 0 {
                fw_api::label_set_text(d1.value,
                    (core::ptr::addr_of!(STAT_VAL) as *const u8).add(STAT_STRIDE));
            }
            // 句柄登记(tick 用): [行, 数值标签, 条] x2
            let hp = core::ptr::addr_of_mut!(DR_HANDLES) as *mut u32;
            write_volatile(hp.add(0), d0.row);
            write_volatile(hp.add(1), d0.value);
            write_volatile(hp.add(2), d0.bar);
            write_volatile(hp.add(3), d1.row);
            write_volatile(hp.add(4), d1.value);
            write_volatile(hp.add(5), d1.bar);
            // 导航入口
            build_rows(ctx, d1.row, &[
                (3, RowSpec::clickable(b"\xe5\xba\x94\xe7\x94\xa8\xe7\xae\xa1\xe7\x90\x86\0")), // 应用管理(页1, Shell++ 移植)
                (4, RowSpec::clickable(b"\xe8\xa1\xa8\xe7\x9b\x98\xe7\xae\xa1\xe7\x90\x86\0")), // 表盘管理(页10)
                (5, RowSpec::clickable(b"\xe7\xb3\xbb\xe7\xbb\x9f\xe7\xbe\x8e\xe5\x8c\x96\0")), // 系统美化(页13: 字体/图标/模糊)
                (8, RowSpec::clickable("NSH终端\0".as_bytes())),
                (6, RowSpec::clickable("设置\0".as_bytes())),
                (7, RowSpec::clickable("关于\0".as_bytes())),   // 时间/日期/亮度/震动/缓存/版本(页14)
            ])?;
            // 主页行的字体不再在这里特殊处理: 逐对象补写(font_tree)会按每个标签
            // 自己的行盒选脸, 覆盖所有页面。这里再无条件写 28px 基准 face 反而
            // 会和它打架(实测: 主页导航行 28px 观感正常, 但同一规则套到列表行就偏大)。
            // 让下一拍 tick 立即刷(周期是 20 拍, 置 19 后 +1 正好命中), 见上面那段注释
            st_wr!(STAT_TICK, 19);
            let _ = cpu;
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            match idx {
                // 3 = 应用管理(页1, Shell++ 移植, 见 appmgr.rs)。
                // (2026-10-15) 原文件管理入口已注释退役:
                //   3 => {
                //       let rp = core::ptr::addr_of_mut!(PATH_STACK) as *mut u8;
                //       write_volatile(rp.add(0), b'/');
                //       write_volatile(rp.add(1), 0u8);
                //       write_volatile(core::ptr::addr_of_mut!(DIR_WIN_SAVE).cast::<u32>(), 0);
                //       nav_goto(1, 0);
                //   }
                3 => nav_goto(1, 0),   // 应用管理
                4 => nav_goto(10, 0),  // 表盘管理
                5 => nav_goto(13, 0),  // 系统美化
                8 => nav_goto(crate::terminal_ui::PAGE_PID, 0), // NSH终端
                6 => nav_goto(14, 0),  // 设置(时间/日期/亮度/震动/缓存/版本)
                7 => nav_goto(18, 0), // 关于(页18, 调试入口)
                _ => {} // 行0-2 是数据行
            }
        }
    }

    fn needs_tick(&self) -> bool { true }

    fn tick(&self) {
        unsafe {
            // 每 20 拍(亮屏 1s)刷新三行实时数据。原先 10 拍(0.5s)一轮要读 5 次 procfs,
            // 现在一轮 3 次、频率减半 —— UI 线程上的文件系统开销降到约 1/3。
            // 存储/内存这类占用率 1 秒一次足够; 跟手靠的是滑动本身, 不是这三个数。
            let tk = st_rd!(STAT_TICK).wrapping_add(1);
            st_wr!(STAT_TICK, tk);
            if tk % 20 != 0 { return; }
            // 息屏 / AOD 一律不刷(续航): 这三行数值只有亮屏时才有人看,
            // 而每 0.5 秒一次的刷新里含三次 /proc 读 + 若干 label 重绘。少了这道门,
            // 息屏期间自愈重建还会把本页拉起来 => 整段息屏时间都在 2Hz 读 /proc 并重画。
            if !fw_api::screen_is_on() { return; }
            if PAGES[0].built == 0 || !page_is_live(0) { return; }
            // 值真变了才写: label_set_text / bar_set_pct 每次都要失效重绘, 而存储、内存
            // 这类值绝大多数拍根本没变。STAT_PREV/BAR_PREV 只在本页 tick 里用。
            static mut STAT_PREV: [[u8; STAT_STRIDE]; 3] = [[0; STAT_STRIDE]; 3];
            static mut BAR_PREV: [i32; 2] = [-1; 2];
            // 返回"文本与上一份不同"(并把新值抄进 prev)。0 终止符之外的字节全比。
            unsafe fn stat_changed(row: usize) -> bool {
                let cur = (core::ptr::addr_of!(STAT_VAL) as *const u8).add(row * STAT_STRIDE);
                let prev = core::ptr::addr_of_mut!(STAT_PREV[row]) as *mut u8;
                let mut i = 0usize;
                let mut diff = false;
                while i < STAT_STRIDE {
                    let c = read_volatile(cur.add(i));
                    if read_volatile(prev.add(i)) != c { diff = true; }
                    write_volatile(prev.add(i), c);
                    if c == 0 { break; }
                    i += 1;
                }
                diff
            }
            let rows = page_rows_ptr(0);
            let upd: unsafe extern "C" fn(u32, *const u8, *const u8, *const u8, i32, u8) -> i32 =
                fw_api::row_update;
            // 行2(CPU): 数值只写值标签, 不走 row_update。row_update 每次都会把
            // "行级联当前解析到的字体"重烘进行内值标签的 local style
            // (0x0C4C89D6 读属性 0x5A -> 0x0C4C8A34 `bl 0xc588e78`), 而本行每秒刷一次
            // => 烘进去的文楷被盖回行级联字体, 逐对象补写 10s 才追回一次。这正是实测
            // "只有 CPU 这类周期刷新的行闪变, 标题与数据行不闪"的根因。
            // 值标签句柄每次从行现取(行的副标签在 +0x40, 已实测核对), 不缓存。
            let r2 = read_volatile(rows.add(2));
            if r2 != 0 {
                stat_fill_pct(2);
                let ch2 = stat_changed(2);
                let val = (core::ptr::addr_of!(STAT_VAL) as *const u8).add(2 * STAT_STRIDE);
                let vl = read_volatile((r2 + ROW_SUB_LB) as *const u32);
                if vl != 0 {
                    if ch2 { fw_api::label_set_text(vl, val); }
                } else {
                    // 值标签还没建出来: 走一次 row_update, 它会顺手把标签建好
                    upd(r2, core::ptr::null(), stat_name(2), val, 0, 0);
                }
            }
            // 行0/行1(数据行): 数值标签与条
            let hp = core::ptr::addr_of_mut!(DR_HANDLES) as *mut u32;
            let bp = core::ptr::addr_of_mut!(BAR_PREV) as *mut i32;
            // 每个文件只读一次: stat_fill_pct 同时产出文本(写进 STAT_VAL)与百分比,
            // 百分比顺手存进 STAT_PCT —— 下次 render 建条就不用再读文件了。
            let pct_pp = core::ptr::addr_of_mut!(STAT_PCT) as *mut u32;
            for i in 0..2usize {
                let p = stat_fill_pct(i);
                write_volatile(pct_pp.add(i), p);
                let ch = stat_changed(i);
                let vl = read_volatile(hp.add(i * 3 + 1));
                if vl != 0 && ch {
                    let val = (core::ptr::addr_of!(STAT_VAL) as *const u8).add(i * STAT_STRIDE);
                    fw_api::label_set_text(vl, val);
                }
                let bar = read_volatile(hp.add(i * 3 + 2));
                let pv = p as i32;
                if bar != 0 && read_volatile(bp.add(i)) != pv {
                    write_volatile(bp.add(i), pv);
                    fw_api::bar_set_pct(bar, p);
                }
            }

        }
    }
}


/// 算 C 字符串长度(不含 \0)

pub(super) static PAGE_INFO: InfoPage = InfoPage;
