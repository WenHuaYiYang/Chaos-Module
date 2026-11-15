#include "nap_sdp.h"
#define API
static unsigned be16(const uint8_t *p){return (unsigned)p[0]<<8|p[1];}
static void put16(uint8_t *p,unsigned v){p[0]=(uint8_t)(v>>8);p[1]=(uint8_t)v;}
API void nap_sdp_init(struct nap_sdp *s)
{
    if(!s)return;
    s->transaction=s->total=0;s->continuation_size=s->rounds=s->done=s->count=0;
    for(unsigned i=0;i<16;i++){s->continuation[i]=0;s->handles[i]=0;}
}
API int nap_sdp_request(struct nap_sdp *s,uint8_t *out,unsigned capacity)
{
    if(!s || !out || s->done || s->continuation_size>16 || s->rounds>=16)return -22;
    unsigned n=13u+s->continuation_size;if(capacity<n)return -90;
    /* ServiceSearchPattern = sequence(UUID16 NAP 0x1116), maximum 16 records. */
    out[0]=2;put16(out+1,++s->transaction);put16(out+3,n-5);
    out[5]=0x35;out[6]=3;out[7]=0x19;out[8]=0x11;out[9]=0x16;
    put16(out+10,16);out[12]=s->continuation_size;
    for(unsigned i=0;i<s->continuation_size;i++)out[13+i]=s->continuation[i];
    s->rounds++;return (int)n;
}
API int nap_sdp_response(struct nap_sdp *s,const uint8_t *p,unsigned n)
{
    if(!s || !p || s->done || !s->rounds || n<5 || be16(p+1)!=s->transaction || be16(p+3)!=n-5)return -71;
    /* Preserve the remote SDP ErrorCode distinctly from local errno values. */
    if(p[0]==1)return n==7?-(int)(0x10000u+be16(p+5)):-71;
    if(p[0]!=3 || n<10)return -71;
    unsigned total=be16(p+5),count=be16(p+7);
    if(total>16 || count>16u-s->count || count>total || (s->count && total!=s->total))return -75;
    unsigned end=9+4*count;if(end>=n)return -71;
    unsigned continuation=p[end];
    if(continuation>16 || n!=end+1+continuation || (!count && continuation))return -71;
    if(s->count+count>total || (!continuation && s->count+count!=total))return -71;
    if(continuation && (s->count+count>=total || s->rounds>=16))return -71;
    /* Validate all handles before committing a partial response. */
    uint32_t handles[16];
    for(unsigned i=0;i<count;i++){
        const uint8_t *h=p+9+4*i;
        uint32_t v=(uint32_t)h[0]<<24|(uint32_t)h[1]<<16|(uint32_t)h[2]<<8|h[3];
        if(!v)return -71;
        for(unsigned j=0;j<s->count;j++)if(s->handles[j]==v)return -71;
        for(unsigned j=0;j<i;j++)if(handles[j]==v)return -71;
        handles[i]=v;
    }
    for(unsigned i=0;i<count;i++)s->handles[s->count+i]=handles[i];
    s->count=(uint8_t)(s->count+count);s->total=(uint16_t)total;
    s->continuation_size=(uint8_t)continuation;
    for(unsigned i=0;i<continuation;i++)s->continuation[i]=p[end+1+i];
    s->done=!continuation;return s->done?1:0;
}
