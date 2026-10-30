/* mpu_trap.h -- p67tc 3.101.043 resolver-entry trap (PMSAv8 XN -> MemManage).
 *
 * Why this exists
 * ---------------
 * p67tc 3.101.043 has no DNS client.  The only name-resolution primitive in
 * the whole image is
 *      0x0C1E9448  BL 0x0C1E8E64          (inside getaddrinfo 0x0C1E9314)
 *      0x0C1E8E64  gethostentbynamer      (numeric literals + "localhost" only)
 * and it is reached by a *direct* branch: no RAM pointer to rewrite, the BES
 * ROM patch unit is read-only, and the FPB comparators cannot be enabled on
 * this silicon.  The only writable control point left is the exception vector
 * table (VTOR = 0x20076400 lives in RAM) plus the PMSAv8 MPU.
 *
 * Lesson from the 2026-10-14 crash (log 29598 #10)
 * ------------------------------------------------
 * The very first device run proved the XN region works and the trap fires at
 * 0x0C1E8E64 with the expected arguments -- and then died with
 *      arm_hardfault: Hard Fault escalation
 *      CFSR = 0x00000001 (IACCVIOL), PC = 0x0C1E8E64, LR = <module>
 * i.e. the MemManage was never delivered to this handler.  Three defects were
 * fixed here, and the whole thing is now a *verified ladder* instead of a
 * blind arm:
 *   1. EVERY handler exit must clear MMFSR and SHCSR.MEMFAULTPENDED.  The
 *      fault status is sticky; leaving it set re-asserts the fault.
 *   2. An unexpected trap PC must not just "replay the instruction": it must
 *      roll the whole trap back (region off, MPU restored, vector restored,
 *      status cleared) so a mistake costs one fault, never the system.
 *   3. The arm path is staged and self-verifying.  It refuses to enable
 *      anything until the previous stage has been proven on the device, and
 *      it includes a synthetic MemManage round trip (execute a 4-byte stub
 *      inside an XN region and check that the fault was delivered, handled and
 *      recovered from) BEFORE the resolver window is armed.  If the fault
 *      delivery cannot be proven, the trap is never armed -- which is exactly
 *      the failure that crashed the device.
 *
 * Mechanism
 * ---------
 *  1. Mark the 32-byte PMSAv8 granule 0x0C1E8E60..0x0C1E8E7F execute-never.
 *  2. Point the MemManage vector at mt_handler().
 *  3. Two PCs are reachable inside that window (proven by a whole-image branch
 *     scan: exactly one BL targets the entry, no branch anywhere targets the
 *     window): 0x0C1E8E60 (previous routine's epilogue pop {r4-r7} ; bx lr)
 *     and 0x0C1E8E64 (gethostentbynamer entry).
 *  4. For 0x0C1E8E60 the handler resumes through mt_resume_neighbour(), which
 *     *is* those two instructions, replayed with the caller's own SP/LR (the
 *     exception return already restored both).
 *  5. For 0x0C1E8E64 the handler reads the register frame, looks the name up
 *     in a cache filled by the module's own DNS state machine, and then *calls
 *     the firmware routine itself* with a dotted-decimal string.  That keeps
 *     the firmware's private result layout (the "extended hostent" with
 *     family/length/address arrays) and its error protocol intact; we never
 *     re-implement either.  On success the cached name is written back over
 *     the numeric copy so h_name stays the real host name.
 *  6. Any anomaly falls back to calling the firmware routine with the caller's
 *     own arguments (exact pre-trap behaviour) or to a full rollback.
 *
 * Ordering rules (never invert):
 *   handler installed BEFORE the MPU is allowed to enforce a region;
 *   region cleared BEFORE the vector is restored.
 */
#ifndef P67TC_MPU_TRAP_H
#define P67TC_MPU_TRAP_H

#include <stdint.h>

/* ---- firmware control points ------------------------------------------ */
/* Geometry matters more than anything else here: a PMSAv8 region is at least
 * 32 bytes, so the trap granule is 0x0C1E8E60..0x0C1E8E7F and the resume point
 * after the trapped call must lie OUTSIDE it.  The resolver's entry is the only
 * such site: its single caller returns to 0x0C1E944C, four granules away, so
 * the handler can complete the call itself and let the firmware continue
 * without ever re-fetching the XN granule.  (Trapping the caller's BL instead
 * cannot work: 0x0C1E944C is inside that BL's own granule, and the frame LR at
 * a BL fetch is still the *enclosing* function's, so no frame can ever match.) */
#define MT_ENTRY_ADDR      0x0C1E8E64u   /* gethostentbynamer entry == fault site */
#define MT_NEIGHBOUR_ADDR  0x0C1E8E60u   /* previous routine's epilogue, same granule */
#define MT_TRAP_BASE       0x0C1E8E60u   /* the 32-byte PMSAv8 granule holding both */
#define MT_TRAP_END        0x0C1E8E7Fu
/* ★★ v98：第二个钩子 —— iob_navail（0x0C1F6504），让它返回 navail+64。
 * 目的：固件里 g_iob_count 保持 24（iob_free 的 DEBUGASSERT 永不触发），
 * 只让"算窗口的那次读"得到 88 ⇒ 窗口 = min(65535, (24+64)*196) 再由 s_rcvbufs 夹到 16384。
 * 实际生效位置已反汇编确认：0x0C2006AA 处 `movs r0,#1; bl 0xC1F6504`。*/
#define MT_NAVAIL_BASE     0x0C1F6500u
#define MT_NAVAIL_END      0x0C1F651Fu
#define MT_NAVAIL_ADDR     0x0C1F6504u
#define MT_NAVAIL_ADD      64u
#define MT_WORD_NEIGHBOUR  0x4770BCF0u   /* f0 bc 70 47 : pop {r4-r7} ; bx lr */
#define MT_WORD_ENTRY      0x4FF0E92Du   /* 2d e9 f0 4f : push.w {r4-r8,sb,sl,fp,lr} */
#define MT_CALLSITE_ADDR   0x0C1E9440u   /* the block holding the only BL */
#define MT_BL_SITE         0x0C1E9448u   /* the only BL into the entry */
#define MT_BL_WORD         0xFD0CF7FFu   /* ff f7 0c fd */
#define MT_ENTRY_LR        0x0C1E944Du   /* what that BL leaves in LR */
#define MT_RESUME_PC       0x0C1E944Cu   /* instruction right after the BL */

/* PMSAv8 MPU */
#define MT_MPU_TYPE        0xE000ED90u
#define MT_MPU_CTRL        0xE000ED94u
#define MT_MPU_RNR         0xE000ED98u
#define MT_MPU_RBAR        0xE000ED9Cu
#define MT_MPU_RLAR        0xE000EDA0u
#define MT_MPU_CTRL_ENABLE     0x00000001u
#define MT_MPU_CTRL_PRIVDEFENA 0x00000004u
#define MT_RLAR_EN         0x00000001u
#define MT_RLAR_ATTR_SHIFT 1u
#define MT_RBAR_XN         0x00000001u
#define MT_RBAR_AP_RW      0x00000006u   /* AP = 3 : RW at any privilege */
#define MT_ATTR_INDEX      1u            /* MAIR0 attr1, the index the firmware's
                                          * own regions 4/5 use (both Normal) */

/* SCB */
#define MT_SCB_VTOR        0xE000ED08u
#define MT_SCB_SHCSR       0xE000ED24u
#define MT_SCB_CFSR        0xE000ED28u
#define MT_SCB_MMFSR       0xE000ED28u
#define MT_SCB_HFSR        0xE000ED2Cu
#define MT_SCB_DEMCR       0xE000EDFCu
#define MT_MM_VECTOR_OFF   0x10u
#define MT_HF_VECTOR_OFF   0x0Cu
/* SHCSR: the enables are BITS 16/17/18.  That is not a guess -- the firmware's
 * own fault-enable routine loads 0xE000ED24 and does mvn r1,#0x10000 /
 * #0x20000 / #0x40000, and the device reads back 0x00070000.
 *
 * ★ THE 2026-10-15 HANG WAS THIS REGISTER ★
 * The PENDED bits (13 = MEMFAULTPENDED, 14 = BUSFAULTPENDED) are plain
 * read/write on p67tc: writing 1 SETS pending.  The old code did
 *      SHCSR |= MT_SHCSR_PEND_ANY
 * "to clear the pending state", which did the exact opposite -- measured on
 * the device as shcsr_before 0x70000 -> shcsr_after 0x76001 (bits 13/14 + bit 0
 * all went 0 -> 1).  Every handler exit therefore re-pended the very fault it
 * had just handled: the handler was re-entered forever, no dump was produced
 * and the watchdog eventually reset the band.  Pending state is cleared
 * *indirectly*, by clearing the fault status bits in CFSR (write 1 to clear).
 * Nothing in this file may ever write a SHCSR bit outside ENA_BITS.
 * (QEMU cannot reproduce this: it ignores SHCSR writes entirely.) */
#define MT_SHCSR_ENA_BITS  ((1u << 16) | (1u << 17) | (1u << 18))
#define MT_SHCSR_PEND_MASK ((1u << 13) | (1u << 14))
#define MT_SHCSR_ACT_MASK  0x0000000Fu
#define MT_MMFSR_CLEAR     0x00000081u   /* IACCVIOL | MMARVALID, write-1-to-clear */

/* ---- register frame ---------------------------------------------------- */
struct mt_ctx {
  uint32_t r4, r5, r6, r7, r8, r9, r10, r11, exc_return;
};
#define MT_F_R0   0u
#define MT_F_R1   1u
#define MT_F_R2   2u
#define MT_F_R3   3u
#define MT_F_R12  4u
#define MT_F_LR   5u
#define MT_F_PC   6u
#define MT_F_XPSR 7u
#define MT_BASIC_BYTES 0x20u
/* The basic frame is NOT always at &ctx + sizeof(struct mt_ctx): that is only
 * true when the faulting code was running on MSP.  See mt_frame_anchor(). */
#define MT_DELTA_MAX 0x90u
#define MT_NMISS     4u   /* v19：被拒候选留档条数 */
/* v21 慢路径：冷名时把调用者挂住、等模块的 DNS 客户端查完（有界自旋，靠 tick 抢占让桥转发） */
/* ★ v38：慢路径等待改成"有界 + 与实际时钟挂钩"：以**排空器（drain）的运行次数**当节拍
 * （drain 每 MT_DRAIN_TICKS=100 tick 跑一次；tick≈1ms ⇒ 每拍≈100ms）。不用"唤醒次数"
 * 计时：WFI 会被任意中断唤醒，中断风暴下会偏短。硬迭代上限兜底（drain 死了也不会无限等）。*/
/* ★ v44：60→80 拍（≈8 秒）——要覆盖"先等旧查询被看门狗清掉(≤4s) + 自己的回包 RTT(几秒)" */
#define MT_SLOW_BEATS    80u      /* 最多等 80 拍 ≈ 8 秒（drain 每 ~100ms 一拍）*/
#define MT_SLOW_SPIN_CAP 800000u  /* ★v48 绝对保险：迭代总上限（≈10-13s@实测唤醒率）*/
#define MT_SLOW_IDLE_CAP 60000u   /* ★v48 早退判据：drain 停摆 ~1s 无进展 ⇒ 不白等 */

/* Build identity: surfaced in both reports so it is always possible to tell
 * which module build is actually resident in RAM (module base addresses change
 * on every load, so they cannot be used as a version check). */
#define MT_BUILD "v108-20261024-dbg-flag"

/* ---- cache / pending --------------------------------------------------- */
#define MT_NAME_MAX   64u
#define MT_CACHE_MAX  10u
#define MT_PEND_MAX   6u    /* v26：真 FIFO 队列（以前是单槽，会互相覆盖）*/
#define MT_DOTTED_MAX 16u

struct mt_cache_entry {
  volatile uint32_t used;   /* published last: readers only see complete rows */
  uint32_t ip;              /* network byte order, byte 0 in the low bits */
  uint32_t stamp;           /* v26：LRU 时间戳（每次命中刷新）*/
  uint32_t expires;         /* ★ v45：过期节拍（drain 运行计数 ≈100ms/拍，按 9Pro 的 TTL 对齐）*/
  char name[MT_NAME_MAX];
};

enum mt_state { MT_ST_OFF = 0, MT_ST_PROBED = 1, MT_ST_ARMED = 2, MT_ST_BLOCKED = 3 };
enum mt_reason {
  MT_R_NONE = 0, MT_R_NOT_PROBED = 1, MT_R_CPUID = 2, MT_R_CALLSITE = 3,
  MT_R_CODE_WORDS = 4, MT_R_VECTOR_UNREADABLE = 5, MT_R_NO_FREE_REGION = 6,
  MT_R_MPU_TYPE = 7, MT_R_CTRL_REJECTED = 8, MT_R_SELFTEST = 9,
  MT_R_DISARMED = 10, MT_R_TRAP_STORM = 11,
  /* new in the verified ladder */
  MT_R_MEMFAULTENA_REFUSED = 12, MT_R_MPU_ENABLE_REFUSED = 13,
  MT_R_PREFLIGHT_FAILED = 14, MT_R_MM_NOT_DELIVERED = 15,
  MT_R_MM_NOT_RECOVERED = 16, MT_R_TRAP_NOT_TAKEN = 17,
  MT_R_SUBSTITUTION_FAILED = 18, MT_R_FAILSAFE = 19, MT_R_STUB_UNUSABLE = 20,
  MT_R_USER_DISARM = 21, MT_R_MM_ESCALATED = 22, MT_R_NOT_ARMED = 23,
  MT_R_TRAP_TEST_FAILED = 24, MT_R_REGION_ARMED = 25
};

struct mt_state_all {
  uint32_t state, reason, armed, stage;
  uint32_t region, rbar, rlar;
  uint32_t vector_addr, vector_before, vector_now;
  uint32_t vector_hf_addr, vector_hf_before, vector_hf_now;
  uint32_t handler_addr, hf_handler_addr, stub_addr, entry_addr, entry_ok, neighbour_ok;
  uint32_t mpu_type, mpu_dregion, mpu_ctrl_before, mpu_ctrl_after;
  uint32_t shcsr_before, shcsr_after, demcr, cfsr;
  /* shcsr_pend must read 0 on every device report: bits 13/14 are the pending
   * state and any non-zero value here means the handler is re-pending itself
   * (the 2026-10-15 hang).  cafsr_now/hfsr_now are the post-handler status. */
  uint32_t shcsr_pend, cfsr_now, hfsr_now, region_off_count;
  /* v15 frame location forensics: exc_return proves which stack the hardware
   * used; frame_anchor is where we then looked for the frame. */
  uint32_t exc_return, psp_now, msp_now, frame_anchor, frame_extra, noframe_pc;
  /* v16: 两栈搜索结果 + 一次到位的取证（三个候选位置的 PC 槽原样上报） */
  uint32_t frame_found, frame_stack, anchor_pc, psp_pc, msp_pc;
  /* v17：把现场录下来 + errp 兜底定位的结果
   *   arg[4]      = 帧里的 r0..r3（name/host/buf/blen，一眼看出帧对不对）
   *   raw_frame[8]= 帧本身的 8 个字（含未掩码的 LR/PC/xPSR）
   *   raw_above[8]= 帧基址 +0x20 起的 8 个字（我模型里"栈参数区"）
   *   errp_slot   = 第 5 参数在第几个字命中（0=模型对；>0=兜底找的；~0=没找到） */
  uint32_t arg[4], raw_frame[8], raw_above[8], errp_slot;
  /* v19：候选打分 + 落选留档 + 被拒后自动重新布防 */
  uint32_t nm[MT_NMISS][5], near_miss, frame_score, rearm_pending, rearm_count;
  /* v21 慢路径计数（冷名：多做一次中转 + 等解析完成） */
  uint32_t last_blen, psp_ok, msp_ok, dns_abort_req, dns_abort_reqs;
  uint32_t slow_enabled, slow_active, slow_entries, slow_calls, slow_hits,
           slow_timeouts, slow_spins_last;
  /* ★ v38：陷阱现场"能不能安全等待"的两个判据（一直记录，不猜）——
   *   basepri_at_trap/primask：非 0 = 调用者在临界区（调度器被屏蔽）⇒ 绝不能阻塞；
   *   nl_*：调用者线程是否持着 g_netlock（holder@+0xC / count@+0x10 @0x200C89D4）。
   *   慢路径现场另记一份（slow_*），并统计"跳过了多少次/释放过多少次锁"。 */
  uint32_t basepri_at_trap, primask_at_trap, nl_cnt, nl_holder, nl_me, nl_mine,
           nl_mine_yes, nl_mine_no, crit_at_trap;
  uint32_t slow_basepri_last, slow_primask_last, slow_nl_cnt, slow_nl_holder,
           slow_nl_me, slow_skip_crit, slow_skip_noread, slow_nl_released,
           slow_wait_beats, slow_skip_netdown;
  uint32_t traps, entry_traps, neigh_traps, unknown_traps, noframe_traps;
  uint32_t hf_traps, hf_fixed, hf_foreign, streak, hf_pc, hf_lr;
  uint32_t nested_faults, skips, last_skip_pc;
  /* ★ v40：嵌套故障（handler 内再故障）的现场与风暴计数 —— 诊断 + 硬复位兜底用 */
  uint32_t nested_pc, nested_storm;
  uint32_t hits, misses, subst_ok, subst_fail, passthrough;
  uint32_t frame_delta, caller_sp_last, lr_last, errp_last, flags_last;
  uint32_t arm_count, disarm_count, failsafe_count;
  uint32_t preflight_rc, mm_roundtrip, mm_traps, trap_test_rc, subst_rc;
  uint32_t mm_stub_addr, mm_stub_seen, mm_pc;
  uint32_t cache_count, pend_seq, pend_done, cmd_seen, region_scan_ok;
  uint32_t regions[8][2];
};

static struct mt_state_all g_mt;
/* v37 排空器运行计数（定义处：tun_module.c 的 worker 里自增；v38 起慢路径用它当节拍源，
 * v45 起缓存 TTL 也用它当时间基）。必须在缓存函数之前声明。*/
static volatile uint32_t g_drain_runs;
static struct mt_cache_entry g_mt_cache[MT_CACHE_MAX];
/* v26：待解析区 = 6 槽 FIFO（原来是单槽，后来的名字会覆盖先前的）*/
struct mt_pend_slot { volatile uint32_t seq; char name[MT_NAME_MAX]; };
static struct mt_pend_slot g_mt_pend_q[MT_PEND_MAX];
static volatile uint32_t g_mt_pend_head, g_mt_pend_tail;
static volatile uint32_t g_mt_pend_seq, g_mt_pend_seq_done, g_mt_pend_drops;
/* v36：每个槽的重试计数（看门狗超时抛弃时 +1 后回到队尾；耗尽才丢弃）*/
static volatile uint8_t g_mt_pend_tries[MT_PEND_MAX];
static volatile uint32_t g_mt_cache_tick;
static volatile uint32_t g_mt_cmd;
static volatile uint32_t g_mt_cmd_seen;
/* Non-zero while mt_dispatch/mt_hf_dispatch is on the stack.  A fault that
 * arrives with this already set happened *inside our own handler*, which is a
 * bug -- and, left alone, it retries the faulting instruction forever (that is
 * how the 2026-10-15 hang happened: hang, then watchdog reset, no dump). */
static volatile uint32_t g_mt_depth;

/* Executable stub used for the synthetic MemManage round trip: it must live
 * in a 32-byte aligned window so a PMSAv8 region can cover exactly it.
 * 5a 20 = movs r0, #0x5a ; 70 47 = bx lr                                        */
static uint8_t g_mt_stub[32] __attribute__((aligned(32))) = {
  0x5Au, 0x20u, 0x70u, 0x47u, 0u, 0u, 0u, 0u,
  0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u,
  0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u,
  0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u };

/* ---- helpers (a module has no libc) ------------------------------------ */
static void mt_zero(void *p, unsigned n) {
  unsigned char *b = (unsigned char *)p;
  while (n--) *b++ = 0u;
}
static void mt_copy(void *d, const void *s, unsigned n) {
  unsigned char *a = (unsigned char *)d;
  const unsigned char *b = (const unsigned char *)s;
  while (n--) *a++ = *b++;
}
static unsigned mt_len(const char *s) { unsigned n = 0u; while (s[n]) ++n; return n; }
static char mt_lower(char c) { return (c >= 'A' && c <= 'Z') ? (char)(c + 32) : c; }
static int mt_same(const char *a, const char *b) {
  unsigned i = 0u;
  for (;;) {
    char x = mt_lower(a[i]), y = mt_lower(b[i]);
    if (x != y) return 0;
    if (!x) return 1;
    if (++i >= MT_NAME_MAX) return 0;
  }
}
/* Only dereference addresses that are known-mapped on p67tc: an OS ROM, the
 * boot ROM alias, the XIP flash window, SRAM (both aliases) and the module's
 * own allocation.  A MemManage handler must never take a second fault. */
static int mt_ptr_ok(uint32_t p) {
  if (p < 0x00020000u) return p >= 0x00000800u;         /* boot ROM alias */
  if (p >= 0x00276400u && p < 0x003E0000u) return 1;    /* SRAM low alias */
  if (p >= 0x0C000000u && p < 0x0D000000u) return 1;    /* XIP flash */
  if (p >= 0x20000000u && p < 0x20160000u) return 1;    /* SRAM */
  if (p >= 0x24000000u && p < 0x24100000u) return 1;    /* OS ROM */
  if (p >= 0x28000000u && p < 0x2D000000u) return 1;    /* flash data view */
  if (p >= 0x3C000000u && p < 0x3E000000u) return 1;    /* module RAM */
  return 0;
}
/* The module is loaded as two far-apart allocations (its .text and its
 * .data/.bss were observed ~10 MB apart), so a per-symbol window is useless.
 * A legitimate call into the resolver from module code always has LR in SRAM,
 * while the firmware's own call site has LR = MT_ENTRY_LR in flash. */
static int mt_in_module(uint32_t l) {
  uint32_t a = l & ~1u;
  return a >= 0x20000000u && a < 0x40000000u;
}
static int mt_read_name(uint32_t p, char *out) {
  unsigned i;
  if (!mt_ptr_ok(p)) return 0;
  for (i = 0u; i < MT_NAME_MAX; ++i) {
    char c = *(volatile const char *)(uintptr_t)(p + i);
    if (!c) { out[i] = 0; return i ? 1 : 0; }
    out[i] = c;
  }
  return 0;                                   /* not NUL-terminated in range */
}
static int mt_numeric_like(const char *s) {
  unsigned i; int dot = 0;
  for (i = 0u; s[i]; ++i) {
    if (s[i] >= '0' && s[i] <= '9') continue;
    if (s[i] == '.') { dot = 1; continue; }
    return 0;
  }
  return dot;
}

/* ---- cache ------------------------------------------------------------- */
static void mt_cache_count(void) {
  unsigned i, c = 0u;
  for (i = 0u; i < MT_CACHE_MAX; ++i) if (g_mt_cache[i].used) ++c;
  g_mt.cache_count = c;
}
__attribute__((unused)) static void mt_cache_shift(void) {
  unsigned i;
  for (i = 0u; i + 1u < MT_CACHE_MAX; ++i) {
    g_mt_cache[i].used = 0u;
    g_mt_cache[i].ip = g_mt_cache[i + 1u].ip;
    mt_copy(g_mt_cache[i].name, g_mt_cache[i + 1u].name, MT_NAME_MAX);
    g_mt_cache[i].used = g_mt_cache[i + 1u].used;
  }
}
/* ★ v45：按 9Pro（NuttX DNS 客户端按应答 TTL 过期）对齐 —— TTL 秒 → drain 节拍数。
 * ttl==0（无 TTL 信息，如探针）→ 60 秒；钳制 [10s, 300s]；×10 是 ~100ms/拍。 */
static uint32_t mt_cache_ttl_beats(uint32_t ttl_sec) {
  if (ttl_sec == 0u) ttl_sec = 60u;
  if (ttl_sec < 10u) ttl_sec = 10u;
  if (ttl_sec > 300u) ttl_sec = 300u;
  return ttl_sec * 10u;
}
static void mt_cache_insert(const char *name, const uint8_t ip[4], uint32_t ttl_sec) {
  unsigned i, slot = MT_CACHE_MAX;
  if (!name || !name[0]) return;
  for (i = 0u; i < MT_CACHE_MAX; ++i) {
    if (g_mt_cache[i].used && mt_same(g_mt_cache[i].name, name)) { slot = i; break; }
  }
  if (slot == MT_CACHE_MAX) {
    for (i = 0u; i < MT_CACHE_MAX; ++i) if (!g_mt_cache[i].used) { slot = i; break; }
  }
  if (slot == MT_CACHE_MAX) {   /* v26：LRU 淘汰（最久未被命中的那个）*/
    uint32_t lru = 0u;
    for (i = 1u; i < MT_CACHE_MAX; ++i)
      if (g_mt_cache[i].used && (!g_mt_cache[lru].used || g_mt_cache[i].stamp < g_mt_cache[lru].stamp)) lru = i;
    slot = lru;
  }
  g_mt_cache[slot].used = 0u;                     /* invalidate while writing */
  g_mt_cache[slot].ip = (uint32_t)ip[0] | ((uint32_t)ip[1] << 8) |
                        ((uint32_t)ip[2] << 16) | ((uint32_t)ip[3] << 24);
  mt_copy(g_mt_cache[slot].name, name, MT_NAME_MAX - 1u);
  g_mt_cache[slot].name[MT_NAME_MAX - 1u] = 0;
  g_mt_cache[slot].stamp = ++g_mt_cache_tick;
  g_mt_cache[slot].expires = g_drain_runs + mt_cache_ttl_beats(ttl_sec);
  g_mt_cache[slot].used = 1u;                     /* publish */
  mt_cache_count();
}
static const uint8_t *mt_cache_lookup(const char *name) {
  g_mt_cache_tick = g_mt_cache_tick + 1u;
  static uint8_t out[4];
  unsigned i, k;
  uint32_t v;
  for (i = 0u; i < MT_CACHE_MAX; ++i) {
    if (!g_mt_cache[i].used) continue;
    if ((int32_t)(g_drain_runs - g_mt_cache[i].expires) >= 0) {   /* v45：TTL 到期 ⇒ 视为未命中 */
      g_mt_cache[i].used = 0u; mt_cache_count(); continue;
    }
    if (!mt_same(g_mt_cache[i].name, name)) continue;
    v = g_mt_cache[i].ip;
    for (k = 0u; k < 4u; ++k) out[k] = (uint8_t)((v >> (8u * k)) & 0xFFu);
    return out;
  }
  return (const uint8_t *)0;
}
static void mt_format_ip(const uint8_t ip[4], char out[MT_DOTTED_MAX]) {
  unsigned o = 0u, i;
  for (i = 0u; i < 4u; ++i) {
    unsigned v = ip[i];
    if (i) out[o++] = '.';
    if (v >= 100u) out[o++] = (char)('0' + (v / 100u));
    if (v >= 10u) out[o++] = (char)('0' + ((v / 10u) % 10u));
    out[o++] = (char)('0' + (v % 10u));
  }
  out[o] = 0;
}

/* ---- pending queue (produced from the handler, drained from a task) ----- */
static void mt_pend_note_ex(const char *name, uint32_t tries) {
  uint32_t i2, dup = 0u, slot;
  if (!name || !name[0]) return;
  if (mt_numeric_like(name)) return;          /* nothing a resolver can add */
  for (i2 = g_mt_pend_head; i2 != g_mt_pend_tail; i2 = (i2 + 1u) % MT_PEND_MAX) {
    const char *a = g_mt_pend_q[i2].name; const char *b = name; unsigned k = 0u;
    while (a[k] && a[k] == b[k]) ++k;
    if (a[k] == b[k]) { dup = 1u; break; }    /* 已在队列里 */
  }
  if (!dup) {
    if ((g_mt_pend_tail + 1u) % MT_PEND_MAX == g_mt_pend_head) ++g_mt_pend_drops;
    else {
      slot = g_mt_pend_tail;
      g_mt_pend_seq = g_mt_pend_seq + 1u;
      mt_copy(g_mt_pend_q[slot].name, name, MT_NAME_MAX - 1u);
      g_mt_pend_q[slot].name[MT_NAME_MAX - 1u] = 0;
      g_mt_pend_q[slot].seq = g_mt_pend_seq;
      g_mt_pend_tries[slot] = (uint8_t)tries;   /* publish before tail++ */
      g_mt_pend_tail = (slot + 1u) % MT_PEND_MAX;
    }
  }
  g_mt.pend_seq = g_mt_pend_seq; g_mt.pend_done = g_mt_pend_seq_done;
}
static void mt_pend_note(const char *name) { mt_pend_note_ex(name, 0u); }
/* v36：看门狗超时后把名字放回队尾（带重试计数），而不是直接丢掉 */
static void mt_pend_requeue(const char *name, uint32_t tries) { mt_pend_note_ex(name, tries); }
/* Non-destructive: the caller commits once the query is really out, so a busy
 * or not-yet-configured network retries the same name later. */
static int mt_pend_peek(char *out, uint32_t *seq_out, uint32_t *tries_out) {
  if (g_mt_pend_head == g_mt_pend_tail) {
    g_mt.pend_seq = g_mt_pend_seq; g_mt.pend_done = g_mt_pend_seq_done;
    return 0;
  }
  mt_copy(out, g_mt_pend_q[g_mt_pend_head].name, MT_NAME_MAX - 1u);
  out[MT_NAME_MAX - 1u] = 0;
  if (seq_out) *seq_out = g_mt_pend_q[g_mt_pend_head].seq;
  if (tries_out) *tries_out = g_mt_pend_tries[g_mt_pend_head];
  g_mt.pend_seq = g_mt_pend_seq; g_mt.pend_done = g_mt_pend_seq_done;
  return 1;
}
static void mt_pend_commit(uint32_t seq) {
  if (g_mt_pend_head != g_mt_pend_tail && g_mt_pend_q[g_mt_pend_head].seq == seq) {
    g_mt_pend_tries[g_mt_pend_head] = 0u;
    g_mt_pend_head = (g_mt_pend_head + 1u) % MT_PEND_MAX;
    g_mt_pend_seq_done = seq;
  }
  g_mt.pend_done = g_mt_pend_seq_done;
}
/* ---- MMIO helpers ------------------------------------------------------ */
static uint32_t mt_rd(uint32_t a) { return *(volatile uint32_t *)(uintptr_t)a; }
static void mt_wr(uint32_t a, uint32_t v) { *(volatile uint32_t *)(uintptr_t)a = v; }
static void mt_barrier(void) {
  __asm__ volatile("dmb sy\n\tdsb sy\n\tisb sy" ::: "memory");
}
static uint32_t mt_irq_save(void) {
  uint32_t s;
  __asm__ volatile("mrs %0, primask\n\tcpsid i" : "=r"(s) :: "memory");
  return s;
}
static void mt_irq_restore(uint32_t s) {
  __asm__ volatile("msr primask, %0" :: "r"(s) : "memory");
}
/* Enable or disable ONE region, leaving RNR as we found it. */
static void mt_region_set(unsigned on) {
  uint32_t saved = mt_rd(MT_MPU_RNR), v;
  mt_wr(MT_MPU_RNR, g_mt.region);
  v = mt_rd(MT_MPU_RLAR);
  if (on) v |= MT_RLAR_EN; else v &= ~MT_RLAR_EN;
  mt_wr(MT_MPU_RLAR, v);
  mt_barrier();
  mt_wr(MT_MPU_RNR, saved);
}
/* A fault handler must always clear its own status -- but *only* its status.
 * MMFSR/BFSR/UFSR bits are write-1-to-clear, and clearing them is exactly what
 * makes the hardware drop SHCSR.<fault>PENDED for us.  Writing SHCSR's pending
 * bits by hand is what hung the band (see the SHCSR comment above), so this
 * function touches CFSR and nothing else. */
static void mt_clear_fault(void) {
  uint32_t cfsr = mt_rd(MT_SCB_CFSR);
  if (cfsr) mt_wr(MT_SCB_CFSR, cfsr);            /* W1C: drops the pending bit */
  mt_barrier();
  g_mt.cfsr_now = mt_rd(MT_SCB_CFSR);
}
/* HardFault status is its own register; a forced-escalation bit left set would
 * make the next foreign HardFault look like ours. */
static void mt_clear_hf(void) {
  uint32_t v = mt_rd(MT_SCB_HFSR);
  if (v) mt_wr(MT_SCB_HFSR, v);                  /* W1C */
  g_mt.hfsr_now = mt_rd(MT_SCB_HFSR);
}
/* Enable the three fault classes if the firmware has not (it has: measured
 * 0x70000).  OR in the enable bits ONLY, then verify -- if the enables do not
 * stick the trap cannot work and the caller must refuse to arm. */
static void mt_shcsr_ena_ensure(void) {
  uint32_t v = mt_rd(MT_SCB_SHCSR);
  uint32_t want = (v | MT_SHCSR_ENA_BITS) & ~(MT_SHCSR_PEND_MASK | MT_SHCSR_ACT_MASK);
  mt_wr(MT_SCB_SHCSR, want);
  mt_barrier();
  g_mt.shcsr_after = mt_rd(MT_SCB_SHCSR);
  g_mt.shcsr_pend = g_mt.shcsr_after & MT_SHCSR_PEND_MASK;
}

/* ---- where did the hardware put the exception frame? -------------------
 * ★ v15/v16 的核心修复 ★
 * 异常帧**不一定**在 handler 用的那个栈上：出故障的代码跑在 PSP 上时，CPU 把帧压进
 * **PSP**，然后才切到 MSP 执行我们 —— 而 mt_handler 的 `push {r4-r11,lr}` 压的是 MSP。
 * 所以"帧 = ctx + 36"只在帧本来就在 MSP 上时成立。
 *
 * v15 按 EXC_RETURN 算出了 PSP 上的帧基址（真机 `mt_exc_return=0xffffffed` ⇒ PSP +
 * FP 区，`mt_frame_extra=0x48`），**但仍然没找到帧** —— 因为那个扫描只往"上"看
 * （anchor + 0..0x90）。只要我对 extra 的估计偏大，帧就在 anchor **下面**，永远扫不到。
 *
 * ★ v16 换策略：不再赌布局。两条栈各取一个基点，在**对称**窗口（-0x60..+0xC0）
 * 里找"栈里存着的 PC 正好落在陷阱颗粒内"的那一帧。陷阱颗粒只有两个可执行落点
 * （0x0C1E8E64 入口 / 0x0C1E8E60 前邻尾声），全镜像没有别的分支指向它们，
 * 所以这个判据几乎不可能误命中；找到了，帧的地址也就顺带把"真正的帧基址"给了我们
 * （栈参数就在帧基址 + 0x20，与布局无关）。
 *
 * 窗口为什么是 -0x60..+0xC0：设 P = 异常发生前的 SP，basic 帧占 [P-0x20, P)，
 * callee 区（压了的话）在 [P-0x40,P-0x20)，FP 区（压了的话）再往下 0x48。
 * ⇒ 帧基址 = 入栈后的 SP + c + f，其中 c∈{0,0x20}、f∈{0,0x48}。
 * 以 PSP 为基点时相对我算出的 anchor(PSP+0x48) 可能是 -0x48 / -0x28 / 0 / +0x20，
 * **-0x48 恰好越过了 v16 第一版的 -0x40 下界** —— 所以下界必须放到 -0x60。 */
#define MT_FRAME_LO  (-0x60)
#define MT_FRAME_HI  ( 0xC0)

static uint32_t *mt_frame_search(struct mt_ctx *ctx, uint32_t *base_out,
                                 uint32_t *side_out) {
  uint32_t ex = ctx->exc_return, sp, anchor, extra = 0u, ctx36;
  uint32_t *found = (uint32_t *)0;
  uint32_t i, bases[2];

  g_mt.exc_return = ex;
  __asm__ volatile("mrs %0, msp" : "=r"(sp));
  g_mt.msp_now = sp;
  __asm__ volatile("mrs %0, psp" : "=r"(sp));
  g_mt.psp_now = sp;
  ctx36 = (uint32_t)(uintptr_t)ctx + sizeof(struct mt_ctx);

  /* 主锚点：纯粹按 EXC_RETURN 的位算出来（上报用，也是"跳过"路径的兜底基址） */
  anchor = (ex & 0x4u) ? g_mt.psp_now : ctx36;   /* bit2 SPSEL */
  if ((ex & 0x8u) == 0u) extra += 0x20u;         /* bit3 DCRS=0：还压了 r4-r11 */
  if ((ex & 0x10u) == 0u) extra += 0x48u;        /* bit4 FType=0：还压了 FP 区 */
  g_mt.frame_extra = extra;
  g_mt.frame_anchor = anchor + extra;

  g_mt.psp_ok = (uint32_t)mt_ptr_ok(g_mt.psp_now);
  g_mt.msp_ok = (uint32_t)mt_ptr_ok(ctx36);
  bases[0] = g_mt.psp_now;                       /* PSP 栈 */
  bases[1] = ctx36;                              /* MSP 栈（handler 那条 push 之后） */
  /* ★ v19：判据从"全中才要"改成**按可信度打分**。
   * 底线只有两条：① PC 落在**整个陷阱颗粒**（0x0C1E8E60..7F，全镜像唯一的一段）；
   *             ② LR 是 Thumb 地址且可读。
   * 其余（xPSR 是否线程模式、LR 是否正好是固件调用点的 0x0C1E944D / 是否模块地址）
   * 只用来**排序**，不再一票否决 —— v18 真机就是栽在这：真 fetch 那次抓到了
   * （mt_psp_pc=0xc1e8e64），却因为"LR 不是我以为的那个值"被否 ⇒ noframe ⇒ failsafe
   * ⇒ 陷阱被回滚，后面两次都没陷阱可用。同时把落选的候选留档，下次报告能直接看出谁否了它。 */
  {
    int32_t best_score = -1;
    uint32_t nmiss = 0u;
    for (i = 0u; i < 2u; ++i) {
      int32_t off;
      if (!mt_ptr_ok(bases[i])) continue;
      for (off = MT_FRAME_LO; off <= MT_FRAME_HI; off += 4) {
        uint32_t *c = (uint32_t *)(uintptr_t)(uint32_t)((int32_t)bases[i] + off);
        uint32_t pc = c[MT_F_PC] & ~1u, l, x;
        int32_t score;
        if (!((pc >= MT_TRAP_BASE && pc <= MT_TRAP_END) ||
              (pc >= MT_NAVAIL_BASE && pc <= MT_NAVAIL_END))) continue;   /* 颗粒内才算候选 */
        l = c[MT_F_LR]; x = c[MT_F_XPSR];
        if ((l & 1u) == 0u || !mt_ptr_ok(l & ~1u)) {           /* 底线不过 ⇒ 落选留档 */
          if (nmiss < MT_NMISS) {
            g_mt.nm[nmiss][0] = i; g_mt.nm[nmiss][1] = (uint32_t)off;
            g_mt.nm[nmiss][2] = pc; g_mt.nm[nmiss][3] = x; g_mt.nm[nmiss][4] = l;
            ++nmiss;
          }
          continue;
        }
        score = 2;                                              /* 底线已过 */
        if ((x & 0x1FFu) == 0u) score += 2;                      /* 线程模式 */
        if ((x & 0x01000000u) != 0u) score += 1;                 /* xPSR.T */
        if (pc == MT_NEIGHBOUR_ADDR) score += 2;                 /* 前邻尾声：不约束 LR */
        else if (l == MT_ENTRY_LR) score += 3;                   /* 固件唯一调用点 ✓✓ */
        else if (mt_in_module(l)) score += 2;                    /* 模块自己发起的调用 ✓ */
        if (score > best_score) {
          best_score = score;
          found = c;
          if (base_out) *base_out = bases[i];
          if (side_out) *side_out = i;
          g_mt.frame_delta = (uint32_t)off;
        }
      }
    }
    g_mt.near_miss = nmiss;
    g_mt.frame_score = (uint32_t)(best_score < 0 ? 0 : best_score);
  }
  /* 一次到位的取证：把三个候选位置的 PC 槽原样上报，下次报告就能看出帧到底在哪 */
  g_mt.anchor_pc = *(volatile uint32_t *)(uintptr_t)(g_mt.frame_anchor + 4u * MT_F_PC);
  g_mt.psp_pc = *(volatile uint32_t *)(uintptr_t)(g_mt.psp_now + 4u * MT_F_PC);
  g_mt.msp_pc = *(volatile uint32_t *)(uintptr_t)(ctx36 + 4u * MT_F_PC);
  g_mt.frame_found = (uint32_t)(uintptr_t)found;
  return found;
}

/* ---- 栈参数基址（= 异常发生前的 SP）------------------------------------
 * ★ v18 的核心修复 ★
 * 帧基址 = "硬件入栈结束后的 SP"（v16 的两栈搜索已经能稳定找到它），
 * 但 **callee/FP 区是压在"基本帧之上"的**（不是下面！），所以：
 *      参数基址 = 帧基址 + 0x20 + extra
 *      extra = 0x20(DCRS=0 压了 r4-r11) + 0x48(FType=0 压了 FP 区)
 * 真机 v17 实测（一次报告定案）：r1=hostent=sp+0x110、r2=buf=sp+0x10 两路反推
 *      sp = 0x3C4F20E8；而 mt_psp = 0x3C4F2080 ⇒ 差 0x68 = 0x20 + 0x48 ✓
 * 之前只加 0x20，指针正好落进 FP 区（全 0）⇒ 第 5 个参数读出 0 ⇒ C2 报
 * native_resolve_failed。（v15 那个 extra 算对了，只是用错了地方：它被拿去定位帧了。） */
__attribute__((used, noinline)) static uint32_t mt_csp_of(uint32_t frame,
                                                          uint32_t exc_return,
                                                          uint32_t xpsr) {
  uint32_t off = MT_BASIC_BYTES;
  if ((exc_return & 0x8u) == 0u) off += 0x20u;    /* DCRS=0：callee 区 */
  if ((exc_return & 0x10u) == 0u) off += 0x48u;   /* FType=0：FP 区 */
  if ((xpsr & 0x200u) != 0u) off += 4u;           /* STKALIGN 把 SP 下移过 4 */
  return frame + off;
}

/* ---- 第 5 个参数（errp）的定位 -----------------------------------------
 * ★ v16 真机实测：帧找对了（frame_found == psp、delta=0、帧内 LR==调用点返回地址），
 *   但 `[csp]`（csp = 帧基址+0x20）读出来是 **0** ⇒ 给固件传了 NULL 的 err 指针
 *   ⇒ 固件解析器没法回写错误码 ⇒ C2 报 native_resolve_failed。
 *   反汇编调用点确认编译器确实写了 `strd sb, r0, [sp]`（sb = sp+0xC = &err），
 *   所以只可能是"我算的 csp 与实际栈位置有偏差"，光靠推理分不清 —— 于是：
 *   ① 把现场原样录进 g_mt（帧 8 字 + 上方 8 字 + r0..r3），一次报告看出真相；
 *   ② 不赌单点：在 csp 起 8 个字里找那个"指向调用者自己栈帧"的指针
 *      （&local 必然满足：SRAM 且离 csp 不远；闪存/外设一律不接受，
 *       猜错就退回 0 == 旧行为，绝不扔一个野指针给固件去写）。 */
static int mt_sram_only(uint32_t p) {
  if (p >= 0x00276400u && p < 0x003E0000u) return 1;
  if (p >= 0x20000000u && p < 0x20160000u) return 1;
  if (p >= 0x3C000000u && p < 0x3E000000u) return 1;
  return 0;
}
__attribute__((used, noinline)) static uint32_t mt_find_errp(uint32_t csp, uint32_t *flags_out, uint32_t *slot_out) {
  uint32_t k;
  for (k = 0u; k < 8u; ++k) {
    uint32_t a = csp + 4u * k, w;
    if (!mt_ptr_ok(a)) break;
    w = *(volatile uint32_t *)(uintptr_t)a;
    if (w == 0u || !mt_sram_only(w)) continue;
    if (w < csp - 0x100u || w > csp + 0x400u) continue;
    if (flags_out) {
      *flags_out = mt_ptr_ok(a + 4u) ? *(volatile uint32_t *)(uintptr_t)(a + 4u) : 0u;
    }
    if (slot_out) *slot_out = k;
    return w;
  }
  if (slot_out) *slot_out = 0xFFFFFFFFu;
  return 0u;
}

/* ---- the trap ---------------------------------------------------------- */
/* Thumb instruction size, for the "skip and move on" recovery.  Only ever
 * called with an already-validated code address. */
static uint32_t mt_insn_size(uint32_t pc) {
  uint16_t h = *(volatile uint16_t *)(uintptr_t)pc;
  return ((h & 0xF800u) >= 0xE800u) ? 4u : 2u;
}
/* Last-resort forward progress: find the frame of a fault that happened inside
 * our own module code (so the stacked PC is in SRAM, which the two resolver
 * trap sites never are) and advance it past the faulting instruction.  A
 * skipped load leaves a stale local, a skipped store simply did not happen --
 * both are harmless for the code we run, and unlike "retry forever" they let
 * the system keep running. */
static int mt_skip_faulting_insn(uint8_t *fb, uint32_t *pc_out) {
  uint32_t delta;
  uint32_t anchor = g_mt.handler_addr & ~1u;         /* our own code, set at probe */
  uint32_t stub = (uint32_t)(uintptr_t)g_mt_stub & ~0x1Fu;
  for (delta = 0u; delta <= MT_DELTA_MAX; delta += 4u) {
    uint32_t *c = (uint32_t *)(void *)(fb + delta);
    uint32_t pc = c[MT_F_PC] & ~1u, l = c[MT_F_LR];
    int ours;
    /* 只认"确实是我们自己的东西"的帧。整模块 < 42KB，所以"mt_handler 附近"
       比"任意 SRAM"精确得多 —— 用后者会在寄存器保存区里匹配到假帧（那些槽位
       本来就装着模块地址），跳过就落空了。 */
    ours = (anchor != 0u && pc > anchor - 0x8000u && pc < anchor + 0x8000u) ||
           (pc >= stub && pc < stub + 0x20u);
    if (!ours) continue;
    if (!mt_ptr_ok(pc)) continue;
    if ((c[MT_F_XPSR] & 0x1FFu) != 0u) continue;           /* thread mode */
    if ((l & 1u) == 0u || !mt_ptr_ok(l & ~1u)) continue;
    g_mt.last_skip_pc = pc;
    ++g_mt.skips;
    c[MT_F_PC] = pc + mt_insn_size(pc);
    if (pc_out) *pc_out = pc;
    return 1;
  }
  return 0;
}

struct mt_ctx;
__attribute__((used)) static void mt_dispatch(struct mt_ctx *ctx);
__attribute__((used)) static void mt_hf_dispatch(struct mt_ctx *ctx);
static int mt_arm(void);
/* ★v98 iob_navail 钩子状态 */
static uint32_t g_nv_on, g_nv_region, g_nv_hits, g_nv_ret;
static uint32_t g_nv_traps, g_nv_arm_rc, g_nv_neigh;
static void mt_navail_off(void);
static void mt_disarm(uint32_t reason);
static void mt_failsafe(void);

__attribute__((used, naked, noinline)) static void mt_handler(void) {
  __asm__ volatile(
      "push {r4-r11, lr}\n"
      "mov  r0, sp\n"
      "bl   mt_dispatch\n"
      "pop  {r4-r11, pc}\n");
}
/* The safety net.  If MEMFAULTENA does not actually take effect, or the
 * vector fetch itself faults, the MemManage escalates straight to HardFault --
 * which is precisely how the 2026-10-14 crash died.  Taking the HardFault
 * vector too turns that fatal escalation into a diagnosable, recoverable
 * event: we roll everything back, hand the caller a clean failure and report
 * memmanage_escalated, and the trap is never armed. */
__attribute__((used, naked, noinline)) static void mt_hf_handler(void) {
  __asm__ volatile(
      "push {r4-r11, lr}\n"
      "mov  r0, sp\n"
      "bl   mt_hf_dispatch\n"
      "pop  {r4-r11, pc}\n");
}
/* Replays the trapped neighbour epilogue with the caller's own SP and LR: the
 * exception return restored both, so these are literally the two instructions
 * the CPU was about to execute. */
__attribute__((used, naked, noinline)) static void mt_resume_neighbour(void) {
  __asm__ volatile(
      "pop {r4, r5, r6, r7}\n"
      "bx  lr\n");
}

typedef int (*mt_ghbn_fn)(const char *name, void *host, void *buf,
                          unsigned buflen, void *errp, unsigned flags);
#define MT_REAL_GHBN ((mt_ghbn_fn)(uintptr_t)(MT_ENTRY_ADDR | 1u))

/* Keep the firmware's own bounds: only rewrite h_name in place when the real
 * name still fits between h_name and the end of the caller's buffer. */
static void mt_writeback_name(uint32_t hostent, uint32_t buf, uint32_t blen,
                              const char *name) {
  uint32_t hp, off;
  if (!mt_ptr_ok(hostent) || !mt_ptr_ok(buf)) return;
  hp = *(volatile uint32_t *)(uintptr_t)hostent;
  if (!mt_ptr_ok(hp)) return;
  if (hp < buf) return;
  off = hp - buf;
  if (off >= blen) return;
  if (mt_len(name) + 1u > (blen - off)) return;
  mt_copy((void *)(uintptr_t)hp, name, mt_len(name) + 1u);
}

/* Roll everything back: region off, MPU as we found it, both vectors as we
 * found them, fault status cleared.  Always safe to call from a handler. */
static void mt_failsafe(void) {
  ++g_mt.failsafe_count;
  mt_navail_off();                 /* ★v102：failsafe 同样必须关，否则残留 XN 区 = 复位 */
  g_mt.mm_pc = 0u;
  if (g_mt.region_scan_ok) mt_region_set(0);
  mt_wr(MT_MPU_CTRL, g_mt.mpu_ctrl_before);
  if (g_mt.vector_addr) mt_wr(g_mt.vector_addr, g_mt.vector_before);
  if (g_mt.vector_hf_addr) mt_wr(g_mt.vector_hf_addr, g_mt.vector_hf_before);
  mt_clear_fault();
  mt_clear_hf();
  mt_barrier();
  g_mt.armed = 0u;
  g_mt.state = MT_ST_PROBED;
  g_mt.reason = MT_R_FAILSAFE;
}

/* ---- v30：NuttX 信号量 ABI 自检（先验约定，绝不阻塞）--------------------
 * 目标：像 9Pro 的 NuttX DNS 客户端那样"真阻塞等待"（CPU 让给别人），而不是空转。
 * 地址是用"源码路径字符串 → 引用它的代码"挖出来的（assert 字符串就在函数体内）：
 *   sem_init 0x0C1EA274 / sem_post 0x0C2070D0 / sem_wait.c 被两个函数引用
 *   ⇒ 候选 0x0C206944(nxsem_wait?) 与 0x0C206A98(sem_wait?)
 * 这一步只做：init → post → wait（信号量已有计数 ⇒ wait **立即返回**，不可能挂住）。 */
/* ★ v40：地址统一补上 thumb 位（bit0=1）。本核实测忽略 bit0（v30 自检用偶地址也返回 0），
 * 但保持与全模块其余固件地址（无一例外为奇数）一致，避免落到实现相关行为上。 */
#define MT_SEM_INIT_ADDR 0x0C1EA275u
#define MT_SEM_POST_ADDR 0x0C2070D1u
#define MT_SEM_WAIT_ADDR 0x0C206945u
typedef int (*mt_sem_fn0)(void *);
typedef int (*mt_sem_fn2)(void *, int, unsigned);
static uint32_t g_mt_sem[8];        /* NuttX sem_t 很小，给足 32B 余量 */
static volatile int32_t g_mt_sem_rc_init, g_mt_sem_rc_post, g_mt_sem_rc_wait;
static volatile uint32_t g_mt_sem_ok;
static volatile uint32_t g_mt_sem_waiting, g_mt_sem_wakes;
static volatile uint32_t g_mt_sem_svc;
static volatile uint32_t g_mt_dns_wd, g_mt_dns_wd_aborts;
/* v36：在飞查询的名字与已重试次数 —— 看门狗超时时用它把名字放回队尾（不再丢名）。
 * v35 的教训：直接抛弃 = 名字在 commit 时就没了，预热清单会被"快而误杀"的看门狗烧光。 */
static char g_mt_inflight_name[MT_NAME_MAX];
static volatile uint32_t g_mt_inflight_tries;
static volatile uint32_t g_mt_dns_stuck_drops;
static volatile int32_t g_mt_sem_test_rc;
static volatile uint32_t g_mt_sem_test_svc;
__attribute__((used)) static void mt_sem_post(void) {
  if (g_mt_sem_ok) (void)((mt_sem_fn0)(uintptr_t)MT_SEM_POST_ADDR)((void *)g_mt_sem);
}
__attribute__((used)) static void mt_sem_selftest(void) {
  void *sem = (void *)g_mt_sem;
  mt_zero(g_mt_sem, (unsigned)sizeof(g_mt_sem));
  g_mt_sem_ok = 0u;
  g_mt_sem_rc_init = ((mt_sem_fn2)(uintptr_t)MT_SEM_INIT_ADDR)(sem, 0, 0);
  g_mt_sem_rc_post = ((mt_sem_fn0)(uintptr_t)MT_SEM_POST_ADDR)(sem);
  g_mt_sem_rc_wait = ((mt_sem_fn0)(uintptr_t)MT_SEM_WAIT_ADDR)(sem);
  if (g_mt_sem_rc_init == 0 && g_mt_sem_rc_post == 0 && g_mt_sem_rc_wait == 0)
    g_mt_sem_ok = 1u;
}

/* ---- v21 慢路径：被"异常返回"直接进入的伪被调用者 ----------------------
 * handler 发现缓存里没有这个域名时，把**调用者那一帧的 PC** 改成这个函数的地址，
 * 然后正常从异常返回。于是 CPU 会用：
 *     · 调用者自己的栈（SP = 入栈前的 SP，第 5/6 个参数就在 [sp]/[sp+4]）
 *     · r0..r3 = 解析器的前 4 个参数
 *     · LR = 调用者的续点（固件调用点就是 0x0C1E944D）
 * 进入它 —— 所以它**天然就是一个 6 参数 C 函数**（ABI 与"被调用者"完全一致），
 * 结尾正常 return 就回到 getaddrinfo 的续点，**同一个调用拿到结果**。
 * 它做的事：通知自制的 DNS 解析系统（入队）→ 有界等待解析完成 → 替换成点分十进制
 * → 关区域调固件解析器 → 返回；等不到就按"没装陷阱"的原样放行（绝不更糟）。 */
/* ---- v38 现场判据工具（全部只读，先看后动）-------------------------------
 * g_netlock @0x200C89D4：holder@+0xC、count@+0x10（net_lock/net_unlock thunk 里
 * 的字面量实证）。★ 非属主调 net_unlock 会 assert 崩溃（0x0C1E6BEE）⇒ 绝不盲解锁。*/
#define MT_NETLOCK_ADDR   0x200C89D4u
#define MT_NET_LOCK_ADDR  0x0C201B41u   /* void net_lock(void)  thunk（自取 &g_netlock）；含 thumb 位 */
#define MT_NET_ULOCK_ADDR 0x0C201B51u   /* void net_unlock(void) thunk；含 thumb 位 */
/* ★ v40：任务身份改成**纯读取，绝不调用固件函数**——陷阱 handler 里不允许任何函数调用：
 * 原来的 v38 版本调 0x0C2096E0（且丢了 thumb 位），一旦调用链里出任何故障就会走嵌套故障
 * 恢复路径（而那条路对 handler 模式故障无效 ⇒ 原指令无限重试 ⇒ 冻机，2026-10-16 真机）。
 * 地址 0x200B24D4 = 那个函数自己的字面量（反汇编实证），逻辑与它逐条一致：
 *   tcb=[0x200B24D4]；tcb==0 → 0；状态字节[tcb+0x3C]!=3 → ~2；否则 [tcb+0x2C]。 */
#define MT_CURTASK_PTR_ADDR 0x200B24D4u
typedef void (*mt_lock_t)(void);
static uint32_t mt_task_id(void) {
  uint32_t tcb = *(volatile uint32_t *)(uintptr_t)MT_CURTASK_PTR_ADDR;
  if (tcb == 0u) return 0u;
  if (*(volatile uint8_t *)(uintptr_t)(tcb + 0x3Cu) != 3u) return 0xFFFFFFFDu;
  return *(volatile uint32_t *)(uintptr_t)(tcb + 0x2Cu);
}
static uint32_t mt_netlock_cnt(void) { return *(volatile uint32_t *)(uintptr_t)(MT_NETLOCK_ADDR + 0x10u); }
static uint32_t mt_netlock_holder(void) { return *(volatile uint32_t *)(uintptr_t)(MT_NETLOCK_ADDR + 0x0Cu); }
static uint32_t mt_basepri(void) { uint32_t v; __asm__ volatile("mrs %0, basepri" : "=r"(v)); return v; }
static uint32_t mt_primask(void) { uint32_t v; __asm__ volatile("mrs %0, primask" : "=r"(v)); return v; }
/* v39：网络是否就绪（定义在 tun_module.c；同 TU 前向声明）——没就绪就不等待，立即放行 */
static int tun_net_ready(void);
/* ★ v44：同步等待的"直发"（定义在 tun_module.c）——释放锁后立刻把本次查询发出去，
 * 不等 drain 节拍、不排队列（真机：队列被失败重试堵住时，6 秒都排不到自己的查询） */
static void tun_dns_start_now(const char *name);
/* ★ v52：socket 传输控制（定义在 tun_module.c；由命令阶梯 14/15 调用） */
static void tun_sd_toggle(void);
static void tun_sd_reset(void);
static void tun_sd_selftest(void);   /* ★ v57：一键 socket 自检（cmd 16）*/
static void tun_sd_inject_selftest(void);  /* ★ v62：注入式收包自检（cmd 17）*/
static void tun_td_selftest(void);         /* ★ v68：TCP-DNS 自检（cmd 18）*/
static void tun_netreg_selftest(void);     /* ★ v77：手动叫 netdev 注册（cmd 19）*/
static void tun_bridge_inject_locked(void);/* ★ v79：宿主侧桥喂回包（cmd 20，调用方已持锁）*/
static void tun_query_named_locked(void);  /* ★ v81：用指定域名查询（cmd 21，调用方已持锁）*/
static void tun_iob_grow(void);            /* ★ v83：IOB 池扩容 24→88（cmd 22，手动，重启自复原）*/
static void tun_netin_inject_locked(void); /* ★ v90：cmd 24 —— 真栈注入，触发一次 IOB 分配 */
static void tun_ip_fake_lease(void);       /* ★ v83：装固定测试租约 10.1.10.2/24（cmd 23，模拟器）*/

/* ★ v27：等待必须**让出 CPU**（死循环空转会拖死整机）。WFI 让内核休眠、中断唤醒；
 * 系统 tick 会把我们叫回来（NuttX 抢占式调度 ⇒ drain/桥都能跑）。 */
static void mt_wait_tick(void) {
  __asm__ volatile("wfi" ::: "memory");
}
/* ★★★ v38 慢路径（真同步等待 v2，2026-10-16 重设计）★★★
 * 目标：冷门域名**第一次**就成功（像电脑/9Pro 那样，同一个调用等到答案）。
 * v27-v32 为什么失败（三版实测全冻）：等待期间 (a) 调用者手里的 net_lock 没人放 ⇒
 * 查询永远发不出去；(b) 没有 drain，没人发查询；(c) 等待无上限 ⇒ 无限挂住。
 * 本版逐条解决，且**先读现场再决定**，绝不猜：
 *   1) 读 BASEPRI/PRIMASK：非 0 = 调用者在临界区（调度器被屏蔽）⇒ 绝不等待，走异步放行；
 *   2) 读 g_netlock（只读！）：holder==自己 且 count∈(0,8] ⇒ 把锁整层释放（等完拿回）
 *      ⇒ drain/桥能跑，查询发得出去、回包进得来；
 *   3) 有界等待：以 drain 运行次数为节拍（≤30 拍 ≈3s）+ 硬迭代上限 ⇒ 永不无限挂住；
 *   4) 超时/不能等 ⇒ 名字已入队，drain ~100ms 内异步查好 ⇒ 行为绝不比"没装陷阱"更糟。*/
__attribute__((used, noinline)) static int mt_slow_call(const char *name, void *host,
                                                        void *buf, unsigned blen,
                                                        void *errp, unsigned flags) {
  char nm[MT_NAME_MAX];
  const uint8_t *ip = (const uint8_t *)0;
  uint32_t beats = 0u, iters = 0u, idle_iters = 0u, last_runs, bp, pm, cnt, holder, me, i;
  int rc, have, relock = 0;
  g_mt.slow_active = 1u;
  ++g_mt.slow_calls;
  have = mt_read_name((uint32_t)(uintptr_t)name, nm);
  if (!have) {                                   /* 名字读不出来：直接原样放行 */
    ++g_mt.slow_skip_noread;
    g_mt.slow_active = 0u;
    goto passthrough;
  }
  mt_pend_note(nm);                              /* 异步兜底：入队（drain ~100ms 内发查询）*/
  /* 判据一：临界区？*/
  bp = mt_basepri(); pm = mt_primask();
  g_mt.slow_basepri_last = bp; g_mt.slow_primask_last = pm;
  /* 判据二：网络锁在谁手里？*/
  cnt = mt_netlock_cnt(); holder = mt_netlock_holder(); me = mt_task_id();
  g_mt.slow_nl_cnt = cnt; g_mt.slow_nl_holder = holder; g_mt.slow_nl_me = me;
  if (bp != 0u || pm != 0u) {
    ++g_mt.slow_skip_crit;                       /* 在临界区：跳等待，走异步（绝不冻系统）*/
    g_mt.slow_active = 0u;
    goto passthrough;
  }
  if (!tun_net_ready()) {                        /* 网络没就绪：不等（名字已入队，等它起来再查）*/
    ++g_mt.slow_skip_netdown;
    g_mt.slow_active = 0u;
    goto passthrough;
  }
  ip = mt_cache_lookup(nm);                      /* 竞态窗口里可能已被别人查好 */
  if (!ip) {
    if (cnt > 0u && cnt <= 8u && holder == me) { /* 只读确认属主后才释放（非属主解锁=崩溃）*/
      for (i = 0u; i < cnt; ++i) ((mt_lock_t)(uintptr_t)MT_NET_ULOCK_ADDR)();
      relock = 1; ++g_mt.slow_nl_released;
    }
    /* ★ v44：直发（在锁已释放的前提下；内部自己拿锁；失败则靠队列异步兜底）*/
    tun_dns_start_now(nm);
    /* ★★★ v48：等待预算修正（真机定案 —— FPI 日志 40+ 次冷查询全部 ~1.0s 失败）★★★
     * 旧循环的 iters 硬上限 60000 在实测唤醒率（~10-16µs/次）下 ~0.7-1.0s 就用完 ⇒
     * "最多等 80 拍 ≈8s" 从未真正生效：任何 RTT>1s 的冷查询必死，只有缓存命中的名字能过
     * （baidu 一直被误认为"正常"，其实只是它在早前某次侥幸进了缓存）。
     * 现在：真正的时间预算仍由 beats（drain 运行次数）控制；iters 只用于"drain 毫无进展"
     * 时的早退判据（IDLE_CAP），每来一拍就重置；SPIN_CAP 提高只是绝对保险。
     * drain 停摆 ⇒ 没人会发查询/收回复 ⇒ ~1s 早退（保留"绝不白等"的旧语义）。 */
    last_runs = g_drain_runs;
    idle_iters = 0u;
    while (beats < MT_SLOW_BEATS && iters < MT_SLOW_SPIN_CAP) {
      mt_wait_tick();                            /* 让出 CPU；tick/中断唤醒 */
      ++iters; ++idle_iters;
      ip = mt_cache_lookup(nm);
      if (ip) break;
      if (g_drain_runs != last_runs) { last_runs = g_drain_runs; ++beats; idle_iters = 0u; }
      else if (idle_iters > MT_SLOW_IDLE_CAP) break;
    }
    if (relock) for (i = 0u; i < cnt; ++i) ((mt_lock_t)(uintptr_t)MT_NET_LOCK_ADDR)();
  }
  g_mt.slow_spins_last = iters;
  g_mt.slow_wait_beats = beats;
  ip = mt_cache_lookup(nm);                      /* 唤醒后再查一次 */
  if (ip) ++g_mt.slow_hits;
  else { ++g_mt.slow_timeouts; g_mt.dns_abort_req = 1u; ++g_mt.dns_abort_reqs; }
  g_mt.slow_active = 0u;
  if (ip) {                                      /* 查到了：与"快路径命中"完全一样 */
    char dotted[MT_DOTTED_MAX];
    mt_format_ip(ip, dotted);
    mt_region_set(0);
    rc = MT_REAL_GHBN(dotted, host, buf, blen, errp, flags);
    mt_region_set(1);
    mt_clear_fault();
    if (rc == 0) {
      mt_writeback_name((uint32_t)(uintptr_t)host, (uint32_t)(uintptr_t)buf, blen, nm);
      ++g_mt.subst_ok;
    } else ++g_mt.subst_fail;
    g_mt.subst_rc = (uint32_t)rc;
    ++g_mt.hits;
    return rc;
  }
passthrough:
  mt_region_set(0);                              /* 超时/没查到/不能等：按原样放行 */
  rc = MT_REAL_GHBN(name, host, buf, blen, errp, flags);
  mt_region_set(1);
  mt_clear_fault();
  ++g_mt.misses; ++g_mt.passthrough;
  return rc;
}

__attribute__((used)) static void mt_dispatch(struct mt_ctx *ctx) {
  /* 帧基址由 mt_frame_search 在两条栈上搜出来（不再假定 ctx+36 / PSP+extra）。 */
  uint32_t base = 0u, side = 0u, pc = 0u, nested = g_mt_depth;
  uint32_t *f;
  uint8_t *fb;

  g_mt_depth = 1u;
  ++g_mt.traps; ++g_mt.streak;
  f = mt_frame_search(ctx, &base, &side);
  g_mt.frame_stack = side;
  fb = (uint8_t *)(uintptr_t)g_mt.frame_anchor;   /* 兜底基址：诊断与"跳过"用 */
  if (f) pc = f[MT_F_PC] & ~1u;
  if (nested) {
    /* ★ v40 重写：嵌套故障 = 我们自己的 handler 代码里出了故障。原实现（v19~v39）想"跳过
     * 故障指令"，但 mt_skip_faulting_insn **只接受线程模式帧**（xPSR.IPSR==0）——而这里的
     * 帧必然是 handler 模式（IPSR≠0）⇒ 跳过永远失败 ⇒ 原指令无限重试 ⇒ 整机冻死
     * （2026-10-16 真机：日志全停 23 秒后看门狗转储复位）。本版三步：
     *   ① 当前这次异常的硬件帧就在 ctx+36（handler 模式故障、基本帧在前、FP 区在后），
     *      直接取，不再"搜帧"（搜帧会误找到线程栈上上一次的旧帧）；
     *   ② 故障指令在模块自己的代码里 ⇒ 跳过它，让外层 handler 继续跑完（一次解析照常完成）；
     *   ③ 认不出来或连续反复（≥3 次）⇒ SYSRESETREQ 硬复位 —— 宁可一次干净可恢复的重启，
     *      绝不再无限重试冻机。 */
    uint32_t *nf = (uint32_t *)(void *)((uint8_t *)ctx + 36u);
    uint32_t npc = nf[MT_F_PC] & ~1u;
    uint32_t anchor2 = g_mt.handler_addr & ~1u;
    int ours = (anchor2 != 0u && npc > anchor2 - 0x8000u && npc < anchor2 + 0x8000u);
    ++g_mt.nested_faults;
    g_mt.nested_pc = npc;
    mt_failsafe();
    g_mt.rearm_pending = 1u;   /* ★ v49：卸防必须**预约装回**。旧版这里漏了 ⇒ 嵌套故障（内层跳过、
                                * 外层解析照常成功）之后陷阱可能永久失效 = 真机"成功一下、马上失效" */
    if (ours && mt_ptr_ok(npc) && (nf[MT_F_XPSR] & 0x1FFu) != 0u) {
      ++g_mt.skips; g_mt.last_skip_pc = npc;
      nf[MT_F_PC] = npc + mt_insn_size(npc);
      g_mt_depth = 0u;
      return;
    }
    if (++g_mt.nested_storm >= 3u) {
      *(volatile uint32_t *)(uintptr_t)0xE000ED0Cu = 0x05FA0004u;  /* AIRCR: SYSRESETREQ */
      for (;;) { }
    }
    g_mt_depth = 0u;
    return;
  }
  g_mt.nested_storm = 0u;      /* 非嵌套的一次正常进入 ⇒ "同一情节"归零 */
  if (g_mt.streak > 8u) {                     /* no progress: stand down */
    ++g_mt.unknown_traps;
    g_mt.reason = MT_R_TRAP_STORM;
    mt_failsafe();
    g_mt.rearm_pending = 1u;   /* ★ v49：风暴卸防同样要装回（否则一次风暴 = 永久失效） */
    (void)mt_skip_faulting_insn(fb, &pc);
    g_mt_depth = 0u;
    return;
  }
  if (!f) {
    /* Unknown trap site (the synthetic round-trip stub, or something we did
     * not anticipate).  Stand the whole trap down; if the PC is our own module
     * code, also skip it so a genuine fault there cannot loop. */
    ++g_mt.noframe_traps;
    g_mt.mm_pc = pc;
    g_mt.noframe_pc = pc;   /* mt_failsafe() 会清 mm_pc，所以这里另存一份 */
    mt_failsafe();
    g_mt.rearm_pending = 1u;   /* 只栽一次：让任务上下文稍后把陷阱装回去 */
    if (!mt_skip_faulting_insn(fb, &pc)) { g_mt_depth = 0u; return; }
    g_mt_depth = 0u;
    return;
  }
  if (pc >= MT_NAVAIL_BASE && pc <= MT_NAVAIL_END && pc != MT_NAVAIL_ADDR) {
    /* ★★ v105：颗粒里还有**前一个函数的尾声**（0x6500 mov r0,r8 / 0x6502 b ...）。
     * 那些地址被执行时绝不能被当成 iob_navail，否则会给调用者返回假值 ⇒ 计数漂移/双释放。
     * 处理：立刻关掉区域、**不改 PC**（异常返回后重新执行这条，此时已能正常取指）。*/
    mt_navail_off();
    ++g_nv_neigh;
    g_mt.streak = 0u;          /* ★v106：这是我们自己的区域，不算"风暴" */
    g_mt_depth = 0u;
    return;
  }
  if (pc == MT_NAVAIL_ADDR) {
    /* ★v98：iob_navail 被调用 —— 不执行函数体，直接返回 真实计数+64。
     * 只在算窗口时生效；g_iob_count 本身不动 ⇒ iob_free 的断言永不触发。*/
    uint32_t cv = *(volatile uint16_t *)(uintptr_t)0x200C89C2u;
    uint32_t want = (cv + MT_NAVAIL_ADD) & 0x7FFFu;
    f[MT_F_R0] = want;
    f[MT_F_PC] = f[MT_F_LR] & ~1u;
    ++g_nv_hits; ++g_nv_traps; g_nv_ret = want;
    /* ★★ v106：关键 —— navail 是**正常命中**（每次算窗口都会来），
     * 绝不能计入 streak；否则 streak>8 会触发"陷阱风暴"判据，
     * 把我们自己的区域连同 **DNS 陷阱** 一起撤掉 ⇒ 按域名的 fetch 全挂、只有 IP 直连能用。*/
    g_mt.streak = 0u;
    g_mt_depth = 0u;
    return;
  }
  if (pc == MT_NEIGHBOUR_ADDR) {
    ++g_mt.neigh_traps;
    g_mt.streak = 0u;
    f[MT_F_PC] = (uint32_t)(uintptr_t)mt_resume_neighbour | 1u;
    mt_clear_fault();
    g_mt_depth = 0u;
    return;
  }

  /* --- the resolver entry --------------------------------------------- */
  {
    uint32_t name = f[MT_F_R0], host = f[MT_F_R1], buf = f[MT_F_R2], blen = f[MT_F_R3];
    /* 栈参数就在帧基址 + 0x20（= 异常发生前的 SP）；若硬件为对齐把 SP 下移过 4 字节，
     * 堆叠的 xPSR bit9(STKALIGN) 会置位 ⇒ 再加 4。与帧在哪条栈上无关。 */
    uint32_t csp = mt_csp_of((uint32_t)(uintptr_t)f, ctx->exc_return, f[MT_F_XPSR]);
    uint32_t errp = 0u, flags = 0u, rl = MT_RESUME_PC;
    char nm[MT_NAME_MAX];
    const uint8_t *ip;
    int rc, have;

    ++g_mt.entry_traps;
    g_mt.last_blen = blen;
    /* 现场留档：帧内 r0..r3 + 帧 8 字 + 帧上方 8 字（一次报告就能看出布局真相） */
    {
      uint32_t k;
      for (k = 0u; k < 4u; ++k) g_mt.arg[k] = f[k];
      for (k = 0u; k < 8u; ++k) g_mt.raw_frame[k] = f[k];
      for (k = 0u; k < 8u; ++k) {
        g_mt.raw_above[k] = mt_ptr_ok(csp + 4u * k)
                              ? *(volatile uint32_t *)(uintptr_t)(csp + 4u * k) : 0u;
      }
    }
    /* 第 5/6 参数：先认 [csp]（模型），不成立就在邻近 8 个字里兜底找；都不行给 NULL */
    if (mt_ptr_ok(csp)) errp = mt_find_errp(csp, &flags, &g_mt.errp_slot);
    if (!mt_ptr_ok(host) || !mt_ptr_ok(buf)) blen = 0u;   /* force safe path */
    /* Resume at the frame's real LR.  For the firmware's own call site that is
     * exactly MT_RESUME_PC (0x0C1E944C, four granules past the trap); for a call
     * issued from module code it is that caller's return address.  What must
     * never happen is resuming inside the XN granule -- the fetch would fault
     * again immediately. */
    rl = f[MT_F_LR] & ~1u;
    if (rl >= MT_TRAP_BASE && rl <= MT_TRAP_END) rl = MT_RESUME_PC;
    g_mt.caller_sp_last = csp; g_mt.lr_last = rl;
    g_mt.errp_last = errp; g_mt.flags_last = flags;
    if (!mt_ptr_ok(rl)) {
      /* Implausible return address: stand the trap down (region off, vectors
       * back) and let the entry re-execute natively -- exact pre-trap
       * behaviour, one fault spent. */
      ++g_mt.noframe_traps;
      g_mt.mm_pc = rl;
      mt_failsafe();
      g_mt.rearm_pending = 1u;
      g_mt_depth = 0u;
      return;
    }

    nm[0] = 0;
    /* ★ v38：现场"能不能安全等待"判据快照（单核异常上下文 ⇒ 一致快照；纯只读）*/
    g_mt.basepri_at_trap = mt_basepri();
    g_mt.primask_at_trap = mt_primask();
    g_mt.nl_cnt = mt_netlock_cnt();
    g_mt.nl_holder = mt_netlock_holder();
    g_mt.nl_me = mt_task_id();
    g_mt.nl_mine = (g_mt.nl_cnt != 0u && g_mt.nl_holder == g_mt.nl_me) ? 1u : 0u;
    if (g_mt.nl_mine) ++g_mt.nl_mine_yes; else ++g_mt.nl_mine_no;
    if (g_mt.basepri_at_trap || g_mt.primask_at_trap) ++g_mt.crit_at_trap;
    have = mt_read_name(name, nm);
    ip = have ? mt_cache_lookup(nm) : (const uint8_t *)0;

    /* The resolver's own entry is inside the XN granule, so the firmware
     * routine may only be called with the region disabled.  It is re-armed
     * before we leave: at that point we are in module code and the caller's
     * continuation is outside the granule, so no thread can be inside it. */
    if (ip) {
      char dotted[MT_DOTTED_MAX];
      mt_format_ip(ip, dotted);
      mt_region_set(0); ++g_mt.region_off_count;
      rc = MT_REAL_GHBN(dotted, (void *)(uintptr_t)host, (void *)(uintptr_t)buf,
                        blen, (void *)(uintptr_t)errp, flags);
      mt_region_set(1);
      mt_clear_fault();
      if (rc == 0) { mt_writeback_name(host, buf, blen, nm); ++g_mt.subst_ok; }
      else ++g_mt.subst_fail;
      g_mt.subst_rc = (uint32_t)rc;
      ++g_mt.hits;
    } else if (!name || !host || !buf || !blen) {
      /* The firmware routine would hit its own argument assert; report a plain
       * resolution failure instead of letting it panic. */
      if (errp && mt_ptr_ok(errp)) *(volatile uint32_t *)(uintptr_t)errp = 1u;
      rc = -1;
      ++g_mt.misses; ++g_mt.subst_fail;
      mt_clear_fault();
    } else if (g_mt.slow_enabled && !g_mt.slow_active && have &&
               !mt_numeric_like(nm) && (f[MT_F_XPSR] & 0x1FFu) == 0u) {
      /* ★ v21 慢路径中转 ★ 缓存里没有这个域名：把**调用者那一帧的 PC** 换成 mt_slow_call，
       * 正常从异常返回 ⇒ CPU 会用调用者自己的栈/参数进入它（ABI 与被调用者一致），
       * 它在"调用者被挂住"的状态下等模块的 DNS 客户端把名字查完，再替调用者把这次
       * 解析做完 —— 于是**同一个调用**就能拿到结果（冷门域名也一次成功）。
       * 不改任何参数、不动栈：stub 直接读 r0..r3 与 [sp]/[sp+4]。 */
      ++g_mt.slow_entries;
      g_mt.slow_active = 1u;                 /* 先占位：别的陷阱这期间走普通路径 */
      f[MT_F_PC] = (uint32_t)(uintptr_t)mt_slow_call;
      mt_clear_fault();
      g_mt.streak = 0u;
      g_mt_depth = 0u;
      return;
    } else {
      if (have) mt_pend_note(nm);
      mt_region_set(0); ++g_mt.region_off_count;
      rc = MT_REAL_GHBN((const char *)(uintptr_t)name, (void *)(uintptr_t)host,
                        (void *)(uintptr_t)buf, blen, (void *)(uintptr_t)errp, flags);
      mt_region_set(1);
      mt_clear_fault();
      ++g_mt.misses; ++g_mt.passthrough;
    }
    f[MT_F_R0] = (uint32_t)rc;
    f[MT_F_PC] = rl;
    g_mt.streak = 0u;
  }
  g_mt_depth = 0u;
}

/* HardFault path: only ever touch faults that our own XN granule or our own
 * code could have caused.  Anything else is handed straight back to the
 * firmware with the machine restored to how we found it. */
__attribute__((used)) static void mt_hf_dispatch(struct mt_ctx *ctx) {
  /* 同上：HardFault 也可能来自 PSP 上的线程，用同一套两栈搜索。 */
  uint32_t base = 0u, side = 0u, pc = 0u, lr = 0u, *f;
  uint8_t *fb;
  uint32_t nested = g_mt_depth, stub = (uint32_t)(uintptr_t)g_mt_stub & ~0x1Fu;
  int mine;

  g_mt_depth = 1u;
  ++g_mt.hf_traps;
  f = mt_frame_search(ctx, &base, &side);
  g_mt.frame_stack = side;
  fb = (uint8_t *)(uintptr_t)g_mt.frame_anchor;
  if (f) { pc = f[MT_F_PC] & ~1u; lr = f[MT_F_LR]; }
  if (nested) {
    /* A fault inside our own handler escalated to HardFault.  Roll back and
     * move past the faulting instruction: retrying it would hang the system
     * (that is what the watchdog reset of 2026-10-15 came from). */
    ++g_mt.nested_faults;
    mt_failsafe();
    g_mt.rearm_pending = 1u;   /* ★ v49：卸防必须预约装回 */
    (void)mt_skip_faulting_insn(fb, &pc);
    g_mt_depth = 0u;
    return;
  }
  g_mt.hf_pc = pc; g_mt.hf_lr = lr;
  if (!f) {
    ++g_mt.hf_foreign;
    mt_failsafe();                            /* restore, so a retry reaches the
                                               * firmware's own handler */
    g_mt.rearm_pending = 1u;                  /* ★ v49 */
    g_mt_depth = 0u;
    return;
  }

  mine = (pc >= MT_TRAP_BASE && pc <= MT_TRAP_END) ||
         (pc >= stub && pc < stub + 0x20u) ||
         (pc >= 0x20000000u && pc < 0x40000000u);
  mt_failsafe();                              /* roll back: no trap from here */
  g_mt.rearm_pending = 1u;                    /* ★ v49：卸防就要装回（下面所有分支都直接返回） */
  if (!mine) {
    ++g_mt.hf_foreign;
    g_mt_depth = 0u;
    return;
  }
  g_mt.reason = MT_R_MM_ESCALATED;
  ++g_mt.hf_fixed;
  if (pc == MT_NEIGHBOUR_ADDR) {
    f[MT_F_PC] = (uint32_t)(uintptr_t)mt_resume_neighbour | 1u;
    g_mt_depth = 0u;
    return;
  }
  if (pc == MT_ENTRY_ADDR) {
    /* Present it to the caller exactly like "host not found" so getaddrinfo
     * completes instead of panicking. */
    uint32_t csp = mt_csp_of((uint32_t)(uintptr_t)f, ctx->exc_return, f[MT_F_XPSR]);
    if (mt_ptr_ok(csp)) {
      uint32_t errp = *(volatile uint32_t *)(uintptr_t)csp;
      if (errp && mt_ptr_ok(errp)) *(volatile uint32_t *)(uintptr_t)errp = 1u;
    }
    f[MT_F_R0] = (uint32_t)-1;
    f[MT_F_PC] = (lr & ~1u) >= MT_TRAP_BASE && (lr & ~1u) <= MT_TRAP_END
                     ? MT_RESUME_PC : (lr & ~1u);
    g_mt_depth = 0u;
    return;
  }
  /* Anything else inside our own code or the granule: skip and continue. */
  if (mt_ptr_ok(pc)) {
    g_mt.last_skip_pc = pc;
    ++g_mt.skips;
    f[MT_F_PC] = pc + mt_insn_size(pc);
  }
  g_mt_depth = 0u;
}

/* ---- staged, self-verifying arming ------------------------------------- */
static void mt_scan_regions(void) {
  uint32_t saved = mt_rd(MT_MPU_RNR), i, n;
  g_mt.mpu_type = mt_rd(MT_MPU_TYPE);
  n = (g_mt.mpu_type >> 8) & 0xFFu;
  if (n > 8u) n = 8u;
  g_mt.mpu_dregion = n;
  for (i = 0u; i < 8u; ++i) {
    g_mt.regions[i][0] = 0u; g_mt.regions[i][1] = 0u;
    if (i >= n) continue;
    mt_wr(MT_MPU_RNR, i);
    mt_barrier();
    g_mt.regions[i][0] = mt_rd(MT_MPU_RBAR);
    g_mt.regions[i][1] = mt_rd(MT_MPU_RLAR);
  }
  mt_wr(MT_MPU_RNR, saved);
  g_mt.region_scan_ok = 1u;
}
static int mt_regions_sane(void) {
  uint32_t i, n = g_mt.mpu_dregion;
  if (n > 8u) n = 8u;
  for (i = 0u; i < n; ++i) {
    uint32_t rbar = g_mt.regions[i][0], rlar = g_mt.regions[i][1];
    if ((rlar & MT_RLAR_EN) == 0u) continue;
    if ((rbar & ~0x1Fu) > (rlar & ~0x1Fu)) return 0;   /* base above limit */
    if ((rbar & ~0x1Fu) == 0xFFFFFFE0u) return 0;
  }
  return 1;
}
static int mt_gate(void) {
  if (!g_mt.region_scan_ok) return MT_R_NOT_PROBED;
  if (!entry_cpuid_ok(g_entry_cpuid)) return MT_R_CPUID;
  if (g_entry_bl0 != MT_BL_WORD) return MT_R_CALLSITE;
  if (entry_read32(MT_NEIGHBOUR_ADDR) != MT_WORD_NEIGHBOUR ||
      entry_read32(MT_ENTRY_ADDR) != MT_WORD_ENTRY) return MT_R_CODE_WORDS;
  if (!g_entry_vtor_aligned || !entry_vtor_readable(g_entry_vtor)) return MT_R_VECTOR_UNREADABLE;
  if (g_mt.mpu_dregion == 0u) return MT_R_MPU_TYPE;
  if (!mt_regions_sane()) return MT_R_CTRL_REJECTED;
  return MT_R_NONE;
}
static int mt_pick_region(void) {
  uint32_t i;
  int best = -1;
  for (i = 0u; i < g_mt.mpu_dregion && i < 8u; ++i) {
    if ((g_mt.regions[i][1] & MT_RLAR_EN) != 0u) continue;
    if (g_mt.regions[i][0] == 0xFFFFFFFFu || g_mt.regions[i][1] == 0xFFFFFFFFu) continue;
    best = (int)i;                            /* highest free slot wins: it
                                               * outranks any overlapping one */
  }
  return best;
}
static void mt_probe(void) {
  /* v39：slow_enabled 的默认值统一在 mt_stage_arm 里设置（这里原来那行随后被
   * mt_zero(&g_mt,...) 清零，是死代码）。*/
  mt_zero(&g_mt, sizeof(g_mt));
  g_mt.state = MT_ST_OFF; g_mt.reason = MT_R_NOT_PROBED;
  g_mt.handler_addr = (uint32_t)(uintptr_t)mt_handler;
  g_mt.stub_addr = (uint32_t)(uintptr_t)mt_resume_neighbour;
  g_mt.mm_stub_addr = (uint32_t)(uintptr_t)g_mt_stub;
  g_mt.entry_addr = MT_ENTRY_ADDR;
  g_mt.entry_ok = (entry_read32(MT_ENTRY_ADDR) == MT_WORD_ENTRY) ? 1u : 0u;
  g_mt.neighbour_ok = (entry_read32(MT_NEIGHBOUR_ADDR) == MT_WORD_NEIGHBOUR) ? 1u : 0u;
  g_mt.demcr = entry_read32(MT_SCB_DEMCR);
  g_mt.cfsr = mt_rd(MT_SCB_MMFSR);
  g_mt.shcsr_before = mt_rd(MT_SCB_SHCSR);
  g_mt.mpu_ctrl_before = mt_rd(MT_MPU_CTRL);
  mt_scan_regions();
  g_mt.vector_addr = g_entry_vtor + MT_MM_VECTOR_OFF;
  g_mt.vector_before = entry_read32(g_mt.vector_addr);
  g_mt.vector_now = g_mt.vector_before;
  g_mt.vector_hf_addr = g_entry_vtor + MT_HF_VECTOR_OFF;
  g_mt.vector_hf_before = entry_read32(g_mt.vector_hf_addr);
  g_mt.vector_hf_now = g_mt.vector_hf_before;
  g_mt.hf_handler_addr = (uint32_t)(uintptr_t)mt_hf_handler;
  g_mt.state = MT_ST_PROBED;
  g_mt.reason = (uint32_t)mt_gate();
  if (g_mt.reason == MT_R_NONE) g_mt.reason = MT_R_NOT_ARMED;   /* probe only */
}

/* Stage A: install the handler and prove the enable bit sticks.  Touches no
 * memory protection at all, so it cannot provoke a fault. */
static int mt_stage_vector(void) {
  uint32_t irq = mt_irq_save();
  int rc = 0;
  mt_wr(g_mt.vector_addr, (uint32_t)(uintptr_t)mt_handler | 1u);
  mt_wr(g_mt.vector_hf_addr, (uint32_t)(uintptr_t)mt_hf_handler | 1u);
  mt_shcsr_ena_ensure();                 /* enable bits only -- never PEND/ACT */
  mt_barrier();
  g_mt.vector_now = entry_read32(g_mt.vector_addr);
  g_mt.vector_hf_now = entry_read32(g_mt.vector_hf_addr);
  if (g_mt.vector_now != ((uint32_t)(uintptr_t)mt_handler | 1u) ||
      g_mt.vector_hf_now != ((uint32_t)(uintptr_t)mt_hf_handler | 1u)) {
    g_mt.reason = MT_R_VECTOR_UNREADABLE;
    mt_wr(g_mt.vector_addr, g_mt.vector_before);
    mt_wr(g_mt.vector_hf_addr, g_mt.vector_hf_before);
    rc = -1;
  } else if ((g_mt.shcsr_after & MT_SHCSR_ENA_BITS) != MT_SHCSR_ENA_BITS) {
    /* Without MEMFAULTENA every MemManage escalates straight to HardFault --
     * exactly the crash seen in log 29598 #10.  Refuse to continue. */
    g_mt.reason = MT_R_MEMFAULTENA_REFUSED;
    mt_wr(g_mt.vector_addr, g_mt.vector_before);
    mt_wr(g_mt.vector_hf_addr, g_mt.vector_hf_before);
    rc = -2;
  } else {
    mt_clear_fault();                    /* and prove nothing is left pending */
    if (g_mt.shcsr_pend != 0u) { g_mt.reason = MT_R_MEMFAULTENA_REFUSED; rc = -3; }
  }
  mt_barrier();
  mt_irq_restore(irq);
  return rc;
}

/* Stage B: turn the MPU on with the firmware's own region set (our XN region
 * still disabled), then prove (1) flash execution still works and (2) a
 * synthetic XN fault is delivered, handled and recovered from.  Every step
 * here self-reverts, so the worst case is a rolled-back trap, not a crash. */
static int mt_stage_mpu(void) {
  uint32_t irq;
  uint32_t traps_before = g_mt.traps, hf_before = g_mt.hf_traps;

  /* No region of ours is armed in this stage, so no fault can happen at all:
     it only turns the MPU on and proves the system is still healthy. */
  irq = mt_irq_save();
  mt_wr(MT_MPU_CTRL, mt_rd(MT_MPU_CTRL) | MT_MPU_CTRL_PRIVDEFENA | MT_MPU_CTRL_ENABLE);
  mt_barrier();
  g_mt.mpu_ctrl_after = mt_rd(MT_MPU_CTRL);
  if ((g_mt.mpu_ctrl_after & (MT_MPU_CTRL_ENABLE | MT_MPU_CTRL_PRIVDEFENA)) !=
      (MT_MPU_CTRL_ENABLE | MT_MPU_CTRL_PRIVDEFENA)) {
    mt_wr(MT_MPU_CTRL, g_mt.mpu_ctrl_before);
    mt_barrier();
    mt_irq_restore(irq);
    g_mt.reason = MT_R_MPU_ENABLE_REFUSED;
    return -2;
  }
  g_mt.armed = 1u;
  ++g_mt.arm_count;
  mt_irq_restore(irq);

  /* flash preflight: the system must still be able to execute flash at all. */
  {
    uint32_t ho[5]; uint8_t buf[0x100]; uint32_t err = 0xDEADBEEFu;
    mt_zero(ho, sizeof(ho)); mt_zero(buf, sizeof(buf));
    g_mt.preflight_rc = (uint32_t)MT_REAL_GHBN("127.0.0.1", ho, buf,
                                               (unsigned)sizeof(buf), &err, 0u);
    g_mt.mm_traps = g_mt.traps - traps_before;      /* expected: 0 */
    if (g_mt.preflight_rc != 0u || err != 0u || g_mt.hf_traps != hf_before) {
      g_mt.reason = MT_R_PREFLIGHT_FAILED;
      mt_wr(MT_MPU_CTRL, g_mt.mpu_ctrl_before);
      g_mt.armed = 0u;
      return -3;
    }
  }
  return 0;
}

/* Stage C1: arm the resolver window and verify the region really took.  No
 * call is made, so this stage cannot raise a fault at all -- if the system
 * dies here it is the MPU/region, not the fault delivery. */
static int mt_stage_arm(void) {
  /* ★ v42：真同步等待**默认开**——冷门域名第一次调用内等到答案（≤6 秒），与 9Pro 行为对齐。
   * 基础路径已在 v40/v41 复验（19 分钟会话零嵌套故障 + 回包宽容匹配命中）。
   * 三道安全闸仍在：BASEPRI 非 0 不等待 / 只读确认属主才释放 net_lock / 有界等待。
   * cmd 10 可随时退回纯异步；退回后 = 第一次放行、drain 查完、第二次命中。*/
  g_mt.slow_enabled = 1u;
  /* ★ v39：预热清单已删除（原 v34 的 www.taobao.com / t.cn / www.baidu.com /
   * mtop.taobao.com 四条）。冷门域名由同步等待（cmd 9）或异步第二次命中解决。*/
  uint32_t irq;

  if (!g_mt.armed) {
    if (mt_stage_vector() != 0) return -1;
    irq = mt_irq_save();
    mt_wr(MT_MPU_CTRL, mt_rd(MT_MPU_CTRL) | MT_MPU_CTRL_PRIVDEFENA | MT_MPU_CTRL_ENABLE);
    mt_barrier();
    mt_irq_restore(irq);
    g_mt.armed = 1u;
  }
  irq = mt_irq_save();
  g_mt.rbar = (MT_TRAP_BASE & ~0x1Fu) | MT_RBAR_AP_RW | MT_RBAR_XN;
  g_mt.rlar = (MT_TRAP_END & ~0x1Fu) | (MT_ATTR_INDEX << MT_RLAR_ATTR_SHIFT) | MT_RLAR_EN;
  mt_wr(MT_MPU_RNR, g_mt.region);
  mt_wr(MT_MPU_RBAR, g_mt.rbar);
  mt_wr(MT_MPU_RLAR, g_mt.rlar);
  mt_barrier();
  g_mt.mpu_ctrl_after = mt_rd(MT_MPU_CTRL);
  if (mt_rd(MT_MPU_RBAR) != g_mt.rbar || mt_rd(MT_MPU_RLAR) != g_mt.rlar ||
      (mt_rd(MT_MPU_RLAR) & MT_RLAR_EN) == 0u) {
    mt_wr(MT_MPU_RLAR, 0u);
    mt_barrier();
    mt_irq_restore(irq);
    g_mt.reason = MT_R_MPU_ENABLE_REFUSED;
    return -1;
  }
  mt_irq_restore(irq);
  /* Region armed and verified, nothing executed. */
  g_mt.stage = 3u;
  g_mt.reason = MT_R_REGION_ARMED;
  return 0;
}

/* ★★ v98：把 iob_navail 所在的那个 32 字节颗粒设成 XN —— 一调用就 MemManage。
 * 返回：0=成功  -1=陷阱没装  -2=没有空余 MPU 区域  -3=区域写不进去 */
static int mt_navail_arm(void) {
  uint32_t irq, i, r2 = 0xFFFFFFFFu, rbar, rlar;
  if (g_nv_on) return 0;
  if (!g_mt.armed) return -1;
  /* ★v102：必须读**实时** RLAR 判断是否空闲 —— g_mt.regions 是探测期的旧快照，
   * 固件可能在之后自己启用/改写了某个区域，照旧快照选就会覆盖它 ⇒ 系统崩。*/
  for (i = 0u; i < g_mt.mpu_dregion && i < 8u; ++i) {
    uint32_t live;
    if (i == g_mt.region) continue;
    mt_wr(MT_MPU_RNR, i);
    mt_barrier();
    live = mt_rd(MT_MPU_RLAR);
    if ((live & MT_RLAR_EN) != 0u) continue;          /* 已被占用 */
    if (live == 0xFFFFFFFFu) continue;
    if (mt_rd(MT_MPU_RBAR) == 0xFFFFFFFFu) continue;
    r2 = i;
  }
  if (r2 == 0xFFFFFFFFu) return -2;
  rbar = (MT_NAVAIL_BASE & ~0x1Fu) | MT_RBAR_AP_RW | MT_RBAR_XN;
  rlar = (MT_NAVAIL_END & ~0x1Fu) | (MT_ATTR_INDEX << MT_RLAR_ATTR_SHIFT) | MT_RLAR_EN;
  irq = mt_irq_save();
  mt_wr(MT_MPU_RNR, r2);
  mt_wr(MT_MPU_RBAR, rbar);
  mt_wr(MT_MPU_RLAR, rlar);
  mt_barrier();
  if (mt_rd(MT_MPU_RBAR) != rbar || mt_rd(MT_MPU_RLAR) != rlar) {
    mt_wr(MT_MPU_RLAR, 0u); mt_barrier(); mt_irq_restore(irq);
    return -3;
  }
  mt_irq_restore(irq);
  g_nv_region = r2; g_nv_on = 1u;
  return 0;
}
/* ★★ v102：**必须**能把它关掉 —— mt_disarm/failsafe 只关原区域，
 * 若不关这个，向量恢复后一取指就 fault 而没人管 ⇒ 复位（真机崩溃根因）。*/
static void mt_navail_off(void) {
  uint32_t irq, v;
  if (!g_nv_on) return;
  irq = mt_irq_save();
  mt_wr(MT_MPU_RNR, g_nv_region);
  v = mt_rd(MT_MPU_RLAR);
  mt_wr(MT_MPU_RLAR, v & ~MT_RLAR_EN);
  mt_barrier();
  mt_irq_restore(irq);
  g_nv_on = 0u;
}
/* ★★ v101：自动装钩子 —— 真机上 /dev/tunentry 写不进去，命令通道不可靠，
 * 所以像自动扩池一样挂到节拍器里：陷阱已武装 + 再等 5 拍 ⇒ 自己把 navail 钩子装上。
 * 只装一次（g_nv_on 置位后不再进）。*/
static unsigned g_nv_beats;
static void mt_navail_auto(void) {
  if (g_nv_on) return;
  if (!g_mt.armed) return;
  if (g_nv_beats < 5u) { ++g_nv_beats; return; }
  /* ★v104：失败就把拍数清零，下一轮再试；成功则由 g_nv_on 挡住重入。
   * 关键：failsafe 会关钩子，这里必须能**反复重装**，不能"只装一次"。*/
  g_nv_arm_rc = (uint32_t)mt_navail_arm();
  if (g_nv_arm_rc != 0u) g_nv_beats = 0u;
}
/* 自检：直接调用 iob_navail(1)，看返回值（钩子关=真实计数，钩子开=+64） */
static uint32_t g_nv_raw, g_nv_w32;
static uint32_t mt_navail_probe(void) {
  int (*fn)(int) = (int (*)(int))(uintptr_t)(MT_NAVAIL_ADDR | 1u);
  /* 三个值在同一时刻取，专门用来解 "+6" 那个矛盾：
   *   w32  = 0x200C89C0 处的整字（高半字应等于计数）
   *   low  = 0x200C89C0 处的低半字
   *   raw  = 0x200C89C2 处的半字（= w32>>16，应与 w32 自洽）
   *   ret  = 调 iob_navail(1) 的返回
   * 若 raw != (w32>>16) ⇒ 模块的半字读有问题；
   * 若 raw == w32>>16 但 ret != raw ⇒ 函数不是简单返回该变量。*/
  g_nv_w32 = *(volatile uint32_t *)(uintptr_t)0x200C89C0u;
  g_nv_raw = (uint32_t)(*(volatile uint16_t *)(uintptr_t)0x200C89C2u);
  g_nv_ret = (uint32_t)fn(1);
  return g_nv_ret;
}

/* Stage C2: the single intentional fault -- call the resolver once so the XN
 * granule triggers, then prove the substitution path. */
static int mt_stage_trap(void) {
  if (mt_stage_arm() != 0) return -1;
  uint32_t irq, ho[5]; uint8_t buf[0x100]; uint32_t err;
  static const uint8_t probe_ip[4] = { 10u, 1u, 10u, 2u };
  uint32_t before;
  int rc;

  if (!g_mt.armed) {
    if (mt_stage_vector() != 0) return -1;
    irq = mt_irq_save();
    g_mt.rbar = (MT_TRAP_BASE & ~0x1Fu) | MT_RBAR_AP_RW | MT_RBAR_XN;
    g_mt.rlar = (MT_TRAP_END & ~0x1Fu) | (MT_ATTR_INDEX << MT_RLAR_ATTR_SHIFT) | MT_RLAR_EN;
    mt_wr(MT_MPU_RNR, g_mt.region);
    mt_wr(MT_MPU_RBAR, g_mt.rbar);
    mt_wr(MT_MPU_RLAR, g_mt.rlar);
    mt_wr(MT_MPU_CTRL, mt_rd(MT_MPU_CTRL) | MT_MPU_CTRL_PRIVDEFENA | MT_MPU_CTRL_ENABLE);
    mt_barrier();
    g_mt.mpu_ctrl_after = mt_rd(MT_MPU_CTRL);
    if (mt_rd(MT_MPU_RBAR) != g_mt.rbar || mt_rd(MT_MPU_RLAR) != g_mt.rlar) {
      mt_wr(MT_MPU_RLAR, 0u);
      mt_wr(MT_MPU_CTRL, g_mt.mpu_ctrl_before);
      mt_barrier();
      mt_irq_restore(irq);
      g_mt.reason = MT_R_MPU_ENABLE_REFUSED;
      return -1;
    }
    g_mt.armed = 1u;
    mt_irq_restore(irq);
  }

  /* (1) trap-optional call: a numeric literal resolves natively too, so this
   * proves the trap fires without *depending* on it -- if the trap never
   * triggers the call still succeeds and we only report trap_not_taken. */
  mt_zero(ho, sizeof(ho)); mt_zero(buf, sizeof(buf)); err = 0xDEADBEEFu;
  before = g_mt.entry_traps;
  {
    uint32_t traps_before = g_mt.traps;
    rc = MT_REAL_GHBN("127.0.0.1", ho, buf, (unsigned)sizeof(buf), &err, 0u);
    g_mt.mm_traps = g_mt.traps - traps_before;
    g_mt.trap_test_rc = (uint32_t)rc;
    if (g_mt.traps > traps_before + 2u) {
      g_mt.reason = MT_R_TRAP_STORM;
      mt_disarm(MT_R_TRAP_STORM);
      return -7;
    }
  }
  if (rc != 0 || err != 0u) {                 /* native resolution must still work */
    g_mt.reason = MT_R_TRAP_TEST_FAILED;
    mt_disarm(MT_R_TRAP_TEST_FAILED);
    return -2;
  }
  if (g_mt.entry_traps == before) {           /* our region never triggered */
    g_mt.reason = MT_R_TRAP_NOT_TAKEN;
    mt_disarm(MT_R_TRAP_NOT_TAKEN);
    return -3;
  }

  /* (2) substitution path: a cached name must come back from the firmware
   * routine with the cached address. */
  mt_cache_insert("probe.p67tc.test", probe_ip, 0u);
  mt_zero(ho, sizeof(ho)); mt_zero(buf, sizeof(buf)); err = 0xDEADBEEFu;
  before = g_mt.hits;
  {
    uint32_t traps_before = g_mt.traps;
    rc = MT_REAL_GHBN("probe.p67tc.test", ho, buf, (unsigned)sizeof(buf), &err, 0u);
    g_mt.mm_traps = g_mt.traps - traps_before;
    g_mt.subst_rc = (uint32_t)rc;
    if (g_mt.traps > traps_before + 2u) {
      g_mt.reason = MT_R_TRAP_STORM;
      mt_disarm(MT_R_TRAP_STORM);
      return -7;
    }
  }
  if (rc != 0 || err != 0u || g_mt.hits == before) {
    g_mt.reason = MT_R_SUBSTITUTION_FAILED;
    mt_disarm(MT_R_SUBSTITUTION_FAILED);
    return -4;
  }
  {
    uint32_t addr_list = ho[4], p, v;
    if (!mt_ptr_ok(addr_list)) { g_mt.reason = MT_R_SUBSTITUTION_FAILED; mt_disarm(MT_R_SUBSTITUTION_FAILED); return -5; }
    p = *(volatile uint32_t *)(uintptr_t)addr_list;
    if (!mt_ptr_ok(p)) { g_mt.reason = MT_R_SUBSTITUTION_FAILED; mt_disarm(MT_R_SUBSTITUTION_FAILED); return -6; }
    v = *(volatile uint32_t *)(uintptr_t)p;
    g_mt.mm_stub_seen = v;
    if (v != 0x020A010Au) { g_mt.reason = MT_R_SUBSTITUTION_FAILED; mt_disarm(MT_R_SUBSTITUTION_FAILED); return -8; }
  }
  g_mt.state = MT_ST_ARMED;
  g_mt.reason = MT_R_NONE;
  return 0;
}

/* Full ladder.  A stage that fails leaves nothing armed and reports why. */
static int mt_arm(void) {
  int r;
  if (g_mt.armed && g_mt.state == MT_ST_ARMED) return 0;
  r = mt_gate();
  if (r != MT_R_NONE) { g_mt.reason = (uint32_t)r; g_mt.state = MT_ST_BLOCKED; return -r; }
  r = mt_pick_region();
  if (r < 0) { g_mt.reason = MT_R_NO_FREE_REGION; g_mt.state = MT_ST_BLOCKED; return -(int)MT_R_NO_FREE_REGION; }
  g_mt.region = (uint32_t)r;

  g_mt.stage = 1u;
  g_mt.state = MT_ST_PROBED;
  if (mt_stage_vector() != 0) { g_mt.state = MT_ST_BLOCKED; return -1; }
  g_mt.stage = 2u;
  if (mt_stage_mpu() != 0) { g_mt.state = MT_ST_BLOCKED; return -2; }
  g_mt.stage = 3u;
  if (mt_stage_trap() != 0) { g_mt.state = MT_ST_BLOCKED; return -3; }
  g_mt.stage = 4u;
  g_mt.state = MT_ST_ARMED;
  g_mt.reason = MT_R_NONE;
  return 0;
}

static void mt_disarm(uint32_t reason) {
  uint32_t irq = mt_irq_save();
  mt_navail_off();                 /* ★v102：必须一起关，否则复位 */
  if (g_mt.region_scan_ok) mt_region_set(0);
  mt_wr(MT_MPU_CTRL, g_mt.mpu_ctrl_before);
  if (g_mt.vector_addr) mt_wr(g_mt.vector_addr, g_mt.vector_before);
  if (g_mt.vector_hf_addr) mt_wr(g_mt.vector_hf_addr, g_mt.vector_hf_before);
  mt_clear_fault();
  mt_clear_hf();
  mt_barrier();
  if (g_mt.armed) ++g_mt.disarm_count;
  g_mt.armed = 0u;
  g_mt.state = MT_ST_PROBED;
  g_mt.reason = reason;
  mt_irq_restore(irq);
}

/* Staged shell control: write g_mt_cmd (address reported as mt_cmd_addr) with
 * "mw <addr>=<n>".  Evaluated only from a task context, never from a handler.
 *   1 = stage A   install both vectors + MEMFAULTENA and verify the readback
 *   2 = stage B   MPU on + flash preflight            (zero faults)
 *   3 = stage C1  arm the resolver window + verify the region took (zero faults)
 *   4 = stage C2  the single intentional fault: call the resolver, then run the
 *                 substitution test
 *   5 = disarm    roll everything back
 *   6 = clear the fault status
 *   7 = clear the DNS cache
 *   8 = run the whole ladder
 *  14 = ★ v52 toggle the DNS transport: system socket (default) vs legacy raw TUN
 *  15 = ★ v52 reset the socket RX state (drop stale datagrams, clear error counters)
 * Staged on purpose: each step is individually revertible, so a problem is
 * bisectable and can never wedge the boot. */
static void mt_cmd_poll(void) {
  uint32_t c = g_mt_cmd;
  int r;
  if (c == 0u || c == g_mt_cmd_seen) return;
  g_mt_cmd_seen = c;
  g_mt.cmd_seen = c;
  r = mt_pick_region();
  if (r >= 0) g_mt.region = (uint32_t)r;
  if (c == 1u) { g_mt.stage = 1u; (void)mt_stage_vector(); }
  else if (c == 2u) { g_mt.stage = 2u; (void)mt_stage_vector(); (void)mt_stage_mpu(); }
  else if (c == 3u) { g_mt.stage = 3u; (void)mt_stage_arm(); }
  else if (c == 4u) { g_mt.stage = 4u; (void)mt_stage_trap(); }
  else if (c == 5u) mt_disarm(MT_R_USER_DISARM);
  else if (c == 6u) mt_clear_fault();
  else if (c == 7u) { mt_zero(g_mt_cache, sizeof(g_mt_cache)); g_mt.cache_count = 0u; }
  else if (c == 8u) (void)mt_arm();
  else if (c == 9u) g_mt.slow_enabled = 1u;    /* 慢路径开（默认开） */
  else if (c == 10u) g_mt.slow_enabled = 0u;   /* 慢路径关 */
  else if (c == 12u) mt_sem_selftest();   /* v30：信号量 ABI 自检（立即返回，不阻塞） */
  else if (c == 13u) {   /* ★ v34 对照实验：任务上下文里真阻塞，由别的任务唤醒 */
    g_mt_sem_test_rc = 0x7FFFFFFF; g_mt_sem_test_svc = 0u;
    if (g_mt_sem_ok) {
      ((mt_sem_fn2)(uintptr_t)MT_SEM_INIT_ADDR)((void *)g_mt_sem, 0, 0);
      g_mt_sem_svc = 0u; g_mt_sem_waiting = 1u;
      g_mt_sem_test_rc = ((mt_sem_fn0)(uintptr_t)MT_SEM_WAIT_ADDR)((void *)g_mt_sem);
      g_mt_sem_waiting = 0u; g_mt_sem_test_svc = g_mt_sem_svc;
    }
  }
  else if (c == 11u) {   /* ★ v26：一键清空 = 缓存 + 待解析队列 + 中止标志 */
    mt_zero(g_mt_cache, (unsigned)sizeof(g_mt_cache));
    g_mt.cache_count = 0u; g_mt_cache_tick = 0u;
    g_mt_pend_head = 0u; g_mt_pend_tail = 0u;
    g_mt.dns_abort_req = 0u;
  }
  else if (c == 14u) tun_sd_toggle();   /* ★ v52：socket / raw-TUN 传输切换 */
  else if (c == 15u) tun_sd_reset();    /* ★ v52：socket 收发状态复位 */
  else if (c == 16u) tun_sd_selftest(); /* ★ v57：一键 socket 自检（结果看 tunsvc st*_*）*/
  else if (c == 17u) tun_sd_inject_selftest(); /* ★ v62：造回包→注入→收→验缓存（inj_*）*/
  else if (c == 18u) tun_td_selftest(); /* ★ v68：TCP-DNS(RFC 7766) 自检（td_*）*/
  else if (c == 19u) tun_netreg_selftest(); /* ★ v77：注册 netdev（reg_*）；唯一碰 registrar 的命令 */
  else if (c == 20u) tun_bridge_inject_locked(); /* ★ v79：宿主桥喂回包（g_rxbuf+bridge_len）*/
  else if (c == 21u) tun_query_named_locked();  /* ★ v81：查 g_rxbuf 里的域名（宿主桥配套）*/
  else if (c == 22u) tun_iob_grow();            /* ★ v83：IOB 池 24→88（窗口 4508→16384）*/
  else if (c == 23u) tun_ip_fake_lease();       /* ★ v83：装测试租约（模拟器跑 curl 用）*/
  else if (c == 24u) tun_netin_inject_locked(); /* ★ v90：真栈注入（NETDEV_INPUT），用于定位 g_iob_count */
  else if (c == 25u) (void)mt_navail_probe();        /* ★v98：调一次 iob_navail，看返回 24 还是 88 */
  else if (c == 26u) (void)mt_navail_arm();          /* ★v98：装 iob_navail 钩子（返回 +64）*/
}
#endif /* P67TC_MPU_TRAP_H */
