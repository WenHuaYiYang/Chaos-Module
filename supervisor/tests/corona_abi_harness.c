/* Host tests for Chaos's actual C ABI wrapper bodies, not the upstream hooks. */
#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "resource_hook_background.h"
static unsigned chaos_corona_enabled, chaos_corona_in_call;
static int idle = 1, commit_result, expected_enabled;
static unsigned commits, polls;
static int p67_blur_native_idle(void) { return idle; }
static int rh_platform_backgrounds_commit(const struct rh_backgrounds *bg) {
    const char *paths[] = {bg->launcher, bg->control_center, bg->notifications, bg->widgets};
    assert(chaos_corona_in_call == 1);
    for (unsigned i = 0; i < 4; ++i) {
        assert(!strcmp(paths[i], expected_enabled ? RH_BG_BLUR : ""));
    }
    ++commits; return commit_result;
}
static void rh_platform_backgrounds_poll(void) {
    assert(chaos_corona_in_call == 1); ++polls;
}
#include CHAOS_CORONA_ABI_SOURCE
int main(void) {
    expected_enabled = 1;
    assert(!chaos_corona_backgrounds_commit(1));
    assert(commits == 1 && chaos_corona_enabled == 1 && !chaos_corona_in_call);
    chaos_corona_backgrounds_poll(); assert(polls == 1 && !chaos_corona_in_call);
    expected_enabled = 0; commit_result = -2145;
    assert(chaos_corona_backgrounds_commit(0) == -2145);
    assert(commits == 2 && chaos_corona_enabled == 1 && !chaos_corona_in_call);
    commit_result = 0;
    assert(!chaos_corona_backgrounds_commit(0));
    assert(commits == 3 && !chaos_corona_enabled && !chaos_corona_in_call);
    idle = 0;
    assert(chaos_corona_backgrounds_commit(1) == -2145);
    assert(commits == 3 && !chaos_corona_enabled && !chaos_corona_in_call);
    idle = 1; chaos_corona_in_call = 1;
    assert(chaos_corona_backgrounds_commit(1) == -2145);
    chaos_corona_backgrounds_poll();
    assert(commits == 3 && polls == 1 && chaos_corona_in_call == 1);
    chaos_corona_in_call = 0; expected_enabled = 1; commit_result = -2143;
    assert(chaos_corona_backgrounds_commit(1) == -2143);
    assert(!chaos_corona_enabled && !chaos_corona_in_call);
    commit_result = 0;
    assert(!chaos_corona_backgrounds_commit(9));
    assert(chaos_corona_enabled == 1 && !chaos_corona_in_call);
    puts("Chaos C ABI: exact four paths, rollback, error propagation and reentry guards passed");
    return 0;
}
