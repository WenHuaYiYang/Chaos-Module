#include "chaos_connect.h"
#include "chaos_io.h"
/* Chaos file operations, direct VFS calls, no module ABI. */
#include <stddef.h>
#include <stdint.h>

#define CCF_PROTO_VERSION 1u
#define CCF_MAX_FRAME CC_MAX_PAYLOAD
#define CCF_MAX_PATH 255u
#define CCF_CHUNK CC_CHUNK
#define CCF_PAYLOAD_MAX CC_MAX_PAYLOAD
#define CCF_MAX_ENTRIES 16u
#define CCF_REPORT_STEP 8192u

#define CCF_OP_HELLO 0x01u
#define CCF_OP_LIST 0x02u
#define CCF_OP_STAT 0x03u
#define CCF_OP_MKDIR 0x04u
#define CCF_OP_REMOVE 0x05u
#define CCF_OP_RMDIR 0x06u
#define CCF_OP_RENAME 0x07u
#define CCF_OP_COPY 0x08u
#define CCF_OP_OPEN_READ 0x10u
#define CCF_OP_READ 0x11u
#define CCF_OP_CLOSE_READ 0x12u
#define CCF_OP_OPEN_WRITE 0x20u
#define CCF_OP_WRITE 0x21u
#define CCF_OP_COMMIT 0x22u
#define CCF_OP_ABORT 0x23u
#define CCF_OP_PING 0x30u
#define CCF_OP_SET_DEV 0x40u
#define CCF_OP_GET_DEV 0x41u

#define CCF_ST_OK 0u
#define CCF_ST_BAD 1u
#define CCF_ST_NOTFOUND 2u
#define CCF_ST_ACCESS 3u
#define CCF_ST_EXIST 4u
#define CCF_ST_NOTDIR 5u
#define CCF_ST_ISDIR 6u
#define CCF_ST_NOSPACE 7u
#define CCF_ST_BUSY 8u
#define CCF_ST_IO 9u
#define CCF_ST_CRC 10u
#define CCF_ST_SEQUENCE 11u
#define CCF_ST_UNSUPPORTED 12u
#define CCF_ST_TOO_LARGE 13u

#define CCF_FLAG_DEV_MODE 1u
#define CCF_FLAG_RMDIR 2u
#define CCF_ST_PENDING 14u
#define CCF_POLICY_ALLOW 0u
#define CCF_POLICY_DENY 1u
#define CCF_POLICY_ASK 2u
extern uint32_t chaos_connect_android_policy(void),chaos_connect_android_mask(void);
extern uint32_t chaos_connect_plugin_policy(void),chaos_connect_plugin_mask(void);
extern uint32_t chaos_connect_now_ms(void);
extern uint32_t chaos_connect_policy(void);
extern uint32_t chaos_connect_take_decision(uint32_t token);
extern void chaos_connect_state(uint32_t flags,uint32_t token);

/* No libc imports. Keep all helpers internal; the stock modlib loader only
 * has to apply ABS32/TARGET1 data relocations, not intra-module Thumb calls. */
static size_t ccf_strlen(const char *s)
{
    size_t n = 0;
    while (s[n]) n++;
    return n;
}

static int ccf_strcmp(const char *a, const char *b)
{
    while (*a && *a == *b) { a++; b++; }
    return (unsigned char)*a - (unsigned char)*b;
}

static uint16_t g_chunk=CC_CHUNK;
static uint8_t g_dev_mode;
static uint8_t g_connected,auth_state;
static uint32_t auth_token,auth_serial,auth_started,last_seen;
static uint8_t android_client,android_forced,connect_auth,access_granted;
static uint32_t connect_token,connect_started;
static uint32_t client_policy(void){return android_client?chaos_connect_android_policy():chaos_connect_plugin_policy();}
static uint32_t client_mask(void){return android_client?chaos_connect_android_mask():chaos_connect_plugin_mask();}
static uint8_t pack_state;static uint32_t pack_token;
static void publish_state(void){chaos_connect_state(g_connected|(uint32_t)g_dev_mode<<1|(uint32_t)auth_state<<2|
    (uint32_t)android_client<<5|(uint32_t)connect_auth<<6|(uint32_t)(pack_state==1)<<9,pack_state==1?pack_token:connect_auth==1?connect_token:auth_token);}
void cc_files_set_android(uint32_t value){android_forced=!!value;}
static uint32_t next_token(void){auth_serial=(auth_serial+1)&0x3fffffffu;if(!auth_serial)auth_serial++;return auth_serial;}


static int32_t g_read_fd = -1;
static uint32_t g_read_size;
static char g_read_target[CC_PATH_MAX];
static uint32_t g_read_transferred;
static int32_t g_write_fd = -1;
static char g_write_target[CC_PATH_MAX];
static char g_write_temp[CC_PATH_MAX];
static uint32_t g_write_expected;
static uint32_t g_write_written;
static uint32_t g_write_crc;
static uint32_t g_write_reported;

static uint16_t ccf_get_u16(const uint8_t *p)
{
    return (uint16_t)((uint16_t)p[0] | ((uint16_t)p[1] << 8));
}

static uint32_t ccf_get_u32(const uint8_t *p)
{
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) |
           ((uint32_t)p[3] << 24);
}

static void ccf_put_u16(uint8_t *p, uint16_t v)
{
    p[0] = (uint8_t)v;
    p[1] = (uint8_t)(v >> 8);
}

static void ccf_put_u32(uint8_t *p, uint32_t v)
{
    p[0] = (uint8_t)v;
    p[1] = (uint8_t)(v >> 8);
    p[2] = (uint8_t)(v >> 16);
    p[3] = (uint8_t)(v >> 24);
}

static uint32_t ccf_copy_bytes(uint8_t *dst, const uint8_t *src, uint32_t n)
{
    uint32_t i;
    for (i = 0; i < n; i++) dst[i] = src[i];
    return n;
}

static char ccf_lower(char c)
{
    if (c >= 'A' && c <= 'Z') return (char)(c + ('a' - 'A'));
    return c;
}


static int ccf_under(const char *path, const char *root)
{
    uint32_t i = 0;
    while (root[i]) {
        if (ccf_lower(path[i]) != ccf_lower(root[i])) return 0;
        i++;
    }
    return path[i] == '\0' || path[i] == '/';
}

static int ccf_protected(const char *path)
{
    if (ccf_under(path, "/dev")) return 1;
    if (ccf_under(path, "/proc")) return 1;
    if (ccf_under(path, "/sys")) return 1;
    return 0;
}

static int ccf_normalize(const char *in, char *out)
{
    uint32_t o = 0;
    const char *s = in;
    if (in == 0 || in[0] != '/') return -1;
    while (*s) {
        const char *e;
        uint32_t n;
        while (*s == '/') s++;
        if (*s == '\0') break;
        e = s;
        while (*e && *e != '/') {
            if ((uint8_t)*e < 0x20u) return -1;
            e++;
        }
        n = (uint32_t)(e - s);
        if (n == 1 && s[0] == '.') {
            /* skip */
        } else if (n == 2 && s[0] == '.' && s[1] == '.') {
            if (o == 0) return -1;
            while (o > 0 && out[o - 1] != '/') o--;
            if (o > 0) o--;
        } else {
            if (o + n + 2u >= CC_PATH_MAX) return -1;
            out[o++] = '/';
            {
                uint32_t i;
                for (i = 0; i < n; i++) out[o++] = s[i];
            }
        }
        s = e;
    }
    if (o == 0) out[o++] = '/';
    out[o] = '\0';
    return 0;
}

static int ccf_parse_path(const uint8_t *p, uint32_t plen, uint32_t *used, char *out)
{
    uint16_t n;
    uint32_t i;
    char raw[CC_PATH_MAX];
    if (plen < 2u) return -1;
    n = ccf_get_u16(p);
    if (n == 0u || n >= CC_PATH_MAX || plen < (uint32_t)n + 2u) return -1;
    for (i = 0; i < n; i++) {
        if (p[2u + i] == 0u) return -1;
        raw[i] = (char)p[2u + i];
    }
    raw[n] = '\0';
    if (ccf_normalize(raw, out) != 0) return -1;
    if (used) *used = (uint32_t)n + 2u;
    return 0;
}

static uint8_t ccf_status_from_err(int32_t err)
{
    switch (err) {
    case 0: return CCF_ST_OK;
    case -2: return CCF_ST_NOTFOUND;
    case -13: return CCF_ST_ACCESS;
    case -17: return CCF_ST_EXIST;
    case -20: return CCF_ST_NOTDIR;
    case -21: return CCF_ST_ISDIR;
    case -28: return CCF_ST_NOSPACE;
    case -16:
    case -39: return CCF_ST_BUSY;
    case -38: return CCF_ST_UNSUPPORTED;
    case -22: return CCF_ST_BAD;
    default: return CCF_ST_IO;
    }
}

static int ccf_client_allowed(uint32_t mask){
    return chaos_bridge_enabled()&&access_granted&&client_policy()!=CCF_POLICY_DENY&&
        (client_mask()&mask)==mask;
}
static int ccf_write_allowed(const char *path)
{
    if(!ccf_client_allowed(0))return 0;
    if(!ccf_under(path,"/data") && (chaos_connect_policy()==1 ||
       !(client_mask()&8u)))return 0;
    if (ccf_protected(path)||path[1]=='\0') return 0;
    /* The peer cannot edit the watch's own consent policy or rename its ancestors. */
    if(ccf_under(path,"/data/chaos/connect-permissions.cfg")||
       ccf_under(path,"/data/chaos/connect-permissions.tmp")||
       ccf_under(path,"/data/chaos/connect-plugin.cfg")||ccf_under(path,"/data/chaos/connect-plugin.tmp")||
       ccf_under(path,"/data/chaos/connect-install.cfg")||ccf_under(path,"/data/chaos/connect-install.tmp")||
       ccf_under(path,"/data/chaos/connect-android.cfg")||ccf_under(path,"/data/chaos/connect-android.tmp")||
       ccf_under("/data/chaos/connect-permissions.cfg",path))return 0;
    if(android_client&&(ccf_under(path,"/data/chaos/font")||ccf_under(path,"/data/chaos/icons")))return 0;
    if (g_dev_mode) return 1;
    return ccf_under(path, "/data");
}

static int ccf_read_allowed(const char *path)
{
    return ccf_client_allowed(1)&&!ccf_protected(path);
}

/* Owner-loop INFO audit. No flash writes or plugin changes; never log file contents.
 * Capture paths before a handler closes/discards its transaction. */
extern void chaos_connect_log(const char *message);
static char audit_context[768];
static uint32_t audit_used,audit_offset,audit_length,benchmark_bytes;
static void audit_text(const char *s){while(*s&&audit_used+1<sizeof(audit_context))audit_context[audit_used++]=*s++;audit_context[audit_used]=0;}
static void audit_number(uint32_t n){char b[10];unsigned i=0;do{b[i++]='0'+n%10;n/=10;}while(n);while(i){char c[2]={b[--i],0};audit_text(c);}}
static void audit_path(const char *s){audit_text(" \"");audit_text(*s?s:"<无活动文件>");audit_text("\"");}
static const char *audit_action(uint8_t op){
 switch(op){
 case CCF_OP_HELLO:return "连接/协商协议";case CCF_OP_LIST:return "浏览文件夹";
 case CCF_OP_STAT:return "查看文件/目录信息";case CCF_OP_MKDIR:return "新建文件夹";
 case CCF_OP_REMOVE:return "删除文件";case CCF_OP_RMDIR:return "删除空文件夹";
 case CCF_OP_RENAME:return "移动/重命名";case CCF_OP_COPY:return "复制文件";
 case CCF_OP_OPEN_READ:return "打开文件读取（预览/下载）";case CCF_OP_READ:return "读取文件数据";
 case CCF_OP_CLOSE_READ:return "关闭读取文件";case CCF_OP_OPEN_WRITE:return "打开文件写入（上传/保存编辑）";
 case CCF_OP_WRITE:return "写入文件数据";case CCF_OP_COMMIT:return "校验并提交文件";
 case CCF_OP_ABORT:return "取消写入/清理临时文件";case CCF_OP_PING:return "连接检查";
 case 0x50:return "启动NSH终端";case 0x51:return "NSH输入";case 0x52:return "读取NSH输出";case 0x53:return "关闭NSH终端";
 case 0x31:return "链路回显测速";case CCF_OP_SET_DEV:return "设置开发者模式";
 case CCF_OP_GET_DEV:return "查询开发者模式";case 0x42:return "查询客户端权限";case 0x43:return "结束客户端会话";default:return "未知请求";}
}
static void audit_begin(const struct cc_frame *f){
 const uint8_t *p=f->payload;uint32_t used=0,next=0;char path[CC_PATH_MAX];
 audit_used=0;audit_offset=audit_length=0;audit_text("req=");audit_number(f->req);
 audit_text(" ");audit_text(audit_action(f->op));
 if((f->op>=CCF_OP_LIST&&f->op<=CCF_OP_COPY)||f->op==CCF_OP_OPEN_READ||f->op==CCF_OP_OPEN_WRITE){
  if(ccf_parse_path(p,f->len,&used,path)){audit_text(" <无效路径>");return;}audit_path(path);
  if(f->op==CCF_OP_RENAME||f->op==CCF_OP_COPY){
   audit_text(" ->");if(ccf_parse_path(p+used,f->len-used,&next,path)){audit_text(" <无效目标路径>");return;}audit_path(path);
   if(f->op==CCF_OP_COPY&&f->len>used+next){audit_text(" overwrite=");audit_number(p[used+next]);}
  }else if(f->op==CCF_OP_LIST&&f->len>=used+5){audit_text(" offset=");audit_number(ccf_get_u32(p+used));audit_text(" limit=");audit_number(p[used+4]);}
  else if(f->op==CCF_OP_OPEN_WRITE&&f->len>=used+5){audit_text(" expected_bytes=");audit_number(ccf_get_u32(p+used));audit_text(" overwrite=");audit_number(p[used+4]);}
 }else if(f->op==CCF_OP_READ||f->op==CCF_OP_CLOSE_READ){
  audit_path(g_read_target);
  if(f->op==CCF_OP_READ&&f->len>=6){audit_offset=ccf_get_u32(p);audit_length=ccf_get_u16(p+4);audit_text(" offset=");audit_number(audit_offset);audit_text(" requested=");audit_number(audit_length);}
  else{audit_text(" transferred_bytes=");audit_number(g_read_transferred);}
 }else if(f->op==CCF_OP_WRITE||f->op==CCF_OP_COMMIT||f->op==CCF_OP_ABORT){
  audit_path(g_write_target);
  if(f->op==CCF_OP_WRITE&&f->len>=4){audit_offset=ccf_get_u32(p);audit_length=f->len-4;audit_text(" offset=");audit_number(audit_offset);audit_text(" bytes=");audit_number(audit_length);}
  else{audit_text(" written_bytes=");audit_number(g_write_written);if(f->op==CCF_OP_COMMIT&&f->len>=4){audit_text(" expected_crc32=");audit_number(ccf_get_u32(p));}}
 }else if(f->op==CCF_OP_SET_DEV&&f->len){audit_text(p[0]?" 开启":" 关闭");}
 else if(f->op==0x31){audit_text(" bytes=");audit_number(f->len);}
 else if(f->op>CCF_OP_GET_DEV){audit_text(" opcode=");audit_number(f->op);}
}
static void ccf_send(uint8_t op,uint16_t req,uint8_t status,const uint8_t *payload,uint32_t plen){
 int queued=cc_reply(CC_RESPONSE,op,status,req,payload,plen);
 /* INFO has start/end and 64KiB transfer checkpoints, not one row per packet.
  * Failures always log, including response queue failures. */
 if(op==CCF_OP_READ&&status==0)g_read_transferred+=plen;
 if(op==0x31&&status==0)benchmark_bytes+=plen;
 if(status==0&&queued>=0){
  if(op==CCF_OP_PING||op==0x42||op==0x52||op==0x61||op==0x63)return; /* Polling is presence, not a user file operation. */
  if(op==CCF_OP_READ&&audit_offset!=0&&audit_offset/65536==(audit_offset+plen)/65536&&audit_offset+plen<g_read_size&&plen==audit_length)return;
  if(op==CCF_OP_WRITE&&audit_offset!=0&&audit_offset/65536==(audit_offset+audit_length)/65536&&g_write_written<g_write_expected)return;
  if(op==0x31&&benchmark_bytes!=plen&&(benchmark_bytes-plen)/262144==benchmark_bytes/262144)return;
 }
 static const char *const result[]={"成功","参数错误","不存在","没有权限","目标已存在","不是目录","目标是目录","空间不足","设备忙/目录非空","读写失败","CRC校验失败","传输顺序错误","不支持","超出大小限制","等待手环授权"};
 audit_text(" | ");audit_text(status<15?result[status]:"未知状态");audit_text(" status=");audit_number(status);
 if(queued<0)audit_text(" 应答入队失败");
 if(status==0){
  if(op==CCF_OP_LIST&&plen>=6){audit_text(" total=");audit_number(ccf_get_u16(payload));audit_text(" returned=");audit_number(ccf_get_u16(payload+4));}
  else if(op==CCF_OP_STAT&&plen>=9){audit_text(payload[0]?" 文件夹":" 文件");audit_text(" size=");audit_number(ccf_get_u32(payload+1));audit_text(" mtime=");audit_number(ccf_get_u32(payload+5));}
  else if(op==CCF_OP_OPEN_READ&&plen>=4){audit_text(" size=");audit_number(ccf_get_u32(payload));}
  else if(op==CCF_OP_READ){audit_text(" returned_bytes=");audit_number(plen);}
  else if(op==CCF_OP_WRITE&&plen>=8){audit_text(" committed_offset=");audit_number(ccf_get_u32(payload));}
  else if(op==CCF_OP_HELLO&&plen>=11){audit_text(" version=");audit_number(payload[0]);audit_text(" chunk=");audit_number(ccf_get_u16(payload+1));audit_text(" window=");audit_number(ccf_get_u32(payload+7));}
  else if(op==CCF_OP_GET_DEV&&plen){audit_text(payload[0]?" 开启":" 关闭");}
  else if(op==0x31){audit_text(" echoed_total=");audit_number(benchmark_bytes);}
 }
 chaos_connect_log(audit_context);
}
static void ccf_event(uint8_t code,const uint8_t *payload,uint32_t plen)
{(void)cc_reply(CC_EVENT,code,0,0,payload,plen);}
static void ccf_close_read(void){if(g_read_fd>=0)(void)cc_io_close(g_read_fd);g_read_fd=-1;g_read_size=0;g_read_target[0]=0;g_read_transferred=0;}
static void ccf_discard_write(void){
 if(g_write_fd>=0)(void)cc_io_close(g_write_fd);g_write_fd=-1;
 if(g_write_temp[0])(void)cc_io_remove(g_write_temp);
 g_write_temp[0]=g_write_target[0]=0;g_write_expected=g_write_written=g_write_reported=0;g_write_crc=0xffffffffu;
}
#include "chaos_terminal.inc"
#include "chaos_package.inc"
void cc_files_reset(void){
 ccf_terminal_stop();ccp_reset();
 if(g_read_fd>=0){audit_used=0;audit_text("会话结束/重置：关闭读取文件");audit_path(g_read_target);chaos_connect_log(audit_context);}
 if(g_write_fd>=0){audit_used=0;audit_text("会话结束/重置：取消未提交写入并清理临时文件");audit_path(g_write_target);chaos_connect_log(audit_context);}
 ccf_close_read();ccf_discard_write();g_dev_mode=0;benchmark_bytes=0;
 g_connected=0;auth_state=0;auth_token=0;android_client=0;connect_auth=0;connect_token=0;access_granted=0;publish_state();
}
int cc_files_tick(uint32_t now){
 if(!g_connected)return 0;
 if((uint32_t)(now-last_seen)>=15000u){chaos_connect_log("插件心跳超时：断开ChaosConnect并撤销开发者模式和待定授权");cc_files_reset();return 1;}
 {
  uint32_t connection_policy=client_policy(),mask=client_mask();
  if(connection_policy==CCF_POLICY_DENY){if(access_granted||connect_auth==1)chaos_connect_log(android_client?"手环撤销Android连接许可":"手环撤销插件连接许可");access_granted=0;connect_auth=3;}
  if(connect_auth==1){
   uint32_t decision=chaos_connect_take_decision(connect_token);
   if((uint32_t)(now-connect_started)>=60000u){connect_auth=4;chaos_connect_log(android_client?"Android连接确认超时，拒绝访问":"插件连接确认超时，拒绝访问");}
   else if(decision||connection_policy==CCF_POLICY_ALLOW){access_granted=decision!=2;connect_auth=access_granted?2:3;chaos_connect_log(android_client?(access_granted?"手环允许Android本次连接":"手环拒绝Android本次连接"):(access_granted?"手环允许插件本次连接":"手环拒绝插件本次连接"));}
  }
  if(!access_granted){ccf_close_read();ccf_discard_write();g_dev_mode=0;auth_state=0;auth_token=0;}
  if(!android_client || !access_granted || !(mask&32u) || !chaos_bridge_enabled())ccf_terminal_stop();
  if(!(mask&1u))ccf_close_read();
  if(!(mask&2u))ccf_discard_write();
  if(!(mask&8u)){if(g_write_fd>=0&&!ccf_under(g_write_target,"/data"))ccf_discard_write();g_dev_mode=0;if(auth_state==1)auth_state=3;}
 }
 uint32_t policy=chaos_connect_policy();
 if(policy==CCF_POLICY_DENY){
  if(g_dev_mode){g_dev_mode=0;if(auth_token)auth_state=3;
   if(g_write_fd>=0&&!ccf_under(g_write_target,"/data")){ccf_discard_write();chaos_connect_log("手环权限关闭：取消系统区域的未提交写入");}
   chaos_connect_log("手环权限设为关闭：撤销开发者模式");}
  if(auth_state==1){auth_state=3;chaos_connect_log("手环权限设为关闭：拒绝待定的开发者模式请求");}
 }
 if(auth_state==1){
  if((uint32_t)(now-auth_started)>=60000u){auth_state=4;chaos_connect_log("开发者模式授权超时，未启用");}
  else{uint32_t decision=chaos_connect_take_decision(auth_token);
   if(decision){auth_state=decision==1?2:3;g_dev_mode=decision==1;
    chaos_connect_log(decision==1?"用户在手环允许插件启用开发者模式":"用户在手环拒绝插件启用开发者模式");}
  }
 }
 ccp_tick(now);publish_state();return 0;
}
void cc_files_set_chunk(uint16_t chunk){g_chunk=chunk>0&&chunk<=CC_CHUNK?chunk:CC_CHUNK;}
static uint32_t ccf_crc32_update(uint32_t crc, const uint8_t *data, uint32_t len)
{
    uint32_t i, j;
    for (i = 0; i < len; i++) {
        crc ^= data[i];
        for (j = 0; j < 8u; j++) {
            if (crc & 1u) crc = (crc >> 1) ^ 0xEDB88320u;
            else crc >>= 1;
        }
    }
    return crc;
}

static int ccf_temp_for(const char *target, char *out)
{
    uint32_t n = (uint32_t)ccf_strlen(target);
    static uint32_t serial;
    static const char hex[]="0123456789abcdef";
    struct cc_stat st;
    if(n==0||n+21>=CC_PATH_MAX)return -1;
    ccf_copy_bytes((uint8_t *)out,(const uint8_t *)target,n);
    ccf_copy_bytes((uint8_t *)out+n,(const uint8_t *)".cc-",4);
    for(unsigned attempt=0;attempt<32;attempt++){
        uint32_t id=++serial;
        for(unsigned i=0;i<8;i++)out[n+4+i]=hex[(id>>(28-4*i))&15];
        ccf_copy_bytes((uint8_t *)out+n+12,(const uint8_t *)".part",6);
        if(cc_io_stat(out,&st)==-2)return 0;
    }
    return -1;
}

static int ccf_path_join(const char *dir, const char *name, char *out)
{
    uint32_t dl = (uint32_t)ccf_strlen(dir);
    uint32_t nl = (uint32_t)ccf_strlen(name);
    if (dl + nl + 2u >= CC_PATH_MAX) return -1;
    if (dl == 1u && dir[0] == '/') {
        out[0] = '/';
        ccf_copy_bytes((uint8_t *)out + 1, (const uint8_t *)name, nl);
        out[nl + 1] = '\0';
    } else {
        ccf_copy_bytes((uint8_t *)out, (const uint8_t *)dir, dl);
        out[dl] = '/';
        ccf_copy_bytes((uint8_t *)out + dl + 1, (const uint8_t *)name, nl);
        out[dl + nl + 1] = '\0';
    }
    return 0;
}

static void ccf_handle_hello(uint16_t req)
{
    uint8_t out[16];
    uint32_t n = 0;
    out[n++] = CCF_PROTO_VERSION;
    ccf_put_u16(&out[n], g_chunk); n += 2u;
    out[n++] = (uint8_t)(CCF_FLAG_DEV_MODE | CCF_FLAG_RMDIR | 4u | 8u | (android_client?16u:0u) | 32u); /* status/authorization support */
    out[n++] = g_dev_mode;
    ccf_put_u16(&out[n], (uint16_t)CCF_MAX_PATH); n += 2u;
    ccf_put_u32(&out[n], CC_WINDOW); n += 4u;
    ccf_send(CCF_OP_HELLO, req, CCF_ST_OK, out, n);
}

static void ccf_handle_list(uint16_t req,
                           const uint8_t *p, uint32_t plen)
{
    char path[CC_PATH_MAX];
    uint32_t used = 0, offset, idx = 0, count = 0, total = 0;
    uint8_t limit, page_full=0;
    static uint8_t out[CCF_MAX_FRAME];
    uint32_t o = 0;
    void *dir;
    if (ccf_parse_path(p, plen, &used, path) != 0 || plen < used + 5u) {
        ccf_send(CCF_OP_LIST, req, CCF_ST_BAD, 0, 0);
        return;
    }
    if (!ccf_read_allowed(path)) {
        ccf_send(CCF_OP_LIST, req, CCF_ST_ACCESS, 0, 0);
        return;
    }
    offset = ccf_get_u32(p + used);
    limit = p[used + 4u];
    if(!limit){ccf_send(CCF_OP_LIST,req,CCF_ST_BAD,0,0);return;}
    if (limit > CCF_MAX_ENTRIES) limit = CCF_MAX_ENTRIES;
    dir = cc_io_opendir(path);
    if (dir == 0) {
        ccf_send(CCF_OP_LIST, req, CCF_ST_NOTFOUND, 0, 0);
        return;
    }
    o = 6u; /* total/offset/count placeholder */
    while (1) {
        struct cc_dirent ent;
        int32_t rc = cc_io_readdir(dir, &ent);
        char full[CC_PATH_MAX];
        struct cc_stat st;
        uint32_t nl;
        uint32_t need;
        if (rc == 1) break;
        if (rc < 0) {
            (void)cc_io_closedir(dir);
            ccf_send(CCF_OP_LIST, req, ccf_status_from_err(rc), 0, 0);
            return;
        }
        if (ent.name[0] == '.' && (ent.name[1] == '\0' ||
            (ent.name[1] == '.' && ent.name[2] == '\0'))) continue;
        nl = (uint32_t)ccf_strlen(ent.name);
        total++;
        if (idx++ < offset) continue;
        if (count >= limit || page_full) continue;
        need = 10u + nl;
        if (o + need > g_chunk) {page_full=1;continue;}
        st.size = 0;
        st.mtime = 0;
        st.is_dir = ent.is_dir;
        if (ccf_path_join(path, ent.name, full) == 0) {
            struct cc_stat tmp;
            if (cc_io_stat(full, &tmp) == 0) {
                st.size = tmp.size;
                st.mtime = tmp.mtime;
                st.is_dir = tmp.is_dir;
            }
        }
        out[o++] = (uint8_t)nl;
        ccf_copy_bytes(&out[o], (const uint8_t *)ent.name, nl); o += nl;
        out[o++] = (uint8_t)(st.is_dir ? 1u : 0u);
        ccf_put_u32(&out[o], (uint32_t)st.size); o += 4u;
        ccf_put_u32(&out[o], (uint32_t)st.mtime); o += 4u;
        count++;
    }
    (void)cc_io_closedir(dir);
    if(!count&&total>offset){ccf_send(CCF_OP_LIST,req,CCF_ST_TOO_LARGE,0,0);return;}
    ccf_put_u16(&out[0], (uint16_t)(total > 0xFFFFu ? 0xFFFFu : total));
    ccf_put_u16(&out[2], (uint16_t)(offset > 0xFFFFu ? 0xFFFFu : offset));
    ccf_put_u16(&out[4], (uint16_t)count);
    ccf_send(CCF_OP_LIST, req, CCF_ST_OK, out, o);
}

static void ccf_handle_stat(uint16_t req,
                           const uint8_t *p, uint32_t plen)
{
    char path[CC_PATH_MAX];
    struct cc_stat st;
    uint8_t out[9];
    int32_t rc;
    if (ccf_parse_path(p, plen, 0, path) != 0) {
        ccf_send(CCF_OP_STAT, req, CCF_ST_BAD, 0, 0);
        return;
    }
    if (!ccf_read_allowed(path)) {
        ccf_send(CCF_OP_STAT, req, CCF_ST_ACCESS, 0, 0);
        return;
    }
    rc = cc_io_stat(path, &st);
    if (rc < 0) {
        ccf_send(CCF_OP_STAT, req, ccf_status_from_err(rc), 0, 0);
        return;
    }
    out[0] = (uint8_t)(st.is_dir ? 1u : 0u);
    ccf_put_u32(&out[1], (uint32_t)st.size);
    ccf_put_u32(&out[5], (uint32_t)st.mtime);
    ccf_send(CCF_OP_STAT, req, CCF_ST_OK, out, 9);
}

static void ccf_handle_mkdir(uint16_t req,
                            const uint8_t *p, uint32_t plen)
{
    char path[CC_PATH_MAX];
    int32_t rc;
    if (ccf_parse_path(p, plen, 0, path) != 0) {
        ccf_send(CCF_OP_MKDIR, req, CCF_ST_BAD, 0, 0);
        return;
    }
    if (!ccf_write_allowed(path)) {
        ccf_send(CCF_OP_MKDIR, req, CCF_ST_ACCESS, 0, 0);
        return;
    }
    rc = cc_io_mkdir(path);
    ccf_send(CCF_OP_MKDIR, req, rc < 0 ? ccf_status_from_err(rc) : CCF_ST_OK, 0, 0);
}

static void ccf_handle_remove(uint16_t req,
                             const uint8_t *p, uint32_t plen, uint8_t op)
{
    char path[CC_PATH_MAX];
    int32_t rc;
    if (ccf_parse_path(p, plen, 0, path) != 0) {
        ccf_send(op, req, CCF_ST_BAD, 0, 0);
        return;
    }
    if (!ccf_write_allowed(path)) {
        ccf_send(op, req, CCF_ST_ACCESS, 0, 0);
        return;
    }
    if (op == CCF_OP_RMDIR) {
        rc = cc_io_rmdir(path);
    } else {
        rc = cc_io_remove(path);
    }
    ccf_send(op, req, rc < 0 ? ccf_status_from_err(rc) : CCF_ST_OK, 0, 0);
}

static void ccf_handle_rename(uint16_t req,
                             const uint8_t *p, uint32_t plen)
{
    char from[CC_PATH_MAX];
    char to[CC_PATH_MAX];
    uint32_t used = 0;
    int32_t rc;
    if (ccf_parse_path(p, plen, &used, from) != 0 || used >= plen ||
        ccf_parse_path(p + used, plen - used, 0, to) != 0) {
        ccf_send(CCF_OP_RENAME, req, CCF_ST_BAD, 0, 0);
        return;
    }
    if (!ccf_write_allowed(from) || !ccf_write_allowed(to)) {
        ccf_send(CCF_OP_RENAME, req, CCF_ST_ACCESS, 0, 0);
        return;
    }
    rc = cc_io_rename(from, to);
    ccf_send(CCF_OP_RENAME, req, rc < 0 ? ccf_status_from_err(rc) : CCF_ST_OK, 0, 0);
}

static void ccf_handle_copy(uint16_t req,
                          const uint8_t *p, uint32_t plen)
{
    char from[CC_PATH_MAX];
    char to[CC_PATH_MAX];
    char temp[CC_PATH_MAX];
    struct cc_stat st;
    uint32_t first = 0, second = 0;
    uint8_t overwrite;
    int32_t src = -1, dst = -1, rc;
    uint8_t buf[512];
    if (ccf_parse_path(p, plen, &first, from) != 0 ||
        ccf_parse_path(p + first, plen - first, &second, to) != 0 ||
        plen < first + second + 1u) {
        ccf_send(CCF_OP_COPY, req, CCF_ST_BAD, 0, 0);
        return;
    }
    overwrite = p[first + second];
    if (!ccf_read_allowed(from) || !ccf_write_allowed(to)) {
        ccf_send(CCF_OP_COPY, req, CCF_ST_ACCESS, 0, 0);
        return;
    }
    if (ccf_strcmp(from, to)) {
        /* Different paths, continue. */
    } else {
        ccf_send(CCF_OP_COPY, req, CCF_ST_BAD, 0, 0);
        return;
    }
    if (cc_io_stat(from, &st) < 0) {
        ccf_send(CCF_OP_COPY, req, CCF_ST_NOTFOUND, 0, 0);
        return;
    }
    if (st.is_dir) {
        ccf_send(CCF_OP_COPY, req, CCF_ST_ISDIR, 0, 0);
        return;
    }
    if (cc_io_stat(to, &st) == 0) {
        if (st.is_dir) {
            ccf_send(CCF_OP_COPY, req, CCF_ST_ISDIR, 0, 0);
            return;
        }
        if (!overwrite) {
            ccf_send(CCF_OP_COPY, req, CCF_ST_EXIST, 0, 0);
            return;
        }
    }
    if (ccf_temp_for(to, temp) != 0) {
        ccf_send(CCF_OP_COPY, req, CCF_ST_BAD, 0, 0);
        return;
    }
    (void)cc_io_remove(temp);
    src = cc_io_open(from, CC_IO_READ);
    if (src < 0) {
        ccf_send(CCF_OP_COPY, req, ccf_status_from_err(src), 0, 0);
        return;
    }
    dst = cc_io_open(temp, CC_IO_WRITE | CC_IO_CREATE | CC_IO_TRUNC);
    if (dst < 0) {
        (void)cc_io_close(src);
        ccf_send(CCF_OP_COPY, req, ccf_status_from_err(dst), 0, 0);
        return;
    }
    for (;;) {
        if(!ccf_client_allowed(3)||!ccf_read_allowed(from)||!ccf_write_allowed(to)){rc=-13;goto fail;}
        int32_t got = cc_io_read(src, buf, sizeof(buf));
        uint32_t sent = 0;
        if (got < 0) {
            rc = got;
            goto fail;
        }
        if (got == 0) break;
        while (sent < (uint32_t)got) {
            int32_t wrote = cc_io_write(dst, buf + sent, (uint32_t)got - sent);
            if (wrote <= 0) {
                rc = wrote < 0 ? wrote : -5;
                goto fail;
            }
            sent += (uint32_t)wrote;
        }
    }
    (void)cc_io_sync(dst);
    rc = cc_io_close(dst);
    dst = -1;
    if (rc < 0) goto fail;
    (void)cc_io_close(src);
    src = -1;
    if(!ccf_client_allowed(3)||!ccf_read_allowed(from)||!ccf_write_allowed(to)){rc=-13;goto fail;}
    rc = cc_io_rename(temp, to);
    if (rc < 0) {
        (void)cc_io_remove(temp);
        ccf_send(CCF_OP_COPY, req, ccf_status_from_err(rc), 0, 0);
        return;
    }
    ccf_send(CCF_OP_COPY, req, CCF_ST_OK, 0, 0);
    return;
fail:
    if (dst >= 0) (void)cc_io_close(dst);
    if (src >= 0) (void)cc_io_close(src);
    (void)cc_io_remove(temp);
    ccf_send(CCF_OP_COPY, req, ccf_status_from_err(rc), 0, 0);
}

static void ccf_handle_open_read(uint16_t req,
                                const uint8_t *p, uint32_t plen)
{
    char path[CC_PATH_MAX];
    uint8_t out[4];
    int32_t fd;
    int64_t size;
    if (ccf_parse_path(p, plen, 0, path) != 0) {
        ccf_send(CCF_OP_OPEN_READ, req, CCF_ST_BAD, 0, 0);
        return;
    }
    if (!ccf_read_allowed(path)) {
        ccf_send(CCF_OP_OPEN_READ, req, CCF_ST_ACCESS, 0, 0);
        return;
    }
    ccf_close_read();
    fd = cc_io_open(path, CC_IO_READ);
    if (fd < 0) {
        ccf_send(CCF_OP_OPEN_READ, req, ccf_status_from_err(fd), 0, 0);
        return;
    }
    size = cc_io_size(fd);
    if (size < 0) {
        (void)cc_io_close(fd);
        ccf_send(CCF_OP_OPEN_READ, req, ccf_status_from_err((int32_t)size), 0, 0);
        return;
    }
    g_read_fd = fd;
    ccf_copy_bytes((uint8_t *)g_read_target,(const uint8_t *)path,(uint32_t)ccf_strlen(path)+1);
    g_read_size = (uint32_t)size;
    ccf_put_u32(out, g_read_size);
    ccf_send(CCF_OP_OPEN_READ, req, CCF_ST_OK, out, 4);
}

static void ccf_handle_read(uint16_t req,
                           const uint8_t *p, uint32_t plen)
{
    static uint8_t out[CCF_CHUNK];
    uint32_t offset, want;
    int32_t rc;
    if (g_read_fd < 0 || plen < 6u) {
        ccf_send(CCF_OP_READ, req, CCF_ST_BAD, 0, 0);
        return;
    }
    offset = ccf_get_u32(p);
    want = ccf_get_u16(p + 4);
    if (want > g_chunk) want = g_chunk;
    if (offset >= g_read_size) {
        ccf_send(CCF_OP_READ, req, CCF_ST_OK, 0, 0);
        return;
    }
    if (want > g_read_size - offset) want = g_read_size - offset;
    if (cc_io_seek(g_read_fd, (int64_t)offset, 0) < 0) {
        ccf_send(CCF_OP_READ, req, CCF_ST_IO, 0, 0);
        return;
    }
    {
        uint32_t got = 0;
        while (got < want) {
            rc = cc_io_read(g_read_fd, out + got, want - got);
            if (rc < 0) {
                ccf_send(CCF_OP_READ, req, ccf_status_from_err(rc), 0, 0);
                return;
            }
            if (rc == 0) break;
            got += (uint32_t)rc;
        }
        ccf_send(CCF_OP_READ, req, CCF_ST_OK, out, got);
    }
}

static void ccf_handle_open_write(uint16_t req,
                                 const uint8_t *p, uint32_t plen)
{
    char path[CC_PATH_MAX];
    char temp[CC_PATH_MAX];
    uint32_t used = 0, expected;
    uint8_t overwrite;
    struct cc_stat st;
    int32_t fd;
    if (ccf_parse_path(p, plen, &used, path) != 0 || plen < used + 5u) {
        ccf_send(CCF_OP_OPEN_WRITE, req, CCF_ST_BAD, 0, 0);
        return;
    }
    expected = ccf_get_u32(p + used);
    overwrite = p[used + 4u];
    if(expected>32u*1024u*1024u){ccf_send(CCF_OP_OPEN_WRITE,req,CCF_ST_TOO_LARGE,0,0);return;}
    if (!ccf_write_allowed(path)) {
        ccf_send(CCF_OP_OPEN_WRITE, req, CCF_ST_ACCESS, 0, 0);
        return;
    }
    if (cc_io_stat(path, &st) == 0 && st.is_dir) {
        ccf_send(CCF_OP_OPEN_WRITE, req, CCF_ST_ISDIR, 0, 0);
        return;
    }
    if (!overwrite && cc_io_stat(path, &st) == 0) {
        ccf_send(CCF_OP_OPEN_WRITE, req, CCF_ST_EXIST, 0, 0);
        return;
    }
    if (ccf_temp_for(path, temp) != 0) {
        ccf_send(CCF_OP_OPEN_WRITE, req, CCF_ST_BAD, 0, 0);
        return;
    }
    ccf_discard_write();
    (void)cc_io_remove(temp);
    fd = cc_io_open(temp, CC_IO_WRITE | CC_IO_CREATE | CC_IO_TRUNC);
    if (fd < 0) {
        ccf_send(CCF_OP_OPEN_WRITE, req, ccf_status_from_err(fd), 0, 0);
        return;
    }
    g_write_fd = fd;
    ccf_copy_bytes((uint8_t *)g_write_target, (const uint8_t *)path, (uint32_t)ccf_strlen(path) + 1u);
    ccf_copy_bytes((uint8_t *)g_write_temp, (const uint8_t *)temp, (uint32_t)ccf_strlen(temp) + 1u);
    g_write_expected = expected;
    g_write_written = 0;
    g_write_reported = 0;
    g_write_crc = 0xFFFFFFFFu;
    ccf_send(CCF_OP_OPEN_WRITE, req, CCF_ST_OK, 0, 0);
}

static void ccf_handle_write(uint16_t req,
                            const uint8_t *p, uint32_t plen)
{
    uint32_t offset, n;
    int32_t rc;
    if (g_write_fd < 0 || plen < 4u) {
        ccf_send(CCF_OP_WRITE, req, CCF_ST_BAD, 0, 0);
        return;
    }
    offset = ccf_get_u32(p);
    n = plen - 4u;
    if(n>g_chunk){ccf_send(CCF_OP_WRITE,req,CCF_ST_TOO_LARGE,0,0);return;}
    if (offset != g_write_written) {
        ccf_send(CCF_OP_WRITE, req, CCF_ST_SEQUENCE, 0, 0);
        return;
    }
    if (g_write_written > g_write_expected ||
        n > g_write_expected - g_write_written) {
        ccf_send(CCF_OP_WRITE, req, CCF_ST_TOO_LARGE, 0, 0);
        return;
    }
    {
        uint32_t sent = 0;
        while (sent < n) {
            rc = cc_io_write(g_write_fd, p + 4 + sent, n - sent);
            if (rc <= 0) {
                uint8_t error=rc<0?ccf_status_from_err(rc):CCF_ST_IO;
                ccf_discard_write();
                ccf_send(CCF_OP_WRITE, req, error, 0, 0);
                return;
            }
            sent += (uint32_t)rc;
        }
    }
    g_write_crc = ccf_crc32_update(g_write_crc, p + 4, n);
    g_write_written += n;
    if (g_write_written - g_write_reported >= CCF_REPORT_STEP ||
        g_write_written == g_write_expected) {
        uint8_t ev[8];
        ccf_put_u32(&ev[0], g_write_written);
        ccf_put_u32(&ev[4], g_write_crc ^ 0xFFFFFFFFu);
        ccf_event(0xC1u, ev, 8u);
        g_write_reported = g_write_written;
    }
    uint8_t ack[8];ccf_put_u32(ack,g_write_written);ccf_put_u32(ack+4,g_write_crc^0xffffffffu);
    ccf_send(CCF_OP_WRITE, req, CCF_ST_OK, ack, sizeof(ack));
}

static void ccf_handle_commit(uint16_t req,
                             const uint8_t *p, uint32_t plen)
{
    uint32_t expected_crc;
    int32_t rc;
    if (g_write_fd < 0 || plen < 4u) {
        ccf_send(CCF_OP_COMMIT, req, CCF_ST_BAD, 0, 0);
        return;
    }
    expected_crc = ccf_get_u32(p);
    if (g_write_written != g_write_expected) {
        ccf_send(CCF_OP_COMMIT, req, CCF_ST_IO, 0, 0);
        return;
    }
    if ((g_write_crc ^ 0xFFFFFFFFu) != expected_crc) {
        ccf_discard_write();
        ccf_send(CCF_OP_COMMIT, req, CCF_ST_CRC, 0, 0);
        return;
    }
    (void)cc_io_sync(g_write_fd);
    rc = cc_io_close(g_write_fd);
    g_write_fd = -1;
    if (rc < 0) {
        ccf_discard_write();
        ccf_send(CCF_OP_COMMIT, req, ccf_status_from_err(rc), 0, 0);
        return;
    }
    if(!ccf_client_allowed(2)||!ccf_write_allowed(g_write_target)){ccf_discard_write();ccf_send(CCF_OP_COMMIT,req,CCF_ST_ACCESS,0,0);return;}
    rc = cc_io_rename(g_write_temp, g_write_target);
    if (rc < 0) {
        ccf_discard_write();
        ccf_send(CCF_OP_COMMIT, req, ccf_status_from_err(rc), 0, 0);
        return;
    }
    g_write_temp[0] = '\0';
    g_write_target[0] = '\0';
    g_write_expected = 0;
    g_write_written = 0;
    g_write_crc = 0xFFFFFFFFu;
    g_write_reported = 0;
    ccf_send(CCF_OP_COMMIT, req, CCF_ST_OK, 0, 0);
}

static void ccf_handle_set_dev(uint16_t req,
                              const uint8_t *p, uint32_t plen)
{
    if (plen < 1u) {
        ccf_send(CCF_OP_SET_DEV, req, CCF_ST_BAD, 0, 0);
        return;
    }
    if(!p[0]){
     if(g_write_fd>=0&&!ccf_under(g_write_target,"/data"))ccf_discard_write();
     g_dev_mode=0;auth_state=0;auth_token=0;publish_state();ccf_send(CCF_OP_SET_DEV,req,CCF_ST_OK,0,0);return;
    }
    uint32_t policy=chaos_connect_policy();
    if(policy==CCF_POLICY_DENY){ccf_send(CCF_OP_SET_DEV,req,CCF_ST_ACCESS,0,0);return;}
    if(g_dev_mode||policy==CCF_POLICY_ALLOW){g_dev_mode=1;auth_state=0;auth_token=0;publish_state();ccf_send(CCF_OP_SET_DEV,req,CCF_ST_OK,0,0);return;}
    if(auth_state!=1){auth_token=next_token();auth_state=1;auth_started=chaos_connect_now_ms();}
    publish_state();uint8_t out[4];ccf_put_u32(out,auth_token);ccf_send(CCF_OP_SET_DEV,req,CCF_ST_PENDING,out,4);
}

void cc_files_dispatch(const struct cc_frame *frame)
{
 uint8_t op=frame->op;uint16_t req=frame->req;
 const uint8_t *p=frame->payload;uint32_t plen=frame->len;
 if(frame->flags!=0||frame->status!=0)return;
    /* HELLO resets active transfers before its own audit context is constructed. */
    if(op==CCF_OP_HELLO){
     cc_files_reset();
     android_client=android_forced; /* Identity comes from the physical carrier, never HELLO bytes. */
     if(plen && !(plen==4&&p[0]=='C'&&p[1]=='C'&&p[2]=='A'&&p[3]=='1')){audit_begin(frame);ccf_send(op,req,CCF_ST_BAD,0,0);return;}
     uint32_t cp=client_policy();access_granted=cp==0;connect_auth=cp==0?2:cp==1?3:1;
     if(connect_auth==1){connect_token=next_token();connect_started=chaos_connect_now_ms();}
     chaos_connect_log(android_client?"Android请求建立ChaosConnect会话，等待手环权限检查":"插件请求建立ChaosConnect会话，等待手环权限检查");
    }
    if(op!=CCF_OP_HELLO)(void)cc_files_tick(chaos_connect_now_ms());
    last_seen=chaos_connect_now_ms();
    if(op==CCF_OP_HELLO){g_connected=1;publish_state();}
    audit_begin(frame);
    if(!g_connected){ccf_send(op,req,CCF_ST_ACCESS,0,0);return;}
    if(op!=CCF_OP_HELLO && op!=CCF_OP_PING && op!=0x42 && op!=0x43){
     uint32_t need=0,mask=client_mask();
     switch(op){
      case CCF_OP_LIST:case CCF_OP_STAT:case CCF_OP_OPEN_READ:case CCF_OP_READ:case CCF_OP_COPY:need=1;break;
      case CCF_OP_MKDIR:case CCF_OP_OPEN_WRITE:case CCF_OP_WRITE:case CCF_OP_COMMIT:need=2;break;
      case CCF_OP_REMOVE:case CCF_OP_RMDIR:case CCF_OP_RENAME:need=4;break;
      case CCF_OP_SET_DEV:if(plen&&p[0])need=8;break;
      case 0x50:case 0x51:case 0x52:case 0x53:need=android_client?32u:0u;break;
     }
     if(op==CCF_OP_COPY)need|=2;
     if(!chaos_bridge_enabled() || !access_granted || client_policy()==CCF_POLICY_DENY || (mask&need)!=need){ccf_send(op,req,CCF_ST_ACCESS,0,0);return;}
    }
    if(op>=0x50&&op<=0x53&&!android_client){ccf_send(op,req,CCF_ST_UNSUPPORTED,0,0);return;}
    if(op>=0x60&&op<=0x66){ccp_dispatch(op,req,p,plen);return;}
    switch (op) {
    case 0x50:case 0x51:case 0x52:case 0x53:ccf_terminal_dispatch(op,req,p,plen);break;
    case 0x42: {
     if(plen){ccf_send(op,req,CCF_ST_BAD,0,0);break;}
     uint8_t out[16]={1,android_client,(uint8_t)client_policy(),connect_auth};
     ccf_put_u32(out+4,client_mask());ccf_put_u32(out+8,connect_token);out[12]=access_granted;
     ccf_send(op,req,CCF_ST_OK,out,sizeof(out));break;
    }
    case 0x43:cc_files_reset();ccf_send(op,req,CCF_ST_OK,0,0);break;
    case CCF_OP_HELLO: ccf_handle_hello(req); break;
    case CCF_OP_LIST: ccf_handle_list(req, p, plen); break;
    case CCF_OP_STAT: ccf_handle_stat(req, p, plen); break;
    case CCF_OP_MKDIR: ccf_handle_mkdir(req, p, plen); break;
    case CCF_OP_REMOVE: ccf_handle_remove(req, p, plen, op); break;
    case CCF_OP_RMDIR: ccf_handle_remove(req, p, plen, op); break;
    case CCF_OP_RENAME: ccf_handle_rename(req, p, plen); break;
    case CCF_OP_COPY: ccf_handle_copy(req, p, plen); break;
    case CCF_OP_OPEN_READ: ccf_handle_open_read(req, p, plen); break;
    case CCF_OP_READ: ccf_handle_read(req, p, plen); break;
    case CCF_OP_CLOSE_READ:
        ccf_close_read();
        ccf_send(op, req, CCF_ST_OK, 0, 0);
        break;
    case CCF_OP_OPEN_WRITE: ccf_handle_open_write(req, p, plen); break;
    case CCF_OP_WRITE: ccf_handle_write(req, p, plen); break;
    case CCF_OP_COMMIT: ccf_handle_commit(req, p, plen); break;
    case CCF_OP_ABORT:
        ccf_discard_write();
        ccf_send(op, req, CCF_ST_OK, 0, 0);
        break;
    case CCF_OP_PING: {
        uint8_t out[8]={g_connected,g_dev_mode,(uint8_t)chaos_connect_policy(),auth_state};
        ccf_put_u32(out+4,auth_token);
        ccf_send(op, req, CCF_ST_OK, out, sizeof(out));
        break;
    }
    case 0x31: /* Link benchmark: echo actual bytes through the full encrypted carrier. */
        ccf_send(op,req,plen>g_chunk?CCF_ST_TOO_LARGE:CCF_ST_OK,plen>g_chunk?0:p,plen>g_chunk?0:plen);
        break;
    case CCF_OP_SET_DEV: ccf_handle_set_dev(req, p, plen); break;
    case CCF_OP_GET_DEV: {
        uint8_t one = g_dev_mode;
        ccf_send(op, req, CCF_ST_OK, &one, 1u);
        break;
    }
    default:
        ccf_send(op, req, CCF_ST_BAD, 0, 0);
        break;
    }
}
