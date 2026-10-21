"""A real song made ready for the music video: decoded, heard as Glance
hears it, and its word-timed lyrics as Glance holds them.

    python studio/track.py AUDIO LYRIC.json OUT-FOLDER

AUDIO is any file ffmpeg reads, LYRIC.json what NetEase answers for the
song's lyrics (see yrc.py). Writes to OUT-FOLDER song.wav (48 kHz stereo),
heard.json (see song.py's `heard`, over the whole song, in its own time) and
lyrics.json (the whole song's lines, in its own time).
"""
import json
import subprocess
import sys
import wave
from pathlib import Path

import imageio_ffmpeg
import numpy as np

from song import RATE, heard
from yrc import lines


def main():
    audio, lyric, folder = sys.argv[1], sys.argv[2], Path(sys.argv[3])
    folder.mkdir(parents=True, exist_ok=True)
    wav = folder / "song.wav"
    subprocess.run([imageio_ffmpeg.get_ffmpeg_exe(), "-y", "-loglevel", "error", "-i", audio, "-map", "0:a", "-ac", "2", "-ar", str(RATE), "-c:a", "pcm_s16le", str(wav)], check=True)
    with wave.open(str(wav)) as read:
        samples = np.frombuffer(read.readframes(read.getnframes()), dtype="<i2").astype(np.float64).reshape(-1, 2) / 32768
    said = json.load(open(lyric, encoding="utf-8"))
    kept = list(lines(said["yrc"]["lyric"]))
    (folder / "lyrics.json").write_text(json.dumps(kept, ensure_ascii=False, indent=1), encoding="utf-8", newline="\n")
    analysed = heard(samples)
    (folder / "heard.json").write_text(json.dumps(analysed, separators=(",", ":")), encoding="utf-8", newline="\n")
    print(f"{len(samples) / RATE:.1f} s, {len(kept)} lines, level {np.mean(analysed['level']):.2f} on average")


if __name__ == "__main__":
    main()
