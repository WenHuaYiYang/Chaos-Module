#!/usr/bin/env python3
"""一键：干净启动 -> 进桌面 -> 装 launch9 -> 点我们的图标 -> 读 on_create 标记。

用法：python work/probe_p65/tap_launch.py
判据：PROBE_RET[20] == 0xCAFE0001 表示固件真的调了我们页描述符的 on_create。
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
import run  # noqa: E402
import qmpmem  # noqa: E402

W, H = 432, 514


def qmp_conn():
    host, p = qmpmem.port().rsplit(':', 1)
    s = socket.create_connection((host, int(p)), timeout=20)
    f = s.makefile('rwb')
    f.readline()
    f.write((json.dumps({"execute": "qmp_capabilities"}) + "\n").encode())
    f.flush()
    f.readline()
    return s, f


def ev(f, x, y, down=None):
    e = [{'type': 'abs', 'data': {'axis': 'x', 'value': int(x / (W - 1) * 0x7fff)}},
         {'type': 'abs', 'data': {'axis': 'y', 'value': int(y / (H - 1) * 0x7fff)}}]
    if down is not None:
        e.append({'type': 'btn', 'data': {'down': down, 'button': 'left'}})
    f.write((json.dumps({"execute": "input-send-event", "arguments": {"events": e}}) + "\n").encode())
    f.flush()
    f.readline()


def swipe(f, x0, y0, x1, y1):
    ev(f, x0, y0, True)
    time.sleep(0.03)
    for i in range(1, 11):
        ev(f, x0 + (x1 - x0) * i // 10, y0 + (y1 - y0) * i // 10)
        time.sleep(0.015)
    time.sleep(0.05)
    ev(f, x1, y1, False)
    time.sleep(0.9)


def tap(f, x, y, hold=0.12):
    ev(f, x, y, True)
    time.sleep(hold)
    ev(f, x, y, False)
    time.sleep(0.5)


def read_ret(base, size):
    d = qmpmem.read(base, max(size, 80))
    i = d.find(struct.pack('<I', 0x52455431))
    if i < 0:
        return None
    return list(struct.unpack_from('<48I', d, i))


def main():
    g = run.restart()
    if g is None:
        sys.exit('guest 没有就绪')
    time.sleep(2)
    s, f = qmp_conn()
    for _ in range(2):
        swipe(f, 216, 430, 216, 150)
    print('桌面截图:', run.screendump('tl_grid'))
    s.close()

    subprocess.run(['bash', os.path.join(HERE, 'build.sh'), 'launch9'], check=True,
                   stdout=subprocess.DEVNULL)
    ko = os.path.join(HERE, 'probe_p65.ko')
    subprocess.run([sys.executable, os.path.join(ROOT, 'scripts', 'ko_entry_patch.py'), ko],
                   check=True, stdout=subprocess.DEVNULL)
    uniq = 'tl9x%d' % (int(time.time()) % 100000)
    if run.upload(g, ko, uniq + '.ko') is None:
        sys.exit('上传失败')
    time.sleep(5)
    print('insmod:', g.nsh('insmod /data/%s.ko %s' % (uniq, uniq)))
    time.sleep(7)
    g.nsh('lsmod')
    time.sleep(4)
    log = open(g.log_path(), encoding='utf-8', errors='replace').read()
    m = re.findall(r'%s\s+0x([0-9a-f]+).*?0x([0-9a-f]+)\s+(\d+)\s+0x([0-9a-f]+)\s+(\d+)' % uniq, log)
    if not m:
        sys.exit('lsmod 里没有本模块')
    base, size = int(m[-1][3], 16), int(m[-1][4])
    w = read_ret(base, size)
    if w is None:
        sys.exit('没找到结果通道魔数')
    print('install=%#x lookup=%#x 桌面记录=%#x 图标对象=%#x' % (w[4], w[5], w[9], w[12]))
    print('截图:', run.screendump('tl_after_add'))

    icon = w[12]
    if icon == 0:
        sys.exit('没有图标对象，后面不用点了')
    c = struct.unpack('<4I', qmpmem.read(icon + 0x1C, 16))
    cx, cy = (c[0] + c[2]) // 2, (c[1] + c[3]) // 2
    print('图标 coords=(%d,%d)-(%d,%d) 点 (%d,%d)' % (c[0], c[1], c[2], c[3], cx, cy))

    s, f = qmp_conn()
    tap(f, cx, cy)
    s.close()
    time.sleep(2)
    print('点击后截图:', run.screendump('tl_tapped'))
    w2 = read_ret(base, size)
    print('on_create 标记 =', hex(w2[20]) if w2 else 'none',
          ' desc=', hex(w2[21]) if w2 else '', ' root=', hex(w2[22]) if w2 else '')


if __name__ == '__main__':
    main()
