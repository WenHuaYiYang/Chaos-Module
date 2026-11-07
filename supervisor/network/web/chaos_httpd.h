#ifndef CHAOS_HTTPD_H
#define CHAOS_HTTPD_H
#include <stdint.h>
#define CHAOS_HTTPD_PORT 80u
/* enable/enabled/status/snapshot are UI-safe. poll has one owner: PAN worker.
 * Starting does not connect Bluetooth automatically. Default is disabled. */
void chaos_httpd_enable(int on);
int chaos_httpd_enabled(void);
void chaos_httpd_poll(uint32_t now_ms);
void chaos_httpd_status(char *out, unsigned capacity);
struct chaos_httpd_view {
    uint32_t enabled, listening, clients, requests, failures, refused;
    int32_t last_error;
};
void chaos_httpd_snapshot(struct chaos_httpd_view *out);
#ifdef CHAOS_HTTPD_TEST
void chaos_httpd_reset(void);
#endif
#endif
