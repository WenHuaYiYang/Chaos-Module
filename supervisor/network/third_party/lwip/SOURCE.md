# lwIP 2.2.1

Upstream: https://github.com/lwip-tcpip/lwip
Tag: STABLE-2_2_1_RELEASE
Commit: 77dcd25a72509eb83f72b033d219b1d40cd8eb95
License: BSD-3-Clause; see COPYING and individual source headers.
Verification: src/ and COPYING compare byte-for-byte with the pinned upstream
Git checkout (`diff -qr` and `diff -u`, 2026-11-07).

The upstream src/ tree is vendored without platform changes. Only the sources
explicitly listed in tools/build_network.py are compiled. Chaos-specific arch
headers and lwipopts.h are in supervisor/network/ports/lwip/; integration is in
supervisor/network/src/pan_ip.c and pan_ethernet.c. This is a module-owned stack,
not a port of the firmware's global network-device ABI.
