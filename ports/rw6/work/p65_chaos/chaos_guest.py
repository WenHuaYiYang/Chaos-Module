#!/usr/bin/env python3
"""p65_chaos 的部署助手：确保 guest 就绪 -> 补 e_entry -> 上传 -> insmod。"""
import os
import re
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
PROBE = os.path.join(ROOT, 'work', 'probe_p65')
sys.path.insert(0, PROBE)
import run as R  # noqa: E402


def build(feature=''):
    subprocess.run(['bash', os.path.join(HERE, 'build.sh')] + ([feature] if feature else []),
                   check=True, stdout=subprocess.DEVNULL)


def deploy(ko_name='p65_chaos.ko', suffix=''):
    ko = os.path.join(HERE, ko_name)
    subprocess.run([sys.executable, os.path.join(ROOT, 'scripts', 'ko_entry_patch.py'), ko],
                   check=True, stdout=subprocess.DEVNULL)
    g = R.guest_ready(2)
    if g is None:
        g = R.restart()
    if g is None:
        # reboot 之后 nativeResourcesReady 可能一直卡住，用 NSH 能应答 + ap> 兜底
        g = R.ensure_guest(60)
    if g is None:
        print('guest 起不来')
        return None, None
    uniq = 'c%s_%d' % (suffix or 'x', int(time.time()) % 100000)
    name = uniq + '.ko'
    if R.upload(g, ko, name) is None:
        print('上传失败')
        return g, None
    time.sleep(4)
    print('insmod:', g.nsh('insmod /data/%s %s' % (name, uniq)))
    time.sleep(8)  # 200ms 一拍，第 20 拍由固件自己打开应用
    log = open(g.log_path(), encoding='utf-8', errors='replace').read()
    plain = re.sub(r'\x1b\[[0-9;]*[A-Za-z]', '', log)
    print('存活:', plain.rstrip().endswith('ap>'))
    return g, uniq


def read_diag(g, uniq):
    """lsmod 找本模块的 DATA 区，按魔数定位 DIAG 数组并读回。"""
    import re
    import struct
    import time as _t
    g.nsh('lsmod')
    _t.sleep(4)
    log = open(g.log_path(), encoding='utf-8', errors='replace').read()
    m = re.findall(r'%s\s+0x([0-9a-f]+).*?0x([0-9a-f]+)\s+(\d+)\s+0x([0-9a-f]+)\s+(\d+)' % uniq, log)
    if not m:
        return None
    base, size = int(m[-1][3], 16), int(m[-1][4])
    d = R.qmpmem.read(base, max(size, 64))
    magic = struct.pack('<I', 0xC0DE0034)
    i = d.find(magic)
    if i < 0:
        return None
    return [hex(struct.unpack_from('<I', d, i + k)[0]) for k in range(0, 16, 4)]


def data_region(g, uniq, tries=2):
    """lsmod 里本模块的 DATA 段 (base, size)。"""
    for _ in range(tries):
        g.nsh('lsmod')
        time.sleep(4)
        log = open(g.log_path(), encoding='utf-8', errors='replace').read()
        plain = re.sub(r'\x1b\[[0-9;]*[A-Za-z]', '', log)
        m = re.findall(
            r'%s\s+0x[0-9a-f]+\s+\d+\s+\d+\s+\d+\s+0x[0-9a-f]+\s+\d+\s+0x([0-9a-f]+)\s+(\d+)'
            % re.escape(uniq), plain)
        if m:
            return int(m[-1][0], 16), int(m[-1][1])
    return None, None


def find_magic(g, uniq, magic):
    """在模块 DATA 段里按 u32 魔数定位一个小结构，返回它的地址。"""
    import re
    import struct
    base, size = data_region(g, uniq)
    if base is None:
        # lsmod 的 DATA SIZE 偶发打印成非数字（如 1UUUU），但 DATA 基址那格仍是有效十六进制。
        g.nsh('lsmod')
        time.sleep(4)
        log = open(g.log_path(), encoding='utf-8', errors='replace').read()
        plain = re.sub(r'\x1b\[[0-9;]*[A-Za-z]', '', log)
        line = next((v for v in plain.splitlines() if uniq in v), '')
        addrs = [int(v, 16) for v in re.findall(r'0x([0-9a-f]+)', line)]
        addrs = [v for v in addrs if v >= 0x3C00_0000]
        if not addrs:
            return None
        base = max(addrs)
        size = 0x40000
    if size < 0x100:
        size = 0x40000
    d = R.qmpmem.read(base, size, step=8192) or b''
    i = d.find(struct.pack('<I', magic))
    if i < 0:
        return None
    return base + i


def crown(action='click', steps=None):
    """按一下表冠（唤醒/回桌面/开应用列表由固件决定）。"""
    import urllib.request
    import json
    st = os.path.expanduser('~') + '/AppData/Local/MiBandStudio/state'
    base = json.load(open(os.path.join(st, 'launcher.json'), encoding='utf-8'))['url'].rstrip('/')
    inst = R.INST
    tok = ''
    try:
        cfg = json.loads(urllib.request.urlopen(
            base + '/instances/%s/config.json' % inst, timeout=10).read())
        tok = cfg.get('controlToken', '')
    except Exception:
        pass
    req = urllib.request.Request(
        base + '/instances/%s/api/crown' % inst,
        data=json.dumps({'action': action} if steps is None else {'action': action, 'steps': steps}).encode(),
        headers={'X-MiBand-Control': tok, 'Content-Type': 'application/json',
                 'Origin': base, 'Referer': base + '/instances/%s/' % inst},
        method='POST')
    try:
        return json.loads(urllib.request.urlopen(req, timeout=55).read()).get('ok')
    except Exception as e:
        return str(e)


def _qmp_conn():
    import socket
    import json
    host, port = R.qmpmem.port().rsplit(':', 1)
    s = socket.create_connection((host, int(port)), timeout=20)
    f = s.makefile('rwb')
    f.readline()
    f.write((json.dumps({"execute": "qmp_capabilities"}) + "\n").encode())
    f.flush()
    f.readline()
    return s, f


def tap(x, y, hold=0.12):
    """QMP 绝对鼠标短按：guest 把它翻成触摸事件（VNC 指针通道对这台 guest 不生效）。"""
    import json
    import time
    w, h = 432, 514
    s, f = _qmp_conn()

    def ev(down=None):
        e = [{'type': 'abs', 'data': {'axis': 'x', 'value': int(x / (w - 1) * 0x7fff)}},
             {'type': 'abs', 'data': {'axis': 'y', 'value': int(y / (h - 1) * 0x7fff)}}]
        if down is not None:
            e.append({'type': 'btn', 'data': {'down': down, 'button': 'left'}})
        f.write((json.dumps({"execute": "input-send-event", "arguments": {"events": e}}) + "\n").encode())
        f.flush()
        f.readline()

    ev(True)
    time.sleep(hold)
    ev(False)
    s.close()


def swipe(x, y, dx=0, dy=-200, steps=12, hold=0.05):
    """QMP 绝对鼠标拖动：按下 -> 逐点移动 -> 抬起。用来翻页/滑出应用列表。"""
    import json
    import time
    w, h = 432, 514
    s, f = _qmp_conn()

    def abs_ev(ax, ay):
        e = [{'type': 'abs', 'data': {'axis': 'x', 'value': int(ax / (w - 1) * 0x7fff)}},
             {'type': 'abs', 'data': {'axis': 'y', 'value': int(ay / (h - 1) * 0x7fff)}}]
        f.write((json.dumps({"execute": "input-send-event", "arguments": {"events": e}}) + "\n").encode())
        f.flush()
        f.readline()

    def btn(down):
        e = [{'type': 'btn', 'data': {'down': down, 'button': 'left'}}]
        f.write((json.dumps({"execute": "input-send-event", "arguments": {"events": e}}) + "\n").encode())
        f.flush()
        f.readline()

    abs_ev(x, y)
    btn(True)
    time.sleep(hold)
    for i in range(1, steps + 1):
        abs_ev(x + dx * i / steps, y + dy * i / steps)
        time.sleep(0.012)
    time.sleep(hold)
    btn(False)
    s.close()


def shot(tag):
    png = R.screendump(tag)
    print('截图:', png)
    return png
