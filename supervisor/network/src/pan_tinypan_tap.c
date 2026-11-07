/* Link-time callback forwarding: preserve the frozen connection callback and
 * copy Ethernet RX into the module-owned stack's worker queue. */
#include "pan_ethernet.h"
static bnep_frame_recv_callback_t original;
static void *original_user;
void __real_bnep_register_frame_callback(bnep_frame_recv_callback_t,void *);
static void received(const bnep_ethernet_frame_t *frame,void *unused)
{
    (void)unused;
    pan_eth_receive(frame,0);
    if(original)original(frame,original_user);
}
void __wrap_bnep_register_frame_callback(bnep_frame_recv_callback_t callback,void *user)
{
    original=callback;original_user=user;
    __real_bnep_register_frame_callback(received,0);
}
