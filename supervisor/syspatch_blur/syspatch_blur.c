/* SysPatch software blur pipeline - implementation.
 *
 * A faithful port of LVGL's official blur filter,
 * lvgl/src/draw/sw/lv_draw_sw_blur.c, stripped of the draw-task / layer
 * plumbing that this firmware does not expose.  Everything the official code
 * does to the pixels is reproduced here, including the four things the
 * round-31 component did not have:
 *
 *   1. warm-start accumulator seed  (blur_{1,3}_bytes_init)
 *   2. skip_cnt downsampling + hole filling  (the quality/speed trick)
 *   3. run-length skip inside a sweep  (`buf_prev` guard, 1- and 2-byte paths)
 *   4. rounded-corner mask  (get_rounded_edge_point + integer sqrt)
 *
 * plus the per-pixel-size dispatch (1 byte / 2 bytes RGB565 / >=3 bytes).
 *
 * STRUCTURE
 *   The three pixel-size paths are kept as three explicit loop bodies, exactly
 *   as in the official file, so this remains line-by-line auditable against
 *   the reference.  tools/verify_blur_pipeline.py does that comparison
 *   pixel-by-pixel offline, against a Python transcription of the reference.
 *
 * ROUND 31 FIX - the accumulator must stay 12-bit
 *   The old port folded the accumulator and the shifted output into a single
 *   variable, so the running sum was truncated to 8 bits on every step.  That
 *   turns the integrator into a plain gain of (4096-intensity)/4096 ~= 0.29,
 *   reached in one step; four sweeps then leave c * 0.29^4 ~= 0.7%, i.e. every
 *   bright pixel lands at 2/255 with alpha still 255 - an opaque black buffer
 *   that looks like the image vanished.  `sum` is now a separate persistent
 *   accumulator and only the buffer receives `sum >> 12`, exactly as in
 *   blur_1_bytes()/blur_3_bytes().  The legacy wrapper below reproduces the
 *   old behaviour on purpose, so page 0x84 is untouched.
 *
 * Nothing here allocates, floats, or calls out.  In-place is safe: the filter
 * only ever reads the pixel it is about to overwrite.
 */

#include "syspatch_blur.h"

typedef syspatch_blur_u32 u32;
typedef signed int s32;
typedef unsigned char u8;
typedef unsigned short u16;

#define BLUR_BITS       12u
#define BLUR_MAX        (1u << BLUR_BITS)     /* 4096 */
#define BLUR_HALF       (BLUR_MAX >> 1u)      /* 2048 */
#define BLUR_RADIUS_MAX 64u

/* ---- small helpers ------------------------------------------------------ */

/* Integer square root.  The official file calls lv_sqrt32(); this firmware
 * does not export it in a form we can rely on, and the corner mask only needs
 * the integer result, so we carry our own.
 * Only called with r*r - p*p where 0 <= p < r, so the input is non-negative. */
static u32 isqrt32(u32 n)
{
    u32 r = 0u;
    u32 bit = 1u << 30;

    while (bit > n) bit >>= 2;
    while (bit != 0u) {
        u32 t = r + bit;
        if (n >= t) {
            n -= t;
            r = (r >> 1) + bit;
        } else {
            r >>= 1;
        }
        bit >>= 2;
    }
    return r;
}

/* get_rounded_edge_point(): how many pixels of a line/column lie outside the
 * rounded rectangle and must therefore be left alone.  Returns 0 in the
 * straight part of the edge. */
static s32 edge_point(s32 p_start, s32 p_end, s32 p, s32 r)
{
    u32 res;

    if (r <= 0) return 0;
    if (p < p_start + r)         p = r - (p - p_start);
    else if (p > p_end - r)      p = r - (p_end - p);
    else return 0;

    if (p < 0) p = 0;
    if (p > r) p = r;
    res = isqrt32((u32)(r * r - p * p));
    return r - (s32)res;
}

static s32 clamp_s32(s32 v, s32 lo, s32 hi)
{
    if (v < lo) return lo;
    if (v > hi) return hi;
    return v;
}

/* ---- one IIR step ------------------------------------------------------- *
 * `sum` is the PERSISTENT 12-bit running sum; only the buffer gets the shifted
 * value.  This split is the whole point (see the round-31 note above).
 */

static void a8_step(u32 *sum, u8 *b, u32 intensity, u32 round_out)
{
    u32 inv = BLUR_MAX - intensity;
    u32 add = round_out ? BLUR_HALF : 0u;
    u32 v;

    sum[0] = ((sum[0] * intensity) >> BLUR_BITS) + ((u32)b[0] * inv);
    v = (sum[0] + add) >> BLUR_BITS;
    b[0] = (u8)(v > 255u ? 255u : v);
}

static void r16_step(u32 *sum, u8 *b, u32 intensity)
{
    u32 inv = BLUR_MAX - intensity;
    u32 v = (u32)b[0] | ((u32)b[1] << 8);
    u32 r  = (v >> 11) & 0x1fu;
    u32 g  = (v >> 5)  & 0x3fu;
    u32 bl = v & 0x1fu;
    u32 o;

    sum[0] = ((sum[0] * intensity) >> BLUR_BITS) + (r  * inv);
    sum[1] = ((sum[1] * intensity) >> BLUR_BITS) + (g  * inv);
    sum[2] = ((sum[2] * intensity) >> BLUR_BITS) + (bl * inv);

    /* The official RGB565 path adds the rounding half unconditionally
     * (blur_2_bytes(), `half = BLUR_INTENSITY_MAX >> 1`), unlike the 1- and
     * >=3-byte paths which store sum >> 12.  Reproduced verbatim. */
    r  = (sum[0] + BLUR_HALF) >> BLUR_BITS;
    g  = (sum[1] + BLUR_HALF) >> BLUR_BITS;
    bl = (sum[2] + BLUR_HALF) >> BLUR_BITS;

    o = ((r & 0x1fu) << 11) | ((g & 0x3fu) << 5) | (bl & 0x1fu);
    b[0] = (u8)(o & 0xffu);
    b[1] = (u8)((o >> 8) & 0xffu);
}

static inline __attribute__((always_inline)) void rgb_step(u32 *sum, u8 *b, u32 intensity, u32 round_out)
{
    u32 inv = BLUR_MAX - intensity;
    u32 add = round_out ? BLUR_HALF : 0u;
    u32 v;
    u32 i;

    /* Three fixed channels: remove the per-pixel loop/call overhead without
     * changing any accumulator, rounding or store operation. */
#if defined(__clang__)
#pragma clang loop unroll(full)
#endif
    for (i = 0; i < 3u; ++i) {
        v = ((sum[i] * intensity) >> BLUR_BITS) + ((u32)b[i] * inv);
        sum[i] = v;
        v = (v + add) >> BLUR_BITS;
        b[i] = (u8)(v > 255u ? 255u : v);
    }
    /* b[3] (alpha) is deliberately not touched. */
}

/* ---- accumulator warm start -------------------------------------------- *
 * blur_{1,3}_bytes_init(): seed `sum` with the mean of the first
 * `sample_len` samples of the sweep, scaled into 12-bit.  Without this the
 * first pixels of every sweep get pulled toward 0 and the image grows a dark
 * fringe along all four edges.
 */

static void a8_init(u32 *sum, const u8 *b, u32 n, s32 stride)
{
    u32 s;
    sum[0] = 0u;
    for (s = 0; s < n; ++s) { sum[0] += b[0]; b += stride; }
    sum[0] = (sum[0] << BLUR_BITS) / n;
}

static void r16_init(u32 *sum, const u8 *b, u32 n, s32 stride)
{
    u32 s;
    sum[0] = sum[1] = sum[2] = 0u;
    for (s = 0; s < n; ++s) {
        u32 v = (u32)b[0] | ((u32)b[1] << 8);
        sum[0] += (v >> 11) & 0x1fu;
        sum[1] += (v >> 5)  & 0x3fu;
        sum[2] += v & 0x1fu;
        b += stride;
    }
    sum[0] = (sum[0] << BLUR_BITS) / n;
    sum[1] = (sum[1] << BLUR_BITS) / n;
    sum[2] = (sum[2] << BLUR_BITS) / n;
}

static void rgb_init(u32 *sum, const u8 *b, u32 n, s32 stride)
{
    u32 s;
    sum[0] = sum[1] = sum[2] = 0u;
    for (s = 0; s < n; ++s) {
        sum[0] += b[0];
        sum[1] += b[1];
        sum[2] += b[2];
        b += stride;
    }
    sum[0] = (sum[0] << BLUR_BITS) / n;
    sum[1] = (sum[1] << BLUR_BITS) / n;
    sum[2] = (sum[2] << BLUR_BITS) / n;
}

static void zero_sum(u32 *sum, u32 nch)
{
    u32 i;
    for (i = 0; i < nch; ++i) sum[i] = 0u;
}

static void copy_bytes(u8 *dst, const u8 *src, u32 n)
{
    u32 i;
    for (i = 0; i < n; ++i) dst[i] = src[i];
}

/* ---- resolved job ------------------------------------------------------- */

typedef struct {
    u8 *base;
    u32 w, h;
    u32 px;            /* bytes per pixel */
    u32 stride;        /* bytes per row */
    u32 x1, x2, y1, y2;/* work area, inclusive, snapped to the skip grid */
    u32 intensity;
    u32 sample_len;
    u32 corner;
    u32 skip;
    u32 warm;
    u32 round_out;
    u32 px_ofs;        /* px * skip */
    s32 row_step;      /* stride * skip */
} blur_job_t;

/* ---- columns: every skip-th column, top->bottom then bottom->top -------- */

static u32 blur_columns(const blur_job_t *j)
{
    u32 x, y, visited = 0u;
    u32 sum[3];

    for (x = j->x1; x <= j->x2; x += j->skip) {
        s32 cir = edge_point(0, (s32)j->w - 1, (s32)x, (s32)j->corner);
        u32 ys, ye, n;
        u8 *p;

        /*
         * Clamp into the SNAPPED work area, exactly as the official code does
         *
         *     LV_CLAMP(clipped_coords.y1, coords->y1 - layer_y_ofs + cir_y,
         *              clipped_coords.y2)
         *
         * - NOT into the full buffer [0, h-1].  Clamping to h-1 first lets
         * y_end snap UP past y2 (e.g. h=23, skip=2: 22 instead of 20), which
         * blurs pixels the opposite sweep will never visit and makes
         * line_bytes exceed stride in the row pass, i.e. a 4-byte write into
         * the next row.  This was the round-33 defect that only showed up
         * when w or h is not a multiple of skip.
         */
        ys = (u32)clamp_s32(cir, (s32)j->y1, (s32)j->y2);
        ye = (u32)clamp_s32((s32)j->h - 1 - cir, (s32)j->y1, (s32)j->y2);
        ys = (ys / j->skip) * j->skip;
        ye = (ye / j->skip) * j->skip;
        if (ys > ye) continue;

        n = (ye - ys) / j->skip + 1u;
        if (n > j->sample_len) n = j->sample_len;
        visited += (ye - ys) / j->skip + 1u;

        if (j->px == 1u) {
            p = j->base + ys * j->stride + x;
            if (j->warm) a8_init(sum, p, n, j->row_step); else zero_sum(sum, 1u);
            {
                u8 prev = (u8)(p[0] + 1u);
                for (y = ys; y <= ye; y += j->skip) {
                    if (prev != p[0]) {
                        a8_step(sum, p, j->intensity, j->round_out);
                        prev = p[0];
                    }
                    p += j->row_step;
                }
            }
            p = j->base + ye * j->stride + x;
            if (j->warm) a8_init(sum, p, n, -j->row_step); else zero_sum(sum, 1u);
            {
                u8 prev = (u8)(p[0] + 1u);
                for (y = ys; y <= ye; y += j->skip) {
                    if (prev != p[0]) {
                        a8_step(sum, p, j->intensity, j->round_out);
                        prev = p[0];
                    }
                    p -= j->row_step;
                }
            }
        } else if (j->px == 2u) {
            p = j->base + ys * j->stride + x * 2u;
            if (j->warm) r16_init(sum, p, n, j->row_step); else zero_sum(sum, 3u);
            {
                u16 prev = (u16)(((u32)p[0] | ((u32)p[1] << 8)) + 1u);
                for (y = ys; y <= ye; y += j->skip) {
                    u16 cur = (u16)((u32)p[0] | ((u32)p[1] << 8));
                    if (prev != cur) {
                        r16_step(sum, p, j->intensity);
                        prev = (u16)((u32)p[0] | ((u32)p[1] << 8));
                    }
                    p += j->row_step;
                }
            }
            p = j->base + ye * j->stride + x * 2u;
            if (j->warm) r16_init(sum, p, n, -j->row_step); else zero_sum(sum, 3u);
            {
                u16 prev = (u16)(((u32)p[0] | ((u32)p[1] << 8)) + 1u);
                for (y = ys; y <= ye; y += j->skip) {
                    u16 cur = (u16)((u32)p[0] | ((u32)p[1] << 8));
                    if (prev != cur) {
                        r16_step(sum, p, j->intensity);
                        prev = (u16)((u32)p[0] | ((u32)p[1] << 8));
                    }
                    p -= j->row_step;
                }
            }
        } else {
            p = j->base + ys * j->stride + x * j->px;
            if (j->warm) rgb_init(sum, p, n, j->row_step); else zero_sum(sum, 3u);
            for (y = ys; y <= ye; y += j->skip) {
                rgb_step(sum, p, j->intensity, j->round_out);
                p += j->row_step;
            }
            p = j->base + ye * j->stride + x * j->px;
            if (j->warm) rgb_init(sum, p, n, -j->row_step); else zero_sum(sum, 3u);
            for (y = ys; y <= ye; y += j->skip) {
                rgb_step(sum, p, j->intensity, j->round_out);
                p -= j->row_step;
            }
        }
    }
    return visited;
}

/* ---- rows: every skip-th row, left->right then right->left ------------- *
 * The reverse sweep also fills the pixels the skip left unhit: each finished
 * pixel is duplicated into the next skip-1 slots, and once the whole row is
 * done the row itself is duplicated into the skipped rows below it.  That is
 * why the column pass can afford to visit only every skip-th column.
 */
static u32 blur_rows(const blur_job_t *j)
{
    u32 x, y, visited = 0u;
    u32 sum[3];

    for (y = j->y1; y <= j->y2; y += j->skip) {
        s32 cir = edge_point(0, (s32)j->h - 1, (s32)y, (s32)j->corner);
        u32 xs, xe, n, line_bytes;
        u8 *p;

        /* Same rule as the column pass: clamp into the snapped work area.
         * This is also what keeps line_bytes <= stride. */
        xs = (u32)clamp_s32(cir, (s32)j->x1, (s32)j->x2);
        xe = (u32)clamp_s32((s32)j->w - 1 - cir, (s32)j->x1, (s32)j->x2);
        xs = (xs / j->skip) * j->skip;
        xe = (xe / j->skip) * j->skip;
        if (xs > xe) continue;

        line_bytes = (xe - xs + j->skip) * j->px;
        n = (xe - xs) / j->skip + 1u;
        if (n > j->sample_len) n = j->sample_len;

        if (j->px == 1u) {
            /* Official starts the forward walk one step in. */
            p = j->base + y * j->stride + xs;
            if (j->warm) a8_init(sum, p, n, (s32)j->px_ofs); else zero_sum(sum, 1u);
            {
                u8 prev = (u8)(p[0] + 1u);
                for (x = xs + j->skip; x <= xe; x += j->skip) {
                    if (prev != p[0]) {
                        a8_step(sum, p, j->intensity, j->round_out);
                        prev = p[0];
                    }
                    p += j->px_ofs;
                }
            }
            p = j->base + y * j->stride + xe;
            if (j->warm) a8_init(sum, p, n, -(s32)j->px_ofs); else zero_sum(sum, 1u);
            {
                u8 prev = (u8)(p[0] + 1u);
                for (x = xs; x <= xe; x += j->skip) {
                    if (prev != p[0]) {
                        a8_step(sum, p, j->intensity, j->round_out);
                        prev = p[0];
                    }
                    if (j->skip == 2u) {
                        p[1] = p[0];
                    } else if (j->skip == 3u) {
                        p[1] = p[0];
                        p[2] = p[0];
                    }
                    if (j->skip > 1u && x + j->skip > xe) {
                        u8 *from = j->base + y * j->stride + xs;
                        copy_bytes(from + j->stride, from, line_bytes);
                        if (j->skip == 3u)
                            copy_bytes(from + j->stride * 2u, from, line_bytes);
                    }
                    p -= j->px_ofs;
                }
            }
        } else if (j->px == 2u) {
            p = j->base + y * j->stride + xs * 2u;
            if (j->warm) r16_init(sum, p, n, (s32)j->px_ofs); else zero_sum(sum, 3u);
            {
                u16 prev = (u16)(((u32)p[0] | ((u32)p[1] << 8)) + 1u);
                for (x = xs; x <= xe; x += j->skip) {
                    u16 cur = (u16)((u32)p[0] | ((u32)p[1] << 8));
                    if (prev != cur) {
                        r16_step(sum, p, j->intensity);
                        prev = (u16)((u32)p[0] | ((u32)p[1] << 8));
                    }
                    p += j->px_ofs;
                }
            }
            p = j->base + y * j->stride + xe * 2u;
            if (j->warm) r16_init(sum, p, n, -(s32)j->px_ofs); else zero_sum(sum, 3u);
            {
                u16 prev = (u16)(((u32)p[0] | ((u32)p[1] << 8)) + 1u);
                for (x = xs; x <= xe; x += j->skip) {
                    u16 cur = (u16)((u32)p[0] | ((u32)p[1] << 8));
                    if (prev != cur) {
                        r16_step(sum, p, j->intensity);
                        prev = (u16)((u32)p[0] | ((u32)p[1] << 8));
                    }
                    if (j->skip == 2u) {
                        p[2] = p[0];
                        p[3] = p[1];
                    } else if (j->skip == 3u) {
                        p[2] = p[0]; p[3] = p[1];
                        p[4] = p[0]; p[5] = p[1];
                    }
                    if (j->skip > 1u && x + j->skip > xe) {
                        u8 *from = j->base + y * j->stride + xs * 2u;
                        copy_bytes(from + j->stride, from, line_bytes);
                        if (j->skip == 3u)
                            copy_bytes(from + j->stride * 2u, from, line_bytes);
                    }
                    p -= j->px_ofs;
                }
            }
        } else {
            p = j->base + y * j->stride + xs * j->px;
            if (j->warm) rgb_init(sum, p, n, (s32)j->px_ofs); else zero_sum(sum, 3u);
            for (x = xs + j->skip; x <= xe; x += j->skip) {
                rgb_step(sum, p, j->intensity, j->round_out);
                p += j->px_ofs;
            }
            p = j->base + y * j->stride + xe * j->px;
            if (j->warm) rgb_init(sum, p, n, -(s32)j->px_ofs); else zero_sum(sum, 3u);
            for (x = xs; x <= xe; x += j->skip) {
                rgb_step(sum, p, j->intensity, j->round_out);
                if (j->skip == 2u) {
                    p[j->px + 0] = p[0];
                    p[j->px + 1] = p[1];
                    p[j->px + 2] = p[2];
                } else if (j->skip == 3u) {
                    p[j->px + 0] = p[0];
                    p[j->px + 1] = p[1];
                    p[j->px + 2] = p[2];
                    p[j->px * 2u + 0] = p[0];
                    p[j->px * 2u + 1] = p[1];
                    p[j->px * 2u + 2] = p[2];
                }
                if (j->skip > 1u && x + j->skip > xe) {
                    u8 *from = j->base + y * j->stride + xs * j->px;
                    copy_bytes(from + j->stride, from, line_bytes);
                    if (j->skip == 3u)
                        copy_bytes(from + j->stride * 2u, from, line_bytes);
                }
                p -= j->px_ofs;
            }
        }
    }
    return visited;
}

/* ---- public entry points ------------------------------------------------ */

void syspatch_blur_cfg_default(syspatch_blur_cfg_t *cfg)
{
    if (!cfg) return;
    cfg->radius = 12u;
    cfg->corner_radius = 0u;
    cfg->quality = SYSPATCH_BLUR_QUALITY_AUTO;
    cfg->warm_start = 1u;
    cfg->skip = 0u;
    cfg->round_out = 0u;
}

static int prepare_job(const syspatch_blur_buf_t *buf,
                       const syspatch_blur_cfg_t *cfg, blur_job_t *j)
{
    u32 stride, radius, skip_cnt, area;

    if (!buf || !cfg || !buf->data) return 0u;
    if (buf->w == 0u || buf->h == 0u) return 0u;
    if (buf->px_bytes < 1u || buf->px_bytes > 4u) return 0u;
    if (cfg->radius == 0u) return 0u;

    radius = cfg->radius;
    if (radius > BLUR_RADIUS_MAX) radius = BLUR_RADIUS_MAX;

    j->base = (u8 *)buf->data;
    j->w = buf->w;
    j->h = buf->h;
    j->px = buf->px_bytes;
    stride = buf->stride ? buf->stride : buf->w * buf->px_bytes;
    j->stride = stride;
    j->corner = cfg->corner_radius;
    j->warm = cfg->warm_start ? 1u : 0u;
    j->round_out = cfg->round_out ? 1u : 0u;

    /* Work area = the whole buffer, snapped inward to the skip grid so that
     * the forward and reverse sweeps hit exactly the same pixels. */
    j->x1 = 0u;
    j->x2 = j->w - 1u;
    j->y1 = 0u;
    j->y2 = j->h - 1u;

    area = j->w * j->h;
    skip_cnt = 1u;
    if (cfg->skip >= 1u && cfg->skip <= 3u) {
        skip_cnt = cfg->skip;
    } else if (cfg->quality == SYSPATCH_BLUR_QUALITY_AUTO) {
        if (radius >= 32u && j->corner == 0u && area > 160u * 160u) skip_cnt = 3u;
        else if (radius >= 8u) skip_cnt = 2u;
    } else if (cfg->quality == SYSPATCH_BLUR_QUALITY_SPEED) {
        skip_cnt = (radius >= 24u) ? 3u : 2u;
    }
    if (skip_cnt > j->w || skip_cnt > j->h) skip_cnt = 1u;
    j->skip = skip_cnt;

    j->x1 = ((j->x1 + (skip_cnt - 1u)) / skip_cnt) * skip_cnt;
    j->x2 = ((j->x2 - (skip_cnt - 1u)) / skip_cnt) * skip_cnt;
    j->y1 = ((j->y1 + (skip_cnt - 1u)) / skip_cnt) * skip_cnt;
    j->y2 = ((j->y2 - (skip_cnt - 1u)) / skip_cnt) * skip_cnt;
    if ((s32)j->x2 < (s32)j->x1 || (s32)j->y2 < (s32)j->y1) return 0u;

    /* The radius the filter actually uses shrinks with the skip: the pixels
     * that are visited are skip apart, so the effective sampling distance
     * grows. */
    radius = radius / skip_cnt;
    j->intensity = (BLUR_MAX * radius) / (radius + 4u);
    j->sample_len = radius / 2u;
    if (j->sample_len < 1u) j->sample_len = 1u;

    /* Rounded corner mask: clamp so a large radius cannot eat the whole box. */
    {
        u32 short_side = (j->w < j->h) ? j->w : j->h;
        if (j->corner > short_side >> 1) j->corner = short_side >> 1;
    }

    j->px_ofs = j->px * skip_cnt;
    j->row_step = (s32)(stride * skip_cnt);

    return 1;
}

u32 syspatch_blur_run(const syspatch_blur_buf_t *buf,
                      const syspatch_blur_cfg_t *cfg)
{
    blur_job_t j;
    if (!prepare_job(buf, cfg, &j)) return 0;
    u32 visited = blur_columns(&j);
    return visited + blur_rows(&j);
}

int syspatch_blur_begin(syspatch_blur_task_t *task,
                        const syspatch_blur_buf_t *buf,
                        const syspatch_blur_cfg_t *cfg)
{
    blur_job_t j;
    if (!task) return 0;
    task->phase = 0;
    task->visited = 0;
    task->cursor = 0;
    if (!prepare_job(buf, cfg, &j)) return 0;
    task->buffer = *buf;
    task->config = *cfg;
    task->phase = 1;
    task->cursor = j.x1;
    return 1;
}

/* A whole column/row preserves its IIR accumulator and both sweep directions.
 * All columns finish before the first row. No resampling or approximation. */
int syspatch_blur_step(syspatch_blur_task_t *task, u32 lines)
{
    blur_job_t j;
    u32 last, count;
    if (!task || !task->phase) return 1;
    if (!lines) return 0;
    if (!prepare_job(&task->buffer, &task->config, &j)) {
        task->phase = 0;
        return 1;
    }
    last = task->phase == 1 ? j.x2 : j.y2;
    count = (last - task->cursor) / j.skip + 1;
    if (lines > count) lines = count;
    if (task->phase == 1) {
        j.x1 = task->cursor;
        j.x2 = j.x1 + (lines - 1) * j.skip;
        task->visited += blur_columns(&j);
        task->cursor = j.x2 + j.skip;
        if (task->cursor > last) {
            task->phase = 2;
            task->cursor = j.y1;
        }
    } else {
        j.y1 = task->cursor;
        j.y2 = j.y1 + (lines - 1) * j.skip;
        task->visited += blur_rows(&j);
        task->cursor = j.y2 + j.skip;
        if (task->cursor > last) task->phase = 0;
    }
    return task->phase == 0;
}

/* Legacy entry point: reproduce the round-31 behaviour bit-for-bit
 * (zero-seeded accumulator, no skipping, rounded store) so that page 0x84,
 * whose on-device look was accepted, does not change. */
void syspatch_blur_argb8888(u32 *buf, u32 w, u32 h, u32 radius)
{
    syspatch_blur_buf_t b;
    syspatch_blur_cfg_t c;

    if (!buf || w == 0u || h == 0u) return;
    if (radius < 1u) radius = 1u;
    if (radius > BLUR_RADIUS_MAX) radius = BLUR_RADIUS_MAX;

    b.data = buf;
    b.w = w;
    b.h = h;
    b.stride = w * 4u;
    b.px_bytes = 4u;

    c.radius = radius;
    c.corner_radius = 0u;
    c.quality = SYSPATCH_BLUR_QUALITY_PRECISION;
    c.warm_start = 0u;
    c.skip = 1u;
    c.round_out = 1u;

    (void)syspatch_blur_run(&b, &c);
}
