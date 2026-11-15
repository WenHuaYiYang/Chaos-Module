#!/usr/bin/env python3
"""rw6-dbg 上量"滚动中"与"静止"的图标尺寸差：QMP 绝对鼠标(按下与移动分开发事件)。

口径来自 work/p65_chaos/chaos_guest.py(另一会话已验证: 这台 guest 只认 QMP 绝对鼠标,
VNC 指针通道不生效)。QMP 口按 runs/<实例号> 反查进程命令行现取, 不用"第一个匹配进程"。
"""
import json
import os
import re
import socket
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

HERE = Path(os.path.dirname(os.path.abspath(__file__)))
OUT = Path(os.path.dirname(os.path.abspath(__file__))).parent.parent / 'build' / 'rw6_measure'
ROOT = HERE.parent.parent
OUT.mkdir(parents=True, exist_ok=True)
sys.path.insert(0, str(ROOT / 'scripts'))
IID = 'i-158e0451f6b64beae40db144269484c7'
W, H = 432, 514
TMP = os.environ.get('TEMP', 'C:/Users/YiYang/AppData/Local/Temp').replace('\\', '/')
import sim_nsh as S                                    # noqa: E402
from PIL import Image                                  # noqa: E402


def port():
    ps = ("Get-CimInstance Win32_Process | Where-Object { $_.Name -like '*qemu*' } | "
          "ForEach-Object { [string]$_.ProcessId + '@@' + $_.CommandLine }")
    r = subprocess.run(['powershell', '-NoProfile', '-Command', ps],
                       capture_output=True, text=True, encoding='utf-8', errors='replace')
    for line in r.stdout.splitlines():
        if '@@' in line and IID in line:
            return int(re.search(r'-qmp tcp:[0-9.]+:(\d+)', line).group(1))
    return None


def ensure_up():
    st = {i['id']: i['state'] for i in S._get_json(S.BASE + '/api/instances', S._mhdr())['instances']}
    if st.get(IID) == 'running' and port():
        return True
    body = json.dumps({'instance': IID, 'skipBinding': True}).encode()
    req = urllib.request.Request(S.BASE + '/api/instances/start', data=body,
                                 headers=S._mhdr(True), method='POST')
    try:
        urllib.request.urlopen(req, timeout=120)
    except Exception as e:
        print('start:', str(e)[:80])
    for i in range(30):
        time.sleep(2)
        if port():
            print('起来了 %ds' % ((i + 1) * 2))
            return True
    return False


class Q:
    def __init__(self):
        self.s = socket.create_connection(('127.0.0.1', port()), timeout=20)
        self.f = self.s.makefile('rwb')
        self.f.readline()
        self._c('qmp_capabilities')

    def _c(self, n, a=None):
        m = {'execute': n}
        if a:
            m['arguments'] = a
        self.f.write((json.dumps(m) + '\n').encode())
        self.f.flush()
        return json.loads(self.f.readline())

    def hmp(self, line):
        return self._c('human-monitor-command', {'command-line': line}).get('return', '')

    def abs_ev(self, x, y):
        self._c('input-send-event', {'events': [
            {'type': 'abs', 'data': {'axis': 'x', 'value': int(x / (W - 1) * 0x7fff)}},
            {'type': 'abs', 'data': {'axis': 'y', 'value': int(y / (H - 1) * 0x7fff)}}]})

    def btn(self, down):
        self._c('input-send-event', {'events': [
            {'type': 'btn', 'data': {'down': down, 'button': 'left'}}]})

    def swipe(self, x, y, dy=-200, steps=12, hold=0.05, gap=0.012):
        self.abs_ev(x, y)
        self.btn(True)
        time.sleep(hold)
        for i in range(1, steps + 1):
            self.abs_ev(x, y + dy * i / steps)
            time.sleep(gap)
        time.sleep(hold)
        self.btn(False)

    def shot(self, tag):
        p = '%s/mb_%s.ppm' % (TMP, tag)
        if os.path.exists(p):
            os.remove(p)
        self.hmp('screendump ' + p)
        for _ in range(30):
            if os.path.exists(p):
                break
            time.sleep(0.02)
        out = str(OUT / (tag + '.png'))
        Image.open(p).save(out)
        return out


def blobs(path):
    im = Image.open(path).convert('L')
    if im.size != (W, H):
        return []
    bw = im.point(lambda v: 255 if v > 40 else 0)
    res = []
    for c in range(4):
        a, b = int(c * W / 4) + 2, int((c + 1) * W / 4) - 2
        band = bw.crop((a, 0, b, H)).resize((1, H), Image.BOX)
        widths = [band.getpixel((0, y)) * (b - a) / 255.0 for y in range(H)]
        y = 0
        while y < H:
            if widths[y] < 5:
                y += 1
                continue
            y0 = y
            while y < H and widths[y] >= 5:
                y += 1
            y1 = y - 1
            if (y1 - y0 + 1) >= 45 and 45 <= max(widths[y0:y1 + 1]) <= 110:
                res.append((c, y0, y1, y1 - y0 + 1, round(max(widths[y0:y1 + 1])),
                            y0 <= 1 or y1 >= H - 2))
            y += 1
    return res


def grid(bl):
    per = {}
    for r in bl:
        per.setdefault(r[0], []).append(r)
    return all(len(per.get(c, [])) >= 3 for c in range(4))


def main():
    if not ensure_up():
        print('起不来')
        return 1
    cfg = S.instance_cfg(IID)
    q = Q()

    def crown(steps=None):
        body = {'action': 'click'} if steps is None else {'action': 'rotate', 'steps': steps}
        try:
            S.api(cfg, '/api/crown', body)
            return True
        except SystemExit:
            return False

    print('crown 可用?', crown())
    for i in range(6):
        p = q.shot('v%d' % i)
        bl = blobs(p)
        print('  v%d 块%d 网格=%s' % (i, len(bl), grid(bl)))
        if grid(bl):
            break
        q.swipe(W // 2, 430, dy=-260)
        time.sleep(1.2)
    else:
        print('没进网格')
        return 1

    def dump(tag):
        p = q.shot(tag)
        bl = [r for r in blobs(p)]
        hs = sorted(set(r[3] for r in bl if not r[5]))
        print('  %-9s 块%2d 高%s 列1段起%s' % (tag, len(bl), hs,
                                              [r[1] for r in bl if r[0] == 1][:4]))
    dump('still_a')
    # 按住慢慢拖: 中途连拍(这就是"手指按住滚动"那一态)
    q.abs_ev(W // 2, 430)
    q.btn(True)
    time.sleep(0.05)
    for i in range(1, 13):
        q.abs_ev(W // 2, 430 - 260 * i / 12)
        if i in (3, 6, 9, 12):
            dump('drag%02d' % i)
        time.sleep(0.06)
    q.btn(False)
    for ms in (100, 250, 500, 900, 1500):
        time.sleep(ms / 1000.0)
        dump('rel%04d' % ms)
    dump('still_b')
    return 0


if __name__ == '__main__':
    sys.exit(main())
