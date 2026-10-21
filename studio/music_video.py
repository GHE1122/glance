"""The music video: the studio's frames of the music widget and the music
full screen (studio/music-shots.json), captioned, cut together on the
song's bars, with the song under them and a few sounds of their own, as a
vertical H.264 MP4.

    python studio/music_video.py FRAMES SONG-FOLDER FONT OUT.mp4 [--preview T ...]

FRAMES holds the studio's shots (music-widget, music-full), SONG-FOLDER the
song (see song.py), FONT a variable CJK font with Bold and Regular
instances. --preview writes the frame at each T seconds as a PNG beside
OUT instead of the video.

The widget grows as the song goes (a strip, a player, a card, the stage
with its record), each turn with a soft whoosh, then the camera falls into
it and the full screen takes over; on the last bar the screen blurs back
behind Glance's mark and where to find it.
"""
import json
import subprocess
import sys
import wave
from pathlib import Path

import imageio_ffmpeg
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

from compose import ACCENT, MARGIN, Fonts, H, W, clamp01, ease, ease_out, kinetic, mark, shadowed

FPS = 30
RATE = 48_000
SCALE = 1.8
# The song's bars (see song.py): 92 beats a minute, four beats a bar.
BAR = 4 * 60 / 92
LENGTH = 12 * BAR + 1.6
# When the camera falls into the widget, when the full screen has taken over, and when the end begins.
FALL = (7.9 * BAR, 8.0 * BAR + 0.15)
END = 10 * BAR
# Where the full screen's frames begin in the song (see music-shots.json).
FULL_FROM = 20.0

# Captions: (from, to, title, subtitle), seconds into the song.
CAPTIONS = [
    (0.7, 2 * BAR + 0.2, "Glance 音乐", "歌词跟着歌声\n一个字一个字亮起来"),
    (2 * BAR + 0.9, 4 * BAR, "拉高一点", "封面、歌名和频谱\n一起出来"),
    (4 * BAR + 0.9, 6 * BAR, "再大一点", "前后几句歌词\n跟着歌一起滚动"),
    (6 * BAR + 0.9, FALL[0], "够宽就放唱片", "唱片随着播放转动"),
    (FALL[1] + 0.3, END - 0.1, "全屏歌词", "烟雾取封面的颜色\n跟着节拍流动"),
]
# The widget's keys as the studio has them (DIPs), for where the camera falls.
STAGE = (20, 500, 560, 360)
# Sounds: whooshes as the widget grows and as the camera falls; a soft pop as it first shows.
WHOOSHES = [(2 * BAR - 0.35, 0.8, 0.5), (4 * BAR - 0.35, 0.8, 0.5), (6 * BAR - 0.35, 0.8, 0.5), (FALL[0] - 0.25, 1.1, 0.9)]
POPS = [(0.75, 0.55)]


class Shot:
    """A shot's frames, read as they are asked for (past its end, its last)."""

    def __init__(self, folder):
        self.files = sorted(Path(folder).glob("*.png"))
        if not self.files:
            raise SystemExit(f"no frames in {folder}")

    def at(self, t):
        index = min(max(int(round(t * FPS)), 0), len(self.files) - 1)
        return Image.open(self.files[index]).convert("RGB")


def fallen(frame, p):
    """The frame as the camera falls into the stage: scaled about its middle till it covers the screen."""
    x, y, w, h = (v * SCALE for v in STAGE)
    cx, cy = x + w / 2, y + h / 2
    zoom = 1 + (max(W / w, H / h) * 1.1 - 1) * p
    # (Its middle drawn toward the screen's as it grows.)
    mx, my = cx + (W / 2 - cx) * p, cy + (H / 2 - cy) * p
    scaled = frame.resize((int(W * zoom), int(H * zoom)), Image.BILINEAR)
    left, top = int(cx * zoom - mx), int(cy * zoom - my)
    return scaled.crop((left, top, left + W, top + H)).filter(ImageFilter.GaussianBlur(10 * p))


def end_card(fonts, frame, t):
    """The end: the screen blurred and darkened behind Glance's mark drawing its pulse, its name, where to find it
    and what it is."""
    p = ease((t - END) / 0.7)
    behind = frame.filter(ImageFilter.GaussianBlur(28 * p))
    behind = Image.blend(behind, Image.new("RGB", (W, H), (8, 6, 18)), 0.45 * p)
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    since = t - END
    size = 260
    logo = mark(size, ease_out((since - 0.2) / 1.0), ease_out((since - 1.1) / 0.4))
    logo_alpha = ease_out((since - 0.1) / 0.4)
    if logo_alpha > 0:
        logo.putalpha(logo.getchannel("A").point(lambda a: int(a * logo_alpha)))
        layer.alpha_composite(logo, ((W - size) // 2, 560))
    lines = [("Glance", fonts.get(120, b"Bold"), (255, 255, 255), 900, 0.9),
             ("glancepc.com", fonts.get(64, b"Bold"), ACCENT, 1060, 1.3),
             ("免费 · 开源 · Windows 10/11", fonts.get(44, b"Regular"), (226, 230, 240), 1170, 1.6)]
    for text, font, colour, y, at in lines:
        a = ease_out((since - at) / 0.5)
        if a <= 0:
            continue
        width = draw.textlength(text, font=font)
        draw.text(((W - width) / 2, y + (1 - a) * 30), text, font=font, fill=colour + (int(255 * a),))
    out = behind.convert("RGBA")
    out.alpha_composite(shadowed(layer, blur=18, strength=0.6))
    return out.convert("RGB")


def frame_at(fonts, widget, full, t):
    if t < FALL[0]:
        frame = widget.at(t)
    elif t < FALL[1]:
        p = ease((t - FALL[0]) / (FALL[1] - FALL[0]))
        frame = Image.blend(fallen(widget.at(t), p), full.at(t - FULL_FROM), ease((p - 0.5) / 0.5))
    else:
        frame = full.at(t - FULL_FROM)
    if t >= END:
        return end_card(fonts, frame, t)
    out = frame.convert("RGBA")
    for start, stop, title, subtitle in CAPTIONS:
        if start <= t < stop:
            out.alpha_composite(kinetic(fonts, title, subtitle, t - start, stop - start, W - 2 * MARGIN, 250, size=92))
    return out.convert("RGB")


def whoosh(seconds, loud, seed):
    """Air going past: noise through a low pass that opens and closes, swelling and dying, moving across."""
    rng = np.random.default_rng(seed)
    count = int(seconds * RATE)
    noise = rng.standard_normal(count)
    x = np.linspace(0, 1, count)
    cutoff = 250 + 5200 * np.sin(np.pi * x) ** 2
    k = 1 - np.exp(-2 * np.pi * cutoff / RATE)
    out = np.empty(count)
    a = b = 0.0
    for i in range(count):
        a += (noise[i] - a) * k[i]
        b += (a - b) * k[i]
        out[i] = b
    envelope = np.sin(np.pi * x) ** 2.2
    out *= envelope / max(np.abs(out).max(), 1e-9) * loud
    pan = np.clip(x * 1.4 - 0.2, 0, 1)
    return np.stack([out * np.cos(pan * np.pi / 2), out * np.sin(pan * np.pi / 2)], axis=1)


def pop(loud):
    """A soft pop: a short falling tone."""
    count = int(0.12 * RATE)
    x = np.arange(count) / RATE
    tone = np.sin(2 * np.pi * (660 * x - 900 * x * x)) * np.exp(-x / 0.03) * loud
    return np.stack([tone, tone], axis=1)


def soundtrack(song_folder, out):
    with wave.open(str(song_folder / "song.wav")) as read:
        song = np.frombuffer(read.readframes(read.getnframes()), dtype="<i2").astype(np.float64).reshape(-1, 2) / 32768
    sounds = np.zeros_like(song)
    for index, (at, seconds, loud) in enumerate(WHOOSHES):
        made = whoosh(seconds, loud * 0.22, index)
        start = int(at * RATE)
        sounds[start:start + len(made)] += made[:len(sounds) - start]
    for at, loud in POPS:
        made = pop(loud * 0.3)
        start = int(at * RATE)
        sounds[start:start + len(made)] += made
    mixed = song + sounds
    mixed *= 10 ** (-1 / 20) / max(np.abs(mixed).max(), 1e-9) if np.abs(mixed).max() > 10 ** (-1 / 20) else 1
    with wave.open(str(out), "wb") as write:
        write.setnchannels(2)
        write.setsampwidth(2)
        write.setframerate(RATE)
        write.writeframes((mixed * 32767).round().astype("<i2").tobytes())


def main():
    frames, song_folder, font, out = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3], Path(sys.argv[4])
    fonts = Fonts(font)
    widget, full = Shot(frames / "music-widget"), Shot(frames / "music-full")
    if "--preview" in sys.argv:
        for t in map(float, sys.argv[sys.argv.index("--preview") + 1:]):
            frame_at(fonts, widget, full, t).save(out.with_name(f"{out.stem}-{t:05.2f}.png"))
        return
    audio = out.with_suffix(".wav")
    soundtrack(song_folder, audio)
    ffmpeg = imageio_ffmpeg.get_ffmpeg_exe()
    encode = subprocess.Popen([
        ffmpeg, "-y", "-loglevel", "error",
        "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{W}x{H}", "-r", str(FPS), "-i", "-",
        "-i", str(audio),
        "-c:v", "libx264", "-preset", "slow", "-crf", "17", "-pix_fmt", "yuv420p", "-movflags", "+faststart",
        "-c:a", "aac", "-b:a", "256k", "-shortest", str(out),
    ], stdin=subprocess.PIPE)
    count = int(LENGTH * FPS)
    for index in range(count):
        encode.stdin.write(frame_at(fonts, widget, full, index / FPS).tobytes())
        if index % 150 == 0:
            print(f"{index}/{count}", flush=True)
    encode.stdin.close()
    encode.wait()
    audio.unlink()
    print(out)


if __name__ == "__main__":
    main()
