#!/usr/bin/env python3
"""桌面入口冒烟：不自动打开，靠桌面图标进 Chaos，再走应用管理。

用法: python work/p65_chaos/smoke4.py [--no-build]

第一张图必须是我们自己的图标（不是系统图标），否则判据不成立。
"""
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(ROOT, 'work', 'probe_p65'))
import chaos_guest  # noqa: E402
import run as R  # noqa: E402

ICON_BOX = (20, 40, 116, 136)
ROW_APPS_Y = 470
SECOND_APP_Y = 258


def icon_noise(png):
    from PIL import Image
    im = Image.open(png).convert('RGB')
    return sum(1 for px in im.crop(ICON_BOX).getdata() if px != (0, 0, 0))


def main():
    if '--no-build' not in sys.argv:
        chaos_guest.build()
    g = R.restart()
    if g is None:
        print('guest 起不来')
        return 1
    print('guest: ok')
    time.sleep(6)
    chaos_guest.crown('click')
    time.sleep(2.5)
    icon = os.path.join(HERE, 'chaos_icon.bin')
    if os.path.exists(icon):
        print('图标上传:', R.upload(g, icon, 'chaos_icon.bin'))
    g, uniq = chaos_guest.deploy('p65_chaos.ko', 'sm4')
    if g is None or not uniq:
        print('部署失败')
        return 1
    time.sleep(5)
    png = R.screendump('sm4_desk')
    for _ in range(6):
        if icon_noise(png) > 1500:
            break
        chaos_guest.crown('click')
        time.sleep(2.5)
        png = R.screendump('sm4_desk')
    print('1 桌面 ->', png, '首格非黑像素:', icon_noise(png))
    chaos_guest.tap(68, 88)
    time.sleep(3)
    print('2 点首格 ->', R.screendump('sm4_open'))
    chaos_guest.tap(150, ROW_APPS_Y)
    time.sleep(2)
    print('3 应用管理 ->', R.screendump('sm4_apps'))
    chaos_guest.swipe(216, 430, dy=-260, steps=16)
    time.sleep(1)
    print('4 列表滚动 ->', R.screendump('sm4_scroll'))
    chaos_guest.tap(60, 28)
    time.sleep(2)
    print('5 返回主页 ->', R.screendump('sm4_back'))
    print('模块:', uniq)
    return 0


if __name__ == '__main__':
    sys.exit(main())
