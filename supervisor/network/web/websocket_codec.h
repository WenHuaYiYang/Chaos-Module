#ifndef SYSPATCH_WEBSOCKET_CODEC_H
#define SYSPATCH_WEBSOCKET_CODEC_H
#include <stdint.h>
void web_base64(const uint8_t *,unsigned,char *);
int web_accept_key(const char *,char[29]);
#endif

