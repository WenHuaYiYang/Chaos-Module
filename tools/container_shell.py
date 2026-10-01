# -*- coding: utf-8 -*-
"""容器壳生成器 — 不依赖任何外部模板, 从文件清单直接产出可刷入的表盘容器。

用法:
  python container_shell.py                      # 往返校验默认的自己的几个包
  python container_shell.py <容器.bin> ...        # 校验指定容器(外部作者的容器一并传进来)
  python container_shell.py extract <容器.bin> <预览块.bin>   # 抠出预览块(引导用)

格式事实(逐条对多个来源不同的容器核过):
  容器 = [头部 0x00..0x27][包名 0x28..0x33][填充 0x34..0x67][显示名 0x68..0xA7]
         [主题表 0xA8..0x147][记录表 0x148..rec_end][预览块 rec_end..file_region]
         [文件数据区 file_region..]
  - 头部魔数 0x1234A55A; 0x04 设备码(手环 10 Pro 用 0x10000); 0x10=0x800; 0x14=0x10000;
    0x1C 主题数; 0x20 与 0xAC 都等于记录表结束地址(= 预览块起点)。
  - 记录表首条 = (0, 0, 尾标记记录地址, 0x10); 每条文件记录 = (0x05000000|槽号, 0, 偏移, 长度);
    尾标记 = (0x05000000, 0, 0, 0)。槽号从 0 起, **条数不受 4 限制**(实测样本有 10 条)。
  - 每条文件的头部 20 字节: u32 = (数据长度 & 0xFFFFFF) | (路径字节数 << 24), 再 16 字节 0,
    然后路径 ASCII, 然后数据。整条 = 20 + 路径长 + 数据长。
  - 预览块 = 12 字节头(u32 恒 0x410, u16 宽, u16 高, u32 数据长) + 数据长字节。
    块内数据是压缩缩略图, 生成器按不透明素材对待(只校验块头与长度自洽)。
  - 主题表 0xA8..0xFF = 9 组 (u32, u32): 前 5 组指针指向第一条文件记录, 后 4 组指向尾标记记录;
    偏移 0xD8 那个 u32 = 文件条数; 0x100 起是主题名(默认"样式1")。
重要: 显示名只许写 0x68..0xA7 这 64 字节。越界踩到 0xA8 的主题表会让整包解析失败(真机事故)。
"""
import os
import struct
import sys

MAGIC = 0x1234A55A
DEV_CODE_P67 = 0x10000
PREVIEW_TAG = 0x410
REC_OFF = 0x148
PKG_OFF, PKG_LEN = 0x28, 12
NAME_OFF, NAME_LEN = 0x68, 64
THEME_OFF, THEME_END = 0xA8, 0x148
FILE_CNT_OFF = 0xD8
THEME_NAME_OFF = 0x100
REC_UID_BASE = 0x05000000
BLOB_HDR_LEN = 20
PREVIEW_HDR_LEN = 12
THEME_PTR_SLOTS = 5      # 指向首条文件记录的主题槽数
THEME_TAIL_SLOTS = 4     # 指向尾标记记录的主题槽数

DEFAULT_THEME_NAME = '样式1'


class ShellError(Exception):
    pass


def _assert(cond, msg):
    if not cond:
        raise ShellError(msg)


def blob_for(path_ascii, data):
    """一条文件在容器里的完整记录(含 20 字节小头)。"""
    pb = path_ascii.encode('ascii') if isinstance(path_ascii, str) else path_ascii
    _assert(all(0x20 <= c < 0x7f for c in pb), '路径必须纯 ASCII: %r' % pb)
    _assert(len(data) < (1 << 24), '单文件不得超过 16MB')
    _assert(len(pb) < (1 << 8), '路径过长')
    head = struct.pack('<I', (len(data) & 0xFFFFFF) | (len(pb) << 24))
    return head + b'\x00' * 16 + pb + data


def build_shell(files, pkg_name, display_name, preview, theme_name=DEFAULT_THEME_NAME,
                dev_code=DEV_CODE_P67):
    """files = [(容器内路径, 数据字节)]，preview = 预览块原始字节(含 12 字节头)。"""
    pkg = pkg_name.encode('ascii') if isinstance(pkg_name, str) else pkg_name
    _assert(len(pkg) == PKG_LEN, '包名必须正好 12 字节')
    _assert(all(0x20 <= c < 0x7f for c in pkg), '包名必须是 12 字节可打印 ASCII')
    nb = display_name.encode('utf-8') if isinstance(display_name, str) else display_name
    _assert(len(nb) < NAME_LEN, '显示名超长(上限 %d 字节)' % (NAME_LEN - 1))
    tb = theme_name.encode('utf-8') if isinstance(theme_name, str) else theme_name
    _assert(len(tb) <= 0x148 - THEME_NAME_OFF, '主题名过长')
    _assert(len(preview) >= PREVIEW_HDR_LEN, '预览块太短')
    tag, pw, ph, plen = struct.unpack_from('<IHHI', preview, 0)
    _assert(tag == PREVIEW_TAG, '预览块魔数不对: %#x' % tag)
    _assert(len(preview) == PREVIEW_HDR_LEN + plen, '预览块长度与头里的字段不符')
    _assert(0 < len(files) < 256, '文件条数必须在 1..255(槽号占一个字节)')

    n = len(files)
    rec_end = REC_OFF + 16 * (2 + n)          # 首记录 + n 条 + 尾标记
    first_rec = REC_OFF + 16
    tail_rec = rec_end - 16
    file_region = rec_end + len(preview)

    shell = bytearray(file_region)
    struct.pack_into('<IIII', shell, 0x00, MAGIC, dev_code, 0, 0)
    struct.pack_into('<IIII', shell, 0x10, 0x800, 0x10000, 0, 1)
    struct.pack_into('<II', shell, 0x20, rec_end, 0)
    shell[PKG_OFF:PKG_OFF + PKG_LEN] = pkg
    shell[NAME_OFF:NAME_OFF + len(nb)] = nb
    struct.pack_into('<IIII', shell, THEME_OFF, 0x80000000, rec_end, 1, REC_OFF)
    for i in range(THEME_PTR_SLOTS):
        cnt = n if (THEME_OFF + 16 + 8 * i) == FILE_CNT_OFF else 0
        struct.pack_into('<II', shell, THEME_OFF + 16 + 8 * i, cnt, first_rec)
    for i in range(THEME_TAIL_SLOTS):
        struct.pack_into('<II', shell,
                         THEME_OFF + 16 + 8 * (THEME_PTR_SLOTS + i), 0, tail_rec)
    shell[THEME_NAME_OFF:THEME_NAME_OFF + len(tb)] = tb
    struct.pack_into('<4I', shell, REC_OFF, 0, 0, tail_rec, 0x10)

    pos = file_region
    blobs = []
    for i, (path, data) in enumerate(files):
        blob = blob_for(path, data)
        struct.pack_into('<4I', shell, REC_OFF + 16 * (1 + i),
                         REC_UID_BASE + i, 0, pos, len(blob))
        blobs.append(blob)
        pos += len(blob)
    struct.pack_into('<4I', shell, tail_rec, REC_UID_BASE, 0, 0, 0)
    shell[rec_end:rec_end + len(preview)] = preview
    out = bytes(shell) + b''.join(blobs)
    _assert(len(out) == pos, '布局不自洽: 尾部字段 %d 实际 %d' % (pos, len(out)))
    return out


def parse_container(raw):
    """把容器拆回 (清单, 包名, 显示名, 主题名, 预览块, 头部杂项)，用于校验。"""
    magic, dev, _a, _b = struct.unpack_from('<4I', raw, 0)
    _assert(magic == MAGIC, '魔数不对')
    w800, w10000, _x18, theme_cnt = struct.unpack_from('<4I', raw, 0x10)
    rec_end = struct.unpack_from('<I', raw, 0x20)[0]
    _assert(struct.unpack_from('<I', raw, THEME_OFF + 4)[0] == rec_end, '0xAC 与 0x20 不一致')
    hdr = struct.unpack_from('<4I', raw, REC_OFF)
    _assert(hdr[0] == 0 and hdr[3] == 0x10, '记录表首条形状不符')
    _assert(hdr[2] == rec_end - 16, '首条里的尾标记地址不符')
    files = []
    off = REC_OFF + 16
    while True:
        uid, zero, foff, fsize = struct.unpack_from('<4I', raw, off)
        if uid == REC_UID_BASE and foff == 0:
            break
        _assert(uid >= REC_UID_BASE, '记录 uid 异常 %#x' % uid)
        _assert(zero == 0, '记录第二条字段应为 0')
        packed, = struct.unpack_from('<I', raw, foff)
        plen, dlen = packed >> 24, packed & 0xFFFFFF
        path = raw[foff + BLOB_HDR_LEN:foff + BLOB_HDR_LEN + plen].decode('ascii')
        data = raw[foff + BLOB_HDR_LEN + plen:foff + BLOB_HDR_LEN + plen + dlen]
        _assert(len(data) == dlen, '数据长度与字段不符')
        _assert(fsize == BLOB_HDR_LEN + plen + dlen, '记录长度与小头不符')
        _assert(raw[foff + 4:foff + 16] == b'\x00' * 12, '小头保留字段非 0')
        files.append((path, data))
        off += 16
    preview = raw[rec_end:rec_end + PREVIEW_HDR_LEN]
    tag, pw, ph, plen = struct.unpack_from('<IHHI', preview, 0)
    _assert(tag == PREVIEW_TAG, '预览块魔数不对')
    preview = raw[rec_end:rec_end + PREVIEW_HDR_LEN + plen]
    file_region = rec_end + len(preview)
    _assert(files and struct.unpack_from('<4I', raw, REC_OFF + 16)[2] == file_region,
            '第一条文件偏移不等于预览块之后')
    tail = struct.unpack_from('<4I', raw, off)
    _assert(tail == (REC_UID_BASE, 0, 0, 0), '尾标记不符')
    # 尾部残留: 记录之后到文件区之外不该再有别的东西
    end_of_files = off + 16
    _assert(off + 16 == rec_end, '记录表长度与 0x20 不一致')
    return {
        'files': files,
        'pkg': raw[PKG_OFF:PKG_OFF + PKG_LEN].decode('ascii'),
        'name': raw[NAME_OFF:NAME_OFF + NAME_LEN].split(b'\0')[0].decode('utf-8'),
        'theme': raw[THEME_NAME_OFF:THEME_NAME_OFF + 64].split(b'\0')[0].decode('utf-8'),
        'preview': preview,
        'dev_code': dev,
        'theme_cnt': theme_cnt,
        'file_cnt_field': struct.unpack_from('<I', raw, FILE_CNT_OFF)[0],
    }


def replace_preview(tpl, new_preview):
    """把容器模板里的预览块换成另一块(长度可不同)。

    投递包各自复刻自己表盘界面的预览块(由打包侧生成), 而壳继承
    主包 —— 替换点在记录表结束(0x20 字段)之后: 记录表/主题表/0x20/0xAC 全部在
    预览块**之前**不受影响; 文件区随预览长度平移, 调用方须以替换后的 tpl 重算
    file_region(两个打包脚本的 template_file_region 本就是动态的)。
    """
    _assert(len(new_preview) >= PREVIEW_HDR_LEN, '新预览块太短')
    tag, pw, ph, plen = struct.unpack_from('<IHHI', new_preview, 0)
    _assert(tag == PREVIEW_TAG, '新预览块魔数不对: %#x' % tag)
    _assert(len(new_preview) == PREVIEW_HDR_LEN + plen, '新预览块长度与头字段不符')
    rec_end = struct.unpack_from('<I', tpl, 0x20)[0]
    old_tag, _ow, _oh, olen = struct.unpack_from('<IHHI', tpl, rec_end)
    _assert(old_tag == PREVIEW_TAG, '模板预览块标签不对: %#x' % old_tag)
    return tpl[:rec_end] + new_preview + tpl[rec_end + PREVIEW_HDR_LEN + olen:]


def roundtrip(path):
    """解析 -> 重建 -> 逐字节比对。相同 = 这个容器的壳我们已能自己生成。"""
    raw = open(path, 'rb').read()
    c = parse_container(raw)
    _assert(c['file_cnt_field'] == len(c['files']),
            '主题表 0xD8 不等于文件条数(%d vs %d)' % (c['file_cnt_field'], len(c['files'])))
    mine = build_shell(c['files'], c['pkg'], c['name'], c['preview'],
                       theme_name=c['theme'], dev_code=c['dev_code'])
    if mine == raw:
        return True, '%d 字节 完全一致 (%d 个文件, 预览 %d 字节)' % (
            len(raw), len(c['files']), len(c['preview']))
    n = min(len(mine), len(raw))
    i = next((k for k in range(n) if mine[k] != raw[k]), n)
    return False, '第一个差异在 %#x: 生成 %02x 样本 %02x (长度 %d vs %d)' % (
        i, mine[i], raw[i], len(mine), len(raw))


SAMPLES = [
    'chaos-installer-10p-043-v1.bin',
    'chaos-fontpack-lxgw-rfn.bin',
    'chaos-iconpack-Delta.bin',
]
# 默认样本是我们自己那三个包。它们是构建产物、不入库, 所以在干净克隆上三个都会被跳过 ——
# 那等于什么都没校验, 脚本会按失败退出, 这时把你手上的容器路径传进来即可。
# 这个格式另外还拿两个独立作者的容器(一个 10 个文件槽、一个 1 个槽且缩略图规格不同)
# 验过逐字节相同, 那两个样本同样不在仓库里。


def extract_preview(container_path, out_path):
    """从既有容器里把预览块原样抠出来。只在"还没有自己的缩略图编码器"这一段当引导用。"""
    raw = open(container_path, 'rb').read()
    rec_end = struct.unpack_from('<I', raw, 0x20)[0]
    plen = struct.unpack_from('<I', raw, rec_end + 8)[0]
    block = raw[rec_end:rec_end + PREVIEW_HDR_LEN + plen]
    _assert(len(block) == PREVIEW_HDR_LEN + plen, '预览块越界')
    open(out_path, 'wb').write(block)
    tag, w, h, _l = struct.unpack_from('<IHHI', block, 0)
    print('抽出 %d 字节 -> %s (规格 %dx%d, 头标签 %#x)' % (len(block), out_path, w, h, tag))
    return block


def main(argv):
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    os.chdir(root)
    if len(argv) > 1 and argv[1] == 'extract':
        _assert(len(argv) == 4, '用法: container_shell.py extract <容器.bin> <输出预览块.bin>')
        extract_preview(argv[2], argv[3])
        return 0
    todo = argv[1:] or SAMPLES
    bad = 0
    checked = 0
    skipped = 0
    for p in todo:
        if not os.path.exists(p):
            print('跳过(文件不在): %s' % p)
            skipped += 1
            continue
        checked += 1
        try:
            ok, msg = roundtrip(p)
        except Exception as e:
            ok, msg = False, '解析失败: %s' % e
        print('%-6s %s  %s' % ('一致' if ok else '不一致', p, msg))
        bad += 0 if ok else 1
    if not checked:
        print('\n没有任何样本可校验(全被跳过) —— 判据不成立, 按失败退出')
        return 1
    print('\n%d/%d 个容器的壳可自建' % (checked - bad, checked))
    if skipped:
        print('(另有 %d 个列出的样本不在本地已跳过; 手上的容器传路径进来即可校验)' % skipped)
    return 1 if bad else 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
