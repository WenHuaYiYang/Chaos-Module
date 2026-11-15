#include <assert.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include "../control_center_button_pixels.h"
int main(void) {
    unsigned char *input=malloc(1024*480),*out=malloc(320*64+4);
    uint32_t *atlas=malloc(CC_ATLAS_W*CC_ATLAS_H*4);
    assert(input && out && atlas);
    struct cc_pixels src={input,336,480,1024,1024*480};
    memset(input,255,1024*480);memset(out,0xa5,320*64+4);
    assert(cc_build_atlas(&src,atlas,44));
    assert(cc_prepare_mask(out,78,64,320,320*64,18));
    assert(cc_copy_atlas(atlas,out,78,64,320,320*64,20,30,2));
    unsigned partial=0;
    for(unsigned y=0;y<64;++y) {
        for(unsigned x=312;x<320;++x)assert(out[y*320+x]==0xa5);
        for(unsigned x=0;x<78;++x) {
            unsigned char *p=out+y*320+x*4;unsigned a=p[3];
            assert(p[0]==p[1] && p[1]==p[2] && p[0]<=a);
            assert(a==cc_corner_alpha(77-x,63-y,78,64,18));
            partial+=a>0&&a<255;
        }
    }
    assert(partial && out[3]==0 && out[32*320+39*4]==112);
    for(unsigned i=320*64;i<320*64+4;++i)assert(out[i]==0xa5);
    for(unsigned y=0;y<480;++y)for(unsigned x=0;x<336;++x)
        for(unsigned c=0;c<3;++c)input[y*1024+x*3+c]=(unsigned char)(y/2);
    assert(cc_build_atlas(&src,atlas,100));
    /* A moving button updates its samples while retaining every alpha byte. */
    unsigned char mask[78*64];
    for(unsigned y=0;y<64;++y)for(unsigned x=0;x<78;++x)mask[y*78+x]=out[y*320+x*4+3];
    for(int top=-40;top<240;top+=2) {
        assert(cc_copy_atlas(atlas,out,78,64,320,320*64,20,top,2));
        assert(out[32*320+39*4]==(top+64)/2);
        for(unsigned y=0;y<64;++y)for(unsigned x=0;x<78;++x)assert(out[y*320+x*4+3]==mask[y*78+x]);
    }
    assert(cc_prepare_mask(out,1,1,4,4,0));
    assert(cc_copy_atlas(atlas,out,1,1,4,4,-1,-1,1));assert(out[0]==0);
    assert(cc_copy_atlas(atlas,out,1,1,4,4,1000,1000,1));assert(out[0]==239);
    unsigned char saved[4];memcpy(saved,out,4);
    assert(!cc_copy_atlas(atlas,out+1,1,1,4,4,0,0,1));
    assert(!cc_copy_atlas(atlas,out,1,1,5,5,0,0,1));
    assert(!cc_copy_atlas(atlas,out,161,1,644,644,0,0,1));
    assert(!cc_copy_atlas(atlas,out,1,129,4,516,0,0,1));
    assert(!cc_copy_atlas(atlas,out,78,64,320,320*64-1,0,0,2));
    assert(!cc_copy_atlas(atlas,out,1,1,4,4,0,0,0));
    assert(!cc_copy_atlas(atlas,out,1,1,4,4,2000,0,1));
    assert(!cc_prepare_mask(out,0,1,4,4,0));
    assert(!memcmp(saved,out,4));
    src.stride=1;assert(!cc_build_atlas(&src,atlas,44));src.stride=1024;
    assert(!cc_build_atlas(&src,atlas,101));
    /* Neutral gray, black/white and all saturated cube corners. */
    for(unsigned v=0;v<264;++v) {
        for(unsigned y=0;y<480;++y)for(unsigned x=0;x<336;++x) {
            unsigned char *p=input+y*1024+x*3;
            p[0]=v<256?v:(v&1?255:0);p[1]=v<256?v:(v&2?255:0);p[2]=v<256?v:(v&4?255:0);
        }
        assert(cc_build_atlas(&src,atlas,44));
        unsigned char *p=(unsigned char *)&atlas[100];
        for(unsigned c=0;c<3;++c)assert(p[c]<=112);
        if(v<256)assert(p[0]==p[1]&&p[1]==p[2]);
        assert(p[3]==255);
    }
    free(input);free(out);free(atlas);
    puts("CC atlas: neutral color, contrast, live samples, reused AA alpha, premultiplication, padding/alignment/bounds passed");
}
