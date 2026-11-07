#ifndef SYSPATCH_PAN_IP_H
#define SYSPATCH_PAN_IP_H
#include <stdint.h>
/* All calls except status run on the PAN worker. Handles are module TCP
 * connections, never operating-system fds. No kernel memory hooks. */
enum {PAN_TCP_RESOLVING=1,PAN_TCP_CONNECTING,PAN_TCP_OPEN,PAN_TCP_EOF,PAN_TCP_ERROR};
void pan_ip_poll(int,const uint8_t[6],const uint8_t[6],uint32_t);
int pan_ip_ready(void);
int pan_ip_faulted(void); /* atomic UI-safe fatal-worker status; reboot required */
void pan_ip_status(char *,unsigned);
struct pan_ip_info {uint8_t ip[4],mask[4],gateway[4],dns[4],mac[6];uint16_t mtu;unsigned ready;};
void pan_ip_info(struct pan_ip_info *); /* locked published snapshot; UI-safe */
uint32_t pan_ip_now(void);
int pan_tcp_open(const char *host,uint16_t port);
int pan_tcp_open_ip(const uint8_t address[4],uint16_t port); /* no DNS */
int pan_tcp_state(int);
int pan_tcp_error(int);
int pan_tcp_send(int,const void *,unsigned);
int pan_tcp_receive(int,void *,unsigned);
void pan_tcp_close(int);
int pan_tcp_finish(int); /* graceful FIN after queued bytes; retry -11 */
int pan_tcp_shutdown_write(int); /* half-close TX; retain RX for proxy streams */
/* One datagram send on the module's netif (IPv4 only). Used by the LAN
 * discovery announcer; no sockets and no listening UDP owner. */
int pan_udp_send(const uint8_t address[4],uint16_t port,const void *data,unsigned length);
/* Listeners and accepted connections belong only to this module's netif.
 * Up to 4 independent listen ports (ids 1..4) are supported so resident
 * services (SSHD 22, built-in HTTP 80, later protocol servers) coexist.
 * Poll accept returns -11 if empty, a positive connection handle otherwise. */
int pan_tcp_listen(uint16_t port);
int pan_tcp_accept(int listener);
void pan_tcp_unlisten(int listener);
int pan_tcp_peer(int connection,uint8_t address[4],uint16_t *port);
#endif
