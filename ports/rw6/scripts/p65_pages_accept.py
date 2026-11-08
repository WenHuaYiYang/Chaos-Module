#!/usr/bin/env python3
"""导航回归：按真实坐标点一遍菜单与二级页，用模块自己的 CUR 判落点。

模块带 navdbg 时每拍往 MARK 发布一块快照（一次 QMP 读就够）：
+0x10 当前页号、+0x14 页根对象、+0x18 行数、+0x1C 页根存活、
+0x100 起每行 16 字节 = 行卡片的屏幕矩形。
点行 = 直接取该行矩形的中心（LVGL coords 是绝对坐标），不猜滑动量；
行不在视口就先滑一段，等惯性停稳再重读。翻页动画没走完时点返回键会被吞，
所以返回键按了要等 CUR 真变，没变就补按。

用法: python scripts/p65_pages_accept.py
"""
import os
import struct
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'work', 'p65_chaos'))
import chaos_guest as C  # noqa: E402

MARK = 0x3C7FF000
SNAP_MAGIC = 0xC0DE00E7
SNAP_LEN = 5 * 4 + 24 * 16   # 魔数 + CUR/ROOT/存活/行数 + 24 个矩形
SCREEN_W, SCREEN_H = 432, 514
TOP = 56          # 内容容器顶（标题栏高度）
BACK = (60, 36)   # 标题栏返回键
PAGE_MENU, PAGE_ABOUT, PAGE_APPS, PAGE_WFMGMT = 0, 1, 2, 12
PAGE_SETTINGS, PAGE_WATCH, PAGE_BRIGHT, PAGE_VIB, PAGE_CACHE, PAGE_LOG = 10, 9, 4, 7, 3, 13

NAV = [0]  # 模块 DATA 段里快照结构的地址，部署后按魔数定位


def snap():
    """(页号, 页根存活, 行矩形列表)。一次 QMP 读。"""
    b = C.R.qmpmem.read(NAV[0], SNAP_LEN) or b''
    if len(b) < 20:
        return -1, 0, []
    c = struct.unpack_from('<I', b, 4)[0]
    alive = struct.unpack_from('<I', b, 12)[0]
    n = struct.unpack_from('<I', b, 16)[0]
    rs = []
    for i in range(min(n, 24)):
        off = 20 + i * 16
        if off + 16 > len(b):
            break
        rs.append(struct.unpack_from('<4i', b, off))
    return c, alive, rs


def report(name, expect, c, alive):
    ok = c == expect
    print('%-28s CUR=%2d 期望=%2d %s%s' % (
        name, c, expect, 'OK' if ok else '**不符**', '' if alive else '（应用已切走）'))
    return ok


def wait_cur(expect, timeout=3.0):
    t0 = time.time()
    while time.time() - t0 < timeout:
        c, alive, _ = snap()
        if c == expect:
            return c, alive
        time.sleep(0.25)
    return snap()[0], snap()[1]


def step(name, expect, wait=1.5):
    time.sleep(wait)
    c, alive, _ = snap()
    return report(name, expect, c, alive)


def back(expect, name):
    """按返回键，等页面真的退回去；被翻页动画吞掉就补按。"""
    time.sleep(0.8)  # 进页动画走完，否则返回键被吞
    for _ in range(3):
        C.tap(*BACK)
        c, alive = wait_cur(expect, 2.0)
        if c == expect:
            return report(name, expect, c, alive)
        time.sleep(0.5)
    return report(name, expect, c, alive)


def wait_rows(n, timeout=6.0):
    t0 = time.time()
    while time.time() - t0 < timeout:
        c, alive, rs = snap()
        if not alive:
            print('应用已切走（页根存活=0）')
            return None
        if len(rs) >= n:
            return rs
        time.sleep(0.25)
    c, alive, rs = snap()
    return rs if alive else None


def settle(timeout=4.0):
    """等列表停下来：连续两次快照行矩形完全相同。"""
    time.sleep(0.5)  # 甩动惯性可能延迟起步，先让它起来
    prev = None
    t0 = time.time()
    while time.time() - t0 < timeout:
        c, alive, rs = snap()
        key = tuple(rs)
        if prev is not None and key == prev:
            return alive
        prev = key
        time.sleep(0.4)
    return False


def tap_row(i, need=None, name=''):
    """点第 i 行：读快照里的矩形；不在视口就滑，滑完等停稳再重读。"""
    rs = wait_rows(need if need else i + 1)
    if rs is None:
        print('%-28s 应用不在了' % (name or '点行'))
        return False
    if len(rs) <= i:
        print('%-28s 行数不足：只有 %d 行（要第 %d 行）' % (name or '点行', len(rs), i))
        return False
    for _ in range(12):
        c, alive, rs = snap()
        if not alive or len(rs) <= i:
            print('%-28s 行消失了（应用切走或页被重建）' % (name or '点行'))
            return False
        x1, y1, x2, y2 = rs[i]
        cx, cy = (x1 + x2) // 2, (y1 + y2) // 2
        if not (TOP + 8 <= cy <= SCREEN_H - 8):
            # 慢拖 + 收尾停住再抬手：不产生甩动。滚动的实际位移比拖动量大（实测约 1.7 倍），
            # 所以按需要的距离反着折算，宁可多走几轮也别一下甩过头。
            delta = cy - 257
            step = int(max(80, min(220, abs(delta) * 0.6)))
            if delta > 0:
                C.swipe(216, 430, dy=-step, steps=30, hold=0.45)
            else:
                C.swipe(216, 90, dy=step, steps=30, hold=0.45)
            time.sleep(0.9)
            continue
        if not settle():
            print('%-28s 列表没停稳' % (name or '点行'))
            return False
        c, alive, rs = snap()
        if not alive or len(rs) <= i:
            print('%-28s 停稳后行消失了' % (name or '点行'))
            return False
        x1, y1, x2, y2 = rs[i]
        cx, cy = (x1 + x2) // 2, (y1 + y2) // 2
        if TOP + 8 <= cy <= SCREEN_H - 8:
            C.tap(cx, cy)
            return True
    print('%-28s 行滚不进视口（落在 y=%d）' % (name or '点行', cy))
    return False


def back_to(page, tries=3):
    """退回指定页：按返回键直到 CUR 对上（最多 tries 次）。"""
    for _ in range(tries):
        c, alive, _ = snap()
        if c == page:
            return True
        if not alive:
            return False
        C.tap(*BACK)
        c, alive = wait_cur(page, 1.5)
        time.sleep(0.3)
    return c == page


def go(i, expect, need, name, from_page, tries=3):
    """点第 i 行并等页面真的切过去；点歪了/没点着就退回父页重来。"""
    c, alive, _ = snap()
    for t in range(tries):
        if not back_to(from_page):
            print('%-28s 回不到起点（CUR=%d，要 %d）' % (name, c, from_page))
            return False
        if not tap_row(i, need=need, name=name):
            return False
        c, alive = wait_cur(expect, 2.5)
        if c == expect:
            return report(name, expect, c, alive)
        print('%-28s 第 %d 次没到（CUR=%d）' % (name, t + 1, c))
        time.sleep(0.4)
    return report(name, expect, c, alive)


def main():
    g = C.R.restart()
    if g is None:
        g = C.R.ensure_guest(60)
    if g is None:
        print('guest 起不来')
        return 1
    C.crown('click')
    time.sleep(2)
    C.build('procfs,autoopen,navdbg')
    g, uniq = C.deploy(suffix='nav')
    if uniq is None:
        return 1
    nav = C.find_magic(g, uniq, SNAP_MAGIC)
    if nav is None:
        print('模块 DATA 里没找到导航快照魔数（这版没带 navdbg？）')
        return 1
    NAV[0] = nav
    print('导航快照 @ %08x' % nav)
    ok = True
    ok &= step('进应用（主页）', PAGE_MENU)

    # 主页 8 行：0 CPU / 1 存储 / 2 内存 / 3 应用管理 / 4 表盘管理 / 5 系统美化 / 6 设置 / 7 关于
    ok &= go(3, PAGE_APPS, 8, '点 应用管理', PAGE_MENU)
    ok &= back(PAGE_MENU, '返回 主页')
    ok &= go(4, PAGE_WFMGMT, 8, '点 表盘管理', PAGE_MENU)
    ok &= go(0, PAGE_WATCH, 1, '表盘管理->表盘切换', PAGE_WFMGMT)
    ok &= back(PAGE_WFMGMT, '返回 表盘管理')
    ok &= back(PAGE_MENU, '返回 主页')

    # 设置 6 行：0 时间 / 1 日期 / 2 亮度 / 3 震动 / 4 缓存 / 5 日志
    ok &= go(6, PAGE_SETTINGS, 8, '点 设置', PAGE_MENU)
    for i, name, expect in ((2, '设置->亮度控制', PAGE_BRIGHT),
                            (3, '设置->震动', PAGE_VIB),
                            (4, '设置->缓存清理', PAGE_CACHE),
                            (5, '设置->运行日志', PAGE_LOG)):
        ok &= go(i, expect, 6, name, PAGE_SETTINGS)
        ok &= back(PAGE_SETTINGS, '返回 设置')
    ok &= back(PAGE_MENU, '返回 主页')

    ok &= go(5, 11, 6, '点 系统美化', PAGE_MENU)
    # 10P 同款独立二级页：更换字体 / 桌面图标 / 系统模糊 / 模糊区域。
    ok &= go(0, 6, 6, '系统美化->更换字体', 11)
    ok &= back(11, '字体返回菜单')
    ok &= go(1, 6, 3, '系统美化->桌面图标', 11)
    ok &= back(11, '图标返回菜单')
    ok &= go(2, 6, 8, '系统美化->系统模糊', 11)
    if tap_row(6, need=8, name='模糊区域'):
        time.sleep(1.0)
        ok &= report('系统模糊->模糊区域', 6, *snap()[:2])
        print(C.shot('p65_zone_page'))
    else:
        ok = False
    ok &= back(6, '回到系统模糊')
    ok &= back(11, '回到系统美化')
    ok &= back(PAGE_MENU, '返回 主页')

    ok &= go(7, PAGE_ABOUT, 8, '点 关于', PAGE_MENU)
    ok &= back(PAGE_MENU, '返回 主页')

    print('结论:', '全过' if ok else '有不符项')
    return 0 if ok else 1


if __name__ == '__main__':
    sys.exit(main())
