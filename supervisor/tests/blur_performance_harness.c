/* Pixel-equivalence and publication-cache regressions; host timings are not FPS. */
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <time.h>
#include "../syspatch_blur/chaos_filter.h"
static unsigned char input[480*1024],output[480*1024],reference[480*1024];
static uint16_t plane[84*120],work[84*120];
static void scalar(unsigned char *out,unsigned stride){
    for(unsigned y=0;y<480;y++)for(unsigned x=0;x<336;x++){
        int fx=(int)x*64-96,fy=(int)y*64-96;
        if(fx<0)fx=0;if(fy<0)fy=0;
        if(fx>83*256)fx=83*256;if(fy>119*256)fy=119*256;
        unsigned sx=(unsigned)fx>>8,sy=(unsigned)fy>>8,ax=(unsigned)fx&255,ay=(unsigned)fy&255;
        unsigned nx=sx<83?sx+1:sx,ny=sy<119?sy+1:sy;
        for(unsigned c=0;c<3;c++){
            unsigned top=chaos_v2_color[(sy*84+sx)*3+c]*(256-ax)+chaos_v2_color[(sy*84+nx)*3+c]*ax;
            unsigned bottom=chaos_v2_color[(ny*84+sx)*3+c]*(256-ax)+chaos_v2_color[(ny*84+nx)*3+c]*ax;
            out[y*stride+x*3+c]=(unsigned char)((top*(256-ay)+bottom*ay+32768)>>16);
        }
    }
}
int main(void){
    uint32_t random=7;
    for(unsigned i=0;i<sizeof(input);i++){random=random*1664525u+1013904223u;input[i]=(unsigned char)(random>>24);}
    chaos_v2_algorithm=1;chaos_v2_snapshot(input,1024);
    assert(!chaos_v2_unchanged());
    for(unsigned y=0;y<120;y++)for(unsigned x=0;x<84;x++)for(unsigned c=0;c<3;c++){
        unsigned sum=0;for(unsigned dy=0;dy<4;dy++)for(unsigned dx=0;dx<4;dx++)sum+=input[(y*4+dy)*1024+(x*4+dx)*3+c];
        assert(chaos_v2_color[(y*84+x)*3+c]==(sum+8)/16);
    }
    assert(chaos_blur_filter(plane,work,84,120,2,2,70));
    assert(!chaos_v2_unchanged()); // A failed/stale publication must not arm reuse.
    chaos_v2_cached_ready=1;
    for(unsigned stride=1008;stride<=1024;stride+=16){
        memset(output,0xcd,sizeof(output));memset(reference,0xcd,sizeof(reference));
        scalar(reference,stride);assert(chaos_v2_store_full(output,stride,sizeof(output)));
        assert(!memcmp(output,reference,sizeof(output)));
    }
    chaos_v2_snapshot(input,1024);assert(chaos_v2_unchanged());
    ++chaos_v2_params_epoch;assert(!chaos_v2_unchanged());
    assert(chaos_blur_filter(plane,work,84,120,2,2,70));chaos_v2_cached_ready=1;
    chaos_v2_snapshot(input,1024);assert(chaos_v2_unchanged());
    for(unsigned y=0;y<4;y++)for(unsigned x=0;x<12;x++)input[y*1024+x]=0;
    chaos_v2_snapshot(input,1024);assert(!chaos_v2_unchanged());
    chaos_v2_snapshot(input,1024);assert(!chaos_v2_unchanged()); // Previous changed attempt never published.
    assert(chaos_blur_filter(plane,work,84,120,2,2,70));chaos_v2_cached_ready=1;
    chaos_v2_snapshot(input,1024);assert(chaos_v2_unchanged());
    chaos_v2_algorithm=0;assert(!chaos_v2_unchanged());
    clock_t begin=clock();for(unsigned i=0;i<40;i++){chaos_v2_color[i]++;scalar(reference,1024);}
    double old=(double)(clock()-begin)/CLOCKS_PER_SEC;
    begin=clock();for(unsigned i=0;i<40;i++){chaos_v2_color[i]++;assert(chaos_v2_store_full(output,1024,sizeof(output)));}
    double fast=(double)(clock()-begin)/CLOCKS_PER_SEC;
    printf("PASS exact snapshot and bilinear pixels/padding; cache publication/config/change gates\n");
    printf("HOST full-frame interpolation x40: scalar %.3f ms, separable %.3f ms (%.2fx); not device FPS\n",old*1000,fast*1000,fast?old/fast:0);
}
