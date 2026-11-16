#include "../corona_vendor/include/resource_hook_blur.h"
#include "syspatch_blur.c"
#include "../blur_color.h"
#include "../blur_v2_params.h"
static unsigned chaos_v2_algorithm;
/* Versioned parameter layout and bounds are documented in blur_v2_params.h. */
static uint32_t chaos_v2_params[BP_COUNT]=BP_DEFAULTS;
static uint32_t chaos_v2_params_epoch=1;
static unsigned char chaos_v2_color[84*130*3];
static unsigned char chaos_v2_button_color[84*120*3];
static unsigned chaos_v2_color_valid,chaos_v2_width,chaos_v2_height;
static unsigned char chaos_v2_previous_raw[84*120*3];
static unsigned chaos_v2_previous_valid,chaos_v2_snapshot_same,chaos_v2_cached_ready;
static uint32_t chaos_v2_filtered_epoch;
static int chaos_v2_unchanged(void) {
    return chaos_v2_algorithm && chaos_v2_cached_ready && chaos_v2_snapshot_same && chaos_v2_filtered_epoch==chaos_v2_params_epoch;
}
static void chaos_v2_snapshot_plane(const unsigned char *data,unsigned stride,uint16_t *plane) {
    unsigned same=chaos_v2_previous_valid;
    for(unsigned y=0;y<120;y++)for(unsigned x=0;x<84;x++) {
        unsigned sum[3]={0,0,0};
        for(unsigned dy=0;dy<4;dy++) {
            const unsigned char *row=data+(y*4+dy)*stride+x*12;
            for(unsigned dx=0;dx<4;dx++)for(unsigned c=0;c<3;c++) sum[c]+=row[dx*3+c];
        }
        if(plane) plane[y*84+x]=(uint16_t)(((sum[2]/16>>3)<<11)|((sum[1]/16>>2)<<5)|(sum[0]/16>>3));
        for(unsigned c=0;c<3;c++) {
            unsigned i=(y*84+x)*3+c;unsigned char v=(unsigned char)((sum[c]+8)/16);
            same &= chaos_v2_previous_raw[i]==v;
            chaos_v2_previous_raw[i]=chaos_v2_color[i]=v;
        }
    }
    chaos_v2_snapshot_same=same;chaos_v2_previous_valid=1;
    if(!same)chaos_v2_cached_ready=0;
    chaos_v2_color_valid=1;chaos_v2_width=84;chaos_v2_height=120;
}
static void chaos_v2_snapshot(const unsigned char *data,unsigned stride) {
    chaos_v2_snapshot_plane(data,stride,0);
}
static int chaos_v2_store_full(void *pixels,unsigned stride,unsigned size) {
    unsigned char *out=pixels;
    if(!out || stride<336*3 || stride>2048 || size<stride*480 ||
       chaos_v2_width!=84 || chaos_v2_height!=120) return 0;
    /* Exact separable bilinear interpolation. A source row is expanded once,
     * then reused by four output rows; keep 8 fractional bits until the final
     * rounding so this is byte-identical to the former per-pixel formula. */
    /* Serialized by the blur owner; keep 4 KiB off the native UI stack. */
    static uint16_t horizontal[2][336*3];
    unsigned cached[2]={~0u,~0u};
    for(unsigned y=0;y<480;y++) {
        int fy=(int)y*64-96;if(fy<0)fy=0;if(fy>119*256)fy=119*256;
        unsigned sy=(unsigned)fy>>8,ny=sy<119?sy+1:sy,ay=(unsigned)fy&255;
        unsigned rows[2]={sy,ny};
        for(unsigned r=0;r<2;r++) {
            unsigned row=rows[r],slot=row&1u;
            if(cached[slot]==row)continue;
            cached[slot]=row;
            const unsigned char *in=chaos_v2_color+row*84*3;
            for(unsigned x=0;x<336;x++) {
                int fx=(int)x*64-96;if(fx<0)fx=0;if(fx>83*256)fx=83*256;
                unsigned sx=(unsigned)fx>>8,nx=sx<83?sx+1:sx,ax=(unsigned)fx&255;
                for(unsigned c=0;c<3;c++) horizontal[slot][x*3+c]=(uint16_t)(in[sx*3+c]*(256-ax)+in[nx*3+c]*ax);
            }
        }
        unsigned char *line=out+y*stride;
        for(unsigned i=0;i<336*3;i++)
            line[i]=(unsigned char)((horizontal[sy&1u][i]*(256-ay)+horizontal[ny&1u][i]*ay+32768)>>16);
    }
    return 1;
}
/* First-stage port: keep the audited p67 snapshot/retirement and four-surface
 * hooks, replacing only the filter. No q63 pointers or GPU structures copied. */
static int chaos_blur_filter(uint16_t *src, uint16_t *tmp, unsigned w, unsigned h,
                             unsigned radius, unsigned passes, unsigned darken) {
    if (!chaos_v2_algorithm)
        return rh_blur_filter_rgb565(src,tmp,w,h,radius,passes,darken);
    if (!src || !w || !h || w>84 || h>130 || darken>200) return 0;
    if(!chaos_v2_color_valid || chaos_v2_width!=w || chaos_v2_height!=h) {
        for(unsigned i=0;i<w*h;i++){
            unsigned v=src[i];
            chaos_v2_color[i*3]=(unsigned char)(((v&31)*255+15)/31);
            chaos_v2_color[i*3+1]=(unsigned char)((((v>>5)&63)*255+31)/63);
            chaos_v2_color[i*3+2]=(unsigned char)(((v>>11)*255+15)/31);
        }
    }
    chaos_v2_color_valid=0;chaos_v2_width=w;chaos_v2_height=h;
    // Copy the raw snapshot before either filter or brightness can affect it.
    if(w==84 && h==120 && chaos_v2_params[6]!=chaos_v2_params[0]) {
        for(unsigned i=0;i<w*h*3;i++) chaos_v2_button_color[i]=chaos_v2_color[i];
        syspatch_blur_buf_t buttons={chaos_v2_button_color,w,h,w*3,3};
        syspatch_blur_cfg_t button_cfg;
        syspatch_blur_cfg_default(&button_cfg);button_cfg.radius=chaos_v2_params[6];
        button_cfg.quality=SYSPATCH_BLUR_QUALITY_PRECISION;button_cfg.skip=1;
        if(button_cfg.radius) syspatch_blur_run(&buttons,&button_cfg);
    }
    syspatch_blur_buf_t buffer={chaos_v2_color,w,h,w*3,3};
    syspatch_blur_cfg_t config;
    syspatch_blur_cfg_default(&config); config.radius=chaos_v2_params[0];
    config.quality=SYSPATCH_BLUR_QUALITY_PRECISION;config.skip=1;
    if(config.radius) syspatch_blur_run(&buffer,&config);
    // Equal radii have identical results: reuse this pass before darkening.
    if(w==84 && h==120 && chaos_v2_params[6]==chaos_v2_params[0])
        for(unsigned i=0;i<w*h*3;i++) chaos_v2_button_color[i]=chaos_v2_color[i];
    struct chaos_blur_color style={darken,chaos_v2_params[BP_BG_SATURATION],chaos_v2_params[BP_BG_CONTRAST],
        chaos_v2_params[BP_BG_TINT],chaos_v2_params[BP_BG_RED],chaos_v2_params[BP_BG_GREEN],chaos_v2_params[BP_BG_BLUE]};
    static struct chaos_blur_color_lut lut; /* serialized blur owner, not UI stack */
    chaos_blur_color_prepare(&lut,&style);
    for (unsigned i=0;i<w*h;i++) {
        chaos_blur_color_apply_lut(chaos_v2_color+i*3,chaos_v2_color+i*3,&lut);
        unsigned char *p=chaos_v2_color+i*3;
        src[i]=(uint16_t)(((p[2]>>3)<<11)|((p[1]>>2)<<5)|(p[0]>>3));
    }
    chaos_v2_cached_ready=0;
    chaos_v2_filtered_epoch=chaos_v2_params_epoch;
    return 1;
}
