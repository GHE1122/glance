"""The music video's song: written here note by note, played by FluidSynth
through a General MIDI sound font, and heard as Glance hears what plays.

    python studio/song.py FLUIDSYNTH SOUNDFONT OUT-FOLDER

Writes to OUT-FOLDER:
  song.mid     the score
  song.wav     the song, 48 kHz stereo, its peaks at -1 dBFS
  lyrics.json  its lines as Glance's lyrics hold them (each character a word,
               from when its note begins till it ends)
  heard.json   what Glance's sound analysis (src/sound.rs) makes of it, every
               8 ms: the level, the 24 bands and the beat

The tune is a soft pop song in C major at 92 beats a minute: two bars in,
four lines of two bars each, and two bars out, the last two lines fuller.
"""
import json
import subprocess
import sys
import wave
from pathlib import Path

import mido
import numpy as np

BPM = 92
BEAT = 60.0 / BPM
BAR = 4 * BEAT
TICKS = 480
RATE = 48_000
# Bars of the song, and the one after it where what still sounds dies away.
BARS = 12
TAIL = 1.6

# The lines, each sung over two bars from `bar`: (character, pitch, beat it begins on, beats it lasts).
LINES = [
    (2, "窗外的灯一盏一盏亮起", [
        ("E4", 0, .5), ("G4", .5, .5), ("A4", 1, .5), ("G4", 1.5, 1.5), ("E4", 3, .5), ("G4", 3.5, .5),
        ("A4", 4, .5), ("C5", 4.5, .5), ("B4", 5, 1), ("A4", 6, 1.5)]),
    (4, "屏幕边上有首歌在唱", [
        ("A4", 0, .5), ("C5", .5, .5), ("C5", 1, .5), ("A4", 1.5, 1), ("G4", 2.5, .5), ("A4", 3, .5),
        ("C5", 3.5, .5), ("B4", 4, 1), ("D5", 5, 2.5)]),
    (6, "每个字都跟着心跳", [
        ("E5", 0, .5), ("D5", .5, .5), ("C5", 1, 1), ("G4", 2, .5), ("C5", 2.5, .5), ("D5", 3, 1),
        ("E5", 4, 1), ("C5", 5, 2.5)]),
    (8, "轻轻一推夜晚就亮了", [
        ("A4", 0, .5), ("A4", .5, .5), ("G4", 1, .5), ("C5", 1.5, 1.5), ("B4", 3, .5), ("A4", 3.5, .5),
        ("G4", 4, .5), ("D5", 4.5, 3.5), ("C5", 8, 3)]),
]

# A chord a bar: its root (for the bass) and its notes (for the keys, the pad and the strings).
CHORDS = [
    ("F2", ["A3", "C4", "E4", "F4"]),   # Fmaj7
    ("G2", ["G3", "B3", "D4", "G4"]),
    ("C3", ["G3", "C4", "E4", "G4"]),
    ("A2", ["A3", "C4", "E4", "A4"]),
    ("F2", ["A3", "C4", "F4", "A4"]),
    ("G2", ["B3", "D4", "G4", "B4"]),
    ("C3", ["C4", "E4", "G4", "C5"]),
    ("A2", ["C4", "E4", "A4", "C5"]),
    ("F2", ["C4", "F4", "A4", "C5"]),
    ("G2", ["B3", "D4", "G4", "D5"]),
    ("C3", ["C4", "E4", "G4", "C5"]),
    ("C3", ["C4", "E4", "G4", "C5"]),
]

NAMES = {"C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11}


def pitch(name):
    """A note's MIDI number from its name (C4 is 60)."""
    return 12 * (int(name[-1]) + 1) + NAMES[name[0]] + name[1:-1].count("#") - name[1:-1].count("b")


class Score:
    """Notes gathered as (seconds, channel, pitch, velocity, seconds long), and programs for the channels."""

    def __init__(self):
        self.notes = []
        self.programs = {}
        self.controls = []

    def program(self, channel, number, volume=100, reverb=40, chorus=0, pan=64):
        self.programs[channel] = number
        for control, value in ((7, volume), (91, reverb), (93, chorus), (10, pan)):
            self.controls.append((channel, control, value))

    def note(self, channel, name, at_beats, beats, velocity):
        number = name if isinstance(name, int) else pitch(name)
        self.notes.append((at_beats * BEAT, channel, number, velocity, beats * BEAT))

    def midi(self, path):
        song = mido.MidiFile(ticks_per_beat=TICKS)
        track = mido.MidiTrack()
        song.tracks.append(track)
        track.append(mido.MetaMessage("set_tempo", tempo=mido.bpm2tempo(BPM), time=0))
        for channel, number in self.programs.items():
            track.append(mido.Message("program_change", channel=channel, program=number, time=0))
        for channel, control, value in self.controls:
            track.append(mido.Message("control_change", channel=channel, control=control, value=value, time=0))
        events = []
        for at, channel, number, velocity, lasts in self.notes:
            events.append((at, 1, mido.Message("note_on", channel=channel, note=number, velocity=velocity)))
            events.append((at + lasts, 0, mido.Message("note_off", channel=channel, note=number, velocity=0)))
        # (At the same moment, a note ends before the next begins.)
        events.sort(key=lambda event: (event[0], event[1]))
        ticks_before = 0
        for at, _, message in events:
            ticks = round(at / BEAT * TICKS)
            track.append(message.copy(time=ticks - ticks_before))
            ticks_before = ticks
        track.append(mido.MetaMessage("end_of_track", time=round(TAIL / BEAT * TICKS)))
        song.save(path)


LEAD, VOICE, KEYS, PAD, BASS, STRINGS, BELLS, DRUMS = 0, 1, 2, 3, 4, 5, 6, 9
KICK, STICK, SNARE, HAT, OPEN_HAT, CRASH, SHAKER, RIDE = 36, 37, 38, 42, 46, 49, 82, 51


def compose():
    score = Score()
    score.program(LEAD, 73, volume=112, reverb=70, chorus=20)      # flute
    score.program(VOICE, 54, volume=72, reverb=80, chorus=30)      # synth voice, under the flute
    score.program(KEYS, 4, volume=92, reverb=55, chorus=40, pan=52)   # electric piano
    score.program(PAD, 89, volume=70, reverb=90, pan=76)           # warm pad
    score.program(BASS, 33, volume=100, reverb=10)                 # fingered bass
    score.program(STRINGS, 48, volume=64, reverb=85, pan=80)       # strings
    score.program(BELLS, 9, volume=70, reverb=90, pan=48)          # glockenspiel
    score.controls.append((DRUMS, 7, 96))
    score.controls.append((DRUMS, 91, 35))

    full = lambda bar: 6 <= bar <= 9

    for bar, (root, chord) in enumerate(CHORDS):
        b = bar * 4
        last = bar == BARS - 1
        # The pad under it all, a bar at a time; the last held till the end.
        for name in chord[:3]:
            score.note(PAD, name, b, 4 if not last else 4 + TAIL / BEAT, 46 if bar < 6 else 54)
        if last:
            continue
        # The keys: broken chords in the quiet part, chords pushed on the off beats in the full one.
        if full(bar) or bar == 10:
            for at, lasts, velocity in ((0, 1.5, 70), (1.5, 1, 58), (2.5, 1.5, 64)) if bar != 10 else ((0, 4, 66),):
                for name in chord:
                    score.note(KEYS, name, b + at, lasts, velocity)
        else:
            order = [0, 1, 2, 3, 2, 1, 2, 3]
            for step, index in enumerate(order):
                score.note(KEYS, chord[index], b + step * .5, 1.0, 54 + (8 if step % 2 == 0 else 0))
        # The bass: in from the first line, on the beats; moving in eighths in the full part.
        if 2 <= bar <= 5:
            score.note(BASS, root, b, 1.9, 92)
            score.note(BASS, root, b + 2, 1.4, 84)
            score.note(BASS, pitch(root) + 7, b + 3.5, .5, 76)
        elif full(bar):
            for at, step, velocity in ((0, 0, 98), (1, 0, 80), (1.5, 12, 84), (2, 7, 88), (3, 0, 82), (3.5, 12, 78)):
                score.note(BASS, pitch(root) + step, b + at, .45, velocity)
        elif bar == 10:
            score.note(BASS, root, b, 4 + 4 + TAIL / BEAT, 96)
        # The strings, held, in the full part and at the close.
        if full(bar) or bar == 10:
            for name in chord[1:]:
                score.note(STRINGS, name, b, 4 if bar != 10 else 8, 58 if bar != 10 else 52)

    # The bells: a shimmer as it begins, and as it closes.
    for step, name in enumerate(["C6", "G5", "E6", "D6", "C6", "G5", "A5", "B5"]):
        score.note(BELLS, name, step * 1.0, 2, 62 - step * 2)
    for step, name in enumerate(["C6", "E6", "G6", "C7"]):
        score.note(BELLS, name, 40 + step * .5, 4, 66)

    # The drums. Two bars in: a shaker. The first lines: a soft kick, a side stick, closed hats. The full part: all of
    # it, a crash as it comes in, and a roll into it.
    for bar in range(BARS - 1):
        b = bar * 4
        if bar < 2:
            for step in range(8):
                score.note(DRUMS, SHAKER, b + step * .5, .25, 40 if step % 2 else 56)
        elif bar <= 5:
            score.note(DRUMS, KICK, b, .25, 86)
            score.note(DRUMS, KICK, b + 2.5, .25, 70)
            score.note(DRUMS, STICK, b + 1, .25, 74)
            score.note(DRUMS, STICK, b + 3, .25, 74)
            for step in range(8):
                score.note(DRUMS, HAT, b + step * .5, .25, 52 if step % 2 else 66)
            if bar == 5:
                for step, velocity in enumerate([56, 64, 72, 80, 88, 96]):
                    score.note(DRUMS, SNARE, b + 2.5 + step * .25, .25, velocity)
        elif full(bar):
            if bar in (6, 8):
                score.note(DRUMS, CRASH, b, 2, 92 if bar == 6 else 78)
            for at, velocity in ((0, 104), (1.5, 76), (2.5, 92)):
                score.note(DRUMS, KICK, b + at, .25, velocity)
            score.note(DRUMS, SNARE, b + 1, .25, 98)
            score.note(DRUMS, SNARE, b + 3, .25, 102)
            for step in range(8):
                score.note(DRUMS, OPEN_HAT if step == 7 and bar == 9 else HAT, b + step * .5, .25, 60 if step % 2 else 78)
            if bar == 9:
                for step, velocity in enumerate([70, 78, 86, 94]):
                    score.note(DRUMS, SNARE, b + 3 + step * .25, .25, velocity)
        elif bar == 10:
            score.note(DRUMS, CRASH, b, 4, 96)
            score.note(DRUMS, KICK, b, .25, 108)

    # The tune: the flute, the voice under it, each character a note.
    lyrics = []
    for bar, text, notes in LINES:
        assert len(text) == len(notes), text
        words = []
        for character, (name, at, beats) in zip(text, notes):
            begins = bar * 4 + at
            score.note(LEAD, name, begins, beats * .96, 96 if bar < 6 else 104)
            score.note(VOICE, name, begins, beats * .96, 64 if bar < 6 else 76)
            words.append({"at": round(begins * BEAT, 3), "end": round((begins + beats) * BEAT, 3), "text": character})
        lyrics.append({"at": words[0]["at"], "end": words[-1]["end"], "text": text, "words": words, "backing": None, "translation": None})
    return score, lyrics


def render(fluidsynth, soundfont, midi, out):
    raw = out.with_name("raw.wav")
    subprocess.run([fluidsynth, "-ni", "-q", "-g", "0.6", "-r", str(RATE), "-F", str(raw), str(soundfont), str(midi)], check=True)
    with wave.open(str(raw)) as read:
        channels, width = read.getnchannels(), read.getsampwidth()
        frames = read.readframes(read.getnframes())
    assert width == 2, width
    samples = np.frombuffer(frames, dtype="<i2").astype(np.float64).reshape(-1, channels) / 32768.0
    # As long as the song and its tail, its peaks at -1 dBFS, and in and out without a click.
    length = int((BARS * BAR + TAIL) * RATE)
    samples = np.pad(samples, ((0, max(0, length - len(samples))), (0, 0)))[:length]
    fade = int(TAIL * RATE)
    samples[-fade:] *= np.linspace(1, 0, fade)[:, None] ** 2
    samples *= 10 ** (-1 / 20) / max(np.abs(samples).max(), 1e-9)
    with wave.open(str(out), "wb") as write:
        write.setnchannels(2)
        write.setsampwidth(2)
        write.setframerate(RATE)
        write.writeframes((samples * 32767).round().astype("<i2").tobytes())
    raw.unlink()
    return samples


# As src/sound.rs measures what plays (see `Analysis::measure`).
BANDS, LOWEST, HIGHEST, WINDOW, FLOOR, CEILING = 24, 40.0, 16_000.0, 2048, -72.0, -22.0
STEP = 0.008


def heard(samples):
    mono = samples.mean(axis=1)
    shape = 0.5 - 0.5 * np.cos(2 * np.pi * np.arange(WINDOW) / (WINDOW - 1))
    decibels = lambda power: np.clip((10 * np.log10(np.maximum(power, 1e-12)) - FLOOR) / (CEILING - FLOOR), 0, 1)
    bin_of = lambda hz: int(np.clip(int(hz / RATE * WINDOW), 1, WINDOW // 2 - 1))
    edges = []
    for band in range(BANDS):
        low = LOWEST * (HIGHEST / LOWEST) ** (band / BANDS)
        high = LOWEST * (HIGHEST / LOWEST) ** ((band + 1) / BANDS)
        edges.append((bin_of(low), max(bin_of(high), bin_of(low) + 1)))
    bass_bins = slice(1, bin_of(260.0) + 1)
    kept_bands, kept_level, beat = np.zeros(BANDS), 0.0, 0.0
    flux_mean = flux_variance = 0.0
    bass_before = None
    since_beat = 1e9
    fall = min(STEP / 0.2, 1.0)
    keep = min(STEP / 1.2, 1.0)
    out = {"step": STEP, "level": [], "bands": [], "beat": []}
    count = int(len(mono) / RATE / STEP)
    for index in range(count):
        end = int(round((index + 1) * STEP * RATE))
        window = mono[max(0, end - WINDOW):end]
        window = np.pad(window, (WINDOW - len(window), 0))
        spectrum = np.fft.fft(window * shape)
        power = spectrum.real ** 2 + spectrum.imag ** 2
        bands = np.array([decibels(power[a:b].max() * 16.0 / (WINDOW * WINDOW)) for a, b in edges])
        level = decibels((window ** 2).sum() / WINDOW * 2.0)
        follow = lambda was, now: np.where(now > was, was + (now - was) * 0.7, was + (now - was) * fall)
        kept_bands = follow(kept_bands, bands)
        kept_level = float(follow(kept_level, level))
        bass = np.sqrt(power[bass_bins])
        flux = float(np.maximum(bass - bass_before, 0).sum()) if bass_before is not None else 0.0
        bass_before = bass
        spread = flux_variance ** 0.5
        since_beat += STEP
        is_beat = flux > flux_mean + 1.8 * spread and flux > 1e-3 and since_beat >= 0.28
        off = flux - flux_mean
        flux_mean += off * keep
        flux_variance = (1 - keep) * (flux_variance + keep * off * off)
        beat *= max(1 - STEP / 0.25, 0)
        if is_beat and kept_level > 0.05:
            since_beat = 0.0
            beat = 1.0
        out["level"].append(round(kept_level, 3))
        out["bands"].append([round(float(v), 3) for v in kept_bands])
        out["beat"].append(round(beat, 3))
    return out


def main():
    fluidsynth, soundfont, folder = map(Path, sys.argv[1:4])
    folder.mkdir(parents=True, exist_ok=True)
    score, lyrics = compose()
    score.midi(folder / "song.mid")
    samples = render(fluidsynth, soundfont, folder / "song.mid", folder / "song.wav")
    (folder / "lyrics.json").write_text(json.dumps(lyrics, ensure_ascii=False, indent=1), encoding="utf-8", newline="\n")
    analysed = heard(samples)
    (folder / "heard.json").write_text(json.dumps(analysed, separators=(",", ":")), encoding="utf-8", newline="\n")
    beats = sum(1 for before, now in zip(analysed["beat"], analysed["beat"][1:]) if now == 1.0 and before < 1.0)
    print(f"{len(samples) / RATE:.1f} s, {len(analysed['level'])} looks, {beats} beats, level {np.mean(analysed['level']):.2f} on average")


if __name__ == "__main__":
    main()
