#include "../corona_vendor/include/resource_hook_blur.h"
#include "syspatch_blur.c"
static unsigned chaos_v2_algorithm;
static unsigned char chaos_v2_color[84*130*3];
static unsigned chaos_v2_color_valid,chaos_v2_width,chaos_v2_height;
static void chaos_v2_snapshot(const unsigned char *data,unsigned stride) {
    for(unsigned y=0;y<120;y++)for(unsigned x=0;x<84;x++)for(unsigned c=0;c<3;c++){
        unsigned sum=0;
        for(unsigned dy=0;dy<4;dy++)for(unsigned dx=0;dx<4;dx++)
            sum+=data[(y*4+dy)*stride+(x*4+dx)*3+c];
        chaos_v2_color[(y*84+x)*3+c]=(unsigned char)((sum+8)/16);
    }
    chaos_v2_color_valid=1;chaos_v2_width=84;chaos_v2_height=120;
}
static int chaos_v2_store_full(void *pixels,unsigned stride,unsigned size) {
    unsigned char *out=pixels;
    if(!out || stride<336*3 || stride>2048 || size<stride*480 ||
       chaos_v2_width!=84 || chaos_v2_height!=120) return 0;
    for(unsigned y=0;y<480;y++)for(unsigned x=0;x<336;x++) {
        int fx=(int)x*64-96,fy=(int)y*64-96;
        if(fx<0)fx=0;if(fy<0)fy=0;
        if(fx>83*256)fx=83*256;if(fy>119*256)fy=119*256;
        unsigned sx=(unsigned)fx>>8,sy=(unsigned)fy>>8,ax=(unsigned)fx&255,ay=(unsigned)fy&255;
        unsigned nx=sx<83?sx+1:sx,ny=sy<119?sy+1:sy;
        for(unsigned c=0;c<3;c++) {
            unsigned top=chaos_v2_color[(sy*84+sx)*3+c]*(256-ax)+chaos_v2_color[(sy*84+nx)*3+c]*ax;
            unsigned bottom=chaos_v2_color[(ny*84+sx)*3+c]*(256-ax)+chaos_v2_color[(ny*84+nx)*3+c]*ax;
            out[y*stride+x*3+c]=(unsigned char)((top*(256-ay)+bottom*ay+32768)>>16);
        }
    }
    return 1;
}
/* First-stage port: keep the audited p67 snapshot/retirement and four-surface
 * hooks, replacing only the filter. No q63 pointers or GPU structures copied. */
static int chaos_blur_filter(uint16_t *src, uint16_t *tmp, unsigned w, unsigned h,
                             unsigned radius, unsigned passes, unsigned darken) {
    if (!chaos_v2_algorithm)
        return rh_blur_filter_rgb565(src,tmp,w,h,radius,passes,darken);
    if (!src || !w || !h || w>84 || h>130 || darken>100) return 0;
    if(!chaos_v2_color_valid || chaos_v2_width!=w || chaos_v2_height!=h) {
        for(unsigned i=0;i<w*h;i++){
            unsigned v=src[i];
            chaos_v2_color[i*3]=(unsigned char)(((v&31)*255+15)/31);
            chaos_v2_color[i*3+1]=(unsigned char)((((v>>5)&63)*255+31)/63);
            chaos_v2_color[i*3+2]=(unsigned char)(((v>>11)*255+15)/31);
        }
    }
    chaos_v2_color_valid=0;chaos_v2_width=w;chaos_v2_height=h;
    syspatch_blur_buf_t buffer={chaos_v2_color,w,h,w*3,3};
    syspatch_blur_cfg_t config;
    syspatch_blur_cfg_default(&config); config.radius=12;
    config.quality=SYSPATCH_BLUR_QUALITY_PRECISION;config.skip=1;
    syspatch_blur_run(&buffer,&config);
    for (unsigned i=0;i<w*h;i++) {
        for(unsigned c=0;c<3;c++) chaos_v2_color[i*3+c]=(unsigned char)((chaos_v2_color[i*3+c]*darken+50)/100);
        unsigned char *p=chaos_v2_color+i*3;
        src[i]=(uint16_t)(((p[2]>>3)<<11)|((p[1]>>2)<<5)|(p[0]>>3));
    }
    return 1;
}
