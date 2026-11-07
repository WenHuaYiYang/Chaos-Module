#ifndef CHAOS_WEB_PLATFORM_H
#define CHAOS_WEB_PLATFORM_H
#include <stdint.h>

/* C/Rust boundary for p67 3.101.043. No firmware address belongs in HTTP.
 * Every call below runs only on the PAN network worker, never a BT callback.
 * Rust must use repr(C) and the exact array sizes / u32 fields below. */
enum { CHAOS_WEB_HAVE_UPTIME = 1u, CHAOS_WEB_HAVE_HEAP = 2u,
       CHAOS_WEB_HAVE_MEMINFO = 4u, CHAOS_WEB_CAN_REBOOT = 8u };
struct chaos_web_sysinfo {
    char model[48], firmware[48], build[80], meminfo[512];
    uint32_t valid, uptime_seconds, heap_total, heap_free, heap_largest;
};
void chaos_web_sysinfo_read(struct chaos_web_sysinfo *out);
int chaos_net_set_credentials(const char *username, const char *password);

/* Username/password come from the watch page, never an HTTP request. The
 * caller supplies 32/65-byte buffers; return 0 only with NUL-terminated ASCII
 * strings (username cannot contain colon). A missing password returns -13.
 * UI and worker must use a synchronized snapshot of the same credentials.
 * The watch accepts passwords of 8..31 bytes. Changing a password must stop
 * and re-enable HTTP so existing authenticated sessions cannot survive it. */
int chaos_web_credentials(char *username, unsigned user_cap,
                          char *password, unsigned password_cap);
enum { CHAOS_WEB_TERMINAL_IDLE = 0, CHAOS_WEB_TERMINAL_STARTING = 1,
       CHAOS_WEB_TERMINAL_RUNNING = 2, CHAOS_WEB_TERMINAL_EXITED = 3,
       CHAOS_WEB_TERMINAL_FAILED = 4, CHAOS_WEB_TERMINAL_STOPPING = 5 };
/* These own an independent PTY/NSH, never the local terminal queues. send()
 * commits the complete block or nothing: 0 success, negative errno failure.
 * stop() is asynchronous; STARTING/RUNNING/STOPPING remain busy until reaped. */
int chaos_web_terminal_start(void);
int chaos_web_terminal_send(const char *, unsigned);
unsigned chaos_web_terminal_receive(char *, unsigned);
void chaos_web_terminal_stop(void);
int chaos_web_terminal_state(void);
int chaos_web_terminal_error(void);
/* Called only after an authenticated explicit reboot request was answered.
 * Advertise CAN_REBOOT only if this operation is implemented and audited. */
int chaos_web_reboot(void);
#endif
