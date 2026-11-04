/* Exact .043 active watchface snapshot provider. Input and output buffers are
 * module-owned; no native transition pool descriptor is read or acquired. */
#include "resource_hook_p67_blur.h"

#define P67_BLUR_AVAILABLE 1
#define P67_BLUR_DARKEN_PERCENT 70u
#define P67_BLUR_COPY(plane, identity) p67_blur_native_copy((plane), (identity))
#define P67_BLUR_PUBLISH(plane, epoch) p67_blur_native_publish((plane), (epoch))
#define P67_BLUR_RETIRE(source) p67_blur_native_retire(source)
#define P67_BLUR_WATCHFACE() p67_blur_native_watchface()
#define P67_BLUR_MAINTAIN() p67_blur_native_maintain()
#define P67_BLUR_CLEANUP_PENDING() \
    (p67_blur_native_input || p67_blur_native_outputs[0].retired || p67_blur_native_outputs[1].retired)
#define P67_BLUR_DISPLAY_ADD(display, callback, generation) \
    p67_blur_native_display_add((display), (uint32_t)(uintptr_t)(callback), (generation))
#define P67_BLUR_DISPLAY_REMOVE(display, callback) \
    p67_blur_native_display_remove((display), (uint32_t)(uintptr_t)(callback))
#define P67_BLUR_DISPLAY_COUNT(display) p67_blur_native_display_count(display)
#define P67_BLUR_DISPLAY_GET(display, index) p67_blur_native_display_get((display), (index))

struct p67_blur_native_output { uint32_t descriptor; unsigned retired; };
static struct p67_blur_native_output p67_blur_native_outputs[2];
static uint32_t p67_blur_native_input;
static unsigned p67_blur_native_capturing, p67_blur_native_power_depth;
/* Memory-only diagnostics. They are serialized by the UI-owner watcher, never
 * written from snapshot, draw, or refresh callbacks. */
static uint32_t p67_blur_native_error, p67_blur_native_idle_error;
static uint32_t p67_blur_native_display_seen, p67_blur_native_render_bits;
static uint32_t p67_blur_native_layer_seen, p67_blur_native_root_seen;
static uint32_t p67_blur_native_width, p67_blur_native_height, p67_blur_native_stride;
static uint32_t p67_blur_native_nonzero, p67_blur_native_capture_calls;
static uint32_t p67_blur_native_input_allocs, p67_blur_native_output_allocs;
static uint32_t p67_blur_native_fences, p67_blur_native_pm_stays, p67_blur_native_pm_relaxes;
static int p67_blur_native_last_gpu, p67_blur_native_capture_result;
static int p67_blur_native_last_pm_stay, p67_blur_native_last_pm_relax;
#define P67_BLUR_NATIVE_DIAGNOSTICS(X) \
    X("native_error", p67_blur_native_error) \
    X("idle_error", p67_blur_native_idle_error) \
    X("native_display", p67_blur_native_display_seen) \
    X("render_bits", p67_blur_native_render_bits) \
    X("layer", p67_blur_native_layer_seen) \
    X("wf_slot", p67_blur_native_root_seen) \
    X("last_gpu", (uint32_t)p67_blur_native_last_gpu) \
    X("capture_result", (uint32_t)p67_blur_native_capture_result) \
    X("input", p67_blur_native_input) \
    X("width", p67_blur_native_width) \
    X("height", p67_blur_native_height) \
    X("stride", p67_blur_native_stride) \
    X("nonzero", p67_blur_native_nonzero) \
    X("capture_calls", p67_blur_native_capture_calls) \
    X("input_allocs", p67_blur_native_input_allocs) \
    X("output_allocs", p67_blur_native_output_allocs) \
    X("fences", p67_blur_native_fences) \
    X("pm_stays", p67_blur_native_pm_stays) \
    X("pm_relaxes", p67_blur_native_pm_relaxes) \
    X("last_pm_stay", (uint32_t)p67_blur_native_last_pm_stay) \
    X("last_pm_relax", (uint32_t)p67_blur_native_last_pm_relax)
static void p67_blur_native_maintain(void);

static int p67_blur_native_heap(uint32_t address, uint32_t size) {
    return size && address >= 0x3c271400u && address < 0x3dfc0000u &&
           size <= 0x3dfc0000u - address;
}
static int p67_blur_native_ram(uint32_t address, uint32_t size) {
    return p67_blur_native_heap(address, size) ||
        (size && address >= 0x200765c0u && address < 0x2015fec0u &&
         size <= 0x2015fec0u - address);
}
/* Display event-list APIs are not the LVGL object-event APIs. */
static uint32_t p67_blur_native_display_count(uint32_t display) {
    if (!p67_blur_native_ram(display, 792u)) return 65u;
    return ((uint32_t (*)(uint32_t))(uintptr_t)P67_NATIVE(0x1c07ab49u))(display + 736u);
}
static uint32_t *p67_blur_native_display_get(uint32_t display, uint32_t index) {
    if (!p67_blur_native_ram(display, 792u)) return 0;
    return ((uint32_t *(*)(uint32_t, uint32_t))(uintptr_t)
        P67_NATIVE(0x1c07a4b1u))(display + 736u, index);
}
static uint32_t *p67_blur_native_display_add(uint32_t display, uint32_t callback, uint32_t generation) {
    if (!p67_blur_native_ram(display, 792u)) return 0;
    return ((uint32_t *(*)(uint32_t, uint32_t, uint32_t, uint32_t))(uintptr_t)
        P67_NATIVE(0x1c07a3e9u))(display + 736u, callback, 0x8000u, generation);
}
static int p67_blur_native_display_remove(uint32_t display, uint32_t callback) {
    uint32_t i, count = p67_blur_native_display_count(display);
    if (count > 64u) return 0;
    for (i = 0; i < count; ++i) {
        uint32_t *d = p67_blur_native_display_get(display, i);
        if (d && d[0] == callback && d[2] == 0x8000u)
            return ((int (*)(uint32_t, uint32_t))(uintptr_t)
                P67_NATIVE(0x1c07a6f9u))(display + 736u, i);
    }
    return 0;
}
static void p67_blur_native_barrier(void) {
#ifdef P67_BLUR_TEST_BARRIER
    P67_BLUR_TEST_BARRIER();
#else
    __asm__ volatile("dsb sy\n\tisb sy" ::: "memory");
#endif
}
/* The stock miwear snapshot wrapper brackets capture with this native PM lock.
 * Borrow it through balanced APIs only; never change its fields directly.
 * COPY already protects nested fences. Only outer transitions enter native PM
 * (and its governor callbacks); standalone maintenance still gets its own lock. */
static void p67_blur_native_power_begin(void) {
    if (p67_blur_native_power_depth++) return;
    p67_blur_native_last_pm_stay = ((int (*)(uint32_t))(uintptr_t)
        P67_NATIVE(0x0c1a78f1u))(0x200eb538u);
    ++p67_blur_native_pm_stays;
}
static void p67_blur_native_power_end(void) {
    if (!p67_blur_native_power_depth || --p67_blur_native_power_depth) return;
    p67_blur_native_last_pm_relax = ((int (*)(uint32_t))(uintptr_t)
        P67_NATIVE(0x0c1a79b5u))(0x200eb538u);
    ++p67_blur_native_pm_relaxes;
}
static int p67_blur_native_fence(void) {
    int status;
    p67_blur_native_power_begin();
    status = ((int (*)(void))(uintptr_t)P67_NATIVE(0x0c187bb1u))();
    ++p67_blur_native_fences;
    p67_blur_native_last_gpu = status;
    p67_blur_native_barrier();
    p67_blur_native_power_end();
    return status == 0;
}
static void p67_blur_native_clean(void) {
    /* The audited HAL switches to the whole-cache command for large ranges.
     * Clean before whole-cache invalidation to preserve unrelated CPU writes. */
    ((int (*)(uint32_t, uint32_t, uint32_t))(uintptr_t)
        P67_NATIVE(0x0c588f89u))(1u, 0u, 0x10000u);
    p67_blur_native_barrier();
}
static void p67_blur_native_invalidate(uint32_t pixels, uint32_t size) {
    ((int (*)(uint32_t, uint32_t, uint32_t))(uintptr_t)
        P67_NATIVE(0x0c588449u))(1u, pixels, size);
    p67_blur_native_barrier();
}
/* Raw GPU completion alone cannot retire queued LVGL tasks. Reject rendering,
 * unresolved layers, outstanding tasks and unbounded/cyclic layer chains. */
static int p67_blur_native_idle(void) {
    uint32_t display = load32(RH_FW_DISPLAY_SLOT), layer;
    unsigned count = 0;
    p67_blur_native_display_seen = display;
    p67_blur_native_idle_error = 0;
    if (!p67_blur_native_ram(display, 684u)) { p67_blur_native_idle_error=1; return 0; }
    p67_blur_native_render_bits = P67_U8(display + 58u);
    if (p67_blur_native_render_bits & 2u) { p67_blur_native_idle_error=2; return 0; }
    layer = P67_U32(display + 680u);
    p67_blur_native_layer_seen = layer;
    while (layer) {
        if (++count > 16u) { p67_blur_native_idle_error=5; return 0; }
        if (!p67_blur_native_ram(layer, 120u)) { p67_blur_native_idle_error=3; return 0; }
        if (P67_U32(layer + 100u)) { p67_blur_native_idle_error=4; return 0; }
        layer = P67_U32(layer + 108u);
    }
    if (display != load32(RH_FW_DISPLAY_SLOT) || (P67_U8(display + 58u) & 2u)) {
        p67_blur_native_idle_error=6; return 0;
    }
    return 1;
}
static uint32_t p67_blur_native_watchface(void) {
    uint32_t root = load32(0x20119778u);
    p67_blur_native_root_seen = root;
    return p67_live(root) ? root : 0;
}
static int p67_blur_native_descriptor_for(uint32_t descriptor, unsigned format,
                                          unsigned width, unsigned height,
                                          struct p67_blur_pixel_view *view) {
    uint32_t header, size, pixels, allocation, packed, stride;
    if (!p67_blur_native_ram(descriptor, 28u)) return 0;
    header = P67_U32(descriptor);
    packed = P67_U32(descriptor + 4u);
    stride = P67_U32(descriptor + 8u) & 0xffffu;
    size = P67_U32(descriptor + 12u);
    pixels = P67_U32(descriptor + 16u);
    allocation = P67_U32(descriptor + 20u);
    p67_blur_native_width = packed & 0xffffu;
    p67_blur_native_height = packed >> 16;
    p67_blur_native_stride = stride;
    if ((header & 0xffu) != 25u || ((header >> 8) & 0xffu) != format ||
        packed != (height << 16 | width) || (pixels & 63u) || pixels < allocation ||
        pixels - allocation > 63u || size > 1024u * 1024u ||
        stride < width * (format == 15u ? 3u : 2u) || stride > 2048u ||
        size < stride * height || !p67_blur_native_ram(allocation, size + pixels - allocation) ||
        P67_U32(descriptor + 24u) != 0x20103244u) return 0;
#ifdef P67_BLUR_TEST_PIXELS
    view->pixels = P67_BLUR_TEST_PIXELS(pixels);
#else
    view->pixels = (const unsigned char *)(uintptr_t)pixels;
#endif
    view->width = width; view->height = height; view->format = format;
    view->stride = stride; view->size = size;
    return 1;
}
/* Input stays full-screen BGR888. Output validation has a separate fixed shape;
 * accepting a small GPU source must never loosen the snapshot input contract. */
static int p67_blur_native_descriptor(uint32_t descriptor, unsigned format,
                                      struct p67_blur_pixel_view *view) {
    return p67_blur_native_descriptor_for(descriptor, format, 336u, 480u, view);
}
static int p67_blur_native_output_descriptor(uint32_t descriptor,
                                             struct p67_blur_pixel_view *view) {
    if (!p67_blur_native_descriptor_for(descriptor, 18u,
            P67_BLUR_WIDTH, P67_BLUR_HEIGHT, view)) return 0;
    /* The audited linear RGB565 VG-Lite path requires a four-byte stride. */
    return !(view->stride & 3u);
}
static int p67_blur_native_copy(uint16_t *plane, uint32_t *identity) {
    struct p67_blur_pixel_view view;
    uint32_t root, descriptor;
    int captured, success = 0;
    p67_blur_native_error = 0;
    if (!plane || !identity || p67_blur_native_capturing) { p67_blur_native_error=17; return 0; }
    p67_blur_native_power_begin();
    p67_blur_native_maintain();
    if (p67_blur_native_input) { p67_blur_native_error=3; goto power_done; }
    root = p67_blur_native_watchface();
    if (!root) { p67_blur_native_error=1; goto power_done; }
    if (!p67_blur_native_idle()) { p67_blur_native_error=2; goto power_done; }
    if (!p67_blur_native_fence()) { p67_blur_native_error=4; goto power_done; }
    descriptor = ((uint32_t (*)(uint32_t, uint32_t, uint32_t, uint32_t))(uintptr_t)
        P67_NATIVE(0x0c587f39u))(336u, 480u, 15u, 0u);
    if (!descriptor) { p67_blur_native_error=5; goto power_done; }
    ++p67_blur_native_input_allocs;
    p67_blur_native_input = descriptor;
    p67_blur_native_capturing = 1;
    if (!p67_blur_native_descriptor(descriptor, 15u, &view)) { p67_blur_native_error=6; goto done; }
    p67_blur_native_clean();
    /* lv_snapshot_take_to_draw_buf falls back to direct root redraw when the
     * root is hidden. Child visibility is respected; no HIDDEN flags change. */
    ++p67_blur_native_capture_calls;
    captured = ((int (*)(uint32_t, uint32_t, uint32_t))(uintptr_t)
        P67_NATIVE(0x0cac0879u))(root, 15u, descriptor);
    p67_blur_native_capture_result = captured;
    /* Snapshot success does not propagate GPU errors. Fence even on failure;
     * never read/free storage until GPU completion and restored native layers. */
    if (!p67_blur_native_fence()) { p67_blur_native_error=8; goto done; }
    if (!p67_blur_native_idle()) { p67_blur_native_error=9; goto done; }
    if (captured != 1) { p67_blur_native_error=7; goto done; }
    if (root != p67_blur_native_watchface()) { p67_blur_native_error=10; goto done; }
    if (!p67_blur_native_descriptor(descriptor, 15u, &view)) { p67_blur_native_error=11; goto done; }
    /* Like Band 11, preserve unrelated dirty CPU writes before the audited
     * large-range invalidator switches to the whole-cache command. */
    p67_blur_native_clean();
    p67_blur_native_invalidate(P67_U32(descriptor + 16u), (uint32_t)view.size);
    if (!p67_blur_downsample_bgr888(&view, plane, P67_BLUR_PIXELS)) { p67_blur_native_error=12; goto done; }
    p67_blur_native_nonzero=0;
    for (unsigned i=0;i<P67_BLUR_PIXELS;++i) p67_blur_native_nonzero += plane[i] != 0;
    if (root != p67_blur_native_watchface()) { p67_blur_native_error=10; goto done; }
    *identity = root;
    success = 1;
done:
    p67_blur_native_capturing = 0;
    /* Successful fences permit immediate release; failure retains input for
     * maintenance at the next safe refresh boundary, without allocating more. */
    p67_blur_native_maintain();
power_done:
    p67_blur_native_power_end();
    return success;
}
static void p67_blur_native_retire(uintptr_t source) {
    unsigned i;
    for (i = 0; i < 2u; ++i)
        if (p67_blur_native_outputs[i].descriptor == source)
            p67_blur_native_outputs[i].retired = 1;
    /* Never fence or free from DELETE, draw, or a reentrant identity check. */
}
static void p67_blur_native_maintain(void) {
    unsigned i, pending = !!p67_blur_native_input;
    if (p67_blur_native_capturing) return;
    for (i = 0; i < 2u; ++i) pending |= p67_blur_native_outputs[i].retired;
    if (!pending || !p67_blur_native_idle() || !p67_blur_native_fence() ||
        !p67_blur_native_idle()) return;
    if (p67_blur_native_input) {
        uint32_t descriptor = p67_blur_native_input;
        p67_blur_native_input = 0;
        ((void (*)(uint32_t))(uintptr_t)P67_NATIVE(0x0cac0db9u))(descriptor);
    }
    for (i = 0; i < 2u; ++i) {
        struct p67_blur_native_output *out = &p67_blur_native_outputs[i];
        if (out->retired) {
            uint32_t descriptor = out->descriptor;
            out->descriptor = 0; out->retired = 0;
            ((void (*)(uint32_t))(uintptr_t)P67_NATIVE(0x0cac0db9u))(descriptor);
        }
    }
}
static uintptr_t p67_blur_native_publish(const uint16_t *plane, uint32_t epoch) {
    struct p67_blur_pixel_view view;
    struct p67_blur_native_output *out = 0;
    unsigned i;
    uint32_t descriptor;
    (void)epoch;
    p67_blur_native_error=0;
    if (!p67_blur_native_idle()) { p67_blur_native_error=2; return 0; }
    p67_blur_native_maintain();
    for (i = 0; i < 2u; ++i) if (!p67_blur_native_outputs[i].descriptor) {
        out = &p67_blur_native_outputs[i]; break;
    }
    if (!out) { p67_blur_native_error=13; return 0; }
    descriptor = ((uint32_t (*)(uint32_t, uint32_t, uint32_t, uint32_t))(uintptr_t)
        P67_NATIVE(0x0c587f39u))(P67_BLUR_WIDTH, P67_BLUR_HEIGHT, 18u, 0u);
    if (!descriptor) { p67_blur_native_error=14; return 0; }
    ++p67_blur_native_output_allocs;
    out->descriptor = descriptor; out->retired = 0;
    if (!p67_blur_native_output_descriptor(descriptor, &view)) {
        p67_blur_native_error=15; out->retired=1; return 0;
    }
    if (!p67_blur_store_rgb565(plane, (void *)(uintptr_t)view.pixels, view.stride, view.size)) {
        p67_blur_native_error=16; out->retired=1; return 0;
    }
    p67_blur_native_clean();
    return descriptor;
}
