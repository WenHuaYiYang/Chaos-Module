/*
 * Chaos integration for Canopus-Module-Corona's complete p67 background blur.
 * AGPL-3.0-only; https://github.com/AstralSightStudios/Canopus-Module-Corona
 * See corona_vendor/LICENSE and licenses/CANOPUS-MODULE-CORONA-NOTICE.txt.
 *
 * Adapted for Chaos on 2026-11-03. The six upstream p67 implementations are
 * included verbatim, including all four draw/scroll/delete hooks and the
 * control-center battery/edit-container handling. Only the host services and
 * the Rust ABI below are Chaos code. Exact target: p67 / 3.101.043.
 */
#include <stdint.h>
#include <stddef.h>

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

/* Sole implementation entry: includes launcher -> native provider/state
 * machine, then all three panels and the control-center battery hooks. */
#include "corona_vendor/src/p67_backgrounds.inc"

static unsigned chaos_corona_enabled, chaos_corona_in_call;

int chaos_corona_backgrounds_idle(void) {
    unsigned i;
    if (chaos_corona_in_call || p67_busy || load32(P67_IMAGE_SLOT) ||
        P67_U8(0x201102d9u) || !p67_blur_native_idle()) return 0;
    for (i = 0; i < 3u; ++i)
        if (!p67_surface_idle(&p67_surfaces[i])) return 0;
    return 1;
}

int chaos_corona_backgrounds_commit(uint32_t enabled) {
    static const struct rh_backgrounds blur = {
        .launcher = RH_BG_BLUR, .control_center = RH_BG_BLUR,
        .notifications = RH_BG_BLUR, .widgets = RH_BG_BLUR
    };
    static const struct rh_backgrounds system = {0};
    int result;
    if (chaos_corona_in_call || !p67_blur_native_idle()) return -2145;
    chaos_corona_in_call = 1;
    result = rh_platform_backgrounds_commit(enabled ? &blur : &system);
    if (!result) chaos_corona_enabled = !!enabled;
    chaos_corona_in_call = 0;
    return result;
}

void chaos_corona_backgrounds_poll(void) {
    if (chaos_corona_in_call) return;
    chaos_corona_in_call = 1;
    rh_platform_backgrounds_poll();
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

