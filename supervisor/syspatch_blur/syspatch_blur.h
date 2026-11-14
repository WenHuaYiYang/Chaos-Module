#ifndef SYSPATCH_BLUR_H
#define SYSPATCH_BLUR_H

/* SysPatch software blur pipeline.
 *
 * A self-contained CPU blur, ported from LVGL's official blur renderer
 * (lvgl/src/draw/sw/lv_draw_sw_blur.c, the file LVGL 9.5 introduced together
 * with the LV_DRAW_TASK_TYPE_BLUR draw task).  No float, no malloc, no LVGL
 * dependency: it filters a plain pixel buffer in place, so any page can call
 * it directly and several pages can share the same code.
 *
 * WHY SOFTWARE
 *   The GCNanoUltraV's vg-lite library has no usable blur: the firmware image
 *   contains neither lv_draw_sw_blur nor lv_draw_blur, and on the device
 *   vg_lite_query_feature(36) - the GAUSSIAN_BLUR capability bit - returns 0
 *   while a filter=0x3000 blit returns success but leaves every pixel
 *   unchanged.  See research/vglite_blur_filter_gaevid.md and
 *   research/blur_pipeline_lvgl95.md.
 *
 * HOW IT DIFFERS FROM THE OFFICIAL RENDERER
 *   Official runs as a draw task and filters the *render target layer's* own
 *   draw buffer in place; it never allocates an image and never touches an
 *   lv_image.  We keep that "filter a buffer in place" shape and drop the
 *   task/layer plumbing, which this firmware does not expose.  So this is the
 *   official *filter*, callable on any buffer - not a replacement for the
 *   draw-task scheduler.
 *
 * ALGORITHM (identical to the official one)
 *   A 4-direction IIR low-pass filter in 12-bit fixed point.  Per sweep:
 *       sum = (sum * intensity) >> 12 + sample * (4096 - intensity)
 *       out = sum >> 12
 *   with intensity = (4096 * radius) / (radius + 4).  Order: each column
 *   top->bottom then bottom->top, then each row left->right then right->left.
 *   The `sum` accumulator is PERSISTENT across the whole sweep while only the
 *   buffer receives the shifted value - folding the two into one variable
 *   truncates the running sum to 8 bits and degenerates the integrator into a
 *   ~0.29x gain (the round-31 bug, documented in MEMORY.md).
 *
 *   Alpha is never filtered: for px_bytes >= 3 only bytes 0..2 of each pixel
 *   are written, so ARGB8888 keeps its original alpha.  Blurring alpha would
 *   pull it up from 0 and make the result look like it vanished on a black
 *   background.
 *
 * RADIUS SEMANTICS
 *   8..16 is a soft frosted glass, 20+ is heavy.  Cost is O(w*h) per sweep
 *   pair, i.e. O(w*h) overall with a small constant.
 */

typedef unsigned int syspatch_blur_u32;

/* ---- pixel buffer ------------------------------------------------------- */

/* The buffer is described by geometry + row pitch + bytes per pixel, mirroring
 * how the official renderer dispatches on `px_size` (1 / 2 / >=3).  We do not
 * carry an LVGL colour-format enum on purpose: px_bytes is the only thing the
 * filter branches on, and using the byte count avoids guessing at this
 * firmware's colour-format numbering.
 *
 * px_bytes:  1 -> single 8-bit channel (A8/L8)
 *            2 -> RGB565 little-endian
 *          >=3 -> the first three bytes of each pixel (B,G,R in ARGB8888
 *                memory order), 4th byte preserved
 *
 * stride:    bytes between the starts of consecutive rows.  0 means
 *            `w * px_bytes` (tightly packed).  A stride wider than the image
 *            is honoured, which is what lets the caller hand us a buffer that
 *            also satisfies vg-lite's 64-byte alignment rule.
 */
typedef struct {
    void *data;
    syspatch_blur_u32 w;
    syspatch_blur_u32 h;
    syspatch_blur_u32 stride;
    syspatch_blur_u32 px_bytes;
} syspatch_blur_buf_t;

/* ---- job configuration -------------------------------------------------- */

#define SYSPATCH_BLUR_QUALITY_AUTO      0u   /* official default */
#define SYSPATCH_BLUR_QUALITY_SPEED     1u
#define SYSPATCH_BLUR_QUALITY_PRECISION 2u

typedef struct {
    syspatch_blur_u32 radius;         /* blur strength, >= 1; 0 = no-op */
    syspatch_blur_u32 corner_radius;  /* rounded-rectangle mask; 0 = rectangle */
    syspatch_blur_u32 quality;        /* SYSPATCH_BLUR_QUALITY_* */
    syspatch_blur_u32 warm_start;     /* 1 = official: seed `sum` from the local
                                       *     average of the first sample_len
                                       *     pixels.  0 = seed with 0, which
                                       *     leaves a dark fringe at the start
                                       *     of every sweep (legacy behaviour). */
    syspatch_blur_u32 skip;           /* 0 = automatic (official), 1..3 = forced.
                                       * The filter visits every skip-th column /
                                       * row and fills the holes by duplication;
                                       * a larger skip is a big speed win at
                                       * large radii and costs sharpness. */
    syspatch_blur_u32 round_out;      /* 1 = add half before the final shift and
                                       *     clamp to 255 (legacy), 0 = exact
                                       *     official truncation. */
} syspatch_blur_cfg_t;

/* Fill a config with the full official semantics: warm start, automatic skip,
 * truncating store.  Call this first, then override what you need. */
void syspatch_blur_cfg_default(syspatch_blur_cfg_t *cfg);

/* Run one blur job IN PLACE.
 *
 * Returns the number of pixels the filter actually visited (0 when the job was
 * a no-op, e.g. radius 0 or a geometry too small after the skip rounding).
 * Echoing that count is how a caller proves on-screen that the filter ran
 * rather than that nothing happened.
 */
syspatch_blur_u32 syspatch_blur_run(const syspatch_blur_buf_t *buf,
                                    const syspatch_blur_cfg_t *cfg);

/* Cooperative filtering: same pixels as run(), bounded complete lines per
 * step. Caller retains the buffer and must not draw partially filtered data.
 * begin returns 0 for a no-op; step returns 1 when finished, 0 while pending.
 * No allocation, no worker thread, no change to quality or filter order. */
typedef struct {
    syspatch_blur_buf_t buffer;
    syspatch_blur_cfg_t config;
    syspatch_blur_u32 cursor, phase, visited;
} syspatch_blur_task_t;
int syspatch_blur_begin(syspatch_blur_task_t *task,
                        const syspatch_blur_buf_t *buf,
                        const syspatch_blur_cfg_t *cfg);
int syspatch_blur_step(syspatch_blur_task_t *task,
                       syspatch_blur_u32 lines);

/* ---- legacy entry point (kept bit-compatible) --------------------------- */

/* Blur a tightly packed ARGB8888 buffer in place.
 *
 * This is the round-31 component signature the 0x84 page already uses, and it
 * is preserved bit-for-bit: zero-initialised accumulator, no pixel skipping,
 * rounded store.  Page 0x84's on-device look was accepted with exactly this
 * behaviour, so it is deliberately NOT migrated to the new defaults.  New code
 * should call syspatch_blur_run() with syspatch_blur_cfg_default().
 */
void syspatch_blur_argb8888(syspatch_blur_u32 *buf, syspatch_blur_u32 w,
                            syspatch_blur_u32 h, syspatch_blur_u32 radius);

#endif /* SYSPATCH_BLUR_H */
