#ifndef RESOURCE_HOOK_BACKGROUND_H
#define RESOURCE_HOOK_BACKGROUND_H
#include "resource_hook.h"

#define RH_BG_BLUR ":blur"

static inline int rh_bytes_equal(const char *a, const char *b, uint32_t size) {
    uint32_t i;
    if (!a || !b) return 0;
    for (i = 0; i < size; i++)
        if (a[i] != b[i]) return 0;
    return 1;
}

static inline int rh_background_is_blur(const char *path) {
    return rh_bytes_equal(path, RH_BG_BLUR, sizeof(RH_BG_BLUR));
}

/* Empty image selects the system background; otherwise it is a native path or special. */
struct rh_backgrounds {
    char launcher[RH_PATH];
    char control_center[RH_PATH];
    char notifications[RH_PATH];
    /* Accepted on every target; unsupported adapters must ignore this field. */
    char widgets[RH_PATH];
};

/* Strict newline-terminated format:
 *   version=1\nlauncher=themes/<id>/<path>\ncontrol_center=system\nnotifications=...\nwidgets=...\n
 * Also accepts legacy format:
 *   version=1\nmode=system\n
 *   version=1\nmode=image\nimage=themes/<id>/<path>\n
 * Returns 0 on success, -1 on invalid input; *out is unchanged on failure. */
int rh_backgrounds_parse(const char *data, uint32_t size,
                         struct rh_backgrounds *out);

#endif
