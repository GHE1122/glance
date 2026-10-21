"""A set of 3:4 pictures for a Xiaohongshu post about Glance's music, from
the studio's stills (see xhs-music-shots.json): the cover, music at the edge
beside the monitor, the sizes it takes, the record and the lines lit as they
are sung, the full screen, the clear background and its switches, and where
its lyrics come from and where to get it.

    python studio/xhs_music.py FRAMES EDGES FONT OUT-FOLDER

EDGES holds the stills of the page with music and the monitor at the
screen's two edges (xhs-music-edges.json: a wider screen, for both to fit).
"""
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

sys.path.insert(0, str(Path(__file__).parent))
from compose import ACCENT, Fonts, mark  # noqa: E402
from xhs import DIM, H, MARGIN, SOFT, W, WHITE, card, finish, heading, write  # noqa: E402

SCALE = 2.0


def still(frames, name):
    return Image.open(Path(frames) / name / "00001.png").convert("RGB")


def lifted(frame, bare, box):
    """What a still shows in `box` (DIPs) over the bare desktop, cut out of it with its shadow: its pixels, clear
    where they are the desktop's."""
    box = tuple(int(v * SCALE) for v in box)
    shown, under = np.asarray(frame.crop(box)).astype(int), np.asarray(bare.crop(box)).astype(int)
    alpha = np.clip(np.abs(shown - under).max(axis=2) * 12, 0, 255).astype(np.uint8)
    alpha = np.asarray(Image.fromarray(alpha).filter(ImageFilter.MaxFilter(3)))
    piece = frame.crop(box).convert("RGBA")
    piece.putalpha(Image.fromarray(alpha))
    return piece


def merged(frames, names, bare):
    """The stills `names` as one: each where it differs from the bare desktop."""
    out = np.asarray(bare).astype(int)
    for name in names:
        shown = np.asarray(still(frames, name)).astype(int)
        differs = np.abs(shown - np.asarray(bare).astype(int)).max(axis=2, keepdims=True) > 2
        out = np.where(differs, shown, out)
    return Image.fromarray(out.astype(np.uint8)).convert("RGBA")


def shade(page, top, bottom, strength):
    """A darkening from `top` to `bottom` (px), strongest at `bottom` (or at `top`, where `bottom` is above it)."""
    mask = Image.new("L", (W, H), 0)
    draw = ImageDraw.Draw(mask)
    a, b = sorted((top, bottom))
    for y in range(a, b):
        k = (y - a) / max(b - a, 1)
        k = k if bottom > top else 1 - k
        draw.line((0, y, W, y), fill=int(255 * strength * k ** 1.3))
    if bottom > top:
        draw.rectangle((0, b, W, H), fill=int(255 * strength))
    else:
        draw.rectangle((0, 0, W, a), fill=int(255 * strength))
    page.alpha_composite(Image.merge("RGBA", (*Image.new("RGB", (W, H), (6, 4, 18)).split(), mask)))


def pill(draw, x, y, text, font, fill=ACCENT, ink=(255, 255, 255)):
    """A rounded label; returns its right edge."""
    width = draw.textlength(text, font=font)
    h = font.size * 1.5
    draw.rounded_rectangle((x, y, x + width + font.size * 1.2, y + h), radius=h / 2, fill=fill + (255,))
    draw.text((x + font.size * 0.6, y + h / 2), text, font=font, anchor="lm", fill=ink + (255,))
    return x + width + font.size * 1.2


def page_cover(frames, fonts, bare):
    page = still(frames, "cover-stage").convert("RGBA")
    shade(page, 900, 0, 0.55)
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    right = pill(draw, MARGIN, 150, "NEW", fonts.get(36, b"Bold"))
    draw.text((right + 24, 150 + 27), "Glance 0.4 新增：音乐", font=fonts.get(40, b"Bold"), anchor="lm", fill=WHITE)
    y = write(draw, (MARGIN, 250), "屏幕边上\n跟着唱的歌词", fonts.get(112, b"Bold"), WHITE, spacing=1.25)
    draw.rounded_rectangle((MARGIN, y + 6, MARGIN + 170, y + 20), radius=6, fill=ACCENT + (255,))
    write(draw, (MARGIN, y + 60), "一个字一个字亮起来", fonts.get(50, b"Medium"), SOFT)
    write(draw, (MARGIN, H - 112), "免费 · 开源 · Windows 10/11", fonts.get(38, b"Regular"), SOFT)
    return finish(page, layer)


def page_edges(frames, fonts, bare):
    page = merged(EDGES, ["monitor-edge", "music-edge"], still(EDGES, "bare"))
    shade(page, 1250, 1390, 0.75)
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    heading(layer, fonts, "音乐也藏在屏幕边上", "和监控一样，鼠标推到边缘就滑出来，移开就收回。\n哪条边放什么，在设置的「布局」页拖过去就行。", top=1380)
    return finish(page, layer)


def page_sizes(frames, fonts, bare):
    page = bare.convert("RGBA")
    shade(page, 0, H, 0.35)
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    y = heading(layer, fonts, "拉多大都行", "拖动边角改大小，排版跟着变。")
    label = fonts.get(34, b"Bold")
    margin = 24
    shown = []
    for name, box, said in (("strip", (30, 378, 561, 72), "横条"), ("player", (30, 316, 561, 196), "播放器"), ("card", (110, 224, 400, 380), "卡片")):
        shown.append((lifted(still(frames, name), bare, (box[0] - margin, box[1] - margin, box[0] + box[2] + margin, box[1] + box[3] + margin)), said))
    # All three as large as fits between the heading and the foot, each under its label.
    rim = int(margin * SCALE)
    room = H - 90 - (y + 30)
    tall = sum(piece.height - 2 * rim for piece, _ in shown)
    k = min(0.92, (room - len(shown) * (70 + 40)) / tall)
    y += 30
    for piece, said in shown:
        piece = piece.resize((int(piece.width * k), int(piece.height * k)), Image.LANCZOS)
        pill(draw, MARGIN, y, said, label, fill=(255, 255, 255), ink=(20, 16, 40))
        y += 70
        page.alpha_composite(piece, (int((W - piece.width) / 2), int(y - rim * k)))
        y += piece.height - int(2 * rim * k) + 40
    return finish(page, layer)


def page_stage(frames, fonts, bare):
    page = bare.convert("RGBA")
    shade(page, 0, H, 0.35)
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    y = heading(layer, fonts, "唱到哪，亮到哪", "有逐字时间的歌，歌词一个字一个字亮起来。\n前后几句跟着滚动，唱片随播放转动，\n律动条跟着节拍跳。")
    margin = 30
    box = (20, 244, 581, 340)
    piece = lifted(still(frames, "stage"), bare, (box[0] - margin, box[1] - margin, box[0] + box[2] + margin, box[1] + box[3] + margin))
    page.alpha_composite(piece, ((W - piece.width) // 2, int(y + (H - y - piece.height) / 2)))
    return finish(page, layer)


def page_full(frames, fonts, bare):
    page = still(frames, "full").convert("RGBA")
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    heading(layer, fonts, "全屏歌词", "烟雾取封面的颜色，跟着节拍流动。\n点组件右上角的全屏按钮进入，按 Esc 退出。")
    return finish(page, layer)


SWITCHES = ["封面", "歌名与歌手", "翻译", "律动条", "常驻播放控件", "透明背景"]


def page_clear(frames, fonts, bare):
    page = bare.convert("RGBA")
    shade(page, 0, H, 0.2)
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    y = heading(layer, fonts, "透明背景，当桌面歌词", "去掉底色，文字加描边，直接浮在桌面上。")
    box = (20, 366, 581, 96)
    margin = 10
    piece = lifted(still(frames, "float"), bare, (box[0] - margin, box[1] - margin, box[0] + box[2] + margin, box[1] + box[3] + margin))
    page.alpha_composite(piece, ((W - piece.width) // 2, int(y) + 120))
    top = 1030
    card(layer, (MARGIN - 20, top, W - MARGIN + 20, top + 520), radius=40)
    draw.text((MARGIN + 30, top + 50), "每一项都能单独开关", font=fonts.get(50, b"Bold"), fill=WHITE)
    draw.rounded_rectangle((MARGIN + 30, top + 128, MARGIN + 150, top + 138), radius=5, fill=ACCENT + (255,))
    chip = fonts.get(38, b"Medium")
    x, row = MARGIN + 30, top + 190
    for said in SWITCHES:
        width = draw.textlength(said, font=chip) + 64
        if x + width > W - MARGIN - 10:
            x, row = MARGIN + 30, row + 100
        draw.rounded_rectangle((x, row, x + width, row + 72), radius=36, fill=(255, 255, 255, 30), outline=(255, 255, 255, 90), width=2)
        draw.text((x + width / 2, row + 36), said, font=chip, anchor="mm", fill=WHITE)
        x += width + 22
    write(draw, (MARGIN + 30, row + 120), "在设置的「音乐」页里", fonts.get(34, b"Regular"), DIM)
    return finish(page, layer)


def page_where(frames, fonts, bare):
    page = bare.convert("RGBA")
    shade(page, 0, H, 0.5)
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    y = heading(layer, fonts, "歌词从哪来")
    points = [
        "网易云音乐、QQ 音乐、酷狗、LRCLIB 一起找",
        "本地歌曲先用文件自带的歌词",
        "有翻译的歌，翻译显示在当前这句下面",
        "能在 Windows 媒体浮窗里显示歌名的播放器都能用",
        "网易云音乐需要 InfLink-rs 插件提供播放进度，设置里可以一键安装",
    ]
    font = fonts.get(38, b"Regular")
    y += 10
    for said in points:
        draw.ellipse((MARGIN, y + 20, MARGIN + 14, y + 34), fill=ACCENT + (255,))
        y = write(draw, (MARGIN + 40, y), said, font, SOFT, width=W - 2 * MARGIN - 40, spacing=1.5) + 26
    top = max(int(y) + 60, 1060)
    card(layer, (MARGIN - 20, top, W - MARGIN + 20, top + 430), radius=40)
    draw.text((MARGIN + 30, top + 50), "去哪下载", font=fonts.get(54, b"Bold"), fill=WHITE)
    draw.rounded_rectangle((MARGIN + 30, top + 136, MARGIN + 150, top + 146), radius=5, fill=ACCENT + (255,))
    draw.text((MARGIN + 30, top + 190), "glancepc.com", font=fonts.get(64, b"Bold"), fill=ACCENT + (255,))
    write(draw, (MARGIN + 30, top + 300), "免费 · 开源 · 无广告 · Windows 10/11", fonts.get(36, b"Regular"), SOFT)
    layer.alpha_composite(mark(96, 1.0, 1.0), (W - MARGIN - 96, top + 44))
    return finish(page, layer)


PAGES = [page_cover, page_edges, page_sizes, page_stage, page_full, page_clear, page_where]


EDGES = None


def main():
    global EDGES
    frames, EDGES, font, out = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3], Path(sys.argv[4])
    out.mkdir(parents=True, exist_ok=True)
    fonts = Fonts(font)
    bare = still(frames, "bare")
    for index, make in enumerate(PAGES, 1):
        path = out / f"{index}-{make.__name__[5:]}.png"
        make(frames, fonts, bare).save(path)
        print(path)


if __name__ == "__main__":
    main()
