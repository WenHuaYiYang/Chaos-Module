#include <stdint.h>
#include <stdio.h>
#include <assert.h>
#include <string.h>
static unsigned chaos_corona_in_call,cc_working,p67_busy,p67_blur_native_capturing,p67_blur_busy,p67_blur_polling,idle=1,resets;
static uint32_t chaos_v2_params[6]={12,70,44,75,38,500},chaos_v2_params_epoch=1;
static int p67_blur_native_idle(void){return idle;}
static void p67_blur_reset(void){++resets;}
#include CHAOS_PARAMS_SETTER
int main(void){
    uint32_t v[6];memcpy(v,chaos_v2_params,sizeof(v));
    assert(chaos_blur_v2_set_params(0,6)==-22);
    assert(chaos_blur_v2_set_params(v,5)==-22);
    for(unsigned i=0;i<6;++i){uint32_t saved=v[i];v[i]=9999;
        assert(chaos_blur_v2_set_params(v,6)==-22);assert(chaos_v2_params[i]==saved);v[i]=saved;}
    v[0]=30;chaos_corona_in_call=1;assert(chaos_blur_v2_set_params(v,6)==-2145);
    chaos_corona_in_call=0;p67_blur_busy=1;assert(chaos_blur_v2_set_params(v,6)==-2145);
    p67_blur_busy=0;p67_blur_polling=1;assert(chaos_blur_v2_set_params(v,6)==-2145);
    p67_blur_polling=0;cc_working=1;assert(chaos_blur_v2_set_params(v,6)==-2145);
    cc_working=0;p67_busy=1;assert(chaos_blur_v2_set_params(v,6)==-2145);
    p67_busy=0;p67_blur_native_capturing=1;assert(chaos_blur_v2_set_params(v,6)==-2145);
    p67_blur_native_capturing=0;idle=0;assert(chaos_blur_v2_set_params(v,6)==-2145);
    assert(resets==0 && chaos_v2_params_epoch==1);idle=1;
    assert(chaos_blur_v2_set_params(v,6)==0 && resets==1 && chaos_v2_params_epoch==2);
    assert(chaos_blur_v2_set_params(v,6)==0 && resets==1 && chaos_v2_params_epoch==2);
    v[1]=50;assert(chaos_blur_v2_set_params(v,6)==0 && resets==2);
    for(unsigned i=2;i<6;++i){v[i]++;assert(chaos_blur_v2_set_params(v,6)==0 && resets==2);}
    assert(chaos_v2_params_epoch==7);
    puts("Native V2 parameters: bounds, atomic rejection, safe defer, filter invalidation and atlas epochs passed");
}
