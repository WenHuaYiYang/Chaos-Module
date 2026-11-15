# -*- coding: utf-8 -*-
"""Lua 源码瘦身：只做"零语义变化"的三件事 —— 去注释、去行首缩进、去空行。

为什么不压缩成一行
------------------
把换行也去掉就得在两个 token 之间补空格，而 Lua 里 `a=1` `-b` 换行合并成 `a=1-b`
是合法的、但 `local a\n(b)()` 会和 `local a(b)()` 变成两种意思。少掉的字节不值这个风险。

为什么不用 luac -s
------------------
带里的 Lua 运行时是固件提供的，字节码版本/端序必须与它逐字一致；源码是它一定认的形态。

用法:
    python scripts/minify_lua.py <in.lua> <out.lua>
    python scripts/minify_lua.py --check <in.lua>      # 只打印能省多少
"""

import re
import sys


def strip_source(text: str) -> str:
    out = []
    i = 0
    n = len(text)
    # 第一行若是 shebang，原样保留（Lua 只允许它出现在第 0 行）。
    if text.startswith('#'):
        e = text.find('\n')
        if e < 0:
            return text
        out.append(text[:e])
        i = e
    while i < n:
        c = text[i]
        # 长括号：[[ .. ]] / [==[ .. ]==]
        m = re.match(r'\[(=*)\[', text[i:])
        if m:
            close = ']' + m.group(1) + ']'
            end = text.find(close, i + m.end())
            if end < 0:
                out.append(text[i:])
                break
            out.append(text[i:end + len(close)])
            i = end + len(close)
            continue
        # 短字符串
        if c in ('"', "'"):
            j = i + 1
            while j < n:
                if text[j] == '\\':
                    j += 2
                    continue
                if text[j] == c:
                    j += 1
                    break
                j += 1
            out.append(text[i:j])
            i = j
            continue
        # 注释：--[[ 长注释 ]] 或 -- 行注释
        if text.startswith('--', i):
            m = re.match(r'--\[(=*)\[', text[i:])
            if m:
                close = ']' + m.group(1) + ']'
                end = text.find(close, i + m.end())
                i = n if end < 0 else end + len(close)
                continue
            e = text.find('\n', i)
            i = n if e < 0 else e
            continue
        out.append(c)
        i += 1

    body = ''.join(out)
    lines = []
    for line in body.split('\n'):
        s = line.strip()
        if not s:
            continue
        lines.append(s)
    return '\n'.join(lines) + '\n'


def tokens(text: str):
    """把源码切成 (kind, text) 序列：字符串与其它 token 分开，注释丢弃。
    用来证明瘦身只丢了注释和空白，token 序列逐个相同。"""
    out = []
    i = 0
    n = len(text)
    while i < n:
        c = text[i]
        m = re.match(r'\[(=*)\[', text[i:])
        if m:
            close = ']' + m.group(1) + ']'
            end = text.find(close, i + m.end())
            end = n if end < 0 else end + len(close)
            out.append(('str', text[i:end]))
            i = end
            continue
        if c in ('"', "'"):
            j = i + 1
            while j < n:
                if text[j] == '\\':
                    j += 2
                    continue
                if text[j] == c:
                    j += 1
                    break
                j += 1
            out.append(('str', text[i:j]))
            i = j
            continue
        if text.startswith('--', i):
            m = re.match(r'--\[(=*)\[', text[i:])
            if m:
                close = ']' + m.group(1) + ']'
                end = text.find(close, i + m.end())
                i = n if end < 0 else end + len(close)
            else:
                e = text.find('\n', i)
                i = n if e < 0 else e
            continue
        if c.isspace():
            i += 1
            continue
        if c.isalnum() or c == '_':
            j = i
            while j < n and (text[j].isalnum() or text[j] == '_' or text[j] == '.'):
                j += 1
            out.append(('word', text[i:j]))
            i = j
            continue
        out.append(('op', c))
        i += 1
    return out


def main() -> int:
    args = [a for a in sys.argv[1:] if not a.startswith('--')]
    if not args:
        print(__doc__)
        return 2
    src = args[0]
    text = open(src, encoding='utf-8').read()
    slim = strip_source(text)
    before = len(text.encode('utf-8'))
    after = len(slim.encode('utf-8'))
    if '--tokens' in sys.argv:
        same = tokens(text) == tokens(slim)
        print('token 序列%s (原 %d / 瘦 %d 个 token)' % (
            '完全一致' if same else '不一致', len(tokens(text)), len(tokens(slim))))
        return 0 if same else 1
    if '--check' in sys.argv or len(args) < 2:
        print('%s: %d -> %d bytes (-%d, %.0f%%)' % (src, before, after, before - after,
                                                    100.0 * (before - after) / max(before, 1)))
        return 0
    with open(args[1], 'w', encoding='utf-8', newline='\n') as fh:
        fh.write(slim)
    print('%s -> %s: %d -> %d bytes (-%d)' % (src, args[1], before, after, before - after))
    return 0


if __name__ == '__main__':
    sys.exit(main())
