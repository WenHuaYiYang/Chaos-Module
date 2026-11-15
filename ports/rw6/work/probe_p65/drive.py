#!/usr/bin/env python3
"""按固定顺序驱动 RW6 guest 完成一次"挂图标并点开自己的应用"自测。

顺序（每步都有程序化判据，不通过就停）：
  1. 冷启动，等 NSH；
  2. 按表冠直到应用列表真的建起来（判据：桌面记录首节点的图标对象 coords 非 0）；
  3. insmod add77（模块第 1 秒挂图标；此时桌面是活的，图标才会排版）；
  4. 读我们记录（id 0x7777）的图标 coords，必须在网格区内；
  5. QMP 鼠标短按图标中心；
  6. 读模块结果通道：on_create 标记 + 原生标题返回值。

用法：python work/probe_p65/drive.py [--no-restart]
"""
import json
import os
import re
import socket
import struct
import subprocess
import sys
import time
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE)
from g import Guest  # noqa: E402
import qmpmem  # noqa: E402
import run  # noqa: E402

W, H = 432, 514
DESK = 0x3C3B_F9DC        # {+0x00 计数, +0x04 首节点, +0x08 尾节点}


def crown(base, tok, inst, action):
    req = urllib.request.Request(
        base + '/instances/%s/api/crown' % inst,
        data=json.dumps({'action': action}).encode(),
        headers={'X-MiBand-Control': tok, 'Content-Type': 'application/json',
                 'Origin': base, 'Referer': base + '/instances/%s/' % inst},
        method='POST')
    try:
        return json.loads(urllib.request.urlopen(req, timeout=55).read()).get('ok')
    except Exception as e:
        return str(e)


def rec_icon(addr):
    t = struct.unpack('<8I', qmpmem.read(addr, 0x20))
    ic = t[4]
    if not (0x3C000000 <= ic < 0x3E000000):
        return None, t[0], (0, 0, 0, 0)
    iw = struct.unpack('<12I', qmpmem.read(ic, 0x30))
    return ic, t[0], (iw[7], iw[8], iw[9], iw[10])


def launcher_ready():
    d = qmpmem.read(DESK, 0x10)
    head = struct.unpack_from('<I', d, 4)[0]
    if not (0x3C000000 <= head < 0x3E000000):
        return False
    _, _, c = rec_icon(head)
    return c[2] > c[0] and c[3] > c[1]


def find_ours():
    d = qmpmem.read(DESK, 0x10)
    p = struct.unpack_from('<I', d, 8)[0]
    for _ in range(64):
        if not (0x3C000000 <= p < 0x3E000000):
            return None
        ic, rid, c = rec_icon(p)
        if rid == 0x7777:
            return ic, c
        p = struct.unpack('<8I', qmpmem.read(p, 0x20))[6]
    return None


def qmp():
    host, port = qmpmem.port().rsplit(':', 1)
    s = socket.create_connection((host, int(port)), timeout=20)
    f = s.makefile('rwb')
    f.readline()
    f.write((json.dumps({"execute": "qmp_capabilities"}) + "\n").encode())
    f.flush()
    f.readline()
    return s, f


def tap(f, x, y, hold=0.12):
    def ev(down=None):
        e = [{'type': 'abs', 'data': {'axis': 'x', 'value': int(x / (W - 1) * 0x7fff)}},
             {'type': 'abs', 'data': {'axis': 'y', 'value': int(y / (H - 1) * 0x7fff)}}]
        if down is not None:
            e.append({'type': 'btn', 'data': {'down': down, 'button': 'left'}})
        f.write((json.dumps({"execute": "input-send-event", "arguments": {"events": e}}) + "\n").encode())
        f.flush()
        f.readline()

    ev(True)
    time.sleep(hold)
    ev(False)
    return True


def main():
    no_restart = '--no-restart' in sys.argv
    if not no_restart:
        print('restart:', bool(run.restart()))
        time.sleep(10)
    g = Guest()
    for _ in range(15):
        try:
            if '\nap> ' in open(g.log_path(), encoding='utf-8', errors='replace').read()[-300:]:
                break
        except Exception:
            pass
        time.sleep(5)
    print('UI:', run.screendump('drive_ui'))

    base = g.base.rstrip('/')
    inst = g.get('/api/machine')['instanceId']
    tok = json.loads(urllib.request.urlopen(base + '/instances/%s/config.json' % inst,
                                            timeout=15).read())['controlToken']

    up = False
    for i in range(3):
        print('crown %d:' % i, crown(base, tok, inst, 'click'))
        time.sleep(1.8)
        up = launcher_ready()
        print('  launcher_ready =', up)
        if up:
            break
    if not up:
        print('应用列表没建起来，停')
        return 1

    ko = os.path.join(HERE, 'probe_p65.ko')
    subprocess.run(['bash', os.path.join(HERE, 'build.sh'), 'app2page'], check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    subprocess.run([sys.executable, os.path.join(ROOT, 'scripts', 'ko_entry_patch.py'), ko],
                   check=True, stdout=subprocess.DEVNULL)
    name = 'drv%d' % (int(time.time()) % 100000)
    r = run.upload(g, ko, name + '.ko')
    if not r:
        print('上传失败')
        return 1
    print('insmod:', g.nsh('insmod /data/%s.ko %s' % (name, name)))
    time.sleep(3)

    ours = find_ours()
    print('ours:', ours and (hex(ours[0]), ours[1]))
    if not ours or ours[1][2] <= ours[1][0]:
        print('图标没排版（coords 无效），停')
        return 1
    c = ours[1]
    cx, cy = (c[0] + c[2]) // 2, (c[1] + c[3]) // 2
    print('tap at', cx, cy)
    s, f = qmp()
    tap(f, cx, cy)
    s.close()
    time.sleep(2.5)

    g.nsh('lsmod')
    time.sleep(4)
    log = open(g.log_path(), encoding='utf-8', errors='replace').read()
    m = re.findall(r'%s\s+0x([0-9a-f]+).*?0x([0-9a-f]+)\s+(\d+)\s+0x([0-9a-f]+)\s+(\d+)' % name, log)
    if not m:
        print('模块不在 lsmod（可能挂了）')
        return 1
    b, sz = int(m[-1][3], 16), int(m[-1][4])
    d = qmpmem.read(b, max(sz, 200))
    i = d.find(struct.pack('<I', 0x52455431))
    if i < 0:
        print('没找到结果通道')
        return 1
    w = struct.unpack_from('<32I', d, i)
    print('on_create', hex(w[20]), 'desc', hex(w[21]), 'root', hex(w[22]))
    print('on_resume', hex(w[23]), 'title ret', hex(w[28]))
    print('shot:', run.screendump('drive_after_tap'))
    return 0


if __name__ == '__main__':
    sys.exit(main())
