"""Two Douyin music covers composed from the video's real studio frames.

    python studio/music_covers.py [FONT] [OUT-FOLDER] [--frames FOLDER] [--widget N] [--full N]

    python studio/music_covers.py --frames target/song/frames --widget 560 --full 240 C:/Windows/Fonts/NotoSansSC-VF.ttf target/song/covers

Run from any directory. Relative arguments resolve from the current directory.
Defaults use the installed Noto Sans SC variable font, the frames of the
yujian cut and target/yujian/covers.
Needs Pillow, numpy and imageio-ffmpeg, as do the existing studio helpers.
Writes 1080x1440 and 1440x1080 PNGs and quality-92, 4:4:4 JPEGs.

The widget is music-widget/<widget>.png (by default 00721, the record player
at 24.00 seconds of the yujian cut), cropped to the native stage rect the
music shots share (560x360 DIP at 20,500). The landscape also uses
music-full/<full>.png (by default 00076). Every UI pixel comes
from those renders. Pillow supplies only typography, background, logo,
shadows and crop masks. UI is downsampled once, never enlarged.
"""

import argparse
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

from compose import ACCENT, Fonts, cutout, mark, shadowed


ROOT = Path(__file__).resolve().parents[1]
FRAMES = ROOT / "target/yujian/frames"
WIDGET, FULL = 721, 76
HEADLINE = "屏幕边上的歌词组件"
LINE = "歌词跟着歌声一个字一个字亮起来"
SMALL = "Glance 0.4 · 免费开源 · Windows 10/11"
WHITE = (255, 255, 255, 255)
SOFT = (232, 228, 244, 255)


def ground(size):
    """Use the exact video's bare wallpaper, softened behind the type."""
    with Image.open(FRAMES / "music-widget/00001.png") as source:
        page = source.convert("RGB").resize(size, Image.Resampling.LANCZOS)
    page = page.filter(ImageFilter.GaussianBlur(5))
    page = Image.blend(page, Image.new("RGB", size, (7, 6, 20)), 0.32)
    return page.convert("RGBA")


def stage():
    """Native 560x360 DIP stage at 1.8 px/DIP, with its native 14 DIP radius."""
    rect = dict(x=36, y=900, w=1008, h=648)
    with Image.open(FRAMES / f"music-widget/{WIDGET:05d}.png") as source:
        return cutout(source.convert("RGB"), {
            "panel": rect, "pieces": [rect], "radius": 25.2,
        })


def full_lyrics():
    """A crop retaining the complete live line and its next line, with smoke."""
    with Image.open(FRAMES / f"music-full/{FULL:05d}.png") as source:
        return source.convert("RGBA").crop((0, 450, 1080, 1760))


class Layout:
    """Record visible content bounds and reject overlaps or unsafe placement."""

    def __init__(self, size, fonts, safe):
        self.page = ground(size)
        self.fonts = fonts
        self.safe = safe
        self.bounds = []

    def reserve(self, name, box):
        left, top, right, bottom = box
        assert 0 <= left < right <= self.safe[0], (name, box, self.safe)
        assert 0 <= top < bottom <= self.safe[1], (name, box, self.safe)
        for other, (a, b, c, d) in self.bounds:
            assert right <= a or left >= c or bottom <= b or top >= d, (name, other)
        self.bounds.append((name, box))

    def text(self, xy, words, size, weight=b"Bold", fill=WHITE):
        assert ";" not in words and "\uFF1B" not in words
        draw = ImageDraw.Draw(self.page)
        font = self.fonts.get(size, weight)
        box = draw.textbbox(xy, words, font=font, anchor="lt")
        self.reserve(words, box)
        draw.text(xy, words, font=font, anchor="lt", fill=fill)

    def brand(self, xy, size=31):
        x, y = xy
        self.reserve("logo", (x, y, x + 52, y + 52))
        self.page.alpha_composite(mark(52, 1.0, 1.0), xy)
        self.text((x + 72, y + 12), SMALL, size, b"Medium", SOFT)

    def badge(self, xy):
        x, y = xy
        words = "0.4 新增：音乐"
        font = self.fonts.get(29, b"Bold")
        draw = ImageDraw.Draw(self.page)
        width = round(draw.textlength(words, font=font)) + 40
        self.reserve("badge", (x, y, x + width, y + 54))
        draw.rounded_rectangle((x, y, x + width, y + 54), radius=27,
                               fill=ACCENT + (255,))
        draw.text((x + width / 2, y + 26), words, font=font, anchor="mm",
                  fill=(24, 12, 20, 255))

    def image(self, name, source, xy, width, rounded=False):
        factor = width / source.width
        assert factor <= 1.0, (name, factor)
        height = round(source.height * factor)
        x, y = xy
        self.reserve(name, (x, y, x + width, y + height))
        piece = source.resize((width, height), Image.Resampling.LANCZOS)
        if rounded:
            # Presentation crop boundary only, no fabricated UI.
            mask = Image.new("L", (width * 4, height * 4))
            ImageDraw.Draw(mask).rounded_rectangle(
                (0, 0, mask.width - 1, mask.height - 1), radius=88, fill=255)
            piece.putalpha(mask.resize(piece.size, Image.Resampling.LANCZOS))
        layer = Image.new("RGBA", self.page.size)
        layer.alpha_composite(piece, xy)
        self.page.alpha_composite(shadowed(layer, blur=22, strength=0.65))
        print(f"{name}: {source.size} -> {piece.size}, scale {factor:.3f}")


def portrait(fonts):
    layout = Layout((1080, 1440), fonts, (972, 1267))
    layout.brand((56, 56))
    layout.badge((56, 150))
    assert "屏幕边上的" + "歌词组件" == HEADLINE
    layout.text((56, 250), "屏幕边上的", 124)
    layout.text((52, 394), "歌词组件", 150)
    layout.text((56, 585), LINE, 43, b"Medium", SOFT)
    layout.image("record player", stage(), (54, 666), 900)
    return layout.page.convert("RGB")


def landscape(fonts):
    layout = Layout((1440, 1080), fonts, (1384, 980))
    layout.brand((56, 54))
    layout.badge((1100, 54))
    layout.text((56, 158), HEADLINE, 112)
    layout.text((60, 302), LINE, 44, b"Medium", SOFT)
    layout.text((60, 383), "唱片与逐字歌词", 28, b"Medium", SOFT)
    layout.text((944, 383), "全屏歌词", 28, b"Medium", SOFT)
    layout.image("record player", stage(), (56, 428), 850)
    layout.image("full-screen lyrics", full_lyrics(), (940, 428), 444,
                 rounded=True)
    return layout.page.convert("RGB")


def main():
    global FRAMES, WIDGET, FULL
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("font", type=Path, nargs="?",
                        default=Path("C:/Windows/Fonts/NotoSansSC-VF.ttf"))
    parser.add_argument("out_folder", type=Path, nargs="?",
                        default=ROOT / "target/yujian/covers")
    parser.add_argument("--frames", type=Path, default=FRAMES,
                        help="the video's frames, with music-widget and music-full in it")
    parser.add_argument("--widget", type=int, default=WIDGET, help="the music widget's frame")
    parser.add_argument("--full", type=int, default=FULL, help="the full screen's frame")
    args = parser.parse_args()
    FRAMES, WIDGET, FULL = args.frames.resolve(), args.widget, args.full
    fonts = Fonts(str(args.font.resolve()))
    out = args.out_folder.resolve()
    out.mkdir(parents=True, exist_ok=True)
    for name, make in (("cover-3x4", portrait), ("cover-4x3", landscape)):
        page = make(fonts)
        page.save(out / f"{name}.png")
        page.save(out / f"{name}.jpg", quality=92, subsampling=0, optimize=True)
        for suffix in ("png", "jpg"):
            path = out / f"{name}.{suffix}"
            with Image.open(path) as check:
                assert check.size == page.size and check.mode == "RGB"
                check.verify()
            print(path)


if __name__ == "__main__":
    main()
