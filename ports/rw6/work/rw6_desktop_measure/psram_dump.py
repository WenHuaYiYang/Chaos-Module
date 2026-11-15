#!/usr/bin/env python3
"""从 MiBand Studio 的 rw6-dbg 实例 dump PSRAM。

为什么长连接: 第一版每读一块就新建一次 QMP 连接, 结果一万多次连接把 guest 打死
(8MB 那次用长连接 20 秒读完、实例没事)。所以这里全程只开一条连接。
为什么要 dump: RW6 的 launcher/LVGL 实现不在 vela_ap.bin 里执行, 它在 PSRAM;
AP 镜像里那些 launcher 函数名只有串没有引用, 就是因为引用方在这儿。
注意 PSRAM 布局不跨启动稳定(同一偏移两次 boot 读出的内容不同), 每次分析要用同一份 dump。

用法: python psram_dump.py [输出文件] [MB数]
"""
import json
import os
import re
import socket
import subprocess
import sys
import time
import urllib.request

IID = 'i-158e0451f6b64beae40db144269484c7'      # rw6-dbg，别的实例不许碰
BASE = 0x1C000000
ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..'))
sys.path.insert(0, os.path.join(ROOT, 'scripts'))
import sim_nsh as S                              # noqa: E402


def live_port():
    """按 runs/<实例号> 反查这条实例的 QMP 口，并用 banner 确认它真的在听。"""
    ps = ("Get-CimInstance Win32_Process | Where-Object { $_.Name -like '*qemu*' } | "
          "ForEach-Object { [string]$_.ProcessId + '@@' + $_.CommandLine }")
    r = subprocess.run(['powershell', '-NoProfile', '-Command', ps],
                       capture_output=True, text=True, encoding='utf-8', errors='replace')
    for line in r.stdout.splitlines():
        if '@@' not in line or IID not in line:
            continue
        m = re.search(r'-qmp tcp:([0-9.]+):(\d+)', line)
        if not m:
            continue
        host, port = m.group(1), int(m.group(2))
        try:
            s = socket.create_connection((host, port), timeout=4)
            banner = s.recv(128)
            s.close()
            if b'QMP' in banner:
                return host, port
        except OSError:
            continue
    return None


def start_instance():
    """实例可能停在 state=error(报"模拟器启动失败")，这时必须先 stop 再 start 才拉得起来。"""
    for path in ('/api/instances/stop', '/api/instances/start'):
        body = {'instance': IID}
        if path.endswith('start'):
            body['skipBinding'] = True
        req = urllib.request.Request(S.BASE + path, data=json.dumps(body).encode(),
                                     headers=S._mhdr(True), method='POST')
        try:
            urllib.request.urlopen(req, timeout=120)
        except Exception as e:
            print(path, type(e).__name__, str(e)[:70])
        time.sleep(3)


class Qmp:
    def __init__(self, host, port):
        self.s = socket.create_connection((host, port), timeout=60)
        self.f = self.s.makefile('rwb')
        self.f.readline()
        self.call('qmp_capabilities')

    def call(self, name, args=None):
        m = {'execute': name}
        if args:
            m['arguments'] = args
        self.f.write((json.dumps(m) + '\n').encode())
        self.f.flush()
        return json.loads(self.f.readline())

    def hmp(self, line):
        return self.call('human-monitor-command', {'command-line': line}).get('return', '')

    def xp(self, addr, cnt):
        t = self.hmp('xp/%dxb 0x%x' % (cnt, addr))
        out = bytearray()
        for row in t.splitlines():
            if ':' not in row:
                continue
            for tok in row.split(':', 1)[1].split():
                try:
                    out.append(int(tok, 16))
                except ValueError:
                    pass
        return bytes(out)

    def close(self):
        try:
            self.f.close()
            self.s.close()
        except OSError:
            pass


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        ROOT, 'build', 'rw6_measure', 'rw6_psram_%dmb.bin' % (int(sys.argv[2]) if len(sys.argv) > 2 else 32))
    total = (int(sys.argv[2]) if len(sys.argv) > 2 else 32) * 1024 * 1024
    os.makedirs(os.path.dirname(out), exist_ok=True)
    lp = live_port()
    if not lp:
        print('实例没在跑，启动它')
        start_instance()
        for _ in range(30):
            time.sleep(3)
            lp = live_port()
            if lp:
                break
    if not lp:
        print('起不来')
        return 1
    print('QMP %s:%d' % lp)
    time.sleep(18)                        # 等固件跑到 UI 建好对象
    q = Qmp(*lp)
    t0 = time.time()
    step = 4096
    written = 0
    with open(out, 'wb') as fp:
        while written < total:
            want = min(step, total - written)
            try:
                b = q.xp(BASE + written, want)
            except Exception as e:
                print('第 %d MB 处连接断了: %s' % (written // 1048576, type(e).__name__))
                break
            if len(b) > want:
                # QEMU 的 xp 按行打印, 返回会向上取整到行宽
                b = b[:want]
            elif len(b) != want:
                # 块太长被截就退一档, 别把空洞当数据
                if want > 512:
                    step = max(512, len(b) or 512)
                    continue
                b = b + bytes(want - len(b))
            fp.write(b)
            written += want
            if written % (4 * 1024 * 1024) < want:
                print('  %2d MB  %.0fs  当前块 %d' %
                      (written // 1048576, time.time() - t0, step), flush=True)
    q.close()
    print('落盘 %s  %d 字节  用时 %.0fs' % (out, written, time.time() - t0))
    return 0


if __name__ == '__main__':
    sys.exit(main())
