#!/usr/bin/env python3
"""打 p65（Redmi Watch 6）安装包：tar.gz 里放内核模块 + 图标 + 安装/卸载脚本。

设备侧是 NuttX 的 NSH：自带 tar 与 sh，所以交付物是一个 tar 包，
用户把包丢进 /data 后两行命令就能装上：

    tar -C /data -x /data/chaos-p65-043-v1.tar
    sh /data/install.sh

注意两个设备侧的限制（都实测过）：
  * NuttX 的 tar 只吃 USTAR 格式，PAX 会报 "tar_extract_all(): Unknown error 22"；
  * 设备上的 tar 不支持 -z，要装 .tar 而不是 .tar.gz。

为什么不是 043 那套容器：043 是 Vela 固件，表盘容器由固件自己解包安装；
RW6 是 NuttX，没有容器安装器，模块靠 insmod 装载（e_entry 必须补，
见 scripts/ko_entry_patch.py）。
"""
import hashlib
import os
import shutil
import subprocess
import sys
import tarfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "work", "p65_chaos")
DIST = os.path.join(ROOT, "dist")
STAGE = os.path.join(ROOT, "build", "p65_pkg")
VERSION = "043-v1"

INSTALL_SH = """# Chaos（Redmi Watch 6 / p65）安装脚本
# 用法: sh /data/install.sh
# 前提: 安装包已经解压到 /data

insmod /data/chaos_sup_p65.ko chaos_p65
echo "---- 装完核对 ----"
lsmod
"""

UNINSTALL_SH = """# Chaos（Redmi Watch 6 / p65）卸载脚本
# 用法: sh /data/uninstall.sh

rmmod chaos_p65
echo "---- 卸载后核对 ----"
lsmod
"""

README = """Chaos for Redmi Watch 6 (p65)
=============================

包内文件
--------
  chaos_sup_p65.ko   内核模块（Chaos 主程序，NuttX modlib 格式，e_entry 已补）
  chaos_icon.bin     桌面图标
  install.sh         安装脚本
  uninstall.sh       卸载脚本

安装（手表 NSH 里执行）
-----------------------
  tar -C /data -x /data/chaos-p65-043-v1.tar
  sh /data/install.sh

装完 lsmod 里应出现一行 chaos_p65；桌面出现 Chaos 图标，点开即用。

功能（主页从上往下）
--------------------
  CPU / 存储(带占用条) / 内存(带占用条)
  应用管理 / 表盘管理 / 系统美化 / 设置 / 关于

  设置:     时间设置(滚轮选时/分，保存写系统时钟) / 日期设置(滚轮选月/日) /
            亮度控制 / 震动 / 缓存清理 / 运行日志
  表盘管理: 表盘切换 (显示内置表盘开关 / 表盘真名+ID / 当前项打勾 / 每页 8 条"更多")
  系统美化: 更换字体(系统字体清单/点选即换/重启恢复) /
            桌面图标(图标包清单，系统原图标与投递包) / 系统模糊 / 模糊区域 /
            重启恢复字体和图标 / 重启恢复模糊 / 返回
  亮度控制: 当前亮度 / 两档 / 自动亮度(系统原生开关)
  震动:     滑动列表震动(系统原生开关) / 测试震动 / 强度
  运行日志: 更早一页 / 更新一页 / 刷新 / 清空日志(两下确认) / 每页 4 条

卸载
----
  sh /data/uninstall.sh

说明
----
  1. 模块靠 insmod 装载，不是开机自启；重启手表后需要重跑 install.sh。
     RW6 有 Lua 表盘执行通道（_lua/<名>/<名>.lua + os.execute 已实测可用），
     但新表盘怎么注册进表盘表还没打通，所以本包暂时仍用 tar 安装。
  2. 模块只读写 /data 下的缓存目录（/data/cache、/data/quickapp/cache、
     /data/log、/data/offlinelog），不碰 /data/fitness/cache。
  3. 表盘切换是"下次开机生效"（固件的行为），页面会提示。
  4. 换机/换固件版本时模块与固件版本绑定：固件版本变了要重新出包。
"""


def run(cmd, **kw):
    subprocess.run(cmd, check=True, **kw)


def main():
    os.makedirs(DIST, exist_ok=True)
    if os.path.isdir(STAGE):
        shutil.rmtree(STAGE)
    os.makedirs(STAGE)

    print("[1/4] 编译模块 ...")
    run(["bash", os.path.join(SRC, "build.sh")])
    ko = os.path.join(SRC, "p65_chaos.ko")
    run([sys.executable, os.path.join(ROOT, "scripts", "ko_entry_patch.py"), ko])

    print("[2/4] 摆包 ...")
    shutil.copy2(ko, os.path.join(STAGE, "chaos_sup_p65.ko"))
    icon = os.path.join(SRC, "chaos_icon.bin")
    if os.path.exists(icon):
        shutil.copy2(icon, os.path.join(STAGE, "chaos_icon.bin"))
    else:
        print("  警告: 没有 %s，桌面图标会缺" % icon)
    # 配置目录由 tar 目录条目创建：安装器里的 mkdir 在目录已存在时会中途中止脚本。
    os.makedirs(os.path.join(STAGE, "chaos", "font"), exist_ok=True)
    os.makedirs(os.path.join(STAGE, "chaos", "icons"), exist_ok=True)
    for name, text in (("install.sh", INSTALL_SH), ("uninstall.sh", UNINSTALL_SH),
                       ("README.txt", README)):
        with open(os.path.join(STAGE, name), "w", encoding="utf-8", newline="\r\n") as f:
            f.write(text)

    print("[3/4] 打包 ...")
    # 设备侧的 tar 只认 USTAR：Python 默认写 PAX，NuttX 解 PAX 会报
    # "tar_extract_all(): Unknown error 22"。也不压缩：设备上的 tar 不带 -z，
    # 要先 gzip -d 才行，多一步不如直接给 .tar。
    out = os.path.join(DIST, "chaos-p65-%s.tar" % VERSION)
    if os.path.exists(out):
        os.remove(out)
    with tarfile.open(out, "w", format=tarfile.USTAR_FORMAT) as tf:
        for name in sorted(os.listdir(STAGE)):
            tf.add(os.path.join(STAGE, name), arcname=name)

    print("[4/4] 校验 ...")
    with tarfile.open(out, "r") as tf:
        names = sorted(tf.getnames())
    data = open(out, "rb").read()
    print("  包内容:", names)
    print("  产物: %s (%d 字节)" % (out, len(data)))
    print("  sha256: %s" % hashlib.sha256(data).hexdigest())


if __name__ == "__main__":
    main()
