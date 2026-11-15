/* Shared V2 material atlas and cached rounded masks for real-time GPU sprites. */
#ifndef CHAOS_CC_BUTTON_PIXELS_H
#define CHAOS_CC_BUTTON_PIXELS_H
#include <stdint.h>
#include <stddef.h>
#define CC_ATLAS_W 168u
#define CC_ATLAS_H 240u
struct cc_pixels { const unsigned char *data; unsigned width,height,stride,size; };
static unsigned cc_corner_alpha(unsigned x,unsigned y,unsigned w,unsigned h,unsigned r) {
    if(!r || (x>=r && x<w-r) || (y>=r && y<h-r)) return 255u;
    unsigned hits=0,rr=r*8u;
    for(unsigned sy=1;sy<8;sy+=2) for(unsigned sx=1;sx<8;sx+=2) {
        unsigned px=x*8u+sx,py=y*8u+sy;
        unsigned dx=px<rr?rr-px:px>(w-r)*8u?px-(w-r)*8u:0;
        unsigned dy=py<rr?rr-py:py>(h-r)*8u?py-(h-r)*8u:0;
        hits+=dx*dx+dy*dy<=rr*rr;
    }
    return (hits*255u+8u)/16u;
}
static int cc_build_atlas(const struct cc_pixels *source,uint32_t *atlas,unsigned brightness) {
    if(!source || !source->data || !atlas || source->width!=336 || source->height!=480 ||
       source->stride<1008 || source->stride>2048 || source->size<source->stride*480u || brightness>100) return 0;
    for(unsigned y=0;y<CC_ATLAS_H;++y)for(unsigned x=0;x<CC_ATLAS_W;++x) {
        const unsigned char *p=source->data+y*2u*source->stride+x*6u;
        unsigned lum=(p[0]*19u+p[1]*183u+p[2]*54u+128u)>>8,packed=0xff000000u;
        for(unsigned c=0;c<3;++c) {
            unsigned neutral=(p[c]*3u+lum+2u)/4u;
            unsigned value=(neutral*brightness+50u)/100u;
            packed|=value<<(c*8u);
        }
        atlas[y*CC_ATLAS_W+x]=packed;
    }
    return 1;
}
static int cc_prepare_mask(unsigned char *out,unsigned w,unsigned h,unsigned stride,unsigned size,unsigned radius) {
    if(!out || !w || !h || w>160 || h>128 || stride<w*4 || stride>1024 || size<stride*h) return 0;
    if(radius>w/2)radius=w/2;if(radius>h/2)radius=h/2;
    for(unsigned y=0;y<h;++y)for(unsigned x=0;x<w;++x) {
        unsigned char *p=out+y*stride+x*4;p[0]=p[1]=p[2]=0;
        p[3]=(unsigned char)cc_corner_alpha(x,y,w,h,radius);
    }
    return 1;
}
static int cc_copy_atlas(const uint32_t *atlas,unsigned char *out,unsigned w,unsigned h,
                         unsigned stride,unsigned size,int left,int top,unsigned scale) {
    if(!atlas || !out || ((uintptr_t)out&3u) || (stride&3u) || !w || !h || w>160 || h>128 || stride<w*4 || stride>1024 || size<stride*h ||
       (scale!=1 && scale!=2) || left < -1024 || left>1024 || top < -2048 || top>2048) return 0;
    for(unsigned y=0;y<h;++y) {
        int sy=(top+(int)(y*scale))/2;if(sy<0)sy=0;if(sy>239)sy=239;
        for(unsigned x=0;x<w;++x) {
            int sx=(left+(int)(x*scale))/2;if(sx<0)sx=0;if(sx>167)sx=167;
            unsigned char *q=out+y*stride+x*4;unsigned a=q[3];
            uint32_t pixel=atlas[(unsigned)sy*CC_ATLAS_W+(unsigned)sx];
            /* Fully covered interior is one word copy. Only rounded-edge
             * pixels need premultiplication; alpha geometry is never rerun. */
            if(a==255u) { *(uint32_t *)(void *)q=pixel;continue; }
            for(unsigned c=0;c<3;++c) q[c]=(unsigned char)((((pixel>>(c*8u))&255u)*a+127u)/255u);
        }
    }
    return 1;
}
#endif
