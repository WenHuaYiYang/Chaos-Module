#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "../control_center_button_pixels.h"
#define CHAOS_CC_BUTTONS_HOST 1
struct node {uint32_t id,parent,clz,record[3];int live,hidden,area[4];unsigned radius;};
static struct node nodes[6];
static unsigned wanted,idle=1,fence_ok=1,moving,source_ready=1,stamp=1;
static unsigned fences,allocs,frees,syncs,draws,fail_alloc,fail_add,bad_buffer,delete_alloc;
static uint32_t root=10;
static unsigned char source_pixels[336*480*3];
static struct image {unsigned used,w,h;unsigned char *pixels;} images[40];
static void cc_button_event(void *);
static struct node *node(uint32_t id){for(unsigned i=0;i<6;++i)if(nodes[i].id==id)return &nodes[i];assert(0);return 0;}
static int chaos_cc_buttons_demand(void){return wanted;}
static int cc_idle(void){return idle;}
static uint32_t cc_root(void){return root;}
static int cc_live(uint32_t id){return node(id)->live;}
static uint32_t cc_class(uint32_t id){return node(id)->clz;}
static int cc_attached(uint32_t id,uint32_t r){return node(id)->live && !node(id)->hidden && node(id)->parent==r;}
static void cc_walk(uint32_t r,int(*cb)(uint32_t,void*),void *p){for(unsigned i=0;i<6;++i)if(nodes[i].live&&nodes[i].parent==r)if(cb(nodes[i].id,p)==2)break;}
static uint32_t *cc_record(uint32_t id){return node(id)->record[0]?node(id)->record:0;}
static uint32_t *cc_add(uint32_t id,uint32_t generation){if(fail_add)return 0;uint32_t *r=node(id)->record;r[0]=1;r[1]=generation;r[2]=0;return r;}
static void cc_remove(uint32_t id){node(id)->record[0]=0;}
static void cc_invalidate(uint32_t id){assert(cc_live(id));}
static void cc_coords(uint32_t id,int *area){memcpy(area,node(id)->area,16);}
static unsigned cc_radius(uint32_t id){return node(id)->radius;}
static int cc_fence(void){assert(idle);++fences;return fence_ok;}
static int cc_moving(void){return moving;}
static void erase(uint32_t id){struct node *n=node(id);uint32_t e[5]={id,id,36,n->record[1],0};cc_button_event(e);n->live=0;n->record[0]=0;}
static uint32_t cc_alloc(unsigned w,unsigned h){
    assert(fences && idle);if(fail_alloc)return 0;
    unsigned i;for(i=1;i<40;++i)if(!images[i].used)break;assert(i<40);
    images[i]=(struct image){1,w,h,calloc(w*h,4)};assert(images[i].pixels);++allocs;
    if(delete_alloc){delete_alloc=0;erase(11);}return i;
}
static void cc_free(uint32_t id){assert(fences&&idle&&images[id].used);free(images[id].pixels);images[id].used=0;++frees;}
static int cc_buffer(uint32_t id,unsigned w,unsigned h,unsigned char **p,unsigned *s,unsigned *size){
    if(bad_buffer)return 0;assert(images[id].used&&images[id].w==w&&images[id].h==h);
    *p=images[id].pixels;*s=w*4;*size=w*h*4;return 1;
}
static void cc_sync(uint32_t id){assert(images[id].used);++syncs;}
static int cc_source(struct cc_pixels *p,uint32_t *s){if(!source_ready)return 0;*p=(struct cc_pixels){source_pixels,336,480,1008,sizeof(source_pixels)};*s=stamp;return 1;}
static void cc_submit(uint32_t layer,uint32_t id,const int *area){assert(layer==99&&images[id].used&&area[2]>=area[0]);++draws;}
#include "../control_center_buttons.inc"
static void draw(uint32_t id){uint32_t e[5]={id,id,25,node(id)->record[1],99};cc_button_event(e);}
static void empty(void){wanted=0;idle=1;fence_ok=1;cc_buttons_poll();assert(!cc_resources()&&!cc_cleanup_pending()&&allocs==frees);}
int main(void){
    memset(source_pixels,100,sizeof(source_pixels));
    nodes[0]=(struct node){11,10,CC_BUTTON_CLASS,{0},1,0,{0,20,155,147},36};
    nodes[1]=(struct node){12,10,CC_BUTTON_CLASS,{0},1,0,{88,200,247,265},33};
    nodes[2]=(struct node){13,10,0x2cce61ec,{0},1,0,{0,0,63,63},0};
    nodes[3]=(struct node){14,20,CC_BUTTON_CLASS,{0},1,0,{0,0,155,127},36};
    cc_buttons_poll();assert(!allocs&&!fences);wanted=1;cc_buttons_poll();
    assert(allocs==2 && cc_resources()==2 && cc_updates==2 && fences==1);
    assert(!node(13)->record[0]&&!node(14)->record[0]);
    assert(images[cc_buttons[0].image].pixels[3]==0);
    draw(11);draw(12);assert(draws==2);
    unsigned f=fences,u=cc_updates;cc_buttons_poll();assert(fences==f&&cc_updates==u);
    node(11)->area[0]++;node(11)->area[2]++;moving=1;cc_buttons_poll();draw(11);
    assert(fences==f&&cc_updates==u&&draws==3);
    moving=0;cc_buttons_poll();assert(cc_updates==u+1);
    ++stamp;cc_buttons_poll();assert(cc_updates==u+3);
    node(11)->record[0]=0;cc_buttons_poll();cc_buttons_poll();
    assert(node(11)->record[0]);
    unsigned rebound=0;for(unsigned i=0;i<CC_BUTTON_LIMIT;++i) rebound+=cc_buttons[i].object==11 && cc_buttons[i].ready;
    assert(rebound==1);
    uint32_t stale_generation=node(11)->record[1];erase(11);
    assert(allocs>frees);cc_buttons_poll();assert(cc_resources()==1);
    node(11)->live=1;cc_buttons_poll();assert(node(11)->record[1]!=stale_generation);
    uint32_t stale[5]={11,11,36,stale_generation,0};cc_button_event(stale);draw(11);assert(draws==4);
    wanted=0;idle=0;draw(11);cc_buttons_poll();assert(draws==4&&cc_cleanup_pending());
    idle=1;fence_ok=0;cc_buttons_poll();assert(cc_cleanup_pending()&&allocs>frees);
    empty();
    source_ready=0;wanted=1;cc_buttons_poll();u=allocs;draw(11);assert(allocs==u&&draws==4);
    source_ready=1;fail_alloc=1;cc_buttons_poll();assert(allocs==u);fail_alloc=0;
    cc_buttons_poll();assert(allocs==u+2);empty();
    wanted=1;fail_add=1;cc_buttons_poll();assert(!cc_resources());fail_add=0;
    delete_alloc=1;cc_buttons_poll();assert(!node(11)->live);cc_buttons_poll();empty();
    node(11)->live=1;wanted=1;bad_buffer=1;cc_buttons_poll();draw(11);assert(draws==4);
    bad_buffer=0;cc_buttons_poll();draw(11);assert(draws==5);
    root=20;cc_buttons_poll();assert(cc_resources()==1 && node(14)->record[0]);
    empty();
    puts("CC native button lifecycle: no-input/style replacement, cached glass, scroll stability, epochs, stale DELETE, failed fence/allocation/registration and shutdown passed");
}
