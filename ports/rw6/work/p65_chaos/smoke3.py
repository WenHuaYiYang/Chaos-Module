#!/usr/bin/env python3
"""p65 功能冒烟：主页数据行 -> 应用管理 -> 返回 -> 点行打开应用。

用法: python work/p65_chaos/smoke3.py [--no-build]

每一张截图都落盘，判据是画面里出现了什么。
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

ROW_APPS_Y = 470   # 「应用管理」行：主页五行里排第四(内容 y=398..518)，只露下半截
SECOND_APP_Y = 258  # 应用页第二行：第一行是本应用自己，代码里跳过自开
BACK_X, BACK_Y = 60, 28


def main():
    if '--no-build' not in sys.argv:
        chaos_guest.build('autoopen')
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
    g, uniq = chaos_guest.deploy('p65_chaos.ko', 'sm3')
    if g is None or not uniq:
        print('部署失败')
        return 1
    time.sleep(4)
    print('1 主页(内核/内存/存储/应用管理) ->', R.screendump('sm3_home'))
    chaos_guest.tap(150, ROW_APPS_Y)
    time.sleep(2)
    print('2 应用管理 ->', R.screendump('sm3_apps'))
    chaos_guest.tap(BACK_X, BACK_Y)
    time.sleep(2)
    print('3 返回键回主页 ->', R.screendump('sm3_back'))
    chaos_guest.tap(150, ROW_APPS_Y)
    time.sleep(2)
    chaos_guest.tap(150, SECOND_APP_Y)
    time.sleep(3)
    print('4 点第二行开系统应用 ->', R.screendump('sm3_launch'))
    print('模块:', uniq)
    return 0


if __name__ == '__main__':
    sys.exit(main())
