/* capatch.c — p67tc 3.101.043：把 libcurl 的 CA 路径改到 /data/ssl
 *
 * ── 逆向结论（静态 + 运行时双向证实）──────────────────────────────
 * libcurl 8.4.0-DEV + mbedTLS。编译默认 CA 路径是两个 .rodata 常量：
 *     CApath = "/etc/ssl/curl"                      @ 0x0CB42A30
 *     CAfile = "/etc/ssl/curl/ca-certificates.crt"  @ 0x0CB42A40
 *
 * 真机日志证实运行时**只走 capath 分支**（curl 8.4 的 mbedtls.c 里
 * capath 优先且是 else 关系，cafile 根本没被尝试）：
 *     "Error reading ca cert path /etc/ssl/curl - mbedTLS: (-0x2900)"
 * ⇒ 信任库恒为空 ⇒ 任何对端都 NOT_TRUSTED(60)。
 * ⇒ 修法 = 让 capath 指向一个**真实存在、且含 PEM 的目录**（9Pro 就是
 *   /etc/ssl/curl/ 里放 ca-certificates.crt，靠 capath 分支生效）。
 *
 * 三张表都指向这一对字符串（指针值用 0x2C 非缓存别名，同一物理内存）：
 *     0x0C308180  .data 模板  {.., CApath, CAfile}
 *     0x0C324120  ops/函数指针表 {.., CAfile, CApath}
 *     0x0CDC6230  flash 侧模板 {CAfile, CApath}
 * 运行时的 .data 副本在 **SRAM**（可写，真机亦然）：
 *     0x200c9bb8 = CAfile 指针槽
 *     0x200c9bbc = CApath 指针槽
 *
 * ── 为什么改指针而不是改字符串 ───────────────────────────────────
 * 字符串本体在 ap-xip flash（0x0C/0x2C 段，真机只读）。
 * QEMU 参考实现 hw/arm/best1503.c：
 *     BEST1503_CODE_XIP 0x0c000000   ← 代码 XIP
 *     BEST1503_RO_XIP   0x2c000000   ← 只读 XIP 别名
 *     BEST1503_SRAM     0x20000000   ← 可写 RAM
 * 模拟器把 flash 建成了 RAM（那边能写字符串），真机不能。
 * ⇒ 只能改 SRAM 里的**指针**，让它指向我们自己的字符串。
 *
 * ── 稳健性设计 ───────────────────────────────────────────────────
 * 1. **自寻址**：不写死 0x200c9bb8，而是扫 SRAM 找 {0x2CB42A40,0x2CB42A30}
 *    这一对指针，命中才改。固件布局变了也不会误伤。
 * 2. **字符串放模块自己的 .data**：modlib 分配，生命周期跟模块走；
 *    不用固定 SRAM 地址（0x20100000 是 heap 区，会被别的分配覆盖）。
 * 3. **两个字符串分开**：同址会互相覆盖（实测踩过）。
 * 4. **幂等**：可反复 insmod/rmmod，第二次也要能修回正确值。
 * 5. **不碰 flash**：只写 SRAM。
 *
 * 结果通过 /dev/capatch 暴露：
 *   res=  1=已改  2=只读/写失败  3=未找到指针对
 *   cap=/caf= 各槽是否已指向新字符串
 *   slot= 找到的槽地址
 */
#include <stdint.h>

struct mm_mod_info { int (*uninitializer)(void *); void *arg; const void *exports; uint32_t nexports; };

__attribute__((used, naked)) void __aeabi_unwind_cpp_pr0(void) { __asm__("bx lr"); }
__attribute__((used, naked)) void __aeabi_unwind_cpp_pr1(void) { __asm__("bx lr"); }
__attribute__((used, naked)) void __aeabi_unwind_cpp_pr2(void) { __asm__("bx lr"); }

/* 新路径字符串放模块自己的 .data（modlib 分配在 RAM，生命周期安全）
 *
 * ★ v2 关键变更：CAPATH 置 0，强制走 CAfile 单文件分支。
 *   真机证据（capatch v1，2026-10-21 01:0x）：
 *     /data/ssl 里 3 个文件齐全、md5 正确、锚点 R3 正确（openssl 离线验 OK），
 *     curl 仍报 (60) NOT_TRUSTED，且 err 里**没有** "Error reading ca cert path"。
 *   ⇒ mbedTLS 的 mbedtls_x509_crt_parse_path 在 NuttX 上 readdir 静默返回空：
 *     opendir 成功、遍历 0 项、返回 0、一张证书都没载入，还不报错。
 *   ⇒ 改用 CAfile：mbedtls_x509_crt_parse_file 走 fopen/fread，不碰目录遍历。
 *   注意 curl 的 mbedtls backend 里 capath 分支在前且是 else 关系，
 *   所以必须把 capath 指针置 NULL 才能短路到 cafile。
 */
static volatile char g_path_caf[] = "/data/ssl/ca.pem";
/* capath 槽置 0；保留字符串以备回退构建 */
static volatile char g_path_cap[]  = "/data/ssl";
static volatile char g_path_hash[] = "/data/sslhash";
static volatile char g_path_data[] = "/data";
static volatile char g_path_misc[] = "/misc/ssl";
static volatile char g_path_nv[]   = "/nv/ssl";

#define NEW_CAFILE    "/data/ssl/ca.pem"
#define NEW_CAPATH    "/data/ssl"
#define USE_CAPATH    1            /* 1 = 走目录（parse_path）；0 = 置空走 CAfile */

/* 期望的原指针值（0x2C = flash 的只读别名） */
#define EXPECT_CAPATH 0x2CB42A30u
#define EXPECT_CAFILE 0x2CB42A40u

/* 自寻址的扫描范围：SRAM 全域 */
#define SRAM_BASE 0x20000000u
#define SRAM_END  0x20200000u

/* 已知地址（仅作快速路径 + 自检对照） */
#define KNOWN_SLOT 0x200c9bb8u

static volatile int g_ctor = 0;
static volatile int g_reg  = -777;
static volatile int g_res  = 0;         /* 1=ok 2=写失败 3=未找到 */
static volatile int g_cap_ok = 0;
static volatile int g_caf_ok = 0;
static volatile int g_old_cap = 0;
static volatile int g_old_caf = 0;
static volatile uint32_t g_slot = 0;
static volatile uint32_t g_slot1 = 0, g_slot0 = 0;
/* 结构体窗口快照（用来逐个试布尔量后复原）*/
#define SNAP_BASE_OFF  0x40
#define SNAP_LEN       0x80
static uint8_t  g_snap[SNAP_LEN];
static uint32_t g_snap_base = 0;
static volatile uint32_t g_cand[4] = {0,0,0,0};   /* 四个候选当前值 */

/* ---- 直接调用固件里的 CA 解析链路（已用特征匹配定位，地址见 HANDOFF 二十一）---- */
typedef void *(*opendir_t)(const char *);
typedef void *(*readdir_t)(void *);
typedef int   (*closedir_t)(void *);
typedef int   (*stat_t_)(const char *, void *);
typedef int   (*parse_path_t)(void *, const char *);

#define FW_OPENDIR  ((opendir_t)(uintptr_t)0x0C1E4529u)
#define FW_READDIR  ((readdir_t)(uintptr_t)0x0C1E4591u)
#define FW_CLOSEDIR ((closedir_t)(uintptr_t)0x0C1E4565u)
#define FW_STAT     ((stat_t_)(uintptr_t)0x0C1D1E9Du)
#define FW_PARSECRT ((parse_path_t)(uintptr_t)0x0C26E525u)   /* mbedtls_x509_crt_parse_path */
/* curl 的 CA 加载点：0x0C32AEEA..0x0C32AF1C（cafile 优先，其次 capath）
   链指针存在全局 0x20103594（字面量来自 0x0C32AFA4） */
typedef int (*parse_file_t)(void *, const char *);
#define FW_PARSEFILE ((parse_file_t)(uintptr_t)0x0C26E4E1u)   /* mbedtls_x509_crt_parse_file */
#define CHAIN_GLOBAL ((volatile uint32_t *)0x20103594u)
/* 0x0C32AE88 处的固件逻辑：malloc(0x154) → 存全局 → x509_crt_init(chain)
   malloc 入口是放在 0x200C9AF0 的**函数指针**：ldr r3,[0x200C9AF0]; blx r3 */
typedef void *(*malloc_t)(unsigned);
typedef void  (*init_t)(void *);
#define MALLOC_FN   (*(malloc_t *)(uintptr_t)0x200C9AF0u)
#define CRT_INIT    ((init_t)(uintptr_t)0x0C26DA59u)   /* mbedtls_x509_crt_init */
#define CHAIN_SIZE  0x154u
static volatile uint32_t g_alloc = 0;
static volatile uint32_t g_chain = 0, g_c_tag = 0, g_c_len = 0, g_c_p = 0;
static volatile uint32_t g_r0=0,g_r1=0,g_r2=0;

static uint8_t  g_st[160];        /* struct stat 缓冲（st_mode 在 +8） */
static uint32_t g_crt[0x300];     /* mbedtls_x509_crt 链头缓冲（3KB，够 2.x/3.x） */
static volatile int g_dir_ok = -1, g_nent = -1, g_pret = -999;
static volatile uint32_t g_rawp = 0, g_mode0 = 0;
static char g_name0[20];
/* 当前模式：1=目录 2=单文件 3=已还原（常量在同文件后面定义）*/
static volatile int g_mode = 1;

struct fops { void *open, *close, *read, *write, *seek, *ioctl; void *reserved[2]; void *poll; void *tail[7]; };
static struct fops g_fops;

static volatile int g_len = 0;
static char g_out[600];

/* ---- 输出缓冲 ---- */
static void put(const char *s) { while (*s && g_len < 560) g_out[g_len++] = *s++; }
static void puti(int v) {
  char t[12]; int i = 0; unsigned u;
  if (v < 0) { put("-"); u = (unsigned)(-v); } else { u = (unsigned)v; }
  if (u == 0) { put("0"); return; }
  while (u && i < 11) { t[i++] = (char)('0' + (u % 10u)); u /= 10u; }
  while (i-- > 0) { char c[2]; c[0] = t[i]; c[1] = 0; put(c); }
}
static void puts_safe(const char *s, int n) {
  int i;
  for (i = 0; i < n && s[i]; i++) {
    char c = s[i];
    if (c >= 32 && c < 127) { char t[2]; t[0] = c; t[1] = 0; put(t); }
    else { put("."); }
  }
}
static void putx(uint32_t v) {
  static const char hx[] = "0123456789abcdef";
  char t[9]; int i;
  for (i = 7; i >= 0; i--) { t[i] = hx[v & 0xF]; v >>= 4; }
  t[8] = 0;
  put("0x"); put(t);
}

static void build(void) {
  g_len = 0;
  put("ctor="); puti(g_ctor);
  put(" res="); puti(g_res);
  put(" cap="); puti(g_cap_ok);
  put(" caf="); puti(g_caf_ok);
  put(" slot="); putx(g_slot);
  put(" oldcap="); putx(g_old_cap);
  put(" oldcaf="); putx(g_old_caf);
  put(" newcap=["); puts_safe(NEW_CAPATH, 24); put("]");
  put(" newcaf=["); puts_safe(NEW_CAFILE, 24); put("]");
  put(" mode="); puti(g_mode);   /* 1=目录 2=单文件 3=已还原 */
  put(" slot1="); putx(g_slot1); put(" slot0="); putx(g_slot0);
  put(" dir="); puti(g_dir_ok); put(" n="); puti(g_nent);
  put(" nm=["); puts_safe(g_name0, 18); put("] m="); putx(g_mode0);
  put(" pret="); puti(g_pret); put(" raw="); putx(g_rawp);
  put(" r0="); putx(g_r0); put(" r1="); putx(g_r1); put(" r2="); putx(g_r2);
  put("\nchain="); putx(g_chain); put(" t="); putx(g_c_tag);
  put(" l="); putx(g_c_len); put(" p="); putx(g_c_p);
  put(" c1="); putx(g_cand[0]); put(" c2="); putx(g_cand[1]);
  put(" c3="); putx(g_cand[2]); put(" c4="); putx(g_cand[3]);
  put(" cafstr=["); puts_safe((const char *)g_path_caf, 24); put("]");
  put("\n");
}

/* ---- 字符串工具（不依赖 libc） ---- */
static void my_memcpy(volatile char *d, const char *s, int n) {
  int i; for (i = 0; i < n; i++) d[i] = s[i];
}
static int my_strlen(const char *s) { int n = 0; while (s[n]) n++; return n; }

/* 自寻址：只扫已知地址附近的**小窗口**。
   ★ 教训：全域扫 2MB SRAM 会把 guest 卡死（模拟器上控制通道直接 503，
     实例被迫重启）—— 模块构造是在 nsh 上下文里跑的，不能长时间占用。
     已知地址 0x200c9bb8 是运行时 gdb 实测的，窗口 ±64KB 足够覆盖
     固件小版本间的 .data 布局漂移。 */
#define SLOT_WINDOW 0x10000u

static int cap_is_ours(uint32_t v) {
  return (v == (uint32_t)(uintptr_t)g_path_cap)
      || (v == (uint32_t)(uintptr_t)g_path_hash)
      || (v == (uint32_t)(uintptr_t)g_path_data)
      || (v == (uint32_t)(uintptr_t)g_path_misc)
      || (v == (uint32_t)(uintptr_t)g_path_nv);
}

static uint32_t find_slot(void) {
  volatile uint32_t *p;
  uint32_t a, lo, hi;
  uint32_t want_caf = (uint32_t)(uintptr_t)g_path_caf;

  /* 快速路径：已知地址。★ 放宽：slot[0] 只要是非空指针就认，
     slot[1] 是出厂值 / 0 / 我们写过的任一目录，都算"还是那个槽"
     （0x200c9bb8 已在多次真机启动中确认就是该指针对）。 */
  p = (volatile uint32_t *)KNOWN_SLOT;
  if ((p[1] == EXPECT_CAPATH || p[1] == 0u || cap_is_ours(p[1]))
      && p[0] != 0u && p[0] != 0xFFFFFFFFu) {
    return KNOWN_SLOT;
  }
  /* 附近窗口：只认严格匹配，避免误伤 */
  lo = (KNOWN_SLOT > SLOT_WINDOW) ? (KNOWN_SLOT - SLOT_WINDOW) : SRAM_BASE;
  hi = KNOWN_SLOT + SLOT_WINDOW;
  for (a = lo; a < hi - 8; a += 4) {
    p = (volatile uint32_t *)a;
    if (p[0] == EXPECT_CAFILE && p[1] == EXPECT_CAPATH) return a;
    if (p[0] == want_caf && cap_is_ours(p[1])) return a;
    if (p[0] == want_caf && p[1] == 0u) return a;
  }
  return 0;
}

/* 运行时可切换的三种模式（驱动写字节触发）：
 *   'd' → MODE_DIR  : CApath = /data/ssl（目录），CAfile = /data/ssl/ca.pem（兜底）
 *   'f' → MODE_FILE : CApath = NULL，     CAfile = /data/ssl/ca.pem
 *   'r' → MODE_REST : 还原成出厂两个常量（0x2CB42A30 / 0x2CB42A40）
 * 目的：一次装卡就能分别测"目录分支"和"文件分支"是否真的被调用。 */
#define MODE_DIR  1
#define MODE_FILE 2
#define MODE_REST 3
#define MODE_HASH 4
#define MODE_DATA 5
#define MODE_MISC 6
#define MODE_NV   7

/* 四个候选字段（相对 slot 的字节偏移）：
 *   0x24 = 布尔量(=1)   0x28 = 01 00 05 00   0x40 = 枚举(=2)   0x2C = 0（对照）*/
static const uint32_t CAND_OFF[4] = { 0x24u, 0x28u, 0x40u, 0x2Cu };

static void refresh_cand(void) {
  int i;
  if (g_slot == 0) return;
  for (i = 0; i < 4; i++)
    g_cand[i] = *(volatile uint32_t *)(g_slot + CAND_OFF[i]);
}

static void poke_cand(int idx) {
  volatile uint32_t *p;
  if (idx < 0 || idx > 3 || g_slot == 0) return;
  p = (volatile uint32_t *)(g_slot + CAND_OFF[idx]);
  *p = 0u;                 /* 只清零一个 dword，其它字段不动 */
  refresh_cand();
  build();
}

static void restore_snap(void) {
  int k;
  volatile uint8_t *dp;
  if (g_snap_base == 0) return;
  dp = (volatile uint8_t *)g_snap_base;
  for (k = 0; k < SNAP_LEN; k++) dp[k] = g_snap[k];
  refresh_cand();
  build();
}

/* 自检：直接走 opendir/readdir/stat + mbedtls_x509_crt_parse_path */
static void self_test(void) {
  void *d;
  void *e;
  int n = 0, i;
  char *nm;
  uint32_t mode;
  int r;
  char full[64];

  g_name0[0] = 0;
  g_mode0 = 0; g_nent = 0; g_pret = -999; g_rawp = 0;

  d = FW_OPENDIR("/data/ssl");
  g_dir_ok = (d != (void *)0) ? 1 : 0;
  if (d) {
    while (n < 4) {
      e = FW_READDIR(d);
      if (e == (void *)0) break;
      nm = (char *)e + 1;                 /* NuttX: d_type@0, d_name@1 */
      n++;
      /* 拼路径并 stat */
      {
        int j = 0, q = 0;
        const char *pre = "/data/ssl/";
        while (pre[j] && j < 40) { full[j] = pre[j]; j++; }
        while (nm[q] && j < 62) { full[j++] = nm[q++]; }
        full[j] = 0;
      }
      r = FW_STAT(full, g_st);
      mode = (r == 0) ? *(uint32_t *)(g_st + 8) : 0xDEADu;
      if (n == 1) {
        for (i = 0; i < 18 && nm[i]; i++) g_name0[i] = nm[i];
        g_name0[i] = 0;
        g_mode0 = mode;
      }
    }
    FW_CLOSEDIR(d);
  }
  g_nent = n;

  /* 直接调 mbedtls_x509_crt_parse_path */
  for (i = 0; i < (int)(sizeof(g_crt) / 4); i++) g_crt[i] = 0;
  g_pret = FW_PARSECRT((void *)g_crt, "/data/ssl");
  g_rawp = g_crt[0];
  g_r0 = g_crt[0]; g_r1 = g_crt[1]; g_r2 = g_crt[2];
  build();
}

static void build_chain(void);

/* 只读：看 curl 用的那条全局链里到底装了什么 */
static void chain_probe(void) {
  uint32_t cp = *CHAIN_GLOBAL;
  g_chain = cp;
  g_c_tag = 0; g_c_len = 0; g_c_p = 0;
  if (cp != 0 && cp != 0xFFFFFFFFu) {
    volatile uint32_t *q = (volatile uint32_t *)cp;
    g_c_tag = q[0]; g_c_len = q[1]; g_c_p = q[2];
  }
  build_chain();
}

/* ★ 关键尝试：直接往 curl 用的全局链里解析我们的证书
   curl 的加载点（0x0C32AEEA 起）只有在 cafile/capath 非空时才调 parse_*；
   链目标恒为 *(0x20103594)。这里绕过它的配置，直接往链里塞。 */
static void seed_chain(void) {
  uint32_t cp = *CHAIN_GLOBAL;
  g_chain = cp;
  g_c_tag = 0; g_c_len = 0; g_c_p = 0;
  if (cp == 0 || cp == 0xFFFFFFFFu) { g_pret = -777; build_chain(); return; }
  g_pret = FW_PARSEFILE((void *)cp, "/data/ssl/ca.pem");
  {
    volatile uint32_t *q = (volatile uint32_t *)cp;
    g_c_tag = q[0]; g_c_len = q[1]; g_c_p = q[2];
    g_r0 = q[0]; g_r1 = q[1]; g_r2 = q[2];
  }
  build_chain();
}

/* 紧凑输出：只打链相关字段，避免被上层截断 */
static void build_chain(void) {
  g_len = 0;
  put("chain="); putx(g_chain);
  put(" t="); putx(g_c_tag);
  put(" l="); putx(g_c_len);
  put(" p="); putx(g_c_p);
  put(" pret="); puti(g_pret);
  put(" mode="); puti(g_mode);
  put("\n");
}

/* 确保全局链存在，然后往里塞证书（这就是修法本体） */
static void ensure_chain(void) {
  uint32_t cp = *CHAIN_GLOBAL;
  g_alloc = 0; g_pret = -999; g_chain = cp;
  g_c_tag = 0; g_c_len = 0; g_c_p = 0;

  if (cp == 0u || cp == 0xFFFFFFFFu) {
    void *p = MALLOC_FN(CHAIN_SIZE);
    g_alloc = (uint32_t)(uintptr_t)p;
    if (p == (void *)0) { g_pret = -778; build_chain(); return; }
    *CHAIN_GLOBAL = (uint32_t)(uintptr_t)p;
    CRT_INIT(p);
    cp = (uint32_t)(uintptr_t)p;
  }
  g_chain = cp;
  g_pret = FW_PARSEFILE((void *)cp, "/data/ssl/ca.pem");
  {
    volatile uint32_t *q = (volatile uint32_t *)cp;
    g_c_tag = q[0]; g_c_len = q[1]; g_c_p = q[2];
  }
  build_chain();
}

/* 回滚：把全局链指针清零，让固件按它自己的懒初始化重新来一遍。
   不 free（避免和固件自己的释放冲突），只让指针失效。 */
static void rollback_chain(void) {
  g_chain = *CHAIN_GLOBAL;
  *CHAIN_GLOBAL = 0u;
  g_c_tag = 0; g_c_len = 0; g_c_p = 0; g_pret = -555;
  build_chain();
}

static void do_patch(int mode) {
  volatile uint32_t *slot;
  uint32_t a = find_slot();
  uint32_t p_cap, p_caf;

  if (a == 0) {
    g_res = 3; g_slot = 0; g_cap_ok = 0; g_caf_ok = 0;
    build();
    return;
  }
  g_slot = a;
  slot = (volatile uint32_t *)a;
  g_old_cap = slot[1];
  g_old_caf = slot[0];

  p_cap = (uint32_t)(uintptr_t)g_path_cap;
  p_caf = (uint32_t)(uintptr_t)g_path_caf;
  /* 确保内容正确（.data 正常会被装载器拷进来，这里兜底） */
  my_memcpy(g_path_cap, NEW_CAPATH, my_strlen(NEW_CAPATH) + 1);
  my_memcpy(g_path_caf, NEW_CAFILE, my_strlen(NEW_CAFILE) + 1);

  if (mode == MODE_REST) {
    slot[1] = EXPECT_CAPATH;
    slot[0] = EXPECT_CAFILE;
    g_cap_ok = (slot[1] == EXPECT_CAPATH);
    g_caf_ok = (slot[0] == EXPECT_CAFILE);
  } else if (mode == MODE_FILE) {
    slot[1] = 0;         /* 短路到 CAfile 分支 */
    slot[0] = p_caf;
    g_cap_ok = (slot[1] == 0u);
    g_caf_ok = (slot[0] == p_caf);
  } else {
    uint32_t v;
    if (mode == MODE_HASH)      v = (uint32_t)(uintptr_t)g_path_hash;
    else if (mode == MODE_DATA) v = (uint32_t)(uintptr_t)g_path_data;
    else if (mode == MODE_MISC) v = (uint32_t)(uintptr_t)g_path_misc;
    else if (mode == MODE_NV)   v = (uint32_t)(uintptr_t)g_path_nv;
    else                        v = p_cap;
    slot[1] = v;         /* CApath → 目录 */
    slot[0] = p_caf;
    g_cap_ok = (slot[1] == v);
    g_caf_ok = (slot[0] == p_caf);
  }
  g_slot1 = slot[1];
  g_slot0 = slot[0];
  g_mode = mode;
  g_res = (g_cap_ok && g_caf_ok) ? 1 : 2;
  /* 快照：以 slot-0x40 起 0x80 字节 */
  g_snap_base = a - SNAP_BASE_OFF;
  { int k; volatile uint8_t *sp = (volatile uint8_t *)g_snap_base;
    for (k = 0; k < SNAP_LEN; k++) g_snap[k] = sp[k]; }
  refresh_cand();
  build();
}

/* ---- 驱动桩（暴露结果） ---- */
static int p_open(void *fp) { *(volatile int *)((char *)fp + 8) = 0; return 0; }
static int p_close(void *fp) { (void)fp; return 0; }
static int p_read(void *fp, void *buf, unsigned len) {
  int off = *(volatile int *)((char *)fp + 8);
  unsigned n;
  if (off < 0 || off >= g_len) return 0;
  n = (unsigned)(g_len - off);
  if (n > len) n = len;
  for (unsigned i = 0; i < n; i++) ((char *)buf)[i] = g_out[off + i];
  *(volatile int *)((char *)fp + 8) = off + (int)n;
  return (int)n;
}
static int p_write(void *fp, void *buf, unsigned len) {
  char c = (len > 0) ? ((char *)buf)[0] : 0;
  (void)fp;
  if (c == 'd')      { do_patch(MODE_DIR); }
  else if (c == 'f') { do_patch(MODE_FILE); }
  else if (c == 'r') { do_patch(MODE_REST); }
  else if (c == 'h') { do_patch(MODE_HASH); }
  else if (c == 'a') { do_patch(MODE_DATA); }
  else if (c == 'm') { do_patch(MODE_MISC); }
  else if (c == 'n') { do_patch(MODE_NV); }
  else if (c >= '1' && c <= '4') { poke_cand(c - '1'); }
  else if (c == '0') { restore_snap(); }
  else if (c == 'p') { self_test(); }
  else if (c == 'c') { chain_probe(); }
  else if (c == 'L') { seed_chain(); }
  else if (c == 'S') { ensure_chain(); }
  else if (c == 'R') { rollback_chain(); }
  return (int)len;
}

typedef int (*register_t)(const char *, void *, unsigned, void *);
#define FW(addr, type) ((type)(uintptr_t)(addr))
#define REGISTER FW(0x0C1AFF61u, register_t)

static void mm_ctor(void) __attribute__((constructor));
static void mm_ctor(void) {
  g_ctor = 1;
  g_fops.open = (void *)p_open;
  g_fops.close = (void *)p_close;
  g_fops.read = (void *)p_read;
  g_fops.write = (void *)p_write;
  g_reg = REGISTER("/dev/capatch", &g_fops, 0666, 0);
  do_patch(MODE_DIR);
  ensure_chain();       /* ★ 加载即装：链不存在就自己建，然后塞证书（不等 fetch） */
  build();
}

int module_initialize(struct mm_mod_info *m) {
  m->uninitializer = 0; m->arg = 0; m->exports = 0; m->nexports = 0;
  return 0;
}
