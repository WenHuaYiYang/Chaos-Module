#!/usr/bin/env python3
"""取"滚动前后"两份 PSRAM 堆窗差分，把图标对象的缩放字段直接读出来。

为什么这么做：静态那一路已经到底（桌面是 zoom_view 控件、代码在 flash
0x71E000..0x742000、blob 与 AP 镜像逐字节等价），但"滚动中缩放怎么变"
只剩数值证据能定。两份 dump 同一次开机的同一份布局，差出来的字就是
LVGL 每帧真正动过的字段；其中落在 0x3E000000..0x3FFFFFFF 的
就是 IEEE754 单精度 0.125..2.0 —— 缩放值的形状。

用法: python scale_probe.py <输出前缀> [dump 起始] [字节数]
"""
import json
import os
import subprocess
import sys
import time
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from psram_dump import BASE, Qmp, live_port          # noqa: E402
import sim_nsh as S                                   # noqa: E402

IID = 'i-158e0451f6b64beae40db144269484c7'           # rw6-dbg，别的实例不许碰
ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..'))
PRE = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, 'build', 'rw6_measure', 'sp')
# 参数里可能带 --walk-only <文件>，所以地址与长度要容错解析
def _num(i, dflt):
    try:
        return int(sys.argv[i], 0) if len(sys.argv) > i else dflt
    except ValueError:
        return dflt


ADDR = _num(2, 0x1C400000)
SIZE = _num(3, 0x180000)


def api(path, body=None):
    if body is None:
        req = urllib.request.Request(S.BASE + path, headers=S._mhdr(False))
    else:
        req = urllib.request.Request(S.BASE + path, data=json.dumps(body).encode(),
                                     headers=S._mhdr(True), method='POST')
    try:
        return json.load(urllib.request.urlopen(req, timeout=30))
    except Exception as e:
        return {'ERR': type(e).__name__ + ' ' + str(e)[:70]}


def shot(q, tag):
    """screendump 的路径不能带空格，落 %TEMP% 再转 PNG。"""
    raw = os.path.join(os.environ['TEMP'], 'sp_%s.ppm' % tag)
    png = os.path.join(os.environ['TEMP'], 'sp_%s.png' % tag)
    for p in (raw, png):
        if os.path.exists(p):
            os.remove(p)
    q.hmp('screendump %s' % raw)
    for _ in range(20):
        if os.path.exists(raw):
            break
        time.sleep(0.3)
    if not os.path.exists(raw):
        return None
    from PIL import Image
    im = Image.open(raw)
    im.save(png)
    g = im.convert('L').getdata()
    nb = sum(1 for v in g if v > 24)
    return {'png': png, 'w': im.width, 'h': im.height, 'nonblack': nb}


def dump(q, tag):
    buf = bytearray()
    step = 4096
    got = 0
    while got < SIZE:
        want = min(step, SIZE - got)
        b = q.xp(ADDR + got, want)
        if len(b) > want:
            b = b[:want]
        elif len(b) != want:
            b = b + bytes(want - len(b))
        buf += b
        got += want
    path = '%s_%s.bin' % (PRE, tag)
    open(path, 'wb').write(bytes(buf))
    return path


# --- 对象图走一遍：桌面控件的逐项缩放回调就挂在 desc[0] 上 -------------------
#
# 依据（文档 §26）：zoom_view 核心 FUN_0x17C24C 里
#   obj+0x68 = 描述符数组、obj+0x70 = 槽数、obj+0x78 = 可见数、obj+100 = 当前索引；
#   每个可见子对象由 (*desc[0])(child, desc[5]) 配置，并把 desc[6]/child+0x34 回写。
# 控件单例怎么找：flash file 0x7302A8 这个字面量存的是 0x3C456FA0，
# 而 0x3C456FA0 这一格里放的才是控件实例指针（代码是 `iVar5 = *DAT_0x7302A8`）。
# 所以 blob 偏移取 0x456FA0（不是字面量自己所在的位置 —— 那一格运行期会被堆覆盖）。
SINGLETON = 0x456FA0
CTX_PTR = 0x2E478            # 桌面上下文的指针槽（blob 偏移）
PSRAM_BASE = 0x1C000000      # 上面两个都是相对 PSRAM 起点的偏移


def _u32(buf, o):
    return int.from_bytes(buf[o:o + 4], 'little') if 0 <= o < len(buf) - 4 else None


def _in(o):
    return o is not None and WBASE <= norm(o) < WBASE + len(SNAPBUF)


def norm(o):
    """0x1C / 0x3C / 0x38 / 0x18 是同一块 PSRAM 的四个别名窗（零位移）。
    实例指针存的是 0x3C 基，不归一就会判成"窗口外"。"""
    return (o & 0x00FFFFFF) | 0x1C000000 if o is not None else None


SNAPBUF = b''
WBASE = 0


def walk(path):
    """在一份 dump 上顺着指针走：单例 -> 控件 -> 描述符数组 -> desc[0]。

    dump 的基址按文件大小判：整份 8MB 就是 PSRAM 起点 0x1C000000，
    否则用命令行给的窗口起点 —— 判错会让每个偏移错开一个常数，读出来全是垃圾。
    """
    global SNAPBUF, WBASE
    SNAPBUF = open(path, 'rb').read()
    # 基址按文件大小判：整份 PSRAM dump（≥4MB）都从 0x1C000000 起；
    # 只 dump 了一个窗口时用命令行给的起点。判错会让每个偏移整体错开，读出来全是垃圾。
    if '--base' in sys.argv:
        WBASE = int(sys.argv[sys.argv.index('--base') + 1], 0)
    elif len(SNAPBUF) >= 0x400000:
        WBASE = 0x1C000000
    else:
        WBASE = ADDR

    def rd(va):
        return _u32(SNAPBUF, norm(va) - WBASE) if _in(va) else None
    sing = PSRAM_BASE + SINGLETON
    print('  dump %s 基址 0x%08x' % (path, WBASE))
    print('  单例槽 0x%08x -> %s' % (sing, hex(rd(sing) or 0)))
    w = rd(sing)
    if not w or not _in(w):
        print('  对象图空：控件实例不在这个 dump 窗口里（桌面没建或窗口取偏了）')
        return
    slots = rd(w + 0x70); vis = rd(w + 0x78); cur = rd(w + 100); arr = rd(w + 0x68)
    print('  控件 0x%08x：槽数=%s 可见=%s 当前索引=%s desc数组=%s' % (
        w, slots, vis, cur, hex(arr) if arr else None))
    if not arr or not _in(arr):
        print('  desc 数组不在窗口里')
        return
    for i in range(min(vis or 0, 8)):
        d = rd(arr + 4 * i)
        if not d or not _in(d):
            print('    desc[%d] = %s（窗口外）' % (i, hex(d) if d else None))
            continue
        cb = rd(d); ud = rd(d + 20); obj = rd(d + 24)
        print('    desc[%d] @0x%08x cb=0x%s ud=0x%s obj=0x%s' % (
            i, d, ('%08x' % cb) if cb else '-----', ('%08x' % ud) if ud else '-----',
            ('%08x' % obj) if obj else '-----'))
    if cur is not None:
        d = rd(arr + 4 * cur)
        cb = rd(d) if _in(d) else None
        print('  当前索引那项的 cb = %s   <- 逐项缩放就发生在这里' % (
            hex(cb) if cb else '取不到'))


def main():
    if '--walk-only' in sys.argv:
        walk(sys.argv[sys.argv.index('--walk-only') + 1])
        return 0
    lp = live_port()
    if not lp:
        print('QMP 口找不到，实例没在跑')
        return 1
    print('QMP %s:%d' % lp)
    time.sleep(20)                                  # 等固件把 UI 建起来
    q = Qmp(*lp)
    print('crown click(亮屏):', json.dumps(api('/instances/%s/api/crown' % IID,
                                               {'action': 'click'}))[:120])
    time.sleep(2.5)
    s = shot(q, 'rest')
    print('rest 画面:', s)
    if s is None or s['nonblack'] < 2000:
        print('画面黑或太暗，桌面没亮着 —— 停，不取 dump')
        q.close()
        return 1
    a = dump(q, 'A')
    print('dump A:', a)
    walk(a)
    for i in range(4):
        r = api('/instances/%s/api/crown' % IID, {'action': 'rotate', 'steps': -8})
        if 'ERR' in r:
            print('第 %d 次转动失败: %s' % (i + 1, r))
            break
        time.sleep(1.2)
    time.sleep(2.0)
    s2 = shot(q, 'scrolled')
    print('scrolled 画面:', s2)
    b = dump(q, 'B')
    print('dump B:', b)
    q.close()
    return 0


if __name__ == '__main__':
    sys.exit(main())
