#!/usr/bin/env python3
"""Build the bounded PAN/lwIP/HTTP port and an exact linker response file."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import shutil
from firmware_symbols import DEFAULT_TARGET, load_target

ROOT = Path(__file__).resolve().parents[1]

def compile_cached(command, output, dependencies, force=False):
    """Content/command cache; also verify the object itself before reusing it."""
    key=hashlib.sha256(json.dumps([command,dependencies],sort_keys=True).encode()).hexdigest()
    stamp=output.with_suffix('.cache.json')
    if not force and output.is_file() and stamp.is_file():
        try:
            saved=json.loads(stamp.read_text())
            if saved.get('key')==key and saved.get('object')==hashlib.sha256(output.read_bytes()).hexdigest(): return False
        except (OSError,ValueError): pass
    subprocess.run(command,check=True)
    value={'key':key,'object':hashlib.sha256(output.read_bytes()).hexdigest()}
    temporary=stamp.with_suffix('.tmp')
    temporary.write_text(json.dumps(value,sort_keys=True)+'\n')
    temporary.replace(stamp)
    return True
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
    target = load_target(ROOT, os.environ.get("CHAOS_FIRMWARE", DEFAULT_TARGET))
    output = source / "build" / target["id"]
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
                  "-include", str(target["symbol_dir"] / "firmware.h")])
    compiler=os.environ.get("CC","clang")
    resolved=Path(shutil.which(compiler) or compiler).resolve()
    digest=hashlib.sha256()
    digest.update(subprocess.check_output([compiler,'--version']))
    digest.update(str((resolved,resolved.stat().st_mtime_ns,resolved.stat().st_size)).encode())
    # Conservative closure: every source/header/web input, not timestamps.
    # A changed shared header or firmware ABI invalidates all network objects.
    for path in sorted(source.rglob('*')):
        if path.is_file() and 'build' not in path.relative_to(source).parts and '__pycache__' not in path.parts:
            digest.update(path.relative_to(source).as_posix().encode());digest.update(path.read_bytes())
    digest.update((target['symbol_dir']/'firmware.h').read_bytes())
    dependencies=digest.hexdigest()
    compiled=0
    objects = []
    for item in SOURCES:
        obj = output / (item.replace("/", "_")[:-2] + ".o")
        local_flags=list(flags)
        # Only this translation unit uses the build ID. Changing the timestamp
        # must not invalidate the other 36 unrelated translation units.
        if item=='src/net_glue.c':
            local_flags.append("-DCHAOS_BUILD_ID="+json.dumps("Chaos "+os.environ.get("CHAOS_BUILD_TIME","local build")))
        compiled+=compile_cached([compiler,*local_flags,"-c",str(source/item),"-o",str(obj)],obj,dependencies,
                                 os.environ.get('CHAOS_NO_C_CACHE')=='1')
        objects.append(obj)
    # lld response syntax preserves spaces. Never glob stale objects into the link.
    (output / "objects.rsp").write_text("\n".join(json.dumps(str(obj)) for obj in objects) + "\n")
    print(f"network C OK: {len(objects)} objects ({compiled} compiled, {len(objects)-compiled} verified cache hits)", flush=True)

if __name__ == "__main__":
    main()
