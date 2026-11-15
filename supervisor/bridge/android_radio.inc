/* Exact p67 / 3.101.043 standard SPP carrier. The private UUID is
 * 43434e31-0001-4a10-8000-4348414f5301. Android uses secure public RFCOMM.
 * No UI/VFS in adapter callbacks; all pipe I/O runs on the BT loop.
 * See docs/CHAOSCONNECT-ANDROID.md for firmware evidence and wire framing. */
#define RADIO_SCN 19u
static void *radio_adapter,*radio_app,*radio_pipe;
static uint32_t radio_callbacks[4],radio_gap_callbacks[17];
static volatile uint32_t radio_ready,radio_generation=1,radio_pair_request,radio_pair_until,radio_close_request;
static volatile uint32_t radio_path_pending,radio_bond_request,radio_bond_done,radio_bonded;
static uint16_t radio_port;
static unsigned radio_has_peer;
static volatile unsigned radio_disconnect_pending;
static uint8_t radio_disconnect_peer[6];
static uint16_t radio_disconnect_port;
static uint8_t radio_peer[6],radio_pair_peer[6];
static char radio_path[108];
static volatile int32_t radio_error;
static volatile uint32_t radio_error_stage;
static void radio_fail(uint32_t stage,int32_t error){radio_error_stage=stage;radio_error=error;}
static uint32_t radio_gap_registered;
static volatile unsigned radio_peer_count,radio_scan_request,radio_select_request;
static struct radio_device {uint8_t address[6];char name[32];} radio_devices[6];
static uint8_t radio_selected[6];
static volatile unsigned radio_outgoing;
static volatile uint32_t radio_bond_state,radio_bond_transport,radio_bond_reason,radio_display_kind=0xffffffffu;
static int32_t radio_acl_result,radio_bond_result;
static uint32_t radio_bond_attempts,radio_bond_retry,radio_visibility_check,radio_bond_query;
#ifndef CHAOS_CONNECT_RADIO_HOST
static unsigned radio_system_pair_gate(void){return *(volatile uint8_t *)(uintptr_t)0x201264a4u;}
static void radio_set_system_pair_gate(unsigned value){*(volatile uint8_t *)(uintptr_t)0x201264a4u=(uint8_t)value;}
#endif
static unsigned radio_gate_owned,radio_gate_saved;
static void radio_open_pair_gate(void){uint32_t mask=lock();
    if(!radio_gate_owned){radio_gate_saved=radio_system_pair_gate();radio_gate_owned=radio_gate_saved==1;}
    if(radio_gate_owned)radio_set_system_pair_gate(0);unlock(mask);
}
static void radio_restore_pair_gate(void){uint32_t mask=lock();
    if(radio_gate_owned&&radio_system_pair_gate()==0)radio_set_system_pair_gate(radio_gate_saved);
    radio_gate_owned=0;unlock(mask);
}
static volatile unsigned radio_server;
static unsigned radio_write_busy,radio_input_used,radio_input_need,radio_mode_saved;
static uint8_t radio_old_mode[2];
static uint8_t radio_input[CC_MAX_WIRE+4],radio_output[CC_MAX_WIRE+4];
extern uint32_t chaos_connect_now_ms(void);
static int radio_exchange(uint32_t command,const void *data,unsigned n,void *out,unsigned len){
    uint32_t message[177];for(unsigned i=0;i<177;++i)((volatile uint32_t *)message)[i]=0;if(!radio_adapter||n>648||len>648)return -22;
    if(n)copy((uint8_t *)message+60,data,n);
    int rc=FN(0x0c3ac47du,int (*)(void *,void *,uint32_t))(radio_adapter,message,command);
    if(rc)return -618;
    if(command==38u || command==92u){
        /* .043 get_scan_mode returns its ENUM in reply+12, not a status.
         * 1=connectable and 2=discoverable are both successful queries.
         * Only mode is observable; follow the stock setter's bondable=true. */
        unsigned mode=message[3]&255u;
        if(mode>2u)return -(int)mode;
        if(!out || len!=(command==38u?2u:1u))return -22;
        ((uint8_t *)out)[0]=(uint8_t)mode;if(command==38u)((uint8_t *)out)[1]=1;return 0;
    }
    if(message[3]&255u)return -(int)(message[3]&255u);
    if(out&&len)copy(out,(uint8_t *)message+60,len);return 0;
}
static void radio_gap_device(void *cookie,const uint8_t *data){
    (void)cookie;if(!data||!wanted||!started||!radio_pair_until)return;
    uint32_t mask=lock();unsigned i;
    for(i=0;i<radio_peer_count;++i){unsigned same=1;for(unsigned j=0;j<6;++j)if(radio_devices[i].address[j]!=data[j])same=0;if(same)break;}
    if(i<6){if(i==radio_peer_count)++radio_peer_count;copy(radio_devices[i].address,data,6);
        unsigned j=0;for(;j<31&&data[12+j];++j)radio_devices[i].name[j]=data[12+j];radio_devices[i].name[j]=0;}
    unlock(mask);
}
void chaos_connect_pair_scan(void){if(!radio_outgoing){radio_scan_request=1;radio_pair_request=1;}}
void chaos_connect_pair_device(uint32_t index){uint32_t mask=lock();if(index<radio_peer_count&&!radio_outgoing){copy(radio_selected,radio_devices[index].address,6);radio_outgoing=1;radio_select_request=1;}unlock(mask);}
uint32_t chaos_connect_pair_name(uint32_t index,char *out,uint32_t cap){
    if(!out||!cap)return 0;uint32_t mask=lock(),n=0;
    if(index<radio_peer_count){
        for(;n+1<cap&&radio_devices[index].name[n];++n)out[n]=radio_devices[index].name[n];
        if(!n&&cap>=18){static const char hex[]="0123456789ABCDEF";for(unsigned i=0;i<6;++i){if(i)out[n++]=':';uint8_t b=radio_devices[index].address[5-i];out[n++]=hex[b>>4];out[n++]=hex[b&15];}}
    }
    out[n]=0;unlock(mask);return n;
}
static void radio_gap_pair(void *cookie,const uint8_t *address){
    (void)cookie;if(!address||!wanted||!started)return;
    uint32_t mask=lock();
    unsigned match=1;if(radio_outgoing)for(unsigned i=0;i<6;++i)if(address[i]!=radio_selected[i])match=0;
    if(radio_pair_until && !radio_bond_request && match){copy(radio_pair_peer,address,6);radio_bond_request=1;}
    unlock(mask);
}
static volatile uint32_t radio_confirm_request,radio_passkey;
static void radio_gap_display(void *cookie,const uint8_t *address,int transport,int kind,uint32_t passkey){
    (void)cookie;if(!address||transport!=1||!wanted||!started)return;
    uint32_t mask=lock();unsigned match=1;for(unsigned i=0;i<6;++i)if(address[i]!=radio_pair_peer[i])match=0;
    if(radio_pair_until&&match){radio_passkey=passkey;radio_display_kind=(uint32_t)kind;
        /* .043: 0=CONFIRM, 1=ENTRY, 2=CONSENT, 3=NOTIFY, 4=PIN.
         * A notification must never generate an unsolicited confirmation. */
        if(kind==0||kind==2)radio_confirm_request=1;
    }unlock(mask);
}
static void radio_gap_bond(void *cookie,const uint8_t *address,int transport,int value,uint8_t reason){
    (void)cookie;if(!address||transport!=1||!wanted||!started)return;
    uint32_t mask=lock();unsigned match=1;
    for(unsigned i=0;i<6;++i)if(address[i]!=radio_pair_peer[i])match=0;
    if(radio_pair_until&&match){radio_bond_transport=(uint32_t)transport;radio_bond_state=(uint32_t)value;radio_bond_reason=reason;
        if(value==2&&reason==0)radio_bond_done=1;
    }
    unlock(mask);
}
static void radio_spp_state(void *cookie,const uint8_t *address,uint16_t scn,uint16_t port,int state){
    (void)cookie;(void)scn;
    if(state==0&&radio_has_peer&&port==radio_port&&address){unsigned match=1;for(unsigned i=0;i<6;++i)if(address[i]!=radio_peer[i])match=0;if(match)radio_close_request=1;}
}
static void radio_spp_proxy(void *cookie,const uint8_t *address,int status,uint16_t port,uint16_t mtu,const char *path){
    (void)cookie;(void)mtu;
    if(status||!address||!path||!wanted||!started)return;
    uint32_t mask=lock();
    if(radio_path_pending||radio_pipe){unlock(mask);return;}
    unsigned n=0;for(;n+1<sizeof(radio_path)&&path[n];++n)radio_path[n]=path[n];
    if(n==0||path[n]){radio_fail(12,-36);unlock(mask);return;}
    radio_path[n]=0;radio_has_peer=1;radio_port=port;copy(radio_peer,address,6);radio_path_pending=1;unlock(mask);
}
static void radio_close(void){
    uint32_t mask=lock();void *old=radio_pipe;radio_pipe=0;radio_ready=0;++radio_generation;
    radio_input_used=radio_input_need=0;
    if(radio_has_peer){radio_has_peer=0;
        copy(radio_disconnect_peer,radio_peer,6);radio_disconnect_port=radio_port;radio_disconnect_pending=1;
    }
    unlock(mask);
    if(old)FN(0x0c3aa051u,void (*)(void *))(old);
    /* Outstanding native writes keep radio_output until their completion. */
}
static void radio_read(void *pipe,const uint8_t *bytes,int32_t count){
    if(pipe!=radio_pipe)return;
    if(count<0){radio_close_request=1;return;}
    if(!bytes||!count)return;
    while(count>0){
        unsigned target=radio_input_need?radio_input_need:4u;
        if(radio_input_used>=target || target>sizeof(radio_input)){radio_fail(15,-90);radio_close_request=1;return;}
        unsigned take=target-radio_input_used;if(take>(unsigned)count)take=(unsigned)count;
        copy(radio_input+radio_input_used,bytes,take);radio_input_used+=take;bytes+=take;count-=(int32_t)take;
        if(radio_input_used==4 && !radio_input_need){
            uint32_t n=0;for(unsigned j=0;j<4;++j)n|=(uint32_t)radio_input[j]<<(8*j);
            if(n<CC_HEADER||n>CC_MAX_WIRE){radio_fail(15,-90);radio_close_request=1;return;}
            radio_input_need=n+4;
        }
        if(radio_input_need&&radio_input_used==radio_input_need){
            if(!chaos_bridge_android_receive(radio_generation,radio_input+4,radio_input_need-4)){
                radio_fail(15,-105);radio_close_request=1;return;
            }
            radio_input_used=radio_input_need=0;
        }
    }
}
static void radio_pipe_ready(void *pipe,int status,void *cookie){
    (void)cookie;
    if(pipe!=radio_pipe)return;
    if(status){radio_fail(12,status);radio_close_request=1;return;}
    int rc=FN(0x0c3a9a75u,int (*)(void *,uint16_t,void *,void *))(pipe,CC_MAX_WIRE,(void *)radio_read,0);
    if(rc){radio_fail(13,rc);radio_close_request=1;}else radio_ready=1;
}
static void radio_written(void *pipe,void *data,int status){
    (void)pipe;(void)data;radio_write_busy=0;
    if(status && pipe==radio_pipe){radio_fail(14,status);radio_close_request=1;}
}
static int cc_radio_send(uint32_t generation,const uint8_t *packet,unsigned n){
    if(!radio_ready||generation!=radio_generation||!radio_pipe)return -1;
    if(radio_write_busy)return -11;
    for(unsigned i=0;i<4;++i)radio_output[i]=(uint8_t)(n>>(8*i));
    copy(radio_output+4,packet,n);radio_write_busy=1;
    int rc=FN(0x0c3a9bfdu,int (*)(void *,void *,uint32_t,void *))(radio_pipe,radio_output,n+4,(void *)radio_written);
    if(rc){radio_write_busy=0;radio_fail(14,rc);}return rc;
}
static int cc_radio_live(void *generation){return radio_ready&&(uint32_t)(uintptr_t)generation==radio_generation;}
void chaos_connect_pair_start(void){uint32_t mask=lock();radio_pair_request=1;unlock(mask);}
uint32_t chaos_connect_radio_status(void){
    if(radio_error)return 0x80000000u|((radio_error_stage&255u)<<16)|((uint32_t)(radio_error<0?-radio_error:radio_error)&0xffffu);
    return (radio_server?1u:0u)|(radio_pair_until?2u:0u)|(radio_bonded?4u:0u)|(radio_ready?8u:0u)|(radio_outgoing?16u:0u);
}
void chaos_connect_pair_diagnostics(uint32_t *out){
    if(!out)return;uint32_t mask=lock();out[0]=(uint32_t)radio_acl_result;out[1]=(uint32_t)radio_bond_result;
    out[2]=radio_bond_transport;out[3]=radio_bond_state;out[4]=radio_bond_reason;out[5]=radio_display_kind;out[6]=radio_bond_attempts;out[7]=radio_passkey;unlock(mask);
}
static void radio_restore_mode(void){
    radio_restore_pair_gate();radio_pair_until=0;radio_outgoing=0;
    if(radio_mode_saved){int rc=radio_exchange(37,radio_old_mode,2,0,0);if(rc)radio_fail(7,rc);else radio_mode_saved=0;}
}
static void cc_radio_service(unsigned enabled){
    if(radio_disconnect_pending){
        uint8_t peer[6];uint16_t port;uint32_t mask=lock();copy(peer,radio_disconnect_peer,6);port=radio_disconnect_port;radio_disconnect_pending=0;unlock(mask);
        (void)FN(0x0c3b1625u,int (*)(void *,void *,const uint8_t *,uint16_t))(radio_adapter,radio_app,peer,port);
    }
    if(!enabled){
        radio_pair_request=0;radio_scan_request=radio_select_request=0;radio_path_pending=0;
        if(radio_pipe)radio_close_request=1;
        if(radio_mode_saved||radio_gate_owned)radio_restore_mode();
        if(radio_server){(void)FN(0x0c3b1589u,int (*)(void *,void *,uint16_t))(radio_adapter,radio_app,RADIO_SCN);radio_server=0;}
        return;
    }
    /* The existing persisted enable switch owns both carriers. Starting the
     * service never changes discoverability; paired phones may reconnect after
     * reboot without opening another pairing window. */
    if(!radio_pair_until&&radio_mode_saved&&!radio_pair_request)radio_restore_mode();
    if(!radio_adapter){
        radio_adapter=FN(0x0c3af1a9u,void *(*)(void))();
        if(!radio_adapter){radio_fail(1,-601);radio_pair_request=0;return;}
    }
    if(!radio_app){
        radio_callbacks[2]=(uint32_t)(uintptr_t)radio_spp_state;
        radio_callbacks[3]=(uint32_t)(uintptr_t)radio_spp_proxy;
        radio_app=FN(0x0c3b14d1u,void *(*)(void *,const uint32_t *))(radio_adapter,radio_callbacks);
        if(!radio_app){radio_fail(2,-602);radio_pair_request=0;return;}
    }
    if(!radio_gap_registered){
        radio_gap_callbacks[2]=(uint32_t)(uintptr_t)radio_gap_device;
        radio_gap_callbacks[5]=(uint32_t)(uintptr_t)radio_gap_pair;
        radio_gap_callbacks[6]=(uint32_t)(uintptr_t)radio_gap_display;
        radio_gap_callbacks[9]=(uint32_t)(uintptr_t)radio_gap_bond;
        radio_gap_registered=FN(0x0c3af3e9u,uint32_t (*)(void *,const uint32_t *))(radio_adapter,radio_gap_callbacks);
        if(!radio_gap_registered){
            radio_fail(3,-602);radio_pair_request=0;return;
        }
    }
    if(!radio_server){
        uint32_t uuid[5]={16,0x414f5301u,0x80004348u,0x00014a10u,0x43434e31u};
        int rc=FN(0x0c3b152du,int (*)(void *,void *,uint16_t,const void *,uint8_t))(radio_adapter,radio_app,RADIO_SCN,uuid,1);
        if(rc){radio_fail(4,rc);radio_pair_request=0;return;}radio_error=0;radio_server=1;
    }
    uint32_t now=chaos_connect_now_ms();
    if(radio_pair_request){
        radio_pair_request=0;radio_error=0;radio_bonded=0;
        if(!radio_mode_saved){int rc=radio_exchange(38,0,0,radio_old_mode,2);if(rc){radio_fail(5,rc);return;}radio_mode_saved=1;}
        radio_pair_until=now+60000u;if(!radio_pair_until)radio_pair_until=1;radio_open_pair_gate();
        uint8_t mode[2]={2,1};int rc=radio_exchange(37,mode,2,0,0);
        if(rc){radio_fail(6,rc);radio_restore_mode();return;}
        radio_visibility_check=now;
    }
    if(radio_pair_until && (int32_t)(now-radio_pair_until)>=0){
        if(radio_outgoing)radio_fail(10,radio_bond_result?radio_bond_result:-110);
        radio_restore_mode();
    }
    /* Stock pairing/link events may force scan mode back to CONNECTABLE.
     * Maintain only this bounded user window, never extend its deadline. */
    if(radio_pair_until && now-radio_visibility_check>=1000u){
        radio_visibility_check=now;uint8_t current[2];int rc=radio_exchange(38,0,0,current,2);
        if(!rc&&current[0]!=2){uint8_t discoverable[2]={2,1};rc=radio_exchange(37,discoverable,2,0,0);}
        if(rc)radio_fail(6,rc);radio_open_pair_gate();
    }
    if(radio_scan_request){radio_scan_request=0;radio_peer_count=0;uint32_t timeout=12;int rc=radio_exchange(30,&timeout,4,0,0);if(rc)radio_fail(8,rc);}
    if(radio_select_request){
        uint8_t selected[6];uint32_t mask=lock();copy(selected,radio_selected,6);radio_select_request=0;
        copy(radio_pair_peer,selected,6);radio_bond_state=radio_bond_reason=radio_bond_attempts=0;radio_bond_result=0;unlock(mask);
        if(radio_pair_until){
            (void)radio_exchange(31,0,0,0,0);
            radio_acl_result=radio_exchange(0x69,selected,6,0,0);
            radio_bond_retry=now+800u;radio_bond_query=now-500u; /* ACL connect is asynchronous. */
            if(radio_acl_result==-618){radio_fail(9,radio_acl_result);radio_outgoing=0;}
        }
    }
    if(radio_outgoing&&radio_pair_until){
        uint8_t query[7],bond=0;copy(query,radio_pair_peer,6);query[6]=1;
        /* GAP callbacks carry immediate changes. IPC is a bounded fallback,
         * not a synchronous round-trip on every 100 ms worker iteration. */
        if(now-radio_bond_query>=500u) {
            radio_bond_query=now;int rc=radio_exchange(92,query,7,&bond,1);
            if(!rc){radio_bond_transport=1;radio_bond_state=bond;
                if(bond==2&&radio_bond_reason==0&&radio_bond_attempts==0)radio_bond_done=1;
            }
        }
        if(!radio_bond_done && radio_bond_state!=1 && (int32_t)(now-radio_bond_retry)>=0){
            if(radio_bond_attempts>=6){radio_fail(10,radio_bond_result?radio_bond_result:-110);radio_outgoing=0;}
            else {++radio_bond_attempts;radio_bond_retry=now+3000u;
                radio_bond_result=radio_exchange(94,query,7,0,0);
                if(radio_bond_result==-618){radio_fail(10,radio_bond_result);radio_outgoing=0;}
            }
        }
    }
    if(radio_bond_request){
        radio_bond_request=0;
        if(radio_pair_until){uint8_t reply[7];copy(reply,radio_pair_peer,6);reply[6]=1;
            int rc=radio_exchange(0x61,reply,7,0,0);if(rc)radio_fail(11,rc);}
    }
    if(radio_confirm_request){radio_confirm_request=0;
        if(radio_pair_until){uint8_t confirm[8];copy(confirm,radio_pair_peer,6);confirm[6]=1;confirm[7]=1;
            int rc=radio_exchange(0x62,confirm,8,0,0);if(rc)radio_fail(11,rc);}
    }
    if(radio_bond_done){radio_bond_done=0;radio_bonded=1;radio_restore_mode();}
}
uint32_t chaos_connect_radio_poll(void){
    cc_radio_service(wanted);
    return radio_pair_until||radio_pair_request||radio_scan_request||radio_select_request||radio_disconnect_pending?100000u:1000000u;
}
static void cc_radio_tick(unsigned enabled){
    if(radio_close_request){radio_close_request=0;radio_close();}
    if(!enabled){radio_path_pending=0;if(radio_pipe)radio_close();return;}
    if(radio_path_pending){
        uint32_t mask=lock();char path[108];copy(path,radio_path,sizeof(path));unlock(mask);
        if(!radio_pipe){radio_error=0;++radio_generation;radio_input_used=radio_input_need=0;
            void *loop=*(void *volatile *)(uintptr_t)BT_LOOP;
            radio_pipe=FN(0x0c3a9cadu,void *(*)(void *,const char *,void *,void *))(loop,path,(void *)radio_pipe_ready,0);
            if(!radio_pipe){radio_fail(12,-12);radio_close_request=1;}
        }
        radio_path_pending=0;
    }
}

static void cc_radio_log_owner(void){
    static uint32_t previous;uint32_t current=chaos_connect_radio_status();
    if(current==previous)return;previous=current;
    extern void chaos_connect_log(const char *);
    if(current&0x80000000u){chaos_connect_log("普通蓝牙服务出现错误，请查看蓝牙配对页返回码");return;}
    if(current&8u)chaos_connect_log("Android普通蓝牙载波已连接，会话仍需手环授权");
    else if(current&2u)chaos_connect_log("手环已开启60秒普通蓝牙配对窗口");
    else if(current&4u)chaos_connect_log("普通蓝牙配对完成，等待Android ChaosConnect连接");
    else if(current&1u)chaos_connect_log("ChaosConnect专用蓝牙服务就绪");
}
