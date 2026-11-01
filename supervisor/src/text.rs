// 文本缓冲与格式化工具

use crate::*;

// 原生读取 /proc: fw_api 的文件接口已在文件管理器验证可用
unsafe fn read_proc(path: *const u8, dst: *mut u8, cap: usize) -> usize {
    let fd = fw_api::open(path, fw_api::oflag::RDONLY, 0);
    if fd < 0 { return 0; }
    let mut total = 0usize;
    while total + 1 < cap {
        let n = fw_api::read(fd, dst.add(total), (cap - 1 - total) as u32);
        if n <= 0 { break; }
        total += n as usize;
    }
    fw_api::close(fd);
    write_volatile(dst.add(total), 0);
    total
}

const PROC_CPULOAD: &[u8] = b"/proc/cpuload\0";
const PROC_MEM: &[u8] = b"/proc/meminfo\0";
const PROC_FS: &[u8] = b"/proc/fs/usage\0";

// 取首行(到换行或结束), 写入 dst
unsafe fn copy_line(src: *const u8, dst: *mut u8, cap: usize) -> usize {
    let mut o = 0usize;
    let mut k = 0usize;
    while o + 1 < cap {
        let c = read_volatile(src.add(k));
        if c == 0 || c == b'\n' || c == b'\r' { break; }
        write_volatile(dst.add(o), c);
        o += 1; k += 1;
    }
    write_volatile(dst.add(o), 0);
    o
}

// 跳过非数字后读一个十进制数; ok=false 表示已到结尾
unsafe fn next_uint(src: *const u8, k: &mut usize, ok: &mut bool) -> u32 {
    loop {
        let c = read_volatile(src.add(*k));
        if c == 0 || c == b'\n' { *ok = false; return 0; }   // 不跨行
        if c >= b'0' && c <= b'9' { break; }
        *k += 1;
    }
    let mut v = 0u32;
    loop {
        let c = read_volatile(src.add(*k));
        if c < b'0' || c > b'9' { break; }
        v = v.wrapping_mul(10).wrapping_add((c - b'0') as u32);
        *k += 1;
    }
    *ok = true;
    v
}

// 内存三值解析(对齐 shellpp native_fs.c 的 memory_read_live_umem, 2026-10-16):
// 小米固件把权威分配器统计放在 Umem 行, 形如 "total used free ... Umem" —— 标签在
// 行尾, 且不保证是第 2 行(旧实现按固定行号取, 读到的不是 Umem 行, 数值全错)。
// 逐行跟踪"本行前三个数 + Umem 匹配态", 换行/结尾时 Umem 命中且凑满三个数即命中。
// 返回 (total, used, free), 单位字节。
unsafe fn mem_umem(src: *const u8, n: usize) -> Option<(u32, u32, u32)> {
    let mut numbers = [0u32; 3];
    let mut nc = 0usize;        // 本行已收的数字个数
    let mut nv = 0u32;          // 正在拼的数字
    let mut active = false;     // 数字是否在拼
    let mut st = 0u8;           // "Umem" 匹配进度 0..4
    let mut k = 0usize;
    loop {
        let c = if k < n { read_volatile(src.add(k)) } else { 0 };
        k += 1;
        if c == b'\n' || c == b'\r' || c == 0 {
            if active && nc < 3 { numbers[nc] = nv; nc += 1; }
            if st == 4 && nc >= 3 { return Some((numbers[0], numbers[1], numbers[2])); }
            if c == 0 { break; }
            nv = 0; nc = 0; active = false; st = 0;
            continue;
        }
        if c >= b'0' && c <= b'9' {
            if !active { active = true; nv = 0; }
            if nv <= 429496729 { nv = nv * 10 + (c - b'0') as u32; }
        } else {
            if active {
                if nc < 3 { numbers[nc] = nv; nc += 1; }
                nv = 0; active = false;
            }
            if st == 0 && c == b'U' { st = 1; }
            else if st == 1 && c == b'm' { st = 2; }
            else if st == 2 && c == b'e' { st = 3; }
            else if st == 3 && c == b'm' { st = 4; }
            else if c == b'U' { st = 1; }
            else if c != b' ' && c != b'\t' { st = 0; }
        }
    }
    None
}

// 字节量按 shellpp memory_append_kb 的口径写: >=1MB 显示一位小数 MB, 否则 KB
unsafe fn mem_kb(w: &mut W, bytes: u32) {
    let kb = bytes >> 10;
    if kb >= 1024 {
        let whole = kb / 1024;
        let tenths = (kb % 1024) * 10 / 1024;
        w.n(whole).c(b'.').n(tenths).s(b"MB");
    } else {
        w.n(kb).s(b"KB");
    }
}

// 信息页内存行文本: "已用/总量 百分比%"(shellpp 同款)
unsafe fn mem_mb(src: *const u8, n: usize, dst: *mut u8) -> bool {
    let Some((total, used, _)) = mem_umem(src, n) else { return false; };
    if total == 0 || used > total { return false; }
    let pct = ((used as u64 * 100) / total as u64) as u32;
    let mut w = W::new(dst, 24);
    mem_kb(&mut w, used);
    w.c(b'/');
    mem_kb(&mut w, total);
    w.c(b' ').n(pct).c(b'%');
    w.end();
    true
}

// /proc/fs/usage 数据行(固件 fs_procfsusage.c):
//   "  %-10s %8llu%c %8llu%c  %8llu%c %s\n"  挂载点 容量+单位 已用+单位 可用+单位 挂载点
// 数字已按下表缩放, 单位表 "BKMGT" 与固件同源(0xBFF505), 每级 1024
const UNIT_SHIFT: [u32; 5] = [0, 10, 20, 30, 40];

// 跳过一个非空白 token
unsafe fn skip_token(src: *const u8, k: &mut usize) {
    loop {
        let c = read_volatile(src.add(*k));
        if c == 0 || c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' { return; }
        *k += 1;
    }
}

// 跳过空格/制表
unsafe fn skip_space(src: *const u8, k: &mut usize) {
    loop {
        let c = read_volatile(src.add(*k));
        if c != b' ' && c != b'\t' { return; }
        *k += 1;
    }
}

// 读「数字+单位字符」 → MB
unsafe fn num_unit_mb(src: *const u8, k: &mut usize, ok: &mut bool) -> u32 {
    let v = next_uint(src, k, ok);
    if !*ok { return 0; }
    let mut u = read_volatile(src.add(*k));
    if u >= b'a' && u <= b'z' { u -= 32; }
    let mut sh = 0u32;
    let mut i = 0usize;
    while i < 5 {
        if u == b"BKMGT"[i] { sh = UNIT_SHIFT[i]; break; }
        i += 1;
    }
    if sh != 0 { *k += 1; }
    if sh >= 20 {
        let s = sh - 20;
        if v > (u32::MAX >> s) { u32::MAX } else { v << s }
    } else {
        v >> (20 - sh)
    }
}

// 逐行找「容量 / 已用」, 取容量最大的一行(真实数据分区远大于其它挂载点), 输出 "总MB/用MB"
unsafe fn fs_usage_mb(src: *const u8, dst: *mut u8) -> bool {
    let mut k = 0usize;
    let mut best = 0u32;
    let mut best_used = 0u32;
    let mut found = false;
    loop {
        skip_space(src, &mut k);
        let c = read_volatile(src.add(k));
        if c == 0 { break; }
        if c == b'\n' || c == b'\r' { k += 1; continue; }
        skip_token(src, &mut k);                 // 挂载点
        skip_space(src, &mut k);
        let mut o1 = false;
        let size = num_unit_mb(src, &mut k, &mut o1);
        skip_space(src, &mut k);
        let mut o2 = false;
        let used = if o1 { num_unit_mb(src, &mut k, &mut o2) } else { 0 };
        if o1 && o2 && size > best { best = size; best_used = used; found = true; }
        while read_volatile(src.add(k)) != 0
            && read_volatile(src.add(k)) != b'\n' { k += 1; }
    }
    if !found || best == 0 { return false; }
    let mut w = W::new(dst, STAT_STRIDE);
    w.n(best).s(b"MB/").n(best_used).s(b"MB");
    w.end();
    true
}

// 与 fs_usage_mb 同一数据源: 返回"容量最大的那行"的已用 MB(缓存清理量释放用)。
// 读不到/解析失败返回 u32::MAX 作为"不可信"标记, 调用方据此不显示释放数。
pub(crate) unsafe fn fs_used_mb() -> u32 {
    let mut buf: [u8; 256] = [0; 256];
    if read_proc(PROC_FS.as_ptr(), buf.as_mut_ptr(), 256) == 0 { return u32::MAX; }
    let mut k = 0usize;
    let mut best = 0u32;
    let mut best_used = 0u32;
    let mut found = false;
    loop {
        skip_space(buf.as_ptr(), &mut k);
        let c = read_volatile(buf.as_ptr().add(k));
        if c == 0 { break; }
        if c == b'\n' || c == b'\r' { k += 1; continue; }
        skip_token(buf.as_ptr(), &mut k);
        skip_space(buf.as_ptr(), &mut k);
        let mut o1 = false;
        let size = num_unit_mb(buf.as_ptr(), &mut k, &mut o1);
        skip_space(buf.as_ptr(), &mut k);
        let mut o2 = false;
        let used = if o1 { num_unit_mb(buf.as_ptr(), &mut k, &mut o2) } else { 0 };
        if o1 && o2 && size > best { best = size; best_used = used; found = true; }
        while read_volatile(buf.as_ptr().add(k)) != 0
            && read_volatile(buf.as_ptr().add(k)) != b'\n' { k += 1; }
    }
    if found { best_used } else { u32::MAX }
}

// 首行(截 22 字节)写入 dst
unsafe fn first_line(src: *const u8, dst: *mut u8) -> bool {
    let mut one: [u8; 32] = [0; 32];
    let n = copy_line(src, one.as_mut_ptr(), 24);
    if n == 0 { return false; }
    let mut i = 0usize;
    while i < n && i < 22 { write_volatile(dst.add(i), one[i]); i += 1; }
    write_volatile(dst.add(i), 0);
    true
}

// 信息页数据行的名称
pub(crate) unsafe fn stat_name(idx: usize) -> *const u8 {
    match idx {
        0 => "存储占用\0".as_bytes().as_ptr(),
        1 => "内存占用\0".as_bytes().as_ptr(),
        _ => "CPU占用\0".as_bytes().as_ptr(),
    }
}

// 刷新信息页第 idx 行(0=存储 1=内存 2=CPU)的数值文本, 并返回该行的占用百分比。
//
// 合并前是 stat_fill(idx) + storage_pct()/mem_pct() 两个调用各读一遍同一个 procfs 文件 ——
// 一轮刷新 5 次 open/read/close。现在一个函数读一次、同时产出文本与百分比, 一轮 3 次。
pub(crate) unsafe fn stat_fill_pct(idx: usize) -> u32 {
    let dst = (core::ptr::addr_of_mut!(STAT_VAL) as *mut u8).add(idx * STAT_STRIDE);
    write_volatile(dst.add(0), 0);
    let mut buf: [u8; 512] = [0; 512];   // meminfo 可能有多行分配器统计, 256 不保险
    let pct: u32;
    match idx {
        0 => {
            // 存储: 容量/已用都带单位后缀, 必须按单位还原(见 fs_usage_mb)
            if read_proc(PROC_FS.as_ptr(), buf.as_mut_ptr(), 256) > 0 {
                pct = storage_pct_of(buf.as_ptr());
                if !fs_usage_mb(buf.as_ptr(), dst) { first_line(buf.as_ptr(), dst); }
            } else {
                pct = 0;
            }
        }
        1 => {
            // Umem 行(标签在行尾, 行号不固定), 解析见 mem_umem
            let n = read_proc(PROC_MEM.as_ptr(), buf.as_mut_ptr(), 512);
            if n > 0 {
                pct = mem_pct_of(buf.as_ptr(), n);
                if !mem_mb(buf.as_ptr(), n, dst) { first_line(buf.as_ptr(), dst); }
            } else {
                pct = 0;
            }
        }
        _ => {
            // CPU 占用: /proc/cpuload, 固件格式 "%3ld.%01ld%%"(每 CPU 一行)
            if read_proc(PROC_CPULOAD.as_ptr(), buf.as_mut_ptr(), 64) > 0 {
                let mut one: [u8; 20] = [0; 20];
                let n = copy_line(buf.as_ptr(), one.as_mut_ptr(), 9);
                let mut j = 0usize;
                while j < n && j < 8 { write_volatile(dst.add(j), one[j]); j += 1; }
                write_volatile(dst.add(j), 0);
                return 0;
            }
            pct = 0;
        }
    }
    if read_volatile(dst) == 0 {
        write_volatile(dst, b'-');
        write_volatile(dst.add(1), 0);
    }
    pct
}


// 解析一个带单位的容量: "3.2G" / "512M" / "1024" → 字节数(1024 进制)。
// /proc/fs/usage 的数字带 K/M/G 后缀, 直接按整数取会失真, 所以这里带单位解析。
unsafe fn next_size(src: *const u8, k: &mut usize, ok: &mut bool) -> u64 {
    let mut c = read_volatile(src.add(*k));
    while c != 0 && !(c >= b'0' && c <= b'9') {
        *k += 1;
        c = read_volatile(src.add(*k));
    }
    if c == 0 { *ok = false; return 0; }
    let mut int_part: u64 = 0;
    while c >= b'0' && c <= b'9' {
        int_part = int_part * 10 + (c - b'0') as u64;
        *k += 1;
        c = read_volatile(src.add(*k));
    }
    let mut frac: u64 = 0;
    let mut div: u64 = 1;
    if c == b'.' {
        *k += 1;
        c = read_volatile(src.add(*k));
        while c >= b'0' && c <= b'9' && div < 1_000_000 {
            frac = frac * 10 + (c - b'0') as u64;
            div *= 10;
            *k += 1;
            c = read_volatile(src.add(*k));
        }
    }
    let mut mult: u64 = 1;
    if c == b'K' || c == b'k' { mult = 1024; }
    else if c == b'M' || c == b'm' { mult = 1024 * 1024; }
    else if c == b'G' || c == b'g' { mult = 1024 * 1024 * 1024; }
    else if c == b'T' || c == b't' { mult = 1024 * 1024 * 1024 * 1024; }
    if mult > 1 { *k += 1; }
    *ok = true;
    int_part * mult + (frac * mult) / div
}

// 存储占用百分比 0..100。数据行 = "挂载点 容量 已用 可用 挂载点", 取首行前两个数,
// 用 小/大 反推占用比(与列顺序无关)。这两行说的是下面的 storage_pct, 不是紧跟的 mem_pct。
// 内存占用比 0..100: meminfo 的 Umem 行 已用/总量(mem_umem; 旧实现按固定行号取错行)
unsafe fn mem_pct_of(buf: *const u8, n: usize) -> u32 {
    let Some((total, used, _)) = mem_umem(buf, n) else { return 0; };
    if total == 0 || used > total { return 0; }
    let p = (used as u64 * 100 / total as u64) as u32;
    if p > 100 { 100 } else { p }
}

pub(crate) unsafe fn mem_pct() -> u32 {
    let mut buf: [u8; 512] = [0; 512];
    let n = read_proc(PROC_MEM.as_ptr(), buf.as_mut_ptr(), 512);
    if n == 0 { return 0; }
    mem_pct_of(buf.as_ptr(), n)
}

unsafe fn storage_pct_of(buf: *const u8) -> u32 {
    let mut k = 0usize;
    let mut ok = false;
    let a = next_size(buf, &mut k, &mut ok);
    if !ok { return 0; }
    let b = next_size(buf, &mut k, &mut ok);
    if !ok { return 0; }
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    if hi == 0 { return 0; }
    let p = (lo as u128 * 100 / hi as u128) as u32;
    if p > 100 { 100 } else { p }
}

pub(crate) unsafe fn storage_pct() -> u32 {
    let mut buf: [u8; 256] = [0; 256];
    if read_proc(PROC_FS.as_ptr(), buf.as_mut_ptr(), 256) == 0 { return 0; }
    storage_pct_of(buf.as_ptr())
}


// ---------------------------------------------------------------------------
// 定容行写入器
// ---------------------------------------------------------------------------
// 判据行/状态行都写在**固定容量的静态缓冲区**里(常见 40 或 64 字节), 缓冲区后面
// 紧挨着别的静态量: 少算一个字节就会写坏邻居, 一点即崩(实测踩过两次)。
// 所以这类"带界的 put"只留这一处 —— 各模块分头写时, 同一段格式化出现过四种写法,
// 其中"写数字"的助手**不查界**。这里收敛成唯一入口: 一切追加都过容量门,
// 放不下就截断。
//
// 用法: `let mut w = W::new(dst, cap); w.s(b"T").n(1).c(b' ').x(ptr); w.end();`
// `end()` 必须调 —— 它负责收尾 NUL(行的长度靠 cstr_len 读, 没有 NUL 就是脏数据)。
pub(crate) struct W {
    buf: *mut u8,
    cap: usize,
    len: usize,
}

impl W {
    pub(crate) fn new(buf: *mut u8, cap: usize) -> W {
        W { buf, cap, len: 0 }
    }

    /// 从缓冲区已有 `len` 字节处接着写(同一行分几段拼时用)
    pub(crate) fn at(buf: *mut u8, cap: usize, len: usize) -> W {
        W { buf, cap, len }
    }

    /// 当前已写长度(不含 NUL)
    pub(crate) fn len(&self) -> usize { self.len }

    /// 追加一段字面量; 放不下就截断
    pub(crate) unsafe fn s(&mut self, t: &[u8]) -> &mut Self {
        let mut i = 0usize;
        while i < t.len() && self.len + 1 < self.cap {
            write_volatile(self.buf.add(self.len), t[i]);
            self.len += 1;
            i += 1;
        }
        self
    }

    /// 追加一个字节
    pub(crate) unsafe fn c(&mut self, b: u8) -> &mut Self {
        if self.len + 1 < self.cap {
            write_volatile(self.buf.add(self.len), b);
            self.len += 1;
        }
        self
    }

    /// 追加十进制
    pub(crate) unsafe fn n(&mut self, v: u32) -> &mut Self {
        let mut t = [0u8; 10];
        let mut i = 0usize;
        let mut x = v;
        loop {
            t[i] = b'0' + (x % 10) as u8;
            i += 1;
            x /= 10;
            if x == 0 { break; }
        }
        while i > 0 {
            i -= 1;
            self.c(t[i]);
        }
        self
    }

    /// 追加人类可读的体积: B / KB / MB / GB(MB 以上保留一位小数)
    pub(crate) unsafe fn sz(&mut self, v: u32) -> &mut Self {
        const KB: u32 = 1024;
        const MB: u32 = 1024 * 1024;
        const GB: u32 = 1024 * 1024 * 1024;
        if v < KB { return self.n(v).s(b" B"); }
        if v < MB { return self.n(v / KB).s(b" KB"); }
        let (div, unit): (u32, &[u8]) = if v < GB { (MB, b" MB") } else { (GB, b" GB") };
        let mut whole = v / div;
        let rem = (v % div) as u64;
        let mut frac = ((rem * 10 + (div as u64) / 2) / (div as u64)) as u32;
        if frac >= 10 { whole += 1; frac = 0; }
        self.n(whole).c(b'.').n(frac).s(unit)
    }

    /// 追加 8 位十六进制(大写, 指针/句柄判据用)
    pub(crate) unsafe fn x(&mut self, v: u32) -> &mut Self {
        let hex = *b"0123456789ABCDEF";
        let mut i = 7i32;
        while i >= 0 {
            let d = ((v >> (i * 4)) & 0xF) as usize;
            self.c(read_volatile(hex.as_ptr().add(d)));
            i -= 1;
        }
        self
    }

    /// 收尾: 截断处补 NUL(缓冲区最后一字节永远留给它)
    pub(crate) unsafe fn end(&mut self) {
        let o = if self.len < self.cap { self.len } else { self.cap - 1 };
        write_volatile(self.buf.add(o), 0);
    }
}

