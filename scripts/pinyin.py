"""Writes src/station/pinyin.bin: for each CJK character from U+3400 to U+9FFF, the first letters of its Mandarin
readings, as a 32-bit mask (bit 0 'a' … bit 25 'z'), little-endian, in order; 0 for one with none.

From the Unicode Character Database's Unihan_Readings.txt (kMandarin, kHanyuPinyin, kXHC1983, kTGHZ2013; Unicode
License v3, see licenses/THIRD-PARTY.txt):

    python scripts/pinyin.py path/to/Unihan_Readings.txt
"""
import re
import struct
import sys
import unicodedata

FIRST, LAST = 0x3400, 0x9FFF
FIELDS = {'kMandarin', 'kHanyuPinyin', 'kXHC1983', 'kTGHZ2013'}


def letter(reading):
    """A reading's first letter, its tone mark taken off: 'ài' -> 'a'."""
    first = unicodedata.normalize('NFD', reading.strip())[:1].lower()
    return first if 'a' <= first <= 'z' else None


masks = [0] * (LAST - FIRST + 1)
for line in open(sys.argv[1], encoding='utf-8'):
    if line.startswith('#') or not line.strip():
        continue
    code, field, value = line.rstrip('\n').split('\t', 2)
    if field not in FIELDS:
        continue
    point = int(code[2:], 16)
    if not FIRST <= point <= LAST:
        continue
    # kHanyuPinyin and kXHC1983 give "where:reading,reading"; kTGHZ2013 "where:reading"; kMandarin readings alone.
    readings = []
    for part in value.split(' '):
        readings.extend(part.split(':', 1)[-1].split(','))
    for reading in readings:
        found = letter(reading)
        if found:
            masks[point - FIRST] |= 1 << (ord(found) - ord('a'))
out = 'src/station/pinyin.bin'
with open(out, 'wb') as f:
    f.write(b''.join(struct.pack('<I', mask) for mask in masks))
print(out, sum(1 for m in masks if m), 'characters')
