#!/usr/bin/env python3
"""给 Chaos 字体投递包补充字形（默认：日文假名）。

背景：第三方字体包的 font.ttf 往往只子集化中文（实测 Phi 包 15437 个汉字、
0 个假名、0 个 emoji），投递后日文/假名显示为空白。

做法：从本机的日文字体（默认 Yu Gothic，Win10/11 自带）里提取假名字形，
按 unitsPerEm 缩放后并入包里的 font.ttf，再按原容器布局重新打包（只替换
最后那个 font.ttf 块，其它块与前缀字节原样保留）。

用法:
  python tools/font_pack_merge.py 原包.bin -o 新包.bin
  python tools/font_pack_merge.py 原包.bin -o 新包.bin --source C:/Windows/Fonts/YuGothM.ttc
  python tools/font_pack_merge.py 原包.bin -o 新包.bin --ranges 3040-3100,FF66-FFA0
"""
import argparse
import struct
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import p67_pack  # noqa: E402

DEFAULT_RANGES = "3040-3100,FF66-FFA0,30FB,30FC"


def parse_ranges(text):
    codes = []
    for part in text.split(','):
        part = part.strip()
        if not part:
            continue
        if '-' in part:
            lo, hi = part.split('-', 1)
            codes.extend(range(int(lo, 16), int(hi, 16) + 1))
        else:
            codes.append(int(part, 16))
    return codes


def extract_ttf(font_pack: Path, workdir: Path) -> Path:
    """从字体包里取出 font.ttf 落盘，返回路径。"""
    buf = font_pack.read_bytes()
    pairs = [struct.unpack_from("<II", buf, p67_pack.PAIR_OFF + i * 8) for i in range(p67_pack.PAIRS)]
    count, script_off = pairs[6]
    for i in range(count):
        off = struct.unpack_from("<I", buf, script_off + i * 16 + 8)[0]
        blk = p67_pack.read_block(buf, off)
        if blk["name"].endswith("font.ttf"):
            out = workdir / "font.ttf"
            out.write_bytes(blk["data"])
            return out
    raise SystemExit("字体包里没有 font.ttf")


def merge(base_path: Path, source_path: Path, codes, workdir: Path) -> Path:
    from fontTools.ttLib import TTFont
    from fontTools.subset import Subsetter, Options
    from fontTools.ttLib.scaleUpem import scale_upem

    base = TTFont(str(base_path))
    upem = base["head"].unitsPerEm
    src = TTFont(str(source_path), fontNumber=0)

    opts = Options()
    opts.layout_features = ["*"]
    opts.name_IDs = ["*"]
    opts.notdef_outline = True
    opts.recommended_glyphs = True
    sub = Subsetter(options=opts)
    sub.populate(unicodes=codes)
    sub.subset(src)
    if src["head"].unitsPerEm != upem:
        scale_upem(src, upem)
    kana_path = workdir / "source_subset.ttf"
    src.save(str(kana_path))

    extra = TTFont(str(kana_path))
    b_cmap = base.getBestCmap()
    k_cmap = extra.getBestCmap()
    b_glyf, k_glyf = base["glyf"], extra["glyf"]
    b_hmtx, k_hmtx = base["hmtx"], extra["hmtx"]

    target = None
    for table in base["cmap"].tables:
        if table.platformID == 3 and table.platEncID == 1:
            target = table
            break
    if target is None:
        raise SystemExit("base 字体缺少 (3,1) cmap 子表")

    def copy_glyph(name, seen):
        if name in b_glyf or name in seen:
            return
        seen.add(name)
        glyph = k_glyf[name]
        if glyph.isComposite():
            for comp in glyph.components:
                copy_glyph(comp.glyphName, seen)
        b_glyf[name] = glyph
        b_hmtx[name] = k_hmtx[name]

    added = 0
    seen = set()
    for code, gname in sorted(k_cmap.items()):
        if code in b_cmap:
            continue
        copy_glyph(gname, seen)
        target.cmap[code] = gname
        added += 1

    out = workdir / "font_merged.ttf"
    base.save(str(out))
    print(f"补充字形 {added} 个（源 {source_path.name}, upem {upem}）")
    return out


def repack(font_pack: Path, merged_ttf: Path, out: Path) -> None:
    buf = bytearray(font_pack.read_bytes())
    pairs = [struct.unpack_from("<II", buf, p67_pack.PAIR_OFF + i * 8) for i in range(p67_pack.PAIRS)]
    count, script_off = pairs[6]

    hit = None
    for i in range(count):
        off = struct.unpack_from("<I", buf, script_off + i * 16 + 8)[0]
        blk = p67_pack.read_block(bytes(buf), off)
        if blk["name"].endswith("font.ttf"):
            hit = (i, off, blk)
            break
    if hit is None:
        raise SystemExit("字体包里没有 font.ttf")
    index, off, blk = hit
    if off + blk["length"] != len(buf):
        raise SystemExit("font.ttf 不是最后一个块，此工具不支持")

    data = merged_ttf.read_bytes()
    name = blk["name"].encode("latin1")
    block = p67_pack._u32(len(data) | (len(name) << 24)) + b"\0" * 16 + name + data
    newbuf = buf[:off] + block
    struct.pack_into("<I", newbuf, script_off + index * 16 + 12, len(block))
    out.write_bytes(newbuf)

    v = p67_pack.inspect(out)
    print(f"新包: {out} ({v['size']} B)")
    for f in v["files"]:
        print(f"  {f['name']}  {f['size']}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("font_pack", type=Path)
    ap.add_argument("-o", "--out", type=Path, required=True)
    ap.add_argument("--source", type=Path, default=Path("C:/Windows/Fonts/YuGothM.ttc"),
                    help="提供补充字形的源字体（默认 Yu Gothic）")
    ap.add_argument("--ranges", default=DEFAULT_RANGES, help="要补充的码位，十六进制，逗号分隔")
    args = ap.parse_args()

    if not args.source.is_file():
        raise SystemExit(f"源字体不存在: {args.source}")
    codes = parse_ranges(args.ranges)
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        base_ttf = extract_ttf(args.font_pack, work)
        print(f"原 font.ttf: {base_ttf.stat().st_size} B")
        merged = merge(base_ttf, args.source, codes, work)
        repack(args.font_pack, merged, args.out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
