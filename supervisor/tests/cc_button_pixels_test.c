#include <assert.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include "../control_center_button_pixels.h"
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
    assert(out[1+3]==0 && out[1+36*4+3]==255 && out[1+36*4]==224);
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
    assert(out[0]==0 && out[1]==0 && out[2]==88 && out[3]==255);
    assert(out[640+4]==0 && out[640+5]==0);
    assert(cc_glass_pixels(&src,out,1,1,4,4,1000,1000,0));
    assert(out[0]==((335%256)*88+50)/100 && out[1]==((479%256)*88+50)/100);
    unsigned char saved[4];memcpy(saved,out,4);
    assert(!cc_glass_pixels(&src,out,161,128,656,656*128,0,0,36));
    assert(!cc_glass_pixels(&src,out,160,129,640,640*128,0,0,36));
    assert(!cc_glass_pixels(&src,out,160,128,639,640*128,0,0,36));
    assert(!cc_glass_pixels(&src,out,160,128,640,640*128-1,0,0,36));
    assert(!cc_glass_pixels(&src,out,160,128,640,640*128,2000,0,36));
    src.stride=1;assert(!cc_glass_pixels(&src,out,1,1,4,4,0,0,0));
    assert(!memcmp(saved,out,4));
    free(input);free(out);
    puts("CC glass pixels: 4x4 corner AA, premultiplied neutral/dark color, source crop/clamp, padding and bounds passed");
}
