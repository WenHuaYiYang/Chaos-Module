/* p67tc runtime FPBv2 write-path probe. No resolver hook or flash writes.
 * Loaded modules leave this idle. Explicit ioctl/read triggers the probe.
 * Only COMP7 is ever written, always with comparator ENABLE bit clear. */
#ifndef P67TC_FPB_PROBE_H
#define P67TC_FPB_PROBE_H
#define FPB_PROBE_COMP7 (ENTRY_FPB_COMP0+7u*4u)
enum fpb_probe_reason {
  FPB_PROBE_IDLE=0, FPB_PROBE_OK=1, FPB_PROBE_CPUID=2,
  FPB_PROBE_CTRL=3, FPB_PROBE_LOCKED=4, FPB_PROBE_BUSY=5,
  FPB_PROBE_MARKER=6, FPB_PROBE_WRITE_MISMATCH=7,
  FPB_PROBE_CLEAR_MISMATCH=8, FPB_PROBE_RESTORE_MISMATCH=9,
  FPB_PROBE_CTRL_CHANGED=10, FPB_PROBE_LSR_UNKNOWN=11
};
struct fpb_probe_state {
  uint32_t runs,reason,rc,cpuid,ctrl_before,ctrl_after,lsr,lar;
  uint32_t comp_before,target,comp_test,comp_clear,comp_after;
  uint32_t wrote,restored,irq_mask,register_rc;
};
static struct fpb_probe_state g_fpb_probe;
static volatile uint32_t g_fpb_probe_marker __attribute__((aligned(4)))=0x67104313u;
static void fpb_probe_barrier(void) {
  __asm__ volatile("dmb sy\n\tdsb sy\n\tisb sy" ::: "memory");
}
static uint32_t fpb_probe_irq_save(void) {
  uint32_t saved;
  __asm__ volatile("mrs %0, primask\n\tcpsid i" : "=r"(saved) :: "memory");
  return saved;
}
static void fpb_probe_irq_restore(uint32_t saved) {
  __asm__ volatile("msr primask, %0" :: "r"(saved) : "memory");
}
static uint32_t fpb_probe_read(uint32_t address) {
  return *(volatile uint32_t *)(uintptr_t)address;
}
static void fpb_probe_write_comp7(uint32_t value) {
  *(volatile uint32_t *)(uintptr_t)FPB_PROBE_COMP7=value;
  fpb_probe_barrier();
}
static __attribute__((unused)) const char *fpb_probe_reason_text(uint32_t reason) {
  switch(reason) {
    case FPB_PROBE_OK:return "passed_disabled_comp7";
    case FPB_PROBE_CPUID:return "cpuid_unverified";
    case FPB_PROBE_CTRL:return "fpb_disabled_v2_unverified";
    case FPB_PROBE_LOCKED:return "fpb_locked";
    case FPB_PROBE_BUSY:return "comp7_not_idle";
    case FPB_PROBE_MARKER:return "marker_address_unverified";
    case FPB_PROBE_WRITE_MISMATCH:return "test_readback_mismatch";
    case FPB_PROBE_CLEAR_MISMATCH:return "clear_readback_mismatch";
    case FPB_PROBE_RESTORE_MISMATCH:return "restore_readback_mismatch";
    case FPB_PROBE_CTRL_CHANGED:return "ctrl_changed";
    case FPB_PROBE_LSR_UNKNOWN:return "lock_state_unknown";
    default:return "not_run";
  }
}
static int fpb_probe_run(void) {
  uint32_t saved=fpb_probe_irq_save();
  int rc=-95;
  ++g_fpb_probe.runs;
  g_fpb_probe.irq_mask=saved;
  g_fpb_probe.reason=FPB_PROBE_IDLE;
  g_fpb_probe.wrote=0u; g_fpb_probe.restored=0u;
  g_fpb_probe.comp_test=0u; g_fpb_probe.comp_clear=0u;
  g_fpb_probe.cpuid=fpb_probe_read(ENTRY_CPUID_ADDR);
  g_fpb_probe.ctrl_before=fpb_probe_read(ENTRY_FPB_CTRL);
  g_fpb_probe.lsr=fpb_probe_read(ENTRY_FPB_LSR);
  g_fpb_probe.lar=fpb_probe_read(ENTRY_FPB_LAR);
  g_fpb_probe.comp_before=fpb_probe_read(FPB_PROBE_COMP7);
  g_fpb_probe.target=(uint32_t)(uintptr_t)&g_fpb_probe_marker;
  g_fpb_probe.target&=~1u;
  if(!entry_cpuid_ok(g_fpb_probe.cpuid)) {
    g_fpb_probe.reason=FPB_PROBE_CPUID; goto cleanup;
  }
  if(!entry_fpb_ok(g_fpb_probe.ctrl_before) ||
     entry_fpb_num_code(g_fpb_probe.ctrl_before)<8u ||
     (g_fpb_probe.ctrl_before&1u)!=0u) {
    g_fpb_probe.reason=FPB_PROBE_CTRL; goto cleanup;
  }
  /* LSR: SLI bit0 indicates lock implementation, SLK bit1 its state.
   * An all-ones read or SLK=1 without SLI is unknown; never unlock. */
  if(g_fpb_probe.lsr==0xffffffffu || (g_fpb_probe.lsr&3u)==2u) {
    g_fpb_probe.reason=FPB_PROBE_LSR_UNKNOWN; goto cleanup;
  }
  if((g_fpb_probe.lsr&3u)==3u) {
    g_fpb_probe.reason=FPB_PROBE_LOCKED; goto cleanup;
  }
  if(g_fpb_probe.comp_before!=0u) {
    g_fpb_probe.reason=FPB_PROBE_BUSY; goto cleanup;
  }
  if(g_fpb_probe.target<0x20000000u || g_fpb_probe.target>=0x40000000u ||
     (g_fpb_probe.target&3u)!=0u) {
    g_fpb_probe.reason=FPB_PROBE_MARKER; goto cleanup;
  }
  /* Interrupts stay masked for this bounded MMIO transaction. Both global
   * ENABLE and comparator ENABLE remain clear; only COMP7 is touched. */
  g_fpb_probe.wrote=1u;
  fpb_probe_write_comp7(g_fpb_probe.target);
  g_fpb_probe.comp_test=fpb_probe_read(FPB_PROBE_COMP7);
  if(g_fpb_probe.comp_test!=g_fpb_probe.target) {
    g_fpb_probe.reason=FPB_PROBE_WRITE_MISMATCH; rc=-5; goto cleanup;
  }
  fpb_probe_write_comp7(0u);
  g_fpb_probe.comp_clear=fpb_probe_read(FPB_PROBE_COMP7);
  if(g_fpb_probe.comp_clear!=0u) {
    g_fpb_probe.reason=FPB_PROBE_CLEAR_MISMATCH; rc=-5; goto cleanup;
  }
  g_fpb_probe.reason=FPB_PROBE_OK; rc=0;
cleanup:
  if(g_fpb_probe.wrote) {
    /* Uniform cleanup even after write/clear mismatch. The saved comparator
     * has already been proven zero, so no breakpoint can be enabled. */
    fpb_probe_write_comp7(0u);
    fpb_probe_write_comp7(g_fpb_probe.comp_before);
  }
  g_fpb_probe.comp_after=fpb_probe_read(FPB_PROBE_COMP7);
  g_fpb_probe.ctrl_after=fpb_probe_read(ENTRY_FPB_CTRL);
  if(g_fpb_probe.wrote) {
    g_fpb_probe.restored=(g_fpb_probe.comp_after==g_fpb_probe.comp_before);
    if(!g_fpb_probe.restored) {g_fpb_probe.reason=FPB_PROBE_RESTORE_MISMATCH;rc=-5;}
    if(g_fpb_probe.ctrl_after!=g_fpb_probe.ctrl_before) {
      g_fpb_probe.reason=FPB_PROBE_CTRL_CHANGED;rc=-5;
    }
  }
  g_fpb_probe.rc=(uint32_t)rc;
  fpb_probe_barrier();
  fpb_probe_irq_restore(saved);
  return rc;
}
#endif


