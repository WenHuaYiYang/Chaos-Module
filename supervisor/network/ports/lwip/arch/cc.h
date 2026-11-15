#ifndef SYSPATCH_LWIP_CC_H
#define SYSPATCH_LWIP_CC_H
#include <stdint.h>
#define LWIP_NO_INTTYPES_H 1
#define LWIP_NO_CTYPE_H 1
#define LWIP_PLATFORM_DIAG(x) ((void)0)
void pan_lwip_assert(const char *);
uint32_t pan_lwip_rand(void);
#define LWIP_PLATFORM_ASSERT(x) pan_lwip_assert(x)
#define LWIP_RAND() pan_lwip_rand()
#define BYTE_ORDER LITTLE_ENDIAN
#endif

