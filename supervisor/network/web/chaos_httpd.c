/* Ported from user-provided Vela_NativeApp-syspatch-band-11, ELF/src/syspatch_httpd.c.
 * See PROVENANCE.md. No Band 11 firmware addresses are used here. */
/* Module-only HTTP control service. All TCP work stays on the PAN owner.
 * Three bounded clients allow one WebSocket shell plus HTTP status/UI traffic.
 * No firmware hooks, no changes to the frozen connection layer. */
#include "chaos_httpd.h"
#include "chaos_web_platform.h"
#include "pan_ip.h"
#include "pan_ethernet.h"
#include "websocket_codec.h"
#include "web_index.h"
#include <string.h>
#define CLIENTS 3u
#define REQUEST_CAP 18432u
#define APP_CAP 16384u
#define OUT_CAP 3072u
struct client {int id;unsigned used,head,content,parsed,method,phase,sent,length,hs,ws,auth,fragment,fraglen,closing,reboot_reply;uint32_t deadline;const unsigned char *data;char request[REQUEST_CAP],out[OUT_CAP],header[768],message[1024];};
static struct client clients[CLIENTS];
static _Atomic int listener,last_error;
/* Enabled bit and generation change atomically, including OFF/ON between polls. */
static unsigned control;
static _Atomic unsigned active_clients;
static unsigned seen_epoch;
static _Atomic unsigned served,failed,refused;
static uint32_t retry_at,reboot_at,exec_idle_at;
static unsigned reboot_pending,exec_session,retry_pending;
static char app[APP_CAP];static unsigned app_size,app_running;
static struct client *shell;
static char credential[132];
static unsigned load_enabled(void){return __atomic_load_n(&control,__ATOMIC_ACQUIRE)&1u;}
void chaos_httpd_enable(int on){unsigned old=__atomic_load_n(&control,__ATOMIC_ACQUIRE),next;do{if((old&1u)==(unsigned)!!on)return;next=((old+2u)&~1u)|(unsigned)!!on;}while(!__atomic_compare_exchange_n(&control,&old,next,0,__ATOMIC_ACQ_REL,__ATOMIC_ACQUIRE));}
int chaos_httpd_enabled(void){return (int)load_enabled();}
static void copy(char *out,const char *s,unsigned cap){unsigned n=0;while(s && s[n] && n+1<cap){out[n]=s[n];n++;}if(cap)out[n]=0;}
static unsigned put(char *out,unsigned at,unsigned cap,const char *s){while(*s && at+1<cap)out[at++]=*s++;out[at]=0;return at;}
static unsigned num(char *out,unsigned at,unsigned cap,unsigned v){char b[12];unsigned n=0;do{b[n++]=(char)('0'+v%10);v/=10;}while(v);while(n && at+1<cap)out[at++]=b[--n];out[at]=0;return at;}
static unsigned quote(char *out,unsigned at,unsigned cap,const char *s){at=put(out,at,cap,"\"");for(;*s && at+8<cap;s++){unsigned char b=(unsigned char)*s;if(b=='"'||b=='\\'){out[at++]='\\';out[at++]=(char)b;}else if(b<32){static const char h[]="0123456789abcdef";out[at++]='\\';out[at++]='u';out[at++]='0';out[at++]='0';out[at++]=h[b>>4];out[at++]=h[b&15];}else out[at++]=(char)b;}out[at]=0;return put(out,at,cap,"\"");}
static unsigned iptext(char *out,unsigned at,unsigned cap,const uint8_t *ip){for(unsigned i=0;i<4;i++){if(i)at=put(out,at,cap,".");at=num(out,at,cap,ip[i]);}return at;}
static int equal_ci(const char *a,const char *b,unsigned n){for(unsigned i=0;i<n;i++){char x=a[i],y=b[i];if(x>='A'&&x<='Z')x+=32;if(y>='A'&&y<='Z')y+=32;if(x!=y)return 0;}return 1;}
/* Return -1 for duplicate headers or overflow. Header names case insensitive. */
static int header(struct client *c,const char *name,char *out,unsigned cap){unsigned at=0,n=(unsigned)strlen(name);int found=0;out[0]=0;while(at<c->head && c->request[at]!='\n')at++;at++;
    while(at+2<c->head){unsigned start=at;while(at<c->head && c->request[at]!='\n')at++;unsigned end=at++;if(end>start && c->request[end-1]=='\r')end--;if(end==start)break;
        if(end>start+n && c->request[start+n]==':' && equal_ci(c->request+start,name,n)){if(found)return -1;found=1;start+=n+1;while(start<end && (c->request[start]==' '||c->request[start]=='\t'))start++;while(end>start && (c->request[end-1]==' '||c->request[end-1]=='\t'))end--;if(end-start>=cap)return -1;memcpy(out,c->request+start,end-start);out[end-start]=0;}}
    return found;
}
static int has_token(const char *s,const char *wanted){unsigned n=(unsigned)strlen(wanted);while(*s){while(*s==' '||*s==',')s++;const char *a=s;while(*s && *s!=',')s++;unsigned k=(unsigned)(s-a);while(k && a[k-1]==' ')k--;if(k==n && equal_ci(a,wanted,n))return 1;}return 0;}
static int token_char(unsigned char c){return (c>='0'&&c<='9')||(c>='A'&&c<='Z')||(c>='a'&&c<='z')||strchr("!#$%&'*+-.^_`|~",c)!=0;}
/* Reject ambiguous request framing before looking up individual headers.
 * Embedded NUL, bare LF/CR and obsolete folded fields must not hide headers. */
static int valid_headers(struct client *c){
    unsigned at=0,method=0,target=0;
    while(at<c->head && c->request[at]!=' '){if(!c->request[at]||!token_char((unsigned char)c->request[at]))return 0;at++;method++;}
    if(!method || at>=c->head)return 0;
    at++;if(c->request[at]!='/')return 0;
    while(at<c->head && c->request[at]!=' '){unsigned char ch=(unsigned char)c->request[at++];if(ch<33||ch>126)return 0;target++;}
    if(!target || ++at+10>c->head)return 0;
    int http11=!memcmp(c->request+at,"HTTP/1.1\r\n",10);
    if(!http11 && memcmp(c->request+at,"HTTP/1.0\r\n",10))return 0;
    at+=10;
    while(at<c->head){
        if(at+2==c->head && !memcmp(c->request+at,"\r\n",2))break;
        unsigned name=0;
        while(at<c->head && c->request[at]!=':'){unsigned char ch=(unsigned char)c->request[at++];if(!ch||!token_char(ch))return 0;name++;}
        if(!name || at++>=c->head)return 0;
        while(at<c->head && c->request[at]!='\r'){unsigned char ch=(unsigned char)c->request[at++];if((ch<32&&ch!='\t')||ch==127)return 0;}
        if(at+2>c->head || c->request[at+1]!='\n')return 0;at+=2;
    }
    char host[96];int present=header(c,"Host",host,sizeof(host));
    if(present<0 || (http11 && present!=1) || (present && !host[0]))return 0;
    for(unsigned i=0;host[i];i++)if((unsigned char)host[i]<33 || (unsigned char)host[i]>126 || strchr("/\\,@",host[i]))return 0;
    return at+2==c->head;
}
static int same_origin(struct client *c){char origin[128],host[96];int n=header(c,"Origin",origin,sizeof(origin));if(n<0)return 0;if(!n)return 1;if(header(c,"Host",host,sizeof(host))!=1)return 0;return !strncmp(origin,"http://",7) && !strcmp(origin+7,host);}
static int authorized(struct client *c){char auth[160];return header(c,"Authorization",auth,sizeof(auth))==1 && !strncmp(auth,"Basic ",6) && !strcmp(auth+6,credential);}
static void drop(struct client *c){if(shell==c){if(c->auth)chaos_web_terminal_stop();shell=0;}if(c->reboot_reply)reboot_pending=0;if(c->id)pan_tcp_close(c->id);memset(c,0,sizeof(*c));}
static void respond(struct client *c,unsigned code,const char *reason,const char *type,const void *data,unsigned length,int gzip){unsigned a=0;a=put(c->header,a,sizeof(c->header),"HTTP/1.1 ");a=num(c->header,a,sizeof(c->header),code);a=put(c->header,a,sizeof(c->header)," ");a=put(c->header,a,sizeof(c->header),reason);a=put(c->header,a,sizeof(c->header),"\r\nContent-Type: ");a=put(c->header,a,sizeof(c->header),type);a=put(c->header,a,sizeof(c->header),"\r\nContent-Length: ");a=num(c->header,a,sizeof(c->header),length);
    if(gzip)a=put(c->header,a,sizeof(c->header),"\r\nContent-Encoding: gzip");
    if(code==401)a=put(c->header,a,sizeof(c->header),"\r\nWWW-Authenticate: Basic realm=\"Chaos\"");
    if(data==(const void*)app)a=put(c->header,a,sizeof(c->header),"\r\nContent-Security-Policy: sandbox allow-scripts; default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; connect-src 'none'; form-action 'none'; base-uri 'none'");
    a=put(c->header,a,sizeof(c->header),"\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n");c->hs=a;c->data=data;c->length=c->method==2?0:length;c->sent=0;c->phase=1;c->deadline=pan_ip_now()+60000;
}
static void json(struct client *c,unsigned code,const char *s){copy(c->out,s,sizeof(c->out));respond(c,code,code==200?"OK":"Error","application/json; charset=utf-8",c->out,(unsigned)strlen(c->out),0);}
static void status(struct client *c){struct chaos_web_sysinfo info={0};struct pan_ip_info ip;struct pan_eth_stats eth;chaos_web_sysinfo_read(&info);info.model[sizeof(info.model)-1]=info.firmware[sizeof(info.firmware)-1]=info.build[sizeof(info.build)-1]=info.meminfo[sizeof(info.meminfo)-1]=0;pan_ip_info(&ip);pan_eth_snapshot(&eth);unsigned a=0;
#define S(k,v) do{a=put(c->out,a,OUT_CAP,k);a=quote(c->out,a,OUT_CAP,v);}while(0)
#define N(k,v) do{a=put(c->out,a,OUT_CAP,k);a=num(c->out,a,OUT_CAP,v);}while(0)
    S("{\"model\":",info.model);S(",\"firmware\":",info.firmware);S(",\"build\":",info.build);
#define O(k,v,flag) do{if(info.valid&(flag)){N(k,v);}else a=put(c->out,a,OUT_CAP,k "null");}while(0)
    O(",\"uptime\":",info.uptime_seconds,CHAOS_WEB_HAVE_UPTIME);O(",\"heapTotal\":",info.heap_total,CHAOS_WEB_HAVE_HEAP);O(",\"heapFree\":",info.heap_free,CHAOS_WEB_HAVE_HEAP);O(",\"heapLargest\":",info.heap_largest,CHAOS_WEB_HAVE_HEAP);
    /* Leave space for the fixed network/statistics suffix even if meminfo
     * consists entirely of characters requiring six-byte JSON escapes. */
    if(info.valid&CHAOS_WEB_HAVE_MEMINFO){a=put(c->out,a,OUT_CAP,",\"meminfo\":");a=quote(c->out,a,OUT_CAP-768u,info.meminfo);}else a=put(c->out,a,OUT_CAP,",\"meminfo\":null");
    a=put(c->out,a,OUT_CAP,(info.valid&CHAOS_WEB_CAN_REBOOT)?",\"canReboot\":true":",\"canReboot\":false");
#undef O
    a=put(c->out,a,OUT_CAP,",\"ip\":\"");a=iptext(c->out,a,OUT_CAP,ip.ip);a=put(c->out,a,OUT_CAP,"\",\"mask\":\"");a=iptext(c->out,a,OUT_CAP,ip.mask);a=put(c->out,a,OUT_CAP,"\",\"gateway\":\"");a=iptext(c->out,a,OUT_CAP,ip.gateway);a=put(c->out,a,OUT_CAP,"\",\"dns\":\"");a=iptext(c->out,a,OUT_CAP,ip.dns);a=put(c->out,a,OUT_CAP,"\"");N(",\"mtu\":",ip.mtu);N(",\"rxFrames\":",eth.rx_packets);N(",\"txFrames\":",eth.tx_packets);N(",\"requests\":",served);N(",\"failures\":",failed);N(",\"refused\":",refused);N(",\"lastError\":",last_error<0?0u-(unsigned)last_error:(unsigned)last_error);a=put(c->out,a,OUT_CAP,",\"cpu\":null,\"temperature\":null,\"storage\":null,\"recordings\":false,\"logs\":false}");respond(c,200,"OK","application/json; charset=utf-8",c->out,a,0);
#undef S
#undef N
}
static void ws_frame(struct client *c,unsigned op,const void *data,unsigned n){if(n>OUT_CAP-4)return;c->out[0]=(char)(128|op);unsigned h=2;if(n<126)c->out[1]=(char)n;else{c->out[1]=126;c->out[2]=(char)(n>>8);c->out[3]=(char)n;h=4;}if(n)memcpy(c->out+h,data,n);c->data=(const unsigned char*)c->out;c->length=n+h;c->sent=0;}
static void ws_close(struct client *c,unsigned code){uint8_t b[2]={(uint8_t)(code>>8),(uint8_t)code};ws_frame(c,8,b,2);c->closing=1;}
static void upgrade(struct client *c){char key[64],accept[29],v[16],up[32],conn[64];
    if(c->method!=1 || c->content || header(c,"Sec-WebSocket-Key",key,sizeof(key))!=1 || web_accept_key(key,accept) || header(c,"Sec-WebSocket-Version",v,sizeof(v))!=1 || strcmp(v,"13") || header(c,"Upgrade",up,sizeof(up))!=1 || strlen(up)!=9 || !equal_ci(up,"websocket",9) || header(c,"Connection",conn,sizeof(conn))!=1 || !has_token(conn,"upgrade") || !same_origin(c)){json(c,400,"{\"error\":\"invalid_websocket_handshake\"}");return;}
    if(shell || reboot_pending || exec_session){json(c,409,"{\"error\":\"terminal_busy\"}");return;}
    /* Authentication happens in the first frame; never put credentials in URLs.
     * No NSH task is created before authentication. */
    unsigned a=put(c->header,0,sizeof(c->header),"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ");a=put(c->header,a,sizeof(c->header),accept);a=put(c->header,a,sizeof(c->header),"\r\n\r\n");c->hs=a;c->phase=1;c->ws=1;c->length=0;c->sent=0;c->deadline=pan_ip_now()+8000;shell=c;
}
static void serve(struct client *c){char path[128];unsigned pos=0,end=0;while(pos<c->head && c->request[pos]!=' ')pos++;if(pos==3 && !memcmp(c->request,"GET",3))c->method=1;else if(pos==4 && !memcmp(c->request,"HEAD",4))c->method=2;else if(pos==4 && !memcmp(c->request,"POST",4))c->method=3;else{json(c,405,"{\"error\":\"method_not_allowed\"}");return;}pos++;end=pos;while(end<c->head && c->request[end]!=' ')end++;if(end==pos || end-pos>=sizeof(path) || end+10>c->head || memcmp(c->request+end+1,"HTTP/1.",7)){json(c,400,"{\"error\":\"request_line\"}");return;}memcpy(path,c->request+pos,end-pos);path[end-pos]=0;
    if(!same_origin(c)){json(c,403,"{\"error\":\"origin\"}");return;}
    if(!strcmp(path,"/ws/nsh")){upgrade(c);return;}
    if(c->method!=3 && (!strcmp(path,"/") || !strcmp(path,"/index.html"))){respond(c,200,"OK","text/html; charset=utf-8",web_index,WEB_INDEX_SIZE,1);return;}
    if(!authorized(c)){json(c,401,"{\"error\":\"authentication_required\"}");return;}
    if(c->method!=3 && (!strcmp(path,"/api/system/status") || !strcmp(path,"/api/system/metrics") || !strcmp(path,"/status"))){status(c);return;}
    if(c->method==3 && !strcmp(path,"/api/nsh/exec")){
        if(shell || reboot_pending){json(c,409,"{\"error\":\"terminal_busy\"}");return;}
        if(!c->content || c->content>1024){json(c,413,"{\"error\":\"command_too_long\"}");return;}
        for(unsigned i=0;i<c->content;i++)if(c->request[c->head+i]==0 || c->request[c->head+i]==4){json(c,400,"{\"error\":\"unsupported_terminal_input\"}");return;}
        if(chaos_web_terminal_state()==CHAOS_WEB_TERMINAL_STOPPING){json(c,503,"{\"error\":\"terminal_stopping\"}");return;}
        /* p67 readline expects LF. Commit command and LF in ONE enqueue so a
         * full TX ring cannot leave an unfinished command in the next session. */
        unsigned length=c->content;char *command=c->request+c->head;
        if(command[length-1]=='\r')command[length-1]='\n';else if(command[length-1]!='\n')command[length++]='\n';
        int rc=chaos_web_terminal_start();if(!rc)rc=chaos_web_terminal_send(command,length);
        if(!rc){exec_session=1;exec_idle_at=pan_ip_now()+60000;}
        else if(!exec_session)chaos_web_terminal_stop();
        json(c,rc?503:200,rc?"{\"error\":\"terminal_unavailable\"}":"{\"accepted\":true,\"outputEndpoint\":\"/api/nsh/output\"}");return;
    }
    if(c->method!=3 && !strcmp(path,"/api/nsh/output")){
        if(shell){json(c,409,"{\"error\":\"terminal_busy\"}");return;}
        /* Worst case is six JSON bytes per output byte, plus metadata. */
        char text[480];unsigned n=c->method==2?0:chaos_web_terminal_receive(text,sizeof(text)-1);text[n]=0;
        unsigned a=put(c->out,0,OUT_CAP,"{\"output\":");a=quote(c->out,a,OUT_CAP,text);a=put(c->out,a,OUT_CAP,",\"state\":");a=num(c->out,a,OUT_CAP,(unsigned)chaos_web_terminal_state());a=put(c->out,a,OUT_CAP,"}");
        if(exec_session)exec_idle_at=pan_ip_now()+60000;respond(c,200,"OK","application/json; charset=utf-8",c->out,a,0);return;
    }
    if(c->method==3 && !strcmp(path,"/api/system/reboot")){
        struct chaos_web_sysinfo info;chaos_web_sysinfo_read(&info);
        if(!(info.valid&CHAOS_WEB_CAN_REBOOT)){json(c,501,"{\"error\":\"reboot_unavailable\"}");return;}
        if(reboot_pending){json(c,409,"{\"error\":\"reboot_pending\"}");return;}
        if(shell && shell->phase==2 && !shell->length)ws_close(shell,1001);
        chaos_web_terminal_stop();reboot_pending=1;
        json(c,200,"{\"accepted\":true}");c->reboot_reply=1;return;
    }
    if(c->method==3 && !strcmp(path,"/api/apps/upload")){if(!c->content || c->content>APP_CAP){json(c,413,"{\"error\":\"maximum_16_KiB\"}");return;}for(unsigned i=0;i<CLIENTS;i++)if(clients[i].id && clients[i].data==(const unsigned char*)app){json(c,409,"{\"error\":\"application_in_use\"}");return;}memcpy(app,c->request+c->head,c->content);app_size=c->content;app_running=0;json(c,200,"{\"id\":\"web\",\"stored\":\"ram\",\"uploaded\":true}");return;}
    if(c->method==3 && !strcmp(path,"/api/apps/run")){if(!app_size){json(c,409,"{\"error\":\"upload_first\"}");return;}app_running=1;json(c,200,"{\"url\":\"/app/\",\"running\":true}");return;}
    if(c->method==3 && !strcmp(path,"/api/apps/stop")){app_running=0;json(c,200,"{\"running\":false}");return;}
    if(c->method!=3 && !strcmp(path,"/app/") && app_running){respond(c,200,"OK","text/html; charset=utf-8",app,app_size,0);return;}
    json(c,404,"{\"error\":\"not_found\"}");
}
/* Fragmented TCP headers/body, strict one Content-Length, no chunked requests. */
static void read_http(struct client *c){int n=pan_tcp_receive(c->id,c->request+c->used,REQUEST_CAP-1-c->used);if(n>0){c->used+=(unsigned)n;c->request[c->used]=0;}else if(n<0 && n!=-11){drop(c);return;}
    if(!c->head){for(unsigned i=3;i<c->used;i++)if(!memcmp(c->request+i-3,"\r\n\r\n",4)){c->head=i+1;break;}if(!c->head && c->used>2048){json(c,431,"{\"error\":\"headers_too_large\"}");return;}}
    if(c->head>2048){json(c,431,"{\"error\":\"headers_too_large\"}");return;}
    if(c->head && !c->parsed){
        if(!valid_headers(c)){json(c,400,"{\"error\":\"invalid_headers\"}");return;}
        char len[16],transfer[32];int r=header(c,"Content-Length",len,sizeof(len));
        if(r<0 || (r && !len[0]) || header(c,"Transfer-Encoding",transfer,sizeof(transfer))!=0){json(c,400,"{\"error\":\"invalid_framing\"}");return;}
        unsigned size=0;for(unsigned i=0;len[i];i++){if(len[i]<'0'||len[i]>'9'){json(c,400,"{\"error\":\"content_length\"}");return;}
            if(size>(APP_CAP-(unsigned)(len[i]-'0'))/10u){json(c,413,"{\"error\":\"body_too_large\"}");return;}size=size*10+(unsigned)(len[i]-'0');}
        if(c->head+size>=REQUEST_CAP){json(c,413,"{\"error\":\"body_too_large\"}");return;}c->content=size;c->parsed=1;
    }
    if(c->parsed && c->used>=c->head+c->content){serve(c);/* Preserve WebSocket bytes coalesced after upgrade. */if(c->ws){unsigned extra=c->used-c->head-c->content;memmove(c->request,c->request+c->head+c->content,extra);c->used=extra;}}
}
static void message(struct client *c){if(!c->auth){unsigned n=c->fraglen;if(n!=strlen(credential)+5 || memcmp(c->message,"AUTH ",5) || memcmp(c->message+5,credential,n-5)){ws_close(c,1008);return;}int s=chaos_web_terminal_state();if(s==CHAOS_WEB_TERMINAL_STARTING || s==CHAOS_WEB_TERMINAL_RUNNING || s==CHAOS_WEB_TERMINAL_STOPPING){ws_close(c,1013);return;}if(chaos_web_terminal_start()){ws_close(c,1011);return;}c->auth=1;c->deadline=pan_ip_now()+120000;return;}
    if(c->fraglen && chaos_web_terminal_send(c->message,c->fraglen)){/* Do not silently drop a full TX queue. */ws_close(c,1013);}
}
static int valid_utf8(const uint8_t *p,unsigned n){
    unsigned at=0;while(at<n){unsigned c=p[at++],extra,value,min;
        if(c<128)continue;
        if(c>=0xc2 && c<=0xdf){extra=1;value=c&31;min=0x80;}
        else if(c>=0xe0 && c<=0xef){extra=2;value=c&15;min=0x800;}
        else if(c>=0xf0 && c<=0xf4){extra=3;value=c&7;min=0x10000;}else return 0;
        if(n-at<extra)return 0;while(extra--){c=p[at++];if((c&0xc0)!=0x80)return 0;value=(value<<6)|(c&63);}
        if(value<min || value>0x10ffff || (value>=0xd800 && value<=0xdfff))return 0;
    }return 1;
}
static int valid_close_code(unsigned code){return (code>=1000 && code<=1014 && code!=1004 && code!=1005 && code!=1006)||(code>=3000 && code<=4999);}
static void read_ws(struct client *c){if(c->used<sizeof(c->request)){int n=pan_tcp_receive(c->id,c->request+c->used,(unsigned)sizeof(c->request)-c->used);if(n>0)c->used+=(unsigned)n;else if(n<0 && n!=-11){drop(c);return;}}
    if(c->length)return;unsigned budget=4;while(c->used>=2 && budget--){uint8_t *p=(uint8_t*)c->request;unsigned op=p[0]&15,fin=p[0]>>7,h=2,n=p[1]&127;if((p[0]&112) || !(p[1]&128) || n==127){ws_close(c,1002);return;}if(n==126){if(c->used<4)return;n=(unsigned)p[2]<<8|p[3];h=4;if(n<126){ws_close(c,1002);return;}}if((op>=8) && (!fin || n>125)){ws_close(c,1002);return;}if(n>1024){ws_close(c,1009);return;}if(c->used<h+4+n)return;uint8_t *mask=p+h,*payload=mask+4;for(unsigned i=0;i<n;i++)payload[i]^=mask[i%4];if(c->auth)c->deadline=pan_ip_now()+120000;
        if(op==8){if(n==1 || (n>=2 && !valid_close_code((unsigned)payload[0]<<8|payload[1]))){ws_close(c,1002);return;}if(n>2 && !valid_utf8(payload+2,n-2)){ws_close(c,1007);return;}ws_frame(c,8,payload,n);c->closing=1;}
        else if(op==9)ws_frame(c,10,payload,n);
        else if(op!=10){if(op==1){ws_close(c,1003);return;}if(op!=0 && op!=2){ws_close(c,1002);return;}if((op==0 && !c->fragment)||(op!=0 && c->fragment)){ws_close(c,1002);return;}if(c->fraglen+n>sizeof(c->message)){ws_close(c,1009);return;}memcpy(c->message+c->fraglen,payload,n);c->fraglen+=n;if(fin){message(c);c->fragment=0;c->fraglen=0;}else c->fragment=1;}
        unsigned total=h+4+n;c->used-=total;memmove(c->request,c->request+total,c->used);if(c->length || !c->id)return;
    }
    if(c->auth && !c->length){char text[768];unsigned n=chaos_web_terminal_receive(text,sizeof(text));if(n)ws_frame(c,2,text,n);else if(chaos_web_terminal_state()==CHAOS_WEB_TERMINAL_EXITED || chaos_web_terminal_state()==CHAOS_WEB_TERMINAL_FAILED)ws_close(c,1000);}
}
static void send_pending(struct client *c){unsigned budget=4;while(budget-- && c->sent<c->hs+c->length){const unsigned char *p;unsigned n;if(c->sent<c->hs){p=(const unsigned char*)c->header+c->sent;n=c->hs-c->sent;}else{unsigned at=c->sent-c->hs;p=c->data+at;n=c->length-at;}if(n>1024)n=1024;int rc=pan_tcp_send(c->id,p,n);if(rc>0){c->sent+=(unsigned)rc;continue;}if(rc!=-11 && rc!=0)drop(c);return;}
    if(c->id && c->sent>=c->hs+c->length){if(c->ws && !c->closing){c->phase=2;c->hs=c->length=c->sent=0;}else{int rc=pan_tcp_finish(c->id);if(!rc){if(shell==c){if(c->auth)chaos_web_terminal_stop();shell=0;}if(c->reboot_reply){reboot_pending=2;reboot_at=pan_ip_now()+1500;}memset(c,0,sizeof(*c));served++;}else if(rc!=-11)drop(c);}}
}
static void stop_transport(void){
    for(unsigned i=0;i<CLIENTS;i++)if(clients[i].id)drop(&clients[i]);
    if(chaos_web_terminal_state()==CHAOS_WEB_TERMINAL_STARTING || chaos_web_terminal_state()==CHAOS_WEB_TERMINAL_RUNNING)chaos_web_terminal_stop();
    if(listener)pan_tcp_unlisten(listener);
    listener=0;active_clients=0;exec_session=exec_idle_at=retry_pending=0;
}
static void wipe(void *data,unsigned n){volatile unsigned char *p=data;while(n--)*p++=0;}
static int load_credentials(void){
    char user[32]={0},password[65]={0},joined[97]={0};
    int rc=chaos_web_credentials(user,sizeof(user),password,sizeof(password));
    unsigned u=0,p=0;
    while(u<sizeof(user) && user[u])u++; while(p<sizeof(password) && password[p])p++;
    if(!rc && (!u || u==sizeof(user) || p<5 || p>31))rc=-13;
    for(unsigned i=0;!rc && i<u;i++)if(user[i]<33 || user[i]>126 || user[i]==':')rc=-22;
    for(unsigned i=0;!rc && i<p;i++)if(password[i]<32 || password[i]>126)rc=-22;
    if(!rc){memcpy(joined,user,u);joined[u]=':';memcpy(joined+u+1,password,p);web_base64((const uint8_t*)joined,u+p+1,credential);}
    wipe(user,sizeof(user));wipe(password,sizeof(password));wipe(joined,sizeof(joined));
    return rc;
}
void chaos_httpd_poll(uint32_t now){
    unsigned epoch=__atomic_load_n(&control,__ATOMIC_ACQUIRE)&~1u;
    if(epoch!=seen_epoch){stop_transport();wipe(credential,sizeof(credential));reboot_pending=0;retry_at=0;seen_epoch=epoch;}
    if(!load_enabled()){stop_transport();wipe(credential,sizeof(credential));reboot_pending=0;return;}
    if(!pan_ip_ready()){stop_transport();wipe(credential,sizeof(credential));reboot_pending=0;return;}
    if(!credential[0]){int rc=load_credentials();if(rc){last_error=rc;return;}last_error=0;}
    if(reboot_pending==2 && (int32_t)(now-reboot_at)>=0){int state=chaos_web_terminal_state();if(state!=CHAOS_WEB_TERMINAL_STARTING && state!=CHAOS_WEB_TERMINAL_RUNNING && state!=CHAOS_WEB_TERMINAL_STOPPING){last_error=chaos_web_reboot();reboot_pending=0;}else if((int32_t)(now-(reboot_at+5000))>=0){last_error=-16;reboot_pending=0;}}
    if(exec_session && !shell && (int32_t)(now-exec_idle_at)>=0){chaos_web_terminal_stop();exec_session=0;}
    if(!listener && (!retry_pending || (int32_t)(now-retry_at)>=0)){int id=pan_tcp_listen(CHAOS_HTTPD_PORT);if(id>0){listener=id;last_error=0;retry_pending=0;}else{last_error=id;retry_at=now+2000;retry_pending=1;}}
    if(listener){int id=pan_tcp_accept(listener);if(id>0){unsigned i=0;while(i<CLIENTS && clients[i].id)i++;if(i==CLIENTS){pan_tcp_close(id);refused++;}else{memset(&clients[i],0,sizeof(clients[i]));clients[i].id=id;clients[i].deadline=now+8000;}}else if(id==-9){listener=0;retry_at=now+2000;retry_pending=1;}}
    unsigned count=0;
    for(unsigned i=0;i<CLIENTS;i++){struct client *c=&clients[i];if(!c->id)continue;int state=pan_tcp_state(c->id);if(state<0 || state==PAN_TCP_ERROR){failed++;drop(c);continue;}if((int32_t)(now-c->deadline)>=0){failed++;drop(c);continue;}if(c->phase==0)read_http(c);else if(c->phase==2)read_ws(c);if(c->id && (c->phase==1 || (c->phase==2 && c->length)))send_pending(c);if(c->id && state==PAN_TCP_EOF && c->phase!=1 && !c->length)drop(c);if(c->id)count++;}
    active_clients=count;
}
void chaos_httpd_snapshot(struct chaos_httpd_view *out){if(!out)return;out->enabled=load_enabled();out->listening=listener>0;out->clients=active_clients;out->requests=served;out->failures=failed;out->refused=refused;out->last_error=last_error;}
void chaos_httpd_status(char *out,unsigned cap){unsigned a=0;if(!out || !cap)return;out[0]=0;if(!load_enabled()){put(out,0,cap,"已停止");return;}struct pan_ip_info ip;pan_ip_info(&ip);if(!ip.ready){put(out,0,cap,"等待局域网连接");return;}if(!listener){put(out,0,cap,last_error==-13?"请先设置网页密码":"正在启动");return;}a=put(out,a,cap,"http://");a=iptext(out,a,cap,ip.ip);a=put(out,a,cap," · ");a=num(out,a,cap,served);put(out,a,cap," 次请求");}
#ifdef CHAOS_HTTPD_TEST
void chaos_httpd_reset(void){memset(clients,0,sizeof(clients));listener=0;control=0;served=failed=refused=0;retry_at=reboot_at=exec_idle_at=0;reboot_pending=exec_session=retry_pending=0;shell=0;app_size=app_running=0;last_error=0;seen_epoch=active_clients=0;wipe(credential,sizeof(credential));}
#endif

