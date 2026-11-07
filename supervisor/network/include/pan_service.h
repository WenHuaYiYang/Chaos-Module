#ifndef SYSPATCH_PAN_SERVICE_H
#define SYSPATCH_PAN_SERVICE_H
#include <stdint.h>
int pan_service_start(void);
void pan_service_poll(void);
void pan_service_network_poll(uint32_t);
void pan_service_status(char *,unsigned);
#endif
