#!/usr/bin/env python3
"""探针一键跑：确保 guest healthy -> 上传 -> insmod -> 读结果通道/marker/寄存器。

用法：
  python work/probe_p65/run.py goto2                 # 编 goto2 并跑
  python work/probe_p65/run.py goto2 --keep          # 不重启 guest（默认会先确保 ap> 就绪）
  python work/probe_p65/run.py --only-regs           # 只读当前寄存器
口径（本轮踩出来的）：
  * 文件接口与 NSH 互斥，上传要重试；fs-busy 时等 5~8 秒再来。
  * 探针崩了 guest 会自己复位；复位可能停在早启动，必须 kill qemu + start。
  * 判"崩没崩"看日志里命令后有没有 `ap>` 提示符，不看 powerState。
"""
import json
import os
import re
import socket
import struct
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE)
from g import Guest, manager, INST  # noqa: E402
import qmpmem  # noqa: E402

MARK = 0x3C7FF000


def guest_ready(timeout=15):
    for _ in range(timeout):
        time.sleep(7)
        try:
            g = Guest()
            st = g.get('/api/machine')
            tail = open(g.log_path(), encoding='utf-8', errors='replace').read()[-150:]
            if st.get('nativeResourcesReady') and 'ap>' in tail:
                return g
        except Exception:
            pass
    return None


def ensure_guest(timeout=90):
    """轮询到 NSH 能应答为止（rebbot 后 nativeResourcesReady 会卡住，别只看那个标志）。"""
    t0 = time.time()
    while time.time() - t0 < timeout:
        time.sleep(5)
        try:
            g = Guest()
        except Exception:
            continue
        try:
            if not g.nsh('date -u'):
                continue
            time.sleep(2)
            tail = open(g.log_path(), encoding='utf-8', errors='replace').read()[-200:]
            if 'ap>' in tail:
                return g
        except Exception:
            pass
    return None


def restart():
    subprocess.run(["powershell", "-NoProfile", "-Command",
                    "Get-CimInstance Win32_Process -Filter \"Name='qemu-system-arm.exe'\" | "
                    "Where-Object { $_.CommandLine -like '*miband-best1502p*' } | "
                    "ForEach-Object { Stop-Process -Id $_.ProcessId -Force }"], capture_output=True)
    time.sleep(3)
    manager('/api/instances/start', {"instance": INST, "skipBinding": True})
    return guest_ready(15)


def screendump(tag):
    """QEMU 的 HMP screendump：直接落一张 guest 画面，路径不能带空格。"""
    host_port = qmpmem.port()
    host, p = host_port.rsplit(":", 1)
    out = os.path.join(os.environ.get('TEMP', 'C:/Users/YiYang/AppData/Local/Temp'),
                       'mb_%s.ppm' % tag)
    s = socket.create_connection((host, int(p)), timeout=15)
    f = s.makefile("rwb")
    f.readline()
    f.write((json.dumps({"execute": "qmp_capabilities"}) + "\n").encode())
    f.flush()
    f.readline()
    f.write((json.dumps({"execute": "human-monitor-command",
                         "arguments": {"command-line": "screendump " + out.replace('\\', '/')}}) + "\n").encode())
    f.flush()
    r = f.readline()
    f.close()
    s.close()
    try:
        from PIL import Image
        png = os.path.join(ROOT, 'build', 'shot_%s.png' % tag)
        Image.open(out).save(png)
        return png
    except Exception as e:
        print('截图失败:', r[:120], e)
        return None


def upload(g, path, name):
    for _ in range(5):
        time.sleep(6)
        try:
            r = g.put(path, name)
            if r[0] == 'complete':
                return r
            print('  put ->', r[0])
        except Exception as e:
            print('  put err', e)
    return None


def regs():
    out = subprocess.run(["powershell", "-NoProfile", "-Command",
                          "(Get-CimInstance Win32_Process -Filter \"Name='qemu-system-arm.exe'\").CommandLine"],
                         capture_output=True, timeout=30).stdout.decode('utf-8', 'replace')
    ports = [l.split("-qmp tcp:")[1].split(",")[0] for l in out.splitlines()
             if "miband-best1502p" in l and "-qmp tcp:" in l]
    if not ports:
        print('找不到 QMP 端口')
        return
    host, p = ports[0].rsplit(":", 1)
    s = socket.create_connection((host, int(p)), timeout=15)
    f = s.makefile("rwb")
    f.readline()
    f.write((json.dumps({"execute": "qmp_capabilities"}) + "\n").encode()); f.flush(); f.readline()
    for cmd in ("info registers", "info status"):
        f.write((json.dumps({"execute": "human-monitor-command",
                             "arguments": {"command-line": cmd}}) + "\n").encode()); f.flush()
        print(f.readline().decode('utf-8', 'replace').strip()[:400])
    f.close(); s.close()


def main():
    args = sys.argv[1:]
    if '--only-regs' in args:
        regs()
        return
    feat = args[0]
    keep = '--keep' in args
    g = guest_ready(2)
    if g is None:
        if keep:
            print('guest 不健康，按 --keep 不重启')
            regs()
            return
        g = restart()
    print('guest ready:', g is not None)
    if g is None:
        return
    ko = os.path.join(HERE, 'probe_p65.ko')
    subprocess.run(['bash', os.path.join(HERE, 'build.sh'), feat], check=True, stdout=subprocess.DEVNULL)
    subprocess.run([sys.executable, os.path.join(ROOT, 'scripts', 'ko_entry_patch.py'), ko],
                   check=True, stdout=subprocess.DEVNULL)
    # 模块名必须每次唯一：NuttX 对同名 insmod 直接 EEXIST(17)，读到的会是上一版的内存。
    uniq = 'p%s_%d' % (feat.replace('-', '')[:8], int(time.time()) % 100000)
    name = uniq + '.ko'
    if upload(g, ko, name) is None:
        print('上传失败')
        return
    time.sleep(5)
    print('insmod:', g.nsh('insmod /data/%s %s' % (name, uniq)))
    time.sleep(5)
    log = open(g.log_path(), encoding='utf-8', errors='replace').read()
    plain = re.sub(r'\[[0-9;]*[A-Za-z]', '', log)
    alive = plain.rstrip().endswith('ap>')
    print('存活:', alive, '| 末尾:', repr(log[-60:]))
    mk = qmpmem.read(MARK, 8)
    if len(mk) == 8:
        print('marker:', hex(struct.unpack_from('<I', mk, 0)[0]), hex(struct.unpack_from('<I', mk, 4)[0]))
    # 结果通道
    g.nsh('lsmod'); time.sleep(4)
    log = open(g.log_path(), encoding='utf-8', errors='replace').read()
    m = re.findall(r'%s\s+0x([0-9a-f]+).*?0x([0-9a-f]+)\s+(\d+)\s+0x([0-9a-f]+)\s+(\d+)' % uniq, log)
    if m:
        base = int(m[-1][3], 16)
        size = int(m[-1][4])
        d = qmpmem.read(base, max(size, 32))
        # PROBE_RET 不一定在 .data 开头（模块里还有别的静态），按魔数找
        magic = struct.pack('<I', 0x52455431)
        i = d.find(magic)
        if i >= 0:
            n = min(96, len(d) - i)
            print('PROBE_RET', [hex(struct.unpack_from('<I', d, i + k)[0]) for k in range(0, n - 3, 4)])
        else:
            print('没找到结果通道魔数；模块 DATA 前 32 字节：',
                  [hex(struct.unpack_from('<I', d, k)[0]) for k in range(0, min(len(d), 32) - 3, 4)])
    else:
        print('lsmod 里没有本模块（可能没装上）')
    if alive:
        png = screendump(feat)
        if png:
            print('截图:', png)
    if not alive:
        regs()


if __name__ == '__main__':
    main()
