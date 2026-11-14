#include <assert.h>
#include <string.h>
#include "../syspatch_blur/chaos_filter.h"
int main(void) {
    uint16_t input[84*120],old[84*120],v2[84*120],reference[84*120],work[84*120];
    for(unsigned i=0;i<84*120;i++) input[i]=(uint16_t)(i*37u);
    memcpy(old,input,sizeof(input));memcpy(reference,input,sizeof(input));
    chaos_v2_algorithm=0;
    assert(chaos_blur_filter(old,work,84,120,2,2,70));
    assert(rh_blur_filter_rgb565(reference,work,84,120,2,2,70));
    assert(!memcmp(old,reference,sizeof(old))); /* Corona remains bit exact. */
    memcpy(v2,input,sizeof(input));memcpy(reference,input,sizeof(input));
    unsigned char reference_color[84*120*3];
    for(unsigned i=0;i<84*120;i++) {
        unsigned v=input[i];
        reference_color[i*3]=(unsigned char)(((v&31)*255+15)/31);
        reference_color[i*3+1]=(unsigned char)((((v>>5)&63)*255+31)/63);
        reference_color[i*3+2]=(unsigned char)(((v>>11)*255+15)/31);
    }
    syspatch_blur_buf_t b={reference_color,84,120,252,3};syspatch_blur_cfg_t cfg;
    syspatch_blur_cfg_default(&cfg);cfg.radius=12;cfg.quality=SYSPATCH_BLUR_QUALITY_PRECISION;cfg.skip=1;
    assert(syspatch_blur_run(&b,&cfg));
    chaos_v2_algorithm=1;
    assert(chaos_blur_filter(v2,work,84,120,2,2,100));
    assert(!memcmp(chaos_v2_color,reference_color,sizeof(reference_color))); /* Original 8-bit filter. */
    assert(memcmp(v2,old,sizeof(v2))); /* V2 is not an alias of Corona. */
    for(unsigned i=0;i<84*120;i++) v2[i]=0xffff;
    assert(chaos_blur_filter(v2,work,84,120,2,2,70));
    for(unsigned i=0;i<84*120;i++) assert(v2[i]!=0); /* No black accumulator regression. */
    for(unsigned i=0;i<84*120;i++) {
        assert(chaos_v2_color[i*3]==chaos_v2_color[i*3+1]);
        assert(chaos_v2_color[i*3]==chaos_v2_color[i*3+2]); /* White stays neutral after darkening. */
    }
    static unsigned char full[480*1024];
    memset(full,0xcd,sizeof(full));
    for(unsigned y=0;y<120;y++)for(unsigned x=0;x<84;x++)for(unsigned c=0;c<3;c++)
        chaos_v2_color[(y*84+x)*3+c]=(unsigned char)(x*3);
    assert(chaos_v2_store_full(full,1024,sizeof(full)));
    for(unsigned y=0;y<480;y++) {
        for(unsigned x=0;x<336;x++) {
            unsigned char *p=full+y*1024+x*3;
            assert(p[0]==p[1]&&p[1]==p[2]);
            if(x)assert(p[0]-full[y*1024+(x-1)*3]<=1); /* No 4px nearest-neighbour steps. */
        }
        for(unsigned x=1008;x<1024;x++)assert(full[y*1024+x]==0xcd);
    }
    assert(!chaos_v2_store_full(full,1007,sizeof(full)));
    /* Real snapshot path preserves 8-bit gray before any RGB565 conversion. */
    for(unsigned y=0;y<480;y++)for(unsigned x=0;x<336;x++)for(unsigned c=0;c<3;c++)
        full[y*1024+x*3+c]=(unsigned char)(x*255/335);
    chaos_v2_snapshot(full,1024);
    for(unsigned i=0;i<84*120;i++) v2[i]=0x07e0; /* Deliberately green legacy plane. */
    assert(chaos_blur_filter(v2,work,84,120,2,2,70));
    assert(chaos_v2_store_full(full,1024,sizeof(full)));
    for(unsigned y=0;y<480;y++)for(unsigned x=0;x<336;x++) {
        unsigned char *p=full+y*1024+x*3;
        assert(p[0]==p[1]&&p[1]==p[2]);
    }
    assert(!chaos_blur_filter(0,work,84,120,2,2,70));
    assert(!chaos_blur_filter(v2,work,85,120,2,2,70));
    return 0;
}
