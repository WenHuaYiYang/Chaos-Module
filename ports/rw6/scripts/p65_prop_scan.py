#!/usr/bin/env python3
"""扫 p65 里直接调用 local-style-prop 漏斗的调用点，把硬编码的属性号捞出来。

漏斗 = file 0x18420（`lv_obj_set_local_style_prop`，SRAM 副本 0x2017E408）。
属性号在 `bl` 之前由 `movs r1,#imm` / `movw r1,#imm` 写入；有的包装器先把 value
存进 r12（`mov ip,r1`），再 `movs r1,#imm`，最后 `mov r2,ip` —— 这条也认。

用途：属性号不许凭 LVGL 版本猜（p65 与 043 差一个：X/Y/ALIGN 在 p65 是 8/9/10），
一律从固件自己的调用点统计出来，再用"同一函数里成对出现的属性号"认语义。

用法：
  python scripts/p65_prop_scan.py                 # 频次榜
  python scripts/p65_prop_scan.py --pairs         # 同一函数里成对出现的属性号
  python scripts/p65_prop_scan.py --prop 1 4      # 指定属性号的调用点
  python scripts/p65_prop_scan.py --func 0x18420  # 换漏斗地址（file offset）
"""
import os
import sys
from collections import Counter, defaultdict

from capstone import Cs, CS_ARCH_ARM, CS_MODE_THUMB, CS_MODE_MCLASS

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
IMG = os.path.join(ROOT, 'fw', 'p65_100_043', 'vela_ap.bin')
FUNNEL = 0x18420


def load():
    data = open(IMG, 'rb').read()
    cs = Cs(CS_ARCH_ARM, CS_MODE_THUMB | CS_MODE_MCLASS)
    cs.skipdata = True
    return data, cs


def scan(data, cs, funnel):
    """返回 {调用点: (属性号, 函数起点)}。函数起点按 `push {..,lr}` 近似回溯。"""
    calls = []
    # 整段一次性喂 capstone 会 OOM（11MB + skipdata），按 1MB 分块，块尾多给 4 字节
    step = 0x100000
    for base in range(0, len(data), step):
        chunk = data[base:base + step + 4]
        for ins in cs.disasm(chunk, base):
            if ins.address >= base + step:
                break
            if ins.mnemonic == 'bl' and ins.op_str.startswith('#'):
                try:
                    tgt = int(ins.op_str[1:], 0)
                except ValueError:
                    continue
                if tgt == funnel:
                    calls.append(ins.address)
    out = {}
    for addr in calls:
        prop = None
        # 往回看 12 条：取最后一条写 r1 的 movs/movw
        start = max(0, addr - 64)
        win = list(cs.disasm(data[start:addr], start))
        for ins in reversed(win[-12:]):
            if ins.mnemonic in ('movs', 'mov') and ins.op_str.startswith('r1, #'):
                try:
                    prop = int(ins.op_str.split('#')[1], 0)
                except ValueError:
                    prop = None
                break
            if ins.mnemonic == 'movw' and ins.op_str.startswith('r1, #'):
                try:
                    prop = int(ins.op_str.split('#')[1], 0)
                except ValueError:
                    prop = None
                break
        # 函数起点：往回找最近一条 push 含 lr
        fn = None
        start2 = max(0, addr - 0x400)
        for ins in reversed(list(cs.disasm(data[start2:addr], start2))):
            if ins.mnemonic == 'push' and 'lr' in ins.op_str:
                fn = ins.address
                break
        if prop is not None:
            out[addr] = (prop, fn)
    return out


def main():
    data, cs = load()
    funnel = FUNNEL
    args = sys.argv[1:]
    for i, a in enumerate(args):
        if a == '--func' and i + 1 < len(args):
            funnel = int(args[i + 1], 0)
    print('漏斗 file %#x' % funnel)
    calls = scan(data, cs, funnel)
    print('调用点 %d 个，拿到属性号的 %d 个' % (sum(1 for _ in calls), len(calls)))
    if '--pairs' in args:
        byfn = defaultdict(list)
        for addr, (prop, fn) in calls.items():
            byfn[fn].append(prop)
        for fn, props in sorted(byfn.items(), key=lambda kv: str(kv[0])):
            if len(props) >= 2:
                print('fn %#x: %s' % (fn or 0, ' '.join('%#x' % p for p in props)))
        return
    if '--prop' in args:
        idx = args.index('--prop')
        want = set(int(x, 0) for x in args[idx + 1:] if not x.startswith('-'))
        for addr, (prop, fn) in sorted(calls.items()):
            if prop in want:
                print('prop %#x  call %#x  fn %#x' % (prop, addr, fn or 0))
        return
    c = Counter(p for p, _ in calls.values())
    for prop, n in c.most_common():
        print('prop %#4x  x%d' % (prop, n))


if __name__ == '__main__':
    main()
