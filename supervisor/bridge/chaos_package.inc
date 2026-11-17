/* Resource-only Android delivery. No Lua/module execution or caller-selected paths.
 * Every file is staged in a new slot; index.txt is the publication point. */
extern uint32_t chaos_connect_install_policy(void);
extern void chaos_connect_install_name(const uint8_t *,uint32_t);
static uint8_t pack_kind,pack_slot,pack_name_len;
static uint16_t pack_expected_files,pack_count,pack_files;
static char pack_name[13],pack_root[64],pack_stage[80],pack_target[80],pack_file[160];
static uint8_t pack_base_index[128],pack_index[128],pack_names[256][65];
static uint32_t pack_total,pack_done,pack_expected_crc,pack_crc,pack_started;
static uint32_t pack_file_size,pack_file_done,pack_file_crc,pack_file_expected_crc;
static int32_t pack_fd=-1;
static void ccp_path(char *out,const char *base,const char *leaf){
 unsigned i=0,j=0;while(base[i]){out[i]=base[i];i++;}out[i++]='/';while(leaf[j])out[i++]=leaf[j++];out[i]=0;
}
static int ccp_write_all(int fd,const uint8_t *p,uint32_t n){
 while(n){int rc=cc_io_write(fd,p,n);if(rc<=0||(uint32_t)rc>n)return -1;p+=rc;n-=(uint32_t)rc;}return 0;
}
static void ccp_cleanup(void){
 if(pack_fd>=0){cc_io_close(pack_fd);pack_fd=-1;}
 if(pack_stage[0]){
  char path[160];for(unsigned i=0;i<pack_count;i++){ccp_path(path,pack_stage,(char *)pack_names[i]);cc_io_remove(path);}
  cc_io_rmdir(pack_stage);pack_stage[0]=0;
 }
}
static void ccp_reset(void){ccp_cleanup();pack_state=0;pack_token=0;pack_count=pack_files=0;pack_total=pack_done=0;}
static void ccp_tick(uint32_t now){
 if(!pack_state||pack_state>=6)return;
 if(!android_client||!access_granted||!chaos_bridge_enabled()||chaos_connect_install_policy()==1){ccp_cleanup();pack_state=3;return;}
 if(pack_state==1){
  if((uint32_t)(now-pack_started)>=60000){pack_state=4;return;}
  uint32_t decision=chaos_connect_take_decision(pack_token);
  if(decision||chaos_connect_install_policy()==0)pack_state=decision==2?3:2;
 }
}
static void ccp_reply(uint8_t op,uint16_t req){uint8_t b[13];ccf_put_u32(b,pack_token);b[4]=pack_state;ccf_put_u32(b+5,pack_done);ccf_put_u32(b+9,pack_total);ccf_send(op,req,0,b,13);}
static int ccp_name_ok(const uint8_t *p,unsigned n,unsigned kind){
 if(!n||n>12||p[n-1]==' ')return 0;
 for(unsigned i=0;i<n;i++)if(p[i]<32||p[i]==127||(kind==2&&(p[i]<=32||p[i]>=127)))return 0;
 /* UTF-8 names are displayed unchanged; reject malformed sequences and bidi controls. */
 for(unsigned i=0;i<n;){unsigned c=p[i++],k=0;uint32_t v=c;
  if(c>=0xf0&&c<=0xf4){k=3;v=c&7;}else if(c>=0xe0&&c<=0xef){k=2;v=c&15;}else if(c>=0xc2&&c<=0xdf){k=1;v=c&31;}else if(c>=128)return 0;
  unsigned width=k;while(k--){if(i>=n||(p[i]&0xc0)!=0x80)return 0;v=(v<<6)|(p[i++]&63);}
  if(v<32||(v>=127&&v<=159))return 0;
  if((width==1&&v<128)||(width==2&&v<2048)||(width==3&&v<65536)||v>0x10ffff||(v>=0xd800&&v<=0xdfff)||(v>=0x202a&&v<=0x202e)||(v>=0x2066&&v<=0x2069))return 0;
 }return 1;
}
static int ccp_prepare(void){
 /* Create only internal fixed roots. Existing index must be exact and well formed. */
 (void)cc_io_mkdir("/data/chaos");
 const char *root=pack_kind==1?"/data/chaos/font":"/data/chaos/icons";
 unsigned l=0;while(root[l]){pack_root[l]=root[l];l++;}pack_root[l]=0;(void)cc_io_mkdir(pack_root);
 char path[160];ccp_path(path,pack_root,"index.txt");
 for(unsigned i=0;i<128;i++)pack_index[i]=' ';
 int fd=cc_io_open(path,CC_IO_READ);if(fd>=0){int64_t size=cc_io_size(fd);int n=cc_io_read(fd,pack_index,128);cc_io_close(fd);if(size!=128||n!=128)return CCF_ST_IO;}
 else if(fd!=-2)return CCF_ST_IO;
 ccf_copy_bytes(pack_base_index,pack_index,128);
 unsigned used=0,row=8;
 for(unsigned i=0;i<8;i++){
  uint8_t *p=pack_index+i*16;if(p[0]==' '){for(unsigned j=0;j<16;j++)if(p[j]!=' ')return CCF_ST_IO;if(row==8)row=i;continue;}
  unsigned id=(p[0]-'0')*10+p[1]-'0';if(id<1||id>8||p[2]!=' '||p[15]!='\n'||(used&(1u<<id)))return CCF_ST_IO;used|=1u<<id;
  unsigned n=12;while(n&&p[3+n-1]==' ')n--;
  if(n==pack_name_len){unsigned same=1;for(unsigned j=0;j<n;j++)if(p[3+j]!=(uint8_t)pack_name[j])same=0;if(same)return CCF_ST_EXIST;}
 }
 if(row==8)return CCF_ST_NOSPACE;
 struct cc_stat st;pack_slot=0;
 for(unsigned id=1;id<=8;id++){
  if(used&(1u<<id))continue;
  char leaf[12];if(pack_kind==1){leaf[0]='s';leaf[1]='t';leaf[2]='0'+id;ccf_copy_bytes((uint8_t *)leaf+3,(const uint8_t *)".ttf",5);}else{leaf[0]='0';leaf[1]='0'+id;leaf[2]=0;}
  ccp_path(pack_target,pack_root,leaf);if(cc_io_stat(pack_target,&st)==-2){pack_slot=id;break;}
 }
 if(!pack_slot)return CCF_ST_NOSPACE;
 /* The staging path contains a non-reused per-session token. Never clean an unrelated directory. */
 char leaf[24]=".ccnp-00000000";for(unsigned i=0;i<8;i++)leaf[6+i]="0123456789abcdef"[(pack_token>>(28-4*i))&15];
 ccp_path(pack_stage,pack_root,leaf);if(cc_io_mkdir(pack_stage)<0){pack_stage[0]=0;return CCF_ST_IO;}
 uint8_t *p=pack_index+row*16;p[0]='0';p[1]='0'+pack_slot;p[2]=' ';for(unsigned i=0;i<12;i++)p[3+i]=i<pack_name_len?(uint8_t)pack_name[i]:' ';p[15]='\n';
 return 0;
}
static uint32_t ccp_be32(const uint8_t *p){return (uint32_t)p[0]<<24|(uint32_t)p[1]<<16|(uint32_t)p[2]<<8|p[3];}
static int ccp_resource_ok(void){
 int fd=cc_io_open(pack_file,CC_IO_READ);if(fd<0)return 0;uint8_t h[28];int n=cc_io_read(fd,h,28),ok=0;
 if(pack_kind==1){
  if(n>=12&&(ccp_be32(h)==0x00010000u||ccp_be32(h)==0x4f54544fu||ccp_be32(h)==0x74727565u)){
   unsigned count=(unsigned)h[4]*256+h[5];ok=count>0&&count<=256&&12+count*16<=pack_file_size;
   for(unsigned i=0;ok&&i<count;i++){
    if(cc_io_seek(fd,12+i*16,0)<0||cc_io_read(fd,h,16)!=16){ok=0;break;}
    uint32_t off=ccp_be32(h+8),len=ccp_be32(h+12);if(off>pack_file_size||len>pack_file_size-off)ok=0;
   }
  }
 }else{
  const char *name=(char *)pack_names[pack_count-1];unsigned len=(unsigned)ccf_strlen(name);
  if(!ccf_strcmp(name,"animations.bin"))ok=pack_file_size==20&&n>=4&&h[0]=='C'&&h[1]=='I'&&h[2]=='A'&&h[3]=='1';
  else if(len>=9&&!ccf_strcmp(name+len-9,"_anim.bin"))ok=n>=4&&h[0]=='D'&&h[1]=='G'&&h[2]=='F'&&h[3]=='1';
  else ok=n>=12&&h[0]==0x19;
 }
 cc_io_close(fd);return ok;
}
static void ccp_dispatch(uint8_t op,uint16_t req,const uint8_t *p,uint32_t n){
 unsigned error=0;
 if(!android_client||!access_granted||!chaos_bridge_enabled()){error=CCF_ST_ACCESS;goto fail;}
 if(op==0x60){
  if(auth_state==1||connect_auth==1){error=CCF_ST_BUSY;goto fail;}
  if(pack_state&&pack_state<6&&pack_state!=3&&pack_state!=4){error=CCF_ST_BUSY;goto fail;}
  if(n<12||p[0]<1||p[0]>2||n!=12u+p[1]||!ccp_name_ok(p+2,p[1],p[0])){error=CCF_ST_BAD;goto fail;}
  unsigned count=ccf_get_u16(p+2+p[1]);uint32_t total=ccf_get_u32(p+4+p[1]);
  if(!count||count>256||(p[0]==1&&count!=1)||!total||total>32u*1024*1024){error=CCF_ST_TOO_LARGE;goto fail;}
  if(chaos_connect_install_policy()==1){error=CCF_ST_ACCESS;goto fail;}
  ccp_reset();pack_kind=p[0];pack_name_len=p[1];ccf_copy_bytes((uint8_t *)pack_name,p+2,pack_name_len);pack_name[pack_name_len]=0;
  pack_expected_files=count;pack_total=total;pack_expected_crc=ccf_get_u32(p+8+pack_name_len);pack_crc=cc_crc32(0xffffffffu,p,n-4);
  pack_token=next_token();pack_started=chaos_connect_now_ms();pack_state=chaos_connect_install_policy()==2?1:2;
  chaos_connect_install_name((const uint8_t *)pack_name,pack_name_len);publish_state();
  audit_text(" 安装投递包：");audit_text(pack_name);chaos_connect_log(audit_context);ccp_reply(op,req);return;
 }
 if(n<4||!pack_token||ccf_get_u32(p)!=pack_token){error=CCF_ST_SEQUENCE;goto fail;}
 if(op==0x66){if(n!=4){error=1;goto fail;}ccp_reset();publish_state();ccp_reply(op,req);return;}
 ccp_tick(chaos_connect_now_ms());
 if(op==0x61){if(n!=4){error=1;goto fail;}ccp_reply(op,req);return;}
 if(chaos_connect_install_policy()==1||pack_state<2||pack_state==3||pack_state==4||pack_state>=6){error=CCF_ST_ACCESS;goto fail;}
 if(op==0x62){
  if(pack_fd>=0||pack_files>=pack_expected_files||n<14){error=1;goto fail;}
  unsigned len=ccf_get_u16(p+4);if(!len||len>64||n!=14u+len){error=1;goto fail;}
  char name[65];ccf_copy_bytes((uint8_t *)name,p+6,len);name[len]=0;
  if(pack_kind==1){if(len!=8||ccf_strcmp(name,"font.ttf")){error=1;goto fail;}}
  else {
   if(len<5||ccf_strcmp(name+len-4,".bin")){error=1;goto fail;}
   for(unsigned i=0;i<len-4;i++){char c=name[i];if(!((c>='a'&&c<='z')||(c>='A'&&c<='Z')||(c>='0'&&c<='9')||c=='_')){error=1;goto fail;}}
  }
  for(unsigned i=0;i<pack_count;i++)if(!ccf_strcmp(name,(char *)pack_names[i])){error=4;goto fail;}
  pack_file_size=ccf_get_u32(p+6+len);pack_file_expected_crc=ccf_get_u32(p+10+len);
  if(!pack_file_size||pack_file_size>(pack_kind==1?0xffffffu:0x100000u)||pack_done+pack_file_size>pack_total){error=13;goto fail;}
  if(!pack_stage[0]){error=ccp_prepare();if(error)goto fail;}
  ccf_copy_bytes(pack_names[pack_count++],(uint8_t *)name,len+1);ccp_path(pack_file,pack_stage,name);
  pack_fd=cc_io_open(pack_file,CC_IO_WRITE|CC_IO_CREATE|CC_IO_TRUNC);if(pack_fd<0){error=9;goto fail;}
  pack_crc=cc_crc32(pack_crc,p+4,n-4);pack_file_done=0;pack_file_crc=0xffffffffu;pack_state=5;ccp_reply(op,req);return;
 }
 if(op==0x63){
  if(pack_fd<0||n<=8||n-8>g_chunk||ccf_get_u32(p+4)!=pack_file_done||n-8>pack_file_size-pack_file_done){error=11;goto fail;}
  if(ccp_write_all(pack_fd,p+8,n-8)){error=9;goto fail;}
  pack_crc=cc_crc32(pack_crc,p+8,n-8);pack_file_crc=cc_crc32(pack_file_crc,p+8,n-8);pack_file_done+=n-8;pack_done+=n-8;
  ccp_reply(op,req);return;
 }
 if(op==0x64){
  if(n!=4||pack_fd<0||pack_file_done!=pack_file_size){error=11;goto fail;}
  if((pack_file_crc^0xffffffffu)!=pack_file_expected_crc){error=10;goto fail;}
  int synced=cc_io_sync(pack_fd),closed=cc_io_close(pack_fd);pack_fd=-1;if(synced<0||closed<0){error=9;goto fail;}
  if(!ccp_resource_ok()){error=CCF_ST_BAD;goto fail;}
  pack_files++;ccp_reply(op,req);return;
 }
 if(op==0x65){
  if(n!=4||pack_fd>=0||pack_files!=pack_expected_files||pack_done!=pack_total){error=11;goto fail;}
  if((pack_crc^0xffffffffu)!=pack_expected_crc){error=10;goto fail;}
  char index[100],temp[100];ccp_path(index,pack_root,"index.txt");ccp_path(temp,pack_stage,"index.tmp");
  int checkfd=cc_io_open(index,CC_IO_READ);uint8_t current[128];
  if(checkfd>=0){int got=cc_io_read(checkfd,current,128);cc_io_close(checkfd);if(got!=128){error=9;goto fail;}}
  else if(checkfd==-2){for(unsigned i=0;i<128;i++)current[i]=' ';}else{error=9;goto fail;}
  for(unsigned i=0;i<128;i++)if(current[i]!=pack_base_index[i]){error=8;goto fail;}
  int fd=cc_io_open(temp,CC_IO_WRITE|CC_IO_CREATE|CC_IO_TRUNC);if(fd<0){error=9;goto fail;}
  int ok=ccp_write_all(fd,pack_index,128)==0;int sync=cc_io_sync(fd),close=cc_io_close(fd);if(!ok||sync<0||close<0){cc_io_remove(temp);error=9;goto fail;}
  /* Index is still unpublished while moving a complete payload into a new slot. */
  if(pack_kind==1){if(cc_io_rename(pack_file,pack_target)<0){cc_io_remove(temp);error=9;goto fail;}}
  else {
   cc_io_remove(temp); /* place the index beside staging so directory rename stays independent */
   ccp_path(temp,pack_root,".ccnp-index.tmp");fd=cc_io_open(temp,CC_IO_WRITE|CC_IO_CREATE|CC_IO_TRUNC);
   if(fd<0){error=9;goto fail;}ok=ccp_write_all(fd,pack_index,128)==0;sync=cc_io_sync(fd);close=cc_io_close(fd);
   if(!ok||sync<0||close<0||cc_io_rename(pack_stage,pack_target)<0){cc_io_remove(temp);error=9;goto fail;}
  }
  if(cc_io_rename(temp,index)<0){
   cc_io_remove(temp);
   if(pack_kind==1)cc_io_remove(pack_target);else{char path[160];for(unsigned i=0;i<pack_count;i++){ccp_path(path,pack_target,(char *)pack_names[i]);cc_io_remove(path);}cc_io_rmdir(pack_target);}
   error=9;goto fail;
  }
  if(pack_kind==1)cc_io_rmdir(pack_stage);pack_stage[0]=0;pack_state=6;
  audit_text(" 投递包已安装：");audit_text(pack_name);chaos_connect_log(audit_context);ccp_reply(op,req);return;
 }
 error=12;
fail:
 if(op!=0x60&&op!=0x61&&op!=0x66&&pack_token&&n>=4&&ccf_get_u32(p)==pack_token){ccp_cleanup();pack_state=7;}
 ccf_send(op,req,error,0,0);publish_state();
}
