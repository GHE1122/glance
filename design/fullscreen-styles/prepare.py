"""Builds the full-screen style mockup for one song into a folder of its own:

    python prepare.py <NetEase song id> <song.mp3> <out folder> "<latin title>" "<artist in latin>"

It fetches the song's per-word lyrics (yrc) and cover from NetEase, finds the tempo and the beats in the mp3 (onset
autocorrelation, then beat tracking by dynamic programming, as Ellis 2007), checks how far the lyric times must move to
match this file (word starts correlated with the voice's onsets), and writes <out>/index.html with <out>/song.mp3 beside
it. Serve the folder (python -m http.server) and open index.html; the page holds the song whole so it seeks anywhere.
Needs numpy, Pillow and imageio-ffmpeg. Nothing it fetches or reads is kept in the repository: lyrics, covers and songs
belong to their owners.
"""
import base64
import io
import json
import re
import shutil
import subprocess
import sys
import urllib.request
from pathlib import Path

import imageio_ffmpeg
import numpy as np
from PIL import Image

here = Path(__file__).parent
song_id, mp3, out, latin_title, latin_artist = sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3]), sys.argv[4], sys.argv[5]
out.mkdir(parents=True, exist_ok=True)


def fetch(url):
    request = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0", "Referer": "https://music.163.com/"})
    return urllib.request.urlopen(request, timeout=20).read()


detail = json.loads(fetch(f"https://music.163.com/api/song/detail?ids=%5B{song_id}%5D"))["songs"][0]
lyric = json.loads(fetch(f"https://music.163.com/api/song/lyric?id={song_id}&lv=-1&tv=-1&yv=-1"))
yrc = lyric.get("yrc", {}).get("lyric", "")
if not yrc:
    sys.exit("This song has no per-word lyrics (yrc) on NetEase.")
lines = []
for row in yrc.splitlines():
    m = re.match(r"\[(\d+),(\d+)\](.*)", row)
    if m:
        words = [[int(a) / 1000, int(b) / 1000, t] for a, b, _, t in re.findall(r"\((\d+),(\d+),(\d+)\)([^(]*)", m.group(3))]
        lines.append([int(m.group(1)) / 1000, int(m.group(2)) / 1000, words])
cover = Image.open(io.BytesIO(fetch(detail["album"]["picUrl"]))).convert("RGB")
cover = cover.resize((min(1000, cover.width),) * 2, Image.LANCZOS)
jpeg = io.BytesIO()
cover.save(jpeg, "JPEG", quality=80)

# The song, as the analysis hears it.
sr = 22050
raw = subprocess.run([imageio_ffmpeg.get_ffmpeg_exe(), "-v", "error", "-i", str(mp3), "-ac", "1", "-ar", str(sr), "-f", "f32le", "-"], capture_output=True, check=True).stdout
y = np.frombuffer(raw, dtype=np.float32)
hop, n = 256, 2048
frames = 1 + (len(y) - n) // hop
idx = np.arange(n)[None, :] + hop * np.arange(frames)[:, None]
spec = np.abs(np.fft.rfft(y[idx] * np.hanning(n).astype(np.float32), axis=1))
freqs = np.fft.rfftfreq(n, 1 / sr)
fps = sr / hop
logs = np.log1p(spec * 10)
flux = np.maximum(0, np.diff(logs, axis=0, prepend=logs[:1]))
onset = flux.sum(axis=1)
onset = (onset - onset.mean()) / (onset.std() + 1e-9)
vocal = flux[:, (freqs > 300) & (freqs < 3400)].sum(axis=1)
vocal = (vocal - vocal.mean()) / (vocal.std() + 1e-9)
env = onset - np.convolve(onset, np.ones(int(fps)) / int(fps), mode="same")
ac = np.correlate(env, env, mode="full")[len(env) - 1:]
lags = np.arange(len(ac))
bpm_of = 60 * fps / np.maximum(lags, 1)
ok = (bpm_of > 60) & (bpm_of < 180)
weight = np.exp(-0.5 * (np.log2(bpm_of / 100) / 0.9) ** 2)
bpm = 60 * fps / int(lags[ok][np.argmax((ac * weight)[ok])])
period = 60 * fps / bpm
score, back = np.copy(env), np.full(len(env), -1)
for t in range(len(env)):
    lo, hi = int(t - 2 * period), int(t - period / 2)
    if hi <= 0:
        continue
    prev = np.arange(max(lo, 0), hi)
    cand = score[prev] - 100 * np.log((t - prev) / period) ** 2
    k = int(np.argmax(cand))
    score[t], back[t] = env[t] + cand[k], prev[k]
t = int(np.argmax(score[-int(period * 2):])) + len(score) - int(period * 2)
beats = []
while t >= 0:
    beats.append(t)
    t = back[t]
beats = np.array(beats[::-1]) / fps
low = spec[:, freqs < 150].sum(axis=1)
down = int(np.argmax([np.mean([low[int(b * fps)] for b in beats[p::4]]) for p in range(4)]))
rms = np.sqrt(np.convolve(y ** 2, np.ones(sr // 10) / (sr // 10), mode="same"))[:: sr // 10]
starts = [w[0] for line in lines for w in line[2] if w[0] > 5]
best = max(((lag, np.mean([vocal[max(0, p - 2):p + 3].max() for p in ((np.array(starts) + lag) * fps).astype(int) if 0 <= p < len(vocal)])) for lag in np.arange(-4, 4.001, 0.01)), key=lambda x: x[1])
shift = round(float(best[0]), 2)
print(f"{detail['name']}: {bpm:.1f} BPM, {len(beats)} beats, lyrics {shift:+.2f} s against this file")
if abs(shift) > 0.08:
    lines = [[s + shift, d, [[t + shift, wd, text] for t, wd, text in words]] for s, d, words in lines]

analysis = {"bpm": round(bpm, 3), "beats": [round(float(b), 3) for b in beats], "down": down, "rms10": [round(float(v), 2) for v in rms / rms.max()], "duration": round(len(y) / sr, 2)}
song = {"title": detail["name"], "artist": " / ".join(a["name"] for a in detail["artists"]), "short": latin_title, "latinArtist": latin_artist, "latin": f"{latin_title}  ·  {latin_artist}"}
names = [("poster", "海报"), ("pop", "波普"), ("pv", "文字 PV")]
page = (here / "template.html").read_text(encoding="utf-8")
for key, value in {
    "__LINES__": json.dumps(lines, ensure_ascii=False, separators=(",", ":")),
    "__ANALYSIS__": json.dumps(analysis, separators=(",", ":")),
    "__COVER__": base64.b64encode(jpeg.getvalue()).decode(),
    "__SONG__": json.dumps(song, ensure_ascii=False),
    "__PAGES__": json.dumps([[f"#{k}", n, k] for k, n in names], ensure_ascii=False),
    "__STYLE__": "any",
    "__TITLE__": "Glance 全屏样式",
}.items():
    page = page.replace(key, value)
(out / "index.html").write_text('<meta charset="utf-8">\n' + page, encoding="utf-8", newline="\n")
if mp3.resolve() != (out / "song.mp3").resolve():
    shutil.copyfile(mp3, out / "song.mp3")
print(f"Written: {out / 'index.html'}")
