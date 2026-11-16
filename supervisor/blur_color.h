#ifndef CHAOS_BLUR_COLOR_H
#define CHAOS_BLUR_COLOR_H
/* BGR888, fixed-point adjustment; used at source/atlas rebuild, never per sprite. */
struct chaos_blur_color {
    unsigned brightness,saturation,contrast,tint,red,green,blue;
};
static int chaos_blur_color_valid(const struct chaos_blur_color *s) {
    return s && s->brightness<=200 && s->saturation<=200 && s->contrast<=200 &&
        s->tint<=100 && s->red<=255 && s->green<=255 && s->blue<=255;
}
static unsigned chaos_blur_color_clamp(int value) {
    return value<0?0u:value>25500?255u:(unsigned)(value+50)/100;
}
static inline void chaos_blur_color_apply(const unsigned char *in,unsigned char *out,const struct chaos_blur_color *s) {
    unsigned lum=(in[0]*19u+in[1]*183u+in[2]*54u+128u)>>8;
    unsigned char channels[3]={in[0],in[1],in[2]};
    unsigned tint[3]={s->blue,s->green,s->red};
    for(unsigned c=0;c<3;++c) {
        unsigned v=chaos_blur_color_clamp((int)channels[c]*(int)s->saturation+(int)lum*(100-(int)s->saturation));
        v=chaos_blur_color_clamp(12800+((int)v-128)*(int)s->contrast);
        v=chaos_blur_color_clamp((int)(v*s->brightness));
        out[c]=(unsigned char)((v*(100-s->tint)+tint[c]*s->tint+50)/100);
    }
}
/* Prepare once per source/parameter update. Preserve each original rounding
 * and clamp, but move contrast/brightness/tint out of the per-pixel loop. */
struct chaos_blur_color_lut { unsigned saturation; unsigned char channel[3][256]; };
static void chaos_blur_color_prepare(struct chaos_blur_color_lut *lut,const struct chaos_blur_color *s) {
    unsigned tint[3]={s->blue,s->green,s->red};
    lut->saturation=s->saturation;
    for(unsigned i=0;i<256;++i) {
        unsigned v=chaos_blur_color_clamp(12800+((int)i-128)*(int)s->contrast);
        v=chaos_blur_color_clamp((int)(v*s->brightness));
        for(unsigned c=0;c<3;++c)
            lut->channel[c][i]=(unsigned char)((v*(100-s->tint)+tint[c]*s->tint+50)/100);
    }
}
static void chaos_blur_color_apply_lut(const unsigned char *in,unsigned char *out,const struct chaos_blur_color_lut *lut) {
    unsigned char channels[3]={in[0],in[1],in[2]};
    if(lut->saturation==100) {
        for(unsigned c=0;c<3;++c) out[c]=lut->channel[c][channels[c]];
        return;
    }
    unsigned lum=(channels[0]*19u+channels[1]*183u+channels[2]*54u+128u)>>8;
    for(unsigned c=0;c<3;++c) {
        unsigned v=chaos_blur_color_clamp((int)channels[c]*(int)lut->saturation+(int)lum*(100-(int)lut->saturation));
        out[c]=lut->channel[c][v];
    }
}
#endif
