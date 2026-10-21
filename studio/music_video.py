"""The music video: the studio's frames of the monitor, the music widget
and the music full screen, captioned and cut together as a timeline says,
with the song under them and a few sounds of their own, as a vertical
H.264 MP4.

    python studio/music_video.py TIMELINE.json FRAMES FONT OUT.mp4 [--preview T ...]

FRAMES holds the studio's shots (monitor, music-widget, music-full: see
music-shots*.json), FONT a variable CJK font with Bold and Regular
instances. --preview writes the frame at each T seconds as a PNG beside
OUT instead of the video.

It begins with the Glance people know, the monitor sliding out of the
right edge and away, and then what is new: music sliding out of the left
edge, marked new. Taken onto the desktop, it grows as the song goes (a
strip, a player, a card, the stage with its record), each turn with a soft
whoosh, then the camera falls into it and the full screen takes over. At
the end the screen blurs back behind Glance's mark, what is new and where
to find it.

The timeline (times in seconds of the video, whose first frame is each
shot's first):
  audio       the song (a WAV), and `from`: where in it the video begins
  seconds     how long the video is, the song fading in over `fade_in` and
              out over the last `fade_out`
  music_from  when the music's frames take over from the monitor's
  fall        when the camera begins to fall into the widget, and when the
              full screen has taken over; `stage` the widget's box then
              (DIPs, as the studio's keys have it) at `scale` px a DIP
  full_from   when the full screen's shot begins
  end         when the end begins
  captions    [from, to, title, subtitle], `new` the one marked new
  whooshes    [at, seconds, loudness], `pops` [at, loudness]
"""
import json
import subprocess
import sys
import wave
from pathlib import Path

import imageio_ffmpeg
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

from compose import ACCENT, MARGIN, Fonts, H, W, ease, ease_out, kinetic, mark, shadowed

FPS = 30
RATE = 48_000


class Shot:
    """A shot's frames, read as they are asked for (past its end, its last)."""

    def __init__(self, folder):
        self.files = sorted(Path(folder).glob("*.png"))
        if not self.files:
            raise SystemExit(f"no frames in {folder}")

    def at(self, t):
        index = min(max(int(round(t * FPS)), 0), len(self.files) - 1)
        return Image.open(self.files[index]).convert("RGB")


def fallen(cut, frame, p):
    """The frame as the camera falls into the stage: scaled about its middle till it covers the screen."""
    x, y, w, h = (v * cut["scale"] for v in cut["stage"])
    cx, cy = x + w / 2, y + h / 2
    zoom = 1 + (max(W / w, H / h) * 1.1 - 1) * p
    # (Its middle drawn toward the screen's as it grows.)
    mx, my = cx + (W / 2 - cx) * p, cy + (H / 2 - cy) * p
    scaled = frame.resize((int(W * zoom), int(H * zoom)), Image.BILINEAR)
    left, top = int(cx * zoom - mx), int(cy * zoom - my)
    return scaled.crop((left, top, left + W, top + H)).filter(ImageFilter.GaussianBlur(10 * p))


def end_card(cut, fonts, frame, t):
    """The end: the screen blurred and darkened behind Glance's mark drawing its pulse, its name, what is new, where
    to find it and what it is."""
    since = t - cut["end"]
    p = ease(since / 0.7)
    behind = frame.filter(ImageFilter.GaussianBlur(28 * p))
    behind = Image.blend(behind, Image.new("RGB", (W, H), (8, 6, 18)), 0.45 * p)
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    size = 260
    logo = mark(size, ease_out((since - 0.2) / 1.0), ease_out((since - 1.1) / 0.4))
    logo_alpha = ease_out((since - 0.1) / 0.4)
    if logo_alpha > 0:
        logo.putalpha(logo.getchannel("A").point(lambda a: int(a * logo_alpha)))
        layer.alpha_composite(logo, ((W - size) // 2, 560))
    lines = [("Glance", fonts.get(120, b"Bold"), (255, 255, 255), 900, 0.9),
             ("0.4 新增：音乐", fonts.get(52, b"Bold"), (255, 255, 255), 1050, 1.2),
             ("glancepc.com", fonts.get(64, b"Bold"), ACCENT, 1150, 1.5),
             ("免费 · 开源 · Windows 10/11", fonts.get(44, b"Regular"), (226, 230, 240), 1260, 1.8)]
    for text, font, colour, y, at in lines:
        a = ease_out((since - at) / 0.5)
        if a <= 0:
            continue
        width = draw.textlength(text, font=font)
        draw.text(((W - width) / 2, y + (1 - a) * 30), text, font=font, fill=colour + (int(255 * a),))
    out = behind.convert("RGBA")
    out.alpha_composite(shadowed(layer, blur=18, strength=0.6))
    return out.convert("RGB")


def new_badge(cut, fonts, t):
    """A small "NEW" mark over the caption that says what is new, popping in and leaving with it."""
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    start, stop = cut["new"]
    a = ease_out((t - start - 0.5) / 0.3) * (1 - ease((t - (stop - 0.3)) / 0.3))
    if a <= 0:
        return layer
    draw = ImageDraw.Draw(layer)
    grow = 0.85 + 0.15 * ease_out((t - start - 0.5) / 0.25)
    font = fonts.get(int(34 * grow), b"Bold")
    text = "NEW"
    width = draw.textlength(text, font=font)
    w, h = width + 36 * grow, 50 * grow
    x, y = MARGIN, 200
    draw.rounded_rectangle((x, y, x + w, y + h), radius=h / 2, fill=ACCENT + (int(255 * a),))
    draw.text((x + (w - width) / 2, y + h / 2), text, font=font, anchor="lm", fill=(255, 255, 255, int(255 * a)))
    return layer


def frame_at(cut, fonts, shots, t):
    monitor, widget, full = shots
    (fall_from, fall_to), full_from = cut["fall"], cut["full_from"]
    if t < cut["music_from"]:
        frame = monitor.at(t)
    elif t < fall_from:
        frame = widget.at(t)
    elif t < fall_to:
        p = ease((t - fall_from) / (fall_to - fall_from))
        frame = Image.blend(fallen(cut, widget.at(t), p), full.at(t - full_from), ease((p - 0.5) / 0.5))
    else:
        frame = full.at(t - full_from)
    if t >= cut["end"]:
        return end_card(cut, fonts, frame, t)
    out = frame.convert("RGBA")
    for start, stop, title, subtitle in cut["captions"]:
        if start <= t < stop:
            out.alpha_composite(kinetic(fonts, title, subtitle, t - start, stop - start, W - 2 * MARGIN, 270, size=92))
    if cut["new"][0] <= t < cut["new"][1]:
        out.alpha_composite(new_badge(cut, fonts, t))
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


def soundtrack(cut, out):
    """The song from `from` for the video's length, faded in and out, the sounds over it, its peaks no higher than
    -1 dBFS."""
    with wave.open(cut["audio"]) as read:
        assert read.getframerate() == RATE, read.getframerate()
        song = np.frombuffer(read.readframes(read.getnframes()), dtype="<i2").astype(np.float64).reshape(-1, 2) / 32768
    count = int(cut["seconds"] * RATE)
    start = int(cut.get("from", 0.0) * RATE)
    song = np.pad(song[start:start + count], ((0, max(0, count - (len(song) - start))), (0, 0)))[:count]
    fade_in, fade_out = int(cut.get("fade_in", 0.0) * RATE), int(cut.get("fade_out", 0.0) * RATE)
    if fade_in:
        song[:fade_in] *= np.linspace(0, 1, fade_in)[:, None]
    if fade_out:
        song[-fade_out:] *= np.linspace(1, 0, fade_out)[:, None] ** 1.5
    sounds = np.zeros_like(song)
    for index, (at, seconds, loud) in enumerate(cut["whooshes"]):
        made = whoosh(seconds, loud * 0.22, index)
        begin = int(at * RATE)
        sounds[begin:begin + len(made)] += made[:len(sounds) - begin]
    for at, loud in cut["pops"]:
        made = pop(loud * 0.3)
        begin = int(at * RATE)
        sounds[begin:begin + len(made)] += made[:len(sounds) - begin]
    mixed = song + sounds
    most = 10 ** (-1 / 20)
    if np.abs(mixed).max() > most:
        mixed *= most / np.abs(mixed).max()
    with wave.open(str(out), "wb") as write:
        write.setnchannels(2)
        write.setsampwidth(2)
        write.setframerate(RATE)
        write.writeframes((mixed * 32767).round().astype("<i2").tobytes())


def main():
    cut = json.load(open(sys.argv[1], encoding="utf-8"))
    frames, font, out = Path(sys.argv[2]), sys.argv[3], Path(sys.argv[4])
    fonts = Fonts(font)
    shots = Shot(frames / "monitor"), Shot(frames / "music-widget"), Shot(frames / "music-full")
    if "--preview" in sys.argv:
        for t in map(float, sys.argv[sys.argv.index("--preview") + 1:]):
            frame_at(cut, fonts, shots, t).save(out.with_name(f"{out.stem}-{t:05.2f}.png"))
        return
    audio = out.with_suffix(".wav")
    soundtrack(cut, audio)
    ffmpeg = imageio_ffmpeg.get_ffmpeg_exe()
    encode = subprocess.Popen([
        ffmpeg, "-y", "-loglevel", "error",
        "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{W}x{H}", "-r", str(FPS), "-i", "-",
        "-i", str(audio),
        "-c:v", "libx264", "-preset", "slow", "-crf", "17", "-pix_fmt", "yuv420p", "-movflags", "+faststart",
        "-c:a", "aac", "-b:a", "256k", "-shortest", str(out),
    ], stdin=subprocess.PIPE)
    count = int(cut["seconds"] * FPS)
    for index in range(count):
        encode.stdin.write(frame_at(cut, fonts, shots, index / FPS).tobytes())
        if index % 150 == 0:
            print(f"{index}/{count}", flush=True)
    encode.stdin.close()
    encode.wait()
    audio.unlink()
    print(out)


if __name__ == "__main__":
    main()
