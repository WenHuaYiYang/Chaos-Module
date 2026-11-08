#!/usr/bin/env python3
"""把 p65_chaos.ko 部署进 guest 并截图。

用法: python work/p65_chaos/run.py [--no-build]
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import chaos_guest  # noqa: E402


def main():
    if '--no-build' not in sys.argv:
        chaos_guest.build()
    g, uniq = chaos_guest.deploy()
    if g is None:
        return 1
    chaos_guest.shot('chaos_boot')
    if uniq:
        print('模块:', uniq)
    return 0


if __name__ == '__main__':
    sys.exit(main())
