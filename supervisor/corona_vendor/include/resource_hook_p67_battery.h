#ifndef RESOURCE_HOOK_P67_BATTERY_H
#define RESOURCE_HOOK_P67_BATTERY_H

#include <stddef.h>
#include <stdint.h>

/* Remove the black mask's contribution from a freshly completed snapshot.
 * Source-over black contributes k to output alpha, but contributes no RGB.
 * Keep coverage of the underlying battery: A' = max(A-k,0). Straight RGB
 * must be un-matted by A/A'; premultiplied RGB is already the desired value.
 * Mask placement may be clipped. Unmasked pixels and row padding are untouched.
 * This is intentionally NOT idempotent: apply once per fresh native snapshot. */
static inline int p67_battery_unmatte_black(
        void *pixels, size_t size, size_t stride, unsigned width, unsigned height,
        const uint8_t *mask, size_t mask_size, size_t mask_stride,
        unsigned mask_width, unsigned mask_height, int32_t mask_x,
        int32_t mask_y, int premultiplied) {
    unsigned x, y;
    unsigned char *p = pixels;
    if (!p || !mask || !width || !height || width > 128u || height > 128u ||
        !mask_width || !mask_height || mask_width > 128u || mask_height > 128u ||
        stride < (size_t)width * 4u || stride > size / height ||
        mask_stride < mask_width || mask_stride > mask_size / mask_height ||
        mask_x < -128 || mask_x > 128 || mask_y < -128 || mask_y > 128 ||
        (premultiplied != 0 && premultiplied != 1))
        return 0;
    for (y = 0; y < mask_height; ++y) {
        int32_t dy = mask_y + (int32_t)y;
        unsigned char *row;
        const uint8_t *mask_row;
        if (dy < 0 || (uint32_t)dy >= height) continue;
        row = p + (size_t)dy * stride;
        mask_row = mask + (size_t)y * mask_stride;
        for (x = 0; x < mask_width; ++x) {
            int32_t dx = mask_x + (int32_t)x;
            unsigned k, alpha, coverage, maximum, c;
            unsigned char *pixel;
            if (dx < 0 || (uint32_t)dx >= width) continue;
            k = mask_row[x];
            if (!k) continue;
            pixel = row + (size_t)dx * 4u;
            alpha = pixel[3];
            coverage = alpha > k ? alpha - k : 0u;
            maximum = pixel[0] > pixel[1] ? pixel[0] : pixel[1];
            if (pixel[2] > maximum) maximum = pixel[2];
            /* A mask-last pixel cannot have premultiplied RGB above A'.
             * Allow one 8-bit premultiplied unit for native rounding. Larger
             * values suggest a later overlay: preserve it, do not whiten it.
             * This guard is conservative, not a replacement for excluding
             * known overlay rectangles in the native caller. */
            if (premultiplied ? maximum > coverage + 1u :
                maximum * alpha > coverage * 255u + 255u)
                continue;
            if (!coverage) {
                pixel[0] = pixel[1] = pixel[2] = pixel[3] = 0;
                continue;
            }
            if (!premultiplied) {
                for (c = 0; c < 3u; ++c) {
                    unsigned restored = (pixel[c] * alpha + coverage / 2u) / coverage;
                    pixel[c] = (unsigned char)(restored > 255u ? 255u : restored);
                }
            }
            pixel[3] = (unsigned char)coverage;
        }
    }
    return 1;
}

#endif
