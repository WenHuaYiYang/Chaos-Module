/* Chaos lifecycle repair; upstream Corona implementation remains unchanged.
 * AGPL-3.0-only, same license as the Corona implementation this integrates. */
static void chaos_corona_launcher_rebind(void) {
    uint32_t *scroll, *owner_event, *draw_event;
    if (p67_busy || load32(P67_IMAGE_SLOT) || P67_U8(0x201102d9u)) return;
    if (!p67_owner || p67_draw_owner != load32(P67_ROOT_SLOT)) return;
    if (!p67_live(p67_owner) || !p67_live(p67_page) || !p67_live(p67_draw_owner)) return;
    scroll = p67_descriptor(p67_owner, (uint32_t)(uintptr_t)p67_scroll);
    owner_event = p67_descriptor(p67_owner, (uint32_t)(uintptr_t)p67_event);
    draw_event = p67_descriptor(p67_draw_owner, (uint32_t)(uintptr_t)p67_event);
    if (scroll && scroll[1] == p67_page && scroll[2] == 0u &&
        owner_event && owner_event[1] == p67_generation && owner_event[2] == 36u &&
        draw_event && draw_event[1] == p67_generation && draw_event[2] == 0u) return;
    /* Same addresses can survive native subscription replacement. Retire our
     * surviving callbacks, then let upstream poll bind a fresh generation. */
    p67_detach();
}
