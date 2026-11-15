/* Chaos display-subscription repair, AGPL-3.0-only.
 * Run only from the serialized UI owner poll, never a display/DELETE callback.
 * A DELETE notification can invalidate the logical observer while its record
 * remains on a still-live display. Upstream correctly refuses a duplicate,
 * but then neither capture nor deferred cleanup can ever receive START again.
 */
static uint32_t chaos_corona_observer_repairs;
static void chaos_corona_display_rebind(void) {
    uint32_t display, *record;
    if (p67_blur_display || p67_blur_busy || p67_blur_polling ||
        !P67_BLUR_OWNER_SAFE()) return;
    display = P67_BLUR_DISPLAY();
    if (!display || !P67_BLUR_DISPLAY_LIVE(display)) return;
    /* The bounded lookup rejects duplicate or unreadable records. Only our
     * exact callback/filter and immediately invalidated generation qualify.
     * No object APIs, raw display-list writes, or snapshot/GPU buffer frees. */
    record = p67_blur_display_record(display);
    if (!record || record[2] != 0x8000u ||
        record[1] != p67_blur_display_generation - 1u) return;
    if (P67_BLUR_DISPLAY_REMOVE(display, p67_blur_display_event) &&
        !p67_blur_display_record(display)) ++chaos_corona_observer_repairs;
    /* The immediately following upstream poll installs a fresh generation.
     * Capturing and fencing still happen exclusively at render START. */
}
