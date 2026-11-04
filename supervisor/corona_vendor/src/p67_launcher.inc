/* Exact .043 launcher ownership implementation, included by p67_backgrounds.inc.
 * No snapshot/pool source is changed. Animation/frame time is NOT_PROBED. */
#if !(defined(RH_TARGET_1043) && RH_TARGET_1043)
#error "p67 launcher adapter requires exact .043"
#endif
#define P67_ROOT_SLOT 0x201102ecu
#define P67_IMAGE_SLOT 0x201102dcu
#define P67_SCROLL_CB 0x0c517d9du
/* Injectable native dispatch/read seams for the host lifecycle model. */
#ifndef P67_NATIVE
#define P67_NATIVE(address) (address)
#endif
#ifndef P67_U8
#define P67_U8(address) (*(volatile unsigned char *)(uintptr_t)(address))
#endif
#ifndef P67_U32
#define P67_U32(address) (*(volatile uint32_t *)(uintptr_t)(address))
#endif
static char p67_path[2][RH_PATH];
static unsigned p67_path_index;
static uint32_t p67_owner, p67_page, p67_draw_owner, p67_generation;
static int p67_busy, p67_wake_pending;
static void p67_event(void *event);
static void p67_scroll(void *event);

struct p67_lookup { uint32_t wanted, found, count; };
static int p67_find_live(uint32_t object, void *cookie) {
    struct p67_lookup *s = cookie;
    if (++s->count > RH_RELOAD_LIMIT) return 2;
    if (object == s->wanted) { s->found = object; return 2; }
    return 0;
}
static int p67_live(uint32_t object) {
    struct p67_lookup s = {object, 0, 0};
    if (!object) return 0;
    ((void (*)(uint32_t, int (*)(uint32_t, void *), void *))
        (uintptr_t)P67_NATIVE(RH_FW_OBJECT_TREE_WALK))(0, p67_find_live, &s);
    return s.found != 0;
}
static uint32_t *p67_descriptor(uint32_t object, uint32_t callback) {
    uint32_t count, i;
    uint32_t *match = 0;
    count = ((uint32_t (*)(uint32_t))(uintptr_t)P67_NATIVE(0x002bf7d5u))(object);
    if (count > 64u) return 0;
    for (i = 0; i < count; ++i) {
        uint32_t *d = ((uint32_t *(*)(uint32_t, uint32_t))
            (uintptr_t)P67_NATIVE(0x002bf7e9u))(object, i);
        if (d && d[0] == callback) {
            if (match) return 0; /* ambiguous owner: fail closed */
            match = d;
        }
    }
    return match;
}
/* ARM .043 always uses the native provider. Host ownership models inject
 * their provider seams and test the native provider separately. */
#if defined(__arm__) || defined(__thumb__)
#include "p67_blur_native.inc"
#ifndef P67_BLUR_OWNER_SAFE
#define P67_BLUR_OWNER_SAFE() p67_blur_native_idle()
#endif
#endif
#include "p67_blur.inc"
static struct p67_blur_session p67_launcher_session;
/* Exact .043 image ABI: 112-byte descriptor, source+28, scale+48/+52,
 * pivot+64/+68. Rectangle backgrounds only center/tile and cannot upscale.
 * Both adapters install their bounded layer clip before submitting this task. */
static void p67_draw_background(uint32_t layer, const char *path, const int32_t *area) {
    uint32_t source = (uint32_t)p67_source(path);
    if (!source) return;
    if (rh_background_is_blur(path)) {
        uint32_t descriptor[28];
        const int32_t source_area[4] = {0, 0, P67_BLUR_WIDTH - 1u, P67_BLUR_HEIGHT - 1u};
        ((void (*)(void *))(uintptr_t)P67_NATIVE(0x0c5882b9u))(descriptor);
        descriptor[7] = source;
        descriptor[12] = descriptor[13] = 4u * 256u;
        descriptor[16] = descriptor[17] = 0;
        p67_blur_draw_seen();
        ((void (*)(uint32_t, const void *, const int32_t *))(uintptr_t)
            P67_NATIVE(0x0c5882a1u))(layer, descriptor, source_area);
    } else {
        uint32_t descriptor[47];
        ((void (*)(void *))(uintptr_t)P67_NATIVE(0x0c588269u))(descriptor);
        ((unsigned char *)descriptor)[32] = 0;
        descriptor[30] = source;
        ((void (*)(uint32_t, const void *, const int32_t *))(uintptr_t)
            P67_NATIVE(0x0c588351u))(layer, descriptor, area);
    }
}
static void p67_remove_event(uint32_t object) {
    ((int (*)(uint32_t, uint32_t))(uintptr_t)P67_NATIVE(0x002bf811u))
        (object, (uint32_t)(uintptr_t)p67_event);
}
static void p67_detach(void) {
    uint32_t owner = p67_owner, draw = p67_draw_owner;
    uint32_t *d;
    p67_blur_session_clear(&p67_launcher_session,1u);
    p67_owner = p67_page = p67_draw_owner = 0;
    ++p67_generation;
    if (p67_live(owner)) {
        d = p67_descriptor(owner, (uint32_t)(uintptr_t)p67_scroll);
        if (d) d[0] = P67_SCROLL_CB;
        p67_remove_event(owner);
    }
    if (draw != owner && p67_live(draw)) p67_remove_event(draw);
}
static void p67_event(void *event) {
    uint32_t obj = *(uint32_t *)event;
    uint32_t target = *((uint32_t *)event + 1);
    unsigned code = *(uint16_t *)((unsigned char *)event + 8) & 0x7fffu;
    int32_t saved_clip[4], clip[4];
    unsigned i;
    int32_t area[4] = {0, 0, 335, 479};
    uint32_t layer;
    if (obj != target || *(uint32_t *)((unsigned char *)event + 12) != p67_generation) return;
    if (code == 36u) {
        /* Invalidate the pair before any native operation. Do not mutate the
         * currently dispatching subscription or walk the deleting subtree.
         * The survivor is removed on the next idle poll/detach. */
        if (obj == p67_owner) {
            p67_blur_session_clear(&p67_launcher_session,1u);
            uint32_t *d = p67_descriptor(obj, (uint32_t)(uintptr_t)p67_scroll);
            if (d) d[0] = P67_SCROLL_CB;
            p67_owner = p67_page = 0;
            ++p67_generation;
        } else if (obj == p67_draw_owner) {
            p67_blur_session_clear(&p67_launcher_session,1u);
            p67_draw_owner = 0;
            ++p67_generation;
        }
        return;
    }
    if (obj != p67_draw_owner || !p67_page || code != 25u || !p67_path[p67_path_index][0]) return;
    layer = *(uint32_t *)((unsigned char *)event + 16);
    if (!layer) return;
    /* .043 redraw intersects layer+24 with object's EXTENDED draw area before
     * event25; that is not proof of page viewport clipping. Explicitly bound
     * to page coordinates +28..40 and screen while keeping destination fixed. */
    for (i = 0; i < 4u; ++i) {
        int32_t page_bound = (int32_t)P67_U32(p67_page + 28u + 4u * i);
        saved_clip[i] = (int32_t)P67_U32(layer + 24u + 4u * i);
        if (i < 2u) {
            clip[i] = saved_clip[i] > page_bound ? saved_clip[i] : page_bound;
            if (clip[i] < area[i]) clip[i] = area[i];
        } else {
            clip[i] = saved_clip[i] < page_bound ? saved_clip[i] : page_bound;
            if (clip[i] > area[i]) clip[i] = area[i];
        }
    }
    if (clip[0] > clip[2] || clip[1] > clip[3]) return;
    for (i = 0; i < 4u; ++i) P67_U32(layer + 24u + 4u * i) = (uint32_t)clip[i];
    p67_draw_background(layer, p67_path[p67_path_index], area);
    for (i = 0; i < 4u; ++i) P67_U32(layer + 24u + 4u * i) = (uint32_t)saved_clip[i];
}
static void p67_scroll(void *event) {
    unsigned code = *(uint16_t *)((unsigned char *)event + 8) & 0x7fffu;
    uint32_t page = *(uint32_t *)((unsigned char *)event + 12);
    unsigned blur=rh_background_is_blur(p67_path[p67_path_index]),entry_binding=0;
    if (blur && p67_owner && page == p67_page && *(uint32_t *)event == p67_owner &&
        (code == 9u || code == 11u || code == 12u)) {
        /* Root DELETE can leave the owner's wrapper/userdata until idle detach.
         * Validate membership only at begin, never on every SCROLL frame. */
        entry_binding=page && p67_draw_owner && p67_generation &&
            load32(P67_ROOT_SLOT)==p67_draw_owner &&
            (code != 9u || (p67_live(p67_owner) && p67_live(page) && p67_live(p67_draw_owner)));
        if (entry_binding)
            p67_blur_session_event(&p67_launcher_session,1u,p67_generation,page,code,
                (int32_t)P67_U32(page + 32u),480,*((uint32_t *)event + 1)==p67_owner);
        else p67_blur_session_clear(&p67_launcher_session,1u);
    }
    /* The native callback's C588310 getter returns USER DATA, not a target.
     * Bubbled events are intentional. Binding identity scopes this wrapper. */
    if (p67_owner && page == p67_page && code == 9u && (!blur || entry_binding) &&
        p67_source(p67_path[p67_path_index])) {
        int32_t y = (int32_t)P67_U32(page + 32u);
        if (y <= 480) {
            P67_U8(0x201102d9u) = 1;
            if (!y) ((void (*)(uint32_t, int, int))(uintptr_t)P67_NATIVE(0x0c5166f1u))
                (load32(P67_ROOT_SLOT), 0, 0);
        }
        return;
    }
    ((void (*)(void *))(uintptr_t)P67_NATIVE(P67_SCROLL_CB))(event);
}
void rh_platform_backgrounds_poll(void) {
    uint32_t owner, page, foreground, list, generation, *d, *owner_event, *draw_event;
    if (p67_busy) return;
#ifndef rh_platform_backgrounds_poll
    p67_blur_watch(rh_background_is_blur(p67_path[p67_path_index]));
#endif
    if (p67_wake_pending && !rh_platform_request_full_redraw()) p67_wake_pending = 0;
    if (!p67_path[p67_path_index][0]) return;
    /* Never install over an existing native snapshot or during scroll. */
    if (load32(P67_IMAGE_SLOT) || P67_U8(0x201102d9u)) return;
    if (p67_owner && p67_draw_owner == load32(P67_ROOT_SLOT)) return;
    if (p67_owner || p67_draw_owner) p67_detach();
    foreground = load32(0x201102f4u);
    list = load32(P67_ROOT_SLOT);
    if (!p67_live(foreground) || !p67_live(list) || load32(list) != 0x2cd2024cu) return;
    page = ((uint32_t (*)(uint32_t))(uintptr_t)P67_NATIVE(0x0c589579u))(foreground);
    if (!p67_live(page)) return;
    owner = ((uint32_t (*)(uint32_t))(uintptr_t)P67_NATIVE(0x0c589579u))(page);
    if (!p67_live(owner)) return;
    d = p67_descriptor(owner, P67_SCROLL_CB);
    if (!d || d[1] != page || d[2] != 0u) return;
    p67_busy = 1;
    generation = p67_generation + 1u;
    owner_event = ((uint32_t *(*)(uint32_t, uint32_t, uint32_t, uint32_t))
        (uintptr_t)P67_NATIVE(0x0c5883f1u))(owner, (uint32_t)(uintptr_t)p67_event, 36, generation);
    draw_event = owner_event ? ((uint32_t *(*)(uint32_t, uint32_t, uint32_t, uint32_t))
        (uintptr_t)P67_NATIVE(0x0c5883f1u))(list, (uint32_t)(uintptr_t)p67_event, 0, generation) : 0;
    /* Registration can resize the descriptor list. Re-find and revalidate the
     * native binding before changing its callback, preserving data and filter. */
    d = p67_descriptor(owner, P67_SCROLL_CB);
    if (!owner_event || !draw_event || !d || d[1] != page || d[2] != 0u) {
        if (owner_event) p67_remove_event(owner);
        if (draw_event) p67_remove_event(list);
        p67_busy = 0;
        return;
    }
    p67_owner = owner;
    p67_page = page;
    p67_draw_owner = list;
    p67_generation = generation;
    d[0] = (uint32_t)(uintptr_t)p67_scroll;
    p67_busy = 0;
    p67_wake_pending = rh_platform_request_full_redraw() != 0;
}
int rh_platform_backgrounds_commit(const struct rh_backgrounds *bg) {
    unsigned i, next = p67_path_index ^ 1u;
    if (!bg || bg->control_center[0] || bg->notifications[0]) return -2144;
    /* Cancellation/reload is deferred to an idle native owner; do not restore
     * snapshot-taking behavior midway through a live swipe. */
    if (p67_busy || load32(P67_IMAGE_SLOT) || P67_U8(0x201102d9u)) return -2145;
    p67_detach();
    p67_blur_reset();
    for (i = 0; i < RH_PATH; ++i) {
        p67_path[next][i] = bg->launcher[i];
        if (!bg->launcher[i]) break;
    }
    p67_path_index = next;
    p67_wake_pending = 1;
    rh_platform_backgrounds_poll();
    return 0;
}
