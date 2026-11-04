#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "resource_hook_p67_battery.h"

static void test_unmatte_examples(void) {
    const uint8_t mask[] = {128,128,128,128,255,0,0,255};
    uint8_t straight[] = {
        127,127,127,255, 0,0,127,255, 0,127,0,255, 0,0,85,192,
        0,0,0,255, 60,60,60,255, 0,0,0,255, 100,100,100,255
    };
    const uint8_t expected[] = {
        255,255,255,127, 0,0,255,127, 0,255,0,127, 0,0,255,64,
        0,0,0,0, 60,60,60,255, 0,0,0,255, 100,100,100,255
    };
    assert(p67_battery_unmatte_black(straight, sizeof(straight), sizeof(straight),
                                    8, 1, mask, sizeof(mask), 8, 8, 1, 0, 0, 0));
    assert(!memcmp(straight, expected, sizeof(expected)));
    uint8_t premult[] = {
        127,127,127,255, 0,0,127,255, 0,127,0,255, 0,0,64,192,
        0,0,0,255, 60,60,60,255, 0,0,0,255, 100,100,100,255
    };
    const uint8_t expected_pm[] = {
        127,127,127,127, 0,0,127,127, 0,127,0,127, 0,0,64,64,
        0,0,0,0, 60,60,60,255, 0,0,0,255, 100,100,100,255
    };
    assert(p67_battery_unmatte_black(premult, sizeof(premult), sizeof(premult),
                                    8, 1, mask, sizeof(mask), 8, 8, 1, 0, 0, 1));
    assert(!memcmp(premult, expected_pm, sizeof(expected_pm)));
    /* A later translucent white overlay is incompatible with mask-last RGB.
     * Preserve it instead of interpreting it as a contaminated battery edge. */
    const uint8_t half_mask = 128;
    uint8_t overlay[] = {192,192,192,255};
    assert(p67_battery_unmatte_black(overlay, 4, 4, 1, 1,
                                    &half_mask, 1, 1, 1, 1, 0, 0, 0));
    assert(overlay[0] == 192 && overlay[1] == 192 && overlay[2] == 192 && overlay[3] == 255);
    assert(p67_battery_unmatte_black(overlay, 4, 4, 1, 1,
                                    &half_mask, 1, 1, 1, 1, 0, 0, 1));
    assert(overlay[0] == 192 && overlay[3] == 255);
}

static void test_clipping_and_padding(void) {
    uint8_t image[3 * 20], before[sizeof(image)];
    uint8_t mask[3 * 5] = {0,0,0,0,0xee, 0,128,0,255,0xee, 0,0,0,0,0xee};
    memset(image, 0xa5, sizeof(image));
    for (unsigned y = 0; y < 3; ++y)
        for (unsigned x = 0; x < 4; ++x) {
            uint8_t *p = image + y * 20 + x * 4;
            p[0] = p[1] = p[2] = 127; p[3] = 255;
        }
    image[2 * 4] = image[2 * 4 + 1] = image[2 * 4 + 2] = 0;
    memcpy(before, image, sizeof(image));
    assert(p67_battery_unmatte_black(image, sizeof(image), 20, 4, 3,
                                    mask, sizeof(mask), 5, 4, 3, -1, -1, 0));
    assert(image[0] == 255 && image[1] == 255 && image[2] == 255 && image[3] == 127);
    assert(!image[8] && !image[9] && !image[10] && !image[11]);
    for (unsigned i = 0; i < sizeof(image); ++i)
        if (i >= 4 && (i < 8 || i >= 12)) assert(image[i] == before[i]);
    /* Entirely clipped masks are valid no-ops, even with opaque mask padding. */
    memcpy(before, image, sizeof(image));
    assert(p67_battery_unmatte_black(image, sizeof(image), 20, 4, 3,
                                    mask, sizeof(mask), 5, 4, 3, 128, 128, 0));
    assert(!memcmp(image, before, sizeof(image)));
}

static void test_invalid_unmatte(void) {
    uint8_t image[48], before[sizeof(image)], mask[12];
    memset(image, 0x7f, sizeof(image)); memset(mask, 128, sizeof(mask));
    memcpy(before, image, sizeof(image));
#define BAD_UNMATTE(p,s,st,w,h,m,ms,mst,mw,mh,x,y,pm) \
    assert(!p67_battery_unmatte_black((p),(s),(st),(w),(h),(m),(ms),(mst),(mw),(mh),(x),(y),(pm)))
    BAD_UNMATTE(NULL,48,24,5,2,mask,12,6,5,2,0,0,0);
    BAD_UNMATTE(image,48,24,5,2,NULL,12,6,5,2,0,0,0);
    BAD_UNMATTE(image,47,24,5,2,mask,12,6,5,2,0,0,0);
    BAD_UNMATTE(image,48,19,5,2,mask,12,6,5,2,0,0,0);
    BAD_UNMATTE(image,48,SIZE_MAX,5,2,mask,12,6,5,2,0,0,0);
    BAD_UNMATTE(image,48,24,0,2,mask,12,6,5,2,0,0,0);
    BAD_UNMATTE(image,48,24,5,0,mask,12,6,5,2,0,0,0);
    BAD_UNMATTE(image,48,24,129,2,mask,12,6,5,2,0,0,0);
    BAD_UNMATTE(image,48,24,5,129,mask,12,6,5,2,0,0,0);
    BAD_UNMATTE(image,48,24,5,2,mask,11,6,5,2,0,0,0);
    BAD_UNMATTE(image,48,24,5,2,mask,12,4,5,2,0,0,0);
    BAD_UNMATTE(image,48,24,5,2,mask,12,SIZE_MAX,5,2,0,0,0);
    BAD_UNMATTE(image,48,24,5,2,mask,12,6,0,2,0,0,0);
    BAD_UNMATTE(image,48,24,5,2,mask,12,6,5,0,0,0,0);
    BAD_UNMATTE(image,48,24,5,2,mask,12,6,129,2,0,0,0);
    BAD_UNMATTE(image,48,24,5,2,mask,12,6,5,129,0,0,0);
    BAD_UNMATTE(image,48,24,5,2,mask,12,6,5,2,INT32_MIN,0,0);
    BAD_UNMATTE(image,48,24,5,2,mask,12,6,5,2,0,INT32_MAX,0);
    BAD_UNMATTE(image,48,24,5,2,mask,12,6,5,2,-129,0,0);
    BAD_UNMATTE(image,48,24,5,2,mask,12,6,5,2,0,129,0);
    BAD_UNMATTE(image,48,24,5,2,mask,12,6,5,2,0,0,2);
#undef BAD_UNMATTE
    assert(!memcmp(image, before, sizeof(image)));
}

static unsigned round255(unsigned value) { return (value + 127u) / 255u; }
static unsigned composite(unsigned color, unsigned alpha, unsigned background, int premultiplied) {
    return (premultiplied ? color : round255(color * alpha)) + round255(background * (255u - alpha));
}
static void test_bright_background_composition(void) {
    const unsigned masks[] = {16,32,64,96,112,127,128,175,191,207,223,239,255};
    const unsigned alphas[] = {255,192,64,0};
    const unsigned colors[][3] = {{255,255,255},{0,0,255},{0,255,0},{35,72,91}};
    const unsigned backgrounds[][3] = {{255,255,255},{220,240,255},{255,224,190}};
    for (unsigned pm = 0; pm < 2; ++pm)
        for (unsigned k = 0; k < sizeof(masks) / sizeof(masks[0]); ++k)
            for (unsigned a = 0; a < sizeof(alphas) / sizeof(alphas[0]); ++a)
                for (unsigned c = 0; c < sizeof(colors) / sizeof(colors[0]); ++c) {
                    unsigned coverage = round255(alphas[a] * (255u - masks[k]));
                    unsigned output_alpha = masks[k] + coverage;
                    uint8_t pixel[4], mask = (uint8_t)masks[k];
                    for (unsigned channel = 0; channel < 3; ++channel) {
                        unsigned premult_color = round255(colors[c][channel] * coverage);
                        pixel[channel] = (uint8_t)(pm ? premult_color :
                            (premult_color * 255u + output_alpha / 2u) / output_alpha);
                    }
                    pixel[3] = (uint8_t)output_alpha;
                    assert(p67_battery_unmatte_black(pixel, 4, 4, 1, 1,
                                                    &mask, 1, 1, 1, 1, 0, 0, (int)pm));
                    assert(pixel[3] == coverage);
                    for (unsigned b = 0; b < sizeof(backgrounds) / sizeof(backgrounds[0]); ++b)
                        for (unsigned channel = 0; channel < 3; ++channel) {
                            unsigned actual = composite(pixel[channel], pixel[3], backgrounds[b][channel], (int)pm);
                            unsigned expected = composite(colors[c][channel], coverage, backgrounds[b][channel], 0);
                            unsigned difference = actual > expected ? actual - expected : expected - actual;
                            /* Native 8-bit capture quantization loses some original
                             * RGB information. Compare composed output, not unstable
                             * unassociated RGB at nearly zero alpha. */
                            assert(difference <= 2u);
                        }
                }
}

int main(void) {
    test_unmatte_examples(); test_clipping_and_padding();
    test_invalid_unmatte(); test_bright_background_composition();
    puts("p67 battery mask unmatte pixels: ok");
    return 0;
}
