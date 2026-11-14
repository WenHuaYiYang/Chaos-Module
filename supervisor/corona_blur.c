/*
 * Chaos integration for Canopus-Module-Corona's complete p67 background blur.
 * AGPL-3.0-only; https://github.com/AstralSightStudios/Canopus-Module-Corona
 * See corona_vendor/LICENSE and licenses/CANOPUS-MODULE-CORONA-NOTICE.txt.
 *
 * Adapted for Chaos on 2026-11-03. The six upstream p67 implementations are
 * included verbatim, including all four draw/scroll/delete hooks and the
 * control-center battery/edit-container handling. Host integration also routes
 * the filter call to SysPatch IIR when V2 is selected. Two provider/draw files
 * have gated V2 8-bit color/full-resolution output extensions.
 * Exact target: p67 / 3.101.043.
 */
#include <stdint.h>
#include <stddef.h>
#include "syspatch_blur/chaos_filter.h"
#define CHAOS_BLUR_V2_COLOR 1
#define rh_blur_filter_rgb565 chaos_blur_filter

#define RH_TARGET_1043 1
#include "corona_vendor/include/resource_hook_target.h"
#include "corona_vendor/include/resource_hook_background.h"
#define RH_RELOAD_LIMIT 1024u
#define RH_FW_DISPLAY_ACTIVE_SCREEN_OFFSET 696u
#define RH_FILE_READ 1
#define RH_FILE_REPLACE 2

static uint32_t load32(uint32_t address) {
    return *(volatile uint32_t *)(uintptr_t)address;
}

/* The only configurations exposed to Rust are :blur and system (empty).
 * File backgrounds and mapping-cache retirement are deliberately unavailable:
 * p67_stage_path short-circuits both supported values before these seams. */
static int cache_valid(uint32_t cache, uint32_t clz) {
    (void)cache; (void)clz; return 0;
}
static int retire_cache(uint32_t cache, uint32_t offset,
                        const struct rh_mapping_view *mapping, const char *path) {
    (void)cache; (void)offset; (void)mapping; (void)path; return -1;
}

/* Preserve Corona's bounded battery diagnostics. This adapter opens only its
 * exact generated diagnostic file, never an arbitrary user-supplied pathname.
 * .043 has no verified O_TRUNC flag: replace by remove + create, as Canopus's
 * native FS service does. No dependency on that service's private handles. */
static int rh_platform_open(const char *path, int mode) {
    static const char diagnostic[] = "/data/offlinelog/resource-hook-p67-battery.log";
    unsigned i;
    int fd;
    if (!path || mode != RH_FILE_REPLACE) return -1;
    for (i = 0; i < sizeof(diagnostic); ++i)
        if (path[i] != diagnostic[i]) return -1;
    if (((int (*)(const char *))(uintptr_t)0x0c1eabe1u)(diagnostic) != 0) {
        fd = ((int (*)(const char *, uint32_t, uint32_t))(uintptr_t)0x0c1d0a29u)
            (diagnostic, 1u, 0u);
        if (fd >= 0) {
            ((int (*)(int))(uintptr_t)0x0c1b9d81u)(fd);
            return -1;
        }
    }
    return ((int (*)(const char *, uint32_t, uint32_t))(uintptr_t)0x0c1d0a29u)
        (diagnostic, 2u | 4u, 0666u);
}
static int rh_platform_write(int fd, const void *data, uint32_t size) {
    return ((int (*)(int, const void *, uint32_t))(uintptr_t)0x0c1d2641u)(fd, data, size);
}
static int rh_platform_read(int fd, void *data, uint32_t size) {
    return ((int (*)(int, void *, uint32_t))(uintptr_t)0x0c1d129du)(fd, data, size);
}
static void rh_platform_close(int fd) {
    (void)((int (*)(int))(uintptr_t)0x0c1b9d81u)(fd);
}

/* Verbatim redraw functions from upstream src/platform.c (1013-1043).
 * They request a native frame and confirm that a full-screen dirty area was
 * retained. No nullable callback and no synchronous render from a UI poll. */
int rh_platform_redraw_ready(void) {
    uint32_t disp = *(volatile uint32_t *)(uintptr_t)RH_FW_DISPLAY_SLOT;
    if (!disp || !*(volatile uint32_t *)(uintptr_t)(disp + RH_FW_DISPLAY_ACTIVE_SCREEN_OFFSET)) return 0;
    if (*(volatile unsigned char *)(uintptr_t)(disp + 58u) & 2u) return 0;
    return *(volatile int32_t *)(uintptr_t)(disp + 608u) > 0;
}
/* Request, rather than synchronously execute, a full redraw. Return success
 * only if the firmware retained a dirty area covering the entire display. */
int rh_platform_request_full_redraw(void) {
    int32_t area[4] = {0, 0, 0x7fff, 0x7fff};
    uint32_t disp = *(volatile uint32_t *)(uintptr_t)RH_FW_DISPLAY_SLOT;
    int32_t width, height;
    uint32_t i, count;
    if (!rh_platform_redraw_ready()) return -1;
    ((void (*)(uint32_t, const void *))(uintptr_t)RH_FW_INVALIDATE_AREA)(disp, area);
    /* Ask the firmware for the resolution rather than reading disp+0/disp+4:
     * which of those two is horizontal depends on the rotation bit at disp+756,
     * and _lv_inv_area clips with these same accessors. Using them keeps this
     * check identical to the firmware's own clip at any rotation. */
    width = ((int32_t (*)(uint32_t))(uintptr_t)RH_FW_DISPLAY_WIDTH)(disp);
    height = ((int32_t (*)(uint32_t))(uintptr_t)RH_FW_DISPLAY_HEIGHT)(disp);
    count = *(volatile uint32_t *)(uintptr_t)(disp + 604u);
    if (count > 32u || width <= 0 || height <= 0) return -1;
    for (i = 0; i < count; i++) {
        volatile int32_t *a = (volatile int32_t *)(uintptr_t)(disp + 60u + i * 16u);
        if (a[0] <= 0 && a[1] <= 0 && a[2] >= width - 1 && a[3] >= height - 1)
            return 0;
    }
    return -1;
}

/* Keep the upstream implementation intact. Its fallback position dispatch
 * cooperates with honeycomb before the native screenshot path executes. */
extern int chaos_honeycomb_pre_scroll(void *event);
#ifndef CHAOS_CORONA_NATIVE_POSITION
#define CHAOS_CORONA_NATIVE_POSITION ((void (*)(void *))(uintptr_t)0x0c517d9du)
#endif
static void chaos_corona_launcher_position(void *event) {
    if (!chaos_honeycomb_pre_scroll(event))
        CHAOS_CORONA_NATIVE_POSITION(event);
}
#define P67_NATIVE(address) ((address) == 0x0c517d9du ? \
    (uintptr_t)chaos_corona_launcher_position : (uintptr_t)(address))

/* Sole implementation entry: includes launcher -> native provider/state
 * machine, then all three panels and the control-center battery hooks. */
#include "corona_vendor/src/p67_backgrounds.inc"
#include "corona_launcher_rebind.inc"

uint32_t rh_platform_launcher_scroll_callback(void) {
    return (uint32_t)(uintptr_t)p67_scroll;
}

static unsigned chaos_corona_enabled, chaos_corona_in_call;
/* 0x201102D9u 是固件 launcher 的“滚动/手势进行中”标记：手势开始时被置 1，
 * 由固件自己的滚动结束回调清零。手势被打断（例如滑动途中切走应用）时它可能
 * 一直保持 1，于是上游 poll 在门口就 return，背景钩子再也装不回去 —— 现象就是
 * 模糊“莫名其妙失效”，重启才恢复。这里只做最后一道自愈：把这条标记以外的
 * 一切都确认空闲（native 渲染/层任务空闲、没有 native 快照、上游不忙）并持续
 * 60 次调用后才清零，避免打断真实手势。 */
static unsigned chaos_corona_latch_calls;
static uint32_t chaos_corona_latch_clears, chaos_corona_commits, chaos_corona_commit_fails;

static void chaos_corona_latch_rescue(void) {
    if (!P67_U8(0x201102d9u)) { chaos_corona_latch_calls = 0; return; }
    if (p67_busy || load32(P67_IMAGE_SLOT) || !p67_blur_native_idle()) {
        chaos_corona_latch_calls = 0;
        return;
    }
    if (++chaos_corona_latch_calls > 60u) {
        P67_U8(0x201102d9u) = 0;
        chaos_corona_latch_calls = 0;
        ++chaos_corona_latch_clears;
    }
}

int chaos_corona_backgrounds_idle(void) {
    unsigned i;
    if (chaos_corona_in_call || p67_busy || load32(P67_IMAGE_SLOT) ||
        P67_U8(0x201102d9u) || !p67_blur_native_idle()) return 0;
    for (i = 0; i < 3u; ++i)
        if (!p67_surface_idle(&p67_surfaces[i])) return 0;
    return 1;
}

int chaos_blur_select_algorithm(unsigned v2) {
    v2=!!v2;
    if (chaos_v2_algorithm==v2) return 0;
    if (!chaos_corona_backgrounds_idle()) return -2145;
    chaos_v2_algorithm=v2;
    p67_blur_drop();
    p67_blur_reset();
    return 0;
}

/* Local launcher glass needs only renderer quiescence, not a Corona region
 * configuration or panel ownership. Reuse the audited balanced GPU fence. */
uint32_t chaos_s541_frame_idle(void) {
    if (chaos_corona_in_call || p67_busy || !p67_blur_native_idle()) return 0;
    return p67_blur_native_fence() && p67_blur_native_idle();
}

/* Ownership watchdog. The native owners are torn down by their own DELETE /
 * gesture handling; when that happens without a matching commit the module
 * would keep believing it still owns the scene and stop painting (blur gone,
 * root bg_opa left at 0 => everything looks transparent). Rust polls this and
 * re-commits when it flips to 0. */
uint32_t chaos_corona_backgrounds_owned(void) {
    unsigned i;
    if (!chaos_corona_enabled || chaos_corona_in_call) return 1u;
    if (rh_background_is_blur(p67_path[p67_path_index])) {
        if (!p67_owner || p67_draw_owner != load32(P67_ROOT_SLOT)) return 0u;
        /* No pixels yet and nothing being captured => nothing to paint. */
        if (!p67_blur_source && !p67_blur_pending) return 0u;
    }
    for (i = 0; i < 3u; ++i) {
        struct p67_surface *s = &p67_surfaces[i];
        if (rh_background_is_blur(p67_surface_path(s)) && !s->owner) return 0u;
    }
    return 1u;
}

int chaos_corona_backgrounds_commit(uint32_t enabled, uint32_t region_mask) {
    /* The upstream commit has its own RH_PATH-sized staging object. Keep
     * this additional 1 KiB off the UI stack, protected for the entire call.
     * Upstream copies each path synchronously and retains none of these
     * pointers after the commit returns. */
    static struct rh_backgrounds backgrounds;
    const char *blur = RH_BG_BLUR;
    unsigned i, j;
    int result;
    if (chaos_corona_in_call) return -2145;
    chaos_corona_latch_rescue();
    if (!p67_blur_native_idle()) return -2145;
    chaos_corona_in_call = 1;
    /* bit0 launcher, bit1 control-center, bit2 widgets, bit3 notifications.
     * Every unselected surface is explicitly the native system background. */
    for (i = 0; i < RH_PATH; ++i) {
        backgrounds.launcher[i] = 0;
        backgrounds.control_center[i] = 0;
        backgrounds.notifications[i] = 0;
        backgrounds.widgets[i] = 0;
    }
    region_mask &= 0x0fu;
    if (enabled) {
        if (region_mask & 1u)
            for (j = 0; j < RH_PATH; ++j) { backgrounds.launcher[j] = blur[j]; if (!blur[j]) break; }
        if (region_mask & 2u)
            for (j = 0; j < RH_PATH; ++j) { backgrounds.control_center[j] = blur[j]; if (!blur[j]) break; }
        if (region_mask & 8u)
            for (j = 0; j < RH_PATH; ++j) { backgrounds.notifications[j] = blur[j]; if (!blur[j]) break; }
        if (region_mask & 4u)
            for (j = 0; j < RH_PATH; ++j) { backgrounds.widgets[j] = blur[j]; if (!blur[j]) break; }
    }
    result = rh_platform_backgrounds_commit(&backgrounds);
    ++chaos_corona_commits;
    if (result) ++chaos_corona_commit_fails;
    else chaos_corona_enabled = !!enabled;
    chaos_corona_in_call = 0;
    return result;
}

void chaos_corona_backgrounds_poll(void) {
    if (chaos_corona_in_call) return;
    /* 自愈也挂在 poll 上：标记卡住时上游 poll 会在门口直接 return，
     * 而 commit 又要求 idle —— 只有 poll 是每拍都跑的那条路。 */
    chaos_corona_latch_rescue();
    chaos_corona_in_call = 1;
    chaos_corona_launcher_rebind();
    rh_platform_backgrounds_poll();
    chaos_corona_rearm_missing_source();
    chaos_corona_in_call = 0;
}

/* Pure scalar observation, used after commit(false). Never fence/free here,
 * and never count historical epochs/counters as pending cleanup. Rust keeps
 * polling the upstream owner until both subscriptions and buffers are gone. */
uint32_t chaos_corona_backgrounds_pending(void) {
    unsigned i;
    if (chaos_corona_in_call || p67_busy || p67_blur_busy || p67_blur_polling ||
        p67_blur_native_capturing || p67_blur_native_power_depth ||
        p67_blur_native_outputs[0].retired || p67_blur_native_outputs[1].retired ||
        p67_blur_native_input || p67_blur_cleanup_pending() || p67_battery_busy) return 1u;
    if (!chaos_corona_enabled &&
        (p67_owner || p67_page || p67_draw_owner ||
         p67_blur_source || p67_blur_retired || p67_blur_display ||
         p67_blur_root || p67_blur_root_registered || p67_blur_pending ||
         p67_blur_native_outputs[0].descriptor || p67_blur_native_outputs[1].descriptor ||
         p67_battery_obj || p67_battery_root || p67_battery_status ||
         p67_battery_buffer || p67_battery_mask_buffer || p67_battery_mask_root ||
         p67_cc_edit_neutralized)) return 1u;
    for (i = 0; i < 3u; ++i) {
        const struct p67_surface *s = &p67_surfaces[i];
        if (s->busy || (!chaos_corona_enabled &&
            (s->owner || s->page || s->root || s->list || s->swapped ||
             s->moving || s->dirty))) return 1u;
    }
    return 0u;
}

/* ===== 只读诊断文本（调试页用） =====
 * 纯整数读取：不调用 native、不改状态、不做 I/O。失败返回 0 且不写 out。 */
struct chaos_text { char *out; uint32_t used, capacity; unsigned failed; };
static void chaos_text_s(struct chaos_text *w, const char *text) {
    while (*text) {
        if (w->used + 1u >= w->capacity) { w->failed = 1; return; }
        w->out[w->used++] = *text++;
    }
}
static void chaos_text_u(struct chaos_text *w, uint32_t value) {
    char digits[10]; unsigned n = 0;
    do { digits[n++] = (char)('0' + value % 10u); value /= 10u; } while (value);
    while (n) {
        if (w->used + 1u >= w->capacity) { w->failed = 1; return; }
        w->out[w->used++] = digits[--n];
    }
}
static void chaos_text_kv(struct chaos_text *w, const char *key, uint32_t value) {
    chaos_text_s(w, key);
    chaos_text_s(w, "=");
    chaos_text_u(w, value);
    chaos_text_s(w, "\n");
}
uint32_t chaos_corona_diagnostics(char *out, uint32_t capacity) {
    struct chaos_text w = {out, 0, capacity, 0};
    unsigned i, surfaces = 0;
    if (!out || capacity < 2u) return 0u;
    chaos_text_kv(&w, "enabled", chaos_corona_enabled);
    chaos_text_kv(&w, "algorithm", chaos_v2_algorithm);
    chaos_text_kv(&w, "owned", chaos_corona_backgrounds_owned());
    chaos_text_kv(&w, "owner", p67_owner);
    chaos_text_kv(&w, "draw", p67_draw_owner == load32(P67_ROOT_SLOT) ? 1u : 0u);
    chaos_text_kv(&w, "imgslot", load32(P67_IMAGE_SLOT) ? 1u : 0u);
    chaos_text_kv(&w, "latch", P67_U8(0x201102d9u) ? 1u : 0u);
    chaos_text_kv(&w, "src", p67_blur_source ? 1u : 0u);
    chaos_text_kv(&w, "pend", p67_blur_pending);
    chaos_text_kv(&w, "arm", p67_blur_armed);
    chaos_text_kv(&w, "entry_scoped", p67_blur_entry_scoped);
    chaos_text_kv(&w, "entry_allowed", p67_blur_allowed_entries);
    chaos_text_kv(&w, "source_rearms", chaos_corona_source_rearms);
    chaos_text_kv(&w, "gate", p67_blur_core_gate);
    chaos_text_kv(&w, "try", p67_blur_attempts);
    chaos_text_kv(&w, "copy", p67_blur_copies);
    chaos_text_kv(&w, "pub", p67_blur_publishes);
    chaos_text_kv(&w, "draws", p67_blur_draws);
    for (i = 0; i < 3u; ++i) surfaces |= p67_surfaces[i].owner ? (1u << i) : 0u;
    chaos_text_kv(&w, "surf", surfaces);
    chaos_text_kv(&w, "commit", chaos_corona_commits);
    chaos_text_kv(&w, "cfail", chaos_corona_commit_fails);
    chaos_text_kv(&w, "latchclr", chaos_corona_latch_clears);
    chaos_text_kv(&w, "natidle", (uint32_t)p67_blur_native_idle());
    chaos_text_kv(&w, "idle_error", p67_blur_native_idle_error);
    chaos_text_kv(&w, "native_error", p67_blur_native_error);
    chaos_text_kv(&w, "render_bits", p67_blur_native_render_bits);
    chaos_text_kv(&w, "wf_slot", p67_blur_native_root_seen);
    if (w.failed) return 0u;
    out[w.used] = 0;
    return w.used;
}

