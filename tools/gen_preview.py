#!/usr/bin/env python3
"""Generate a Chaos-themed preview block for Chaos-Module/shell/preview.bin.

Format (reverse-engineered from fw_pkg_043/vela_ap.bin, decoder @ RVA 0x0CA91EC4):
  Outer block = LVGL v8 lv_img_dsc_t consumed directly by the vendor decoder:
    +0  u8  cf    = 0x10 (vendor image format, registered in RAM bpp table)
    +1  u8  flags = 0x04 (bits 2..4 == 1 -> "Xiaomi RLE compressed" branch)
    +2  u16 always 0
    +4  u16 width
    +6  u16 height
    +8  u32 data_size (length of the compressed stream)
    +12     stream
  Stream:
    +0  u32 magic 0x5AA521E0
    +4  u32 low nibble = RLE unit size m (1 = 8bpp indexed),
        bits 4..27 = total decompressed size T = 1024 (palette) + w*h (indices)
    +8  RLE stream, byte-oriented (m=1), control byte c then payload:
        c < 0x80 : repeat next 1 byte c times (c == 0: skip next byte)
        c >= 0x80: copy next (c & 0x7F) bytes literally
    Output = 1024B palette (256 x lv_color32_t: B,G,R,A) + w*h palette indices.
"""
import struct, sys, os
from PIL import Image, ImageDraw, ImageFont

W, H = 336, 480
BRAND_BLUE = (0x2A, 0x82, 0xE4)
BRAND_YELLOW = (0xFF, 0xC3, 0x00)
MAGIC = 0x5AA521E0
# 圆角半径: 48px —— 从真实表盘容器(archive/watchface_qiyan_bw.bin)的预览块
# 逐像素解出并最小二乘拟合得到(R=48, 误差 0.8px²), 是系统显示端实际使用的口径。
RADIUS = 48
ICON_PNG = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'chaos.png')

# 预览块用字体: 手环系统字体就是 MiSans-Regular(小米官方免费商用)。字体本体
# 7.9MB 且许可单独一份, 所以不入仓库, 只能按优先级在机器上找。CI 上没有任何
# 中文字体时曾经直接 OSError: cannot open resource, 把整个安装包构建卡死 ——
# 现在一个都找不到就退回 Pillow 内置位图字体: 缩略图上的字会难看, 但构建不该
# 因为"这台机器没装中文字体"而失败。
FONT_CANDIDATES = (
    os.path.join(os.path.dirname(os.path.abspath(__file__)), 'MiSans-Regular.ttf'),
    os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'scripts', 'MiSans-Regular.ttf'),
    r'C:/Windows/Fonts/msyh.ttc',
    r'C:/Windows/Fonts/msyh.ttf',
    '/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc',
    '/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf',
    '/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc',
    '/usr/share/fonts/truetype/wqy/wqy-microhei.ttc',
    '/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc',
    '/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf',
    '/System/Library/Fonts/PingFang.ttc',
)


def preview_font(size):
    """按优先级解析预览字体; 全都没有时退回内置位图字体并告警。"""
    override = os.environ.get('CHAOS_PREVIEW_FONT')
    for path in ((override,) if override else ()) + FONT_CANDIDATES:
        if not path or not os.path.isfile(path):
            continue
        try:
            return ImageFont.truetype(path, size)
        except OSError:
            continue
    print('WARN: 预览块找不到中文字体(可用 CHAOS_PREVIEW_FONT 指定), 退回内置位图字体',
          file=sys.stderr)
    try:
        return ImageFont.load_default(size=size)  # Pillow >= 10.1
    except TypeError:
        return ImageFont.load_default()


# 界面骨架的两套口径, 每个数字都取自创建那块界面的 Lua(不是设计稿):
#   installer —— chaos_installer.lua 的运行界面:
#                 标题 32px @ y_ofs -212 / 状态卡 304x68 @ -150 (边框 #2A2A2C) /
#                 四个按钮 304x64: 运行 @ -76(primary)、注入设置 @ 2(ghost)、
#                 重启 @ 80(ghost)、清除重置 @ 158(danger)
#   pack      —— font_pack.lua 与 icon_pack.lua 的投递界面:
#                 标题 40px @ -170 / 状态卡 304x150 @ -20 / 单按钮 @ 110
# 两套都: 页底 #1C1C1E、卡底 #2C2C2E、圆角 24、状态文字 22px、按钮文字 24px。
LAYOUTS = {
    'installer': dict(title=32, title_y=-212, card_h=68, card_y=-150,
                      line=(0x2A, 0x2A, 0x2C), btn_ys=(-76, 2, 80, 158)),
    'pack': dict(title=40, title_y=-170, card_h=150, card_y=-20,
                 line=(0x3C, 0x3C, 0x3E), btn_ys=(110,)),
}


def make_image(title='Chaos 安装器', status='准备就绪',
               buttons=(('运行', 'primary'), ('注入设置', 'ghost'),
                        ('重启', 'ghost'), ('清除重置', 'danger')),
               layout='installer'):
    """Chaos thumbnail = 对应表盘运行界面的 1:1 复刻。

    几何全部取自 `LAYOUTS[layout]`, 也就是各自 Lua 里的 align y_ofs 与 w/h;
    改 Lua 的界面就要同步改这里, 否则表盘列表里的缩略图与点进去的界面不一致
    (这一条曾经漂过: 安装器从双按钮改成三按钮、卡片从 150 高改成 94 高,
     预览块还画着旧骨架)。
    字体用 MiSans 本体(见下), 最大还原手环观感。
    """
    L = LAYOUTS[layout]
    if len(buttons) != len(L['btn_ys']):
        raise SystemExit('%s 这一套用 %d 个按钮, 传进来 %d 个'
                         % (layout, len(L['btn_ys']), len(buttons)))
    ss = 6  # supersample: 越高圆角边缘的渐变带越细腻(6x 下采样后约 1.5px 过渡)
    img = Image.new('RGBA', (W * ss, H * ss), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    s = lambda v: v * ss
    r = RADIUS * ss
    page = (0x1C, 0x1C, 0x1E)

    # 页面底(圆角 = 屏幕圆角口径)
    d.rounded_rectangle([0, 0, W * ss - 1, H * ss - 1], radius=r, fill=page + (255,))

    font_title = preview_font(s(L['title']))
    font_status = preview_font(s(22))
    font_btn = preview_font(s(24))

    def card(cx, cy, w, h, bg, line, rad=24):
        d.rounded_rectangle(
            [s(cx - w // 2), s(cy - h // 2), s(cx + w // 2), s(cy + h // 2)],
            radius=s(rad), fill=bg + (255,),
            outline=line + (255,), width=max(1, ss // 3))

    # 标题 + 状态卡片 + 卡内状态文字(位置取自 LAYOUTS)
    # 文字必须画在**独立透明图层**再 alpha_composite: 直接在 RGBA 图上 draw.text
    # 时 PIL 会把 glyph 抗锯齿位图的黑色底像素一并写进图像(实测文字边缘出现
    # 一圈 (0,0,0,255) 黑点), over 混合才是正确语义。
    layer = Image.new('RGBA', img.size, (0, 0, 0, 0))
    dl = ImageDraw.Draw(layer)
    dl.text((s(168), s(240 + L['title_y'])), title, font=font_title,
            fill=(0xF2, 0xF2, 0xF2, 255), anchor='mm')
    card(168, 240 + L['card_y'], 304, L['card_h'], (0x2C, 0x2C, 0x2E), L['line'])
    dl.text((s(168), s(240 + L['card_y'])), status, font=font_status,
            fill=(0xB3, 0xB3, 0xB3, 255), anchor='mm')

    # 按钮: 每个按钮的 y_ofs 由 LAYOUTS 给, 与各自 Lua 的 make_button 一一对应
    for (btn_text, btn_style), y_ofs in zip(buttons, L['btn_ys']):
        if btn_style == 'primary':
            # bg 与边框同色, 文字用主文字色
            bgc, linec, fg = (0x25, 0x63, 0xEB), (0x25, 0x63, 0xEB), (0xF2, 0xF2, 0xF2)
        elif btn_style == 'ghost':
            # 表面色就是页底, 只靠 1px 边框认轮廓 —— Lua 里 bg=V_DEEP border=V_LINE2
            bgc, linec, fg = page, L['line'], (0xB3, 0xB3, 0xB3)
        else:  # danger: bg_opa=OPA(15) 的红压在页底上, 无边框, 红字
            bgc = tuple(round(a + (b - a) * 0.15) for a, b in zip(page, (0xFF, 0x45, 0x3A)))
            linec, fg = bgc, (0xFF, 0x45, 0x3A)
        card(168, 240 + y_ofs, 304, 64, bgc, linec)
        dl.text((s(168), s(240 + y_ofs)), btn_text, font=font_btn,
                fill=fg + (255,), anchor='mm')
    img.alpha_composite(layer)

    # 缩放滤镜用 Box(面积平均): LANCZOS 在"白字 vs 暗底"这种强边缘上会振铃下冲,
    # 缝隙色被压到比页底还黑 → 文字周围出现孤立黑点(实测 180 个)。Box 无振铃。
    img = img.resize((W, H), Image.BOX)
    return img


def build_palette_indexed(img):
    """Return (indices bytes, palette list of (B,G,R,A)). Entry 0 = transparent.

    **预览块里不保留半透明**。单一黑 ramp 阶梯是给"整图只有一个圆角前景色"的
    占位图设计的; 界面复刻后图里有蓝按钮/灰卡/红按钮多个圆角对象, 它们的抗锯齿
    边缘全部被映射到同一个黑 ramp → 每个圆角边缘一圈黑晕。

    第二个坑(黑点): PIL 在透明底上画形状后, 边缘像素的 RGB **已经是预乘样子**
    (rgb = 卡色 x 覆盖率), 调色板阶段再做 rgb*a/255 就是二次预乘 → 边缘出现
    近黑孤立像素。所以可见像素直接量化原 RGB, 只有圆角外(含超采样噪声)用透明。

    注意: 这一条**与参考实现不同**。真实表盘与工具占位块的预览块都靠调色板 alpha
    阶梯做边缘抗锯齿(主体 00 00 00 FE, 边缘 31 档 alpha), 可见像素并非全不透明;
    我们改成"可见像素二元不透明 + 抗锯齿预乘进 RGB"。理由是上面那条黑晕, 代价是
    圆角边缘那圈像素变成不透明页底色, 观感要上机看。
    """
    px = img.load()
    opaque = {}
    # 可见门限 16: 保留完整的边缘渐变带(抗锯齿尾巴), 只丢掉真正的角外噪声。
    # 门限过高(64)会把渐变带砍掉一截, 圆角边缘看起来发硬发锐。
    VIS_MIN = 16
    for y in range(H):
        for x in range(W):
            r, g, b, a = px[x, y]
            if a < VIS_MIN:
                continue                      # 圆角外/超采样噪声 → 透明
            # 直接量化预乘 RGB。**不许 un-premultiply**(除回覆盖率)——那会把
            # 页底边缘的"深色 75% 覆盖"像素放大成亮灰白点: 预乘值本身就是显示端
            # alpha 合成的正确结果, 深色列表上直接不透明显示视觉无缝。文字区
            # "黑点"另有真凶(LANCZOS 振铃 + glyph 黑底), 与这里无关。
            opaque[(r, g, b)] = opaque.get((r, g, b), 0) + 1

    # 量化: 5bit/通道 均匀网格, 按出现频次取代表色
    def qkey(rgb):
        return (rgb[0] >> 3, rgb[1] >> 3, rgb[2] >> 3)
    qmap = {}
    for rgb in sorted(opaque, key=opaque.get, reverse=True):
        k = qkey(rgb)
        if k not in qmap:
            qmap[k] = rgb
    colors = list(qmap.values())
    # ensure core colors survive: 页底色 + 图标蓝/黄
    for c in ((0x1C, 0x1C, 0x1E), BRAND_BLUE, BRAND_YELLOW):
        if c not in colors:
            colors.append(c)
    palette = [(0, 0, 0, 0)]  # entry 0 transparent
    for c in colors:
        palette.append((c[2], c[1], c[0], 255))  # store as B,G,R,A
    assert len(palette) <= 256
    pal_index = {qkey(c): i + 1 for i, c in enumerate(colors)}
    indices = bytearray(W * H)
    for y in range(H):
        for x in range(W):
            r, g, b, a = px[x, y]
            if a < VIS_MIN:
                indices[y * W + x] = 0        # 圆角外透明
            else:
                indices[y * W + x] = pal_index[qkey((r, g, b))]
    return bytes(indices), palette


def rle_encode(data):
    out = bytearray()
    i, n = 0, len(data)
    while i < n:
        j = i
        while j < n and data[j] == data[i]:
            j += 1
        run = j - i
        if run >= 3:
            while run > 0:
                c = min(run, 127)
                out.append(c)
                out.append(data[i])
                run -= c
                i += c
        else:
            k = i
            while k < n and k - i < 127:
                if k + 2 < n and data[k] == data[k + 1] == data[k + 2]:
                    break
                k += 1
            assert k > i
            out.append(0x80 | (k - i))
            out += data[i:k]
            i = k
    return bytes(out)


def encode(indices, palette):
    total = 1024 + len(indices)
    pal_bytes = bytearray()
    assert len(palette) <= 256
    for b, g, r, a in palette:
        pal_bytes += bytes((b, g, r, a))
    pal_bytes += b'\x00' * (1024 - len(pal_bytes))
    m = 1
    stream = struct.pack('<II', MAGIC, m | (total << 4))
    stream += rle_encode(pal_bytes + indices)
    blob = struct.pack('<BBH HH I', 0x10, 0x04, 0, W, H, len(stream))
    return blob + stream


def decode_block(blob):
    """Reference decoder for roundtrip verification (mirrors fw logic)."""
    cf, flags, _, w, h, dlen = struct.unpack_from('<BBH HH I', blob, 0)
    data = blob[12:12 + dlen]
    assert (flags >> 2) & 7 == 1, 'not the compressed branch'
    magic, w1 = struct.unpack_from('<II', data, 0)
    assert magic == MAGIC
    m = w1 & 0xF
    total = (w1 >> 4) & 0xFFFFFF
    assert m == 1 and total == 1024 + w * h
    out = bytearray()
    i = 8
    while len(out) < total:
        c = data[i]; i += 1
        if c < 0x80:
            if c:
                out += data[i:i + m] * c
            i += m
        else:
            k = (c & 0x7F) * m
            out += data[i:i + k]; i += k
    assert len(out) == total and i == len(data), (len(out), total, i, len(data))
    return w, h, bytes(out[:1024]), bytes(out[1024:])


def build_blob(title='Chaos 安装器', status='准备就绪',
               buttons=(('运行', 'primary'), ('注入设置', 'ghost'),
                        ('重启', 'ghost'), ('清除重置', 'danger')),
               layout='installer'):
    """按指定界面生成一块完整预览块(含 roundtrip 校验)。

    三个打包脚本各取所需: 主包 = 安装器运行界面(layout='installer'),
    字体包与图标包 = 各自的投递界面(layout='pack', 标题带短名) ——
    预览块在表盘列表里就是各自表盘点进去的那个样子。
    """
    img = make_image(title, status, buttons, layout)
    indices, palette = build_palette_indexed(img)
    blob = encode(indices, palette)

    # roundtrip verify
    w, h, pal, px = decode_block(blob)
    assert (w, h) == (W, H)
    assert px == indices, 'roundtrip pixel mismatch'
    rebuilt = bytearray()
    for idx in px:
        b, g, r, a = pal[idx * 4:idx * 4 + 4]
        rebuilt += bytes((r, g, b, a))
    src = bytearray()
    p = img.load()
    for y in range(H):
        for x in range(W):
            src += bytes(p[x, y])
    # alpha-coverage comparison only (RGB was quantized/premultiplied by design):
    # 可见像素一律不透明, 只有圆角外 a<VIS_MIN(16) 保持透明 —— 覆盖判据与
    # build_palette_indexed 的可见门限一致。
    mism = sum(1 for i in range(0, len(src), 4)
               if (src[i + 3] >= 16) != (rebuilt[i + 3] >= 16))
    assert mism == 0, f'{mism} alpha-coverage mismatches'
    print(f'roundtrip OK: {len(blob)} bytes, palette entries used: {len(palette)}')
    return blob


def render_png(blob, png_path):
    """把预览块解码渲染成 2x PNG(人工检查用)。"""
    w, h, pal, px = decode_block(blob)
    vis = Image.new('RGBA', (w, h))
    vp = vis.load()
    for y in range(h):
        for x in range(w):
            idx = px[y * w + x]
            b, g, r, a = pal[idx * 4:idx * 4 + 4]
            vp[x, y] = (r, g, b, a)
    vis.resize((w * 2, h * 2), Image.NEAREST).save(png_path)
    print('preview render ->', png_path)


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else \
        os.path.join(os.path.dirname(__file__), '..', 'Chaos-Module', 'shell', 'preview.bin')
    png_path = sys.argv[2] if len(sys.argv) > 2 else None

    blob = build_blob()

    if png_path:
        render_png(blob, png_path)

    with open(out_path, 'wb') as f:
        f.write(blob)
    print(f'written {out_path} ({len(blob)} bytes)')


if __name__ == '__main__':
    main()
