#!/usr/bin/env python3
"""p65_chaos 全链路冒烟：桌面图标 -> 点开 -> 主菜单 -> 应用管理 -> 滚动。

用法: python work/p65_chaos/smoke2.py [--no-build]

每一步都落一张截图到 build/shot_sm2_*.png，判据是「截图里看见了什么」，
不是「命令返回成功」。
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


def icon_visible(png):
    """桌面左上格有没有我们的图标（那个区域有非黑像素就算有）。"""
    from PIL import Image
    im = Image.open(png).convert('RGB')
    n = 0
    for px in im.crop((20, 40, 116, 136)).getdata():
        if px != (0, 0, 0):
            n += 1
    return n > 1500


def main():
    if '--no-build' not in sys.argv:
        chaos_guest.build()   # 不带 autoopen：停在桌面，才能验「点桌面图标」这一步
    g = R.restart()
    if g is None:
        print('guest 起不来')
        return 1
    print('guest: ok')
    time.sleep(5)
    # 表盘上按一下表冠 = 进桌面应用页（屏幕同时被唤醒）
    chaos_guest.crown('click')
    time.sleep(2.5)
    # 先把应用图标传到 /data（描述符里的图标路径指的就是它）
    icon = os.path.join(HERE, 'chaos_icon.bin')
    if os.path.exists(icon):
        print('图标上传:', R.upload(g, icon, 'chaos_icon.bin'))
        time.sleep(2)
    g, uniq = chaos_guest.deploy('p65_chaos.ko', 'sm2')
    if g is None or not uniq:
        print('部署失败')
        return 1
    time.sleep(4)
    # 表冠有时进控制中心、有时进桌面应用页；按到看见图标为止
    png = R.screendump('sm2_desk')
    for _ in range(6):
        if icon_visible(png):
            break
        chaos_guest.crown('click')
        time.sleep(2.5)
        png = R.screendump('sm2_desk')
    print('1 桌面(应有 Chaos 图标) ->', png, '图标可见:', icon_visible(png))
    # 点桌面上的 Chaos 图标（左上格）
    chaos_guest.tap(68, 88)
    time.sleep(2.5)
    print('2 点图标进 Chaos ->', R.screendump('sm2_open'))
    # 主菜单第一行 = 应用管理
    chaos_guest.tap(150, 125)
    time.sleep(2)
    print('3 应用管理 ->', R.screendump('sm2_apps'))
    # 页内上滑
    chaos_guest.swipe(216, 400, dy=-200, steps=14)
    time.sleep(1)
    print('4 滚动 ->', R.screendump('sm2_scroll'))
    print('模块:', uniq)
    return 0


if __name__ == '__main__':
    sys.exit(main())
