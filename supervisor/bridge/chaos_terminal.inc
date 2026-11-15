/* CCN1 owner serializes this lease. This backend owns an independent PTY;
 * a stale token must never write to, consume, or close a later shell. */
extern int32_t chaos_connect_terminal_start(void),chaos_connect_terminal_state(void),chaos_connect_terminal_error(void);
extern int32_t chaos_connect_terminal_send(const uint8_t *,uint32_t);
extern uint32_t chaos_connect_terminal_receive(uint8_t *,uint32_t);
extern void chaos_connect_terminal_stop(void);
static uint32_t terminal_lease,terminal_serial;
static uint8_t terminal_output[CC_CHUNK+9];
static void ccf_terminal_stop(void){
 if(terminal_lease){terminal_lease=0;chaos_connect_terminal_stop();chaos_connect_log("NSH终端会话结束，取消待执行输入并回收独立PTY");}
}
static void ccf_terminal_reply(uint8_t op,uint16_t req,uint32_t count){
 ccf_put_u32(terminal_output,terminal_lease);terminal_output[4]=(uint8_t)chaos_connect_terminal_state();
 ccf_put_u32(terminal_output+5,(uint32_t)chaos_connect_terminal_error());
 ccf_send(op,req,CCF_ST_OK,terminal_output,9+count);
}
static void ccf_terminal_dispatch(uint8_t op,uint16_t req,const uint8_t *p,uint32_t n){
 if(op==0x50){
  if(n){ccf_send(op,req,CCF_ST_BAD,0,0);return;}
  if(!terminal_lease){
   int rc=chaos_connect_terminal_start();
   if(rc){ccf_send(op,req,rc==-16?CCF_ST_BUSY:CCF_ST_IO,0,0);return;}
   ++terminal_serial;if(!terminal_serial)++terminal_serial;terminal_lease=terminal_serial;
  }
  ccf_terminal_reply(op,req,0);return;
 }
 if(n<4){ccf_send(op,req,CCF_ST_BAD,0,0);return;}
 if(!terminal_lease||ccf_get_u32(p)!=terminal_lease){ccf_send(op,req,CCF_ST_SEQUENCE,0,0);return;}
 if(op==0x51){
  if(n<=4||n-4>g_chunk){ccf_send(op,req,CCF_ST_BAD,0,0);return;}
  int rc=chaos_connect_terminal_send(p+4,n-4);
  ccf_send(op,req,rc==0?CCF_ST_OK:rc==-16?CCF_ST_BUSY:rc==-22||rc==-95?CCF_ST_BAD:CCF_ST_IO,0,0);
 }else if(op==0x52){
  if(n!=6){ccf_send(op,req,CCF_ST_BAD,0,0);return;}
  uint32_t capacity=ccf_get_u16(p+4);if(!capacity||capacity>g_chunk){ccf_send(op,req,CCF_ST_BAD,0,0);return;}
  uint32_t got=chaos_connect_terminal_receive(terminal_output+9,capacity);
  if(got>capacity){ccf_terminal_stop();ccf_send(op,req,CCF_ST_IO,0,0);return;}
  ccf_terminal_reply(op,req,got);
 }else{
  if(n!=4){ccf_send(op,req,CCF_ST_BAD,0,0);return;}
  ccf_terminal_stop();ccf_send(op,req,CCF_ST_OK,0,0);
 }
}
