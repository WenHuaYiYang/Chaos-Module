/* Rounded premultiplied BGRA glass sampled from the immutable V2 BGR888 image. */
#ifndef CHAOS_CC_BUTTON_PIXELS_H
#define CHAOS_CC_BUTTON_PIXELS_H
#include <stdint.h>
#include <stddef.h>
struct cc_pixels { const unsigned char *data; unsigned width,height,stride,size; };
static unsigned cc_corner_alpha(unsigned x,unsigned y,unsigned w,unsigned h,unsigned r) {
    if (!r || (x>=r && x<w-r) || (y>=r && y<h-r)) return 255u;
    unsigned hits=0, rr=r*8u;
    for (unsigned sy=1;sy<8;sy+=2) for (unsigned sx=1;sx<8;sx+=2) {
        unsigned px=x*8u+sx,py=y*8u+sy;
        unsigned dx=px<rr?rr-px:px>(w-r)*8u?px-(w-r)*8u:0;
        unsigned dy=py<rr?rr-py:py>(h-r)*8u?py-(h-r)*8u:0;
        hits += dx*dx+dy*dy<=rr*rr;
    }
    return (hits*255u+8u)/16u;
}
static int cc_glass_pixels(const struct cc_pixels *src,unsigned char *out,
                          unsigned w,unsigned h,unsigned stride,unsigned size,
                          int left,int top,unsigned radius) {
    if (!src || !src->data || src->width!=336 || src->height!=480 ||
        src->stride<1008 || src->stride>2048 || src->size<src->stride*480u ||
        !out || !w || !h || w>160 || h>128 || stride<w*4u || stride>1024 ||
        size<stride*h || left < -1024 || left > 1024 || top < -2048 || top > 2048) return 0;
    if (radius>w/2) radius=w/2;
    if (radius>h/2) radius=h/2;
    for (unsigned y=0;y<h;++y) for (unsigned x=0;x<w;++x) {
        int sx=left+(int)x,sy=top+(int)y;
        if(sx<0)sx=0;if(sx>335)sx=335;
        if(sy<0)sy=0;if(sy>479)sy=479;
        const unsigned char *p=src->data+(unsigned)sy*src->stride+(unsigned)sx*3u;
        unsigned char *q=out+y*stride+x*4u;
        unsigned a=cc_corner_alpha(x,y,w,h,radius);
        for(unsigned c=0;c<3;++c) q[c]=(unsigned char)((p[c]*88u*a+12750u)/25500u);
        q[3]=(unsigned char)a;
    }
    return 1;
}
#endif
