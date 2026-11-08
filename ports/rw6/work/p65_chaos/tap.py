#!/usr/bin/env python3
"""对 p65 guest 注触摸并抓图（触摸走 VNC 指针通道，画面走 QMP screendump）。

用法:
  python work/p65_chaos/tap.py tap 216 146 tag
  python work/p65_chaos/tap.py drag 216 420 216 140 600 tag
只做注入与截图，不解析 VNC 像素（该实例会推 ZRLE，解析器不支持）。
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, os.path.join(ROOT, 'scripts'))
sys.path.insert(0, os.path.join(ROOT, 'work', 'probe_p65'))
import qemu_vnc  # noqa: E402
import run as R  # noqa: E402


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    cmd = argv[1]
    s = qemu_vnc.open_screen()
    if cmd == 'tap':
        x, y = int(argv[2]), int(argv[3])
        s.tap(x, y)
        tag = argv[4] if len(argv) > 4 else 'tap'
    else:
        x1, y1, x2, y2 = (int(a) for a in argv[2:6])
        ms = int(argv[6]) if len(argv) > 6 else 600
        s.drag(x1, y1, x2, y2, ms)
        tag = argv[7] if len(argv) > 7 else 'drag'
    s.drain(400)
    try:
        s.sock.close()
    except Exception:
        pass
    png = R.screendump(tag)
    print('截图:', png)
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
