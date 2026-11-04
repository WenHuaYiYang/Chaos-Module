#ifndef RESOURCE_HOOK_BLUR_H
#define RESOURCE_HOOK_BLUR_H
#include <stdint.h>

/* Parameterized Band11 box-pass arithmetic: edge windows shrink rather than
 * replicate pixels, and each axis truncates channels separately. The accepted
 * pipelines keep their own downsample and upscale operations. Bounded
 * arguments cover both supported low-resolution planes and prevent overflow.
 * Optional inherited darkening is applied once before filtering. */
static inline int rh_blur_filter_rgb565(uint16_t *src, uint16_t *tmp,
                                        unsigned width, unsigned height,
                                        unsigned radius, unsigned passes,
                                        unsigned darken_percent) {
    unsigned x,y,axis,pass,i;
    if(!src || !tmp || src==tmp || !width || !height || width>84u ||
       height>130u || radius>8 || passes>8 || darken_percent>100) return 0;
    if(darken_percent!=100) for(i=0;i<width*height;++i) {
        unsigned v=src[i],r=(v>>11)*darken_percent/100;
        unsigned g=((v>>5)&63)*darken_percent/100,b=(v&31)*darken_percent/100;
        src[i]=(uint16_t)((r<<11)|(g<<5)|b);
    }
    for(pass=0;pass<passes;++pass) for(axis=0;axis<2;++axis) {
        uint16_t *in=axis?tmp:src,*out=axis?src:tmp;
        for(y=0;y<height;++y) for(x=0;x<width;++x) {
            unsigned r=0,g=0,b=0,count=0; int d;
            for(d=-(int)radius;d<=(int)radius;++d) {
                int xx=(int)x+(axis?0:d),yy=(int)y+(axis?d:0);
                if(xx>=0 && xx<(int)width && yy>=0 && yy<(int)height) {
                    unsigned v=in[(unsigned)yy*width+(unsigned)xx];
                    r+=v>>11; g+=(v>>5)&63; b+=v&31; ++count;
                }
            }
            out[y*width+x]=(uint16_t)(((r/count)<<11)|((g/count)<<5)|(b/count));
        }
    }
    return 1;
}
#endif
