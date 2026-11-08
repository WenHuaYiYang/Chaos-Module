#!/usr/bin/env python3
"""出 RW6 原生的"Chaos 安装器表盘"容器。

口径（全部取自固件自带的那张会跑 Lua 的表盘，不做外推）：
  样本 fw/rw6_watchface_mood.bin（vela_resource.bin 的 0x1FF3600 处抠出）。
  头 0x00 = 魔数 0x1234A55A，0x04 = 版本 0x00010133，0x10 = 记录表尾，
  0x20 = 预览块起点，0x28 = 12 字节 pkgName（就是表盘 id 本身）。
  0x0..0xA477 是头 + widget 表 + 预览块 + 主题图，整段照抄不动。
  0xA477 起是文件链，每条 = [u24 数据长][u8 名字长][16 字节 0][名字][数据]，
  最后一条的数据正好落在文件尾，所以这条链自描述、没有中心表。

渲染节点是从包体内嵌的 widget 表建的（不是扫 _lua/），所以本脚本只替换
_lua/mood/mood.lua 这一条的内容、并把要投递的文件追加在链尾，
其余 14 条与整段前缀逐字节保留 —— 那张表指的就是这个文件名。

产物: dist/chaos-installer-rw6.bin
"""
import hashlib
import io
import os
import struct
import sys
import tarfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SHELL = os.path.join(ROOT, 'fw', 'rw6_watchface_mood.bin')
TAR = os.path.join(ROOT, 'dist', 'chaos-p65-043-v1.tar')
LUA = os.path.join(ROOT, 'work', 'p65_chaos', 'installer', 'chaos.lua')
OUT = os.path.join(ROOT, 'dist', 'chaos-installer-rw6.bin')

RECORD_LEN = 20
MAGIC = 0x1234A55A
# 样本里第一条文件记录的位置；脚本会自己扫一遍并核对，对不上就停。
FIRST_RECORD = 0xA477
# widget 表指向的脚本名，换掉就没有任何渲染节点会跑我们的脚本
SCRIPT_NAME = b'_lua/mood/mood.lua'
PKG_NAME = b'979820261101'


class BuildError(Exception):
    pass


def _assert(cond, msg):
    if not cond:
        raise BuildError(msg)


def read_records(blob, start):
    """从 start 起逐条走文件链，返回 [(记录偏移, 名字, 数据)]。"""
    out = []
    p = start
    while p < len(blob):
        data_len = blob[p] | (blob[p + 1] << 8) | (blob[p + 2] << 16)
        name_len = blob[p + 3]
        _assert(4 <= name_len <= 60, '名字长度 %#x 处不像记录' % p)
        _assert(blob[p + 4:p + RECORD_LEN] == b'\0' * 16, '记录保留位非零 @%#x' % p)
        name = blob[p + RECORD_LEN:p + RECORD_LEN + name_len]
        _assert(all(32 <= c < 127 for c in name), '名字不可打印 @%#x' % p)
        d0 = p + RECORD_LEN + name_len
        d1 = d0 + data_len
        _assert(d1 <= len(blob), '%s 的数据越界' % name)
        out.append((p, name, blob[d0:d1]))
        p = d1
    _assert(p == len(blob), '文件链没有正好走到文件尾（停在 %#x，全文件 %#x）' % (p, len(blob)))
    return out


def pack_record(name, data):
    _assert(len(data) < (1 << 24), '%s 超过 16MB' % name)
    _assert(4 <= len(name) <= 60, '名字长度必须落在 4..60')
    head = bytearray(RECORD_LEN)
    head[0:3] = struct.pack('<I', len(data))[:3]
    head[3] = len(name)
    return bytes(head) + name + data


def find_first_record(blob):
    """扫出第一条文件记录的位置（不写死）。"""
    for p in range(0x100, len(blob) - RECORD_LEN):
        data_len = blob[p] | (blob[p + 1] << 8) | (blob[p + 2] << 16)
        name_len = blob[p + 3]
        if not (4 <= name_len <= 60) or data_len == 0 or data_len > len(blob):
            continue
        if blob[p + 4:p + RECORD_LEN] != b'\0' * 16:
            continue
        name = blob[p + RECORD_LEN:p + RECORD_LEN + name_len]
        if name.startswith(b'_lua/') and all(32 <= c < 127 for c in name):
            return p
    raise BuildError('扫不到文件链起点')


def load_payload():
    _assert(os.path.exists(TAR), '缺少交付包 %s（先跑 build_chaos_p65_installer.py）' % TAR)
    tf = tarfile.open(TAR)
    ko = tf.extractfile('chaos_sup_p65.ko').read()
    icon = tf.extractfile('chaos_icon.bin').read()
    tf.close()
    _assert(ko[:4] == b'\x7fELF', '包里的 ko 不是 ELF')
    return ko, icon


def build():
    shell = open(SHELL, 'rb').read()
    magic, ver = struct.unpack_from('<II', shell, 0)
    _assert(magic == MAGIC, '样本容器魔数不对: %#x' % magic)
    start = find_first_record(shell)
    _assert(start == FIRST_RECORD, '文件链起点 %#x 与记录值 %#x 不符' % (start, FIRST_RECORD))
    recs = read_records(shell, start)
    names = [n for _, n, _ in recs]
    _assert(SCRIPT_NAME in names, '样本里没有 %s' % SCRIPT_NAME.decode())

    lua = open(LUA, 'rb').read()
    _assert(len(lua) < 0x80000, '安装器脚本过大')
    ko, icon = load_payload()

    # 前缀逐字节保留：pkgName 换成新号，其余（widget 表/预览/主题图）不动
    prefix = bytearray(shell[:start])
    _assert(len(PKG_NAME) == 12, 'pkgName 必须正好 12 字节')
    prefix[0x28:0x28 + 12] = PKG_NAME

    chain = bytearray()
    replaced = False
    for _, name, data in recs:
        if name == SCRIPT_NAME:
            chain += pack_record(name, lua)
            replaced = True
        else:
            chain += pack_record(name, data)
    _assert(replaced, '替换脚本失败')
    # 投递件挂在同一个目录下，脚本用 SCRIPT_PATH 拼路径读
    base = b'_lua/mood/'
    chain += pack_record(base + b'chaos_sup.bin', ko)
    chain += pack_record(base + b'chaos_icon.bin', icon)

    blob = bytes(prefix) + bytes(chain)
    return blob, ver, len(recs)


def verify(blob):
    """往返：重新按同一套口径走一遍文件链，逐条比对名字与长度。"""
    start = find_first_record(blob)
    recs = read_records(blob, start)
    magic, ver = struct.unpack_from('<II', blob, 0)
    _assert(magic == MAGIC and ver == 0x00010133, '头被改坏')
    _assert(blob[0x28:0x28 + 12] == PKG_NAME, 'pkgName 没换掉')
    got = {n: len(d) for _, n, d in recs}
    _assert(len(got) == len(recs), '有重名条目')
    for want in (SCRIPT_NAME, b'_lua/mood/chaos_sup.bin', b'_lua/mood/chaos_icon.bin'):
        _assert(want in got, '回读缺 %s' % want.decode())
    _assert(got[b'_lua/mood/chaos_sup.bin'] > 0x4000, 'ko 长度可疑')
    return recs


SEED_SH = """# Chaos 安装器表盘（Redmi Watch 6）
# 用法: tar -C /data -x /data/chaos-watchface-rw6.tar ; sh /data/wf-install.sh
#
# 做一件事：把安装器写进内置「心情」表盘的 Lua 槽位。
# 之后长按表盘 -> 选「心情」-> 点「运行」，Chaos 就装好了。
# 这是整台机器上唯一可写且会执行 Lua 的位置；市场表盘那条路要手机在线。

D=/data/app/watchface/builtin/120917365540/_lua/mood
cp /data/wf/mood.lua $D/mood.lua
cp /data/wf/chaos_sup.bin $D/chaos_sup.bin
cp /data/wf/chaos_icon.bin $D/chaos_icon.bin
echo "---- 槽位内容 ----"
ls $D
echo "---- 下一步 ----"
echo "长按表盘 -> 选「心情」 -> 点「运行」"
"""


def build_tar(lua, ko, icon):
    """另出一份播种包：把三个文件摆进内置槽位，之后表盘本身就是安装器。"""
    out = os.path.join(ROOT, 'dist', 'chaos-watchface-rw6.tar')
    files = [
        ('wf', None),
        ('wf/mood.lua', lua),
        ('wf/chaos_sup.bin', ko),
        ('wf/chaos_icon.bin', icon),
        ('wf-install.sh', SEED_SH.encode('utf-8')),
    ]
    with tarfile.open(out, 'w', format=tarfile.USTAR_FORMAT) as tf:
        for name, data in files:
            info = tarfile.TarInfo(name)
            info.mtime = 0
            info.uid = info.gid = 0
            info.uname = info.gname = ''
            if data is None:
                info.type = tarfile.DIRTYPE
                info.mode = 0o755
                tf.addfile(info)
            else:
                info.mode = 0o755 if name.endswith('.sh') else 0o644
                info.size = len(data)
                tf.addfile(info, io.BytesIO(data))
    blob = open(out, 'rb').read()
    print('  播种包: %s (%d 字节)' % (out, len(blob)))
    print('  sha256: %s' % hashlib.sha256(blob).hexdigest())
    return out


def main():
    blob, ver, n = build()
    recs = verify(blob)
    open(OUT, 'wb').write(blob)
    print('  样本条目 %d 条 -> 产出 %d 条' % (n, len(recs)))
    for _, name, data in recs:
        print('    %-34s %8d' % (name.decode(), len(data)))
    print('  产物: %s (%d 字节)' % (OUT, len(blob)))
    print('  sha256: %s' % hashlib.sha256(blob).hexdigest())
    lua = open(LUA, 'rb').read()
    ko, icon = load_payload()
    build_tar(lua, ko, icon)
    return 0


if __name__ == '__main__':
    sys.exit(main())
