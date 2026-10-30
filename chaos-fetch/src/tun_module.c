/* p67tc 3.101.043 only. See evidence/ and README.md for verified ABI scope.
 * No firmware code patches. Kernel owns every IOB; module owns flat buffers.
 *
 * This translation unit is deliberately built for ARMv8-M/p67tc.  The old
 * ARMv7-M patch SDK is retained below as historical evidence only.  If a
 * future build accidentally defines __ARM_ARCH_7EM__, fail closed instead of
 * silently compiling a writer for the wrong FPB architecture.
 */
#if defined(__ARM_ARCH_7EM__)
#error "p67tc build must not enable the legacy ARMv7-M patch writer"
#endif
#include <stdint.h>
#include <stddef.h>
#include "dhcp_codec.h"
#include "raw_udp.h"
#include "standalone_dns.h"
#include "sock_dns.h"
#include "dns_reply.h"
#include "tcp_dns.h"
struct tun_mod_info { int (*uninitializer)(void *); void *arg; const void *exports; uint32_t nexports; };
__attribute__((used,naked)) void __aeabi_unwind_cpp_pr0(void) { __asm__("bx lr"); }
__attribute__((used,naked)) void __aeabi_unwind_cpp_pr1(void) { __asm__("bx lr"); }
__attribute__((used,naked)) void __aeabi_unwind_cpp_pr2(void) { __asm__("bx lr"); }
#define FW(addr,type) ((type)(uintptr_t)(addr))
typedef int (*register_t)(const char *,void *,unsigned,void *);
typedef int (*netreg_t)(void *,int);
typedef void (*devvoid_t)(void *);
typedef void (*lock_t)(void);
typedef int (*poll_t)(void *,int (*)(void *));
typedef int (*input_t)(void *);
typedef int (*input_bridge_t)(void *, int (*)(void *), uint32_t);
typedef void (*notify_t)(void **,int,unsigned);
typedef int (*work_t)(int,void *,void (*)(void *),void *,uint64_t);
typedef int (*ifcfg_t)(const char *,const uint32_t *);
#define REGISTER FW(0x0C1AFF61,register_t)
#define NETREG FW(0x0C1FC215,netreg_t)
#define CARRIER_ON FW(0x0C1FC949,devvoid_t)
#define CARRIER_OFF FW(0x0C1FC965,devvoid_t)
#define NET_LOCK FW(0x0C201B41,lock_t)
#define NET_UNLOCK FW(0x0C201B51,lock_t)
/* ★ v107：调试总闸 —— 默认关，关的时候 /dev/tunsvc 只回一行，省掉 3.5KB 的 field() 格式化，
 *   也缩短 NET_LOCK 占用。开关由命令控制：
 *       echo 98 > /dev/tunentry   → 打开完整日志
 *       echo 97 > /dev/tunentry   → 关掉（只回一行）
 *   ★★ 刻意**不在模块里 stat("/data/tundebug")**：实测在模块上下文里调 VFS（open/read/write
 *      任一路径）都会把系统挂死（模拟器 A/B 已验证两次）。所以"有没有 /data/tundebug"
 *      这个判断放在自启动脚本里用 nsh 的 [ -f ] 做，模块这边只收一条命令。 */
static int g_dbg_on = 0;                  /* 0 关（默认）/ 1 开 */
#define DEVIF_POLL FW(0x0C1F9689,poll_t)
#define IPV4_INPUT FW(0x0C1F95C9,input_t)
#define NETDEV_INPUT FW(0x0C1FC9F5,input_bridge_t)
#define IPV4_CALLBACK FW(0x0C1F953D,int (*)(void *))
#define NOTIFY FW(0x0C1D0B45,notify_t)
#define WORK_QUEUE FW(0x0C20A59D,work_t)
#define IFADDR_SET FW(0x0C237611,ifcfg_t)
#define IFMASK_SET FW(0x0C2376C1,ifcfg_t)
#define IFDST_SET FW(0x0C237669,ifcfg_t)
#define MAXPKT 1518u
/* v36：SCRATCH 从 4096 降到 2048 —— 只服务单帧缓冲（g_txbuf 由 devif_poll 填一帧、
 * g_rxbuf 只收 len<=MAXPKT 的包），2048 对 1518 上限留 530B 余量；省下的 4KB .bss
 * 用于把 .bss 从 25,664 拉回 24,576 上限以内（tunsvc 诊断页需要新缓冲）。 */
#define SCRATCH 2048u
#define SLOTS 4u
_Static_assert(SCRATCH >= MAXPKT + 64u, "scratch must hold one full frame with margin");
#define TUN_DNS_QUERY 0x7d01
#define TUN_DNS_RESULT 0x7d02
#define TUN_ENTRY_APPLY 0x7d10
#define TUN_ENTRY_DISABLE 0x7d11
#define TUN_ENTRY_RESTORE 0x7d12
#define TUN_ENTRY_SELFTEST 0x7d13
#define TUN_DNS_NAME_MAX 255u
#define U32(p,o) (*(uint32_t *)((unsigned char *)(p)+(o)))
#define U16(p,o) (*(uint16_t *)((unsigned char *)(p)+(o)))
#define ND_FLAGS 0x14
#define ND_IOB 0x2c
#define ND_BUF 0x30
#define ND_LEN 0x38
#define ND_SNDLEN 0x3a
struct fops { void *open,*close,*read,*write,*seek,*ioctl; void *reserved[2]; void *poll; void *tail[7]; };
_Static_assert(sizeof(void *)==4,"ARM32 required");
_Static_assert(offsetof(struct fops,poll)==0x20,"poll ABI");
_Static_assert(offsetof(struct fops,ioctl)==0x14,"ioctl ABI");
_Static_assert(sizeof(struct fops)==0x40,"fops size");
static struct fops g_tun_fops,g_svc_fops,g_entry_fops;
static unsigned char g_netdev[0x200] __attribute__((aligned(8)));
static unsigned char g_work[0x30] __attribute__((aligned(8)));
static unsigned char g_work2[0x30] __attribute__((aligned(8))); /* v37 周期排空器 work 项 */
static unsigned char g_txbuf[SCRATCH] __attribute__((aligned(8)));
static unsigned char g_rxbuf[SCRATCH] __attribute__((aligned(8)));
static unsigned char g_ring[SLOTS][MAXPKT];
static uint16_t g_ringlen[SLOTS];
static unsigned g_head,g_tail;
static void *g_pollfds;
static unsigned g_netdev_up,g_registered;
/* ★★ v77：netdev 注册的诊断盲区（真机"卡在哪"就卡在这里）。
 * tun_netdev_register_once() 的唯一入口是 tun_ioctl(cmd==0x728)，而 main.lua
 * 从头到尾没有发过 0x728。在没有下面这几个计数之前，tunsvc 上
 *   ①「从来没有进程 ioctl 过 /dev/tun」
 *   ②「ioctl 过但没发 0x728」
 *   ③「发了 0x728 但 NETREG 被 registrar 拒」
 * 三种情况的 netdev_ready 都是 0，**完全无法区分** ⇒ 只能靠猜。
 * 现在分开记账：
 *   ioctl_n    tun_ioctl 被调用的总次数（0 ⇒ 没有任何用户态进程碰过 /dev/tun）
 *   ioctl_728  其中 cmd==0x728 的次数（0 而 ioctl_n>0 ⇒ 没人叫注册）
 *   reg_tries  NETREG 实际尝试次数（含已注册时的短路）
 *   reg_last_rc 最近一次 NETREG 的返回值（<0 ⇒ 被 registrar 拒）
 *   reg_short  因 g_netdev_up 已置位而短路返回 0 的次数
 */
static unsigned g_ioctl_n,g_ioctl_728,g_reg_tries,g_reg_short;
static uint32_t g_reg_last_rc;
enum { DHCP_IDLE=0, DHCP_DISCOVER_PENDING=1, DHCP_SELECTING=2,
       DHCP_REQUEST_PENDING=3, DHCP_REQUESTING=4, DHCP_INSTALLING=5,
       DHCP_BOUND=6, DHCP_FAILED=7, DHCP_REJECTED=8 };
static unsigned g_dhcp_state,g_dhcp_epoch,g_ip_configured;
static uint32_t g_dhcp_xid;
static void *g_tun_owner;
static const uint8_t g_dhcp_mac[6]={2,0x67,0x10,0,0,1};
static uint8_t g_dhcp_frame[328];
static struct dhcp_codec_lease g_dhcp_offer,g_dhcp_lease;
static int g_ip_rc,g_mask_rc,g_gw_rc,g_rollback_rc;
static unsigned g_dhcp_discover,g_dhcp_offers,g_dhcp_requests,g_dhcp_acks;
static unsigned g_dhcp_bad,g_dhcp_naks,g_dhcp_queue_full;
static struct standalone_dns g_dns;
/* ★ v41：回包匹配诊断（ID 环 + 逐项 ignored 归因 + 最后一包快照）*/
static struct standalone_dns_diag g_dns_diag;
/* ★ v42：往返诊断 —— 查询发出到回包命中之间的 drain 节拍数（1 拍≈100ms）*/
static unsigned g_dns_start_runs, g_dns_last_rtt_beats, g_dns_rtt_max_beats;
static unsigned g_dns_seq,g_dns_queries,g_dns_sent,g_dns_replies,g_dns_ignored,g_dns_failures;
static unsigned g_dns_resends;   /* ★v69 原地重发次数（对齐 9Pro 的第 2/3 次 send）*/
static uint8_t  g_dns_srv_idx;   /* ★v71 DNS 服务器 0/1 轮转（option6 两条时重试换目标）*/
static int g_dns_last_rc,g_dns_last_transport;
static uint16_t g_dns_sport_n,g_dns_sport_rot;   /* ★v87 DNS 源端口轮转 */
/* ★ v69：看门狗改为按 **drain 拍** 计（≈100ms/拍）。原来 g_mt_dns_wd 数的是
 * "服务调用次数"，而服务函数被 drain worker + tun_read + tun_poll 三处调用 ⇒
 * 桥的 uv 循环一热就在几十毫秒内杀�� WAIT_REPLY 查询。现在记录"本次查询发出时的
 * g_drain_runs"，超时判据 = 当前 drain_runs - 该基准 > MT_DNS_WD_LIMIT（50 拍≈5 秒）。
 * wd_beats 仅作诊断上报（当前这次已等多少拍）。*/
static unsigned g_dns_wd_runs;
/* ★v69 诊断：看门狗实际等到的最大拍数。**判据**：它应 ≈ WD_LIMIT(50)；
 * 若实测远小于 50 就说明查询是"被回包正常收口"的，若 == 50 则说明真超时了。*/
static unsigned g_dns_wd_beats_max;
/* ★ v52：socket 版传输（系统网络栈自带收发）—— 取代"raw 帧经手机桥"的不确定链路。
 * 只在 drain worker（固定线程）里建/用，fd 表上下文恒定；不用 sendto/recvfrom
 * （该固件 sendto 的 tolen 是指针语义，见 src/sock_dns.h 顶注）。 */
static struct sock_dns g_sd;
static unsigned g_sd_polls, g_sd_recv_ok, g_sd_recv_empty, g_sd_recv_err, g_sd_consume_drop;
static unsigned g_sd_send_fail, g_sd_resets;
/* ★★★ v53（真机 v52 事故定案，2026-10-16 16:24 本地）★★★
 * 现象：nslookup 后**整机静默**（08:24:11 起连传感器线程都不再输出）⇒ resetcause=rtc_watchdog。
 * 机制：v52 把 socket I/O 放在 drain worker（队列 0 = hpwork）里。NuttX 的 UDP 发送在
 * 写缓冲满时会**阻塞等 TX 完成**，而 TX 完成只能由 tun_txworker（同样排 hpwork）推进
 * ⇒ hpwork 自己等自己（永久死锁）；hpwork 又是全系统的高优先工作队列，被占死即整机停摆。
 * 修复：socket I/O 全部搬到 **lpwork（队列 1）** —— 与 TX 路径无依赖、同样固定线程
 * （fd 表恒定）。hpwork 里的 drain worker 只做"填请求 + 踢一拍"，绝不碰 socket。
 * 另加 SO_SNDTIMEO（发送也有界）作为第二道保险。 */
struct sd_req { uint16_t len; uint8_t want_open; uint8_t reset; uint8_t server[4]; };
static unsigned char g_work3[0x30] __attribute__((aligned(8)));
static struct sd_req g_sdq;
static unsigned g_sd_runs, g_sd_req_sent, g_sd_req_drop, g_sd_reset_seen;
static unsigned g_sd_stall, g_sd_stall_max;                    /* v53/v58：卡死判据 */
static unsigned g_sd_polls_last;                               /* v58：卡死判据改看轮询推进 */
static unsigned g_sd_wedge;                                    /* v55/v56: 退回 raw 的次数 */
static unsigned g_sd_stall_events, g_sd_kick_busy;             /* v59：中止在飞查询的次数/踢被跳过 */
static unsigned g_sd_tx_enq, g_sd_open_ok, g_sd_open_fail, g_sd_open_warn;      /* v56: raw 发射 / socket 建链 */
static unsigned g_sd_rounds;                                                  /* ★9PRO-ROUND: sendto 轮次 */
/* ★ v57：socket 自检（mt_cmd 16）—— 把收发链路的每一步单独跑一遍并留档，
 * 设备上一条命令即可复现（tunsvc 上报 st0..st4 的 rc/errno），不必再跑完整 DNS 流程。*/
static int g_sd_selftest;
/* ★ v62：注入式收包自检（mt_cmd 17）—— 自己造一条 DNS 回包，从**手机同一个入口**
 *   （NETDEV_INPUT → IPV4_CALLBACK）注入协议栈，再用 socket 收回来、消费、写缓存。
 *   于是"除手机是否转发以外"的整条 RX 链都能在设备上一条命令内自证。*/
static uint8_t  g_inj_req;
/* ★ v62：回归夹具开关（模拟器注入镜像预置=1；真机恒为 0）*/
static volatile uint8_t g_inj_auto;   /* volatile：否则真机恒 0 会被优化掉、符号消失 */
static uint8_t  g_inj_frame[600];
static uint8_t  g_inj_rx[512];
static unsigned g_inj_n;
/* ★ v73：cmd 17 被 netdev 门拒收的次数（inj_n 不涨而这个涨 = 门在工作） */
static unsigned g_inj_deny_n; static uint8_t g_inj_denied;
static int32_t  g_inj_build_len, g_inj_recv_rc, g_inj_consume_rc;
static uint32_t g_inj_sentinel_ok, g_inj_cache_ip;
static unsigned g_sd_self_done;
/* ★ v68：TCP-DNS(端口 53 + 2 字节长度前缀, RFC 7766) 自检 —— 一条命令验证这条路通不通。
 *   动机：手机对 UDP/53 是零星转发，而 TCP/HTTP 通道稳（fetch 的 HTTP 每次都通 ✓）；
 *   9Pro 的 lib_dnsquery 也按传输分支用 16B(UDP) / 28B(TCP 前缀) 两种长度 ✓。*/
static uint8_t  g_td_req;
static unsigned g_td_n;
static int32_t  g_td_socket_rc, g_td_conn_rc, g_td_send_rc, g_td_hdr_rc, g_td_recv_rc;
static int32_t  g_td_len, g_td_consume_rc;
static uint32_t g_td_cache_ip;
static int32_t g_sd_st_rc[6], g_sd_st_err[6];
#define MT_SD_STALL_LIMIT 30u    /* ★v58：在飞查询期间 30 拍没有 recv 轮询推进 ⇒ 判卡死
                                  *（旧版 120 拍 + 看 lpwork 运行次数 ⇒ 把"半拍自排"误判成卡死）*/
#define SD_WQ_ID 1u                     /* 1 = lpwork（0 = hpwork，留给 TX 路径）*/
#define SD_KICK_TICKS 5u                 /* 请求后 ~50ms 内发出去 */
static void tun_sd_kick(void);
static void tun_sd_worker(void *arg);
struct tun_dns_query { uint16_t id; uint16_t reserved; char hostname[TUN_DNS_NAME_MAX+1u]; };
struct tun_dns_result { uint16_t id; uint8_t phase; uint8_t address_count; int32_t status; uint8_t address[4]; uint32_t queries,sent,replies,ignored,failures,last_transport; };
__attribute__((used,section(".data"))) static volatile unsigned g_data_anchor=1;
struct stats {
  uint32_t tx_notify,work_queued,work_runs,work_error,tx_packets,tx_bytes,tx_drop;
  uint32_t read_packets,read_bytes,read_empty,rx_packets,rx_bytes,rx_error,rx_reply;
  uint32_t poll_setup,poll_teardown,poll_notify;
  int32_t last_work_rc,last_input_rc,register_rc,stat_register_rc;
};
static struct stats g_stats;

/* Resolver entry patch guard -------------------------------------------------
 *
 * The p67tc 3.101.043 firmware has four direct Thumb BL callsites into the
 * resolver implementation.  A loadable module cannot replace those calls by
 * exporting a same-named symbol: they are direct branches in firmware .text.
 * This probe therefore records the evidence needed by a future FPB/BES patch,
 * but deliberately performs no write to either controller.  In particular,
 * the remap RAM base and the vendor patch-controller identity are not known.
 */
#define ENTRY_CPUID_ADDR   0xE000ED00u
#define ENTRY_FPB_CTRL     0xE0002000u
#define ENTRY_FPB_REMAP    0xE0002004u
#define ENTRY_FPB_COMP0    0xE0002008u
#define ENTRY_FPB_LAR      0xE0002FB0u
#define ENTRY_FPB_LSR      0xE0002FB4u
#define ENTRY_DEMCR       0xE000EDFCu
#define ENTRY_VTOR        0xE000ED08u
#define ENTRY_DEBUGMON_VECTOR_OFFSET 0x30u
#define ENTRY_CALLSITE0    0x0C1E9448u
#define ENTRY_CALLSITE1    0x0C23E8C2u
#define ENTRY_CALLSITE2    0x0C255C80u
#define ENTRY_CALLSITE3    0x0C2FE632u
/* Raw little-endian words for the original 32-bit Thumb BL instructions. */
#define ENTRY_BL0          0xFD0CF7FFu
#define ENTRY_BL1          0xFD27F7AAu
#define ENTRY_BL2          0xFB48F793u
#define ENTRY_BL3          0xFE6FF6EAu

enum entry_patch_reason_code {
  ENTRY_REASON_NOT_RUN=0,
  ENTRY_REASON_CPUID_UNVERIFIED=1,
  ENTRY_REASON_FPB_UNVERIFIED=2,
  ENTRY_REASON_CALLSITE_MISMATCH=3,
  ENTRY_REASON_MODULE_EXEC_UNVERIFIED=4,
  ENTRY_REASON_REMAP_RAM_UNKNOWN=5,
  ENTRY_REASON_PATCH_BACKEND_UNKNOWN=6,
  ENTRY_REASON_BLOCKED_READ_ONLY=7
};
/* The backend is deliberately an interface, not a guessed MMIO implementation.
 * A verified p67tc backend must provide all three operations and set the
 * validation cookie. Until then every apply/disable/restore operation stops
 * before issuing a write. */
#define ENTRY_BACKEND_COOKIE 0x45564c44u /* "EVLD" */
#define ENTRY_PATCH_ENABLE 0u            /* build-time opt-in; default OFF */
#define ENTRY_PATCH_COUNT 4u
typedef int (*entry_apply_fn)(const uint32_t *,const uint32_t *,const uint32_t *,unsigned,void *);
typedef int (*entry_restore_fn)(const uint32_t *,const uint32_t *,unsigned,void *);
typedef int (*entry_disable_fn)(void *);
typedef int (*entry_read_fn)(uint32_t, uint32_t *, void *);
struct entry_patch_backend {
  entry_read_fn read;
  entry_apply_fn apply;
  entry_restore_fn restore;
  entry_disable_fn disable;
  void *ctx;
  uint32_t kind;
  uint32_t validated;
};
enum entry_patch_state {
  ENTRY_STATE_OFF=0, ENTRY_STATE_PROBED=1, ENTRY_STATE_ARMED=2,
  ENTRY_STATE_APPLIED=3, ENTRY_STATE_DISABLED=4, ENTRY_STATE_RESTORED=5,
  ENTRY_STATE_BLOCKED=6
};
static unsigned g_entry_probe_ran;
static unsigned g_entry_patch_attempted;
static unsigned g_entry_patch_ready;
static unsigned g_entry_callsite_ok;
static uint32_t g_entry_reason;
static uint32_t g_entry_cpuid,g_entry_fp_ctrl,g_entry_fp_remap,g_entry_fp_comp0;
static uint32_t g_entry_fp_comp[8];
static uint32_t g_entry_fp_lar,g_entry_fp_lsr;
static uint32_t g_entry_demcr,g_entry_vtor;
static uint32_t g_entry_debugmon_addr,g_entry_debugmon_vector;
static uint32_t g_entry_vtor_aligned,g_entry_debugmon_read;
static uint32_t g_entry_text0,g_entry_text1,g_entry_data;
static uint32_t g_entry_bl0,g_entry_bl1,g_entry_bl2,g_entry_bl3;
static uint32_t g_entry_state=ENTRY_STATE_OFF;
static uint32_t g_entry_backend_kind;
static uint32_t g_entry_backend_validated;
static uint32_t g_entry_last_op_rc;
static uint32_t g_entry_apply_count,g_entry_disable_count,g_entry_restore_count;
static uint32_t g_entry_backup_valid;
static uint32_t g_entry_target0,g_entry_target1,g_entry_target2,g_entry_target3;
static uint32_t g_entry_backup0,g_entry_backup1,g_entry_backup2,g_entry_backup3;
static const uint32_t g_entry_callsites[ENTRY_PATCH_COUNT]={
  ENTRY_CALLSITE0,ENTRY_CALLSITE1,ENTRY_CALLSITE2,ENTRY_CALLSITE3};
static const uint32_t g_entry_original[ENTRY_PATCH_COUNT]={
  ENTRY_BL0,ENTRY_BL1,ENTRY_BL2,ENTRY_BL3};
/* Zero targets are intentional: no module absolute address or shim ABI has
 * been verified. A future backend must populate replacements before apply. */
static uint32_t g_entry_replacement[ENTRY_PATCH_COUNT];
static struct entry_patch_backend g_entry_backend;

/* Definitions appear below the probe; keep the build freestanding and
 * reject implicit declarations under -Werror. */
static void __attribute__((noinline,used)) tun_notify_readable(void);
static int __attribute__((noinline,used)) tun_netdev_register_once(void);

static uint16_t entry_read16(uint32_t addr) {
  return *(volatile uint16_t *)(uintptr_t)addr;
}
static uint32_t entry_read32(uint32_t addr) {
  return (uint32_t)entry_read16(addr) | ((uint32_t)entry_read16(addr+2u)<<16);
}
static int entry_cpuid_ok(uint32_t v) {
  /* p67tc observed CPUID. Do not accept a generic ARM/Cortex-M33 guess. */
  return v==0x630F1321u;
}
static unsigned entry_fpb_num_code(uint32_t ctrl) {
  return ((ctrl>>4)&0xFu)|((ctrl>>8)&0x70u);
}
static unsigned entry_fpb_num_lit(uint32_t ctrl) {
  return (ctrl>>8)&0xFu;
}
static unsigned entry_fpb_rev(uint32_t ctrl) {
  return (ctrl>>28)&0xFu;
}
static int entry_fpb_ok(uint32_t ctrl) {
  unsigned code=entry_fpb_num_code(ctrl), lit=entry_fpb_num_lit(ctrl);
  return ctrl!=0xFFFFFFFFu && entry_fpb_rev(ctrl)==1u &&
         code!=0u && code<=64u && lit<=16u;
}
#include "fpb_probe.h"
/* Read-only vector-table diagnostics.  VTOR is architected SCB state; derive
 * the DebugMonitor slot at VTOR+0x30, but only dereference an aligned table in
 * the firmware or SRAM windows observed on p67tc.  This helper never writes. */
static int entry_vtor_readable(uint32_t vtor) {
  uint32_t end;
  if(!vtor || (vtor&0x7Fu)!=0u) return 0;
  end=vtor+ENTRY_DEBUGMON_VECTOR_OFFSET+4u;
  if(end<vtor) return 0;
  if((vtor>=0x0C000000u && end<=0x10000000u) ||
     (vtor>=0x20000000u && end<=0x40000000u)) return 1;
  return 0;
}
static const char *entry_reason_text(uint32_t r) {
  switch(r) {
    case ENTRY_REASON_CPUID_UNVERIFIED: return "cpuid_unverified";
    case ENTRY_REASON_FPB_UNVERIFIED: return "fpb_unverified";
    case ENTRY_REASON_CALLSITE_MISMATCH: return "callsite_mismatch";
    case ENTRY_REASON_MODULE_EXEC_UNVERIFIED: return "module_exec_unverified";
    case ENTRY_REASON_REMAP_RAM_UNKNOWN: return "remap_ram_unknown";
    case ENTRY_REASON_PATCH_BACKEND_UNKNOWN: return "patch_backend_unknown";
    case ENTRY_REASON_BLOCKED_READ_ONLY: return "blocked_read_only";
    default: return "not_run";
  }
}
/* Runtime resolver-entry trap is included here because it needs the read-only
 * probe helpers above and the entry-table statics declared earlier.
 *
 * AUTO ARM IS OFF on purpose: the trap is armed in three individually
 * revertible stages from the shell (see mt_cmd_poll) so that any problem is
 * bisectable and a bad stage can never wedge the boot.  Set to 1 only once the
 * stages have been proven on the device. */
#include "mpu_trap.h"
#define TUN_MT_AUTO_ARM 0u
static const char *mt_reason_text(uint32_t r) {
  switch(r) {
    case MT_R_NONE: return "armed_ok";
    case MT_R_NOT_PROBED: return "not_probed";
    case MT_R_CPUID: return "cpuid_unverified";
    case MT_R_CALLSITE: return "bl_site_mismatch";
    case MT_R_CODE_WORDS: return "entry_words_mismatch";
    case MT_R_VECTOR_UNREADABLE: return "vector_unreadable";
    case MT_R_NO_FREE_REGION: return "no_free_mpu_region";
    case MT_R_MPU_TYPE: return "mpu_type_unverified";
    case MT_R_CTRL_REJECTED: return "ctrl_or_regions_rejected";
    case MT_R_SELFTEST: return "selftest_failed";
    case MT_R_DISARMED: return "disarmed";
    case MT_R_TRAP_STORM: return "trap_storm_disarmed";
    case MT_R_MEMFAULTENA_REFUSED: return "memfaultena_refused";
    case MT_R_MPU_ENABLE_REFUSED: return "mpu_enable_refused";
    case MT_R_PREFLIGHT_FAILED: return "flash_preflight_failed";
    case MT_R_MM_NOT_DELIVERED: return "memmanage_not_delivered";
    case MT_R_MM_NOT_RECOVERED: return "memmanage_not_recovered";
    case MT_R_TRAP_NOT_TAKEN: return "trap_not_taken";
    case MT_R_SUBSTITUTION_FAILED: return "substitution_failed";
    case MT_R_FAILSAFE: return "failsafe_rolled_back";
    case MT_R_STUB_UNUSABLE: return "stub_not_covrable";
    case MT_R_USER_DISARM: return "disarmed_by_user";
    case MT_R_NOT_ARMED: return "probe_only_not_armed";
    case MT_R_TRAP_TEST_FAILED: return "native_resolve_failed";
    case MT_R_REGION_ARMED: return "region_armed_no_fault_yet";
    default: return "unknown";
  }
}
static int entry_backend_ready(void) {
  return g_entry_backend.validated==ENTRY_BACKEND_COOKIE &&
         g_entry_backend.read!=0 &&
         g_entry_backend.apply!=0 && g_entry_backend.restore!=0 &&
         g_entry_backend.disable!=0;
}
static int entry_targets_ready(void) {
  unsigned i;
  for(i=0u;i<ENTRY_PATCH_COUNT;++i)
    if(g_entry_replacement[i]==0u || g_entry_replacement[i]==g_entry_original[i]) return 0;
  return 1;
}
static int entry_prereq(void) {
  if(!g_entry_probe_ran) { g_entry_reason=ENTRY_REASON_NOT_RUN; return -11; }
  if(!entry_cpuid_ok(g_entry_cpuid)) { g_entry_reason=ENTRY_REASON_CPUID_UNVERIFIED; return -95; }
  if(!entry_fpb_ok(g_entry_fp_ctrl)) { g_entry_reason=ENTRY_REASON_FPB_UNVERIFIED; return -95; }
  if(!g_entry_callsite_ok) { g_entry_reason=ENTRY_REASON_CALLSITE_MISMATCH; return -74; }
  if((g_entry_text0&1u)==0u || (g_entry_text1&1u)==0u || g_entry_data==0u) {
    g_entry_reason=ENTRY_REASON_MODULE_EXEC_UNVERIFIED; return -95;
  }
  /* Remap base and executable state are intentionally not inferred from the
   * candidate FPB registers. They require a verified backend registration. */
  if(!g_entry_backend.ctx || !g_entry_backend.kind) {
    g_entry_reason=ENTRY_REASON_REMAP_RAM_UNKNOWN; return -95;
  }
  if(!entry_backend_ready()) { g_entry_reason=ENTRY_REASON_PATCH_BACKEND_UNKNOWN; return -95; }
  if(!entry_targets_ready()) { g_entry_reason=ENTRY_REASON_MODULE_EXEC_UNVERIFIED; return -95; }
  return 0;
}
static void entry_capture_backup(void) {
  g_entry_backup0=g_entry_bl0; g_entry_backup1=g_entry_bl1;
  g_entry_backup2=g_entry_bl2; g_entry_backup3=g_entry_bl3;
  g_entry_backup_valid=(g_entry_callsite_ok!=0u);
}
/* Candidate operation API. These functions are intentionally reachable only
 * through private ioctl opcodes and remain blocked while ENTRY_PATCH_ENABLE=0
 * or while no validated backend is bound. */
static int entry_patch_apply(void) {
  int rc=entry_prereq();
  g_entry_patch_attempted=1u;
  if(rc<0 || !ENTRY_PATCH_ENABLE) {
    if(!ENTRY_PATCH_ENABLE) g_entry_reason=ENTRY_REASON_BLOCKED_READ_ONLY;
    g_entry_state=ENTRY_STATE_BLOCKED; g_entry_last_op_rc=(uint32_t)(rc<0?rc:-95); return rc<0?rc:-95;
  }
  entry_capture_backup();
  rc=g_entry_backend.apply(g_entry_callsites,g_entry_original,g_entry_replacement,ENTRY_PATCH_COUNT,g_entry_backend.ctx);
  g_entry_last_op_rc=(uint32_t)rc;
  if(rc<0) { g_entry_state=ENTRY_STATE_BLOCKED; return rc; }
  g_entry_patch_ready=1u; g_entry_state=ENTRY_STATE_APPLIED; ++g_entry_apply_count; return 0;
}
static int entry_patch_disable(void) {
  int rc;
  if(g_entry_state!=ENTRY_STATE_APPLIED || !entry_backend_ready()) {
    g_entry_state=ENTRY_STATE_BLOCKED; g_entry_last_op_rc=(uint32_t)-95; return -95;
  }
  rc=g_entry_backend.disable(g_entry_backend.ctx); g_entry_last_op_rc=(uint32_t)rc;
  if(rc<0) return rc;
  g_entry_patch_ready=0u; g_entry_state=ENTRY_STATE_DISABLED; ++g_entry_disable_count; return 0;
}
static int entry_patch_restore(void) {
  int rc;
  if(!g_entry_backup_valid || !entry_backend_ready()) {
    g_entry_state=ENTRY_STATE_BLOCKED; g_entry_last_op_rc=(uint32_t)-95; return -95;
  }
  rc=g_entry_backend.restore(g_entry_callsites,g_entry_original,ENTRY_PATCH_COUNT,g_entry_backend.ctx);
  g_entry_last_op_rc=(uint32_t)rc;
  if(rc<0) return rc;
  g_entry_patch_ready=0u; g_entry_state=ENTRY_STATE_RESTORED; ++g_entry_restore_count; return 0;
}
static void entry_patch_probe(void) {
  unsigned funcs_ok;
  g_entry_probe_ran=1u;
  g_entry_patch_attempted=0u;
  g_entry_patch_ready=0u;
  g_entry_reason=ENTRY_REASON_CPUID_UNVERIFIED;
  g_entry_cpuid=*(volatile uint32_t *)(uintptr_t)ENTRY_CPUID_ADDR;
  g_entry_fp_ctrl=*(volatile uint32_t *)(uintptr_t)ENTRY_FPB_CTRL;
  g_entry_fp_remap=*(volatile uint32_t *)(uintptr_t)ENTRY_FPB_REMAP;
  g_entry_fp_lar=entry_read32(ENTRY_FPB_LAR);
  g_entry_fp_lsr=entry_read32(ENTRY_FPB_LSR);
  g_entry_fp_comp0=0u;
  { unsigned i; for(i=0u;i<8u;++i) g_entry_fp_comp[i]=entry_read32(ENTRY_FPB_COMP0+4u*i); }
  g_entry_fp_comp0=g_entry_fp_comp[0];
  g_entry_demcr=entry_read32(ENTRY_DEMCR);
  g_entry_vtor=entry_read32(ENTRY_VTOR);
  g_entry_vtor_aligned=(g_entry_vtor!=0u && (g_entry_vtor&0x7Fu)==0u);
  g_entry_debugmon_addr=g_entry_vtor+ENTRY_DEBUGMON_VECTOR_OFFSET;
  g_entry_debugmon_vector=0u;
  g_entry_debugmon_read=0u;
  if(entry_vtor_readable(g_entry_vtor)) {
    g_entry_debugmon_vector=entry_read32(g_entry_debugmon_addr);
    g_entry_debugmon_read=1u;
  }
  g_entry_bl0=entry_read32(ENTRY_CALLSITE0);
  g_entry_bl1=entry_read32(ENTRY_CALLSITE1);
  g_entry_bl2=entry_read32(ENTRY_CALLSITE2);
  g_entry_bl3=entry_read32(ENTRY_CALLSITE3);
  g_entry_callsite_ok=(g_entry_bl0==ENTRY_BL0 && g_entry_bl1==ENTRY_BL1 &&
                       g_entry_bl2==ENTRY_BL2 && g_entry_bl3==ENTRY_BL3);
  g_entry_text0=(uint32_t)(uintptr_t)&tun_notify_readable;
  g_entry_text1=(uint32_t)(uintptr_t)&tun_netdev_register_once;
  g_entry_data=(uint32_t)(uintptr_t)&g_data_anchor;
  g_entry_backend_kind=g_entry_backend.kind;
  g_entry_backend_validated=g_entry_backend.validated;
  entry_capture_backup();
  g_entry_state=ENTRY_STATE_PROBED;
  funcs_ok=((g_entry_text0&1u)!=0u && (g_entry_text1&1u)!=0u && g_entry_data!=0u);
  if(!entry_cpuid_ok(g_entry_cpuid)) return;
  g_entry_reason=ENTRY_REASON_FPB_UNVERIFIED;
  if(!entry_fpb_ok(g_entry_fp_ctrl)) return;
  g_entry_reason=ENTRY_REASON_CALLSITE_MISMATCH;
  if(!g_entry_callsite_ok) return;
  g_entry_reason=ENTRY_REASON_MODULE_EXEC_UNVERIFIED;
  if(!funcs_ok) return;
  /* A module function pointer is not proof of executable remap RAM. */
  g_entry_reason=ENTRY_REASON_REMAP_RAM_UNKNOWN;
  /* No remap base, MPU/XN state, or vendor patch backend has been verified. */
}
static void copy(unsigned char *d,const unsigned char *s,unsigned n) { while(n--) *d++=*s++; }
static int full(void);
static int full(void) { return (g_head+1u)%SLOTS==g_tail; }
static void __attribute__((noinline,used)) tun_notify_readable(void) {
  if(g_pollfds) { ++g_stats.poll_notify; NOTIFY(&g_pollfds,1,1); }
}
static uint32_t ip4_le(const uint8_t *p) {
  return (uint32_t)p[0]|((uint32_t)p[1]<<8)|((uint32_t)p[2]<<16)|((uint32_t)p[3]<<24);
}
/* ★ v84：TCP 探针环 —— 真机上只能在失败**之后**看现场，所以把最近 4 个 TCP 包的
 * 头部原样留在 .bss 里，失败后 dump 出来就能还原握手/请求/响应到底走到哪一步。
 * 记录点：tun_enqueue_locked（上行，dir=0）+ tun_write（下行，dir=1）。
 * 布局固定 20 B：tick seq ack sport dport len flags dir —— 直接 dump_hex 出来。 */
#define TCPR_N 20u
struct tun_tcpr {
  uint32_t seq, ack;
  uint16_t sport, dport, len;
  uint8_t  flags, dir;
};
static struct tun_tcpr g_tcpr[TCPR_N];
static volatile uint32_t g_tcpr_head;
static void tcp_probe_note(const uint8_t *p, unsigned n, uint8_t dir) {
  uint32_t ihl, thl, tot;
  struct tun_tcpr *r;
  if(!p || n<40u || (p[0]>>4)!=4u || p[9]!=6u) return;
  ihl=(uint32_t)(p[0]&15u)*4u;
  if(ihl<20u || ihl+20u>n) return;
  thl=(uint32_t)(p[ihl+12u]>>4)*4u;
  if(thl<20u || ihl+thl>n) return;
  tot=((uint32_t)p[2]<<8)|p[3];
  r=&g_tcpr[g_tcpr_head%TCPR_N];
  r->sport=(uint16_t)(((uint16_t)p[ihl]<<8)|p[ihl+1u]);
  r->dport=(uint16_t)(((uint16_t)p[ihl+2u]<<8)|p[ihl+3u]);
  r->seq=((uint32_t)p[ihl+4u]<<24)|((uint32_t)p[ihl+5u]<<16)|
         ((uint32_t)p[ihl+6u]<<8)|p[ihl+7u];
  r->ack=((uint32_t)p[ihl+8u]<<24)|((uint32_t)p[ihl+9u]<<16)|
         ((uint32_t)p[ihl+10u]<<8)|p[ihl+11u];
  r->flags=p[ihl+13u];
  r->len=(uint16_t)((tot>=ihl+thl)?(tot-ihl-thl):0u);
  r->dir=dir;
  g_tcpr_head++;
}
static int tun_enqueue_locked(const uint8_t *p,unsigned n) {
  if(!p || n>MAXPKT || full()) return -28;
  tcp_probe_note(p,n,0u);
  copy(g_ring[g_head],p,n); g_ringlen[g_head]=(uint16_t)n;
  g_head=(g_head+1u)%SLOTS; ++g_stats.tx_packets; g_stats.tx_bytes+=n;
  tun_notify_readable(); return 0;
}
static void tun_dhcp_emit_locked(void) {
  static const uint8_t zero[4]={0,0,0,0},broadcast[4]={255,255,255,255};
  size_t plen=0,n=0; unsigned type; int rc;
  if(g_dhcp_state!=DHCP_DISCOVER_PENDING && g_dhcp_state!=DHCP_REQUEST_PENDING) return;
  if(full()) { ++g_dhcp_queue_full; return; }
  type=g_dhcp_state==DHCP_DISCOVER_PENDING?DHCP_CODEC_DISCOVER:DHCP_CODEC_REQUEST;
  rc=dhcp_codec_build(g_dhcp_frame+28,sizeof(g_dhcp_frame)-28,g_dhcp_xid,g_dhcp_mac,
      (uint8_t)type,g_dhcp_offer.address,g_dhcp_offer.server,&plen);
  if(!rc) rc=raw_udp_build(g_dhcp_frame,sizeof(g_dhcp_frame),zero,broadcast,68,67,
      (uint16_t)g_dhcp_xid,g_dhcp_frame+28,plen,&n);
  if(rc || tun_enqueue_locked(g_dhcp_frame,(unsigned)n)) { g_dhcp_state=DHCP_FAILED; return; }
  if(type==DHCP_CODEC_DISCOVER) { ++g_dhcp_discover; g_dhcp_state=DHCP_SELECTING; }
  else { ++g_dhcp_requests; g_dhcp_state=DHCP_REQUESTING; }
}
static void tun_dhcp_start_locked(void) {
  standalone_dns_fail(&g_dns,-125); g_dns.phase=STANDALONE_DNS_IDLE;
  g_dns_last_rc=0; g_dns_last_transport=0;
  ++g_dhcp_epoch; g_dhcp_xid=0x67104310u+g_dhcp_epoch;
  dhcp_codec_clear(&g_dhcp_offer,sizeof(g_dhcp_offer));
  dhcp_codec_clear(&g_dhcp_lease,sizeof(g_dhcp_lease));
  g_ip_configured=0; g_ip_rc=g_mask_rc=g_gw_rc=-11; g_rollback_rc=0;
  g_dhcp_state=DHCP_DISCOVER_PENDING; tun_dhcp_emit_locked();
}
/* ★ v47/v52：成功收口的唯一实现（raw 帧路径与 socket 路径共用）。
 * 旧版这俩只清在超时/中止路径 ⇒ 成功后 phase 停在 DONE(≠0)，看门狗继续数，
 * 40 拍后对**已完成的查询**做"幽灵中止"：bogus 失败计数 + 把已解析好的名字重新
 * 塞回 6 槽队列；若这 40 拍期间用户发起下一次解析，这记中止还会**杀掉那次正在
 * 等回包的在飞查询**（回包被丢、只剩重试）—— 真机症状即"连续解析时好时坏"。 */
static int __attribute__((noinline)) tun_dns_settle_locked(int rc,unsigned *ignored_ctr) {
  if(rc==STANDALONE_DNS_IGNORED) { if(ignored_ctr) ++(*ignored_ctr); return 0; }
  if(rc==STANDALONE_DNS_CONSUMED) {
    ++g_dns_replies; g_dns_last_rc=g_dns.status;
    { unsigned b=g_drain_runs-g_dns_start_runs; g_dns_last_rtt_beats=b;
      if(b>g_dns_rtt_max_beats) g_dns_rtt_max_beats=b; }
    /* Publish the answer for the resolver trap. Network byte order is kept. */
    if(g_dns.result.address_count)
      mt_cache_insert(g_dns.hostname,g_dns.result.addresses[0].bytes,
                      g_dns.result.addresses[0].ttl);   /* ★ v45：按应答 TTL 过期（对齐 9Pro）*/
    g_dns_srv_idx=0u;   /* ★v71：新查询从第 1 条 DNS 起 */
    g_mt_inflight_name[0]=0; g_mt_inflight_tries=0u; g_mt_dns_wd=0u;
    /* ★v69：同步看门狗基准，避免 DONE 之后残留旧基准污染下一次判定 */
    g_dns_wd_runs=g_drain_runs;
    return 1;
  }
  ++g_dns_failures; g_dns_last_rc=g_dns.status; return 1;
}
static int tun_dns_receive_locked(const uint8_t *p,unsigned n) {
  int rc=standalone_dns_consume(&g_dns,p,n,&g_dns_diag);
  g_dns_last_transport=(int)g_dns.last_transport_status;
  return tun_dns_settle_locked(rc,&g_dns_ignored);
}
/* ★ v52：socket 版回包 —— 内核已按四元组把回包过滤好，这里只吃 DNS 负载。*/
static int __attribute__((noinline)) tun_dns_consume_payload_locked(const uint8_t *p,size_t n) {
  int rc=standalone_dns_consume_payload(&g_dns,p,n,&g_dns_diag);
  g_dns_last_transport=(int)g_dns.last_transport_status;
  return tun_dns_settle_locked(rc,&g_sd_consume_drop);
}
#define SD_EAGAIN 11
static int tun_dns_start_locked(const struct tun_dns_query *q) {
  uint8_t src[4],dns[4]; unsigned i; uint16_t id; int rc; size_t n;
  if(!q || !g_netdev_up || g_dhcp_state!=DHCP_BOUND || !g_ip_configured) return -11;
  if(g_dns.phase==STANDALONE_DNS_WAIT_REPLY) return -16;
  /* ★ v71：DNS 服务器 failover —— option6 有两条时，重试轮转目标。
   * 旧版恒用第 1 条：第 1 条不通时 3 次重试全废。*/
  for(i=0u;i<4u;++i) {
    src[i]=g_dhcp_lease.address[i];
    dns[i]=(g_dns_srv_idx && g_dhcp_lease.dns2_present) ? g_dhcp_lease.dns2[i]
                                                      : g_dhcp_lease.dns[i];
  }
  if((dns[0]|dns[1]|dns[2]|dns[3])==0u) return -6;
  id=q->id; if(id==0u) { ++g_dns_seq; id=(uint16_t)(0xd000u+(g_dns_seq&0x0fffu)); }
  /* ★★★ v87：源端口必须轮转 —— 手机 NetProxy 按四元组缓存 UDP 会话，会话一旦进
   * state 2 就永远 "UDP ignore session from source_ip/53167 to dest_ip/53 state 2"，
   * 于是固定端口 53167 的后续查询全部被丢（真机 23:24 实测：27 发 1 回）。
   * 每查一次换一个端口 ⇒ 每次都是新会话。窗口取 64 个，远小于手机 409 的上限。*/
  /* 端口窗取 384 个（49152..49535）：手机侧 max sessions=409，取 384 保证不撞上限；
   * 实测同一个四元组只会建一次会话，所以每个端口只当"一次性票据"用。*/
  /* ★ v89：照 9Pro 的实测行为放大端口窗。
   * 9Pro 实测（23:49:34/37）：new 8016 → close(300ms) → new 8017 → close …
   * 即"一查询一端口、用完就关、从不复用"。我们的 raw 帧关不掉会话，
   * 只能靠"不复用端口"绕开 state 2；384 个太窄（几十分钟就绕回来），
   * 改成 20000 个（32768..52767），循环周期远大于会话寿命。*/
  g_dns_sport_rot = (uint16_t)(32768u + (g_dns_sport_n++ % 20000u));
  rc=standalone_dns_start(&g_dns,q->hostname,id,src,dns,g_dns_sport_rot); g_dns_last_rc=rc; ++g_dns_queries;
  if(rc==DNS_CODEC_OK) {
    standalone_dns_diag_push(&g_dns_diag,id); g_dns_start_runs=g_drain_runs;
    /* ★v69：看门狗基准 —— 本次查询"发出这一刻"的 drain 拍。超时判据
       = g_drain_runs - g_dns_wd_runs > MT_DNS_WD_LIMIT，与调用频率无关。*/
    g_dns_wd_runs=g_drain_runs;
  }
  if(rc!=DNS_CODEC_OK) { ++g_dns_failures; return rc; }
  n=g_dns.tx_length;
  if(g_sd.enabled) {
    /* ★9PRO-ROUND: 仿 9Pro 机制 —— socket 只发 **DNS 负载**（不塞 raw IP 帧），
     * 交给内核 UDP/IP 栈；不再走 TUN 环注入。
     * 9Pro 配方：sendto(query,len,0,&dns53,16) → recvfrom(buf,512,...) → 失败重试 3 次。
     * 实际收发在 lpwork 里做（见 tun_sd_worker 的 sd_round 调用），这里只登记目标。 */
    for(i=0u;i<4u;++i) g_sd.server[i]=dns[i];       /* 诊断用：当前 DNS 服务器 */
    g_sdq.want_open=1u;                              /* 让 lpwork 去 socket（只能在那里碰）*/
    tun_sd_kick();
    /* 计入"已发出"（真正 sendto 在 lpwork 里，回填 g_sd_sendto_rc）
     * v70：这里不再 +g_dns_sent，改由 sd_round 成功时计 —— 避免"发出"与"到达"混淆。 */
    return 0;
  }
  n=g_dns.tx_length; rc=tun_enqueue_locked(g_dns.tx_frame,(unsigned)n);
  if(rc) { standalone_dns_fail(&g_dns,rc); ++g_dns_failures; g_dns_last_rc=rc; return rc; }
  ++g_dns_sent; return 0;
}
static void tun_dns_result_copy(struct tun_dns_result *r) {
  unsigned i;
  if(!r) return;
  for(i=0u;i<sizeof(*r);++i) ((uint8_t *)r)[i]=0u;
  r->id=g_dns.query_id; r->phase=g_dns.phase; r->status=g_dns.status;
  r->address_count=g_dns.result.address_count;
  if(g_dns.result.address_count) for(i=0u;i<4u;++i) r->address[i]=g_dns.result.addresses[0].bytes[i];
  r->queries=g_dns_queries; r->sent=g_dns_sent; r->replies=g_dns_replies;
  r->ignored=g_dns_ignored; r->failures=g_dns_failures; r->last_transport=(uint32_t)g_dns_last_transport;
}
/* ★v69：**原地重发**当前在飞的名字（对齐 9Pro 的"失败立刻重发、不排队"）。
 * 与 mt_pend_requeue 的区别：名字不进队列，**下一拍就重新上线**，因此
 * "3 次 × 5 秒"的窗口是连续的，不会被队列里其它名字插队拖长。
 * 必须已持有 net_lock（调用方 tun_service_mt_locked 已持有）。*/
static int tun_net_ready(void);   /* v69 前向声明（定义在下方服务路径段）*/
/* ★★★ v82：改成返回 int —— 调用方必须知道"重发到底有没有成功"。
 * 原来返回 void，超时处理就无条件 `g_mt_inflight_name[0]=0`，
 * 于是**第二次超时时名字已经空了 ⇒ 不再重发** ⇒ 设计的 MT_TRIES_MAX=3 实际只生效 1 次。
 * 真机 v81 的读数正是这个签名：`dns_resends=3` 而 `dns_stuck_drops=0`
 * （3 个不同的名字各只重发了 1 次，没有一个走到"重试用尽"）。
 * 返回：0=已成功重发（调用方保留 inflight_name）；非 0=未重发/已退回队列（调用方清空）。*/
static int tun_dns_resend_inplace_locked(unsigned tries) {
  struct tun_dns_query rq;
  unsigned i;
  if(!g_mt_inflight_name[0]) return -1;
  if(g_dns.phase==STANDALONE_DNS_WAIT_REPLY) return -2;   /* 已有在飞，不重复发 */
  if(!tun_net_ready()) { mt_pend_requeue(g_mt_inflight_name,tries); return -3; }
  for(i=0u;i<sizeof(rq);++i) ((uint8_t *)&rq)[i]=0u;
  for(i=0u;i<TUN_DNS_NAME_MAX && g_mt_inflight_name[i];++i) rq.hostname[i]=g_mt_inflight_name[i];
  rq.hostname[i]=0u;
  if(tun_dns_start_locked(&rq)==0) {
    ++g_dns_resends;
    /* ★v71：重试打另一条 DNS（仅当 option6 真的给了两条） */
    if(g_dhcp_lease.dns2_present) g_dns_srv_idx ^= 1u;
    g_mt_inflight_tries=tries;
    return 0;
  }
  /* 发不出去（网络刚掉/队列忙）⇒ 退回排队重试，绝不丢名 */
  mt_pend_requeue(g_mt_inflight_name,tries);
  return -4;
}
/* Task-context driver for the resolver trap. Runs under net_lock from the data
 * path: it executes shell commands deposited in g_mt_cmd and turns a name the
 * trap could not answer into a real DNS transaction, so the next resolution of
 * that name is served from the cache. */
/* ★ v39 服务路径计数：把"桥没跑 / 网络没起 / 查询发不出"三种静默失败一次分清。
 * 全部经 /dev/tunsvc 上报（tunstat 3072B 装不下 157 行，尾部字段事实被截断）。*/
static unsigned g_svc_calls, g_svc_q_none, g_svc_netdown, g_svc_net_ok;
static unsigned g_svc_starts, g_svc_busy, g_svc_startfail, g_svc_skip_cached;
/* v39：网络就绪的唯一定义（服务循环与陷阱慢路径共用；mpu_trap.h 有前向声明） */
static int tun_net_ready(void) {
  return (g_netdev_up && g_dhcp_state == DHCP_BOUND && g_ip_configured) ? 1 : 0;
}
/* ★ v44：同步等待的"直发" —— 由陷阱慢路径在释放调用者 net_lock 之后调用（内部自己拿锁）。
 * 目的：不等 drain 节拍（~100ms）、不排队列旧名字后面，本次查询立刻上路。
 * 队列入队照旧保留（失败时作为异步兜底重试）。*/
static unsigned g_slow_kicks;
static void tun_drain_kick(void);   /* v52：socket 路径只用它把下一拍踢醒 */
static void tun_dns_start_now(const char *name) {
  struct tun_dns_query q;
  unsigned i;
  if(!name || !name[0]) return;
  /* ★ v52：socket 路径**绝不在陷阱慢路径里碰** —— socket 的 fd 属于"当前任务组"
   * 的 fd 表，而这里是被抢占的调用者线程；建/用都会落到错的表上。所以只把节拍
   * 踢醒，让 drain worker（固定线程）在下一拍做完。代价：冷查询晚 ≤1 拍。 */
  if(g_sd.enabled) { tun_drain_kick(); return; }
  NET_LOCK();
  if(tun_net_ready() && (g_dns.phase==STANDALONE_DNS_IDLE || g_dns.phase==STANDALONE_DNS_DONE)) {
    for(i=0u;i<sizeof(q);++i) ((uint8_t *)&q)[i]=0u;
    for(i=0u;i<TUN_DNS_NAME_MAX && name[i];++i) q.hostname[i]=name[i];
    q.hostname[i]=0u;
    if(tun_dns_start_locked(&q)==0) ++g_slow_kicks;
  }
  NET_UNLOCK();
}
/* ★ v36 看门狗参数：8 次服务太激进（桥的 uv 循环热时正常回包前就会误杀），且 v35
 * 误杀 = 丢名。改成"保守阈值 + 回队重试"：达到 256 次服务仍未完成 ⇒ 抛弃这次
 * 查询，把名字放回队尾（重试计数 +1），累计 5 次仍无回包才丢弃（mt_dns_stuck_drops）。*/
/* ★★ v69 根因修正（v66/v67 都改错了方向）：**看门狗原来按"服务调用次数"计数**，
 * 而 tun_service_mt_locked() 有三个调用点 —— drain worker(每 ~100ms 一次)、tun_read()、
 * tun_poll()，后两个是**桥主动调的**（uv 循环热时高频）。⇒ "5 拍"根本不是 5 秒：
 * 桥一热，WAIT_REPLY 的查询在几十毫秒内就被杀掉，回包随后到达时 phase 已是 IDLE
 * ⇒ 被 ign_idle 拒掉。这正是真机 v65 的现象（34 查询只成 2、ign_idle=8、
 *   good_rtt_beats=10 即真回包要等 10 拍才到 —— 而窗口只有 5）。
 * ⇒ 改为按 **g_drain_runs**（≈100ms/拍，真实时钟，与 v38 慢路径同源）计拍。
 * 50 拍 ≈ 5 秒，正好等于 9Pro 的 SO_RCVTIMEO=5s。*/
#define MT_DNS_WD_LIMIT 50u
/* ★ v67：对齐 9Pro(n67) lib_dnsquery 的重试结构 —— 那边是
 *   socket + SO_RCVTIMEO/SO_SNDTIMEO=5s + connect，然后**最多 3 次** send/recv：
 *   每次 recv 各自吃满 5 秒超时、失败立刻重发，**没有 sleep 退避** ⇒ 总窗口 ≈15 秒。
 *   我们走异步 raw 发送（不能阻塞在 socket 调用里），等价物就是"外部看门狗 5 秒 × 3 次"
 *   —— ★ v69 修正：那个"5"必须是**秒**（50 drain 拍），不是 5 次服务调用。*/
#define MT_TRIES_MAX    3u
static void tun_drain_kick(void);   /* v37：周期排空器的（重）排程，见服务函数之后 */
static void tun_service_mt_locked(void) {
  char nm[MT_NAME_MAX];
  struct tun_dns_query q;
  uint32_t seq=0u, tries=0u;
  unsigned i;
  int rc;
  ++g_svc_calls;
  tun_drain_kick(); /* 兜底重排（已排或链还活着时为空操作），保证任何服务调用都能救活节拍 */
  mt_cmd_poll();
  /* ★ v62 夹具自动触发：g_inj_auto 由模拟器注入镜像预置成 1（真机恒为 0），
   *   首次 drain worker 运行就自动做一次注入自检 ⇒ 模拟器"一次 continue"跑完整条 RX 链。*/
  if (g_inj_auto) { g_inj_auto = 0u; g_inj_req = 1u; }
  /* ★ v31 看门狗：慢路径真阻塞等待时靠这里兜底唤醒（回包不来也绝不永久挂住调用者）*/
  if (g_mt_sem_waiting) {
    if (++g_mt_sem_svc > 60u) {
      g_mt_sem_svc = 0u;
      ((mt_sem_fn0)(uintptr_t)MT_SEM_POST_ADDR)((void *)g_mt_sem);
    }
  } else g_mt_sem_svc = 0u;
  /* ★ v19：handler 里遇到认不出的故障会 failsafe 回滚（安全），但那会让陷阱从此失效 ——
   * 真机 v18 就是这样白丢了一轮。这里在任务上下文把它装回去（有上限，且只在没布防时做）。
   * ★ v49：上限 64 → 实质无限（旧版 64 次用完后，任何一次 failsafe 都变成**永久**失效）；
   * 且 stage_arm 失败时保留 rearm_pending 下次再试（旧版失败一次就此罢休）。 */
  if(g_mt.rearm_pending && !g_mt.armed && g_mt.rearm_count < 0x3FFFFFFFu) {
    ++g_mt.rearm_count;
    g_mt.rearm_pending = 0u;
    if(mt_stage_arm()!=0) g_mt.rearm_pending = 1u;
  }
  /* ★ v53：socket 收发全部在 lpwork（tun_sd_worker）里做 —— 这里只兜底重排一次，
   * 保证 drain 节拍还在时 lpwork 那一拍不会断（它自己也会自排）。*/
  /* ★ v53：lpwork 卡住判据（只上报，不自动切换）—— sd_stall_max 持续增长而
   * sd_lp_runs 不动 ⇒ socket 路径被卡住，用 mt_cmd 14 退回 raw 传输。*/
  if(g_sd.enabled) {
    if(g_sd_polls != g_sd_polls_last) { g_sd_polls_last = g_sd_polls; g_sd_stall = 0u; }
    else if(g_dns.phase==STANDALONE_DNS_WAIT_REPLY) {
      ++g_sd_stall; if(g_sd_stall > g_sd_stall_max) g_sd_stall_max = g_sd_stall;
      /* ★ v55 兜底：lpwork 太久没进展 ⇒ 判"卡死"，自动退回 raw 传输并计数上报，
       * 免得 DNS 永久静默（真机 v54 就是 send 永久阻塞 ⇒ 全灭却只能靠人猜）。*/
      if(g_sd_stall >= MT_SD_STALL_LIMIT) {
        /* ★v59：只中止这次在飞查询（让队列继续走、下一次查询重试 socket 路径），
         * **绝不永久关闭 socket 路径** —— v58 真机就是被这条误触发一枪打死
         * （lpwork 被固件抢走 ≥30 拍 ⇒ 误判卡死 ⇒ sd_en=0 且永不恢复）。*/
        g_sd_stall=0u; ++g_sd_stall_events;
        if(g_dns.phase==STANDALONE_DNS_WAIT_REPLY) {
          standalone_dns_fail(&g_dns,-125); g_dns.phase=STANDALONE_DNS_IDLE;
          if(g_mt_inflight_name[0]) {
            if(g_mt_inflight_tries + 1u < MT_TRIES_MAX)
              mt_pend_requeue(g_mt_inflight_name,g_mt_inflight_tries + 1u);
            else ++g_mt_dns_stuck_drops;
            g_mt_inflight_name[0]=0;
          }
        }
      }
    } else g_sd_stall = 0u;
  }
  if(g_sd.enabled) tun_sd_kick();
  if(!mt_pend_peek(nm,&seq,&tries)) { ++g_svc_q_none; return; }
  if(!tun_net_ready()) { ++g_svc_netdown; return; }
  ++g_svc_net_ok;
  /* ★ v25/v36 等待列表：接力超时 ⇒ 应答等不到了 ⇒ 抛弃它，让队列推进到下一个名字
   * （v38：与看门狗一致——名字回队尾重试，不丢名）*/
  if(g_mt.dns_abort_req) {
    g_mt.dns_abort_req=0u;
    /* ★ v47：只有"确实在飞"才允许中止（v25~v46 是 phase!=0 就下手）—— 否则一个早已
     * 超时者的中止请求，会把**其它**刚发出、正在等回包的查询连坐杀掉，表现为"连续
     * 解析时好时坏"。DONE/FAILED 下这请求已过期，直接丢弃即可。 */
    if(g_dns.phase==STANDALONE_DNS_WAIT_REPLY) {
      g_dns.phase=0u; ++g_dns_failures;
      if(g_mt_inflight_name[0]) {
        if(g_mt_inflight_tries + 1u < MT_TRIES_MAX)
          mt_pend_requeue(g_mt_inflight_name, g_mt_inflight_tries + 1u);
        else ++g_mt_dns_stuck_drops;
        g_mt_inflight_name[0]=0;
      }
    }
  }
  /* ★ v36 DNS 查询看门狗；v44：上限 256→40；★ v69：**基准从"服务调用次数"换成
   * drain 拍**（≈100ms/拍）。旧基准下 tun_read/tun_poll 也会推进计数 ⇒ 桥越热
   * 死得越快；真机 v65 的"34 查询只成 2、ign_idle=8、good_rtt=10 拍"就是这个。
   * 新基准：本次查询发出时的 g_drain_runs 记在 g_dns_wd_runs，超时判据是
   *   g_drain_runs - g_dns_wd_runs > MT_DNS_WD_LIMIT(50 拍 ≈ 5s = 9Pro SO_RCVTIMEO)。
   * ★ v47：只对 WAIT_REPLY（真在飞）与 FAILED（已死待回收）计数 —— DONE(2) 是"已成功
   * 收口"的终端态，绝不能再被当"卡死"数（幽灵中止的第二来源；成功路径已在
   * tun_dns_receive_locked 里把 wd 清零，这里双保险）。 */
  if(g_dns.phase==STANDALONE_DNS_WAIT_REPLY || g_dns.phase==STANDALONE_DNS_FAILED) {
    unsigned wd_beats = g_drain_runs - g_dns_wd_runs;
    g_mt_dns_wd = wd_beats;                    /* ★v69 仅作诊断上报 */
    if(wd_beats > MT_DNS_WD_LIMIT) {
      g_mt_dns_wd=0u; g_dns.phase=0u; ++g_dns_failures; ++g_mt_dns_wd_aborts;
      ++g_dns_wd_beats_max;
      if(wd_beats > g_dns_wd_beats_max) g_dns_wd_beats_max = wd_beats;
      if(g_mt_inflight_name[0]) {
        /* ★v69：对齐 9Pro —— **原地立刻重发**，不再回队尾。
         * 9Pro lib_dnsquery 是同步的：recv 吃满 5s 超时就立刻重发同一个查询，
         * 3 次一气呵成、不排任何队列。旧写法 mt_pend_requeue 把名字塞回 6 槽队尾 ⇒
         * 前面若还堵着别的名字，同一个查询的重试会被推迟十几秒（远超 9Pro 的 15s
         * 总窗口），命中概率就是这么被拖光的。
         * ★★★ v82 修复：原来这里**无条件**清 inflight_name ⇒ 第二次超时名字已空
         * ⇒ 不再重发 ⇒ MT_TRIES_MAX=3 实际只生效 1 次。
         * 现在按重发结果决定：成功就保留名字，让它还能继续重试。*/
        if(g_mt_inflight_tries + 1u < MT_TRIES_MAX) {
          if(tun_dns_resend_inplace_locked(g_mt_inflight_tries + 1u) != 0)
            g_mt_inflight_name[0]=0;   /* 已退回队列/发不出去：由队列接手 */
        } else {
          ++g_mt_dns_stuck_drops;      /* 次数用尽才算丢 */
          g_mt_inflight_name[0]=0;
        }
      }
    }
  } else { g_mt_dns_wd=0u; g_dns_wd_runs=g_drain_runs; }
  if(g_dns.phase==STANDALONE_DNS_WAIT_REPLY) { ++g_svc_busy; return; }
  /* v36：名字已在缓存里（例如陷阱刚查过 / 表盘预喂过）⇒ 不必再发一次查询 */
  if(mt_cache_lookup(nm)) { mt_pend_commit(seq); ++g_svc_skip_cached; return; }
  for(i=0u;i<sizeof(q);++i) ((uint8_t *)&q)[i]=0u;
  for(i=0u;i<TUN_DNS_NAME_MAX && nm[i];++i) q.hostname[i]=nm[i];
  q.hostname[i]=0u;
  rc=tun_dns_start_locked(&q);
  if(rc==0) {
    ++g_svc_starts;
    g_mt_inflight_tries=tries;
    mt_copy(g_mt_inflight_name,nm,MT_NAME_MAX-1u); g_mt_inflight_name[MT_NAME_MAX-1u]=0;
  } else if(rc==-16) ++g_svc_busy;
  else {
    /* ★ v52：启动失败（socket 建链失败 / 发送失败 / 网络刚起 / 无 DNS 服务器）是**可重试**
     * 故障 —— 旧版直接 commit 掉 = 丢名。现在放回队尾重试（次数上限与超时路径一致）。*/
    ++g_svc_startfail;
    if(tries + 1u < MT_TRIES_MAX) { mt_pend_commit(seq); mt_pend_requeue(nm,tries + 1u); return; }
  }
  if(rc!=-16) mt_pend_commit(seq); /* -EBUSY retries the same name later */
}
/* ★ v37 周期排空器 —— v36 真机数据定案：`svc_calls=8` 全部发生在 DHCP 握手期间、之后
 * 桥就睡死了（DHCP 一完成就没流量 ⇒ uv 循环不再 poll /dev/tun）⇒ 队列**不能依赖桥的
 * poll/read 驱动**，否则永远等不到下一次服务（v34/v36 两次都是这个病）。
 * 用工作队列（与既有 txavail 同款、ABI 已在真机验证）自带节拍：worker 执行一次服务，
 * 再把同一个 work 项排到 MT_DRAIN_TICKS 之后 ⇒ 与桥活动完全解耦。
 * 注意：运行时 work_s 的 +0x18（worker 指针）已被调度器清空 ⇒ 可以重排自己
 * （与 txavail 那句 "A running worker can be queued again" 同款语义）。
 * 卸载安全：本项目从不 rmmod（每版都是重启 + insmod），与既有 txavail 工作项同级。*/
#define MT_DRAIN_TICKS 100u   /* ≈1s @10ms tick；若 tick=1ms 则 ≈100ms，都可接受 */
static unsigned g_drain_kicks;   /* g_drain_runs 在 mpu_trap.h（v38 慢路径也读它当节拍）*/
static int g_drain_last_rc;
static void tun_iob_auto_maybe(void);   /* ★v94 定义在 iob_grow 之后 */
static void tun_drain_worker(void *arg) {
  (void)arg;
  ++g_drain_runs;
  NET_LOCK(); tun_service_mt_locked(); /* ★v96：自动扩池已关闭（真机网络操作崩溃）*/
  mt_navail_auto();   /* ★v102：自动装钩子（已加固：disarm/failsafe 会一并关闭）*/ NET_UNLOCK();
  /* 重排走带守卫的 kick：服务入口通常已排好（空操作）；万一那里排队失败，这里再试一次 */
  tun_drain_kick();
}
static void tun_drain_kick(void) {
  if(U32(g_work2,0x18)) return;             /* 已排（worker 正在跑时也算已排）*/
  ++g_drain_kicks;
  g_drain_last_rc=WORK_QUEUE(0,g_work2,tun_drain_worker,0,(uint64_t)MT_DRAIN_TICKS);
}
/* ★★★ v53：socket I/O 专用工作项（队列 1 = lpwork）★★★
 * 为什么不能用 hpwork：UDP 发送在写缓冲满时会阻塞等 TX 完成，而 TX 完成由
 * tun_txworker（队列 0 = hpwork）推进 ⇒ 在 hpwork 里发 = 自己等自己。真机 v52
 * 就是这样：nslookup 后整机静默 → rtc_watchdog 复位。
 * 本 worker 只在 lpwork 里做 socket 收发（可阻塞，但有 SO_RCV/SNDTIMEO 兜底），
 * net_lock 只用来读写模块自己的状态，**绝不跨阻塞调用持有**。
 * 节拍：请求来了 ~50ms 内就发（SD_KICK_TICKS）；在飞时自排半拍继续收；空闲不自排
 * （drain worker 每拍会兜底踢一次）。*/
static void tun_sd_kick(void) {
  if(U32(g_work3,0x18)) { ++g_sd_kick_busy; return; }
  (void)WORK_QUEUE(SD_WQ_ID,g_work3,tun_sd_worker,0,(uint64_t)SD_KICK_TICKS);
}
static void tun_sd_selfarm(void) {
  if(U32(g_work3,0x18)) return;
  (void)WORK_QUEUE(SD_WQ_ID,g_work3,tun_sd_worker,0,(uint64_t)(MT_DRAIN_TICKS/2u));
}
static void tun_sd_worker(void *arg) {
  unsigned char rx[512];
  int rc=0;
  unsigned do_open, do_poll, do_reset, do_self, do_inject, do_td, j;
  uint8_t sopen_srv[4];
  (void)arg;
  ++g_sd_runs;
  NET_LOCK();
  do_open=g_sdq.want_open; g_sdq.want_open=0u;
  do_td=g_td_req; g_td_req=0;
  do_self=g_sd_selftest; g_sd_selftest=0;
  do_inject=g_inj_req; g_inj_req=0;
  for(j=0u;j<4u;++j) sopen_srv[j]=g_dns.dns_ip[j];   /* v60：connect 用 */
  do_reset=g_sdq.reset; g_sdq.reset=0u;
  do_poll=(g_dns.phase==STANDALONE_DNS_WAIT_REPLY) ? 1u : 0u;
  NET_UNLOCK();

  if(do_self) {
    int32_t tv[4]; uint8_t ssa[16]; int sfd; uint8_t rb[64]; unsigned k;
    static const uint8_t zero4[4]={0,0,0,0};
    for(k=0u;k<6u;++k) { g_sd_st_rc[k]=0x7FFFFFFF; g_sd_st_err[k]=0; }
    sfd=((sd_fn_socket)(uintptr_t)SD_FN_SOCKET)(SD_AF_INET,SD_SOCK_DGRAM,0);
    g_sd_st_rc[0]=sfd; g_sd_st_err[0]=sd_errno_get();
    if(sfd>=0) {
      tv[0]=0; tv[1]=0; tv[2]=100000; tv[3]=0;
      g_sd_st_rc[1]=((sd_fn_setsockopt)(uintptr_t)SD_FN_SETSOCKOPT)(sfd,SD_SOL_SOCKET,SD_SO_RCVTIMEO,(void*)tv,(int)SD_TIMEVAL_SIZE);
      g_sd_st_err[1]=sd_errno_get();
      g_sd_st_rc[2]=((sd_fn_setsockopt)(uintptr_t)SD_FN_SETSOCKOPT)(sfd,SD_SOL_SOCKET,SD_SO_SNDTIMEO,(void*)tv,(int)SD_TIMEVAL_SIZE);
      g_sd_st_err[2]=sd_errno_get();
      sd_build_sa(ssa,zero4,(uint16_t)STANDALONE_DNS_SOURCE_PORT);
      /* 先做一次 recv(NULL) 作对照（预期 ENOTCONN=-107，这是 v57 实测到的坑）*/
      g_sd_st_rc[4]=((sd_fn_recv)(uintptr_t)SD_FN_RECV)(sfd,(void*)rb,(int)sizeof(rb),0);
      g_sd_st_err[4]=sd_errno_get();
      g_sd_st_rc[3]=((sd_fn_bind)(uintptr_t)SD_FN_BIND)(sfd,(const void*)ssa,16);
      g_sd_st_err[3]=sd_errno_get();
      /* v61：规范形状 recvfrom(from=&sa, fromlen=&16)；先埋哨兵，收到数据必须被改写 */
      { uint32_t fl=(uint32_t)sizeof(ssa);
        rb[0]=0x5Au; rb[1]=0x5Au; rb[2]=0x5Au; rb[3]=0x5Au;
        ssa[0]=(uint8_t)SD_AF_INET; ssa[1]=0u;
        g_sd_st_rc[5]=((sd_fn_recvfrom)(uintptr_t)SD_FN_RECVFROM)(sfd,(void*)rb,(int)sizeof(rb),0,(void*)ssa,(void*)&fl);
        g_sd_st_err[5]=sd_errno_get();
        /* st4 = 哨兵是否被改写：0=未改写（无数据/收不到）  1=收到真数据 */
        g_sd_st_rc[4]=(rb[0]==0x5Au&&rb[1]==0x5Au&&rb[2]==0x5Au&&rb[3]==0x5Au)?0:1;
        g_sd_st_err[4]=(int32_t)fl; }
      (void)zero4;
    }
    ++g_sd_self_done;
  }
  if(do_inject) {
    uint8_t  dns[320];
    size_t   dl, fl;
    uint32_t dst;
    uint16_t qid;
    unsigned k;
    char     hn[MT_NAME_MAX];
    /* (1) 保证有在飞查询：没有就自己起一个 probe.test（顺带验发送路径）*/
    if (g_dns.phase != STANDALONE_DNS_WAIT_REPLY) {
      struct tun_dns_query tq;
      tq.id = (uint16_t)(0xd400u + (g_dns_seq & 0x0fffu));
      mt_copy(tq.hostname, "probe.test", 11u);
      (void)tun_dns_start_locked(&tq);
    }
    qid = g_dns.query_id;
    mt_copy(hn, g_dns.hostname, MT_NAME_MAX - 1u); hn[MT_NAME_MAX - 1u] = 0;
    /* (2) 造回包：114.114.114.114:53 → 本机:53167，答案 A=93.184.216.34 */
    dl = dr_build_reply(dns, sizeof(dns), hn, qid, 0x5DB8D822u, 60u);
    dst = ((uint32_t)g_netdev[0x20] << 24) | ((uint32_t)g_netdev[0x21] << 16) |
          ((uint32_t)g_netdev[0x22] << 8) | (uint32_t)g_netdev[0x23];
    fl = dr_build_frame(g_inj_frame, sizeof(g_inj_frame), dst,
                        (uint16_t)STANDALONE_DNS_SOURCE_PORT, dns, dl);
    g_inj_build_len = (int32_t)fl;
    g_inj_recv_rc = -9999; g_inj_sentinel_ok = 0u; g_inj_consume_rc = 0; g_inj_cache_ip = 0u;
    /* (3) 埋哨兵 → 从手机入口注入协议栈 */
    for (k = 0u; k < 4u; ++k) g_inj_rx[k] = 0x5Au;
    if (fl > 0u) NETDEV_INPUT(g_netdev, IPV4_CALLBACK, 1);
    /* (4) 用 socket 收（哨兵法验"真拷贝"） */
    if (fl > 0u) {
      int r = sd_recv(&g_sd, (void *)g_inj_rx, sizeof(g_inj_rx));
      g_inj_recv_rc = (int32_t)r;
      if (r > 0) g_inj_sentinel_ok =
          (g_inj_rx[0] != 0x5Au || g_inj_rx[1] != 0x5Au ||
           g_inj_rx[2] != 0x5Au || g_inj_rx[3] != 0x5Au) ? 1u : 0u;
    }
    /* (5) 消费 + 查缓存（要过 ID 环与问题名两道闸） */
    if (g_inj_recv_rc > 0 && g_inj_sentinel_ok) {
      const uint8_t *ip;
      NET_LOCK();
      g_inj_consume_rc = tun_dns_consume_payload_locked((const uint8_t *)g_inj_rx, (size_t)g_inj_recv_rc);
      ip = mt_cache_lookup(hn);
      g_inj_cache_ip = (ip != (const uint8_t *)0)
          ? (((uint32_t)ip[0] << 24) | ((uint32_t)ip[1] << 16) | ((uint32_t)ip[2] << 8) | (uint32_t)ip[3])
          : 0u;
      NET_UNLOCK();
    }
    ++g_inj_n;
  }
  if(do_td) {
    /* (1) 造查询（用模块真代码，拿到 payload 与 ID） */
    struct tun_dns_query tq;
    uint8_t wire[400], reply[512], rsa[16];
    size_t dl, wl;
    int fd, rc2;
    if (g_dns.phase != STANDALONE_DNS_WAIT_REPLY) {
      tq.id = (uint16_t)(0xe000u + (g_dns_seq & 0x0fffu));
      mt_copy(tq.hostname, "tcp.test", 9u);
      (void)tun_dns_start_locked(&tq);          /* 顺带验证 raw 发送路径 */
    }
    dl = (size_t)g_dns.tx_length > (size_t)RAW_UDP_HEADER_SIZE
         ? (size_t)g_dns.tx_length - (size_t)RAW_UDP_HEADER_SIZE : 0u;
    wl = dl ? td_frame(wire, sizeof(wire), g_dns.tx_frame + RAW_UDP_HEADER_SIZE, dl) : 0u;
    g_td_len = (int32_t)wl;
    standalone_dns_diag_push(&g_dns_diag, g_dns.query_id);
    /* (2) TCP socket + connect:53 */
    fd = ((sd_fn_socket)(uintptr_t)SD_FN_SOCKET)(TD_AF_INET, TD_SOCK_STREAM, 0);
    g_td_socket_rc = fd;
    g_td_conn_rc = g_td_send_rc = g_td_hdr_rc = g_td_recv_rc = -1;
    g_td_consume_rc = 0; g_td_cache_ip = 0u;
    if (fd >= 0) {
      (void)td_build_sa(rsa, g_dns.dns_ip, (uint16_t)TD_PORT);
      g_td_conn_rc = ((sd_fn_connect)(uintptr_t)SD_FN_CONNECT)(fd, (const void *)rsa, 16);
      if (g_td_conn_rc == 0 && wl > 0u) {
        /* (3) 发 [len][DNS] */
        g_td_send_rc = ((sd_fn_send)(uintptr_t)SD_FN_SEND)(fd, (const void *)wire, (int)wl, 0);
        if (g_td_send_rc > 0) {
          /* (4) 收 2 字节长度 + 报文 */
          uint8_t hdr[2];
          g_td_hdr_rc = ((sd_fn_recv)(uintptr_t)SD_FN_RECV)(fd, (void *)hdr, 2, 0);
          if (g_td_hdr_rc == 2) {
            size_t rl = ((size_t)hdr[0] << 8) | hdr[1];
            if (rl > 0u && rl <= sizeof(reply)) {
              rc2 = ((sd_fn_recv)(uintptr_t)SD_FN_RECV)(fd, (void *)reply, (int)rl, 0);
              g_td_recv_rc = rc2;
              if (rc2 > 0) {
                NET_LOCK();
                g_td_consume_rc = tun_dns_consume_payload_locked((const uint8_t *)reply, (size_t)rc2);
                { const uint8_t *ip = mt_cache_lookup(g_dns.hostname);
                  g_td_cache_ip = (ip != (const uint8_t *)0)
                      ? (((uint32_t)ip[0]<<24)|((uint32_t)ip[1]<<16)|((uint32_t)ip[2]<<8)|(uint32_t)ip[3])
                      : 0u; }
                NET_UNLOCK();
              }
            } else g_td_recv_rc = -2;
          }
        }
      }
    }
    ++g_td_n;
  }
  if(do_reset) {
    ++g_sd_reset_seen;
    if(g_sd.fd>=0) (void)sd_drain(&g_sd,(void *)rx,sizeof(rx),8u);
    sd_reset_stats(&g_sd);
    g_sd_send_fail=0u; g_sd_consume_drop=0u;
    g_sd_polls=g_sd_recv_ok=g_sd_recv_empty=g_sd_recv_err=0u;
  }
  /* ★v59：不只是响应 want_open —— 只要还没 bind 且有在飞查询，任何一次运行都补建。
   * （v58 真机：want_open 只置一次，若那一次 lpwork 被固件抢走没跑到，socket 就永远建不起来）*/
  if((do_open || g_dns.phase==STANDALONE_DNS_WAIT_REPLY) && g_sd.enabled && !g_sd.bound) {
    /* ★9PRO-ROUND: auto_port=1 ⇒ 不 bind，源端口交给内核分配。
     * 9Pro 用 sendto（每帧自带目的地址），不需要固定源端口；
     * 而我们 v61 之前 bind(0.0.0.0:53167) 疑似被手机侧按四元组丢帧。 */
    g_sd.auto_port = 1u;
    rc=sd_open(&g_sd,(uint16_t)STANDALONE_DNS_SOURCE_PORT,(const uint8_t *)0);  /* v61：不 connect */
    NET_LOCK();
    if(rc==0) ++g_sd_open_ok;
    else { ++g_sd_open_fail; g_sd.enabled=0u; ++g_sd_wedge; }
    NET_UNLOCK();
  }
  /* ★9PRO-ROUND: 完整一轮 = sendto + recvfrom(512) + 重试 3 次。
   * 只有在"确有在飞查询"时才走这一轮；空闲时只排空残留。
   * ⚠ 本固件 recvfrom 不吃 SO_RCVTIMEO（v65 真机 sd_step=3 实证），
   *   所以每次只做**一次** recv（SD_RECV_ROUNDS=1），把总阻塞时间压到最小，
   *   剩下的重试由 lpwork 的 selfarm 分拍推进，不在一次调用里死等。 */
  if(do_poll && g_sd.bound && g_dns.phase==STANDALONE_DNS_WAIT_REPLY) {
    size_t dl;
    rc=sd_recv(&g_sd,(void *)rx,sizeof(rx));      /* 先试收（可能已经回来了） */
    ++g_sd_polls;
    if(rc<=0) {
      /* 没收到 ⇒ 发一次查询（sendto），把"发出"计数放在这里 */
      dl=(size_t)g_dns.tx_length>RAW_UDP_HEADER_SIZE
        ? (size_t)g_dns.tx_length-(size_t)RAW_UDP_HEADER_SIZE : 0u;
      if(dl>0u && g_sd.fd>=0) {
        int sr=sd_sendto(&g_sd,(const void *)(g_dns.tx_frame+RAW_UDP_HEADER_SIZE),dl,
                         (const uint8_t *)g_dns.dns_ip,(uint16_t)SD_SERVER_PORT);
        if(sr>0) NET_LOCK(), ++g_dns_sent, NET_UNLOCK();
        ++g_sd_rounds;
      }
      if(g_sd.last_errno==SD_EAGAIN) ++g_sd_recv_empty;
      else ++g_sd_recv_err;
    }
    if(rc>0) {
      ++g_sd_recv_ok;
      NET_LOCK();
      (void)tun_dns_consume_payload_locked((const uint8_t *)rx,(size_t)rc);
      NET_UNLOCK();
    }
  } else if(!do_open && !do_reset && g_sd.bound &&
            g_dns.phase!=STANDALONE_DNS_WAIT_REPLY) {
    (void)sd_drain(&g_sd,(void *)rx,sizeof(rx),2u);   /* 空闲：丢陈旧数据报 */
  }
  if(do_open || do_poll || do_reset) tun_sd_selfarm();  /* 有活：半拍后再来 */
}

/* ★ v52/v53：socket 传输的运行时控制（由 mpu_trap.h 的命令阶梯 14/15 调，任务上下文）。
 *   14 = 切换 socket / raw-TUN 传输（默认 socket）
 *   15 = 复位 socket 收发状态 —— ★v53：只置标志 + 踢 lpwork，**绝不在调用者线程里碰
 *        socket**（fd 属于 lpwork 的 fd 表；在别的线程里用会落到错的表上）。*/
static void tun_sd_toggle(void) {
  g_sd.enabled = g_sd.enabled ? 0u : 1u;
  /* v65：打开 socket 收包要提醒——本固件 recvfrom 不吃 SO_RCVTIMEOUT（真机 sd_step=3 实证），
   * 一旦开就会卡死 lpwork。仅作诊断用，别长时间开着。 */
  if (g_sd.enabled) g_sd_open_warn = 1u;
  if(!g_sd.enabled) { standalone_dns_fail(&g_dns,-125); g_dns.phase=STANDALONE_DNS_IDLE; }
  else tun_sd_kick();
}
/* ★ v57：mt_cmd 16 —— 一键 socket 自检（在 lpwork 里跑，结果由 tunsvc 上报）*/
static void tun_sd_selftest(void) { g_sd_selftest=1; tun_sd_kick(); }
/* ★ v62：mt_cmd 17 —— 注入式收包自检（造回包 → 注入 → 收 → 消费 → 验缓存）*/
/* ★ v73：这个自检**必须**先过 netdev 门。
 *
 * 实测（v72 在模拟器上归因）：cmd 17 是唯一会打挂 control 通道的命令 ——
 * 写完 ~2s 后 GET /api/files 直接 503，连 `cat /dev/tunsvc` 都取不回来，只能重启实例。
 * 逐条隔离（bisect_write.py，每个命令独立重启一次实例）结论：
 *   cmd 16 OK（st_self_n 0→1）    —— 含 setsockopt/bind/recvfrom/recv，不阻塞
 *   cmd 18 OK（td_n 0→1）         —— 含 socket/connect/send/recvfrom
 *   cmd 17 通道死亡                —— 唯一独有的是 NETDEV_INPUT(g_netdev,...)
 * ⇒ 阻塞 recv 不是元凶（16/18 都调了且没事），**netdev 未注册时喂帧**才是。
 * g_netdev 在 registrar 没接受 bt-net 时只是静态结构（netdev_ready=0），
 * 把构造好的 IP 帧塞进去 = 往一个没被协议栈接管的 netdev 里灌数据。
 *
 * 所以门控放在这里：netdev 没起来就拒绝 + 记账，绝不碰 NETDEV_INPUT。
 * 真机上 netdev_ready=1 时该自检照常可用（这正是它设计的用途）。 */
static void tun_sd_inject_selftest(void) {
  if(!g_netdev_up) { g_inj_denied=1u; g_inj_deny_n++; return; }
  g_inj_req=1; tun_sd_kick();
}
/* ★ v68：mt_cmd 18 —— TCP-DNS 自检（connect:53 → 发 [len][DNS] → 收 [len][应答] → 解析写缓存）
 * 实测 v72 隔离：这个命令含 connect/send/recvfrom 但**不**打挂通道，保留原样。*/
static void tun_td_selftest(void) { g_td_req=1; tun_sd_kick(); }
/* ★★ v77：mt_cmd 19 —— 手动叫一次 netdev 注册，并把返回值摊开。
 *
 * 为什么需要它：`netdev_ready=0` 是主线上唯一的卡口，但在此之前**无法判定成因** ——
 *   ioctl_n=0        ⇒ 从来没有任何进程 ioctl 过 /dev/tun（连 DNS 查询都没发过）
 *   ioctl_728=0 而 ioctl_n>0 ⇒ 有进程用它，但从没叫过注册（main.lua 就是这种）
 *   reg_tries=0      ⇒ 同上，NETREG 一次都没试过
 *   reg_last_rc<0    ⇒ 试了，被 registrar 拒（固件这个入口只收 loopback）
 *   reg_last_rc=0    ⇒ 注册成功但 g_netdev_up 仍为 0 —— 不可能，出现即记账错
 * 这条命令在**任务上下文**（mt_cmd_poll 的调用者：lpwork / 读诊断页）里执行，
 * 不走中断，和 cmd 16/17/18 同一档安全性。
 *
 * ★ 风险提示：这是唯一一条会真正去碰固件 registrar 的命令。若 registrar 在
 * "只收 loopback" 的实现里对未知 netdev 有副作用，这条命令可能像 v72 的 cmd 17
 * 一样打挂 control 通道 —— 真机上表现为模块节点读不动，重启即可恢复（模块不持久）。*/
static void tun_netreg_selftest(void) {
  int rc;
  rc = tun_netdev_register_once();     /* 内部已记账 reg_tries / reg_last_rc / reg_short */
  if (!rc) {
    NET_LOCK();
    if (g_dhcp_state == DHCP_IDLE) { g_head = g_tail = 0; tun_dhcp_start_locked(); }
    NET_UNLOCK();
  }
}
/* ★★★ v79：mt_cmd 20 —— 宿主侧桥的回包入口（让模拟器能跑**真实公网 DNS**）。
 *
 * 为什么要它：模拟器 QEMU 命令行里**没有任何网络设备**（无 -netdev/-nic/-device），
 * `-M bes1503,help` 的 60+ 个属性里也零网络相关；而模块的 bt-net 是纯软件 netdev
 * （TX 只是把包放进 g_ring 等"桥"来读）。⇒ 客体自己出不了公网。
 *
 * 于是把桥搬到宿主（本命令是最后一块拼图）：
 *   ① 客体发 DNS 查询 → 包进 g_ring（已验证 dns_sent 0→1）
 *   ② 宿主 QMP `xp` 把 g_ring 里的包 dump 出来
 *   ③ 宿主用 Windows socket 真发到 114.114.114.114:53，收回包
 *   ④ 宿主用 nsh 的 `mw` 把回包写进 g_rxbuf，再 mw 写 g_bridge_len
 *   ⑤ 宿主 `echo 20 > /dev/tunentry` 触发本条
 *   ⑥ 本条对 g_rxbuf 调 tun_dns_receive_locked ⇒ 走完整收包链路 ⇒ cache 有 IP
 *
 * ★★ 上下文约束（关键，别改错）：mt_cmd_poll() 的三个调用点**都已持有 net_lock**
 *    —— tun_service_mt_locked()（由 drain worker 在 NET_LOCK 后调）、svc_read、
 *    stat_read（两者都是先 NET_LOCK 再 mt_cmd_poll）。
 *    ⇒ 这里**绝对不能**调 tun_write()：它自己会 NET_LOCK ⇒ 重入死锁。
 *    只能用 tun_dns_receive_locked()（这个后缀就是"调用方已持锁"的意思）。
 *
 * ★ 复用 g_rxbuf（SCRATCH=2048B）而不是新开缓冲：sock_dns.h 的既有约定就是
 *   "接收复用 g_rxbuf，两处都在 net_lock 下串行"，不额外吃 .bss（只剩 1.5KB）。
 *
 * bridge_result 取值（便于 nsh 一眼看懂）：
 *   0=还没跑  1=成功收下  2=长度/版本不对  3=netdev 没起来  4=不是 DNS 包 */
/* ★★ 必须是 volatile：这几个变量**只由外部（nsh 的 mw）写**，模块内只读。
 *    踩过：不加 volatile 时 clang 认定 g_bridge_len 恒为 0（模块内从不写它）
 *    ⇒ 常量折叠 ⇒ 符号直接从 .bss 里消失（nm 查不到），mw 也就没地方可写。 */
static volatile unsigned g_bridge_len;
static volatile unsigned g_bridge_calls,g_bridge_result,g_bridge_ok;
/* ★★ v90：cmd 24 —— 把包经**真实协议栈**送进去（NETDEV_INPUT），不是模块自己的 DNS 解析。
 * 目的：让固件真分配一个 IOB，从而可以
 *   (a) 挂 gdb 写观察点抓 iob_alloc/iob_free，顺藤摸到 g_iob_count；
 *   (b) 或做 RAM 差分，找 24→23 的 int16。
 * 调用方（svc_read → mt_cmd_poll）已持 net_lock，所以这里只能自己做、不能再加锁。
 * 包体自造：IPv4/UDP，10.1.10.1:1234 → 10.1.10.2:53167，43 字节。*/
static int tun_txdone(void *dev);   /* 定义在后面 */
static volatile unsigned g_win_rewrites;   /* ★v97 被改写窗口的包数 */
/* ★★ v90：cmd 24 —— 把包经**真实协议栈**送进去（NETDEV_INPUT），触发一次 IOB 分配。
 * 目的：给 gdb 观察点/ RAM 差分提供一次真实的 iob_alloc/free。
 * 调用方已持 net_lock，这里不能再加锁。包体用常量模板，省体积。*/
static const uint8_t g_netin_pkt[28] = {
  0x45,0x00,0x00,0x1C, 0x00,0x00,0x00,0x00, 0x40,0x11,0x52,0xCD,
  10,1,10,1, 10,1,10,2, 0x04,0xD2,0xCF,0xAF, 0x00,0x08,0x00,0x00
};
static volatile unsigned g_netin_calls, g_netin_rc;
static void tun_netin_inject_locked(void) {
  int rc;
  ++g_netin_calls; g_netin_rc = 0xFFFFFFFFu;
  if (!g_netdev_up) { g_netin_rc = 0xFFFFFFFEu; return; }
  copy(g_rxbuf,g_netin_pkt,sizeof(g_netin_pkt));
  U32(g_netdev,ND_IOB)=0;
  U32(g_netdev,ND_BUF)=(uint32_t)(uintptr_t)g_rxbuf;
  U16(g_netdev,ND_LEN)=(uint16_t)sizeof(g_netin_pkt); U16(g_netdev,ND_SNDLEN)=0;
  rc=NETDEV_INPUT(g_netdev,IPV4_CALLBACK,1);
  g_netin_rc=(unsigned)rc;
  if(rc>=0 && U16(g_netdev,ND_LEN)) tun_txdone(g_netdev);
  U32(g_netdev,ND_BUF)=0; U16(g_netdev,ND_LEN)=0; U16(g_netdev,ND_SNDLEN)=0;
}
static void tun_bridge_inject_locked(void) {
  const uint8_t *p = g_rxbuf;
  unsigned len = g_bridge_len;
  ++g_bridge_calls;
  if (!len || len < 28u || len > MAXPKT || (p[0] >> 4) != 4) { g_bridge_result = 2u; return; }
  if (!g_netdev_up) { g_bridge_result = 3u; return; }
  ++g_stats.rx_packets; g_stats.rx_bytes += len;
  if (tun_dns_receive_locked(p, len)) { g_bridge_result = 1u; ++g_bridge_ok; }
  else g_bridge_result = 4u;   /* 不是 DNS / 不是给我们的（ID 或端口不符）*/
}
/* ★★★ v81：mt_cmd 21 —— 用**宿主指定**的域名发一次真实 DNS 查询。
 *
 * 为什么需要它：cmd 18 里的域名写死成 "tcp.test"，公网只能回 NXDOMAIN，
 * 于是只能验到"回包被认下"（bridge_result=1），验不到"解析出 IP 并写缓存"。
 *
 * 用法（宿主侧）：
 *   ① nsh `mw` 把域名（ASCII、NUL 结尾，如 "www.baidu.com"）写进 g_rxbuf
 *   ② `echo 21 > /dev/tunentry` → 本条构造查询、走 raw 帧入环（sd_en=0）
 *   ③ 宿主从 g_ring 取包、真发公网、把回包 mw 写回 g_rxbuf（覆盖掉域名）
 *   ④ `echo 20` → cmd 20 消费 → cache 里就有 IP 了
 *   ★ 顺序不能颠倒：②要读 g_rxbuf 的域名，④要写 g_rxbuf 的回包。
 *
 * ★ 上下文：与 cmd 20 相同，调用方已持 net_lock，只能用 *_locked。
 * named_result：0=还没跑 1=查询已发出 2=域名空 3=发送被拒（如已有在飞查询）*/
static volatile unsigned g_named_calls,g_named_result;
static void tun_query_named_locked(void) {
  struct tun_dns_query tq;
  unsigned i, n = 0u;
  ++g_named_calls;
  if (!g_rxbuf[0]) { g_named_result = 2u; return; }
  for (i = 0u; i < sizeof(tq); ++i) ((uint8_t *)&tq)[i] = 0u;
  for (i = 0u; i + 1u < TUN_DNS_NAME_MAX && i < 128u; ++i) {
    char c = (char)g_rxbuf[i];
    if (!c) break;
    tq.hostname[i] = c;
    ++n;
  }
  tq.hostname[n] = 0;
  tq.id = 0u;
  if (tun_dns_start_locked(&tq) == 0) {
    /* ★★ v82：必须登记 inflight_name —— 否则看门狗超时后 `if(g_mt_inflight_name[0])`
     * 不成立 ⇒ 既不重发也不计 stuck_drops ⇒ 模拟器永远验不到重试路径。
     * （真机走队列时由 tun_service_mt_locked 在 mt_pend_peek 之后登记。）*/
    for (i = 0u; i + 1u < MT_NAME_MAX && tq.hostname[i]; ++i)
      g_mt_inflight_name[i] = tq.hostname[i];
    g_mt_inflight_name[i] = 0;
    g_mt_inflight_tries = 0u;
    g_named_result = 1u;
  } else g_named_result = 3u;
}
/* ★★★ v83：IOB 池扩容（cmd 22）★★★
 *
 * 实测基线（p67tc 3.101.043，模拟器 QMP dump 反推 + 真机 SYN 对照）：
 *   struct iob_s   = 12B 头 + 196B 数据 = 208B（0xD0），池 24 个，freelist 单链表；
 *   freelist 头指针变量在 ap-ram（3.101.043 = 0x200F4B50，运行期自动定位）；
 *   SYN 通告窗口 = min(空闲 IOB 数 × 196, 16384)
 *       10Pro：24 个池 ⇒ 23×196 = 4508 ⇒ TLS 证书链塞不下 ⇒ b 站永远失败
 *       9Pro ：2048 个池 ⇒ 2047×128 = 262016 ⇒ 截到 16384 ⇒ 正常
 *   ⇒ 只要让空闲 IOB ≥ 84，10Pro 也能拿到 16384。
 *
 * 三条硬约束：
 *   1) 固件在 XIP ROM（0x0C000000，只读）⇒ 只动数据，不碰代码；
 *   2) 节点内存不占模块 .bss（只剩 808B）⇒ 用 ap-ram 里实测恒零的空闲区，
 *      挂载前逐字节验零，不干净就放弃（rc=3），绝不硬写；
 *   3) 只由 cmd 22 手动触发，绝不进 insmod ⇒ 重启后 iob_initialize 重建
 *      freelist 与池，节点自然失效，等于自动复原。 */
#define IOB_STEP_       208u        /* sizeof(struct iob_s) */
#define IOB_DATA_       196u        /* CONFIG_NET_IOB_BUFSIZE */
#define IOB_EXT_N_      64u         /* 24 + 64 = 88 ⇒ 88×196 = 17248 > 16384 */
#define IOB_EXT_BYTES_  (IOB_EXT_N_ * IOB_STEP_)
#define IOB_RAM_LO_     0x20000000u /* miband.ap-ram 起 */
#define IOB_RAM_HI_     0x20160000u /* miband.ap-ram 止（不含）*/
#define IOB_SAFE_HI_    0x20070000u /* .data/.bss 起于 0x20076000，留 24KB 余量 */
#define IOB_MARGIN_     0x1000u     /* 节点区后面再留 4KB 全零缓冲 */

struct tun_iob_node {           /* 必须与固件 struct iob_s 逐字节同构 */
  uint32_t link;                /* +0x00 io_link   */
  uint16_t len;                 /* +0x04 io_len    */
  uint16_t off;                 /* +0x06 io_offset */
  uint32_t pktlen;              /* +0x08 io_pktlen */
  uint8_t  data[IOB_DATA_];     /* +0x0c io_data   */
};

static volatile uint32_t g_iob_head_var;    /* &g_iob_freelist（运行期定位）*/
static volatile uint32_t g_iob_scratch;     /* 我们节点的起始地址 */
static volatile uint32_t g_iob_chain_n;     /* 定位时看到的链长 */
/* ★★★ v93：g_iob_count 的真实地址（p67tc 3.101.043 实测定位，不是猜的）
 *   定位方法：用 cmd 24 把 IOB 扣在 socket 队列里，让 /proc/iobinfo 的 nfree 24→23，
 *   同一次启动内抓两份 RAM 做差 ⇒ 全 RAM 只有 0x200C89C2 这一个 16 位量 24→23；
 *   再把它写成 88 ⇒ iobinfo 立刻报 nfree=88 ⇒ 确认。
 *   它是 int16，落在 0x200C89C0 这个字的**高半字**上。*/
#define IOB_CNT_ADDR_ 0x200C89C2u
static volatile uint32_t g_iob_head_before, g_iob_head_after;
static volatile uint32_t g_iob_cnt_before, g_iob_cnt_after;
static volatile uint32_t g_iob_grow_calls, g_iob_grow_rc, g_iob_attached;
/* 注：/proc/iobinfo 的 nfree 不是遍历链表算出来的 —— 把 freelist 头往下挪 10 个节点，
 * nfree 仍然报 24（模拟器实测）。所以"窗口按空闲 IOB 数算"这条路径是否走 freelist，
 * 光靠 iobinfo 判不出来，必须看真机 SYN 里的窗口字段（见 evidence/README）。*/

/* 从 p 开始按 0xD0 步长走链；返回节点数（0 = 这条不是 freelist）。
 * lo/hi 回填链上最小/最大节点地址 —— 用来把「链里的节点」和「头指针变量」区分开：
 * 池里每个节点的 +0 都指向下一个节点，光看"能走出链"会把节点误认成头指针。*/
static uint32_t iob_chain_walk(uint32_t p, uint32_t *lo, uint32_t *hi) {
  uint32_t n = 0u, mn, mx;
  if (p < IOB_RAM_LO_ || p + IOB_STEP_ > IOB_RAM_HI_) return 0u;
  mn = mx = p;
  while (p && n < 512u) {
    uint32_t q;
    if (p < IOB_RAM_LO_ || p + IOB_STEP_ > IOB_RAM_HI_) return 0u;
    if (mt_rd(p + 4u) != 0u) return 0u;        /* len/offset 必须为 0 ⇒ 空闲节点 */
    if (p < mn) mn = p;
    if (p > mx) mx = p;
    q = mt_rd(p);
    if (!q) { if (lo) *lo = mn; if (hi) *hi = mx; return n + 1u; }   /* 链尾 */
    if (q < IOB_RAM_LO_ || q >= IOB_RAM_HI_) return 0u;
    if ((q > p) ? ((q - p) != IOB_STEP_) : ((p - q) != IOB_STEP_)) return 0u;
    p = q; ++n;
  }
  return 0u;
}

/* 在 ap-ram 里找 freelist 头指针变量：唯一一个「指向 ≥8 节点、步长 0xD0 的链，
 * 且自己不在链里」的字。池节点自身也会命中外层条件，靠 lo/hi 排除。*/
static uint32_t iob_locate(void) {
  uint32_t a, w, n, lo = 0u, hi = 0u, best = 0u, bestn = 0u;
  for (a = IOB_RAM_LO_; a + 4u <= IOB_RAM_HI_; a += 4u) {
    w = mt_rd(a);
    if (w < IOB_RAM_LO_ || w >= IOB_RAM_HI_) continue;
    n = iob_chain_walk(w, &lo, &hi);
    if (n < 8u) continue;
    if (a >= lo && a <= hi) continue;          /* 这是链里的节点，不是头指针 */
    if (n > bestn) { bestn = n; best = a; }
  }
  g_iob_chain_n = bestn;
  return best;
}

/* 在 ap-ram 里找**最大的一段全零页**，把节点放在这段的起始处：
 * 上行还留着整段余量（栈往下长也够不着），且挂载前逐字节验零 —— 不干净就用 rc=3 放弃。*/
static uint32_t iob_scratch_find(void) {
  uint32_t a, i, run = 0u, best = 0u, bestlen = 0u;
  for (a = IOB_RAM_LO_; a + 0x1000u <= IOB_RAM_HI_; a += 0x1000u) {
    uint32_t z = 1u;
    for (i = 0u; i < 0x1000u; ++i)
      if (*(volatile uint8_t *)(uintptr_t)(a + i)) { z = 0u; break; }
    if (!z) { run = 0u; continue; }
    if (!run) run = a;
    if ((a + 0x1000u - run) > bestlen) { bestlen = a + 0x1000u - run; best = run; }
  }
  if (!best || bestlen < IOB_EXT_BYTES_ + IOB_MARGIN_) return 0u;
  /* 再验一遍真要用到的那一段（页粒度检查已经覆盖，这里防"恰好边界有脏字节"）*/
  for (i = 0u; i < IOB_EXT_BYTES_ + IOB_MARGIN_; ++i)
    if (*(volatile uint8_t *)(uintptr_t)(best + i)) return 0u;
  return best;
}

static void tun_iob_grow(void) {
  uint32_t i, head, tail, s, n, irq;
  ++g_iob_grow_calls;
  if (g_iob_attached) { g_iob_grow_rc = 1u; return; }        /* 已挂过 */
  if (!g_iob_head_var) g_iob_head_var = iob_locate();
  if (!g_iob_head_var) { g_iob_grow_rc = 2u; return; }       /* 找不到池 */
  s = iob_scratch_find();
  if (!s) { g_iob_grow_rc = 3u; return; }                    /* 没有干净区 */
  irq = mt_irq_save();
  head = mt_rd(g_iob_head_var);
  n = iob_chain_walk(head, 0, 0);
  if (!n) { mt_irq_restore(irq); g_iob_grow_rc = 4u; return; }
  /* 先把节点全写好，最后一步才接链 —— 半成品绝不进链 */
  for (i = 0u; i < IOB_EXT_N_; ++i) {
    struct tun_iob_node *p = (struct tun_iob_node *)(uintptr_t)(s + i * IOB_STEP_);
    p->link   = (i + 1u < IOB_EXT_N_) ? (s + (i + 1u) * IOB_STEP_) : 0u;
    p->len    = 0u;
    p->off    = 0u;
    p->pktlen = 0u;
  }
  if (!head) mt_wr(g_iob_head_var, s);
  else {
    tail = head;
    for (i = 0u; i < 512u; ++i) {
      uint32_t q = mt_rd(tail);
      if (!q) break;
      tail = q;
    }
    mt_wr(tail, s);
  }
  /* ★ v93：接链之后，把计数器一起抬高 —— 窗口算的是它（iob_navail→g_iob_count），
   * 只接链不抬计数，窗口仍然是 (24-1)*196=4508。*/
  {
    uint32_t w = mt_rd(IOB_CNT_ADDR_ & ~3u);
    uint32_t cv = (w >> 16) & 0xFFFFu;          /* 高半字 */
    g_iob_cnt_before = cv;
    w = (w & 0x0000FFFFu) | ((cv + IOB_EXT_N_) << 16);
    mt_wr(IOB_CNT_ADDR_ & ~3u, w);
    g_iob_cnt_after = (mt_rd(IOB_CNT_ADDR_ & ~3u) >> 16) & 0xFFFFu;
  }
  g_iob_head_before = head;
  g_iob_head_after  = mt_rd(g_iob_head_var);
  g_iob_scratch     = s;
  g_iob_attached    = 1u;
  g_iob_grow_rc     = 0u;
  mt_irq_restore(irq);
}

/* ★★ v94：自动扩池 —— 真机上 /dev/tunentry 的写从来没成功过（entry_writes 恒 0），
 * 命令触发这条路不可靠。改成模块自己在一个安全时刻跑一次：
 * netdev 一注册、再等 5 拍（≈5s）就动手：接 64 节点并把 g_iob_count +64。*/
static unsigned g_iob_auto_beats;
static void __attribute__((unused)) tun_iob_auto_maybe(void) {
  if (g_iob_attached) return;
  if (!g_netdev_up) return;   /* ★v95：不再等 IP —— 池和 IP 无关，早扩早好 */
  if (g_iob_auto_beats < 5u) { ++g_iob_auto_beats; return; }
  g_iob_auto_beats = 0xFFFFFFF0u;      /* 不再进入 */
  tun_iob_grow();
}
/* ★ v83：cmd 23 —— 把固定测试租约装进协议栈（10.1.10.2/24，网关 10.1.10.1）。
 *
 * 为什么需要它：模拟器里 g_dhcp_state/g_ip_configured 只是模块自己的标志，
 * 协议栈 netdev 的 d_ipaddr 仍是 0 ⇒ `curl` 的 connect() 立刻 ENETUNREACH，
 * 一个包都发不出去（实测 "Failed to connect ... after 0 ms"）。
 * 真机上这条路由由 DHCP ACK 走 tun_dhcp_install() 完成；模拟器没有真 DHCP，
 * 所以用本命令直接调同一个 IFADDR_SET/IFMASK_SET/IFDST_SET 三元组。
 * 装好后 `curl http://10.1.10.1/` 就会真的发 SYN，窗口值可从 g_ring 里读。*/
static volatile uint32_t g_fake_ip_rc, g_fake_mask_rc, g_fake_gw_rc, g_fake_calls;
static void tun_ip_fake_lease(void) {
  uint32_t ip = 0x020A010Au;      /* 10.1.10.2  */
  uint32_t mask = 0x00FFFFFFu;    /* 255.255.255.0 */
  uint32_t gw = 0x010A010Au;      /* 10.1.10.1  */
  ++g_fake_calls;
  g_fake_ip_rc   = (uint32_t)IFADDR_SET("bt-net", &ip);
  g_fake_mask_rc = (uint32_t)IFMASK_SET("bt-net", &mask);
  g_fake_gw_rc   = (uint32_t)IFDST_SET("bt-net", &gw);
}
static void tun_sd_reset(void) {
  ++g_sd_resets;
  g_sdq.reset = 1u;
  tun_sd_kick();
}
static int tun_dhcp_receive_locked(const uint8_t *p,unsigned n) {
  struct raw_udp_view v; struct dhcp_codec_lease lease; unsigned ihl;
  static const uint8_t broadcast[4]={255,255,255,255};
  if(n<28u || p[9]!=17u) return 0;
  ihl=(p[0]&15u)*4u;
  if(ihl<20u || ihl+8u>n || raw_udp_read16(p+ihl)!=67u || raw_udp_read16(p+ihl+2)!=68u) return 0;
  if(raw_udp_parse(p,n,&v) || dhcp_codec_parse(v.payload,v.payload_length,g_dhcp_xid,g_dhcp_mac,&lease)) {
    ++g_dhcp_bad; return 1;
  }
  /* The DHCP payload is carried over the phone bridge as server->broadcast.
   * Bind the outer IPv4 addresses to the parsed server-id before accepting
   * an OFFER/ACK.  Port/xid/chaddr alone would let an unrelated UDP frame
   * drive the address setters.  The phone implementation used by p67tc
   * 3.101.043 emits 10.1.10.1 -> 255.255.255.255 for both replies. */
  if(!raw_udp_same_ip(v.source_ip,lease.server) ||
     !raw_udp_same_ip(v.destination_ip,broadcast)) {
    ++g_dhcp_bad; return 1;
  }
  if(lease.message_type==DHCP_CODEC_NAK) {
    ++g_dhcp_naks; g_dhcp_state=DHCP_REJECTED; return 1;
  }
  if(lease.message_type==DHCP_CODEC_OFFER && g_dhcp_state==DHCP_SELECTING) {
    copy((uint8_t *)&g_dhcp_offer,(const uint8_t *)&lease,sizeof(lease));
    ++g_dhcp_offers; g_dhcp_state=DHCP_REQUEST_PENDING; tun_dhcp_emit_locked(); return 1;
  }
  if(lease.message_type!=DHCP_CODEC_ACK || g_dhcp_state!=DHCP_REQUESTING ||
     !raw_udp_same_ip(lease.server,g_dhcp_offer.server) ||
     !raw_udp_same_ip(lease.address,g_dhcp_offer.address)) { ++g_dhcp_bad; return 1; }
  if((lease.present&(DHCP_CODEC_HAVE_MASK|DHCP_CODEC_HAVE_ROUTER|DHCP_CODEC_HAVE_DNS))!=
      (DHCP_CODEC_HAVE_MASK|DHCP_CODEC_HAVE_ROUTER|DHCP_CODEC_HAVE_DNS)) { ++g_dhcp_bad; return 1; }
  copy((uint8_t *)&g_dhcp_lease,(const uint8_t *)&lease,sizeof(lease));
  ++g_dhcp_acks; g_dhcp_state=DHCP_INSTALLING; return 2;
}
static void tun_dhcp_install(unsigned epoch) {
  uint32_t ip,mask,gw,old_ip,old_mask,old_gw; int a,b,c,rollback=0;
  NET_LOCK();
  if(epoch!=g_dhcp_epoch || g_dhcp_state!=DHCP_INSTALLING) { NET_UNLOCK(); return; }
  ip=ip4_le(g_dhcp_lease.address); mask=ip4_le(g_dhcp_lease.mask); gw=ip4_le(g_dhcp_lease.router);
  old_ip=U32(g_netdev,0x20); old_mask=U32(g_netdev,0x28); old_gw=U32(g_netdev,0x24);
  NET_UNLOCK();
  a=IFADDR_SET("bt-net",&ip); b=IFMASK_SET("bt-net",&mask); c=IFDST_SET("bt-net",&gw);
  if(a || b || c) {
    rollback=IFADDR_SET("bt-net",&old_ip); rollback|=IFMASK_SET("bt-net",&old_mask); rollback|=IFDST_SET("bt-net",&old_gw);
  }
  NET_LOCK();
  if(epoch==g_dhcp_epoch && g_dhcp_state==DHCP_INSTALLING) {
    g_ip_rc=a; g_mask_rc=b; g_gw_rc=c; g_rollback_rc=rollback;
    g_ip_configured=(!a && !b && !c); g_dhcp_state=g_ip_configured?DHCP_BOUND:DHCP_FAILED;
  }
  NET_UNLOCK();
}
/* Called with net_lock held; bytes already copied out of kernel IOB by firmware. */
static int tun_txdone(void *dev) {
  const unsigned char *p=(const unsigned char *)(uintptr_t)U32(dev,ND_BUF);
  unsigned n=U16(dev,ND_LEN);
  if(n) {
    if(!p || n>MAXPKT || full()) { ++g_stats.tx_drop; return 1; }
    tcp_probe_note(p,n,0u);          /* ★v85：协议栈自发上行包也要进探针环
                                      * （v84 只挂 tun_enqueue_locked，漏了这条路径，
                                      *  结果环里 4 条全是 DOWN，看不到手表的 SYN/请求）*/
    copy(g_ring[g_head],p,n); g_ringlen[g_head]=(uint16_t)n;
    /* ★ v99：v97 的"改包窗口"逻辑已**移除** —— 用户否定该方案（超卖窗口、
     * 且会干扰 navail 钩子实验）。现在窗口只由固件自己的 tcp_get_recvwindow 决定，
     * 我们只在 iob_navail 上做钩子（cmd 26）。win_rewrites 恒 0。*/
    g_head=(g_head+1u)%SLOTS; ++g_stats.tx_packets; g_stats.tx_bytes+=n;
    tun_notify_readable();
  }
  return full();
}
static void tun_txworker(void *arg) {
  void *dev=arg;
  NET_LOCK(); ++g_stats.work_runs;
  if(g_netdev_up && (U32(dev,ND_FLAGS)&6u)==6u && !full()) {
    /* Non-NULL d_buf selects firmware's IOB-to-flat compatibility path.
     * devif_poll allocates/prepares IOBs and releases them on exit. */
    U32(dev,ND_BUF)=(uint32_t)(uintptr_t)g_txbuf;
    U16(dev,ND_LEN)=0; U16(dev,ND_SNDLEN)=0;
    DEVIF_POLL(dev,tun_txdone);
    U32(dev,ND_BUF)=0; U16(dev,ND_LEN)=0; U16(dev,ND_SNDLEN)=0;
  }
  NET_UNLOCK();
}
static int tun_dev_txavail(void *dev) {
  int rc;
  if(!dev) return -22;
  ++g_stats.tx_notify;
  /* work_s callback at +0x18 is cleared before dispatch. A running worker
   * can be queued again; queue 0 serializes invocations like native loopback. */
  if(U32(g_work,0x18)) return 0;
  rc=WORK_QUEUE(0,g_work,tun_txworker,dev,0);
  g_stats.last_work_rc=rc;
  if(rc<0) ++g_stats.work_error; else ++g_stats.work_queued;
  return rc;
}
static int tun_dev_ifup(void *dev) { CARRIER_ON(dev); return 0; }
static int tun_dev_ifdown(void *dev) { CARRIER_OFF(dev); return 0; }
static int __attribute__((noinline,used)) tun_netdev_register_once(void) {
  unsigned i; int rc; const char *name="bt-net";
  NET_LOCK();
  if(g_netdev_up) { NET_UNLOCK(); ++g_reg_short; return 0; }
  for(i=0;i<sizeof(g_netdev);++i) g_netdev[i]=0;
  for(i=0;name[i];++i) g_netdev[4+i]=(unsigned char)name[i];
  U32(g_netdev,0x48)=(uint32_t)(uintptr_t)tun_dev_ifup;
  U32(g_netdev,0x4c)=(uint32_t)(uintptr_t)tun_dev_ifdown;
  U32(g_netdev,0x50)=(uint32_t)(uintptr_t)tun_dev_txavail;
  U32(g_netdev,0x54)=(uint32_t)(uintptr_t)g_netdev;
  ++g_reg_tries;
  rc=NETREG(g_netdev,1); /* this build's registrar only accepts loopback */
  g_reg_last_rc=(uint32_t)rc;   /* ★v77：负值=被 registrar 拒；0=注册成功 */
  if(!rc) {
    /* Publish TUN semantics while holding network lock. Native n67 TUN=3.
     * p67 default routing skips types 1 and 9; clearing flags alone is not enough.
     * p67 proc formatter may display UNSPEC because TUN's label was compiled out. */
    g_netdev[0x18]=3; g_netdev[0x19]=0;
    U32(g_netdev,ND_FLAGS)&=~0x20u;
    g_netdev_up=1;
  }
  NET_UNLOCK(); return rc;
}
static int tun_open(void *filep) { (void)filep; return 0; }
static int tun_close(void *filep) {
  NET_LOCK();
  if(g_tun_owner==filep) {
    g_tun_owner=0; ++g_dhcp_epoch; g_dhcp_state=DHCP_IDLE;
    standalone_dns_fail(&g_dns,-125); g_dns.phase=STANDALONE_DNS_IDLE;
    g_ip_configured=0; g_head=0; g_tail=0; g_pollfds=0;
  }
  NET_UNLOCK(); return 0;
}
static int stat_close(void *filep) { (void)filep; return 0; }
static int tun_read(void *filep,void *buf,unsigned len) {
  unsigned n; (void)filep;
  if(!buf || !len) return -22;
  NET_LOCK();
  tun_service_mt_locked();
  if(g_tail==g_head) { ++g_stats.read_empty; NET_UNLOCK(); return -11; }
  n=g_ringlen[g_tail];
  if(n>len) { NET_UNLOCK(); return -90; } /* preserve the packet for a full read */
  copy(buf,g_ring[g_tail],n); g_tail=(g_tail+1u)%SLOTS;
  ++g_stats.read_packets; g_stats.read_bytes+=n;
  tun_dev_txavail(g_netdev);
  NET_UNLOCK(); return (int)n;
}
static int tun_write(void *filep,const void *buf,unsigned len) {
  int rc,dhcp; unsigned epoch; const unsigned char *p=buf; (void)filep;
  if(!p || len<20 || len>MAXPKT || (p[0]>>4)!=4) return -22;
  tcp_probe_note(p,len,1u);          /* ★v84：下行 TCP 头进探针环 */
  NET_LOCK();
  if(!g_netdev_up) { NET_UNLOCK(); return -19; }
  ++g_stats.rx_packets; g_stats.rx_bytes+=len;
  dhcp=tun_dhcp_receive_locked(p,len); epoch=g_dhcp_epoch;
  if(dhcp) {
    NET_UNLOCK();
    if(dhcp==2) tun_dhcp_install(epoch);
    return (int)len;
  }
  /* ★v60：两道保险 —— raw 消费者只认"sport=53 + ID 在最近环 + 问题名逐字节相符"的帧
   * （= 确凿是我们的回包才会截走）；其余一律交给系统栈投递给 socket。
   * 这样即使手机侧改写了回包源地址（connected socket 会按四元组过滤掉），raw 这条路仍能接住。*/
  if(tun_dns_receive_locked(p,len)) { NET_UNLOCK(); return (int)len; }
  copy(g_rxbuf,p,len);
  /* The NULL-IOB entry owns allocation, copyin, protocol dispatch,
   * response copyout, and release. */
  U32(g_netdev,ND_IOB)=0;
  U32(g_netdev,ND_BUF)=(uint32_t)(uintptr_t)g_rxbuf;
  U16(g_netdev,ND_LEN)=(uint16_t)len; U16(g_netdev,ND_SNDLEN)=0;
  rc=NETDEV_INPUT(g_netdev,IPV4_CALLBACK,1); g_stats.last_input_rc=rc;
  if(rc<0) ++g_stats.rx_error;
  else if(U16(g_netdev,ND_LEN)) { ++g_stats.rx_reply; tun_txdone(g_netdev); }
  U32(g_netdev,ND_BUF)=0; U16(g_netdev,ND_LEN)=0; U16(g_netdev,ND_SNDLEN)=0;
  NET_UNLOCK(); return rc<0?rc:(int)len;
}
static int tun_poll(void *filep,void *fds,int setup) {
  int rc=0; (void)filep;
  NET_LOCK();
  if(setup) {
    tun_service_mt_locked();
    if(g_pollfds && g_pollfds!=fds) rc=-16;
    else { g_pollfds=fds; ++g_stats.poll_setup; if(g_tail!=g_head) tun_notify_readable(); }
  } else if(g_pollfds==fds) { g_pollfds=0; ++g_stats.poll_teardown; }
  NET_UNLOCK(); return rc;
}
static int tun_ioctl(void *filep,int cmd,unsigned long arg) {
  int rc; (void)filep;
  ++g_ioctl_n;                                  /* ★v77：有没有人碰过 /dev/tun */
  if(cmd==0x728) ++g_ioctl_728;                 /* ★v77：其中几次是"叫注册" */
  if(cmd==TUN_DNS_QUERY) {
    if(!arg) return -22;
    /* ★ v52：socket 路径下**不能在这里直接发** —— 本 ioctl 由桥任务调用，socket 的 fd
     * 会落进那张 fd 表，而轮询在 drain worker（另一条线程）里 ⇒ 互相看不见。
     * 改成"名字入队 + 踢醒节拍"，由 worker 在下一拍完成（语义等价，晚 ≤1 拍）。*/
    if(g_sd.enabled) {
      const struct tun_dns_query *qq=(const struct tun_dns_query *)(uintptr_t)arg;
      if(qq->hostname[0]) mt_pend_note(qq->hostname);
      tun_drain_kick();
      return 0;
    }
    NET_LOCK(); rc=tun_dns_start_locked((const struct tun_dns_query *)(uintptr_t)arg); NET_UNLOCK(); return rc;
  }
  if(cmd==TUN_DNS_RESULT) {
    if(!arg) return -22;
    NET_LOCK(); tun_dns_result_copy((struct tun_dns_result *)(uintptr_t)arg); NET_UNLOCK(); return 0;
  }
  /* Resolver entry candidate controls.  These calls are safe while the
   * backend is absent: entry_patch_apply() returns -ENOTSUP and performs no
   * memory-mapped or firmware-text write. */
  if(cmd==TUN_ENTRY_APPLY) { NET_LOCK(); rc=entry_patch_apply(); NET_UNLOCK(); return rc; }
  if(cmd==TUN_ENTRY_DISABLE) { NET_LOCK(); rc=entry_patch_disable(); NET_UNLOCK(); return rc; }
  if(cmd==TUN_ENTRY_RESTORE) { NET_LOCK(); rc=entry_patch_restore(); NET_UNLOCK(); return rc; }
  if(cmd==TUN_ENTRY_SELFTEST) { NET_LOCK(); rc=fpb_probe_run(); NET_UNLOCK(); return rc; }
  if(cmd!=0x728) return -25;
  if(!arg) return -22;
  rc=tun_netdev_register_once();
  if(!rc) {
    NET_LOCK();
    if(g_tun_owner!=filep || g_dhcp_state==DHCP_IDLE) {
      g_tun_owner=filep; g_head=g_tail=0; tun_dhcp_start_locked();
    }
    U32((void *)(uintptr_t)arg,0x14)=U16(g_netdev,0x1c);
    NET_UNLOCK();
  }
  return rc;
}
/* Read-only diagnostic node; file_s f_pos is signed 64-bit at +8.
 * file_read does not advance it automatically. No printf/libc dependencies. */
struct text { char *p; unsigned n,cap; };
static void addstr(struct text *t,const char *s) { while(*s && t->n+1u<t->cap) t->p[t->n++]=*s++; }
static void addnum(struct text *t,uint32_t v,unsigned base) {
  char digits[32]; unsigned n=0; const char *hex="0123456789abcdef";
  do { digits[n++]=hex[v%base]; v/=base; } while(v);
  while(n && t->n+1u<t->cap) t->p[t->n++]=digits[--n];
}
static void field(struct text *t,const char *s,uint32_t v,unsigned base) {
  addstr(t,s); addstr(t,"="); if(base==16) addstr(t,"0x"); addnum(t,v,base); addstr(t,"\n");
}
/* 一行打 8 个 32 位字（帧/栈现场留档用，比 8 行省一半缓冲） */
static void __attribute__((unused)) field_words(struct text *t,const char *s,const uint32_t *w,unsigned n) {
  unsigned i;
  addstr(t,s); addstr(t,"=");
  for(i=0u;i<n;++i){ if(i) addstr(t,","); addnum(t,w[i],16); }
  addstr(t,"\n");
}

/* ★ /dev/tunresolve —— 把域名"提前"查进缓存，让应用**第一次**就能命中替换。
 * 用法（nsh）：echo www.baidu.com > /dev/tunresolve
 * 原理：只做入队（mt_pend_note），由桥读 /dev/tun 时排空并发起 DNS 查询、回包填缓存 ——
 * 与陷阱那条路共用同一套机制，所以它同时也是"DNS 客户端本身通不通"的直接探针。 */
static unsigned g_resolve_writes, g_resolve_last_len;
/* ★ v72：/dev/tunentry 写入口的账（判据：写不进 / 解析错 / 正常执行 各计数） */
static unsigned g_entry_writes, g_entry_bad, g_entry_cmds, g_entry_last_cmd;
/* ★ v73：收包现场（len + 前 8 字节）—— 实测「文件字节明明是 mt_cmd=16\n 却被拒」，
 * 光看结果无法区分「nsh 传的不是这些字节」和「解析器逻辑错」，必须把现场留下来。 */
static unsigned g_entry_last_len, g_entry_head[8];
/* ★★ v74：跨 write 累积缓冲。
 *
 * 实测（v74 现场字段定案）：nsh 的 `echo 16 > /dev/tunentry` **不是一次 write**，
 * 而是逐字节多次调用 write()：
 *   write("1") → write("6") → write("\n") → …（`echo 16` 一共让 entry_writes +4）
 * 而 `cat 文件 > 设备` 那条路只调 1 次、但**缓冲区是 4 字节步长散开的**
 * （entry_head = 6d 00 00 00 74 00 00 00 ⇒ 'm'@0、't'@4，不是连续的 "mt_cmd=18"）。
 *
 * ⇒ 原来「一次 write 就是一条完整命令」的假设是错的：
 *    纯数字 "16" 之所以偶尔能成，是因为 '1' 先到被判 too-short 拒收、'6' 到达时
 *    恰好单独成一条；而 "mt_cmd=18" 的 'm'… 每次只带一个有效字符 ⇒ 永远拼不齐 ⇒ 全拒。
 *
 * 修法：把 write 变成**流式解析** —— 字节进累积缓冲，遇 '\n'/'\r'/'\0' 或缓冲满即结算；
 * 解析器只吃累积缓冲，天然同时支持「逐字节」和「整块」两种下发方式。 */
static char     g_entry_buf[16];
static unsigned g_entry_buf_len;

/* ★ v50：十六进制转储（抓帧回放用：宿主机上原样喂回 dns_codec/raw_udp 看判因） */
static void dump_hex(struct text *t,const char *label,const uint8_t *b,unsigned n) {
  unsigned i;
  addstr(t,label);
  for(i=0u;i<n;++i) {
    char c3[3]; unsigned hi=(b[i]>>4)&15u, lo=b[i]&15u;
    c3[0]=(char)(hi<10u?('0'+hi):('a'+hi-10u));
    c3[1]=(char)(lo<10u?('0'+lo):('a'+lo-10u));
    c3[2]=0; addstr(t,c3);
  }
  addstr(t,"\n");
}
/* ★ v72/v74：/dev/tunentry 写入口 —— 把 mt_cmd 从「只能 gdb poke」变成「shell 可写」。
 *
 * 之前唯一的下发通道是 tunsvc 报出来的 mt_cmd_addr，再用 `mw <addr>=<n>` 往 .bss
 * 里打一个字节。模拟器上有 BES gdb 夹具（emu_udp_inject.py 的 poke）所以能测，
 * 真机上没有 gdb ⇒ mt_cmd 14/15/16/17/18 永远跑不了。现在改成写设备节点：
 *
 *   echo 16 > /dev/tunentry          # 一键 socket 自检
 *   echo 17 > /dev/tunentry          # 注入式收包自检（netdev 未起会被拒，见 v73）
 *   echo 18 > /dev/tunentry          # TCP-DNS 自检
 *   echo mt_cmd=14 > /dev/tunentry   # 等价写法（前缀可选）
 *
 * ★★ v74：write 是**流式**的，不是「一次调用一条命令」。见 g_entry_buf 处的实测记录。
 * 结算时机 = 遇到 '\n' / '\r' / '\0'，或缓冲满。只接受 1..99 的十进制数字。
 * 执行本身仍走 mt_cmd_poll —— 只在 lpwork / 读诊断页的任务上下文里评估，
 * 绝不从中断或本 write 的调用者线程直接做 stage/trap 那些硬件动作。
 */
static void entry_settle(void) {
  const char *p=g_entry_buf;
  unsigned i=0u,skip=0u,n=g_entry_buf_len; uint32_t v=0u; unsigned digits=0u;
  g_entry_buf_len=0u;
  if(!n) return;
  /* 允许 "mt_cmd=16" 这种带前缀的写法 */
  if(n>=6u && p[0]=='m' && p[1]=='t' && p[2]=='_' && p[3]=='c' && p[4]=='m' &&
     p[5]=='=') skip=6u;
  for(i=skip;i<n;++i) {
    char c=p[i];
    if(c==' '||c=='\t') { if(digits) break; continue; }  /* 数字前的空白吃掉 */
    if(c<'0'||c>'9') break;
    if(++digits>2u) { ++g_entry_bad; return; }           /* 最多两位，挡掉垃圾输入 */
    v=v*10u+(uint32_t)(c-'0');
  }
  if(!digits || v==0u || v>99u) { ++g_entry_bad; return; }
  g_entry_last_cmd=v;
  /* ★ v107：调试开关 = 纯内存标志，不碰 VFS。 */
  if(v==98u) g_dbg_on=1;
  else if(v==97u) g_dbg_on=0;
  /* 先清 seen 再置 cmd：同一个命令连写两次也能重复触发（mt_cmd_poll 是边沿触发）。 */
  g_mt_cmd_seen=0u;
  g_mt_cmd=v;
  ++g_entry_cmds;
}
/* ★ v74：每次 open 都清掉上一轮可能残留的半条命令（没等到终结符就 close 了）。*/
static int entry_open(void *fp) { g_entry_buf_len=0u; U32(fp,8)=0; U32(fp,12)=0; return 0; }
static int entry_write(void *filep,const void *buf,unsigned len) {
  const unsigned char *p=(const unsigned char *)buf;
  unsigned i;
  (void)filep;
  ++g_entry_writes;
  if(!buf||!len) return -22;
  g_entry_last_len=len;
  { unsigned k, m=(len<8u?len:8u);
    for(k=0u;k<8u;++k) g_entry_head[k]=0u;
    for(k=0u;k<m;++k) g_entry_head[k]=(unsigned)p[k]; }
  /* v74：逐字节进累积缓冲，遇终结符或缓冲满即结算。 */
  for(i=0u;i<len;++i) {
    unsigned char c=p[i];
    if(g_entry_buf_len<sizeof(g_entry_buf)) g_entry_buf[g_entry_buf_len++]=(char)c;
    if(c=='\n'||c=='\r'||c=='\0'||g_entry_buf_len>=sizeof(g_entry_buf)) entry_settle();
  }
  /* 没有终结符就等下一次 write —— 返回值仍然报本次全部字节数（吃掉，不报短写）。 */
  return (int)len;
}
/* ★ v36 /dev/tunsvc —— 专用诊断页：一次 cat 看清"桥→网络→查询→回包"哪一段断了。
 * 与 /dev/tunstat 分开的原因：tunstat 有 157 行、3072B 装不下（尾部字段事实上被截断），
 * 新字段再塞进去只会互相挤掉。这一页只放判因必需的字段，读一次全可见（≤1280B）。 */
static int svc_open(void *fp) { U32(fp,8)=0; U32(fp,12)=0; return 0; }
static int svc_read(void *fp,void *buf,unsigned len) {
  /* ★ v83：3328 → 3584（新增 9 个 iob_* 字段 ≈ 220B）*/
  static char text[3712]; struct text t={text,0,sizeof(text)}; unsigned n,pos;
  if(!fp || !buf) return -22;
  if(!len || U32(fp,12)) return 0;
  pos=U32(fp,8);
  NET_LOCK();
  mt_cmd_poll();
  if(g_dbg_on!=1) {          /* 关闸：一行收工，不做任何重格式化 */
    addstr(&t,"tunsvc v107 / p67tc 3.101.043\n");
    addstr(&t,"dbg=off  (touch /data/tundebug 开启日志)\n");
    addstr(&t,"mt_build="); addstr(&t,MT_BUILD); addstr(&t,"\n");
    field(&t,"mt_armed",g_mt.armed,10);
    field(&t,"netdev_ready",g_netdev_up,10);
    field(&t,"dns_queries",g_dns_queries,10);
    field(&t,"dns_sent",g_dns_sent,10);
    field(&t,"dns_replies",g_dns_replies,10);
    addstr(&t,"mt_end=1\n");
    NET_UNLOCK();
    if(pos>=t.n) return 0; n=t.n-pos; if(n>len) n=len;
    copy(buf,(const unsigned char *)text+pos,n); U32(fp,8)=pos+n; return (int)n;
  }
  addstr(&t,"tunsvc v107 / p67tc 3.101.043\n");
  field(&t,"mt_cmd_addr",(uint32_t)(uintptr_t)&g_mt_cmd,16);   /* ★v80 前置，防截断 */
  addstr(&t,"mt_build="); addstr(&t,MT_BUILD); addstr(&t,"\n");
  /* ★ v83：IOB 扩容判读（前置，永不被截断）
   *   iob_grow_rc: 0=成功 1=已挂过 2=找不到 freelist 3=没有干净空闲区 4=链失效
   *   iob_head_var = &g_iob_freelist；iob_scratch = 我们自己节点的起始地址；
   *   iob_chain_n  = 定位那一刻看到的老链长（24 个池 ⇒ 通常 23~24）。
   *   挂成功后立刻看 /proc/iobinfo：nfree 应从 24 变成 88。*/
  field(&t,"iob_ext_n",IOB_EXT_N_,10);
  field(&t,"iob_grow_calls",g_iob_grow_calls,10);
  field(&t,"iob_grow_rc",g_iob_grow_rc,10);
  field(&t,"iob_attached",g_iob_attached,10);
  field(&t,"iob_head_var",g_iob_head_var,16);
  field(&t,"iob_scratch",g_iob_scratch,16);
  field(&t,"iob_chain_n",g_iob_chain_n,10);
  field(&t,"iob_head_before",g_iob_head_before,16);
  field(&t,"iob_head_after",g_iob_head_after,16);
  field(&t,"iob_cnt_before",g_iob_cnt_before,10);
  field(&t,"iob_cnt_after",g_iob_cnt_after,10);
  /* ★ v84：最近 4 个 TCP 包头部（每 20 B：tick seq ack sport dport len flags dir）*/
  field(&t,"tcpring_n",g_tcpr_head,10);
  /* ★ v91：cmd 24 自证 —— calls 不涨就说明命令没被执行 */
  field(&t,"navail_on",g_nv_on,6);
  field(&t,"navail_hits",g_nv_hits,8);
  field(&t,"navail_ret",g_nv_ret,8);
  field(&t,"navail_raw",g_nv_raw,8);
  field(&t,"navail_w32",g_nv_w32,10);
  field(&t,"nv_traps",g_nv_traps,8);
  field(&t,"nv_arm_rc",g_nv_arm_rc,8);
  field(&t,"nv_neigh",g_nv_neigh,8);
  field(&t,"win_rewrites",g_win_rewrites,10);
  field(&t,"netin_calls",g_netin_calls,10);
  field(&t,"netin_rc_hex",g_netin_rc,10);

  field(&t,"fake_calls",g_fake_calls,10);
  field(&t,"fake_ip_rc",g_fake_ip_rc,10);
  field(&t,"fake_mask_rc",g_fake_mask_rc,10);
  field(&t,"fake_gw_rc",g_fake_gw_rc,10);
  /* —— ★ v52 socket 传输（DNS 收发主路径）——
   * 判读：sd_en=1 且 sd_fd>0 ⇒ 建链成功；sd_erc 涨 ⇒ 建链失败（看 *_rc/errno）；
   * sd_ers 涨 ⇒ 发不出去（11=忙 101=无路由）；sd_rok=0 且 sd_rnil 涨 ⇒
   * 发得出去、回包不来（对端/手机侧问题）。*/
  field(&t,"sd_en",(uint32_t)g_sd.enabled,10);
  field(&t,"sd_fd",(uint32_t)g_sd.fd,10);
  field(&t,"sd_open_rc",(uint32_t)g_sd.open_rc,16);
  field(&t,"sd_orcv_rc",(uint32_t)g_sd.so_rcv_rc,16);
  field(&t,"sd_osnd_rc",(uint32_t)g_sd.so_snd_rc,16);
  field(&t,"sd_bind_rc",(uint32_t)g_sd.bind_rc,16);
  field(&t,"sd_conn_rc",(uint32_t)g_sd.conn_rc,16);
  field(&t,"sd_connected",(uint32_t)g_sd.connected,10);
  field(&t,"sd_connects",g_sd.connects,10);
  field(&t,"sd_port",(uint32_t)g_sd.port,10);
  field(&t,"sd_bound",(uint32_t)g_sd.bound,10);
  field(&t,"sd_send_rc",(uint32_t)g_sd.send_rc,16);
  field(&t,"sd_recv_rc",(uint32_t)g_sd.recv_rc,16);
  field(&t,"sd_errno",(uint32_t)g_sd.last_errno,10);
  field(&t,"sd_sends",g_sd.sends,10);
  field(&t,"sd_ready",g_sd.ready,10);
  field(&t,"sd_erc",(uint32_t)(g_sd.err_sock+g_sd.err_opt+g_sd.err_bind),10);
  field(&t,"sd_ers",(uint32_t)(g_sd.err_send+g_sd_send_fail),10);
  field(&t,"sd_errr",g_sd.err_recv,10);
  field(&t,"sd_polls",(uint32_t)g_sd_polls,10);
  field(&t,"sd_rok",(uint32_t)g_sd_recv_ok,10);
  field(&t,"sd_rnil",(uint32_t)g_sd_recv_empty,10);
  field(&t,"sd_rdrop",(uint32_t)g_sd_consume_drop,10);
  field(&t,"sd_resets",(uint32_t)g_sd_resets,10);
  field(&t,"sd_lp_runs",(uint32_t)g_sd_runs,10);
  field(&t,"sd_req_sent",(uint32_t)g_sd_req_sent,10);
  field(&t,"sd_req_drop",(uint32_t)g_sd_req_drop,10);
  field(&t,"sd_want_open",(uint32_t)g_sdq.want_open,10);
  field(&t,"sd_step",(uint32_t)g_sd.step,10);      /* 0空闲 1建链 2send 3recv：卡住时看这个 */
  field(&t,"sd_binds",g_sd.binds,10);
  field(&t,"sd_tx_enq",(uint32_t)g_sd_tx_enq,10);
  field(&t,"sd_rounds",(uint32_t)g_sd_rounds,10);                          /* ★9PRO */
  field(&t,"sd_sendto_n",(uint32_t)g_sd.sendto_n,10);                      /* ★9PRO */
  field(&t,"sd_sendto_rc",(uint32_t)g_sd.sendto_rc,10);                    /* ★9PRO */
  field(&t,"sd_retries",(uint32_t)g_sd.retries,10);                        /* ★9PRO */
  field(&t,"sd_auto_port",(uint32_t)g_sd.auto_port,10);                    /* ★9PRO */
  field(&t,"sd_open_ok",(uint32_t)g_sd_open_ok,10);
  field(&t,"sd_open_fail",(uint32_t)g_sd_open_fail,10);
  field(&t,"sd_open_warn",(uint32_t)g_sd_open_warn,10);   /* 1=曾手动开过 socket 收包（有卡死风险）*/
  /* ★ v57/60 自检（mt_cmd 16）：0=socket 1=RCVTIMEO 2=SNDTIMEO 3=bind 4=connect 5=recvfrom(NULL) */
  field(&t,"st_self_n",(uint32_t)g_sd_self_done,10);
  field(&t,"st0_rc",(uint32_t)g_sd_st_rc[0],16); field(&t,"st0_errno",(uint32_t)g_sd_st_err[0],10);
  field(&t,"st1_rc",(uint32_t)g_sd_st_rc[1],16); field(&t,"st1_errno",(uint32_t)g_sd_st_err[1],10);
  field(&t,"st2_rc",(uint32_t)g_sd_st_rc[2],16); field(&t,"st2_errno",(uint32_t)g_sd_st_err[2],10);
  field(&t,"st3_rc",(uint32_t)g_sd_st_rc[3],16); field(&t,"st3_errno",(uint32_t)g_sd_st_err[3],10);
  field(&t,"st4_rc",(uint32_t)g_sd_st_rc[4],16); field(&t,"st4_errno",(uint32_t)g_sd_st_err[4],10);
  field(&t,"st5_rc",(uint32_t)g_sd_st_rc[5],16); field(&t,"st5_errno",(uint32_t)g_sd_st_err[5],10);
  /* ★ v62 注入式自检（mt_cmd 17）：build=帧长 recv=收到字节 sentinel=1 缓冲真被改写
   * consume=2(CONSUMED) cache_ip=0x5DB8D822 就是全链路打通 */
  field(&t,"inj_n",(uint32_t)g_inj_n,10);
  field(&t,"inj_build",(uint32_t)g_inj_build_len,10);
  field(&t,"inj_recv_rc",(uint32_t)g_inj_recv_rc,16);
  field(&t,"inj_sentinel",(uint32_t)g_inj_sentinel_ok,10);
  field(&t,"inj_consume",(uint32_t)g_inj_consume_rc,16);
  field(&t,"inj_cache_ip",(uint32_t)g_inj_cache_ip,16);
  field(&t,"inj_deny_n",(uint32_t)g_inj_deny_n,10);   /* ★v73 被 netdev 门拒收次数 */
  field(&t,"inj_denied",(uint32_t)g_inj_denied,10);   /* ★v73 最近一次是否被拒 */
  /* ★ v68 TCP-DNS 自检（mt_cmd 18）：socket/connect/send/hdr/recv 逐步 rc，
   * cache_ip 有值 = TCP 这条路走通了（RFC 7766） */
  field(&t,"td_n",(uint32_t)g_td_n,10);
  field(&t,"td_len",(uint32_t)g_td_len,10);
  field(&t,"td_socket_rc",(uint32_t)g_td_socket_rc,16);
  field(&t,"td_conn_rc",(uint32_t)g_td_conn_rc,16);
  field(&t,"td_send_rc",(uint32_t)g_td_send_rc,16);
  field(&t,"td_hdr_rc",(uint32_t)g_td_hdr_rc,16);
  field(&t,"td_recv_rc",(uint32_t)g_td_recv_rc,16);
  field(&t,"td_consume_rc",(uint32_t)g_td_consume_rc,16);
  field(&t,"td_cache_ip",(uint32_t)g_td_cache_ip,16);
  /* —— ★v72 tunentry 写入口（mt_cmd 14/15/16/17/18 现在真机上也能下发）—— */
  field(&t,"entry_writes",g_entry_writes,10);
  field(&t,"entry_cmds",g_entry_cmds,10);
  field(&t,"entry_bad",g_entry_bad,10);
  field(&t,"entry_last_cmd",g_entry_last_cmd,10);
  field(&t,"entry_last_len",g_entry_last_len,10);   /* ★v73 收到的字节数 */
  dump_hex(&t,"entry_head=",(const uint8_t *)g_entry_head,8u);  /* ★v73 前 8 字节 */
  field(&t,"sd_wedge",(uint32_t)g_sd_wedge,10);    /* 建链失败自动退 raw 的次数 */
  field(&t,"sd_stall_ev",(uint32_t)g_sd_stall_events,10);  /* v59：中止在飞查询次数 */
  field(&t,"sd_kick_busy",(uint32_t)g_sd_kick_busy,10);    /* 踢被"已排队"跳过次数 */
  field(&t,"sd_flag_extra",(uint32_t)g_sd.flag_extra,10);
  field(&t,"sd_stall_max",(uint32_t)g_sd_stall_max,10);
  field(&t,"sd_srv",(uint32_t)(((uint32_t)g_sd.server[0]<<24)|((uint32_t)g_sd.server[1]<<16)|
                               ((uint32_t)g_sd.server[2]<<8)|(uint32_t)g_sd.server[3]),16);
  /* —— v37 自持节拍（排空器）：drain_runs 应在持续增长（每 ~1s 一次）—— */
  field(&t,"drain_runs",(uint32_t)g_drain_runs,10);
  field(&t,"drain_kicks",(uint32_t)g_drain_kicks,10);
  field(&t,"drain_last_rc_hex",(uint32_t)g_drain_last_rc,16);
  /* —— v38 陷阱现场判据（每次陷阱都记录）：能不能安全等待 —— */
  field(&t,"nested_faults",g_mt.nested_faults,10);
  field(&t,"nested_pc_hex",g_mt.nested_pc,16);
  field(&t,"nested_storm",g_mt.nested_storm,10);
  field(&t,"skips",g_mt.skips,10);
  field(&t,"last_skip_pc_hex",g_mt.last_skip_pc,16);
  field(&t,"crit_at_trap",g_mt.crit_at_trap,10);
  field(&t,"basepri_at_trap",g_mt.basepri_at_trap,16);
  field(&t,"primask_at_trap",g_mt.primask_at_trap,16);
  field(&t,"nl_cnt",g_mt.nl_cnt,10);
  field(&t,"nl_holder_hex",g_mt.nl_holder,16);
  field(&t,"nl_me_hex",g_mt.nl_me,16);
  field(&t,"nl_mine",g_mt.nl_mine,10);
  field(&t,"nl_mine_yes",g_mt.nl_mine_yes,10);
  field(&t,"nl_mine_no",g_mt.nl_mine_no,10);
  /* —— ★ v49 陷阱健康（诊断"用着用着突然失效"：卸防后有没有装回、装了几次）——
   * armed=1 正常；armed=0 且 rearm_pending/count 有动静 = 刚被卸防正在/已经装回；
   * noframe→ 认不出帧；kstorm→连续认不出 9 次；failsafe_n 每次卸防 +1。 */
  field(&t,"mt_state",g_mt.state,10);
  field(&t,"mt_armed",g_mt.armed,10);
  field(&t,"mt_reason_hex",g_mt.reason,16);
  field(&t,"rearm_count",g_mt.rearm_count,10);
  field(&t,"rearm_pending",g_mt.rearm_pending,10);
  field(&t,"failsafe_n",g_mt.failsafe_count,10);
  field(&t,"noframe_traps",g_mt.noframe_traps,10);
  field(&t,"unknown_traps",g_mt.unknown_traps,10);
  field(&t,"streak",g_mt.streak,10);
  field(&t,"entry_traps",g_mt.entry_traps,10);
  /* —— v38 慢路径（真同步等待；cmd 9 开 / 10 关）—— */
  field(&t,"slow_enabled",g_mt.slow_enabled,10);
  field(&t,"slow_entries",g_mt.slow_entries,10);
  field(&t,"slow_calls",g_mt.slow_calls,10);
  field(&t,"slow_hits",g_mt.slow_hits,10);
  field(&t,"slow_timeouts",g_mt.slow_timeouts,10);
  field(&t,"slow_skip_crit",g_mt.slow_skip_crit,10);
  field(&t,"slow_skip_netdown",g_mt.slow_skip_netdown,10);
  field(&t,"slow_nl_released",g_mt.slow_nl_released,10);
  field(&t,"slow_wait_beats",g_mt.slow_wait_beats,10);
  field(&t,"slow_kicks",g_slow_kicks,10);
  field(&t,"slow_basepri",g_mt.slow_basepri_last,16);
  field(&t,"slow_nl_cnt",g_mt.slow_nl_cnt,10);
  /* —— 看门狗 / 在飞查询 —— */
  field(&t,"dns_wd_now",g_mt_dns_wd,10);
  field(&t,"dns_wd_aborts",g_mt_dns_wd_aborts,10);
  field(&t,"dns_phase",(unsigned)g_dns.phase,10);   /* ★ v47：0=IDLE 1=WAIT 2=DONE 3=FAILED */
  field(&t,"dns_stuck_drops",g_mt_dns_stuck_drops,10);
  addstr(&t,"inflight="); addstr(&t,g_mt_inflight_name[0]?g_mt_inflight_name:"(none)"); addstr(&t,"\n");
  field(&t,"inflight_tries",g_mt_inflight_tries,10);
  /* —— 待解析队列 —— */
  {
    char nm[MT_NAME_MAX]; uint32_t sq=0u, tr=0u;
    if(mt_pend_peek(nm,&sq,&tr)) {
      addstr(&t,"pend_name="); addstr(&t,nm); addstr(&t,"\n");
      field(&t,"pend_tries",tr,10);
    } else addstr(&t,"pend_name=(empty)\n");
  }
  field(&t,"pend_done",g_mt.pend_done,10);
  field(&t,"pend_drops",g_mt_pend_drops,10);
  /* —— 网络状态 —— */
  field(&t,"netdev_ready",g_netdev_up,10);
  /* ★★ v77：注册链路的四级证据（按顺序读就能定位卡在哪一层）*/
  field(&t,"ioctl_n",g_ioctl_n,10);          /* 0 ⇒ 没有任何进程碰过 /dev/tun */
  field(&t,"ioctl_728",g_ioctl_728,10);      /* 0 ⇒ 有进程用它，但从没叫过注册 */
  field(&t,"reg_tries",g_reg_tries,10);      /* 0 ⇒ NETREG 一次都没试过 */
  field(&t,"reg_rc_abs",(uint32_t)(g_reg_last_rc>0x80000000u
                                   ? 0u-g_reg_last_rc : g_reg_last_rc),10);
  field(&t,"reg_rc_neg",g_reg_last_rc>0x80000000u?1u:0u,10);  /* 1 ⇒ 被 registrar 拒 */
  field(&t,"reg_short",g_reg_short,10);      /* >0 ⇒ 早已注册成功（短路返回）*/
  /* ★★ v79：宿主侧桥（cmd 20）—— 模拟器端到端验 DNS 用 */
  field(&t,"bridge_calls",g_bridge_calls,10);
  field(&t,"bridge_len",g_bridge_len,10);      /* 宿主用 mw 写进来的包长 */
  field(&t,"bridge_result",g_bridge_result,10);/* 0未跑 1成功 2长度错 3netdev 4非DNS */
  field(&t,"bridge_ok",g_bridge_ok,10);
  /* ★ v81：指定域名查询（cmd 21）*/
  field(&t,"named_calls",g_named_calls,10);
  field(&t,"named_result",g_named_result,10);   /* 0未跑 1已发出 2域名空 3被拒 */
  field(&t,"dhcp_state",g_dhcp_state,10);
  field(&t,"ip_configured",g_ip_configured,10);
  field(&t,"dns_raw_le",ip4_le(g_dhcp_lease.dns),16);
  field(&t,"dns_raw2_le",g_dhcp_lease.dns2_present?ip4_le(g_dhcp_lease.dns2):0u,16); /* ★v71*/
  field(&t,"dns_srv_idx",(uint32_t)g_dns_srv_idx,10);
  /* —— DNS 客户端 —— */
  field(&t,"dns_phase",g_dns.phase,10);
  field(&t,"dns_queries",g_dns_queries,10);
  field(&t,"dns_sent",g_dns_sent,10);
  field(&t,"dns_replies",g_dns_replies,10);
  field(&t,"dns_ignored",g_dns_ignored,10);
  field(&t,"dns_rtt_beats",g_dns_last_rtt_beats,10);
  field(&t,"dns_rtt_max_beats",g_dns_rtt_max_beats,10);
  /* ★v69 看门狗当前拍数（limit=50 拍≈5 秒；==50 表示真超时）*/
  field(&t,"wd_beats",g_mt_dns_wd,10);
  /* ★v69 原地重发次数（对齐 9Pro 的第 2/3 次 send）*/
  field(&t,"dns_resends",g_dns_resends,10);
  field(&t,"ign_idle",g_dns_diag.ign_idle,10);
  field(&t,"ign_parse",g_dns_diag.ign_parse,10);
  field(&t,"ign_port",g_dns_diag.ign_port,10);
  field(&t,"ign_id",g_dns_diag.ign_id,10);
  field(&t,"ign_qname",g_dns_diag.ign_qname,10);
  field(&t,"loose_accepts",g_dns_diag.loose_accepts,10);
  field(&t,"strict_ok",g_dns_diag.strict_ok,10);
  field(&t,"last_ign_sport",g_dns_diag.last_ign_sport,10);
  field(&t,"last_ign_dport",g_dns_diag.last_ign_dport,10);
  field(&t,"last_ign_id",g_dns_diag.last_ign_id,16);
  field(&t,"last_ign_status",g_dns_diag.last_ign_status,16);
  { uint32_t a=((uint32_t)g_dns_diag.last_ign_sip[0]<<24)|((uint32_t)g_dns_diag.last_ign_sip[1]<<16)|((uint32_t)g_dns_diag.last_ign_sip[2]<<8)|g_dns_diag.last_ign_sip[3];
    uint32_t b=((uint32_t)g_dns_diag.last_ign_dip[0]<<24)|((uint32_t)g_dns_diag.last_ign_dip[1]<<16)|((uint32_t)g_dns_diag.last_ign_dip[2]<<8)|g_dns_diag.last_ign_dip[3];
    field(&t,"last_ign_sip_hex",a,16); field(&t,"last_ign_dip_hex",b,16); }
  field(&t,"dns_failures",g_dns_failures,10);
  field(&t,"dns_status",(uint32_t)g_dns.status,16);
  /* —— ★ v50 抓帧 + 旁路探测（"回包丢了"的定案数据；可宿主机离线重放）——
   * pf_* = 解析失败的精确原因分布；look_* = 绕过严格校验的形状探测；
   * prob/good = 最后被拒/最后被消费的原始帧（各 64B hex）。 */
  field(&t,"pf_badip",g_dns_diag.parse_fail_hist[3],10);
  field(&t,"pf_badlen",g_dns_diag.parse_fail_hist[4],10);
  field(&t,"pf_frag",g_dns_diag.parse_fail_hist[5],10);
  field(&t,"pf_notudp",g_dns_diag.parse_fail_hist[6],10);
  field(&t,"pf_ipck",g_dns_diag.parse_fail_hist[7],10);
  field(&t,"pf_udpck",g_dns_diag.parse_fail_hist[8],10);
  field(&t,"look_dns",g_dns_diag.look_dns,10);
  field(&t,"look_udp",g_dns_diag.look_udp,10);
  field(&t,"look_other",g_dns_diag.look_other,10);
  field(&t,"prob_kind",(unsigned)g_dns_diag.prob_kind,10);
  field(&t,"prob_len",(unsigned)g_dns_diag.prob_len,10);
  field(&t,"prob_status_hex",(uint32_t)g_dns_diag.prob_status,16);
  field(&t,"good_len",(unsigned)g_dns_diag.good_len,10);
  field(&t,"good_status_hex",(uint32_t)g_dns_diag.good_status,16);
  /* —— 桥的节拍（服务多久被调一次：用两次 cat 的差值估）—— */
  field(&t,"poll_setup",(uint32_t)g_stats.poll_setup,10);
  field(&t,"poll_teardown",(uint32_t)g_stats.poll_teardown,10);
  field(&t,"read_packets",(uint32_t)g_stats.read_packets,10);
  field(&t,"rx_packets",(uint32_t)g_stats.rx_packets,10);
  field(&t,"tx_packets",(uint32_t)g_stats.tx_packets,10);
  field(&t,"cache_count",g_mt.cache_count,10);
  addstr(&t,"mt_end=1\n");
  NET_UNLOCK();
  if(pos>=t.n) return 0; n=t.n-pos; if(n>len) n=len;
  copy(buf,(const unsigned char *)text+pos,n); U32(fp,8)=pos+n; return (int)n;
}
/* ★ v53：/dev/tunentry 整页摘除 —— 文件体积闸门（65536）需要空间，而这一页的全部
 * 字段（entry_xx/fpb_xx 等 20 余项）在 /dev/tunstat 里都有；FPB 路线早已定案
 * （comp7_not_idle / wrote=0），探针仍可由 TUN_ENTRY_SELFTEST ioctl 触发。
 * 摘除释放：.text ≈2.7KB + .bss 2.4KB。*/
static char g_text[3488];   /* ★v86：tunstat 与 tunlog 共用（都持 NET_LOCK 串行）*/
static int stat_read(void *fp,void *buf,unsigned len) {
  /* ★ v80：3072 → 3584。踩过：加上 v77/v79 的字段后本页正好顶到 3071/3072，
   * 生成器是从前往后写、写满即止 ⇒ **mt_cmd_addr 这些靠后的字段被静默截掉**，
   * 表现为"tunstat 里查不到 mt_cmd_addr"。缓冲区要留余量。 */
  struct text t={g_text,0,sizeof(g_text)}; unsigned n,pos;
  if(!fp || !buf) return -22;
  if(!len) return 0;
  if(U32(fp,12)) return 0;
  pos=U32(fp,8);
  NET_LOCK(); addstr(&t,"tun ipdns v3 / p67tc 3.101.043\n");
  /* ★ v80：把这行**前置到页面最前面** —— 它是宿主算 base 的唯一依据
   * （base = mt_cmd_addr - off(g_mt_cmd)），绝不能因为后来加字段被挤掉。 */
  field(&t,"mt_cmd_addr",(uint32_t)(uintptr_t)&g_mt_cmd,16);
  /* Execute any staged trap command first, so this very read reports the
   * result of "mw <mt_cmd_addr>=<n>" typed just before it. */
  mt_cmd_poll();
  field(&t,"netdev_ready",g_netdev_up,10);
  /* ★★ v77：注册链路的四级证据（按顺序读就能定位卡在哪一层）*/
  field(&t,"ioctl_n",g_ioctl_n,10);          /* 0 ⇒ 没有任何进程碰过 /dev/tun */
  field(&t,"ioctl_728",g_ioctl_728,10);      /* 0 ⇒ 有进程用它，但从没叫过注册 */
  field(&t,"reg_tries",g_reg_tries,10);      /* 0 ⇒ NETREG 一次都没试过 */
  field(&t,"reg_rc_abs",(uint32_t)(g_reg_last_rc>0x80000000u
                                   ? 0u-g_reg_last_rc : g_reg_last_rc),10);
  field(&t,"reg_rc_neg",g_reg_last_rc>0x80000000u?1u:0u,10);  /* 1 ⇒ 被 registrar 拒 */
  field(&t,"reg_short",g_reg_short,10);      /* >0 ⇒ 早已注册成功（短路返回）*/
  /* ★★ v79：宿主侧桥（cmd 20）—— 模拟器端到端验 DNS 用 */
  field(&t,"bridge_calls",g_bridge_calls,10);
  field(&t,"bridge_len",g_bridge_len,10);      /* 宿主用 mw 写进来的包长 */
  field(&t,"bridge_result",g_bridge_result,10);/* 0未跑 1成功 2长度错 3netdev 4非DNS */
  field(&t,"bridge_ok",g_bridge_ok,10);
  /* ★ v81：指定域名查询（cmd 21）*/
  field(&t,"named_calls",g_named_calls,10);
  field(&t,"named_result",g_named_result,10);   /* 0未跑 1已发出 2域名空 3被拒 */
  field(&t,"lltype",g_netdev[0x18],10);
  field(&t,"flags",U32(g_netdev,ND_FLAGS),16);
  field(&t,"ip_raw_le",U32(g_netdev,0x20),16);
  field(&t,"dr_raw_le",U32(g_netdev,0x24),16);
  field(&t,"mask_raw_le",U32(g_netdev,0x28),16);
  field(&t,"dhcp_state",g_dhcp_state,10);
  field(&t,"dhcp_discover",g_dhcp_discover,10);
  field(&t,"dhcp_offer",g_dhcp_offers,10);
  field(&t,"dhcp_request",g_dhcp_requests,10);
  field(&t,"dhcp_ack",g_dhcp_acks,10);
  field(&t,"dhcp_bad",g_dhcp_bad,10);
  field(&t,"dhcp_nak",g_dhcp_naks,10);
  field(&t,"dhcp_queue_full",g_dhcp_queue_full,10);
  field(&t,"ip_configured",g_ip_configured,10);
  field(&t,"ip_set_rc",(uint32_t)g_ip_rc,16);
  field(&t,"mask_set_rc",(uint32_t)g_mask_rc,16);
  field(&t,"gw_set_rc",(uint32_t)g_gw_rc,16);
  field(&t,"rollback_rc",(uint32_t)g_rollback_rc,16);
  field(&t,"dns_raw_le",ip4_le(g_dhcp_lease.dns),16);
  field(&t,"dns_raw2_le",g_dhcp_lease.dns2_present?ip4_le(g_dhcp_lease.dns2):0u,16); /* ★v71*/
  field(&t,"dns_srv_idx",(uint32_t)g_dns_srv_idx,10);
  field(&t,"system_dns_hook",(g_mt.armed&&g_mt.state==MT_ST_ARMED)?1u:0u,10);
  field(&t,"dns_phase",g_dns.phase,10);
  field(&t,"dns_id",g_dns.query_id,16);
  field(&t,"dns_status",(uint32_t)g_dns.status,16);
  field(&t,"dns_queries",g_dns_queries,10);
  field(&t,"dns_sent",g_dns_sent,10);
  field(&t,"dns_replies",g_dns_replies,10);
  field(&t,"dns_ignored",g_dns_ignored,10);
  field(&t,"dns_rtt_beats",g_dns_last_rtt_beats,10);
  field(&t,"dns_rtt_max_beats",g_dns_rtt_max_beats,10);
  /* ★v69 看门狗当前拍数（limit=50 拍≈5 秒；==50 表示真超时）*/
  field(&t,"wd_beats",g_mt_dns_wd,10);
  /* ★v69 原地重发次数（对齐 9Pro 的第 2/3 次 send）*/
  field(&t,"dns_resends",g_dns_resends,10);
  field(&t,"ign_idle",g_dns_diag.ign_idle,10);
  field(&t,"ign_parse",g_dns_diag.ign_parse,10);
  field(&t,"ign_port",g_dns_diag.ign_port,10);
  field(&t,"ign_id",g_dns_diag.ign_id,10);
  field(&t,"ign_qname",g_dns_diag.ign_qname,10);
  field(&t,"loose_accepts",g_dns_diag.loose_accepts,10);
  field(&t,"strict_ok",g_dns_diag.strict_ok,10);
  field(&t,"last_ign_sport",g_dns_diag.last_ign_sport,10);
  field(&t,"last_ign_dport",g_dns_diag.last_ign_dport,10);
  field(&t,"last_ign_id",g_dns_diag.last_ign_id,16);
  field(&t,"last_ign_status",g_dns_diag.last_ign_status,16);
  { uint32_t a=((uint32_t)g_dns_diag.last_ign_sip[0]<<24)|((uint32_t)g_dns_diag.last_ign_sip[1]<<16)|((uint32_t)g_dns_diag.last_ign_sip[2]<<8)|g_dns_diag.last_ign_sip[3];
    uint32_t b=((uint32_t)g_dns_diag.last_ign_dip[0]<<24)|((uint32_t)g_dns_diag.last_ign_dip[1]<<16)|((uint32_t)g_dns_diag.last_ign_dip[2]<<8)|g_dns_diag.last_ign_dip[3];
    field(&t,"last_ign_sip_hex",a,16); field(&t,"last_ign_dip_hex",b,16); }
  field(&t,"dns_failures",g_dns_failures,10);
  field(&t,"dns_last_transport",(uint32_t)g_dns_last_transport,16);
  field(&t,"dns_answer_count",g_dns.result.address_count,10);
  field(&t,"dns_answer0_le",g_dns.result.address_count?ip4_le(g_dns.result.addresses[0].bytes):0u,16);
  addstr(&t,"mt_build="); addstr(&t,MT_BUILD); addstr(&t,"\n");
  field(&t,"entry_probe_ran",g_entry_probe_ran,10);
  field(&t,"entry_patch_attempted",g_entry_patch_attempted,10);
  field(&t,"entry_patch_ready",g_entry_patch_ready,10);
  field(&t,"entry_callsite_ok",g_entry_callsite_ok,10);
  field(&t,"entry_reason_code",g_entry_reason,10);
  /* ★v71 修笔误：原来是 "\\n"（双反斜杠）⇒ 输出字面 '\'+'n' 而不是换行，
     把后面的 entry_cpuid 粘在同一行，脚本按行解析就取不到该字段。 */
  addstr(&t,"entry_reason="); addstr(&t,entry_reason_text(g_entry_reason)); addstr(&t,"\n");
  field(&t,"entry_cpuid",g_entry_cpuid,16);
  field(&t,"entry_fp_ctrl",g_entry_fp_ctrl,16);
  field(&t,"entry_fp_num_code",entry_fpb_num_code(g_entry_fp_ctrl),10);
  field(&t,"entry_fp_num_lit",entry_fpb_num_lit(g_entry_fp_ctrl),10);
  field(&t,"entry_fp_rev",entry_fpb_rev(g_entry_fp_ctrl),10);
  field(&t,"entry_fp_remap",g_entry_fp_remap,16);
  field(&t,"entry_fp_lar",g_entry_fp_lar,16);
  field(&t,"entry_fp_lsr",g_entry_fp_lsr,16);
  field(&t,"entry_fp_comp0",g_entry_fp_comp[0],16);
  field(&t,"entry_fp_comp7",g_entry_fp_comp[7],16);
  field(&t,"entry_demcr",g_entry_demcr,16);
  field(&t,"entry_vtor",g_entry_vtor,16);
  field(&t,"entry_vtor_aligned",g_entry_vtor_aligned,10);
  field(&t,"entry_debugmon_vector_addr",g_entry_debugmon_addr,16);
  field(&t,"entry_debugmon_vector_read",g_entry_debugmon_read,10);
  field(&t,"entry_debugmon_vector",g_entry_debugmon_vector,16);
  field(&t,"entry_bl0",g_entry_bl0,16);
  field(&t,"entry_bl1",g_entry_bl1,16);
  field(&t,"entry_bl2",g_entry_bl2,16);
  field(&t,"entry_bl3",g_entry_bl3,16);
  field(&t,"entry_module_text0",g_entry_text0,16);
  field(&t,"entry_module_text1",g_entry_text1,16);
  field(&t,"entry_module_data",g_entry_data,16);
  field(&t,"entry_state",g_entry_state,10);
  field(&t,"entry_backend_kind",g_entry_backend_kind,16);
  field(&t,"entry_backend_validated",g_entry_backend_validated,16);
  field(&t,"entry_last_op_rc",g_entry_last_op_rc,16);
  field(&t,"entry_apply_count",g_entry_apply_count,10);
  field(&t,"entry_disable_count",g_entry_disable_count,10);
  field(&t,"entry_restore_count",g_entry_restore_count,10);
  field(&t,"entry_backup_valid",g_entry_backup_valid,10);
  field(&t,"entry_target0",g_entry_target0,16);
  field(&t,"entry_target1",g_entry_target1,16);
  field(&t,"entry_target2",g_entry_target2,16);
  field(&t,"entry_target3",g_entry_target3,16);
  field(&t,"entry_backup0",g_entry_backup0,16);
  field(&t,"entry_backup1",g_entry_backup1,16);
  field(&t,"entry_backup2",g_entry_backup2,16);
  field(&t,"entry_backup3",g_entry_backup3,16);
  field(&t,"mt_state",g_mt.state,10);
  field(&t,"mt_armed",g_mt.armed,10);
  field(&t,"mt_reason_code",g_mt.reason,10);
  addstr(&t,"mt_reason="); addstr(&t,mt_reason_text(g_mt.reason)); addstr(&t,"\n");
  field(&t,"mt_handler",g_mt.handler_addr,16);
  field(&t,"mt_hf_handler",g_mt.hf_handler_addr,16);
  field(&t,"mt_stub",g_mt.stub_addr,16);
  field(&t,"mt_entry_ok",g_mt.entry_ok,10);
  field(&t,"mt_neighbour_ok",g_mt.neighbour_ok,10);
  field(&t,"mt_vector_addr",g_mt.vector_addr,16);
  field(&t,"mt_vector_before",g_mt.vector_before,16);
  field(&t,"mt_vector_now",g_mt.vector_now,16);
  field(&t,"mt_vector_hf_addr",g_mt.vector_hf_addr,16);
  field(&t,"mt_vector_hf_before",g_mt.vector_hf_before,16);
  field(&t,"mt_vector_hf_now",g_mt.vector_hf_now,16);
  field(&t,"mt_hf_traps",g_mt.hf_traps,10);
  field(&t,"mt_hf_fixed",g_mt.hf_fixed,10);
  field(&t,"mt_hf_foreign",g_mt.hf_foreign,10);
  field(&t,"mt_hf_pc",g_mt.hf_pc,16);
  field(&t,"mt_hf_lr",g_mt.hf_lr,16);
  field(&t,"mt_nested_faults",g_mt.nested_faults,10);
  field(&t,"mt_skips",g_mt.skips,10);
  field(&t,"mt_last_skip_pc",g_mt.last_skip_pc,16);
  field(&t,"mt_streak",g_mt.streak,10);
  field(&t,"mt_region",g_mt.region,10);
  field(&t,"mt_rbar",g_mt.rbar,16);
  field(&t,"mt_rlar",g_mt.rlar,16);
  field(&t,"mt_stage",g_mt.stage,10);
  field(&t,"mt_mpu_type",g_mt.mpu_type,16);
  field(&t,"mt_mpu_ctrl_before",g_mt.mpu_ctrl_before,16);
  field(&t,"mt_mpu_ctrl_after",g_mt.mpu_ctrl_after,16);
  field(&t,"mt_shcsr_before",g_mt.shcsr_before,16);
  field(&t,"mt_shcsr_after",g_mt.shcsr_after,16);
  field(&t,"mt_shcsr_pend",g_mt.shcsr_pend,16);
  field(&t,"mt_failsafe_count",g_mt.failsafe_count,10);
  field(&t,"mt_preflight_rc",g_mt.preflight_rc,16);
  field(&t,"mt_mm_traps",g_mt.mm_traps,10);
  field(&t,"mt_mm_stub_addr",g_mt.mm_stub_addr,16);
  field(&t,"mt_mm_stub_seen",g_mt.mm_stub_seen,16);
  field(&t,"mt_mm_pc",g_mt.mm_pc,16);
  field(&t,"mt_trap_test_rc",g_mt.trap_test_rc,16);
  field(&t,"mt_subst_rc",g_mt.subst_rc,16);
  field(&t,"mt_traps",g_mt.traps,10);
  field(&t,"mt_entry_traps",g_mt.entry_traps,10);
  field(&t,"mt_neigh_traps",g_mt.neigh_traps,10);
  field(&t,"mt_noframe_traps",g_mt.noframe_traps,10);
  field(&t,"mt_frame_delta",g_mt.frame_delta,10);
  field(&t,"mt_caller_sp",g_mt.caller_sp_last,16);
  field(&t,"mt_lr",g_mt.lr_last,16);
  field(&t,"mt_errp",g_mt.errp_last,16);
  field(&t,"mt_flags",g_mt.flags_last,16);
  field(&t,"mt_cache_hits",g_mt.hits,10);
  field(&t,"mt_cache_misses",g_mt.misses,10);
  field(&t,"mt_subst_ok",g_mt.subst_ok,10);
  field(&t,"mt_subst_fail",g_mt.subst_fail,10);
  field(&t,"mt_cache_count",g_mt.cache_count,10);
  field(&t,"mt_resolve_writes",g_resolve_writes,10);
  field(&t,"mt_dns_abort_reqs",g_mt.dns_abort_reqs,10);
  field(&t,"mt_dns_wd_aborts",g_mt_dns_wd_aborts,10);
  field(&t,"mt_sem_ok",g_mt_sem_ok,10);
  field(&t,"mt_sem_init_rc",(uint32_t)g_mt_sem_rc_init,16);
  field(&t,"mt_sem_post_rc",(uint32_t)g_mt_sem_rc_post,16);
  field(&t,"mt_sem_wait_rc",(uint32_t)g_mt_sem_rc_wait,16);
  field(&t,"mt_resolve_last_len",g_resolve_last_len,10);
  field(&t,"mt_pend_seq",g_mt.pend_seq,10);
  field(&t,"mt_pend_drops",g_mt_pend_drops,10);
  field(&t,"mt_pend_done",g_mt.pend_done,10);
  /* ★v80：mt_cmd_addr 已前置到页面开头，这里不再重复（重复会白占缓冲）*/
  field(&t,"mt_cmd_seen",g_mt.cmd_seen,10);
  field(&t,"entry_writes",g_entry_writes,10);      /* ★v72 tunentry 写入口计数 */
  field(&t,"entry_cmds",g_entry_cmds,10);
  field(&t,"entry_bad",g_entry_bad,10);
  field(&t,"entry_last_cmd",g_entry_last_cmd,10);
  field(&t,"entry_last_len",g_entry_last_len,10);   /* ★v73 收到的字节数 */
  dump_hex(&t,"entry_head=",(const uint8_t *)g_entry_head,8u);  /* ★v73 前 8 字节 */
  field(&t,"iob_idle",U32(g_netdev,ND_IOB),16);
  field(&t,"buf_idle",U32(g_netdev,ND_BUF),16);
  field(&t,"queue_packets",(g_head+SLOTS-g_tail)%SLOTS,10);
#define STAT(s) field(&t,#s,(uint32_t)g_stats.s,10)
  STAT(tx_notify); STAT(work_queued); STAT(work_runs); STAT(work_error);
  STAT(tx_packets); STAT(tx_bytes); STAT(tx_drop);
  STAT(read_packets); STAT(read_bytes); STAT(read_empty);
  STAT(rx_packets); STAT(rx_bytes); STAT(rx_error); STAT(rx_reply);
  STAT(poll_setup); STAT(poll_teardown); STAT(poll_notify);
#undef STAT
  field(&t,"last_work_rc_hex",(uint32_t)g_stats.last_work_rc,16);
  field(&t,"last_input_rc_hex",(uint32_t)g_stats.last_input_rc,16);
  field(&t,"register_rc_hex",(uint32_t)g_stats.register_rc,16);
  field(&t,"stat_register_rc_hex",(uint32_t)g_stats.stat_register_rc,16);
  addstr(&t,"mt_end=1\n");
  NET_UNLOCK();
  if(pos>=t.n) return 0; n=t.n-pos; if(n>len) n=len;
  copy((uint8_t *)buf,(const uint8_t *)(g_text+pos),n); U32(fp,8)=pos+n; return (int)n;
}
static int tun_uninit(void *arg) { (void)arg; return g_registered?-16:0; }
int module_initialize(struct tun_mod_info *m) {
  m->uninitializer=tun_uninit; m->arg=0; m->exports=0; m->nexports=0; return 0;
}
static void tun_ctor(void) __attribute__((constructor));
static void tun_ctor(void) {
  int rc;
  /* Read-only by design.  A successful probe still ends in
   * remap_ram_unknown until the controller, remap base and patch backend are
   * independently established for this exact firmware. */
  entry_patch_probe();
  /* Resolver-entry trap.  mt_probe() is read-only; mt_arm() is a *verified
   * ladder* that refuses to arm anything until the previous stage has been
   * proven on this device (vector+SHCSR readback, flash preflight, synthetic
   * MemManage round trip), and rolls everything back on any failure. */
  mt_probe();
#if TUN_MT_AUTO_ARM
  (void)mt_arm();
#endif
  /* ★ v52：socket 传输初始化 —— fd 未知（-1）表示"还没建"；
   * enabled 默认 1（socket 路径优先），raw TUN 路径仍可经 mt_cmd 14 切回。*/
  g_sd.fd=-1;
  /* ★★★ v65（真机 v64 dump 定案）：**socket 收包默认关闭**，收包回到 raw 消费者。
   * 证据：v64 抓到了 `sd_step=3`（卡在 recvfrom 里）+ `sd_polls=0` ⇒ lpwork 被永久卡死
   *   （本固件的 recvfrom 对 SO_RCVTIMEO 不生效；模拟器计时也证明 0x04/0x08 都不是
   *   MSG_DONTWAIT，非阻塞拿不到）。lpwork 一死 ⇒ 查询被反复中止（sd_stall_ev=17）
   *   ⇒ 阶段不在 WAIT_REPLY ⇒ 手机**确实转发回来的 8 个回包**（ign_idle=8、
   *   last_ign_sport=53/dport=53167/sip=114.114.114.114）全被 ign_idle 拒掉。
   *   raw 消费者跑在 hpwork、永不阻塞 ⇒ 稳定收包；socket 路径保留为诊断开关（cmd 14），
   *   但**开启有卡死风险**，仅在有非阻塞手段（fcntl O_NONBLOCK）之后再开。 */
  /* ★9PRO-ROUND: v70 起**默认启用 socket 传输**（仿 9Pro 机制）。
   * v52~v69 都是默认关（sd_en=0），socket 通路实际从未在真机上跑过。 */
  /* ★★★★★ v78（真机 v75 实测，rev 掉 v70 的改动）：v70 那个"仿 9Pro 默认开 socket"
   * 是**错的** —— v65 的定案才是对的。真机 v75 的读数把 v64 的症状一字不差地复现：
   *
   *   sd_step=3      ← lpwork 永久卡在 recvfrom(SD_FN_RECVFROM) 里
   *   sd_polls=0     ← 铁证：`++g_sd_polls` 在 sd_recv() 之后，卡死就永远到不了
   *   sd_rok=0  sd_rnil=0  sd_errr=0  sd_recv_rc=0x0   ← 同一原因，recv 一个都没返回过
   *   sd_sends=0  sd_sendto_n=0  sd_rounds=0  sd_sent=0(dns_sent) ← 一个 DNS 包都没发出去
   *   sd_stall_ev=21（= dns_queries 21）  dns_stuck_drops=7  ign_idle=9
   *   dns_status=0xffffff83(-125)  ← 每次都被 stall 看门狗判死后中止
   *   dns_raw_le=0x72727272（114.114.114.114）dhcp_state=6 ip_configured=1 netdev_ready=1
   *   ⇒ **网络侧一切正常，唯独 DNS 发不出去**；而 ign_idle=9 证明手机**确实把回包转发
   *   回来了**，只是 phase 已被中止成 IDLE 而被拒（与 v64 的 ign_idle=8 同源）。
   *
   * 因为 sd_en=1 时 tun_dns_start_locked() 在 599 行直接 return 0（只登记 want_open），
   * `++g_dns_sent` 那条 raw 分支根本走不到 ⇒ 21 次查询 0 次发送。
   * ⇒ 恢复 v65 定案：**默认 raw，socket 仅由 cmd 14 手动开启做实验**。 */
  g_sd.enabled=0u;
  g_tun_fops.open=(void *)tun_open; g_tun_fops.close=(void *)tun_close;
  g_tun_fops.read=(void *)tun_read; g_tun_fops.write=(void *)tun_write;
  g_tun_fops.ioctl=(void *)tun_ioctl; g_tun_fops.poll=(void *)tun_poll;
  rc=REGISTER("/dev/tun",&g_tun_fops,0666,0); g_stats.register_rc=rc;
  if(rc==0) {
    g_registered=1;
    /* ★ v53：/dev/tunentry 改成 /dev/tunstat 的**别名**（原专用页已为体积闸门摘除）。
     * 表盘按钮③ 读它取 mt_stage/mt_reason=/mt_preflight_rc/mt_mm_traps/mt_hf_traps/
     * mt_mm_stub_seen/mt_skips/mt_vector_now/mt_handler/mt_shcsr_after —— 这些字段
     * stat_read 全都有（且是同一份 live 值），所以表盘侧无需任何改动。*/
    /* ★ v72：tunentry 保留 tunstat 的读（表盘侧不用改），但换成独立 fops 以便带 write。 */
    g_entry_fops.open=(void *)entry_open;   /* ★v74 open 时清残留缓冲 */
    g_entry_fops.close=(void *)stat_close; g_entry_fops.read=(void *)stat_read;
    g_entry_fops.write=(void *)entry_write;
    (void)REGISTER("/dev/tunentry",&g_entry_fops,0666,0);
    g_svc_fops.open=(void *)svc_open; g_svc_fops.close=(void *)stat_close;
    g_svc_fops.read=(void *)svc_read;
    (void)REGISTER("/dev/tunsvc",&g_svc_fops,0444,0);
  }
  /* ★ v37：自持节拍从这里起步（此后由 worker 自排程，服务调用做兜底重排）。
   * 不依赖桥、不依赖用户操作 —— DHCP 完成后 ≤1s 内就会开始排空待解析队列。*/
  tun_drain_kick();
}

/***************************************************************************
 *
 * Copyright 2015-2019 BES.
 * All rights reserved. All unpublished rights reserved.
 *
 * No part of this work may be used or reproduced in any form or by any
 * means, or stored in a database or retrieval system, without prior written
 * permission of BES.
 *
 * Use of this work is governed by a license granted by BES.
 * This work contains confidential and proprietary information of
 * BES. which is protected by copyright, trade secret,
 * trademark and other intellectual property rights.
 *
 ****************************************************************************/
#ifndef __REG_PATCH_ARMV7M_H__
#define __REG_PATCH_ARMV7M_H__

#ifdef __ARM_ARCH_7EM__

#include "plat_types.h"
#include "cmsis.h"

struct PATCH_ARMV7M_T {
    __IO uint32_t FP_CTRL;
    __IO uint32_t FP_REMAP;
    __IO uint32_t FP_COMP[512];
};

#define NUM_CODE_6_4_SHIFT                      12
#define NUM_CODE_6_4_MASK                       (0x7 << NUM_CODE_6_4_SHIFT)
#define NUM_CODE_6_4(n)                         BITFIELD_VAL(NUM_CODE_6_4, n)
#define NUM_LIT_SHIFT                           8
#define NUM_LIT_MASK                            (0xF << NUM_LIT_SHIFT)
#define NUM_LIT(n)                              BITFIELD_VAL(NUM_LIT, n)
#define NUM_CODE_3_0_SHIFT                      4
#define NUM_CODE_3_0_MASK                       (0xF << NUM_CODE_3_0_SHIFT)
#define NUM_CODE_3_0(n)                         BITFIELD_VAL(NUM_CODE_3_0, n)
#define FP_CTRL_KEY                             (1 << 1)
#define FP_CTRL_ENABLE                          (1 << 0)

#define REMAP_RMPSPT                            (1 << 29)
#define REMAP_ADDR_28_5_SHIFT                   5
#define REMAP_ADDR_28_5_MASK                    (0xFFFFFF << REMAP_ADDR_28_5_SHIFT)
#define REMAP_ADDR_28_5(n)                      BITFIELD_VAL(REMAP_ADDR_28_5, n)
#define REMAP_ADDR_SHIFT                        0
#define REMAP_ADDR_MASK                         (0x1FFFFFFF << REMAP_ADDR_SHIFT)
#define REMAP_ADDR(n)                           BITFIELD_VAL(REMAP_ADDR, n)

#define COMP_REPLACE_SHIFT                      30
#define COMP_REPLACE_MASK                       (0x3 << COMP_REPLACE_SHIFT)
#define COMP_REPLACE(n)                         BITFIELD_VAL(COMP_REPLACE, n)
#define COMP_ADDR_28_2_SHIFT                    2
#define COMP_ADDR_28_2_MASK                     (0x7FFFFFF << COMP_ADDR_28_2_SHIFT)
#define COMP_ADDR_28_2(n)                       BITFIELD_VAL(COMP_ADDR_28_2, n)
#define COMP_ADDR_SHIFT                         0
#define COMP_ADDR_MASK                          (0x1FFFFFFF << COMP_ADDR_SHIFT)
#define COMP_ADDR(n)                            BITFIELD_VAL(COMP_ADDR, n)
#define COMP_ENABLE                             (1 << 0)

#endif

#endif

/***************************************************************************
 *
 * Copyright 2015-2019 BES.
 * All rights reserved. All unpublished rights reserved.
 *
 * No part of this work may be used or reproduced in any form or by any
 * means, or stored in a database or retrieval system, without prior written
 * permission of BES.
 *
 * Use of this work is governed by a license granted by BES.
 * This work contains confidential and proprietary information of
 * BES. which is protected by copyright, trade secret,
 * trademark and other intellectual property rights.
 *
 ****************************************************************************/
#ifdef __ARM_ARCH_7EM__

#include "patch.h"
#include "reg_patch_armv7m.h"
#include "plat_addr_map.h"
#include "cmsis.h"

static struct PATCH_ARMV7M_T * const patch = (struct PATCH_ARMV7M_T *)0xE0002000;

static uint32_t patch_map[(__NUM_CODE_PATCH + __NUM_LIT_PATCH + 31) / 32];

static uint32_t get_num_code(void)
{
    return __NUM_CODE_PATCH;
}

static uint32_t get_num_lit(void)
{
    return __NUM_LIT_PATCH;
}

static uint32_t get_remap_addr(void)
{
    return GET_BITFIELD(patch->FP_REMAP, REMAP_ADDR) | (RAM_BASE & 0xE0000000);
}

int patch_open(uint32_t remap_addr)
{
    patch->FP_REMAP = REMAP_ADDR(remap_addr);
    patch->FP_CTRL = FP_CTRL_KEY | FP_CTRL_ENABLE;
    return 0;
}

int patch_code_enable_id(uint32_t id, uint32_t addr, uint32_t data)
{
    uint32_t *remap_addr;

    if (id >= get_num_code()) {
        return 1;
    }

    remap_addr = (uint32_t *)get_remap_addr();
    remap_addr[id] = data;
    patch->FP_COMP[id] = COMP_REPLACE(0) | COMP_ADDR(addr) | COMP_ENABLE;
    return 0;
}

int patch_code_disable_id(uint32_t id)
{
    if (id >= get_num_code()) {
        return 1;
    }

    patch->FP_COMP[id] = 0;
    return 0;
}

int patch_data_enable_id(uint32_t id, uint32_t addr, uint32_t data)
{
    uint32_t *remap_addr;

    if (id >= get_num_lit()) {
        return 1;
    }
    id += get_num_code();

    remap_addr = (uint32_t *)get_remap_addr();
    remap_addr[id] = data;
    patch->FP_COMP[id] = COMP_REPLACE(0) | COMP_ADDR(addr) | COMP_ENABLE;
    return 0;
}

int patch_data_disable_id(uint32_t id)
{
    if (id >= get_num_lit()) {
        return 1;
    }
    id += get_num_code();

    patch->FP_COMP[id] = 0;
    return 0;
}

static uint32_t func_patch_ins(uint32_t old_func, uint32_t new_func)
{
#ifdef __ARM_ARCH_7EM__
    union {
        uint32_t d32;
        uint16_t d16[2];
    } ins;
    uint32_t immd;
    uint8_t j1, j2, s;

    ins.d32 = 0x9000F000;
    immd = (new_func & ~1) - ((old_func + 4) & ~1);
    s = immd >> 31;
    j1 = s ^ !((immd >> 23) & 1);
    j2 = s ^ !((immd >> 22) & 1);
    ins.d16[0] |= (s << 10) | ((immd >> 12) & 0x3FF);
    ins.d16[1] |= (j1 << 13) | (j2 << 11) | ((immd >> 1) & 0x7FF);

    return ins.d32;
#else
#error "Only ARMv7-M function can be patched"
#endif
}

PATCH_ID patch_enable(enum PATCH_TYPE_T type, uint32_t addr, uint32_t data)
{
    uint8_t start, end;
    uint8_t id, idfunc;
    uint8_t cnt;
    uint32_t bit;
    uint32_t offset;

#if (ROMX_BASE != ROM_BASE)
    if (type == PATCH_TYPE_DATA) {
        // Not in rom data and code region
        if ((addr < ROM_BASE || addr >= (ROM_BASE + ROM_TOTAL_SIZE)) &&
                (addr < ROMX_BASE || addr >= (ROMX_BASE + ROM_TOTAL_SIZE))) {
            return -1;
        }
    } else
#endif
    {
        // Not in rom code region
        if (addr < ROMX_BASE || addr >= (ROMX_BASE + ROM_TOTAL_SIZE)) {
            return -1;
        }
    }
    if (type == PATCH_TYPE_FUNC) {
        if (data >= addr) {
            offset = data - addr;
        } else {
            offset = addr - data;
        }
        if (offset >= 0x01000000) {
            // Branch distance too long to fit in a 32-bit branch instruction
            return -2;
        }
    }

    if (type == PATCH_TYPE_CODE || type == PATCH_TYPE_FUNC) {
        start = 0;
        end = start + get_num_code();
    } else if (type == PATCH_TYPE_DATA) {
        start = get_num_code();
        end = start + get_num_lit();
    } else {
        return -3;
    }

    for (id = start; id < end; id++) {
        cnt = id / 32;
        bit = (1 << (id % 32));
        if ((patch_map[cnt] & bit) == 0) {
            patch_map[cnt] |= bit;
            break;
        }
    }
    if (id >= end) {
        return -4;
    }

    idfunc = 0xFF;

    if (type == PATCH_TYPE_FUNC && (addr & 2)) {
        for (idfunc = id; idfunc < end; idfunc++) {
            cnt = idfunc / 32;
            bit = (1 << (idfunc % 32));
            if ((patch_map[cnt] & bit) == 0) {
                patch_map[cnt] |= bit;
                break;
            }
        }
        if (idfunc >= end) {
            // Release previous patch
            patch_map[cnt] &= ~bit;
            return -5;
        }
    }

    if (type == PATCH_TYPE_CODE) {
        patch_code_enable_id(id - start, addr, data);
    } else if (type == PATCH_TYPE_DATA) {
        patch_data_enable_id(id - start, addr, data);
    } else if (type == PATCH_TYPE_FUNC) {
        uint32_t ins;

        ins = func_patch_ins(addr, data);

        if (addr & 2) {
            uint32_t real_addr[2];
            uint32_t real_data[2];

            real_addr[0] = addr & ~3;
            real_addr[1] = (addr + 4) & ~3;

            real_data[0] = *(volatile uint32_t *)real_addr[0];
            real_data[1] = *(volatile uint32_t *)real_addr[1];
            real_data[0] = (real_data[0] & 0x0000FFFF) | ((ins & 0xFFFF) << 16);
            real_data[1] = (real_data[1] & 0xFFFF0000) | ((ins >> 16) & 0xFFFF);

            patch_code_enable_id(id - start, real_addr[0], real_data[0]);
            patch_code_enable_id(idfunc - start, real_addr[1], real_data[1]);
        } else {
            patch_code_enable_id(id - start, addr, ins);
        }
    }

    return ((id + 1) & 0xFF) | (((idfunc + 1) & 0xFF) << 8);
}

int patch_disable(PATCH_ID patch_id)
{
    uint8_t id;
    uint8_t code_start, code_end, data_start, data_end;
    uint8_t cnt, bit;
    int i;

    code_start = 0;
    code_end = code_start + get_num_code();
    data_start = code_end;
    data_end = data_start + get_num_lit();

    for (i = 0; i < 2; i++) {
        id = (uint8_t)(patch_id >> (8 * i)) - 1;

        if (id == 0xFF) {
            return 0;
        } else if (id >= code_start && id < code_end) {
            patch_code_disable_id(id);
        } else if (id >= data_start && id < data_end) {
            patch_data_disable_id(id - data_start);
        } else {
            return -1;
        }

        cnt = id / 32;
        bit = (1 << (id % 32));
        patch_map[cnt] &= ~bit;
    }

    return 0;
}

void patch_close(void)
{
    int i;

    patch->FP_CTRL = FP_CTRL_KEY;

    for (i = 0; i < get_num_code(); i++) {
        patch_code_disable_id(i);
    }

    for (i = get_num_code(); i < (get_num_code() + get_num_lit()); i++) {
        patch_data_disable_id(i);
    }

    for (i = 0; i < ARRAY_SIZE(patch_map); i++) {
        patch_map[i] = 0;
    }
}

#endif

