#!/usr/bin/env python3
"""QMP 读 guest 内存（物理地址）。用法：
  python work/probe_p65/qmpmem.py 0x1C0309C4 64          # 十六进制 + ASCII
  python work/probe_p65/qmpmem.py 0x1C0309C4 64 --raw out.bin
Studio 的 /api/debug 只放行 info *，qemu 的 QMP 口接受 human-monitor-command，
所以这里走 xp/<n>xb。读的是**物理**地址，PSRAM 的低别名（0x1C..）能否直接读要试。
"""
import json
import socket
import subprocess
import sys


def _ps_lines():
    """进程命令行。不吃 text=True：PowerShell 输出是 GBK，Python 的 UTF-8 模式会解码炸掉。"""
    out = subprocess.run(["powershell", "-NoProfile", "-Command",
                          "(Get-CimInstance Win32_Process -Filter \"Name='qemu-system-arm.exe'\").CommandLine"],
                         capture_output=True, timeout=60).stdout
    return out.decode('utf-8', 'replace').splitlines()


_port_cache = []


def port():
    if _port_cache:
        return _port_cache[0]
    for line in _ps_lines():
        if "miband-best1502p" in line and "-qmp tcp:" in line:
            _port_cache.append(line.split("-qmp tcp:")[1].split(",")[0])
            return _port_cache[0]
    raise SystemExit("找不到 RW6 的 QMP 端口")


def read(addr, n, step=512):
    host, p = port().rsplit(":", 1)
    s = socket.create_connection((host, int(p)), timeout=10)
    f = s.makefile("rwb")
    f.readline()
    f.write((json.dumps({"execute": "qmp_capabilities"}) + "\n").encode()); f.flush(); f.readline()
    out = bytearray()
    for off in range(0, n, step):
        cnt = min(step, n - off)
        f.write((json.dumps({"execute": "human-monitor-command",
                             "arguments": {"command-line": "xp/%dxb 0x%x" % (cnt, addr + off)}}) + "\n").encode())
        f.flush()
        r = json.loads(f.readline())
        txt = r.get("return", "")
        for line in txt.splitlines():
            if ":" not in line:
                continue
            body = line.split(":", 1)[1]
            for tok in body.split():
                try:
                    out.append(int(tok, 16))
                except ValueError:
                    pass
    f.close(); s.close()
    return bytes(out[:n])


def main():
    addr = int(sys.argv[1], 16)
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 64
    data = read(addr, n)
    if "--raw" in sys.argv:
        path = sys.argv[sys.argv.index("--raw") + 1]
        open(path, "wb").write(data)
        print("%d 字节 -> %s" % (len(data), path))
        return
    for i in range(0, len(data), 16):
        chunk = data[i:i + 16]
        hx = " ".join("%02x" % b for b in chunk)
        asc = "".join(chr(b) if 32 <= b < 127 else "." for b in chunk)
        print("%08x  %-47s  %s" % (addr + i, hx, asc))


if __name__ == "__main__":
    main()
