/* Source: user-provided Vela_NativeApp-syspatch-band-11/ELF/src/websocket_codec.c.
 * Preserved protocol implementation; see PROVENANCE.md. */
/* RFC 6455 handshake hash. SHA-1 is used only by the protocol handshake,
 * never for password storage or authentication. No external runtime. */
#include "websocket_codec.h"
#include <string.h>
static uint32_t rol(uint32_t x,unsigned n){return (x<<n)|(x>>(32-n));}
void web_base64(const uint8_t *in,unsigned n,char *out){
    static const char b[]="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";unsigned j=0;
    for(unsigned i=0;i<n;i+=3){uint32_t v=(uint32_t)in[i]<<16;if(i+1<n)v|=(uint32_t)in[i+1]<<8;if(i+2<n)v|=in[i+2];
        out[j++]=b[v>>18];out[j++]=b[(v>>12)&63];out[j++]=i+1<n?b[(v>>6)&63]:'=';out[j++]=i+2<n?b[v&63]:'=';}out[j]=0;
}
int web_accept_key(const char *key,char out[29]){
    if(strlen(key)!=24 || key[22]!='=' || key[23]!='=')return -1;
    for(unsigned i=0;i<22;i++)if(!((key[i]>='A'&&key[i]<='Z')||(key[i]>='a'&&key[i]<='z')||(key[i]>='0'&&key[i]<='9')||key[i]=='+'||key[i]=='/'))return -1;
    /* A 16-byte nonce has four zero pad bits in its final base64 digit. */
    if(!strchr("AQgw",key[21]))return -1;
    uint8_t input[128]={0},hash[20];memcpy(input,key,24);memcpy(input+24,"258EAFA5-E914-47DA-95CA-C5AB0DC85B11",36);input[60]=128;input[126]=1;input[127]=224;
    uint32_t h[5]={0x67452301,0xefcdab89,0x98badcfe,0x10325476,0xc3d2e1f0};
    for(unsigned block=0;block<2;block++){uint32_t w[80];for(unsigned i=0;i<16;i++){unsigned k=block*64+i*4;w[i]=(uint32_t)input[k]<<24|(uint32_t)input[k+1]<<16|(uint32_t)input[k+2]<<8|input[k+3];}
        for(unsigned i=16;i<80;i++)w[i]=rol(w[i-3]^w[i-8]^w[i-14]^w[i-16],1);
        uint32_t a=h[0],b=h[1],c=h[2],d=h[3],e=h[4];for(unsigned i=0;i<80;i++){uint32_t f,k;if(i<20){f=(b&c)|(~b&d);k=0x5a827999;}else if(i<40){f=b^c^d;k=0x6ed9eba1;}else if(i<60){f=(b&c)|(b&d)|(c&d);k=0x8f1bbcdc;}else{f=b^c^d;k=0xca62c1d6;}uint32_t t=rol(a,5)+f+e+k+w[i];e=d;d=c;c=rol(b,30);b=a;a=t;}h[0]+=a;h[1]+=b;h[2]+=c;h[3]+=d;h[4]+=e;
    }for(unsigned i=0;i<20;i++)hash[i]=(uint8_t)(h[i/4]>>(24-8*(i%4)));web_base64(hash,20,out);return 0;
}

