/* Rounded premultiplied BGRA glass sampled from the immutable V2 BGR888 image. */
#ifndef CHAOS_CC_BUTTON_PIXELS_H
#define CHAOS_CC_BUTTON_PIXELS_H
#include <stdint.h>
#include <stddef.h>
struct cc_pixels { const unsigned char *data; unsigned width,height,stride,size; };
/* Retain wallpaper variation. Contrast comes from a translucent dark tint,
 * not crushing every input into a narrow, nearly solid gray interval. */
static unsigned cc_readable_channel(unsigned channel,unsigned luminance,unsigned sheen,unsigned rim) {
    unsigned neutral=(channel*3u+luminance+2u)/4u;
    return 8u+(neutral*44u+50u)/100u+sheen+rim;
}
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
static int cc_glass_pixels_scaled(const struct cc_pixels *src,unsigned char *out,
                          unsigned w,unsigned h,unsigned stride,unsigned size,
                          int left,int top,unsigned radius,unsigned sample_scale) {
    if (!src || !src->data || src->width!=336 || src->height!=480 ||
        src->stride<1008 || src->stride>2048 || src->size<src->stride*480u ||
        !out || !w || !h || w>160 || h>128 || stride<w*4u || stride>1024 ||
        size<stride*h || left < -1024 || left > 1024 || top < -2048 || top > 2048 ||
        (sample_scale!=1 && sample_scale!=2)) return 0;
    if (radius>w/2) radius=w/2;
    if (radius>h/2) radius=h/2;
    for (unsigned y=0;y<h;++y) for (unsigned x=0;x<w;++x) {
        int sx=left+(int)(x*sample_scale),sy=top+(int)(y*sample_scale);
        if(sx<0)sx=0;if(sx>335)sx=335;
        if(sy<0)sy=0;if(sy>479)sy=479;
        const unsigned char *p=src->data+(unsigned)sy*src->stride+(unsigned)sx*3u;
        unsigned char *q=out+y*stride+x*4u;
        unsigned a=cc_corner_alpha(x,y,w,h,radius);
        unsigned luminance=(p[0]*19u+p[1]*183u+p[2]*54u+128u)>>8;
        unsigned sheen=h>1?4u*(h-1u-y)/(h-1u):0;
        /* One-pixel inner keyline, with coverage following the same rounded
         * edge. It belongs to the immutable texture, never overlays the icon. */
        unsigned inner=0;
        if(x>0 && y>0 && x+1<w && y+1<h)
            inner=cc_corner_alpha(x-1,y-1,w-2,h-2,radius?radius-1:0);
        unsigned rim=a>inner?(10u*(a-inner)+127u)/255u:0;
        for(unsigned c=0;c<3;++c) {
            unsigned value=cc_readable_channel(p[c],luminance,sheen,rim);
            q[c]=(unsigned char)((value*a+127u)/255u);
        }
        q[3]=(unsigned char)a;
    }
    return 1;
}
#endif
