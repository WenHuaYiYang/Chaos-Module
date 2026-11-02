/* Host tests for Chaos's actual C ABI wrapper bodies, not the upstream hooks. */
#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "resource_hook_background.h"

static unsigned chaos_corona_enabled, chaos_corona_in_call;
static int idle = 1, commit_result;
static unsigned expected_enabled, expected_mask;
static unsigned commits, polls;
static unsigned reenter_commit, reenter_poll, reentry_checks;

int chaos_corona_backgrounds_commit(uint32_t enabled, uint32_t region_mask);
void chaos_corona_backgrounds_poll(void);

static int p67_blur_native_idle(void) { return idle; }

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
    puts("Chaos C ABI: 16 masks, mixed system/blur paths, rollback, busy, scratch and reentry guards passed");
    return 0;
}
