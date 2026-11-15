/* Host tests for Chaos's actual C ABI wrapper bodies, not the upstream hooks. */
#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "resource_hook_background.h"

static unsigned chaos_corona_enabled, chaos_corona_in_call;
static uint32_t chaos_corona_idle_wait;
static int idle = 1, commit_result;
static unsigned expected_enabled, expected_mask;
static unsigned commits, polls;
static unsigned reenter_commit, reenter_poll, reentry_checks;

int chaos_corona_backgrounds_commit(uint32_t enabled, uint32_t region_mask);
void chaos_corona_backgrounds_poll(void);
static unsigned honeycomb_consumes, position_forwards;
static int chaos_honeycomb_pre_scroll(void *event) { assert(event); return honeycomb_consumes; }
static void native_position(void *event) { assert(event); ++position_forwards; }
#define CHAOS_CORONA_NATIVE_POSITION native_position

static unsigned native_idle_queries, surface_idle_queries;
static int p67_busy;
static uint32_t image_slot;
static unsigned char scroll_latch;
struct surface_stub { unsigned idle; };
static struct surface_stub p67_surfaces[3] = {{1}, {1}, {1}};
#define P67_IMAGE_SLOT 0x201102dcu
#define P67_U8(address) test_byte(address)
static unsigned char test_byte(uint32_t address) { assert(address == 0x201102d9u); return scroll_latch; }
static uint32_t load32(uint32_t address) { assert(address == P67_IMAGE_SLOT); return image_slot; }
static int p67_blur_native_idle(void) { ++native_idle_queries; return idle; }
static int p67_surface_idle(struct surface_stub *surface) { ++surface_idle_queries; return surface->idle; }
/* 生产实现里由自愈/计数器引用的外部件，这里打桩（不含任何 native 读）。 */
static uint32_t chaos_corona_commits, chaos_corona_commit_fails;
static unsigned latch_rescues;
static void chaos_corona_latch_rescue(void) { ++latch_rescues; }
static unsigned display_repairs;
static void chaos_corona_display_rebind(void) {
    assert(chaos_corona_in_call == 1 && display_repairs == polls);
    ++display_repairs;
}
/* Callback integrity is exercised against real p67 objects in launcher harness. */
static void chaos_corona_launcher_rebind(void) {}
static void chaos_corona_rearm_missing_source(void) {}
static void cc_buttons_poll(void) {}

static int rh_platform_backgrounds_commit(const struct rh_backgrounds *bg) {
    const char *paths[] = {bg->launcher, bg->control_center, bg->notifications, bg->widgets};
    const unsigned bits[] = {1u, 2u, 8u, 4u};
    assert(chaos_corona_in_call == 1);
    for (unsigned i = 0; i < 4; ++i) {
        const char *expected = expected_enabled && (expected_mask & bits[i]) ? RH_BG_BLUR : "";
        assert(!strcmp(paths[i], expected));
    }
    ++commits;
    if (reenter_commit) {
        /* A nested call must reject before touching the shared scratch. */
        struct rh_backgrounds before = *bg;
        unsigned saved_commits = commits, saved_polls = polls;
        assert(chaos_corona_backgrounds_commit(0, 0xfu) == -2145);
        chaos_corona_backgrounds_poll();
        assert(commits == saved_commits && polls == saved_polls);
        assert(chaos_corona_in_call == 1 && !memcmp(&before, bg, sizeof(before)));
        ++reentry_checks;
    }
    return commit_result;
}

static void rh_platform_backgrounds_poll(void) {
    assert(chaos_corona_in_call == 1);
    assert(display_repairs == polls + 1);
    ++polls;
    if (reenter_poll) {
        unsigned saved_commits = commits, saved_polls = polls;
        assert(chaos_corona_backgrounds_commit(1, 4u) == -2145);
        chaos_corona_backgrounds_poll();
        assert(commits == saved_commits && polls == saved_polls);
        assert(chaos_corona_in_call == 1);
        ++reentry_checks;
    }
}

#include CHAOS_CORONA_ABI_SOURCE

int main(void) {
    unsigned mask;

    /* Every one of the 16 region masks must map independently to blur/system. */
    expected_enabled = 1;
    commit_result = 0;
    for (mask = 0; mask < 16u; ++mask) {
        expected_mask = mask;
        assert(!chaos_corona_backgrounds_commit(1, mask));
        assert(chaos_corona_enabled == 1 && !chaos_corona_in_call);
    }
    assert(commits == 16);

    /* High bits are ignored, while the low four bits retain the region ABI. */
    expected_mask = 0;
    assert(!chaos_corona_backgrounds_commit(1, 0xf0u));
    assert(commits == 17 && chaos_corona_enabled == 1);

    /* A failed disable rolls back the enabled state and propagates busy. */
    expected_enabled = 0; expected_mask = 0xfu; commit_result = -2145;
    assert(chaos_corona_backgrounds_commit(0, 0xfu) == -2145);
    assert(commits == 18 && chaos_corona_enabled == 1 && !chaos_corona_in_call);
    commit_result = 0;
    assert(!chaos_corona_backgrounds_commit(0, 0xfu));
    assert(commits == 19 && !chaos_corona_enabled && !chaos_corona_in_call);

    /* Native idle and re-entry guards must prevent a downstream commit/poll. */
    idle = 0;
    assert(chaos_corona_backgrounds_commit(1, 1u) == -2145);
    assert(commits == 19 && !chaos_corona_enabled);
    idle = 1;
    chaos_corona_in_call = 1;
    assert(chaos_corona_backgrounds_commit(1, 1u) == -2145);
    chaos_corona_backgrounds_poll();
    assert(commits == 19 && polls == 0 && chaos_corona_in_call == 1);
    chaos_corona_in_call = 0;

    /* Downstream errors do not publish a new enabled state. */
    expected_enabled = 1; expected_mask = 9u; commit_result = -2143;
    assert(chaos_corona_backgrounds_commit(1, 9u) == -2143);
    assert(commits == 20 && !chaos_corona_enabled && !chaos_corona_in_call);
    commit_result = 0;
    assert(!chaos_corona_backgrounds_commit(1, 9u));
    assert(commits == 21 && chaos_corona_enabled == 1 && !chaos_corona_in_call);

    /* Exercise re-entry from the downstream calls, not only a preset guard. */
    reenter_commit = 1;
    expected_mask = 4u;
    assert(!chaos_corona_backgrounds_commit(1, 4u));
    assert(commits == 22 && reentry_checks == 1 && !chaos_corona_in_call);
    reenter_commit = 0;
    reenter_poll = 1;
    chaos_corona_backgrounds_poll();
    assert(polls == 1 && reentry_checks == 2 && !chaos_corona_in_call);
    reenter_poll = 0;

    /* mask=0 uses system everywhere but can be re-enabled without a reset. */
    expected_mask = 0;
    assert(!chaos_corona_backgrounds_commit(1, 0));
    expected_mask = 8u;
    assert(!chaos_corona_backgrounds_commit(1, 8u));
    assert(commits == 24 && chaos_corona_enabled == 1 && !chaos_corona_in_call);

    /* Disabled is system for every mask, including masks with high bits. */
    expected_enabled = 0;
    for (mask = 0; mask < 16u; ++mask) {
        expected_mask = mask;
        assert(!chaos_corona_backgrounds_commit(0, mask | 0xf0u));
        assert(!chaos_corona_enabled && !chaos_corona_in_call);
    }
    assert(commits == 40);
    honeycomb_consumes=0;chaos_corona_launcher_position(&polls);assert(position_forwards==1);
    honeycomb_consumes=1;chaos_corona_launcher_position(&polls);assert(position_forwards==1);
    /* Observing the wait reason must preserve short-circuit safety: no native
     * registry walk while reentrant, scrolling, holding a snapshot or drawing. */
    unsigned queries = native_idle_queries;
    chaos_corona_in_call=1;
    assert(!chaos_corona_backgrounds_idle() && chaos_corona_idle_wait==1);
    chaos_corona_in_call=0;p67_busy=1;
    assert(!chaos_corona_backgrounds_idle() && chaos_corona_idle_wait==2);
    p67_busy=0;image_slot=0x1234;
    assert(!chaos_corona_backgrounds_idle() && chaos_corona_idle_wait==3);
    image_slot=0;scroll_latch=1;
    assert(!chaos_corona_backgrounds_idle() && chaos_corona_idle_wait==4);
    assert(native_idle_queries==queries && !surface_idle_queries);
    scroll_latch=0;idle=0;
    assert(!chaos_corona_backgrounds_idle() && chaos_corona_idle_wait==5 && !surface_idle_queries);
    idle=1;p67_surfaces[1].idle=0;
    assert(!chaos_corona_backgrounds_idle() && chaos_corona_idle_wait==7 && surface_idle_queries==2);
    p67_surfaces[1].idle=1;
    assert(chaos_corona_backgrounds_idle() && !chaos_corona_idle_wait && surface_idle_queries==5);
    puts("Chaos C ABI: 16 masks, mixed paths, rollback/reentry and honeycomb position guard passed");
    return 0;
}
