#!/usr/bin/env python3
"""Build the bounded PAN/lwIP/HTTP port and an exact linker response file."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
SOURCES = [
    "third_party/tinypan/src/tinypan_bnep.c",
    *[f"src/{name}.c" for name in (
        "bt_probe", "pan_radio", "pan_service", "network_service", "network_sdp",
        "nap_sdp", "pan_ethernet", "pan_ip", "pan_libc", "pan_tinypan_tap", "net_glue")],
    "web/chaos_httpd.c", "web/websocket_codec.c",
    *[f"third_party/lwip/src/core/{name}.c" for name in (
        "init", "def", "dns", "inet_chksum", "ip", "mem", "memp", "netif",
        "pbuf", "stats", "sys", "tcp", "tcp_in", "tcp_out", "timeouts", "udp")],
    *[f"third_party/lwip/src/core/ipv4/{name}.c" for name in ("acd", "dhcp", "etharp", "icmp", "ip4", "ip4_addr")],
    "third_party/lwip/src/netif/ethernet.c",
]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=ROOT / "supervisor/network")
    args = parser.parse_args()
    source = args.source.resolve()
    output = source / "build"
    output.mkdir(parents=True, exist_ok=True)
    subprocess.run([sys.executable, str(ROOT / "tools/embed_web.py"),
                    "--web-dir", str(source / "web")], check=True)
    flags = ["--target=thumbv8m.main-none-eabi", "-mcpu=cortex-m33", "-mthumb",
             "-mfloat-abi=soft", "-ffreestanding", "-fno-builtin", "-fno-common",
             "-fno-stack-protector", "-fno-unwind-tables", "-fno-asynchronous-unwind-tables",
             "-ffunction-sections", "-fdata-sections", "-Os", "-Wall", "-Wextra"]
    for include in ("include", "web", "ports/libc", "ports/lwip", "third_party/lwip/src/include"):
        flags.extend(["-I", str(source / include)])
    flags.extend(["-include", str(source / "include/tinypan_port.h"),
                  "-DCHAOS_BUILD_ID=" + json.dumps("Chaos " + os.environ.get("CHAOS_BUILD_TIME", "local build"))])
    objects = []
    for item in SOURCES:
        obj = output / (item.replace("/", "_")[:-2] + ".o")
        subprocess.run([os.environ.get("CC", "clang"), *flags, "-c", str(source / item), "-o", str(obj)], check=True)
        objects.append(obj)
    # lld response syntax preserves spaces. Never glob stale objects into the link.
    (output / "objects.rsp").write_text("\n".join(json.dumps(str(obj)) for obj in objects) + "\n")
    print(f"network C OK: {len(objects)} objects", flush=True)

if __name__ == "__main__":
    main()
