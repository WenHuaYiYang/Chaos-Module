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
/* 每个 TCP 客户端的全部状态。REQUEST_CAP 是最贵的一块（3 个连接 × 18 KiB），
 * 它必须装得下 /api/apps/upload 的 16 KiB 请求体，所以别再往下削。
 * 字段含义：
 *   id             pan_tcp 句柄，0 表示空槽
 *   used/head      已收字节数 / 头部结束位置（\r\n\r\n 之后）
 *   content        本次请求体长度；parsed = 头部已校验过
 *   method         1=GET 2=HEAD 3=POST
 *   phase          0=读请求 1=响应中 2=已升级为 WebSocket
 *   sent/length/hs 已发字节 / 响应体总长 / 响应头字节数
 *   ws/auth        已切 WebSocket / 该连接已通过鉴权（Basic 或 AUTH 帧）
 *   fragment/fraglen  WS 分片状态与累计长度
 *   closing        已发 close 帧、等对端回
 *   reboot_reply   本条响应发完就重启
 *   deadline       超时点（毫秒），到点 drop
 *   data           待发内容，可能指向 out / header / app
 *   request/out/header/message  请求、响应体、响应头、单帧 WS 载荷的暂存 */
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
/* 开关带一个"代"计数器：bit0 是开关本身，高位每次翻转都 +2。
 * 消费者只看代的奇偶/变化就能知道"中途被关过又重新开过"，从而必须重建监听与凭据。
 * 用 CAS 循环而不是锁：调用方可能在 BT 回调里，不能阻塞。 */
void chaos_httpd_enable(int on)
{
    unsigned old = __atomic_load_n(&control, __ATOMIC_ACQUIRE), next;
    do {
        if ((old & 1u) == (unsigned)!!on) return;
        next = ((old + 2u) & ~1u) | (unsigned)!!on;
    } while (!__atomic_compare_exchange_n(&control, &old, next, 0,
                                          __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE));
}
int chaos_httpd_enabled(void){return (int)load_enabled();}
static void copy(char *out,const char *s,unsigned cap){unsigned n=0;while(s && s[n] && n+1<cap){out[n]=s[n];n++;}if(cap)out[n]=0;}
static unsigned put(char *out,unsigned at,unsigned cap,const char *s){while(*s && at+1<cap)out[at++]=*s++;out[at]=0;return at;}
static unsigned num(char *out,unsigned at,unsigned cap,unsigned v){char b[12];unsigned n=0;do{b[n++]=(char)('0'+v%10);v/=10;}while(v);while(n && at+1<cap)out[at++]=b[--n];out[at]=0;return at;}
/* JSON 字符串转义写入：引号/反斜杠前加 \，控制字符转成 \u00XX，其余原样（UTF-8 不动）。
 * 预算按 at+8<cap 收：最坏一条转义要 6 字节，留 8 字节余量保证收尾引号放得下。
 * 注意它会截断而不是报错 —— 调用方都是"拼诊断 JSON"，截断可接受。 */
static unsigned quote(char *out, unsigned at, unsigned cap, const char *s)
{
    at = put(out, at, cap, "\"");
    for (; *s && at + 8 < cap; s++) {
        unsigned char b = (unsigned char)*s;
        if (b == '"' || b == '\\') {
            out[at++] = '\\';
            out[at++] = (char)b;
        } else if (b < 32) {
            static const char h[] = "0123456789abcdef";
            out[at++] = '\\';
            out[at++] = 'u';
            out[at++] = '0';
            out[at++] = '0';
            out[at++] = h[b >> 4];
            out[at++] = h[b & 15];
        } else out[at++] = (char)b;
    }
    out[at] = 0;
    return put(out, at, cap, "\"");
}
static unsigned iptext(char *out,unsigned at,unsigned cap,const uint8_t *ip){for(unsigned i=0;i<4;i++){if(i)at=put(out,at,cap,".");at=num(out,at,cap,ip[i]);}return at;}
static int equal_ci(const char *a,const char *b,unsigned n){for(unsigned i=0;i<n;i++){char x=a[i],y=b[i];if(x>='A'&&x<='Z')x+=32;if(y>='A'&&y<='Z')y+=32;if(x!=y)return 0;}return 1;}
/* 取一个请求头的值。返回 1 = 找到并写入 out；0 = 没有；-1 = 重复头或放不下。
 *
 * 两条硬规矩：
 *   - 头名大小写不敏感，但冒号前不允许有空格（等于拒绝 "Host : x" 这种走私写法）；
 *   - 同名头出现第二次直接失败 —— 这是请求走私的经典入口，不猜哪个才对。
 * 值的首尾 OWS（空格/TAB）会被剥掉，中间原样保留。 */
static int header(struct client *c, const char *name, char *out, unsigned cap)
{
    unsigned at = 0, n = (unsigned)strlen(name);
    int found = 0;
    out[0] = 0;
    /* 跳过请求行。 */
    while (at < c->head && c->request[at] != '\n') at++;
    at++;
    /* 逐行扫；空行（或只剩 CR）表示头结束。 */
    while (at + 2 < c->head) {
        unsigned start = at;
        while (at < c->head && c->request[at] != '\n') at++;
        unsigned end = at++;
        if (end > start && c->request[end - 1] == '\r') end--;
        if (end == start) break;

        /* end>start+n 保证冒号在头名之后，不会越界读。 */
        if (end > start + n && c->request[start + n] == ':'
            && equal_ci(c->request + start, name, n)) {
            if (found) return -1;
            found = 1;
            start += n + 1;
            while (start < end && (c->request[start] == ' ' || c->request[start] == '\t')) start++;
            while (end > start && (c->request[end - 1] == ' ' || c->request[end - 1] == '\t')) end--;
            if (end - start >= cap) return -1;
            memcpy(out, c->request + start, end - start);
            out[end - start] = 0;
        }
    }
    return found;
}
/* 逗号分隔头里找 token（用于 Connection: keep-alive, Upgrade）。
 * 前后空格吃掉再比，比较大小写不敏感；不分配、不改原串。 */
static int has_token(const char *s, const char *wanted)
{
    unsigned n = (unsigned)strlen(wanted);
    while (*s) {
        while (*s == ' ' || *s == ',') s++;
        const char *a = s;
        while (*s && *s != ',') s++;
        unsigned k = (unsigned)(s - a);
        while (k && a[k - 1] == ' ') k--;
        if (k == n && equal_ci(a, wanted, n)) return 1;
    }
    return 0;
}
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
/* 同源检查：Origin 缺失视为同源（curl/脚本），有就必须与 Host 完全一致。
 * 只认 http:// 前缀：这条服务本身不跑 TLS，https 页面来调它一定是跨源。 */
static int same_origin(struct client *c)
{
    char origin[128], host[96];
    int n = header(c, "Origin", origin, sizeof(origin));
    if (n < 0) return 0;
    if (!n) return 1;
    if (header(c, "Host", host, sizeof(host)) != 1) return 0;
    return !strncmp(origin, "http://", 7) && !strcmp(origin + 7, host);
}
static int authorized(struct client *c){char auth[160];return header(c,"Authorization",auth,sizeof(auth))==1 && !strncmp(auth,"Basic ",6) && !strcmp(auth+6,credential);}
static void drop(struct client *c){if(shell==c){if(c->auth)chaos_web_terminal_stop();shell=0;}if(c->reboot_reply)reboot_pending=0;if(c->id)pan_tcp_close(c->id);memset(c,0,sizeof(*c));}
/* 组装响应头并进入"发送中"态。要点：
 *   - 头部攒在 c->header、正文留在 c->data，两者分开走，发送时先头后体；
 *   - HEAD 请求只发头（length 记 0），但 Content-Length 仍报真实长度；
 *   - 401 补 WWW-Authenticate，让浏览器弹账号框；
 *   - app 是用户上传的网页，必须套 CSP sandbox，别让它拿到同源权限。 */
static void respond(struct client *c, unsigned code, const char *reason, const char *type,
                    const void *data, unsigned length, int gzip)
{
    unsigned a = 0;
    a = put(c->header, a, sizeof(c->header), "HTTP/1.1 ");
    a = num(c->header, a, sizeof(c->header), code);
    a = put(c->header, a, sizeof(c->header), " ");
    a = put(c->header, a, sizeof(c->header), reason);
    a = put(c->header, a, sizeof(c->header), "\r\nContent-Type: ");
    a = put(c->header, a, sizeof(c->header), type);
    a = put(c->header, a, sizeof(c->header), "\r\nContent-Length: ");
    a = num(c->header, a, sizeof(c->header), length);

    if (gzip)
        a = put(c->header, a, sizeof(c->header), "\r\nContent-Encoding: gzip");
    if (code == 401)
        a = put(c->header, a, sizeof(c->header), "\r\nWWW-Authenticate: Basic realm=\"Chaos\"");
    if (data == (const void *)app)
        /* 这一条是一整根字符串：内容安全策略不能被拆成相邻字面量，否则 token 校验过不去，
         * 也说明"改版式"和"改内容"混在了一起。 */
        a = put(c->header, a, sizeof(c->header), "\r\nContent-Security-Policy: sandbox allow-scripts; default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; connect-src 'none'; form-action 'none'; base-uri 'none'");
    a = put(c->header, a, sizeof(c->header), "\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n");

    c->hs = a;
    c->data = data;
    c->length = c->method == 2 ? 0 : length;
    c->sent = 0;
    c->phase = 1;
    c->deadline = pan_ip_now() + 60000;
}
/* 一次性 JSON 响应：body 先拷进 c->out（respond 要求 length 与 data 自洽），
 * 再走统一响应头。200 用 OK，其余 Error。 */
static void json(struct client *c, unsigned code, const char *s)
{
    copy(c->out, s, sizeof(c->out));
    respond(c, code, code == 200 ? "OK" : "Error",
            "application/json; charset=utf-8", c->out, (unsigned)strlen(c->out), 0);
}
/* 状态快照：一次把系统信息、IP、以太网计数与计数器拼成一段 JSON。
 *
 * 三段拼接各有原因，别合并：
 *   - model/firmware/build 是无条件字段，走 S()（带引号转义）；
 *   - uptime/heap* 只在平台报了对应能力位时才存在，走 O()（否则写 null）；
 *   - meminfo 可能全是需要 6 字节转义的字符，所以 quote 的上限先扣掉 768 字节，
 *     给后面固定的 ip/统计后缀留位置，避免尾巴被截断。
 * 末尾的 cpu/temperature/storage 恒为 null：这一版没有对应数据源，字段留着是为了
 * 让前端不用做存在性判断。 */
static void status(struct client *c)
{
    struct chaos_web_sysinfo info = {0};
    struct pan_ip_info ip;
    struct pan_eth_stats eth;

    chaos_web_sysinfo_read(&info);
    /* 平台回填的三段字符串不保证以 NUL 结尾，这里按各自容量兜底截断。 */
    info.model[sizeof(info.model) - 1] = info.firmware[sizeof(info.firmware) - 1]
        = info.build[sizeof(info.build) - 1] = info.meminfo[sizeof(info.meminfo) - 1] = 0;
    pan_ip_info(&ip);
    pan_eth_snapshot(&eth);

    unsigned a = 0;
/* clang-format off —— 这三个宏是"一个表达式一个字段"的拼装器，展开成多行反而更难读。 */
#define S(k,v) do{a=put(c->out,a,OUT_CAP,k);a=quote(c->out,a,OUT_CAP,v);}while(0)
#define N(k,v) do{a=put(c->out,a,OUT_CAP,k);a=num(c->out,a,OUT_CAP,v);}while(0)
/* clang-format on */
    S("{\"model\":", info.model);
    S(",\"firmware\":", info.firmware);
    S(",\"build\":", info.build);
/* clang-format off */
#define O(k,v,flag) do{if(info.valid&(flag)){N(k,v);}else a=put(c->out,a,OUT_CAP,k "null");}while(0)
/* clang-format on */
    O(",\"uptime\":", info.uptime_seconds, CHAOS_WEB_HAVE_UPTIME);
    O(",\"heapTotal\":", info.heap_total, CHAOS_WEB_HAVE_HEAP);
    O(",\"heapFree\":", info.heap_free, CHAOS_WEB_HAVE_HEAP);
    O(",\"heapLargest\":", info.heap_largest, CHAOS_WEB_HAVE_HEAP);

    /* Leave space for the fixed network/statistics suffix even if meminfo
     * consists entirely of characters requiring six-byte JSON escapes. */
    if (info.valid & CHAOS_WEB_HAVE_MEMINFO) {
        a = put(c->out, a, OUT_CAP, ",\"meminfo\":");
        a = quote(c->out, a, OUT_CAP - 768u, info.meminfo);
    } else
        a = put(c->out, a, OUT_CAP, ",\"meminfo\":null");
    a = put(c->out, a, OUT_CAP,
            (info.valid & CHAOS_WEB_CAN_REBOOT) ? ",\"canReboot\":true" : ",\"canReboot\":false");
#undef O

    a = put(c->out, a, OUT_CAP, ",\"ip\":\"");
    a = iptext(c->out, a, OUT_CAP, ip.ip);
    a = put(c->out, a, OUT_CAP, "\",\"mask\":\"");
    a = iptext(c->out, a, OUT_CAP, ip.mask);
    a = put(c->out, a, OUT_CAP, "\",\"gateway\":\"");
    a = iptext(c->out, a, OUT_CAP, ip.gateway);
    a = put(c->out, a, OUT_CAP, "\",\"dns\":\"");
    a = iptext(c->out, a, OUT_CAP, ip.dns);
    a = put(c->out, a, OUT_CAP, "\"");

    N(",\"mtu\":", ip.mtu);
    N(",\"rxFrames\":", eth.rx_packets);
    N(",\"txFrames\":", eth.tx_packets);
    N(",\"requests\":", served);
    N(",\"failures\":", failed);
    N(",\"refused\":", refused);
    /* last_error 是负数 errno 风格，前端只认大小，这里转成绝对值。 */
    N(",\"lastError\":", last_error < 0 ? 0u - (unsigned)last_error : (unsigned)last_error);

    a = put(c->out, a, OUT_CAP,
            ",\"cpu\":null,\"temperature\":null,\"storage\":null,\"recordings\":false,\"logs\":false}");
    respond(c, 200, "OK", "application/json; charset=utf-8", c->out, a, 0);
#undef S
#undef N
}

/* 组一个 WebSocket 帧塞进 c->out 当待发内容。
 * 兼容两种长度头：<=125 用 1 字节，否则用 126 + 2 字节大端（我们不产生 127 那种 8 字节长度）。
 * 注意 c->out 是响应体缓冲，每次调用都会覆盖它 —— 所以"先发文本帧再发 close"时，
 * 前者的内容会被后者冲掉，要带原因的 close 必须走 ws_close_reason。 */
static void ws_frame(struct client *c, unsigned op, const void *data, unsigned n)
{
    if (n > OUT_CAP - 4) return;
    c->out[0] = (char)(128 | op);
    unsigned h = 2;
    if (n < 126) c->out[1] = (char)n;
    else {
        c->out[1] = 126;
        c->out[2] = (char)(n >> 8);
        c->out[3] = (char)n;
        h = 4;
    }
    if (n) memcpy(c->out + h, data, n);
    c->data = (const unsigned char *)c->out;
    c->length = n + h;
    c->sent = 0;
}
/* 带原因的 close 帧：状态码 2 字节大端 + UTF-8 原因，总长受限（控制帧 <=125）。
 * 原因写在这里而不是先发文本帧，是因为 ws_frame 每次都覆盖 c->out。 */
static void ws_close_reason(struct client *c, unsigned code, const char *text, unsigned n)
{
    uint8_t b[125];
    b[0] = (uint8_t)(code >> 8);
    b[1] = (uint8_t)code;
    if (n > sizeof(b) - 2) n = sizeof(b) - 2;
    if (n) memcpy(b + 2, text, n);
    ws_frame(c, 8, b, 2 + n);
    c->closing = 1;
}
static void ws_close(struct client *c,unsigned code){ws_close_reason(c,code,0,0);}
/* 升级到 WebSocket。握手校验按 RFC 6455 逐条来，任何一条不过就是 400：
 *   method 必须是 GET、不能带请求体、Sec-WebSocket-Key 必须能算出 accept、
 *   Version 必须是 13、Upgrade 必须是 websocket（大小写不敏感但长度必须 9）、
 *   Connection 必须含 upgrade、并且必须同源。
 * 同一时刻只允许一条终端通道：已有 shell / 待重启 / 有执行会话 -> 409。
 * 鉴权不在这一步做：先回 101，凭据放在第一帧 AUTH 里 —— 这样密码不会出现在 URL 或
 * 代理日志里，而且鉴权通过之前不会创建任何 NSH 任务。 */
static void upgrade(struct client *c)
{
    char key[64], accept[29], v[16], up[32], conn[64];
    if (c->method != 1 || c->content
        || header(c, "Sec-WebSocket-Key", key, sizeof(key)) != 1
        || web_accept_key(key, accept)
        || header(c, "Sec-WebSocket-Version", v, sizeof(v)) != 1 || strcmp(v, "13")
        || header(c, "Upgrade", up, sizeof(up)) != 1 || strlen(up) != 9
        || !equal_ci(up, "websocket", 9)
        || header(c, "Connection", conn, sizeof(conn)) != 1
        || !has_token(conn, "upgrade") || !same_origin(c)) {
        json(c, 400, "{\"error\":\"invalid_websocket_handshake\"}");
        return;
    }
    if (shell || reboot_pending || exec_session) {
        json(c, 409, "{\"error\":\"terminal_busy\"}");
        return;
    }
    /* Authentication happens in the first frame; never put credentials in URLs.
     * No NSH task is created before authentication. */
    /* 这一整根字符串是握手响应头：不能拆成相邻字面量，否则 token 会变。 */
    unsigned a = put(c->header, 0, sizeof(c->header), "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ");
    a = put(c->header, a, sizeof(c->header), accept);
    a = put(c->header, a, sizeof(c->header), "\r\n\r\n");
    c->hs = a;
    c->phase = 1;
    c->ws = 1;
    c->length = 0;
    c->sent = 0;
    c->deadline = pan_ip_now() + 8000;   /* 8s 内必须发完 101 并收到 AUTH */
    shell = c;
}
/* 路由分发。顺序本身就是语义，别随手调换：
 *   1) 方法不认识 -> 405，连路径都不读；
 *   2) 请求行形状不对 -> 400；
 *   3) 跨源 -> 403（在校验账号之前挡，免得外部页面拿它探口令）；
 *   4) /ws/nsh 走 WebSocket 升级，它自己用 AUTH 帧鉴权，不走 Basic；
 *   5) 首页免鉴权 —— 只是一层静态外壳，真正数据都在 /api/ 下；
 *   6) 其余路由逐条精确匹配，命中即 return，末尾统一 404。
 * 每条路由的 c->method 判据是"3 = POST"：!=3 表示 GET/HEAD 共用（HEAD 用于探活）。 */
static void serve(struct client *c)
{
    char path[128];
    unsigned pos = 0, end = 0;

    /* --- 请求行 METHOD SP PATH SP HTTP/1.x --- */
    while (pos < c->head && c->request[pos] != ' ') pos++;
    if (pos == 3 && !memcmp(c->request, "GET", 3)) c->method = 1;
    else if (pos == 4 && !memcmp(c->request, "HEAD", 4)) c->method = 2;
    else if (pos == 4 && !memcmp(c->request, "POST", 4)) c->method = 3;
    else {
        json(c, 405, "{\"error\":\"method_not_allowed\"}");
        return;
    }

    pos++;
    end = pos;
    while (end < c->head && c->request[end] != ' ') end++;
    /* end+10>c->head 是留给 "HTTP/1." 的最小余量，这一条别削。 */
    if (end == pos || end - pos >= sizeof(path) || end + 10 > c->head
        || memcmp(c->request + end + 1, "HTTP/1.", 7)) {
        json(c, 400, "{\"error\":\"request_line\"}");
        return;
    }
    memcpy(path, c->request + pos, end - pos);
    path[end - pos] = 0;

    /* --- 三道前置门 --- */
    if (!same_origin(c)) {
        json(c, 403, "{\"error\":\"origin\"}");
        return;
    }
    if (!strcmp(path, "/ws/nsh")) {
        upgrade(c);
        return;
    }
    if (c->method != 3 && (!strcmp(path, "/") || !strcmp(path, "/index.html"))) {
        respond(c, 200, "OK", "text/html; charset=utf-8", web_index, WEB_INDEX_SIZE, 1);
        return;
    }
    if (!authorized(c)) {
        json(c, 401, "{\"error\":\"authentication_required\"}");
        return;
    }

    /* --- 只读状态 --- */
    if (c->method != 3 && (!strcmp(path, "/api/system/status")
        || !strcmp(path, "/api/system/metrics") || !strcmp(path, "/status"))) {
        status(c);
        return;
    }

    /* --- 终端：写入命令 --- */
    if (c->method == 3 && !strcmp(path, "/api/nsh/exec")) {
        if (shell || reboot_pending) {
            json(c, 409, "{\"error\":\"terminal_busy\"}");
            return;
        }
        if (!c->content || c->content > 1024) {
            json(c, 413, "{\"error\":\"command_too_long\"}");
            return;
        }
        /* 0 和 ^D 会把 readline 带进奇怪状态，宁可拒收。 */
        for (unsigned i = 0; i < c->content; i++)
            if (c->request[c->head + i] == 0 || c->request[c->head + i] == 4) {
                json(c, 400, "{\"error\":\"unsupported_terminal_input\"}");
                return;
            }
        if (chaos_web_terminal_state() == CHAOS_WEB_TERMINAL_STOPPING) {
            json(c, 503, "{\"error\":\"terminal_stopping\"}");
            return;
        }
        /* p67 readline expects LF. Commit command and LF in ONE enqueue so a
         * full TX ring cannot leave an unfinished command in the next session. */
        unsigned length = c->content;
        char *command = c->request + c->head;
        if (command[length - 1] == '\r') command[length - 1] = '\n';
        else if (command[length - 1] != '\n') command[length++] = '\n';
        int rc = chaos_web_terminal_start();
        if (!rc) rc = chaos_web_terminal_send(command, length);
        if (!rc) {
            exec_session = 1;
            exec_idle_at = pan_ip_now() + 60000;
        } else if (!exec_session) chaos_web_terminal_stop();
        json(c, rc ? 503 : 200,
             rc ? "{\"error\":\"terminal_unavailable\"}"
                : "{\"accepted\":true,\"outputEndpoint\":\"/api/nsh/output\"}");
        return;
    }

    /* --- 终端：取回显 --- */
    if (c->method != 3 && !strcmp(path, "/api/nsh/output")) {
        if (shell) {
            json(c, 409, "{\"error\":\"terminal_busy\"}");
            return;
        }
        /* Worst case is six JSON bytes per output byte, plus metadata. */
        char text[480];
        unsigned n = c->method == 2 ? 0 : chaos_web_terminal_receive(text, sizeof(text) - 1);
        text[n] = 0;
        unsigned a = put(c->out, 0, OUT_CAP, "{\"output\":");
        a = quote(c->out, a, OUT_CAP, text);
        a = put(c->out, a, OUT_CAP, ",\"state\":");
        a = num(c->out, a, OUT_CAP, (unsigned)chaos_web_terminal_state());
        a = put(c->out, a, OUT_CAP, "}");
        if (exec_session) exec_idle_at = pan_ip_now() + 60000;
        respond(c, 200, "OK", "application/json; charset=utf-8", c->out, a, 0);
        return;
    }

    /* --- 重启：先让 WebSocket 收尾，再交给平台层 --- */
    if (c->method == 3 && !strcmp(path, "/api/system/reboot")) {
        struct chaos_web_sysinfo info;
        chaos_web_sysinfo_read(&info);
        if (!(info.valid & CHAOS_WEB_CAN_REBOOT)) {
            json(c, 501, "{\"error\":\"reboot_unavailable\"}");
            return;
        }
        if (reboot_pending) {
            json(c, 409, "{\"error\":\"reboot_pending\"}");
            return;
        }
        if (shell && shell->phase == 2 && !shell->length) ws_close(shell, 1001);
        chaos_web_terminal_stop();
        reboot_pending = 1;
        json(c, 200, "{\"accepted\":true}");
        c->reboot_reply = 1;
        return;
    }

    /* --- 网页小程序：上传 / 运行 / 停止（16 KiB 上限，装 RAM） --- */
    if (c->method == 3 && !strcmp(path, "/api/apps/upload")) {
        if (!c->content || c->content > APP_CAP) {
            json(c, 413, "{\"error\":\"maximum_16_KiB\"}");
            return;
        }
        /* 客户端正在读这份缓冲时不能覆盖它。 */
        for (unsigned i = 0; i < CLIENTS; i++)
            if (clients[i].id && clients[i].data == (const unsigned char *)app) {
                json(c, 409, "{\"error\":\"application_in_use\"}");
                return;
            }
        memcpy(app, c->request + c->head, c->content);
        app_size = c->content;
        app_running = 0;
        json(c, 200, "{\"id\":\"web\",\"stored\":\"ram\",\"uploaded\":true}");
        return;
    }
    if (c->method == 3 && !strcmp(path, "/api/apps/run")) {
        if (!app_size) {
            json(c, 409, "{\"error\":\"upload_first\"}");
            return;
        }
        app_running = 1;
        json(c, 200, "{\"url\":\"/app/\",\"running\":true}");
        return;
    }
    if (c->method == 3 && !strcmp(path, "/api/apps/stop")) {
        app_running = 0;
        json(c, 200, "{\"running\":false}");
        return;
    }
    if (c->method != 3 && !strcmp(path, "/app/") && app_running) {
        respond(c, 200, "OK", "text/html; charset=utf-8", app, app_size, 0);
        return;
    }

    json(c, 404, "{\"error\":\"not_found\"}");
}

/* Fragmented TCP headers/body, strict one Content-Length, no chunked requests. */
/* 收 HTTP 请求：先攒头、头齐了校验、等够 Content-Length 再交给 serve()。
 * 只有一份 request 缓冲，全靠四个游标推进，改动这里先看懂它们：
 *   used     已收字节数
 *   head     头部结束位置（\r\n\r\n 之后）；0 = 还没找到
 *   content  本次请求体长度
 *   parsed   头部已校验通过（同一份头不重复校验）
 * 头部超过 2048 字节直接 431：我们自己的页面不会发这么大的头。
 * 注意这里刻意不加花括号——这一版只做换行，任何括号增减都会让
 * scripts/check_c_tokens.py 判不等，那说明改动不止于版式。 */
static void read_http(struct client *c)
{
    int n = pan_tcp_receive(c->id, c->request + c->used, REQUEST_CAP - 1 - c->used);
    if (n > 0) {
        c->used += (unsigned)n;
        c->request[c->used] = 0;
    } else if (n < 0 && n != -11) {
        drop(c);
        return;
    }

    /* 找头部结束标记；从头往后扫，跨包时前 4 字节会重叠，所以从 i-3 开始比。 */
    if (!c->head) {
        for (unsigned i = 3; i < c->used; i++)
            if (!memcmp(c->request + i - 3, "\r\n\r\n", 4)) {
                c->head = i + 1;
                break;
            }
        if (!c->head && c->used > 2048) {
            json(c, 431, "{\"error\":\"headers_too_large\"}");
            return;
        }
    }
    if (c->head > 2048) {
        json(c, 431, "{\"error\":\"headers_too_large\"}");
        return;
    }

    /* 头部完整后校验一次：Content-Length 必须是纯数字且不超 APP_CAP，
     * 且不接受 Transfer-Encoding（我们会话里没有分块请求）。 */
    if (c->head && !c->parsed) {
        if (!valid_headers(c)) {
            json(c, 400, "{\"error\":\"invalid_headers\"}");
            return;
        }
        char len[16], transfer[32];
        int r = header(c, "Content-Length", len, sizeof(len));
        if (r < 0 || (r && !len[0])
            || header(c, "Transfer-Encoding", transfer, sizeof(transfer)) != 0) {
            json(c, 400, "{\"error\":\"invalid_framing\"}");
            return;
        }
        /* 边解析边溢出检查：size > (上限-当前位)/10 就说明再加一位要超。 */
        unsigned size = 0;
        for (unsigned i = 0; len[i]; i++) {
            if (len[i] < '0' || len[i] > '9') {
                json(c, 400, "{\"error\":\"content_length\"}");
                return;
            }
            if (size > (APP_CAP - (unsigned)(len[i] - '0')) / 10u) {
                json(c, 413, "{\"error\":\"body_too_large\"}");
                return;
            }
            size = size * 10 + (unsigned)(len[i] - '0');
        }
        if (c->head + size >= REQUEST_CAP) {
            json(c, 413, "{\"error\":\"body_too_large\"}");
            return;
        }
        c->content = size;
        c->parsed = 1;
    }

    /* 请求体到齐就处理。升级成 WebSocket 之后，紧跟在请求后面的字节是 WS 帧，
     * 不能丢，往前挪到缓冲开头继续用。 */
    if (c->parsed && c->used >= c->head + c->content) {
        serve(c);
        /* Preserve WebSocket bytes coalesced after upgrade. */
        if (c->ws) {
            unsigned extra = c->used - c->head - c->content;
            memmove(c->request, c->request + c->head + c->content, extra);
            c->used = extra;
        }
    }
}
/* 一条完整的 WebSocket 消息。
 * 未鉴权时只接受 AUTH 帧：格式 "AUTH <password>"，长度必须与凭据严格相等，
 * 错一个字都不谈（1008 直接关），绝不在 URL 里带密码。
 * 鉴权通过后这里只做一件事：把消息塞进本地终端的输入队列。
 * 队列满时不静默丢字符——回 1013（稍后再试），让前端知道被拒了。 */
static void message(struct client *c)
{
    if (!c->auth) {
        unsigned n = c->fraglen;
        if (n != strlen(credential) + 5 || memcmp(c->message, "AUTH ", 5)
            || memcmp(c->message + 5, credential, n - 5)) {
            ws_close(c, 1008);
            return;
        }
        int s = chaos_web_terminal_state();
        /* 关闭原因写进 close 帧的 reason —— ws_frame 每次覆盖 c->out, 先发一个文本帧
         * 再发 close 帧时前者会被覆盖掉(实测前端只看到空原因)。 */
        if (s == CHAOS_WEB_TERMINAL_STARTING || s == CHAOS_WEB_TERMINAL_RUNNING
            || s == CHAOS_WEB_TERMINAL_STOPPING) {
            char note[24];
            unsigned a = put(note, 0, sizeof(note), "会话占用 ");
            a = num(note, a, sizeof(note), (unsigned)s);
            ws_close_reason(c, 1013, note, a);
            return;
        }
        if (chaos_web_terminal_start()) {
            ws_close(c, 1011);
            return;
        }
        c->auth = 1;
        c->deadline = pan_ip_now() + 120000;
        return;
    }
    if (c->fraglen && chaos_web_terminal_send(c->message, c->fraglen)) {
        /* Do not silently drop a full TX queue. */
        ws_close(c, 1013);
    }
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
/* 收 WebSocket 帧。协议要点在这里，改之前先对齐：
 *   - 客户端帧必须带掩码（p[1]&128），没掩码就是攻击，1002 断开；
 *   - 控制帧（op>=8）必须 fin 且载荷 <=125；
 *   - 数据帧只接受 text(1)/binary(2)/continuation(0)，别的 op 一律 1003/1002；
 *   - 分片：continuation 只能接在分片之后，累计长度受 message 缓冲限制；
 *   - 每次拼够一帧就把剩余字节前移，缓冲区当作环形用。
 * 一条连接只允许一个客户端，所以 state 都在 struct client 上，没有额外会话表。 */
static void read_ws(struct client *c)
{
    if (c->used < sizeof(c->request)) {
        int n = pan_tcp_receive(c->id, c->request + c->used,
                                (unsigned)sizeof(c->request) - c->used);
        if (n > 0) c->used += (unsigned)n;
        else if (n < 0 && n != -11) {
            drop(c);
            return;
        }
    }
    /* 上一条响应还没发完就先不解析新帧，避免 c->out 被覆盖。 */
    if (c->length) return;

    unsigned budget = 4;   /* 一次 poll 最多处理 4 帧，防止饿死别的连接 */
    while (c->used >= 2 && budget--) {
        uint8_t *p = (uint8_t *)c->request;
        unsigned op = p[0] & 15, fin = p[0] >> 7, h = 2, n = p[1] & 127;
        /* RSV 位必须为 0、必须带掩码、载荷不能是 127（那是 8 字节长度）。 */
        if ((p[0] & 112) || !(p[1] & 128) || n == 127) {
            ws_close(c, 1002);
            return;
        }
        if (n == 126) {
            if (c->used < 4) return;
            n = (unsigned)p[2] << 8 | p[3];
            h = 4;
            if (n < 126) {
                ws_close(c, 1002);
                return;
            }
        }
        if ((op >= 8) && (!fin || n > 125)) {
            ws_close(c, 1002);
            return;
        }
        if (n > 1024) {
            ws_close(c, 1009);
            return;
        }
        if (c->used < h + 4 + n) return;   /* 帧还没收全，等下一次 */

        uint8_t *mask = p + h, *payload = mask + 4;
        for (unsigned i = 0; i < n; i++) payload[i] ^= mask[i % 4];
        if (c->auth) c->deadline = pan_ip_now() + 120000;

        if (op == 8) {
            /* close：1 字节载荷非法；带状态码时必须是合法码 + UTF-8 原因。 */
            if (n == 1 || (n >= 2 && !valid_close_code((unsigned)payload[0] << 8 | payload[1]))) {
                ws_close(c, 1002);
                return;
            }
            if (n > 2 && !valid_utf8(payload + 2, n - 2)) {
                ws_close(c, 1007);
                return;
            }
            ws_frame(c, 8, payload, n);
            c->closing = 1;
        } else if (op == 9) ws_frame(c, 10, payload, n);   /* ping -> pong */
        else if (op != 10) {
            /* 只收文本帧：二进制没有语义，1003 拒绝。 */
            if (op == 1) {
                ws_close(c, 1003);
                return;
            }
            if (op != 0 && op != 2) {
                ws_close(c, 1002);
                return;
            }
            /* 断续必须配对：没有分片却在续、或已有分片又起新帧，都是协议错。 */
            if ((op == 0 && !c->fragment) || (op != 0 && c->fragment)) {
                ws_close(c, 1002);
                return;
            }
            if (c->fraglen + n > sizeof(c->message)) {
                ws_close(c, 1009);
                return;
            }
            memcpy(c->message + c->fraglen, payload, n);
            c->fraglen += n;
            if (fin) {
                message(c);
                c->fragment = 0;
                c->fraglen = 0;
            } else c->fragment = 1;
        }

        unsigned total = h + 4 + n;
        c->used -= total;
        memmove(c->request, c->request + total, c->used);
        if (c->length || !c->id) return;
    }

    /* 没有待发内容就把终端回显泵出去；子进程结束/失败时补一个 1000 收尾。 */
    if (c->auth && !c->length) {
        char text[768];
        unsigned n = chaos_web_terminal_receive(text, sizeof(text));
        if (n) ws_frame(c, 2, text, n);
        else if (chaos_web_terminal_state() == CHAOS_WEB_TERMINAL_EXITED
                 || chaos_web_terminal_state() == CHAOS_WEB_TERMINAL_FAILED)
            ws_close(c, 1000);
    }
}
/* 把当前响应推出去。一次 poll 最多发 4 块，每块 <=1024 字节：
 * 手环的 TCP 发送窗口很小，一次塞太多只会拿到 -11(EAGAIN)。
 * 先发头（hs 字节），再发体（length 字节），body 可能指向 c->out / c->header / app。
 * 发送完成后的两种收尾：
 *   - WebSocket：不关连接，清空长度回到 phase 2 继续收帧；
 *   - HTTP：主动 FIN；若是终端会话则顺手停掉子进程，重启请求则排 1.5s 后重启。 */
static void send_pending(struct client *c)
{
    unsigned budget = 4;
    while (budget-- && c->sent < c->hs + c->length) {
        const unsigned char *p;
        unsigned n;
        if (c->sent < c->hs) {
            p = (const unsigned char *)c->header + c->sent;
            n = c->hs - c->sent;
        } else {
            unsigned at = c->sent - c->hs;
            p = c->data + at;
            n = c->length - at;
        }
        if (n > 1024) n = 1024;
        int rc = pan_tcp_send(c->id, p, n);
        if (rc > 0) {
            c->sent += (unsigned)rc;
            continue;
        }
        if (rc != -11 && rc != 0) drop(c);
        return;
    }

    if (c->id && c->sent >= c->hs + c->length) {
        if (c->ws && !c->closing) {
            c->phase = 2;
            c->hs = c->length = c->sent = 0;
        } else {
            int rc = pan_tcp_finish(c->id);
            if (!rc) {
                if (shell == c) {
                    if (c->auth) chaos_web_terminal_stop();
                    shell = 0;
                }
                if (c->reboot_reply) {
                    reboot_pending = 2;
                    reboot_at = pan_ip_now() + 1500;
                }
                memset(c, 0, sizeof(*c));
                served++;
            } else if (rc != -11) drop(c);
        }
    }
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
/* 每轮由网络 worker 调一次，节奏见 btpan::net_poll_interval（20/100/500 ms）。
 *
 * 顺序很重要，别调整：
 *   1) 开关换代 / 关闭 / IP 未就绪 -> 先把所有连接和终端收干净再退出；
 *   2) 没凭据 -> 现读一次，读不到就记错误退出（宁可不开，也不空密码裸奔）；
 *   3) 重启倒计时到点 -> 等终端收尾，最多再等 5s，超时就放弃并记 -16；
 *   4) 终端会话空闲超时 -> 停掉子进程，别让它一直占着；
 *   5) 监听没起来 -> 起一次，失败就 2s 后重试（不退化成忙等）；
 *   6) 每条连接按 phase 推进：0 读请求 / 2 读帧；phase 1 或"有响应要发"就推发送；
 *   7) 对端 EOF 且没有待发内容 -> 收尸。 */
void chaos_httpd_poll(uint32_t now)
{
    unsigned epoch = __atomic_load_n(&control, __ATOMIC_ACQUIRE) & ~1u;
    if (epoch != seen_epoch) {
        stop_transport();
        wipe(credential, sizeof(credential));
        reboot_pending = 0;
        retry_at = 0;
        seen_epoch = epoch;
    }
    if (!load_enabled()) {
        stop_transport();
        wipe(credential, sizeof(credential));
        reboot_pending = 0;
        return;
    }
    if (!pan_ip_ready()) {
        stop_transport();
        wipe(credential, sizeof(credential));
        reboot_pending = 0;
        return;
    }
    if (!credential[0]) {
        int rc = load_credentials();
        if (rc) {
            last_error = rc;
            return;
        }
        last_error = 0;
    }

    if (reboot_pending == 2 && (int32_t)(now - reboot_at) >= 0) {
        int state = chaos_web_terminal_state();
        if (state != CHAOS_WEB_TERMINAL_STARTING && state != CHAOS_WEB_TERMINAL_RUNNING
            && state != CHAOS_WEB_TERMINAL_STOPPING) {
            last_error = chaos_web_reboot();
            reboot_pending = 0;
        } else if ((int32_t)(now - (reboot_at + 5000)) >= 0) {
            /* 终端 5 秒还没退干净就放弃重启，留个 -16 给页面看。 */
            last_error = -16;
            reboot_pending = 0;
        }
    }
    if (exec_session && !shell && (int32_t)(now - exec_idle_at) >= 0) {
        chaos_web_terminal_stop();
        exec_session = 0;
    }

    if (!listener && (!retry_pending || (int32_t)(now - retry_at) >= 0)) {
        int id = pan_tcp_listen(CHAOS_HTTPD_PORT);
        if (id > 0) {
            listener = id;
            last_error = 0;
            retry_pending = 0;
        } else {
            last_error = id;
            retry_at = now + 2000;
            retry_pending = 1;
        }
    }
    if (listener) {
        int id = pan_tcp_accept(listener);
        if (id > 0) {
            unsigned i = 0;
            while (i < CLIENTS && clients[i].id) i++;
            if (i == CLIENTS) {
                /* 三个槽都满了：直接关掉新来的，计数给页面看。 */
                pan_tcp_close(id);
                refused++;
            } else {
                memset(&clients[i], 0, sizeof(clients[i]));
                clients[i].id = id;
                clients[i].deadline = now + 8000;   /* 8s 内必须把请求头发完 */
            }
        } else if (id == -9) {
            /* -9 = 监听 fd 失效，重开一次并退避。 */
            listener = 0;
            retry_at = now + 2000;
            retry_pending = 1;
        }
    }

    unsigned count = 0;
    for (unsigned i = 0; i < CLIENTS; i++) {
        struct client *c = &clients[i];
        if (!c->id) continue;
        int state = pan_tcp_state(c->id);
        if (state < 0 || state == PAN_TCP_ERROR) {
            failed++;
            drop(c);
            continue;
        }
        if ((int32_t)(now - c->deadline) >= 0) {
            failed++;
            drop(c);
            continue;
        }
        if (c->phase == 0) read_http(c);
        else if (c->phase == 2) read_ws(c);
        if (c->id && (c->phase == 1 || (c->phase == 2 && c->length))) send_pending(c);
        if (c->id && state == PAN_TCP_EOF && c->phase != 1 && !c->length) drop(c);
        if (c->id) count++;
    }
    active_clients = count;
}
/* 给 Rust 侧页面用的只读快照；字段顺序与 chaos_httpd_view 一致。 */
void chaos_httpd_snapshot(struct chaos_httpd_view *out)
{
    if (!out) return;
    out->enabled = load_enabled();
    out->listening = listener > 0;
    out->clients = active_clients;
    out->requests = served;
    out->failures = failed;
    out->refused = refused;
    out->last_error = last_error;
}
/* 手环页面上那一行摘要文字。四种状态按"为什么打不开"排：
 * 没开 -> 已停止；IP 没就绪 -> 等局域网；监听没起来 -> 缺密码/启动中；否则给 URL + 次数。 */
void chaos_httpd_status(char *out, unsigned cap)
{
    unsigned a = 0;
    if (!out || !cap) return;
    out[0] = 0;
    if (!load_enabled()) {
        put(out, 0, cap, "已停止");
        return;
    }
    struct pan_ip_info ip;
    pan_ip_info(&ip);
    if (!ip.ready) {
        put(out, 0, cap, "等待局域网连接");
        return;
    }
    if (!listener) {
        put(out, 0, cap, last_error == -13 ? "请先设置网页密码" : "正在启动");
        return;
    }
    a = put(out, a, cap, "http://");
    a = iptext(out, a, cap, ip.ip);
    a = put(out, a, cap, " · ");
    a = num(out, a, cap, served);
    put(out, a, cap, " 次请求");
}
#ifdef CHAOS_HTTPD_TEST
/* 宿主侧测试用：把整份状态清回"刚加载"的样子。只在 CHAOS_HTTPD_TEST 下编进来，
 * 真机固件里不存在这个符号。 */
void chaos_httpd_reset(void)
{
    memset(clients, 0, sizeof(clients));
    listener = 0;
    control = 0;
    served = failed = refused = 0;
    retry_at = reboot_at = exec_idle_at = 0;
    reboot_pending = exec_session = retry_pending = 0;
    shell = 0;
    app_size = app_running = 0;
    last_error = 0;
    seen_epoch = active_clients = 0;
    wipe(credential, sizeof(credential));
}
#endif

