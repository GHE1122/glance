"""The serif face the site's music full screen draws its lines in (Noto Serif SC
Black, as Glance does), cut down to the characters the song's lines and their
English use, and the ellipsis a line too long for its room ends in.

    python studio/site_serif.py SONG-JSON NotoSerifSC-Black.otf OUT.woff

Run again when the lyrics change.
"""
import json
import sys
from pathlib import Path

from fontTools import subset


def main():
    song, face, out = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
    lines = json.loads(song.read_text(encoding="utf-8"))["lines"]
    said = "".join(line["text"] + (line.get("en") or "") + "".join(word[2] for word in line["words"]) for line in lines)
    wanted = sorted(set(said + "…") - set("\n\r\t"))
    options = subset.Options()
    options.flavor = "woff"
    options.layout_features = ["*"]
    options.name_IDs = ["*"]
    options.notdef_outline = True
    font = subset.load_font(str(face), options)
    cutter = subset.Subsetter(options)
    cutter.populate(unicodes=[ord(ch) for ch in wanted])
    cutter.subset(font)
    out.parent.mkdir(parents=True, exist_ok=True)
    subset.save_font(font, str(out), options)
    print(out.name, out.stat().st_size, "bytes,", len(wanted), "characters")


if __name__ == "__main__":
    main()
