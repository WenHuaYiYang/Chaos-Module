#!/usr/bin/env python3
"""romfs 遍历器: 列目录树、按名字筛、导出文件。

用途: RW6 的 218MB 资源镜像是 romfs(头 8 字节 '-rom1fs-', 卷名 resource)。
AP 镜像里那些 launcher 函数名只有串没有代码引用, 怀疑真正的 launcher 代码是
/resource/app/<应用>/ 下的可加载模块, 所以先把这棵树翻开。

romfs 记录布局(全大端): next_unit(4) type|size(4: 高3位类型低29位尺寸) spec_info(4) name(补到16字节)
"""
import struct
import sys

TYPE = {0: 'link', 1: 'dir', 2: 'file', 3: 'symlink', 4: 'blk', 5: 'chr', 6: 'sock'}


def rd(img, off, n):
    return img[off:off + n]


def walk(img, unit, path, out, depth=0, maxdepth=9):
    n = len(img)
    while unit and depth <= maxdepth:
        if unit + 16 > n:
            return
        nxt, tsz, spec = struct.unpack_from('>III', img, unit)
        typ = tsz >> 29
        size = tsz & 0x1FFFFFFF
        name_off = unit + 16
        if name_off >= n:
            return
        end = img.index(b'\0', name_off, min(name_off + 256, n))
        name = img[name_off:end].decode('utf-8', 'replace')
        full = path + '/' + name
        out.append((full, TYPE.get(typ, '?'), size, unit, spec))
        if typ == 1 and spec and spec + 16 <= n:
            walk(img, spec, full, out, depth + 1, maxdepth)
        unit = nxt


def main():
    p = sys.argv[1]
    kw = sys.argv[2] if len(sys.argv) > 2 else 'launcher'
    img = open(p, 'rb').read()
    magic, full_size, cksum = struct.unpack_from('>8sII', img, 0)
    assert magic == b'-rom1fs-', magic
    vol_end = img.index(b'\0', 16)
    print('卷名 %s  全尺寸 %d 字节 (%.1f MB)  实读 %d'
          % (img[16:vol_end].decode(), full_size, full_size / 1e6, len(img)))
    # 卷名(含结尾 NUL)补到 16 字节倍数, 之后第一个单元就是根目录的 file header
    root_unit = 16 + ((vol_end - 16 + 1) + 15) // 16 * 16
    rows = []
    walk(img, root_unit, '', rows)
    print('条目 %d 个' % len(rows))
    top = [r for r in rows if r[0].count('/') <= 2]
    print('--- 前 40 条(深度<=2) ---')
    for full, typ, size, unit, spec in top[:40]:
        print('   %-8s %10d  %s' % (typ, size, full))
    print('--- 名字含 %s 的文件 ---' % kw)
    n = 0
    for full, typ, size, unit, spec in rows:
        if typ == 'file' and kw in full.lower():
            print('   %10d  %s  (单元 0x%X)' % (size, full, unit))
            n += 1
            if n > 40:
                break
    if n == 0:
        print('   (没有)')
    return 0


if __name__ == '__main__':
    sys.exit(main())
