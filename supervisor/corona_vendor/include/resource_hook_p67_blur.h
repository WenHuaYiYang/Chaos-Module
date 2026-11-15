#ifndef RESOURCE_HOOK_P67_BLUR_H
#define RESOURCE_HOOK_P67_BLUR_H
#include <stdint.h>
#include <stddef.h>
#include "resource_hook_blur.h"

#define P67_BLUR_WIDTH 84u
#define P67_BLUR_HEIGHT 120u
#define P67_BLUR_PIXELS (P67_BLUR_WIDTH * P67_BLUR_HEIGHT)
/* This view contains an already validated, coherent CPU alias. No native
 * descriptor/pixel pointer is retained by these helpers. Format15 is BGR888
 * only when the caller's exact-target channel-order gate has passed. */
struct p67_blur_pixel_view {
    const unsigned char *pixels;
    size_t size, stride;
    unsigned width, height, format;
};
static inline int p67_blur_view_valid(const struct p67_blur_pixel_view *v) {
    size_t row;
    if(!v || !v->pixels || v->format!=15 || v->width!=336 || v->height!=480) return 0;
    row=(size_t)v->width*3;
    if(v->stride<row || v->stride>SIZE_MAX/v->height) return 0;
    /* Require full stride storage: never trust a malformed native size. */
    return v->size>=v->stride*v->height;
}
static inline int p67_blur_downsample_bgr888(const struct p67_blur_pixel_view *v,
                                            uint16_t *out, size_t count) {
    unsigned x,y,dx,dy;
    if(!out || count<P67_BLUR_PIXELS || !p67_blur_view_valid(v)) return 0;
    for(y=0;y<P67_BLUR_HEIGHT;++y) for(x=0;x<P67_BLUR_WIDTH;++x) {
        unsigned r=0,g=0,b=0;
        for(dy=0;dy<4;++dy) for(dx=0;dx<4;++dx) {
            const unsigned char *p=v->pixels+(y*4+dy)*v->stride+(x*4+dx)*3;
            b+=p[0]; g+=p[1]; r+=p[2];
        }
        out[y*P67_BLUR_WIDTH+x]=(uint16_t)(((r/16>>3)<<11)|((g/16>>2)<<5)|(b/16>>3));
    }
    return 1;
}
/* Publish a separate immutable 84x120 format18 image. Native image drawing
 * performs the 4x transform; CPU work never expands this to screen resolution.
 * Little-endian pixels support unaligned padded rows without touching padding. */
static inline int p67_blur_store_rgb565(const uint16_t *in, void *pixels,
                                       size_t stride, size_t size) {
    unsigned x,y;
    unsigned char *out=pixels;
    if(!in || !out || stride<P67_BLUR_WIDTH*2u ||
       stride>SIZE_MAX/P67_BLUR_HEIGHT || size<stride*P67_BLUR_HEIGHT) return 0;
    /* Publication storage must not alias scratch, even with overlapping rows. */
    const uintptr_t source=(uintptr_t)in,target=(uintptr_t)out;
    const size_t input_bytes=P67_BLUR_PIXELS*sizeof(*in),output_bytes=stride*P67_BLUR_HEIGHT;
    if(source>UINTPTR_MAX-input_bytes || target>UINTPTR_MAX-output_bytes ||
       (target>=source ? target-source<input_bytes : source-target<output_bytes)) return 0;
    for(y=0;y<P67_BLUR_HEIGHT;++y) for(x=0;x<P67_BLUR_WIDTH;++x) {
        unsigned v=in[y*P67_BLUR_WIDTH+x];
        out[y*stride+x*2u]=(unsigned char)v;
        out[y*stride+x*2u+1u]=(unsigned char)(v>>8);
    }
    return 1;
}
#endif
