/* Exact .043 Control Center battery: capture at idle, draw an immutable image.
 * A derived per-instance class suppresses only DRAW_MAIN. PREPROCESS cannot
 * suppress class drawing on this firmware (2BF690 -> 2BF710).
 * Default geometry is 62x32 plus 30px extra on all sides: 122x92, NOT 64x36.
 */
#include "resource_hook_p67_battery.h"

#define P67_BATTERY_CLASS 0x2cd198dcu
#define P67_BATTERY_EVENT 0x0c4c22cdu
#define P67_BATTERY_ROOT_SLOT 0x2010fe50u
#define P67_BATTERY_STATUS_SLOT 0x2010fe60u

struct p67_battery_key {
    uint32_t width, height, extra;
    unsigned char state[42];
};
static uint32_t p67_battery_class[9];
static uint32_t p67_battery_obj, p67_battery_root, p67_battery_status;
static uint32_t p67_battery_generation, p67_battery_buffer;
static unsigned p67_battery_busy, p67_battery_ready;
/* Sample the actual mapped cutout through the native decoder, once per source
 * or resource revision. Auxiliary GPU storage is retained on failed fences. */
#define P67_BATTERY_MASK_WIDTH 64u
#define P67_BATTERY_MASK_HEIGHT 36u
static unsigned char p67_battery_mask[P67_BATTERY_MASK_WIDTH * P67_BATTERY_MASK_HEIGHT];
static uint32_t p67_battery_mask_source, p67_battery_mask_buffer, p67_battery_mask_root;
static unsigned p67_battery_mask_ready;
#define P67_BATTERY_CHARGE_WIDTH 16u
#define P67_BATTERY_CHARGE_HEIGHT 48u
static unsigned char p67_battery_charge[P67_BATTERY_CHARGE_WIDTH * P67_BATTERY_CHARGE_HEIGHT];
static uint32_t p67_battery_charge_source;
static unsigned p67_battery_charge_ready;
#include "p67_battery_diagnostics.inc"
static struct p67_battery_key p67_battery_cached;
static void p67_battery_event(uint32_t clz, void *event);

static uint32_t p67_battery_shim(void) {
    return (uint32_t)(uintptr_t)p67_battery_class;
}
static uint32_t p67_battery_parent(uint32_t obj) {
    return ((uint32_t (*)(uint32_t))(uintptr_t)P67_NATIVE(0x0c589579u))(obj);
}
/* Draw dispatch owns obj and its attached parents. Never walk the global
 * object registry during rendering or dereference a retained, detached parent. */
static int p67_battery_attached(uint32_t obj) {
    uint32_t status;
    if (!obj || obj != p67_battery_obj ||
        load32(P67_BATTERY_ROOT_SLOT) != p67_battery_root ||
        load32(P67_BATTERY_STATUS_SLOT) != p67_battery_status) return 0;
    status = p67_battery_parent(obj);
    return status && status == p67_battery_status &&
           p67_battery_parent(status) == p67_battery_root;
}
static int p67_battery_key_read(uint32_t obj, struct p67_battery_key *key,
                                 int32_t area[4]) {
    int32_t width, height;
    uint32_t extra;
    unsigned i;
    for (i = 0; i < 4u; ++i) area[i] = (int32_t)P67_U32(obj + 28u + 4u * i);
    /* Firmware coordinates are screen-sized. Reject pathological values
     * before signed subtraction and before passing dimensions to allocation. */
    for (i = 0; i < 4u; ++i)
        if (area[i] < -4096 || area[i] > 4096) return 0;
    width = area[2] - area[0] + 1;
    height = area[3] - area[1] + 1;
    extra = ((uint32_t (*)(uint32_t))(uintptr_t)P67_NATIVE(0x0c587e71u))(obj);
    if (width <= 0 || height <= 0 || width > 128 || height > 128 ||
        extra > 63u || (uint32_t)width + 2u * extra > 128u ||
        (uint32_t)height + 2u * extra > 128u) return 0;
    key->width = (uint32_t)width; key->height = (uint32_t)height; key->extra = extra;
    for (i = 0; i < 20u; ++i) key->state[i] = P67_U8(obj + 52u + i);
    for (i = 0; i < 22u; ++i) key->state[20u + i] = P67_U8(obj + 80u + i);
    return 1;
}
static int p67_battery_key_equal(const struct p67_battery_key *a,
                                  const struct p67_battery_key *b) {
    unsigned i;
    if (a->width != b->width || a->height != b->height || a->extra != b->extra) return 0;
    for (i = 0; i < 42u; ++i) if (a->state[i] != b->state[i]) return 0;
    return 1;
}
static void p67_battery_key_store(const struct p67_battery_key *key) {
    unsigned i;
    p67_battery_cached.width = key->width;
    p67_battery_cached.height = key->height;
    p67_battery_cached.extra = key->extra;
    for (i = 0; i < 42u; ++i) p67_battery_cached.state[i] = key->state[i];
}
/* Styles and resource/font transactions can change pixels without changing
 * native source pointers. Invalidation is logical only, safe during callbacks. */
static void p67_battery_invalidate(void) {
    p67_battery_mask_source = 0; p67_battery_mask_ready = 0;
    p67_battery_charge_source = 0; p67_battery_charge_ready = 0;
    p67_battery_ready = 0; ++p67_battery_generation;
}
static void p67_battery_native_event(void *event) {
    ((void (*)(uint32_t, void *))(uintptr_t)P67_NATIVE(P67_BATTERY_EVENT))
        (P67_BATTERY_CLASS, event);
}
static void p67_battery_event(uint32_t clz, void *event) {
    uint32_t obj = *((uint32_t *)event + 1);
    unsigned code = *(uint16_t *)((unsigned char *)event + 8) & 0x7fffu;
    struct p67_battery_key key;
    int32_t area[4];
    (void)clz;
    /* C107104 sends STYLE_CHANGED=45; forwarded styles may change fonts,
     * opacity or pixels while all battery scalar/source fields stay equal. */
    if (code == 45u && obj == p67_battery_obj) p67_battery_invalidate();
    if (code == 36u && obj == p67_battery_obj) {
        /* No registry walk, class mutation, GPU fence or buffer free in DELETE.
         * The destructor walker will visit the original class exactly once. */
        p67_battery_obj = p67_battery_root = p67_battery_status = 0;
        p67_battery_ready = 0; ++p67_battery_generation;
    }
    if (code == 24u && !p67_battery_busy) {
        if (p67_battery_ready && p67_battery_attached(obj) &&
            p67_battery_key_read(obj, &key, area) && p67_battery_key_equal(&key, &p67_battery_cached)) {
            uint32_t layer = *((uint32_t *)event + 4);
            if (layer) {
                uint32_t descriptor[28];
                int32_t extra = (int32_t)key.extra;
                area[0] -= extra; area[1] -= extra;
                area[2] += extra; area[3] += extra;
                ((void (*)(void *))(uintptr_t)P67_NATIVE(0x0c5882b9u))(descriptor);
                descriptor[7] = p67_battery_buffer;
                ((void (*)(uint32_t, const void *, const int32_t *))(uintptr_t)
                    P67_NATIVE(0x0c5882a1u))(layer, descriptor, area);
                ++p67_battery_diag_draws;
                return;
            }
        }
        /* Never substitute native/black-key pixels for failed recovery. A
         * bound battery without a current recovered image stays undrawn. */
        ++p67_battery_diag_blocked;
        if (p67_battery_ready) p67_battery_diag_fail(P67_BAT_STALE, 3u);
        return;
    }
    /* Native drawing is used only inside an intentional offscreen capture.
     * Non-draw events retain normal native lifetime/layout behavior. */
    p67_battery_native_event(event);
}
static void p67_battery_detach(void) {
    uint32_t obj = p67_battery_obj;
    p67_battery_obj = p67_battery_root = p67_battery_status = 0;
    p67_battery_invalidate();
    if (p67_live(obj) && load32(obj) == p67_battery_shim())
        P67_U32(obj) = P67_BATTERY_CLASS;
    /* Buffer remains pinned until idle LVGL queues AND GPU completion. */
}
static uint32_t p67_battery_find(void) {
    uint32_t root = load32(P67_BATTERY_ROOT_SLOT);
    uint32_t status = load32(P67_BATTERY_STATUS_SLOT), count, found = 0;
    unsigned i;
    if (!p67_live(root) || !p67_live(status) || p67_battery_parent(status) != root) return 0;
    count = ((uint32_t (*)(uint32_t))(uintptr_t)P67_NATIVE(0x0c5881d9u))(status);
    if (count > 16u) return 0;
    for (i = 0; i < count; ++i) {
        uint32_t child = ((uint32_t (*)(uint32_t, int32_t))(uintptr_t)
            P67_NATIVE(0x0c5886e1u))(status, (int32_t)i);
        uint32_t clz;
        if (!child) continue;
        clz = load32(child);
        if (clz != P67_BATTERY_CLASS && clz != p67_battery_shim()) continue;
        if (found || !p67_live(child) || p67_battery_parent(child) != status) return 0;
        found = child;
    }
    return found;
}
static int p67_battery_buffer_view_for(uint32_t buf, uint32_t width, uint32_t height,
                                       uint32_t *pixels, uint32_t *stride, uint32_t *size) {
    uint32_t allocation;
    if (!p67_blur_native_ram(buf, 28u) || (P67_U32(buf) & 0xffffu) != 0x1019u ||
        P67_U32(buf + 4u) != (height << 16 | width)) return 0;
    *stride = P67_U32(buf + 8u) & 0xffffu;
    *size = P67_U32(buf + 12u); *pixels = P67_U32(buf + 16u);
    allocation = P67_U32(buf + 20u);
    return !(*pixels & 63u) && *pixels >= allocation && *pixels - allocation <= 63u &&
           !(*stride & 3u) && *stride >= width * 4u && *stride <= 2048u &&
           *size >= *stride * height && *size <= 128u * 2048u &&
           p67_blur_native_ram(allocation, *size + *pixels - allocation) &&
           P67_U32(buf + 24u) == 0x20103244u;
}
static int p67_battery_buffer_view(uint32_t width, uint32_t height,
                                    uint32_t *pixels, uint32_t *stride, uint32_t *size) {
    return p67_battery_buffer_view_for(p67_battery_buffer, width, height, pixels, stride, size);
}
/* Call only after idle queues and a successful fence, under the capture guard. */
static void p67_battery_mask_release(void) {
    uint32_t root = p67_battery_mask_root, buffer = p67_battery_mask_buffer;
    p67_battery_mask_root = p67_battery_mask_buffer = 0;
    if (root) ((void (*)(uint32_t))(uintptr_t)P67_NATIVE(0x0c588de9u))(root);
    if (buffer) ((void (*)(uint32_t))(uintptr_t)P67_NATIVE(0x0cac0db9u))(buffer);
}
static int p67_battery_mask_style(uint32_t obj) {
    uint32_t (*get)(uint32_t, uint32_t, uint32_t) = (uint32_t (*)(uint32_t, uint32_t, uint32_t))
        (uintptr_t)P67_NATIVE(0x0c1068a9u);
    /* The sampled source has native opacity and no recolor/expansion. */
    p67_battery_diag_style[0] = get(obj, 0, 68u);
    p67_battery_diag_style[1] = get(obj, 0, 70u);
    p67_battery_diag_style[2] = get(obj, 0, 104u);
    p67_battery_diag_style[3] = get(obj, 0, 105u);
    return p67_battery_diag_style[0] == 255u && !p67_battery_diag_style[1] &&
           !p67_battery_diag_style[2] && !p67_battery_diag_style[3];
}
/* 1 = sampled, 0 = unsupported, -1 = unresolved/stale, -2 = retryable OOM.
 * Explicit zero initialization options prevent screen theme/background styles.
 * The native decoder/open hook honors actual resource mappings. */
static int p67_battery_alpha_sample(uint32_t obj, uint32_t generation, uint32_t source,
                                     unsigned width, unsigned height, unsigned char *alpha,
                                     int black_only) {
    uint32_t header[3] = {0, 0, 0}, image, pixels, stride, size;
    unsigned x, y, transparent = 0, nonzero = 0;
    unsigned kind = black_only ? 0u : 1u;
    struct p67_battery_sample_diag *diag = &p67_battery_sample_diag[kind];
    const unsigned char *data;
    int result;
    static const unsigned char no_theme = 0;
    diag->reason = diag->source = diag->need_w = diag->need_h = 0;
    diag->info_rc = diag->info_w = diag->info_h = diag->info_cf = 0;
    diag->image_w = diag->image_h = diag->draw_w = diag->draw_h = diag->extra = 0;
    diag->snapshot_rc = diag->buffer_flags = diag->buffer_w = diag->buffer_h = 0;
    diag->alpha_min = diag->alpha_max = diag->alpha_zero = diag->alpha_nonzero = 0;
    diag->bad_x = diag->bad_y = diag->bad_rgba = 0;
    diag->source = source; diag->need_w = width; diag->need_h = height;
    diag->alpha_min = 255u;
    if (!source || !p67_battery_mask_style(obj)) {
        diag->reason = P67_BAT_STYLE; p67_battery_diag_fail(diag->reason, kind); return 0;
    }
    /* A failed lookup can be transient (I/O/cache allocation); do not cache
     * it as permanent unsupported input for an otherwise unchanged source. */
    diag->info_rc = ((int (*)(uint32_t, void *))(uintptr_t)P67_NATIVE(RH_FW_IMAGE_GET_INFO))(source, header);
    diag->info_w = header[1] & 0xffffu; diag->info_h = header[1] >> 16;
    diag->info_cf = (header[0] >> 8) & 0xffu;
    if (diag->info_rc != 1u) {
        diag->reason = P67_BAT_INFO; p67_battery_diag_fail(diag->reason, kind); return -2;
    }
    if (header[1] != (height << 16 | width)) {
        diag->reason = P67_BAT_SOURCE_SHAPE; p67_battery_diag_fail(diag->reason, kind); return 0;
    }
    if (p67_battery_obj != obj || p67_battery_generation != generation || !p67_live(obj) ||
        !p67_battery_attached(obj)) {
        diag->reason = P67_BAT_STALE; p67_battery_diag_fail(diag->reason, kind); return -1;
    }
    p67_battery_mask_buffer = ((uint32_t (*)(uint32_t, uint32_t, uint32_t, uint32_t))(uintptr_t)
        P67_NATIVE(0x0c587f39u))(width, height, 16u, 0u);
    if (!p67_battery_mask_buffer) {
        diag->reason = P67_BAT_ALLOC; p67_battery_diag_fail(diag->reason, kind); return -2;
    }
    /* A parentless image is a display-sized screen: set_size cannot resize
     * its coordinates. Keep that root inactive and snapshot a sized CHILD. */
    p67_battery_mask_root = ((uint32_t (*)(uint32_t, const unsigned char *))(uintptr_t)
        P67_NATIVE(0x0c588d59u))(0, &no_theme);
    image = p67_battery_mask_root ? ((uint32_t (*)(uint32_t, const unsigned char *))(uintptr_t)
        P67_NATIVE(0x0c588d59u))(p67_battery_mask_root, &no_theme) : 0;
    if (!image) {
        diag->reason = P67_BAT_CREATE; p67_battery_diag_fail(diag->reason, kind);
        p67_battery_mask_release(); return -2;
    }
    ((void (*)(uint32_t, uint32_t))(uintptr_t)P67_NATIVE(0x0c587fa1u))(image, source);
    ((void (*)(uint32_t, uint32_t, uint32_t))(uintptr_t)P67_NATIVE(0x0c588629u))(image, width, height);
    p67_blur_native_clean();
    result = ((int (*)(uint32_t, uint32_t, uint32_t))(uintptr_t)P67_NATIVE(0x0ca5fb41u))
        (image, 16u, p67_battery_mask_buffer);
    diag->snapshot_rc = (uint32_t)result;
    if (!p67_blur_native_fence() || !p67_blur_native_idle()) {
        diag->reason = P67_BAT_FENCE; p67_battery_diag_fail(diag->reason, kind); return -1;
    }
    diag->image_w = P67_U32(image + 68u); diag->image_h = P67_U32(image + 72u);
    diag->draw_w = P67_U32(image + 36u) - P67_U32(image + 28u) + 1u;
    diag->draw_h = P67_U32(image + 40u) - P67_U32(image + 32u) + 1u;
    diag->extra = ((uint32_t (*)(uint32_t))(uintptr_t)P67_NATIVE(0x0c587e71u))(image);
    if (p67_blur_native_ram(p67_battery_mask_buffer, 28u)) {
        uint32_t packed = P67_U32(p67_battery_mask_buffer + 4u);
        diag->buffer_flags = P67_U32(p67_battery_mask_buffer) >> 16;
        diag->buffer_w = packed & 0xffffu; diag->buffer_h = packed >> 16;
    }
    if (result != 1) {
        diag->reason = P67_BAT_SNAPSHOT; p67_battery_diag_fail(diag->reason, kind);
        p67_battery_mask_release(); return -2;
    }
    if (!p67_battery_buffer_view_for(p67_battery_mask_buffer, width, height, &pixels, &stride, &size)) {
        diag->reason = P67_BAT_BUFFER; p67_battery_diag_fail(diag->reason, kind);
        p67_battery_mask_release(); return -2;
    }
    if (diag->image_w != width || diag->image_h != height ||
        diag->draw_w != width || diag->draw_h != height || diag->extra) {
        diag->reason = P67_BAT_IMAGE_SHAPE; p67_battery_diag_fail(diag->reason, kind);
        p67_battery_mask_release(); return 0;
    }
    if (p67_battery_obj != obj || p67_battery_generation != generation || !p67_live(obj) ||
        !p67_battery_attached(obj)) {
        diag->reason = P67_BAT_STALE; p67_battery_diag_fail(diag->reason, kind);
        p67_battery_mask_release(); return -1;
    }
    p67_blur_native_clean(); p67_blur_native_invalidate(pixels, size);
#ifdef P67_BATTERY_TEST_PIXELS
    data = P67_BATTERY_TEST_PIXELS(pixels);
#else
    data = (const unsigned char *)(uintptr_t)pixels;
#endif
    for (y = 0; y < height; ++y) {
        for (x = 0; x < width; ++x) {
            const unsigned char *p = data + y * stride + 4u * x;
            if (p[3] < diag->alpha_min) diag->alpha_min = p[3];
            if (p[3] > diag->alpha_max) diag->alpha_max = p[3];
            diag->alpha_zero += p[3] == 0; diag->alpha_nonzero += p[3] != 0;
            if (black_only && p[3] && (p[0] | p[1] | p[2])) {
                diag->bad_x = x; diag->bad_y = y;
                diag->bad_rgba = p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24;
                diag->reason = P67_BAT_NONBLACK; p67_battery_diag_fail(diag->reason, kind);
                p67_battery_mask_release(); return 0;
            }
            alpha[y * width + x] = p[3];
            transparent |= p[3] == 0; nonzero |= p[3] != 0;
        }
    }
    p67_battery_mask_release();
    if (p67_battery_obj != obj || p67_battery_generation != generation) {
        diag->reason = P67_BAT_STALE; p67_battery_diag_fail(diag->reason, kind); return -1;
    }
    /* Unmatting uses measured coverage; it does not require an exact 255
     * sample. GPU rounding and valid fractional masks can have max alpha 254. */
    if (black_only && (!transparent || !nonzero)) {
        diag->reason = P67_BAT_EMPTY_ALPHA; p67_battery_diag_fail(diag->reason, kind); return 0;
    }
    diag->reason = P67_BAT_NONE;
    return 1;
}
static int p67_battery_mask_sample(uint32_t obj, uint32_t generation, uint32_t source) {
    int result;
    if (p67_battery_mask_source == source && source) {
        if (!p67_battery_mask_ready) p67_battery_diag_fail(p67_battery_sample_diag[0].reason, 0u);
        return p67_battery_mask_ready ? 1 : 0;
    }
    p67_battery_mask_ready = 0;
    result = p67_battery_alpha_sample(obj, generation, source, P67_BATTERY_MASK_WIDTH,
                                     P67_BATTERY_MASK_HEIGHT, p67_battery_mask, 1);
    if (result >= 0) { p67_battery_mask_source = source; p67_battery_mask_ready = result == 1; }
    return result == -2 ? 0 : result;
}
static int p67_battery_centered_charge(uint32_t obj, const struct p67_battery_key *key) {
    return (key->state[41] & 5u) == 5u && P67_U32(obj + 56u);
}
static int p67_battery_charge_sample(uint32_t obj, uint32_t generation) {
    uint32_t source = P67_U32(obj + 56u);
    int result;
    if (p67_battery_charge_source == source && source) {
        if (!p67_battery_charge_ready) p67_battery_diag_fail(p67_battery_sample_diag[1].reason, 1u);
        return p67_battery_charge_ready ? 1 : 0;
    }
    p67_battery_charge_ready = 0;
    if (P67_U32(obj + 60u) != P67_BATTERY_CHARGE_WIDTH ||
        P67_U32(obj + 64u) != P67_BATTERY_CHARGE_HEIGHT) {
        p67_battery_charge_source = source;
        p67_battery_sample_diag[1].source = source;
        p67_battery_sample_diag[1].image_w = P67_U32(obj + 60u);
        p67_battery_sample_diag[1].image_h = P67_U32(obj + 64u);
        p67_battery_sample_diag[1].reason = P67_BAT_CHARGE_SHAPE;
        p67_battery_diag_fail(P67_BAT_CHARGE_SHAPE, 1u); return 0;
    }
    result = p67_battery_alpha_sample(obj, generation, source, P67_BATTERY_CHARGE_WIDTH,
                                     P67_BATTERY_CHARGE_HEIGHT, p67_battery_charge, 0);
    if (result >= 0) { p67_battery_charge_source = source; p67_battery_charge_ready = result == 1; }
    return result == -2 ? 0 : result;
}
static int p67_battery_unmatte(void *pixels, uint32_t size, uint32_t stride,
                                const struct p67_battery_key *key, int centered, int premultiplied) {
    unsigned x, y, width = key->width + 2u, height = key->height + 1u;
    uint32_t snapshot_width = key->width + 2u * key->extra;
    uint32_t snapshot_height = key->height + 2u * key->extra;
    int32_t charge_x = 2 + ((int32_t)key->width - (int32_t)P67_BATTERY_CHARGE_WIDTH) / 2;
    int32_t charge_y = 1 + ((int32_t)key->height - (int32_t)P67_BATTERY_CHARGE_HEIGHT) / 2;
    if (width > P67_BATTERY_MASK_WIDTH || height > P67_BATTERY_MASK_HEIGHT ||
        snapshot_width > 128u || snapshot_height > 128u) return 0;
    for (y = 0; y < height; ++y) {
        unsigned char row[P67_BATTERY_MASK_WIDTH];
        for (x = 0; x < width; ++x) {
            unsigned k = p67_battery_mask[y * P67_BATTERY_MASK_WIDTH + x];
            int32_t cx = (int32_t)x - charge_x, cy = (int32_t)y - charge_y;
            if (centered && cx >= 0 && cy >= 0 && cx < (int32_t)P67_BATTERY_CHARGE_WIDTH &&
                cy < (int32_t)P67_BATTERY_CHARGE_HEIGHT) {
                unsigned a = p67_battery_charge[(unsigned)cy * P67_BATTERY_CHARGE_WIDTH + (unsigned)cx];
                /* The later glyph attenuates the mask contribution. Removing
                 * a whole rectangle would leave black behind transparent glyph pixels. */
                k = (k * (255u - a) + 127u) / 255u;
            }
            row[x] = (unsigned char)k;
        }
        if (!p67_battery_unmatte_black(pixels, size, stride, snapshot_width, snapshot_height,
                row, sizeof(row), sizeof(row), width, 1u, (int32_t)key->extra - 2,
                (int32_t)key->extra - 1 + (int32_t)y, premultiplied)) return 0;
    }
    return 1;
}
static void p67_battery_free(void) {
    uint32_t buffer = p67_battery_buffer;
    p67_battery_buffer = 0; p67_battery_ready = 0;
    if (buffer) ((void (*)(uint32_t))(uintptr_t)P67_NATIVE(0x0cac0db9u))(buffer);
}
/* Caller is the reserved UI-owner background poll, never a draw callback.
 * One persistent allocation; safe idle fences precede ALL reuse/free. */
static void p67_battery_poll(int enabled) {
    uint32_t obj, generation, width, height, pixels, stride, size, source;
    struct p67_battery_key key, after;
    int32_t area[4];
    int captured, centered;
    unsigned i, powered = 0;
    if (p67_battery_busy) return;
    p67_battery_busy = 1;
    if (!enabled) {
        p67_battery_diag_fail(P67_BAT_DISABLED, 2u);
        p67_battery_detach();
        if ((p67_battery_buffer || p67_battery_mask_buffer || p67_battery_mask_root) &&
            p67_blur_native_idle() && p67_blur_native_fence() && p67_blur_native_idle()) {
            p67_battery_mask_release(); p67_battery_free();
        }
        goto done;
    }
    if (!p67_blur_native_idle()) { p67_battery_diag_fail(P67_BAT_IDLE, 2u); goto done; }
    obj = p67_battery_find();
    if (obj != p67_battery_obj || (obj && !p67_battery_attached(obj))) {
        p67_battery_detach();
        if (obj && load32(obj) == P67_BATTERY_CLASS) {
            for (i = 0; i < 9u; ++i) p67_battery_class[i] = P67_U32(P67_BATTERY_CLASS + 4u * i);
            p67_battery_class[0] = P67_BATTERY_CLASS;
            p67_battery_class[1] = p67_battery_class[2] = 0;
            p67_battery_class[3] = (uint32_t)(uintptr_t)p67_battery_event;
            p67_battery_obj = obj;
            p67_battery_root = load32(P67_BATTERY_ROOT_SLOT);
            p67_battery_status = load32(P67_BATTERY_STATUS_SLOT);
            P67_U32(obj) = p67_battery_shim();
        }
    }
    obj = p67_battery_obj; generation = p67_battery_generation;
    if (!obj) {
        p67_battery_diag_fail(P67_BAT_BINDING, 2u);
        if ((p67_battery_buffer || p67_battery_mask_buffer || p67_battery_mask_root) &&
            p67_blur_native_idle() && p67_blur_native_fence() && p67_blur_native_idle()) {
            p67_battery_mask_release(); p67_battery_free();
        }
        goto done;
    }
    if (!p67_battery_key_read(obj, &key, area)) {
        p67_battery_ready = 0; p67_battery_diag_fail(P67_BAT_KEY, 2u); goto done;
    }
    p67_battery_diag_main[0] = key.width; p67_battery_diag_main[1] = key.height;
    p67_battery_diag_main[2] = key.extra;
    source = P67_U32(obj + 52u);
    centered = p67_battery_centered_charge(obj, &key);
    if (p67_battery_ready && p67_battery_key_equal(&key, &p67_battery_cached) &&
        p67_battery_mask_source == source && !p67_battery_mask_root && !p67_battery_mask_buffer &&
        p67_battery_mask_ready && (!centered || (p67_battery_charge_ready &&
         p67_battery_charge_source == P67_U32(obj + 56u)))) {
        p67_battery_diag_mode = 2u; p67_battery_diag_reason = P67_BAT_NONE; goto done;
    }
    p67_battery_ready = 0; p67_battery_diag_mode = 1u;
    p67_battery_diag_reason = P67_BAT_NONE;
    /* Keep the native snapshot power guard across capture and CPU readback;
     * nested snapshot/fence helpers borrow the same refcounted lock. */
    p67_blur_native_power_begin(); powered = 1;
    if (!p67_blur_native_fence() || !p67_blur_native_idle()) {
        p67_battery_diag_fail(P67_BAT_FENCE, 2u); goto done;
    }
    if (p67_battery_obj != obj || p67_battery_generation != generation ||
        !p67_live(obj) || !p67_battery_attached(obj) || load32(obj) != p67_battery_shim()) goto done;
    p67_battery_mask_release();
    if (p67_battery_mask_sample(obj, generation, source) != 1) goto done;
    if (!p67_blur_native_idle()) { p67_battery_diag_fail(P67_BAT_IDLE, 0u); goto done; }
    if (key.width + 2u > P67_BATTERY_MASK_WIDTH || key.height + 1u > P67_BATTERY_MASK_HEIGHT) {
        p67_battery_diag_fail(P67_BAT_SOURCE_SHAPE, 2u); goto done;
    }
    if (p67_battery_obj != obj || p67_battery_generation != generation ||
        !p67_live(obj) || !p67_battery_attached(obj)) goto done;
    width = key.width + 2u * key.extra; height = key.height + 2u * key.extra;
    if (p67_battery_buffer && !p67_battery_buffer_view(width, height, &pixels, &stride, &size))
        p67_battery_free();
    if (!p67_battery_buffer)
        p67_battery_buffer = ((uint32_t (*)(uint32_t, uint32_t, uint32_t, uint32_t))
            (uintptr_t)P67_NATIVE(0x0c587f39u))(width, height, 16u, 0u);
    if (!p67_battery_buffer || !p67_battery_buffer_view(width, height, &pixels, &stride, &size)) {
        p67_battery_diag_fail(P67_BAT_MAIN_ALLOC, 2u); goto done;
    }
    /* Allocators/cache/PM calls may invoke native callbacks. Revalidate the
     * captured identity before handing any borrowed object to snapshot. */
    p67_blur_native_clean();
    if (p67_battery_obj != obj || p67_battery_generation != generation ||
        !p67_live(obj) || !p67_battery_attached(obj) || load32(obj) != p67_battery_shim()) goto done;
    captured = ((int (*)(uint32_t, uint32_t, uint32_t))(uintptr_t)
        P67_NATIVE(0x0ca5fb41u))(obj, 16u, p67_battery_buffer);
    /* The snapshot drains its LVGL tasks; a successful GPU fence is still
     * required before CPU reads. Failed completion pins storage for retry. */
    if (!p67_blur_native_fence() || !p67_blur_native_idle()) {
        p67_battery_diag_fail(P67_BAT_FENCE, 2u); goto done;
    }
    if (captured != 1) { p67_battery_diag_fail(P67_BAT_MAIN_SNAPSHOT, 2u); goto done; }
    p67_battery_diag_main[3] = P67_U32(p67_battery_buffer) >> 16;
    if (p67_battery_obj != obj || p67_battery_generation != generation ||
        !p67_live(obj) || !p67_battery_attached(obj) ||
        !p67_battery_key_read(obj, &after, area)) goto done;
    /* Layout and first draw may update geometry and cached source dimensions. */
    if (after.width + 2u * after.extra != width || after.height + 2u * after.extra != height ||
        !p67_battery_buffer_view(width, height, &pixels, &stride, &size)) goto done;
    centered = p67_battery_centered_charge(obj, &after);
    if (p67_battery_mask_ready && after.width + 2u <= P67_BATTERY_MASK_WIDTH &&
        after.height + 1u <= P67_BATTERY_MASK_HEIGHT && centered) {
        if (p67_battery_charge_sample(obj, generation) != 1) goto done;
        if (!p67_blur_native_idle()) { p67_battery_diag_fail(P67_BAT_IDLE, 1u); goto done; }
        /* Auxiliary snapshot/layout must not change the already captured main image's state. */
        if (p67_battery_obj != obj || p67_battery_generation != generation || !p67_live(obj) ||
            !p67_battery_attached(obj) || !p67_battery_key_read(obj, &key, area) ||
            !p67_battery_key_equal(&key, &after)) goto done;
    }
    p67_blur_native_clean();
    p67_blur_native_invalidate(pixels, size);
    {
        void *data;
#ifdef P67_BATTERY_TEST_PIXELS
        data = P67_BATTERY_TEST_PIXELS(pixels);
#else
        data = (void *)(uintptr_t)pixels;
#endif
        if (p67_battery_mask_ready && p67_battery_mask_source == P67_U32(obj + 52u) &&
            after.width + 2u <= P67_BATTERY_MASK_WIDTH && after.height + 1u <= P67_BATTERY_MASK_HEIGHT &&
            (!centered || (p67_battery_charge_ready && p67_battery_charge_source == P67_U32(obj + 56u)))) {
            /* Native mask starts at object(x1-2,y1-1), inside the expanded
             * snapshot. Read its actual coverage, never infer alpha from gray. */
            int premultiplied = !!(P67_U32(p67_battery_buffer) & (1u << 16));
            if (!p67_battery_unmatte(data, size, stride, &after, centered, premultiplied)) {
                p67_battery_diag_fail(P67_BAT_UNMATTE, 2u); goto done;
            }
        } else { p67_battery_diag_fail(P67_BAT_STALE, 2u); goto done; }
    }
    p67_blur_native_clean();
    p67_battery_key_store(&after); p67_battery_ready = 1;
    p67_battery_diag_mode = 2u; p67_battery_diag_reason = P67_BAT_NONE;
    p67_wake_pending = 1;
done:
    if (powered) p67_blur_native_power_end();
    if (!p67_battery_ready && p67_battery_diag_mode == 1u)
        p67_battery_diag_fail(P67_BAT_STALE, 2u);
    /* Keep the capture guard through diagnostic I/O to reject reentrant polls. */
    p67_battery_diag_log();
    p67_battery_busy = 0;
}
