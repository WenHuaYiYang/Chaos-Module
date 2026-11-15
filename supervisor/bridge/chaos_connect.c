#include "chaos_connect.h"
#include <stddef.h>
static uint16_t u16(const uint8_t *p){return p[0]|((uint16_t)p[1]<<8);}
static uint32_t u32(const uint8_t *p){return u16(p)|((uint32_t)u16(p+2)<<16);}
static void p16(uint8_t *p,uint16_t v){p[0]=v;p[1]=v>>8;}
static void p32(uint8_t *p,uint32_t v){p16(p,v);p16(p+2,v>>16);}
static void copy(uint8_t *out,const void *data,uint32_t len){const uint8_t *p=data;for(uint32_t i=0;i<len;i++)out[i]=p[i];}
uint32_t cc_crc32(uint32_t crc,const void *data,uint32_t len){
    const uint8_t *p=data;for(uint32_t i=0;i<len;i++){crc^=p[i];for(unsigned j=0;j<8;j++)crc=(crc>>1)^(0xedb88320u&(0u-(crc&1u)));}return crc;
}
int cc_encode(uint8_t *out,uint32_t cap,uint8_t flags,uint8_t op,uint8_t status,uint16_t req,const void *data,uint32_t len){
    if(!out||len>CC_MAX_PAYLOAD||cap<CC_HEADER+len||(len&&!data)||flags>CC_EVENT)return -1;
    out[0]='C';out[1]='C';out[2]='N';out[3]='1';out[4]=CC_VERSION;out[5]=flags;out[6]=op;out[7]=status;p16(out+8,req);p16(out+10,len);
    if(len)copy(out+CC_HEADER,data,len);
    p32(out+12,cc_crc32(cc_crc32(0xffffffffu,out,12),out+CC_HEADER,len)^0xffffffffu);return CC_HEADER+len;
}
int cc_decode(const uint8_t *p,uint32_t n,struct cc_frame *f){
    if(!p||!f||n<CC_HEADER||n>CC_MAX_FRAME||p[0]!='C'||p[1]!='C'||p[2]!='N'||p[3]!='1'||p[4]!=CC_VERSION||p[5]>CC_EVENT||n!=CC_HEADER+(uint32_t)u16(p+10))return -1;
    if(u32(p+12)!=(cc_crc32(cc_crc32(0xffffffffu,p,12),p+CC_HEADER,n-CC_HEADER)^0xffffffffu))return -1;
    f->flags=p[5];f->op=p[6];f->status=p[7];f->req=u16(p+8);f->len=u16(p+10);f->payload=p+CC_HEADER;return 0;
}
/* Small, bounded protobuf reader. Reject duplicate target fields and overflowing varints. */
static int varint(const uint8_t *p,uint32_t n,uint32_t *at,uint32_t *v){
    uint32_t x=0;for(unsigned shift=0;shift<35;shift+=7){if(*at>=n)return -1;uint8_t b=p[(*at)++];if(shift==28&&(b&0xf0))return -1;x|=(uint32_t)(b&127)<<shift;if(!(b&128)){*v=x;return 0;}}return -1;
}
static int field(const uint8_t *p,uint32_t n,uint32_t wanted,uint32_t wire,const uint8_t **data,uint32_t *size,uint32_t *value){
    uint32_t at=0;int found=0;
    while(at<n){uint32_t tag,v,start;if(varint(p,n,&at,&tag)||tag<8)return -1;start=at;
        switch(tag&7){case 0:if(varint(p,n,&at,&v))return -1;break;
        case 2:if(varint(p,n,&at,&v)||v>n-at)return -1;start=at;at+=v;break;
        case 1:v=8;if(v>n-at)return -1;at+=v;break;
        case 5:v=4;if(v>n-at)return -1;at+=v;break;default:return -1;}
        if((tag>>3)==wanted){if(found||(tag&7)!=wire)return -1;found=1;if(wire==0){if(value)*value=v;}else{if(data)*data=p+start;if(size)*size=v;}}
    }return found;
}
static int eq(const uint8_t *p,uint32_t n,const char *s){uint32_t i=0;for(;s[i];i++)if(i>=n||p[i]!=(uint8_t)s[i])return 0;return i==n;}
int cc_pb_unwrap(const uint8_t *p,uint32_t n,const uint8_t **frame,uint32_t *size,uint32_t *id){
    const uint8_t *app,*msg,*basic,*name;uint32_t a,m,b,s,type=0;
    int r=field(p,n,1,0,0,0,&type);if(r!=1||type!=CC_PB_TYPE)return r<0?-1:0;
    if(field(p,n,2,0,0,0,id)!=1||field(p,n,22,2,&app,&a,0)!=1||field(app,a,9,2,&msg,&m,0)!=1||field(msg,m,1,2,&basic,&b,0)!=1||field(basic,b,1,2,&name,&s,0)!=1||!eq(name,s,"ChaosConnect")||field(msg,m,2,2,frame,size,0)!=1||*size>CC_MAX_FRAME)return -1;
    return 1;
}
static uint32_t putvar(uint8_t *p,uint32_t v){uint32_t n=0;do{p[n++]=(v&127)|(v>=128?128:0);v>>=7;}while(v);return n;}
int cc_pb_wrap(uint8_t *out,uint32_t cap,const uint8_t *frame,uint32_t len,uint32_t id){
    /* BasicInfo = package_name + required empty fingerprint. */
    static const uint8_t basic[]={0x0a,12,'C','h','a','o','s','C','o','n','n','e','c','t',0x12,0};
    uint8_t sizes[5];uint32_t vn=putvar(sizes,len),msg=2+sizeof(basic)+1+vn+len,app=1+putvar(sizes,msg)+msg;
    uint32_t n=0;if(!out||!frame||len>CC_MAX_FRAME||cap<len+64)return -1;
    out[n++]=8;n+=putvar(out+n,CC_PB_TYPE);out[n++]=16;n+=putvar(out+n,id);n+=putvar(out+n,(22u<<3)|2);n+=putvar(out+n,app);
    out[n++]=(9u<<3)|2;n+=putvar(out+n,msg);out[n++]=10;n+=putvar(out+n,sizeof(basic));copy(out+n,basic,sizeof(basic));n+=sizeof(basic);
    out[n++]=18;n+=putvar(out+n,len);copy(out+n,frame,len);return n+len;
}
