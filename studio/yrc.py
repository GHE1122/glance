"""A song's word-timed lyrics as NetEase gives them (its "yrc": each line
`[start,length]` and each word `(start,length,0)text`, in ms), written as
Glance's lyrics hold them (src/lyrics.rs: lines of words, in seconds), from
`from` seconds on, its times counted from there.

    python studio/yrc.py LYRIC.json OUT.json [FROM TO]

LYRIC.json is what https://music.163.com/api/song/lyric/v1?id=ID&lv=1&yv=1
answers. Lines of credits (in JSON, not timed by word) are left out; so are
lines outside FROM to TO, where they are given.
"""
import json
import re
import sys

LINE = re.compile(r"^\[(\d+),(\d+)\](.*)$")
WORD = re.compile(r"\((\d+),(\d+),\d+\)([^(]*)")


def lines(yrc):
    for row in yrc.splitlines():
        matched = LINE.match(row.strip())
        if not matched:
            continue
        at, lasts, rest = int(matched[1]) / 1000, int(matched[2]) / 1000, matched[3]
        words = [{"at": int(w[1]) / 1000, "end": (int(w[1]) + int(w[2])) / 1000, "text": w[3]} for w in WORD.finditer(rest)]
        if words:
            yield {"at": at, "end": at + lasts, "text": "".join(w["text"] for w in words), "words": words, "backing": None, "translation": None}


def main():
    said = json.load(open(sys.argv[1], encoding="utf-8"))
    out = sys.argv[2]
    start, stop = (float(sys.argv[3]), float(sys.argv[4])) if len(sys.argv) > 4 else (0.0, float("inf"))
    kept = []
    for line in lines(said["yrc"]["lyric"]):
        if line["at"] < start or line["at"] >= stop:
            continue
        shift = lambda t: round(t - start, 3)
        line = dict(line, at=shift(line["at"]), end=shift(line["end"]), words=[dict(w, at=shift(w["at"]), end=shift(w["end"])) for w in line["words"]])
        kept.append(line)
    with open(out, "w", encoding="utf-8", newline="\n") as file:
        json.dump(kept, file, ensure_ascii=False, indent=1)
    for line in kept:
        print(f"{line['at']:7.2f} {line['end']:7.2f}  {line['text']}")


if __name__ == "__main__":
    main()
