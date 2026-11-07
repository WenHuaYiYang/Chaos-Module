/* Chaos p67 3.101.043 移植版：无 pthread/clock 依赖。
 * 原参考实现（Vela_NativeApp-syspatch-band-11 ELF/src/pan_service.c）自建 worker
 * 线程；这里改为纯轮询入口，由 Rust 侧 chaos_net worker（nsh_spawn + nsh_usleep）
 * 每 20ms 调一次 chaos_pan_net_poll、每 250ms 调一次 chaos_pan_bt_poll。
 * 无 LVGL 对象、无页面生命周期依赖。 */
#include "bt_probe.h"
#include "pan_ip.h"
#include "pan_radio.h"
#include "network_service.h"
#include "../web/chaos_httpd.h"
#include <stdint.h>

/* BT 状态机低频拍：配对/扫描推进。 */
void chaos_pan_bt_poll(void)
{
    bt_probe_poll();
    network_service_poll();
}

/* 网络高频拍：lwip 超时、DHCP、TCP 收发、HTTP 服务。 */
void chaos_pan_net_poll(uint32_t now)
{
    struct bt_probe_view view;
    bt_probe_view(&view);
    /* Opening the web page alone must not perform a blocking BT IPC query. */
    if(view.phase == 4)pan_radio_poll(now);
    uint8_t local[6], remote[6];
    int addresses = pan_radio_addresses(local, remote);
    pan_ip_poll(view.phase == 4 && view.bnep == 3 && addresses, local, remote, now);
    chaos_httpd_poll(now);
}

void chaos_pan_status(char *out, unsigned capacity)
{
    pan_ip_status(out, capacity);
}
