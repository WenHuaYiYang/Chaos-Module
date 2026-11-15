#include <assert.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include "../control_center_button_pixels.h"
#define cc_glass_pixels(...) cc_glass_pixels_scaled(__VA_ARGS__,1u)
int main(void) {
    unsigned char *input=malloc(1024*480),*out=malloc(656*128+2);
    assert(input && out);memset(input,255,1024*480);memset(out,0xa5,656*128+2);
    struct cc_pixels src={input,336,480,1024,1024*480};
    assert(cc_glass_pixels(&src,out+1,156,128,656,656*128,20,30,36));
    assert(out[0]==0xa5 && out[656*128+1]==0xa5);
    for(unsigned y=0;y<128;++y) for(unsigned x=0;x<156;++x) {
        const unsigned char *p=out+1+y*656+x*4;
        assert(p[0]==p[1] && p[1]==p[2] && p[0]<=p[3]);
        assert(p[3]==cc_corner_alpha(155-x,127-y,156,128,36));
    }
    assert(out[1+3]==0 && out[1+36*4+3]==255);
    assert(out[1+(64*656+78*4)]>=120 && out[1+(64*656+78*4)]<=124);
    assert(out[1+36*4]>out[1+3*656+36*4]); /* subtle inner edge */
    unsigned partial=0;
    for(unsigned y=0;y<128;++y) {
        for(unsigned x=624;x<656;++x) assert(out[1+y*656+x]==0xa5);
        for(unsigned x=0;x<156;++x) { unsigned a=out[1+y*656+x*4+3];partial+=a>0&&a<255; }
    }
    assert(partial>0);
    for(unsigned y=0;y<480;++y)for(unsigned x=0;x<336;++x) {
        input[y*1024+x*3]=(unsigned char)x;
        input[y*1024+x*3+1]=(unsigned char)y;
        input[y*1024+x*3+2]=100;
    }
    assert(cc_glass_pixels(&src,out,160,66,640,640*66,-1,-1,0));
    assert(out[0]>=8 && out[1]==out[0] && out[2]>out[0] && out[3]==255);
    assert(out[640+4]==out[640+5]);
    assert(cc_glass_pixels(&src,out,1,1,4,4,1000,1000,0));
    assert(out[0]>=8 && out[0]<=134 && out[1]>out[0]);
    unsigned char saved[4];memcpy(saved,out,4);
    assert(!cc_glass_pixels(&src,out,161,128,656,656*128,0,0,36));
    assert(!cc_glass_pixels(&src,out,160,129,640,640*128,0,0,36));
    assert(!cc_glass_pixels(&src,out,160,128,639,640*128,0,0,36));
    assert(!cc_glass_pixels(&src,out,160,128,640,640*128-1,0,0,36));
    assert(!cc_glass_pixels(&src,out,160,128,640,640*128,2000,0,36));
    src.stride=1;assert(!cc_glass_pixels(&src,out,1,1,4,4,0,0,0));
    assert(!memcmp(saved,out,4));
    src.stride=1024;
    /* Bright/dark saturated sources all keep white icons distinct. Exhaust
     * the RGB cube corners and gray ramp, without duplicating the tone map. */
    for(unsigned v=0;v<264;++v) {
        unsigned char b=v<256?v:(v&1?255:0);
        unsigned char g=v<256?v:(v&2?255:0);
        unsigned char r=v<256?v:(v&4?255:0);
        for(unsigned y=0;y<480;++y)for(unsigned x=0;x<336;++x) {
            unsigned char *p=input+y*1024+x*3;p[0]=b;p[1]=g;p[2]=r;
        }
        assert(cc_glass_pixels(&src,out,156,128,656,656*128,0,0,36));
        const unsigned char *center=out+64*656+78*4;
        for(unsigned c=0;c<3;++c) assert(center[c]>=8 && center[c]<=124);
        if(v<256) assert(center[0]==center[1] && center[1]==center[2]);
        if(v==0) assert(out[36*4]>center[0]);
    }
    /* Half-size texture samples the same physical span for GPU 2x scaling. */
    for(unsigned y=0;y<480;++y)for(unsigned x=0;x<336;++x)
        for(unsigned c=0;c<3;++c) input[y*1024+x*3+c]=(unsigned char)(x/2);
    assert(cc_glass_pixels_scaled(&src,out,78,64,320,320*64,10,10,18,2));
    assert(out[32*320+60*4]>out[32*320+20*4]+12);
    assert(out[3]==0 && out[32*320+39*4+3]==255);
    assert(!cc_glass_pixels_scaled(&src,out,78,64,320,320*64,0,0,18,0));
    free(input);free(out);
    puts("CC glass pixels: bounded contrast over white/black/saturated wallpaper, neutral gray, inner rim, 4x4 corner AA, crop/padding/bounds passed");
}
