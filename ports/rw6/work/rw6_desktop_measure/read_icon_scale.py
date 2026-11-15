#!/usr/bin/env python3
"""桌面亮着时 dump PSRAM 并直接读图标对象的 scale 字段。

为什么这么读: RW6 的 launcher 代码在 PSRAM(见 docs/Chaos_桌面图标缩放_20261024.md §13),
对象也在 PSRAM 堆上(超出 8MB 窗口, 所以要 dump 32MB)。按"4 个连续 u32 构成 96x96 的矩形"
这个几何模式找对象, 比按类指针找稳(类指针每次启动可能不同)。
scale 字段口径沿用 043 的 lv_image_t: +0x50 / +0x54, 256 为一倍。
"""
import os
import struct
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, '..', '..'))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(ROOT, 'scripts'))
import psram_dump as PD                                       # noqa: E402
import sim_nsh as S                                           # noqa: E402

BASE = 0x1C000000
TOTAL = 32 * 1024 * 1024


def find_icon_objects(buf):
    """按几何找 96x96 的对象: coords 四个 i32 (x1,y1,x2,y2)。"""
    out = []
    n = len(buf)
    for o in range(0, n - 16, 4):
        x1, y1, x2, y2 = struct.unpack_from('<4i', buf, o)
        w = x2 - x1
        h = y2 - y1
        if 94 <= w <= 98 and 94 <= h <= 98 and 0 <= x1 <= 432 and 0 <= y1 <= 514:
            out.append((o, x1, y1, x2, y2))
    return out


def main():
    lp = PD.live_port()
    if not lp:
        print('拉起实例')
        PD.start_instance()
        for _ in range(30):
            time.sleep(3)
            lp = PD.live_port()
            if lp:
                break
    if not lp:
        print('实例起不来')
        return 1
    print('QMP %s:%d' % lp)
    time.sleep(22)
    q = PD.Qmp(*lp)
    # 用表冠把桌面翻出来(触摸注入这台机器没有)。实例的 HTTP 口可能还没就绪(503),
    # 那就不导航, 先把 dump 拿到手 —— 读对象比导航更值钱。
    def crown(body):
        try:
            S.api(S.instance_cfg(PD.IID), '/api/crown', body)
            return True
        except BaseException:      # sim_nsh 的 api 失败是抛 SystemExit, 不是 Exception
            return False
    for i in range(4):
        if not crown({'action': 'click'}):
            print('表冠不通, 直接 dump 当前状态')
            break
        time.sleep(1.2)
    # dump
    t0 = time.time()
    buf = bytearray()
    step = 4096
    while len(buf) < TOTAL:
        want = min(step, TOTAL - len(buf))
        try:
            b = q.xp(BASE + len(buf), want)
        except Exception as e:
            print('断连于 %d MB: %s' % (len(buf) // 1048576, type(e).__name__))
            break
        if len(b) > want:
            b = b[:want]
        elif len(b) != want:
            if want > 512:
                step = max(512, len(b) or 512)
                continue
            b = b + bytes(want - len(b))
        buf += b
        if len(buf) % (8 * 1024 * 1024) < want:
            print('  %2d MB %.0fs' % (len(buf) // 1048576, time.time() - t0), flush=True)
    q.close()
    out = os.path.join(ROOT, 'build', 'rw6_measure', 'rw6_psram_desk.bin')
    open(out, 'wb').write(bytes(buf))
    print('dump %d 字节 -> %s  用时 %.0fs' % (len(buf), out, time.time() - t0))

    cand = find_icon_objects(buf)
    print('几何命中 96x96 的候选: %d 个' % len(cand))
    # 对象起点未知(coords 在 lv_obj_t 里是 +0x1C 起), 所以按 coords-0x1C 当对象头读 scale
    n = len(buf)
    got = []
    for o, x1, y1, x2, y2 in cand:
        obj = o - 0x1C
        if obj < 0 or obj + 0x58 > n:
            continue
        sx, sy = struct.unpack_from('<2I', buf, obj + 0x50)
        got.append((y1 + (y2 - y1) // 2, x1, sx, sy, obj + BASE))
    got.sort()
    print('  中心y  x1    scale_x scale_y   对象VA')
    for yc, x1, sx, sy, va in got[:40]:
        mark = '' if 0 < sx <= 512 else '  (不像 scale)'
        print('   %4d  %4d   %5d   %5d   0x%08X%s' % (yc, x1, sx, sy, va, mark))
    return 0


if __name__ == '__main__':
    sys.exit(main())
