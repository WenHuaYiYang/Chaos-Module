#ifndef SYSPATCH_PAN_RADIO_H
#define SYSPATCH_PAN_RADIO_H
#include <stdint.h>
void pan_radio_poll(uint32_t);
int pan_radio_addresses(uint8_t[6],uint8_t[6]);
int pan_radio_error(void);
#endif
