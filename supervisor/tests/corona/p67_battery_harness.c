/* Actual .043 battery adapter through native seams, not device acceptance. */
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#define RH_TARGET_1043 1
#include "resource_hook_target.h"

#define ROOT 0x20001000u
#define STATUS 0x20002000u
#define BATTERY 0x20003000u
#define OTHER 0x20004000u
#define DUPLICATE 0x20005000u
#define BUFFER 0x20006000u
#define ALLOCATION 0x20010001u
#define PIXELS 0x20010040u
#define LAYER 0x20008000u
#define STOCK_CLASS 0x2cd198dcu
#define PIXEL_CAPACITY (128u * 2048u)
#define MASK_SOURCE 0x2cba0000u
#define OTHER_MASK_SOURCE 0x2cba0004u
#define CHARGE_SOURCE 0x2cba0010u
#define OTHER_CHARGE_SOURCE 0x2cba0014u
#define MASK_ROOT 0x2000a000u
#define MASK_IMAGE 0x2000b000u
#define MASK_BUFFER 0x20009000u
#define MASK_ALLOCATION 0x20060001u
#define MASK_PIXELS 0x20060040u
#define MASK_BYTES (64u * 36u * 4u)
struct object { uint32_t id, parent, words[26], extra; unsigned live; };
static struct object objects[7];
static uint32_t stock_class[9], buffer_words[7], mask_buffer_words[7], children[17];
static uint32_t root_slot, status_slot, child_count;
static unsigned char image[PIXEL_CAPACITY], mask_image[MASK_BYTES];
static unsigned mask_enabled, mask_alive, mask_info_calls, mask_captures;
static unsigned mask_allocations, mask_destroys, mask_root_creates, mask_root_deletes;
static unsigned mask_child_creates, mask_set_sources, mask_sizes, mask_invalidates;
static unsigned mask_bad_dimensions, mask_nonblack, mask_snapshot_fail, mask_info_fail;
static unsigned mask_allocation_fail, mask_root_fail, mask_bad_image_metadata, mask_style_fault;
static unsigned expecting_mask_buffer, main_premultiplied, mask_edge;
static unsigned invalidate_in_mask_capture, delete_in_mask_capture;
static uint32_t sampled_mask_source, sampled_charge_source;
static unsigned charge_enabled, charge_info_calls, charge_captures, charge_allocations;
static unsigned charge_destroys, charge_invalidates, charge_root_creates, charge_root_deletes;
static unsigned charge_bad_dimensions, charge_bad_image_metadata, charge_snapshot_fail, charge_info_fail;
static unsigned charge_allocation_fail, charge_root_fail, charge_cached_width, charge_cached_height;
static unsigned invalidate_in_charge_capture, delete_in_charge_capture, mutate_in_charge_capture;
static unsigned aux_kind, charge_partial_alpha;
static unsigned char captured_charge_pixel[4];
static unsigned idle_ok, fence_ok, fence_fail_after, allocation_fail, snapshot_fail, alive;
static unsigned captures, allocations, destroys, fences, cleans, invalidates;
static unsigned native_events[64], image_draws, init_calls, live_calls, reads;
static unsigned forbid_reads, forbid_native_work, change_source_dims, delete_in_capture;
static unsigned malformed, change_capture_geometry, invalidate_in_capture;
static unsigned power_depth, power_stays, power_relaxes, delete_in_power;
static int p67_wake_pending;
static uint32_t draw_layer, last_descriptor[28];
static int32_t draw_area[4];
static uint32_t layer_clip[4];
static char operations[256];
static unsigned operation_count;
static uintptr_t native(uint32_t address);
static uint32_t *word_at(uint32_t address);
static unsigned char *byte_at(uint32_t address);
static uint32_t load32(uint32_t address);
static int p67_live(uint32_t obj);
static int p67_blur_native_ram(uint32_t address, uint32_t size);
static int p67_blur_native_idle(void);
static int p67_blur_native_fence(void);
static void p67_blur_native_power_begin(void);
static void p67_blur_native_power_end(void);
static void p67_blur_native_clean(void);
static void p67_blur_native_invalidate(uint32_t address, uint32_t size);
static void *pixels_at(uint32_t address);
#define P67_NATIVE(address) native(address)
#define P67_U32(address) (*word_at(address))
#define P67_U8(address) (*byte_at(address))
#define P67_BATTERY_TEST_PIXELS(address) pixels_at(address)
static unsigned diag_writes;
static char diag_text[2048];
static int diag_write(const char *data, uint32_t size) {
    assert(!forbid_native_work && size < sizeof(diag_text));
    memcpy(diag_text, data, size); diag_text[size] = 0; ++diag_writes; return 1;
}
#define P67_BATTERY_DIAG_WRITE(data,size) diag_write(data,size)
#include "../src/p67_battery.inc"

static void op(char value) {
    assert(operation_count < sizeof(operations)); operations[operation_count++] = value;
}
static struct object *object(uint32_t id) {
    for (unsigned i = 0; i < 7; ++i) if (objects[i].id == id) return &objects[i];
    assert(!"unknown object"); return NULL;
}
static uint32_t *word_at(uint32_t address) {
    assert(!forbid_reads); ++reads;
    for (unsigned i = 0; i < 7; ++i) {
        struct object *obj = &objects[i];
        if (address >= obj->id && address < obj->id + sizeof(obj->words)) {
            assert(obj->live && !((address - obj->id) & 3u));
            return &obj->words[(address - obj->id) / 4u];
        }
    }
    if (address >= STOCK_CLASS && address < STOCK_CLASS + sizeof(stock_class))
        return &stock_class[(address - STOCK_CLASS) / 4u];
    if (address >= BUFFER && address < BUFFER + sizeof(buffer_words)) {
        assert(alive); return &buffer_words[(address - BUFFER) / 4u];
    }
    if (address >= MASK_BUFFER && address < MASK_BUFFER + sizeof(mask_buffer_words)) {
        assert(mask_alive); return &mask_buffer_words[(address - MASK_BUFFER) / 4u];
    }
    /* The injected object header holds a truncated module RAM class address,
     * just as the native 32-bit target does. Resolve it without dereferencing
     * that truncated pointer on the 64-bit host. */
    uint32_t shim = p67_battery_shim();
    if (address >= shim && address - shim < sizeof(p67_battery_class))
        return &p67_battery_class[(address - shim) / 4u];
    assert(!"unsupported word read: no layer/framebuffer seam exists"); return NULL;
}
static unsigned char *byte_at(uint32_t address) {
    assert(!forbid_reads); ++reads;
    for (unsigned i = 0; i < 7; ++i) {
        struct object *obj = &objects[i];
        if (address >= obj->id && address < obj->id + sizeof(obj->words)) {
            assert(obj->live); return (unsigned char *)obj->words + address - obj->id;
        }
    }
    assert(!"unsupported byte read"); return NULL;
}
static uint32_t load32(uint32_t address) {
    assert(!forbid_reads);
    if (address == P67_BATTERY_ROOT_SLOT) return root_slot;
    if (address == P67_BATTERY_STATUS_SLOT) return status_slot;
    return *word_at(address);
}
static int p67_live(uint32_t id) {
    assert(!forbid_reads); ++live_calls;
    if (!id) return 0;
    return object(id)->live != 0;
}
static int p67_blur_native_ram(uint32_t address, uint32_t size) {
    if (address == BUFFER) return alive && size <= sizeof(buffer_words);
    if (address == MASK_BUFFER) return mask_alive && size <= sizeof(mask_buffer_words);
    if (address >= MASK_ALLOCATION && address <= MASK_PIXELS)
        return mask_alive && size <= MASK_BYTES + MASK_PIXELS - address;
    return alive && address >= ALLOCATION && address <= PIXELS &&
           size <= PIXEL_CAPACITY + PIXELS - address;
}
static int p67_blur_native_idle(void) { assert(!forbid_native_work); return idle_ok != 0; }
static int p67_blur_native_fence(void) {
    assert(!forbid_native_work); ++fences; op('F');
    if (fence_fail_after && fences >= fence_fail_after) return 0;
    return fence_ok != 0;
}
static void p67_blur_native_power_begin(void) {
    assert(!forbid_native_work); ++power_depth; ++power_stays;
    if (delete_in_power && p67_battery_obj) {
        uint32_t id = p67_battery_obj, event[5] = {id, id, 36, 0, 0};
        forbid_reads = 1; p67_battery_event(p67_battery_shim(), event); forbid_reads = 0;
        object(id)->live = 0; child_count = 0;
    }
}
static void p67_blur_native_power_end(void) {
    assert(!forbid_native_work && power_depth && p67_battery_busy);
    --power_depth; ++power_relaxes;
}
static void p67_blur_native_clean(void) { assert(!forbid_native_work); ++cleans; op('C'); }
static void p67_blur_native_invalidate(uint32_t address, uint32_t size) {
    assert(!forbid_native_work);
    if (address == MASK_PIXELS) {
        assert(mask_alive && size == mask_buffer_words[3]);
        if (aux_kind == 2) ++charge_invalidates; else ++mask_invalidates;
    } else { assert(address == PIXELS && size == buffer_words[3]); ++invalidates; }
    assert(operation_count && operations[operation_count - 1] == 'C');
    op('I');
}
static void *pixels_at(uint32_t address) {
    if (address == MASK_PIXELS) { assert(mask_alive); return mask_image; }
    assert(address == PIXELS && alive); return image;
}
static uint32_t parent(uint32_t id) { assert(!forbid_reads && object(id)->live); return object(id)->parent; }
static uint32_t count(uint32_t id) { assert(id == STATUS && object(id)->live); return child_count; }
static uint32_t child(uint32_t id, int32_t index) {
    assert(id == STATUS && index >= 0 && (uint32_t)index < child_count && index < 17);
    return children[index];
}
static uint32_t extra(uint32_t id) { assert(object(id)->live); return object(id)->extra; }
static void stock_event(uint32_t clz, void *event) {
    unsigned code = *(uint16_t *)((unsigned char *)event + 8) & 0x7fffu;
    assert(clz == STOCK_CLASS && code < 64u); ++native_events[code];
}
static void dispatch(uint32_t id, unsigned code, uint32_t layer) {
    uint32_t event[5] = { id, id, code, 0, layer };
    if (object(id)->words[0] == p67_battery_shim()) p67_battery_event(p67_battery_shim(), event);
    else stock_event(STOCK_CLASS, event);
}
static void image_init(void *data) {
    memset(data, 0, 28u * sizeof(uint32_t)); ++init_calls;
    ((uint32_t *)data)[5] = 112u;
    ((uint32_t *)data)[12] = ((uint32_t *)data)[13] = 256u;
}
static void image_submit(uint32_t layer, const void *descriptor, const int32_t *area) {
    assert(layer == LAYER); ++image_draws; draw_layer = layer;
    memcpy(last_descriptor, descriptor, sizeof(last_descriptor));
    memcpy(draw_area, area, sizeof(draw_area));
    assert(last_descriptor[7] == BUFFER && p67_battery_ready && !p67_battery_busy);
    assert(last_descriptor[5] == 112u && last_descriptor[12] == 256u && last_descriptor[13] == 256u);
}
static int mask_info(uint32_t source, void *out) {
    assert(!forbid_native_work); expecting_mask_buffer = 0;
    uint32_t *header = out;
    if (source == CHARGE_SOURCE || source == OTHER_CHARGE_SOURCE) {
        ++charge_info_calls;
        assert(captures && object(BATTERY)->words[15] == charge_cached_width &&
               object(BATTERY)->words[16] == charge_cached_height);
        if (!charge_enabled || charge_info_fail) return 0;
        header[0] = 25u | (10u << 8);
        header[1] = (48u << 16) | (charge_bad_dimensions ? 15u : 16u);
        header[2] = 16u;
        expecting_mask_buffer = charge_bad_dimensions ? 0 : 2;
        return 1;
    }
    ++mask_info_calls;
    if (!mask_enabled || mask_info_fail || (source != MASK_SOURCE && source != OTHER_MASK_SOURCE)) return 0;
    header[0] = 25u | (10u << 8);
    header[1] = (36u << 16) | (mask_bad_dimensions ? 63u : 64u);
    header[2] = 64u;
    expecting_mask_buffer = !mask_bad_dimensions;
    return 1;
}
static uint32_t mask_style_get(uint32_t id, uint32_t part, uint32_t property) {
    assert(!forbid_native_work && id == BATTERY && object(id)->live && part == 0);
    assert(property == 68 || property == 70 || property == 104 || property == 105);
    if (property == 68) return mask_style_fault == property ? 128 : 255;
    return mask_style_fault == property ? 1 : 0;
}
static uint32_t mask_child_create(uint32_t parent_id, const unsigned char *options) {
    assert(!forbid_native_work && options && *options == 0);
    if (!parent_id) {
        if (aux_kind == 2) { ++charge_root_creates; if (charge_root_fail) return 0; }
        else { ++mask_root_creates; if (mask_root_fail) return 0; }
        object(MASK_ROOT)->live = 1; object(MASK_ROOT)->parent = 0;
        object(MASK_ROOT)->words[9] = 335; object(MASK_ROOT)->words[10] = 479;
        return MASK_ROOT;
    }
    assert(parent_id == MASK_ROOT && object(MASK_ROOT)->live && !object(MASK_IMAGE)->live);
    ++mask_child_creates;
    object(MASK_IMAGE)->live = 1; object(MASK_IMAGE)->parent = parent_id;
    return MASK_IMAGE;
}
static void mask_source_set(uint32_t id, uint32_t source) {
    assert(!forbid_native_work && id == MASK_IMAGE && object(id)->live);
    object(id)->words[13] = source;
    if (aux_kind == 2) {
        assert(source == CHARGE_SOURCE || source == OTHER_CHARGE_SOURCE);
        sampled_charge_source = source;
        object(id)->words[17] = charge_bad_image_metadata ? 15 : 16;
        object(id)->words[18] = 48;
    } else {
        assert(source == MASK_SOURCE || source == OTHER_MASK_SOURCE);
        sampled_mask_source = source; ++mask_set_sources;
        object(id)->words[17] = mask_bad_image_metadata ? 63 : 64;
        object(id)->words[18] = 36;
    }
}
static void mask_size_set(uint32_t id, uint32_t width, uint32_t height) {
    assert(!forbid_native_work && id == MASK_IMAGE && object(id)->live);
    if (aux_kind == 2) assert(width == 16 && height == 48);
    else { assert(width == 64 && height == 36); ++mask_sizes; }
    /* Actual firmware refuses to resize a parentless screen. */
    if (object(id)->parent) {
        object(id)->words[7] = object(id)->words[8] = 0;
        object(id)->words[9] = width - 1; object(id)->words[10] = height - 1;
    }
}
static void mask_root_delete(uint32_t id) {
    assert(!forbid_native_work && id == MASK_ROOT && object(id)->live && idle_ok && fence_ok);
    object(MASK_ROOT)->live = 0; object(MASK_IMAGE)->live = 0;
    if (aux_kind == 2) ++charge_root_deletes; else ++mask_root_deletes;
}
static unsigned mask_alpha(unsigned x, unsigned y) {
    if ((x == 1 && y >= 1 && y <= 4) || (x == 26 && y == 1)) return mask_edge;
    if (x >= 2 && x < 62 && y >= 2 && y < 34) return 0;
    return 255;
}
static void render_masked_battery(uint32_t id) {
    unsigned width = buffer_words[1] & 0xffffu, height = buffer_words[1] >> 16;
    for (unsigned y = 0; y < height; ++y) memset(image + y * buffer_words[2], 0, width * 4u);
    int32_t origin_x = (int32_t)object(id)->extra - 2;
    int32_t origin_y = (int32_t)object(id)->extra - 1;
    for (unsigned y = 0; y < 36; ++y) for (unsigned x = 0; x < 64; ++x) {
        int32_t dx = origin_x + (int32_t)x, dy = origin_y + (int32_t)y;
        if (dx < 0 || dy < 0 || (uint32_t)dx >= width || (uint32_t)dy >= height) continue;
        unsigned char *p = image + (unsigned)dy * buffer_words[2] + 4u * (unsigned)dx;
        unsigned a = mask_alpha(x, y), color = 255u - a;
        p[0] = p[1] = p[2] = (unsigned char)color; p[3] = 255;
        if (x == 1 && y == 2) p[0] = p[1] = 0; /* Red fill edge. */
        if (x == 3 && y == 3) p[0] = p[1] = p[2] = 128; /* Intentional gray. */
        if ((x == 4 && y == 3) || (x == 28 && y == 2))
            p[0] = p[1] = p[2] = 0; /* Black percentage glyph. */
        if (x == 27 && y == 3) p[0] = p[1] = p[2] = 128; /* Gray under a transparent charger pixel. */
        if (x == 1 && y == 4) p[0] = p[1] = p[2] = (unsigned char)(255u - a / 2u);
        /* Native blit consumes body.height+1 rows, not the source's last
         * three rows. Unrelated pixels there must not be clipped by the mask. */
        if (y >= object(id)->words[10] - object(id)->words[8] + 2u) {
            if (!x) { p[0] = p[1] = p[2] = 0; p[3] = 255; }
            else { p[0] = 23; p[1] = 44; p[2] = 66; p[3] = 128; }
        }
    }
}
static unsigned charge_alpha(unsigned x, unsigned y) {
    const unsigned alpha[4] = {0, charge_partial_alpha, 255, 64};
    unsigned phase = y == 7 || y == 8 ? 0 : y % 4;
    return alpha[(x + phase) % 4];
}
static unsigned div255(unsigned value) { return (value + 127u) / 255u; }
static void render_charging_image(uint32_t id) {
    unsigned flags = *byte_at(id + 101u);
    if (!charge_enabled || !(flags & 1u) || !object(id)->words[14]) return;
    int32_t w = (int32_t)(object(id)->words[9] - object(id)->words[7] + 1u);
    int32_t h = (int32_t)(object(id)->words[10] - object(id)->words[8] + 1u);
    int32_t x0 = (int32_t)object(id)->extra + ((flags & 4u) ? (w - 16) / 2 : w + 1);
    int32_t y0 = (int32_t)object(id)->extra + (h - 48) / 2;
    unsigned width = buffer_words[1] & 0xffffu, height = buffer_words[1] >> 16;
    for (unsigned y = 0; y < 48; ++y) for (unsigned x = 0; x < 16; ++x) {
        int32_t dx = x0 + (int32_t)x, dy = y0 + (int32_t)y;
        if (dx < 0 || dy < 0 || (uint32_t)dx >= width || (uint32_t)dy >= height) continue;
        unsigned char *p = image + (unsigned)dy * buffer_words[2] + 4u * (unsigned)dx;
        unsigned a = charge_alpha(x, y), old_alpha = p[3], alpha = a + div255(old_alpha * (255 - a));
        for (unsigned c = 0; c < 3; ++c) {
            unsigned old_pm = main_premultiplied ? p[c] : div255(p[c] * old_alpha);
            unsigned pm = a + div255(old_pm * (255 - a));
            p[c] = (unsigned char)(main_premultiplied || !alpha ? pm : (pm * 255u + alpha / 2u) / alpha);
        }
        p[3] = (unsigned char)alpha;
    }
    if ((flags & 5u) == 5u) {
        const unsigned char *p = image + (object(id)->extra - 1u) * buffer_words[2] +
                                 4u * (object(id)->extra - 2u + 26u);
        memcpy(captured_charge_pixel, p, sizeof(captured_charge_pixel));
    }
}
static uint32_t create(uint32_t width, uint32_t height, uint32_t format, uint32_t stride) {
    if (expecting_mask_buffer) {
        aux_kind = expecting_mask_buffer; expecting_mask_buffer = 0;
        assert(!forbid_native_work && !mask_alive && format == 16 && !stride);
        if (aux_kind == 2) {
            assert(width == 16 && height == 48); ++charge_allocations;
            if (charge_allocation_fail) return 0;
        } else {
            assert(width == 64 && height == 36); ++mask_allocations;
            if (mask_allocation_fail) return 0;
        }
        op('a'); mask_alive = 1;
        mask_buffer_words[0] = 25u | (16u << 8) | (0x31u << 16);
        mask_buffer_words[1] = width | height << 16; mask_buffer_words[2] = width * 4;
        mask_buffer_words[3] = width * height * 4; mask_buffer_words[4] = MASK_PIXELS;
        mask_buffer_words[5] = MASK_ALLOCATION; mask_buffer_words[6] = 0x20103244u;
        memset(mask_image, 0xa5, sizeof(mask_image));
        return MASK_BUFFER;
    }
    assert(!forbid_native_work && !alive && format == 16u && stride == 0u);
    assert(width && width <= 128 && height && height <= 128);
    ++allocations; op('A');
    if (allocation_fail) return 0;
    alive = 1;
    buffer_words[0] = 25u | (16u << 8) | (0x30u << 16);
    buffer_words[1] = width | (height << 16);
    buffer_words[2] = (width * 4u + 15u) & ~15u;
    buffer_words[3] = buffer_words[2] * height;
    buffer_words[4] = PIXELS; buffer_words[5] = ALLOCATION; buffer_words[6] = 0x20103244u;
    assert(buffer_words[3] <= sizeof(image)); memset(image, 0xa5, sizeof(image));
    switch (malformed) {
    case 0: break;
    case 1: buffer_words[0] ^= 1u; break;
    case 2: buffer_words[1] ^= 1u; break;
    case 3: buffer_words[2] = width * 4u - 1u; break;
    case 4: buffer_words[3] = buffer_words[2] * height - 1u; break;
    case 5: buffer_words[4] = PIXELS + 1u; break;
    case 6: buffer_words[5] = PIXELS + 1u; break;
    case 7: buffer_words[6] = 0; break;
    case 8: buffer_words[3] = PIXEL_CAPACITY + 1u; break;
    default: assert(!"unknown malformed mode");
    }
    return BUFFER;
}
static void destroy(uint32_t buffer) {
    if (buffer == MASK_BUFFER) {
        assert(!forbid_native_work && mask_alive && idle_ok && fence_ok);
        mask_alive = 0;
        if (aux_kind == 2) ++charge_destroys; else ++mask_destroys;
        op('d'); return;
    }
    assert(!forbid_native_work && buffer == BUFFER && alive && idle_ok && fence_ok);
    alive = 0; ++destroys; op('D');
}
static int capture(uint32_t id, uint32_t format, uint32_t buffer) {
    if (id == MASK_IMAGE) {
        assert(!forbid_native_work && object(id)->live && object(id)->parent == MASK_ROOT);
        assert(object(id)->words[9] - object(id)->words[7] + 1u == (mask_buffer_words[1] & 0xffffu));
        assert(object(id)->words[10] - object(id)->words[8] + 1u == (mask_buffer_words[1] >> 16));
        assert(buffer == MASK_BUFFER && mask_alive && format == 16 && p67_battery_busy);
        unsigned width = mask_buffer_words[1] & 0xffffu, height = mask_buffer_words[1] >> 16;
        assert(object(id)->words[9] == width - 1 && object(id)->words[10] == height - 1);
        if (aux_kind == 2) ++charge_captures; else ++mask_captures;
        op('s');
        for (unsigned y = 0; y < height; ++y) for (unsigned x = 0; x < width; ++x) {
            unsigned char *p = mask_image + (y * width + x) * 4;
            unsigned a = aux_kind == 2 ? charge_alpha(x, y) : mask_alpha(x, y);
            p[0] = p[1] = p[2] = aux_kind == 2 ? (unsigned char)a : 0;
            p[3] = (unsigned char)a;
        }
        if (aux_kind == 2) {
            if (invalidate_in_charge_capture) p67_battery_invalidate();
            if (mutate_in_charge_capture) ++*byte_at(BATTERY + 80u);
            if (delete_in_charge_capture) {
                forbid_reads = forbid_native_work = 1; dispatch(BATTERY, 36, 0);
                forbid_reads = forbid_native_work = 0; object(BATTERY)->live = 0; child_count = 0;
            }
            return charge_snapshot_fail ? 0 : 1;
        }
        if (mask_nonblack) mask_image[0] = 127;
        if (invalidate_in_mask_capture) p67_battery_invalidate();
        if (delete_in_mask_capture) {
            forbid_reads = forbid_native_work = 1; dispatch(BATTERY, 36, 0);
            forbid_reads = forbid_native_work = 0; object(BATTERY)->live = 0; child_count = 0;
        }
        return mask_snapshot_fail ? 0 : 1;
    }
    assert(!forbid_native_work && object(id)->live && format == 16u && buffer == BUFFER && alive);
    assert(p67_battery_busy && !p67_battery_ready); ++captures; op('S');
    unsigned before = native_events[24], draws = image_draws;
    dispatch(id, 24, LAYER);
    assert(native_events[24] == before + 1 && image_draws == draws);
    unsigned width = buffer_words[1] & 0xffffu, height = buffer_words[1] >> 16;
    for (unsigned y = 0; y < height; ++y) {
        unsigned char *row = image + y * buffer_words[2];
        for (unsigned x = 0; x < width; ++x) {
            row[4*x] = row[4*x+1] = row[4*x+2] = 0; row[4*x+3] = 255;
        }
        /* White backing, colored charge and nonblack antialias pixels. */
        row[4] = row[5] = row[6] = row[7] = 255;
        row[8] = row[9] = 0; row[10] = row[11] = 255;
        row[12] = row[13] = row[14] = 1; row[15] = 127;
    }
    if (change_source_dims) {
        object(id)->words[15] = charge_cached_width; object(id)->words[16] = charge_cached_height;
    }
    if (mask_enabled) render_masked_battery(id);
    render_charging_image(id);
    buffer_words[0] = (buffer_words[0] & ~(1u << 16)) | (main_premultiplied << 16);
    if (change_capture_geometry) ++object(id)->words[9];
    if (delete_in_capture) {
        forbid_reads = forbid_native_work = 1; dispatch(id, 36, 0);
        forbid_reads = forbid_native_work = 0; object(id)->live = 0; child_count = 0;
    }
    if (invalidate_in_capture) p67_battery_invalidate();
    return snapshot_fail ? 0 : 1;
}
static uintptr_t native(uint32_t address) {
    switch (address) {
    case 0x0c589579u: return (uintptr_t)parent;
    case 0x0c5881d9u: return (uintptr_t)count;
    case 0x0c5886e1u: return (uintptr_t)child;
    case 0x0c587e71u: return (uintptr_t)extra;
    case P67_BATTERY_EVENT: return (uintptr_t)stock_event;
    case 0x0c5882b9u: return (uintptr_t)image_init;
    case 0x0c5882a1u: return (uintptr_t)image_submit;
    case 0x0c587f39u: return (uintptr_t)create;
    case 0x0cac0db9u: return (uintptr_t)destroy;
    case 0x0ca5fb41u: return (uintptr_t)capture;
    case RH_FW_IMAGE_GET_INFO: return (uintptr_t)mask_info;
    case 0x0c1068a9u: return (uintptr_t)mask_style_get;
    case 0x0c588d59u: return (uintptr_t)mask_child_create;
    case 0x0c587fa1u: return (uintptr_t)mask_source_set;
    case 0x0c588629u: return (uintptr_t)mask_size_set;
    case 0x0c588de9u: return (uintptr_t)mask_root_delete;
    default: assert(!"unexpected native API"); return 0;
    }
}
static void geometry(uint32_t id, int32_t x, int32_t y, unsigned width, unsigned height, unsigned padding) {
    struct object *obj = object(id);
    obj->words[7] = (uint32_t)x; obj->words[8] = (uint32_t)y;
    obj->words[9] = (uint32_t)(x + (int32_t)width - 1);
    obj->words[10] = (uint32_t)(y + (int32_t)height - 1); obj->extra = padding;
}
static void reset(void) {
    const uint32_t ids[7] = {ROOT, STATUS, BATTERY, OTHER, DUPLICATE, MASK_ROOT, MASK_IMAGE};
    memset(objects, 0, sizeof(objects));
    for (unsigned i = 0; i < 7; ++i) { objects[i].id = ids[i]; objects[i].live = i < 5; }
    object(STATUS)->parent = ROOT;
    object(BATTERY)->parent = object(OTHER)->parent = object(DUPLICATE)->parent = STATUS;
    object(BATTERY)->words[0] = object(DUPLICATE)->words[0] = STOCK_CLASS;
    object(OTHER)->words[0] = 0x2cce61ecu;
    geometry(BATTERY, 100, 20, 62, 32, 30);
    geometry(DUPLICATE, 0, 0, 62, 32, 30);
    object(BATTERY)->words[15] = object(BATTERY)->words[16] = UINT32_MAX;
    object(BATTERY)->words[20] = 50;
    for (unsigned i = 0; i < 9; ++i) stock_class[i] = 0x100u + i;
    stock_class[0] = 0x2cce0308u; stock_class[3] = P67_BATTERY_EVENT;
    stock_class[6] = 62; stock_class[7] = 32; stock_class[8] = 0x680;
    root_slot = ROOT; status_slot = STATUS; child_count = 3;
    children[0] = OTHER; children[1] = BATTERY; children[2] = 0;
    idle_ok = fence_ok = 1;
    alive = fence_fail_after = allocation_fail = snapshot_fail = malformed = change_capture_geometry = 0;
    captures = allocations = destroys = fences = cleans = invalidates = 0;
    image_draws = init_calls = live_calls = reads = 0;
    forbid_reads = forbid_native_work = delete_in_capture = invalidate_in_capture = 0;
    change_source_dims = 1; operation_count = 0;
    power_depth = power_stays = power_relaxes = delete_in_power = 0;
    mask_enabled = mask_alive = mask_info_calls = mask_captures = 0;
    mask_allocations = mask_destroys = mask_root_creates = mask_root_deletes = 0;
    mask_child_creates = mask_set_sources = mask_sizes = mask_invalidates = 0;
    mask_bad_dimensions = mask_nonblack = mask_snapshot_fail = mask_info_fail = 0;
    mask_allocation_fail = mask_root_fail = mask_bad_image_metadata = mask_style_fault = 0;
    expecting_mask_buffer = main_premultiplied = 0; mask_edge = 128;
    invalidate_in_mask_capture = delete_in_mask_capture = 0; sampled_mask_source = 0;
    charge_enabled = charge_info_calls = charge_captures = charge_allocations = 0;
    charge_destroys = charge_invalidates = charge_root_creates = charge_root_deletes = 0;
    charge_bad_dimensions = charge_bad_image_metadata = charge_snapshot_fail = charge_info_fail = 0;
    charge_allocation_fail = charge_root_fail = 0;
    charge_cached_width = 16; charge_cached_height = 48; charge_partial_alpha = 128;
    invalidate_in_charge_capture = delete_in_charge_capture = mutate_in_charge_capture = 0;
    sampled_charge_source = aux_kind = 0; memset(captured_charge_pixel, 0, sizeof(captured_charge_pixel));
    p67_battery_charge_source = p67_battery_charge_ready = 0;
    memset(p67_battery_charge, 0, sizeof(p67_battery_charge));
    p67_battery_mask_source = p67_battery_mask_buffer = p67_battery_mask_root = 0;
    p67_battery_mask_ready = 0; memset(p67_battery_mask, 0, sizeof(p67_battery_mask));
    memset(native_events, 0, sizeof(native_events));
    memset(p67_battery_class, 0, sizeof(p67_battery_class));
    memset(&p67_battery_cached, 0, sizeof(p67_battery_cached));
    p67_battery_obj = p67_battery_root = p67_battery_status = 0;
    p67_battery_generation = p67_battery_buffer = 0;
    p67_battery_busy = p67_battery_ready = 0; p67_wake_pending = 0;
    p67_battery_diag_mode = p67_battery_diag_reason = p67_battery_diag_kind = 0;
    memset(p67_battery_sample_diag, 0, sizeof(p67_battery_sample_diag));
    diag_writes = 0; diag_text[0] = 0;
    layer_clip[0] = 0; layer_clip[1] = 0; layer_clip[2] = 335; layer_clip[3] = 479;
}
static void ready(void) {
    p67_battery_poll(1);
    assert(p67_battery_ready && alive && p67_battery_obj == BATTERY && p67_wake_pending);
    assert(!power_depth && power_stays == power_relaxes);
}
static void draw_without_native_work(unsigned code) {
    unsigned f = fences, a = allocations, s = captures, l = live_calls;
    unsigned ma = mask_allocations, ms = mask_captures, md = mask_destroys, rd = mask_root_deletes;
    unsigned ca = charge_allocations, cs = charge_captures, cd = charge_destroys, crd = charge_root_deletes;
    uint32_t clip[4]; memcpy(clip, layer_clip, sizeof(clip));
    forbid_native_work = 1; dispatch(BATTERY, code, LAYER); forbid_native_work = 0;
    assert(f == fences && a == allocations && s == captures && l == live_calls);
    assert(ma == mask_allocations && ms == mask_captures && md == mask_destroys && rd == mask_root_deletes);
    assert(ca == charge_allocations && cs == charge_captures && cd == charge_destroys && crd == charge_root_deletes);
    assert(!memcmp(clip, layer_clip, sizeof(clip)));
}
static void enable_mask(unsigned premultiplied) {
    mask_enabled = 1; main_premultiplied = premultiplied;
    object(BATTERY)->words[13] = MASK_SOURCE;
}
static unsigned char *mask_pixel(unsigned x, unsigned y) {
    return image + ((object(BATTERY)->extra - 1u) + y) * buffer_words[2] +
           ((object(BATTERY)->extra - 2u) + x) * 4u;
}
static void assert_edge(unsigned alpha, unsigned premultiplied) {
    const unsigned char *white = mask_pixel(1, 1), *red = mask_pixel(1, 2);
    unsigned color = premultiplied ? alpha : 255;
    assert(white[0] == color && white[1] == color && white[2] == color && white[3] == alpha);
    assert(!red[0] && !red[1] && red[2] == color && red[3] == alpha);
}
static void test_mask_edge_and_cache(void) {
    for (unsigned premultiplied = 0; premultiplied < 2; ++premultiplied) {
        reset(); enable_mask(premultiplied); ready();
        assert(p67_battery_mask_ready && p67_battery_mask_source == MASK_SOURCE);
        assert(mask_info_calls == 1 && mask_captures == 1 && captures == 1);
        assert(mask_allocations == 1 && allocations == 1 && mask_destroys == 1 && !destroys);
        assert(mask_root_creates == 1 && mask_root_deletes == 1 && mask_set_sources == 1 && mask_sizes == 1);
        assert(sampled_mask_source == MASK_SOURCE && mask_invalidates == 1 && invalidates == 1);
        assert(!mask_alive && !object(MASK_IMAGE)->live && !p67_battery_mask_root && !p67_battery_mask_buffer);
        assert(!!(buffer_words[0] & (1u << 16)) == premultiplied);
        assert_edge(127, premultiplied);
        const unsigned char *gray = mask_pixel(3, 3), *black = mask_pixel(4, 3);
        assert(gray[0] == 128 && gray[1] == 128 && gray[2] == 128 && gray[3] == 255);
        assert(!black[0] && !black[1] && !black[2] && black[3] == 255);
        const unsigned char *outside = mask_pixel(0, 0), *overlay = mask_pixel(1, 4);
        assert(!outside[0] && !outside[1] && !outside[2] && !outside[3]);
        assert(overlay[0] == 191 && overlay[1] == 191 && overlay[2] == 191 && overlay[3] == 255);
        /* Only stock64x33 is blitted. The last three source rows must not
         * erase unrelated charging pixels below the native mask rectangle. */
        for (unsigned y = 33; y < 36; ++y) {
            const unsigned char *last = mask_pixel(0, y), *colored = mask_pixel(1, y);
            /* Black makes this detect an incorrect36-row unmatte even when
             * the conservative overlay guard would preserve colored pixels. */
            assert(!last[0] && !last[1] && !last[2] && last[3] == 255);
            assert(colored[0] == 23 && colored[1] == 44 && colored[2] == 66 && colored[3] == 128);
        }
        unsigned f = fences;
        draw_without_native_work(24); assert(image_draws == 1);
        geometry(BATTERY, -200, -15, 62, 32, 30);
        draw_without_native_work(24);
        assert(draw_area[0] == -230 && draw_area[1] == -45);
        p67_battery_poll(1);
        assert(f == fences && captures == 1 && mask_captures == 1 && mask_info_calls == 1);
        ++*byte_at(BATTERY + 80); ready();
        assert(captures == 2 && mask_captures == 1 && mask_allocations == 1 && allocations == 1);
        assert_edge(127, premultiplied);
    }
}
static void test_mask_mapping_and_invalidation(void) {
    reset(); enable_mask(0); ready();
    unsigned f = fences, d = mask_destroys, rd = mask_root_deletes, r = reads;
    forbid_reads = forbid_native_work = 1; p67_battery_invalidate();
    forbid_reads = forbid_native_work = 0;
    assert(!p67_battery_mask_ready && !p67_battery_mask_source && !p67_battery_ready);
    assert(f == fences && d == mask_destroys && rd == mask_root_deletes && r == reads);
    mask_edge = 64; ready();
    assert_edge(191, 0);
    assert(mask_captures == 2 && captures == 2 && mask_info_calls == 2 && allocations == 1);
    object(BATTERY)->words[13] = OTHER_MASK_SOURCE;
    draw_without_native_work(24); assert(!image_draws);
    ready(); assert(sampled_mask_source == OTHER_MASK_SOURCE && mask_captures == 3);
    assert(p67_battery_mask_source == OTHER_MASK_SOURCE && allocations == 1);
    draw_without_native_work(45); assert(!p67_battery_mask_ready);
    ready(); assert(mask_captures == 4 && captures == 4 && allocations == 1);
}
static void enable_charge(unsigned premultiplied, int centered) {
    enable_mask(premultiplied); charge_enabled = 1;
    object(BATTERY)->words[14] = CHARGE_SOURCE;
    *byte_at(BATTERY + 101u) = centered ? 5 : 1;
}
static void assert_charged_edges(unsigned premultiplied, unsigned partial) {
    const unsigned char *transparent = mask_pixel(25, 0), *half = mask_pixel(26, 0);
    const unsigned char *opaque = mask_pixel(27, 0), *low = mask_pixel(28, 0);
    assert(!transparent[0] && !transparent[1] && !transparent[2] && !transparent[3]);
    unsigned color = premultiplied ? partial : 255;
    assert(half[0] == color && half[1] == color && half[2] == color && half[3] == partial);
    assert(opaque[0] == 255 && opaque[1] == 255 && opaque[2] == 255 && opaque[3] == 255);
    color = premultiplied ? 64 : 255;
    assert(low[0] == color && low[1] == color && low[2] == color && low[3] == 64);
    const unsigned char *mixed = mask_pixel(26, 1);
    unsigned coverage = 255 - (mask_edge * (255 - partial) + 127) / 255;
    color = premultiplied ? coverage : 255;
    assert(mixed[0] == color && mixed[1] == color && mixed[2] == color && mixed[3] == coverage);
    const unsigned char *black = mask_pixel(28, 2), *gray = mask_pixel(27, 3);
    assert(!charge_alpha(3, 9) && !charge_alpha(2, 10));
    assert(!black[0] && !black[1] && !black[2] && black[3] == 255);
    assert(gray[0] == 128 && gray[1] == 128 && gray[2] == 128 && gray[3] == 255);
}
static void test_centered_charge_edges_and_cache(void) {
    for (unsigned premultiplied = 0; premultiplied < 2; ++premultiplied) {
        reset(); enable_charge(premultiplied, 1); ready();
        assert(p67_battery_mask_ready && p67_battery_charge_ready);
        assert(p67_battery_charge_source == CHARGE_SOURCE && sampled_charge_source == CHARGE_SOURCE);
        assert(mask_captures == 1 && charge_captures == 1 && captures == 1 && allocations == 1);
        assert(charge_allocations == 1 && charge_destroys == 1 && charge_invalidates == 1);
        assert(charge_root_creates == 1 && charge_root_deletes == 1 && !mask_alive);
        assert_charged_edges(premultiplied, 128);
        assert(captured_charge_pixel[0] == 128 && captured_charge_pixel[3] == 255);
        if (premultiplied) assert(!memcmp(mask_pixel(26, 0), captured_charge_pixel, 3));
        unsigned f = fences;
        draw_without_native_work(24); p67_battery_poll(1);
        assert(f == fences && charge_captures == 1 && mask_captures == 1 && captures == 1);
        ++*byte_at(BATTERY + 80u); ready();
        assert(charge_captures == 1 && mask_captures == 1 && captures == 2 && allocations == 1);
        assert_charged_edges(premultiplied, 128);
        object(BATTERY)->words[14] = OTHER_CHARGE_SOURCE; ready();
        assert(charge_captures == 2 && mask_captures == 1 && captures == 3);
        assert(p67_battery_charge_source == OTHER_CHARGE_SOURCE);
        p67_battery_invalidate(); charge_partial_alpha = 64; ready();
        assert(charge_captures == 3 && mask_captures == 2 && captures == 4 && allocations == 1);
        assert_charged_edges(premultiplied, 64);
    }
    reset(); enable_charge(1, 0); ready();
    assert(!charge_captures && !charge_allocations && !p67_battery_charge_ready);
    assert_edge(127, 1); /* Right-side glyph cannot attenuate the mask. */
    const unsigned char *right = image + 22u * buffer_words[2] + 4u * 94u;
    assert(right[0] == 128 && right[3] == 128);
    reset(); enable_charge(1, 1); geometry(BATTERY, -200, -15, 13, 33, 30); ready();
    /* Signed half division truncates toward zero for negativeW-CW andH-CH.
     * This shape gives charge source(cx1,cy6) at mask(2,0), whose alpha is64. */
    assert(mask_pixel(2, 0)[0] == 64 && mask_pixel(2, 0)[3] == 64);
}
static void test_strict_failures(void) {
    reset(); enable_mask(1); mask_info_fail=1; p67_battery_poll(1);
    assert(!p67_battery_ready && p67_battery_diag_reason==P67_BAT_INFO);
    draw_without_native_work(24); assert(!native_events[24] && !image_draws);
    mask_info_fail=0; ready(); assert_edge(127,1);
    reset(); enable_mask(1); mask_style_fault=68; p67_battery_poll(1);
    assert(!p67_battery_ready && p67_battery_diag_reason==P67_BAT_STYLE);
    mask_style_fault=0; dispatch(BATTERY,45,0); ready();
    reset(); enable_mask(1); mask_bad_dimensions=1; p67_battery_poll(1);
    assert(!p67_battery_ready && !captures && p67_battery_diag_reason==P67_BAT_SOURCE_SHAPE);
    mask_bad_dimensions=0; p67_battery_invalidate(); ready();
    reset(); enable_mask(1); mask_nonblack=1; p67_battery_poll(1);
    assert(!p67_battery_ready && p67_battery_diag_reason==P67_BAT_NONBLACK);
    reset(); enable_mask(1); mask_snapshot_fail=1; p67_battery_poll(1);
    assert(!p67_battery_ready && p67_battery_diag_reason==P67_BAT_SNAPSHOT);
    mask_snapshot_fail=0; ready();
    reset(); enable_mask(1); mask_allocation_fail=1; p67_battery_poll(1);
    assert(!p67_battery_ready && p67_battery_diag_reason==P67_BAT_ALLOC);
    mask_allocation_fail=0; ready();
    reset(); enable_charge(1,1); charge_info_fail=1; p67_battery_poll(1);
    assert(!p67_battery_ready && p67_battery_diag_reason==P67_BAT_INFO);
    unsigned n=native_events[24]; draw_without_native_work(24); assert(native_events[24]==n);
    charge_info_fail=0; ready();
}
static void test_strict_lifetime(void) {
    reset(); enable_mask(1); ready();
    unsigned n=native_events[24]; ++*byte_at(BATTERY+80);
    draw_without_native_work(24); assert(native_events[24]==n && !image_draws);
    ready(); draw_without_native_work(24); assert(image_draws==1);
    unsigned f=fences,d=destroys;
    forbid_reads=forbid_native_work=1; dispatch(BATTERY,36,0);
    forbid_reads=forbid_native_work=0;
    assert(!p67_battery_obj && !p67_battery_ready && f==fences && d==destroys);
    object(BATTERY)->live=0; child_count=0; p67_battery_poll(0); assert(!alive);
    reset(); enable_mask(1); fence_fail_after=2; p67_battery_poll(1);
    assert(mask_alive && object(MASK_ROOT)->live && object(MASK_IMAGE)->live && !p67_battery_ready);
    assert(p67_battery_mask_root==MASK_ROOT && !mask_destroys);
    draw_without_native_work(24); assert(!native_events[24] && !image_draws);
    p67_battery_poll(1); assert(mask_allocations==1);
    fence_fail_after=0; ready(); assert(!mask_alive && !object(MASK_ROOT)->live);
    char text[2048],small[8]; memset(small,0x5a,sizeof(small));
    assert(!p67_battery_diag_snapshot(small,sizeof(small)));
    for(unsigned i=0;i<sizeof(small);++i) assert(small[i]==0x5a);
    assert(p67_battery_diag_snapshot(text,sizeof(text)) && strstr(text,"mode=ready"));
    assert(p67_battery_sample_diag[0].draw_w==64 && p67_battery_sample_diag[0].draw_h==36);
    assert(object(MASK_ROOT)->words[9]==335 && object(MASK_ROOT)->words[10]==479);
    /* Model the native screen-size prohibition independently of the child. */
    object(MASK_IMAGE)->live=1; object(MASK_IMAGE)->parent=0;
    object(MASK_IMAGE)->words[9]=335; object(MASK_IMAGE)->words[10]=479;
    mask_size_set(MASK_IMAGE,64,36);
    assert(object(MASK_IMAGE)->words[9]==335 && object(MASK_IMAGE)->words[10]==479);
    object(MASK_IMAGE)->live=0;
    p67_battery_poll(0); assert(object(BATTERY)->words[0]==STOCK_CLASS && !alive);
    /* Diagnostics deduplicate volatile draw counters and throttle failures. */
    p67_battery_diag_attempts=p67_battery_diag_cooldown=0;
    p67_battery_diag_logged=p67_battery_diag_recovery_used=0; diag_writes=0;
    p67_battery_diag_fail(P67_BAT_INFO,0); p67_battery_diag_log();
    assert(diag_writes==1);
    for(unsigned i=0;i<64;++i) { ++p67_battery_diag_blocked; p67_battery_diag_log(); }
    assert(diag_writes==1);
    for(unsigned i=0;i<7;++i) { p67_battery_sample_diag[0].info_rc=i+2; p67_battery_diag_log(); }
    assert(diag_writes==8);
    p67_battery_sample_diag[0].info_rc=99;
    for(unsigned i=0;i<31;++i) p67_battery_diag_log();
    assert(diag_writes==8); p67_battery_diag_log(); assert(diag_writes==9);
}
int main(int argc,char **argv) {
    if(argc==2 && !strcmp(argv[1],"--charge")) test_centered_charge_edges_and_cache();
    else if(argc==2 && !strcmp(argv[1],"--mask")) {
        test_mask_edge_and_cache(); test_mask_mapping_and_invalidation(); test_strict_failures();
    } else { assert(argc==1); test_strict_lifetime(); }
    puts("p67 battery strict recovery: ok (host seams only)");
    return 0;
}
