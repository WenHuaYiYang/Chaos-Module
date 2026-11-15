#!/usr/bin/env python3
"""p65_chaos 交互冒烟：唤醒 -> 装进去 -> 固件自己打开 -> 点第一行切页 -> 再点切回。

用法: python work/p65_chaos/smoke.py [--no-build]
每步落一张截图到 build/shot_csmoke_*.png。

触摸必须走 QMP 绝对鼠标：这台 guest 的 VNC 指针通道发出去不生效
（同一坐标用 VNC 点标题栏返回箭头毫无反应，QMP abs+btn 一次就退）。
屏幕睡着时 open_self 不会打开应用，先按一次表冠。
"""
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE)
import chaos_guest  # noqa: E402


def main():
    if '--no-build' not in sys.argv:
        chaos_guest.build('autoopen')
    print('crown:', chaos_guest.crown('click'))
    time.sleep(1.5)
    g, uniq = chaos_guest.deploy()
    if g is None or not uniq:
        return 1
    print('boot   ->', chaos_guest.shot('csmoke_boot'))
    chaos_guest.tap(216, 146)
    time.sleep(0.8)
    print('about  ->', chaos_guest.shot('csmoke_about'))
    chaos_guest.tap(216, 146)
    time.sleep(0.8)
    print('back   ->', chaos_guest.shot('csmoke_back'))
    print('模块:', uniq)
    return 0


if __name__ == '__main__':
    sys.exit(main())
