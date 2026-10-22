"""The song the site's music plays, made ready for the page: its lyrics, what
Glance hears of it (its level, its bands and its beat, every STEP seconds, in
bytes), its cover and the song itself, small enough to load with the page.

    python studio/site_song.py SONG-FOLDER OUT-FOLDER

SONG-FOLDER holds what song.py and music_art.py made (lyrics.json,
heard.json, cover.png, song.wav).
"""
import base64
import json
import subprocess
import sys
from pathlib import Path

import imageio_ffmpeg
from PIL import Image

# Every other of the 8 ms looks Glance takes: the page draws at most 60 times a second.
STEP = 0.016

# The lines in English, for the English page's translation under each.
ENGLISH = [
    "Outside, the lights come on one by one",
    "A song is playing at the edge of the screen",
    "Every word keeps time with the heartbeat",
    "One gentle push, and the night lights up",
]


def bytes_of(values):
    return base64.b64encode(bytes(max(0, min(255, round(v * 255))) for v in values)).decode()


def main():
    song, out = Path(sys.argv[1]), Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)
    lyrics = json.loads((song / "lyrics.json").read_text(encoding="utf-8"))
    heard = json.loads((song / "heard.json").read_text())
    every = round(STEP / heard["step"])
    level = heard["level"][every - 1::every]
    beat = [max(heard["beat"][i:i + every]) for i in range(0, len(heard["beat"]), every)][:len(level)]
    bands = heard["bands"][every - 1::every]
    lines = [{"at": l["at"], "end": l["end"], "text": l["text"], "en": en,
              "words": [[w["at"], w["end"], w["text"]] for w in l["words"]]} for l, en in zip(lyrics, ENGLISH)]
    data = {
        "title": "一盏一盏", "artist": "Glance", "length": 32.9,
        "lines": lines,
        "heard": {"step": STEP, "count": len(level), "bands": len(bands[0]),
                  "level": bytes_of(level), "beat": bytes_of(beat), "spectrum": bytes_of(v for row in bands for v in row)},
    }
    (out / "song.json").write_text(json.dumps(data, ensure_ascii=False, separators=(",", ":")), encoding="utf-8", newline="")
    Image.open(song / "cover.png").convert("RGB").resize((256, 256), Image.LANCZOS).save(out / "cover.webp", quality=88)
    subprocess.run([imageio_ffmpeg.get_ffmpeg_exe(), "-y", "-loglevel", "error", "-i", str(song / "song.wav"),
                    "-c:a", "libmp3lame", "-b:a", "112k", str(out / "song.mp3")], check=True)
    for name in ("song.json", "cover.webp", "song.mp3"):
        print(name, (out / name).stat().st_size)


if __name__ == "__main__":
    main()
