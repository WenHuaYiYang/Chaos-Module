/* No VFS in this probe: production transport/codec run as ARM, file engine is
 * independently exercised through the POSIX harness. */
#include "chaos_connect.h"
unsigned test_dispatches,test_resets,test_chunk;
void cc_files_reset(void){test_resets++;}
void cc_files_set_chunk(uint16_t n){test_chunk=n;}
void cc_files_dispatch(const struct cc_frame *f){test_dispatches++;(void)cc_reply(CC_RESPONSE,f->op,0,f->req,f->payload,f->len);}
void chaos_bridge_log(const char *s){(void)s;}
