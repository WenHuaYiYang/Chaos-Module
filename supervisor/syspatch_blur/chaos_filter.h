#include "../corona_vendor/include/resource_hook_blur.h"
#include "syspatch_blur.c"
static unsigned chaos_v2_algorithm;
/* First-stage port: keep the audited p67 snapshot/retirement and four-surface
 * hooks, replacing only the filter. No q63 pointers or GPU structures copied. */
static int chaos_blur_filter(uint16_t *src, uint16_t *tmp, unsigned w, unsigned h,
                             unsigned radius, unsigned passes, unsigned darken) {
    if (!chaos_v2_algorithm)
        return rh_blur_filter_rgb565(src,tmp,w,h,radius,passes,darken);
    if (!src || !w || !h || w>84 || h>130 || darken>100) return 0;
    syspatch_blur_buf_t buffer={src,w,h,w*2,2};
    syspatch_blur_cfg_t config;
    syspatch_blur_cfg_default(&config); config.radius=12;
    syspatch_blur_run(&buffer,&config);
    for (unsigned i=0;i<w*h;i++) {
        unsigned v=src[i],r=(v>>11)*darken/100;
        unsigned g=((v>>5)&63)*darken/100,b=(v&31)*darken/100;
        src[i]=(uint16_t)((r<<11)|(g<<5)|b);
    }
    return 1;
}
