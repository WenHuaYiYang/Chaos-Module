/* Freestanding <string.h> for thumbv8m bare target.
 * 实现体在 src/pan_libc.c。 */
#ifndef CHAOS_FS_STRING_H
#define CHAOS_FS_STRING_H
#include <stddef.h>
void *memcpy(void *, const void *, size_t);
void *memmove(void *, const void *, size_t);
void *memset(void *, int, size_t);
int memcmp(const void *, const void *, size_t);
size_t strlen(const char *);
int strcmp(const char *, const char *);
int strncmp(const char *, const char *, size_t);
char *strchr(const char *, int);
char *strstr(const char *, const char *);
int atoi(const char *);
#endif
