#!/usr/bin/env python3
"""用 043 已定名的函数当模板，在 RW6 镜像里找同形函数。

为什么这么做: RW6 的名字表是"死"的(名字字符串没有任何 VA 引用)，find_fn_by_name 那套不成立，
Ghidra 建出来的 18005 个函数全叫 FUN_xxxxxxxx，两代偏移也不能直接对齐。但两代是同一套
launcher 框架, 滚动回调这种函数的**指令形状**几乎不变, 只有 BL 目标与字面量池地址会变。
所以比较时把分支类指令与立即数打掩码, 按形状滑窗找最优命中。

用法: python find_twin.py <043里的文件偏移hex> [窗口半字数]
"""
import sys

A43 = 'fw_pkg_043/vela_ap.bin'
ARW6 = 'fw_pkg_rw6/vela_ap.bin'


def norm(img, off, n):
    """取 n 个半字并掩码: 16 位分支(B/B1/BLX)整条置 0xFFFF;
    32 位里 BL 的第二半字与所有含立即数的第二半字置 0xFFFF。"""
    import struct
    hw = struct.unpack_from('<%dH' % n, img, off)
    out = []
    i = 0
    while i < n:
        h0 = hw[i]
        if i + 1 < n:
            h1 = hw[i + 1]
        else:
            h1 = 0
        if h0 >= 0xE800:                      # 32 位前缀
            if (h0 & 0xF800) == 0xF000 and (h1 & 0xD000) == 0xD000:
                out += [0xF000, 0xFFFF]       # BL/BC 族: 保留族位, 掩掉偏移
            elif (h0 & 0xFB7F) == 0xF85F:
                out += [h0, h1]               # ldr.w pc,[pc] 壳: 形状有意义, 留着
            else:
                out += [h0, h1 if (h1 & 0xF000) != 0xA000 else h1]
            i += 2
        else:
            if (h0 & 0xE000) == 0xE000:       # B(短)
                out.append(0xE000)
            elif (h0 & 0xF800) == 0xB000:     # ADD/SUB SP, #imm
                out.append(0xB000)
            elif (h0 & 0xF800) == 0xB800:     # CBZ/CBNZ: 含偏移
                out.append(0xB800)
            elif (h0 & 0xF000) == 0xD000:     # B(条件)
                out.append(0xD000)
            else:
                out.append(h0)
            i += 1
    return out[:n]


def main():
    base = int(sys.argv[1], 16)
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 40
    a43 = open(A43, 'rb').read()
    arw6 = open(ARW6, 'rb').read()
    tpl = norm(a43, base, n)
    if len(tpl) < n:
        print('模板取不满', n, '个半字')
        return 1
    # 只扫 RW6 的代码段(0x2E44 起, 到 0xAD0000), 半字对齐
    lo, hi = 0x2E44, min(len(arw6) - 2 * n, 0xAD0000)
    best = []
    for off in range(lo, hi, 2):
        h0 = int.from_bytes(arw6[off:off + 2], 'little')
        if h0 != tpl[0]:
            continue
        cand = norm(arw6, off, n)
        score = sum(1 for x, y in zip(tpl, cand) if x == y)
        best.append((score, off))
    best.sort(reverse=True)
    print('模板: 043 file 0x%X  (VA 0x%X)  %d 半字' % (base, base + 0x0C0C0000, n))
    print('候选(满分 %d):' % n)
    for score, off in best[:8]:
        print('   RW6 file 0x%06X  VA 0x%08X  命中 %d/%d' % (off, off + 0x0C0C0000, score, n))
    if not best:
        print('   没有同首半字的候选')
    return 0


if __name__ == '__main__':
    sys.exit(main())
