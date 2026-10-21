// What plays, as Glance's music widget shows it (src/ui/music.rs, drawn on a
// canvas): laid out by the room it is given (a strip, a player, a card of
// lines, a card stood on end, a stage with its record, a strip down), each
// of what it can show switched on or off, on the cover blurred behind it or
// on nothing (its words edged). The line sung now is filled as it is sung,
// the card's lines glide past, the record turns, the spectrum moves with what
// Glance hears of the song. Drawn in DIPs into a context the caller has
// placed; it keeps from frame to frame what the widget keeps.
(function () {
  "use strict";

  var GD = window.GlanceDesk = window.GlanceDesk || {};

  // ---------------- type ----------------
  // Glance's Segoe; Chinese in the face Windows falls back to for it.
  var SANS = '"Segoe UI Variable Text", "Segoe UI", -apple-system, "PingFang SC", "Microsoft YaHei UI", system-ui, sans-serif';
  var CJK_FACE = '"Microsoft YaHei UI", "PingFang SC", "Microsoft YaHei", "Noto Sans CJK SC", sans-serif';
  function Font(size, weight) { return { size: size, weight: weight }; }
  function sized(font, size) { return { size: size, weight: font.weight }; }
  function weighed(font, weight) { return { size: font.size, weight: weight }; }

  // The words: the line sung now in the card's lines, the others, the translation under it, what plays.
  var NOW_FONT = Font(25, 700), UNDER_FONT = Font(14, 500), TITLE = Font(15, 700), ABOUT = Font(12, 500);
  // The player's line sung now and what is under it; how long is left.
  var PLAYER_LINE = Font(18, 700), PLAYER_UNDER = Font(12.5, 500), TIME = Font(11, 500);
  // The strip's line, the least it is made to fit; large, in the middle (floating lyrics), and what is under it there.
  var STRIP_LINE = Font(16, 700), STRIP_LEAST = 12.5, FLOAT_LINE = Font(36, 800), FLOAT_UNDER = 0.5;
  var FONTS = { now: NOW_FONT, under_font: UNDER_FONT, title: TITLE, about: ABOUT, line: PLAYER_LINE, under: PLAYER_UNDER, time: TIME, strip: STRIP_LINE, float: FLOAT_LINE };

  // ---------------- its measures (DIPs, as music.rs has them) ----------------
  var PLAYER_FROM = 146, STRIP_UNDER = 230, CARD_FROM = 250, TALL_UNDER = 360, STAGE_FROM = 520, COLUMN_UNDER = 200, HOLD = 12;
  var LEAST = [56, 56], STRIP_MOST = 96, PLAYER_MOST = 236;
  var CONTROL = 32, KEY_ICON = 18, TOGGLE_ICON = 22, KEY_GAP = 2;
  var KEY_LEAD = (CONTROL - KEY_ICON) / 2 + KEY_ICON * 4 / 24;
  var SOUND_TRAIL = (CONTROL - KEY_ICON) / 2 + KEY_ICON * 1 / 24;
  var LIGHT_UP = 0.12, SOUND_LINGER = 150, STAGE_FOOT = 22, CORNER = 12, SOUND_WIDE = 72, SOUND_OPEN = 0.18;
  var SPECTRUM_ROW = 24, ROW_GAP = 12, SPECTRUM_FALL = 1.7, CONTROLS_FADE = 0.18, TURN = 260;
  var RECORD_TURN = 24, ARM_SWING = 0.6, ARM_ON = 100, ARM_OFF = 78, STAGE_SPECTRUM = 40;
  var INTERLUDE_DOT = 0.08, INTERLUDE_STEP = 0.43, INTERLUDE_WIDE = INTERLUDE_DOT * 2.5 + INTERLUDE_STEP * 2;
  var RECORD_LABEL = 0.62, BACKDROP_GROWN = 1.35, BACKDROP_DARKENED = 0.42, RADIUS = 14, HOSTED_COVER = 44;
  var TRANSLATION_ROW = 20, CARD_COVER = 64, SPECTRUM_PITCH = 7, DOWN_ROW = 1.2, BANDS = 24;
  // Its buttons while the pointer is over it (see ui/buttons.rs): their side and the room between. In a panel, in its
  // corner (panel.rs), the pin, the settings and the full screen. As a widget, in a row off its edge, BUTTON_INSET
  // from it (widget.rs `buttons`): the full screen, the settings, the pin, click-through and putting it away.
  var BUTTON = 22, BUTTON_GAP = 4, BUTTON_INSET = 6;
  var RACK = ["full", "settings", "pin", "through", "close"];
  // How far the row reaches past its edge, and how long it is.
  var REACH = BUTTON + 2 * BUTTON_INSET, ROW = RACK.length * BUTTON + (RACK.length - 1) * BUTTON_GAP;
  // The room round its box it draws in (DIPs, each side), as Glance's window for a widget has: its shadow, and its
  // buttons' row (widget.rs: the larger of the two).
  var MARGIN = Math.max(24, REACH);
  // Its colours with no cover.
  var TINT = [1.0, 0.9, 0.8], ACCENT = [1.0, 0.75, 0.6];
  // The sound as the studio pretends it (Glance shows the player's own): how loud, and not muted.
  var SOUND = [60, false];
  // The side of the cover's copy kept (pixels, square).
  var SIDE = 192;

  // ---------------- icons (Lucide, ISC: as src/ui/icons.rs has them) ----------------
  var ICONS = {
    skipBack: ["M19 20L9 12L19 4Z", "M5 19V5"],
    play: ["M6 3L20 12L6 21Z"],
    pause: ["M7 4h2a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1z", "M15 4h2a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1h-2a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1z"],
    skipForward: ["M5 4L15 12L5 20Z", "M19 5V19"],
    music: ["M9 18V5l12-2v13", "M9 18a3 3 0 1 1-6 0a3 3 0 1 1 6 0z", "M21 16a3 3 0 1 1-6 0a3 3 0 1 1 6 0z"],
    volume: ["M11 5L6 9H2v6h4l5 4V5Z", "M15.5 8.5a5 5 0 0 1 0 7", "M19 5a10 10 0 0 1 0 14"],
    muted: ["M11 5L6 9H2v6h4l5 4V5Z", "M22 9L16 15", "M16 9L22 15"],
    pointer: ["M4.037 4.688a.495.495 0 0 1 .651-.651l16 6.5a.5.5 0 0 1-.063.947l-6.124 1.58a2 2 0 0 0-1.438 1.435l-1.579 6.126a.5.5 0 0 1-.947.063z"],
    close: ["M18 6 6 18", "m6 6 12 12"],
    maximize: ["M8 3H5a2 2 0 0 0-2 2v3", "M21 8V5a2 2 0 0 0-2-2h-3", "M3 16v3a2 2 0 0 0 2 2h3", "M16 21h3a2 2 0 0 0 2-2v-3"],
    pin: ["M12 17v5", "M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z"],
    settings: ["M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915", "M15 12a3 3 0 1 1-6 0a3 3 0 1 1 6 0z"]
  };
  var PATHS = {};
  Object.keys(ICONS).forEach(function (k) { PATHS[k] = ICONS[k].map(function (d) { return new Path2D(d); }); });

  // ---------------- colours ----------------
  function Color(r, g, b, a) { return { r: r, g: g, b: b, a: a }; }
  function hex(h, a) { return Color((h >> 16 & 255) / 255, (h >> 8 & 255) / 255, (h & 255) / 255, a); }
  function colour(rgb, a) { return Color(rgb[0], rgb[1], rgb[2], a); }
  function alpha(c, a) { return Color(c.r, c.g, c.b, c.a * a); }
  function css(c) {
    var k = function (v) { return Math.round(Math.max(0, Math.min(1, v)) * 255); };
    return "rgba(" + k(c.r) + "," + k(c.g) + "," + k(c.b) + "," + Math.max(0, Math.min(1, c.a)) + ")";
  }
  function toward(a, b, t) { return [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]; }
  // A colour's hue (degrees), saturation and lightness, and back (src/cover.rs).
  function hsl(c) {
    var r = c[0], g = c[1], b = c[2], most = Math.max(r, g, b), least = Math.min(r, g, b), l = (most + least) / 2;
    if (most - least < 1e-6) return [0, 0, l];
    var span = most - least, s = span / Math.max(1e-6, 1 - Math.abs(2 * l - 1)), h;
    var rem = function (v, m) { return ((v % m) + m) % m; };
    if (most === r) h = 60 * rem((g - b) / span, 6); else if (most === g) h = 60 * ((b - r) / span + 2); else h = 60 * ((r - g) / span + 4);
    return [h, Math.min(1, s), l];
  }
  function rgb(h, s, l) {
    var rem = function (v, m) { return ((v % m) + m) % m; };
    var chroma = (1 - Math.abs(2 * l - 1)) * s, x = chroma * (1 - Math.abs(rem(h / 60, 2) - 1)), p;
    switch (Math.floor(rem(h, 360) / 60)) {
      case 0: p = [chroma, x, 0]; break; case 1: p = [x, chroma, 0]; break; case 2: p = [0, chroma, x]; break;
      case 3: p = [0, x, chroma]; break; case 4: p = [x, 0, chroma]; break; default: p = [chroma, 0, x];
    }
    var m = l - chroma / 2;
    return [p[0] + m, p[1] + m, p[2] + m];
  }
  // A colour light enough to read on the dark (its hue kept; its lightness raised to at least `least`).
  function readable(c, least) { var q = hsl(c); return rgb(q[0], Math.min(q[1], 0.85), Math.max(q[2], least)); }

  // A cover's main colours and how much of it each is: its pixels (at four bits a channel) split, box by box, along
  // the widest range of the box that holds the most range and weight, into `count` boxes, each by its weighted mean.
  function medianCut(px, count) {
    var counted = new Uint32Array(4096), i;
    for (i = 0; i < px.length; i += 4) counted[(px[i] >> 4) << 8 | (px[i + 1] >> 4) << 4 | px[i + 2] >> 4]++;
    var colours = [];
    for (i = 0; i < 4096; i++) if (counted[i] > 0) colours.push([[i >> 8, i >> 4 & 15, i & 15], counted[i]]);
    var boxes = [colours];
    var range = function (cs, ch) { var hi = 0, lo = 255; cs.forEach(function (c) { hi = Math.max(hi, c[0][ch]); lo = Math.min(lo, c[0][ch]); }); return cs.length ? hi - lo : 0; };
    var worth = function (cs) { return Math.max(range(cs, 0), range(cs, 1), range(cs, 2)) * Math.sqrt(cs.reduce(function (a, c) { return a + c[1]; }, 0)); };
    while (boxes.length < count) {
      // (The last of the most worth, as Rust's max_by takes it.)
      var at = -1, best = -Infinity;
      boxes.forEach(function (b, k) { if (b.length > 1) { var v = worth(b); if (v >= best) { best = v; at = k; } } });
      if (at < 0) break;
      var split = boxes[at];
      boxes[at] = boxes[boxes.length - 1]; boxes.pop();
      var channel = 0, widest = -1;
      [0, 1, 2].forEach(function (ch) { var r = range(split, ch); if (r >= widest) { widest = r; channel = ch; } });
      split = split.map(function (c, k) { return [c, k]; }).sort(function (a, b) { return a[0][0][channel] - b[0][0][channel] || a[1] - b[1]; }).map(function (p) { return p[0]; });
      var total = split.reduce(function (a, c) { return a + c[1]; }, 0), soFar = 0, middle = -1;
      for (var k = 0; k < split.length; k++) { soFar += split[k][1]; if (soFar * 2 >= total) { middle = k; break; } }
      middle = Math.min(Math.max(middle < 0 ? 0 : middle, 1), split.length - 1);
      boxes.push(split.slice(0, middle));
      boxes.push(split.slice(middle));
    }
    var all = Math.max(1, boxes.reduce(function (a, b) { return a + b.reduce(function (s, c) { return s + c[1]; }, 0); }, 0));
    return boxes.map(function (cs) {
      var weight = cs.reduce(function (a, c) { return a + c[1]; }, 0);
      var mean = function (ch) { return cs.reduce(function (a, c) { return a + (c[0][ch] * 17 + 8) * c[1]; }, 0) / Math.max(1, weight) / 255; };
      return [[mean(0), mean(1), mean(2)], weight / all];
    });
  }
  // The colours of a cover's pixels (RGBA): the most telling of its main colours, and a second of another hue.
  function coverColours(px) {
    var main = medianCut(px, 8);
    var telling = function (c) { var q = hsl(c); return q[1] * (1 - Math.abs(q[2] - 0.5) * 1.2); };
    var pick = function (list) { var best = null, v = -Infinity; list.forEach(function (c) { var t = telling(c); if (t >= v) { v = t; best = c; } }); return best; };
    var base = pick(main.map(function (m) { return m[0]; })) || [0.5, 0.5, 0.5];
    var q = hsl(base);
    var apart = function (c) { var away = Math.abs(hsl(c)[0] - q[0]); return Math.min(away, 360 - away); };
    var accent = pick(main.map(function (m) { return m[0]; }).filter(function (c) { var a = apart(c); return a >= 25 && a <= 160 && hsl(c)[1] > 0.15; }));
    return [base, accent || rgb((q[0] + 40) % 360, q[1], q[2])];
  }

  // A cover's pixels blurred (three box blurs, of `radius` pixels, across then down), for behind it.
  function blurred(px, side, radius) {
    var image = Float32Array.from(px), other = new Float32Array(px.length);
    for (var n = 0; n < 3; n++) { boxPass(image, other, side, radius, true); boxPass(other, image, side, radius, false); }
    var out = new Uint8ClampedArray(px.length);
    for (var i = 0; i < px.length; i++) out[i] = Math.round(image[i]);
    return out;
  }
  function boxPass(from, to, side, radius, across) {
    var at = across ? function (line, i) { return (line * side + i) * 4; } : function (line, i) { return (i * side + line) * 4; };
    var span = 2 * radius + 1, clamp = function (v) { return Math.max(0, Math.min(side - 1, v)); };
    for (var line = 0; line < side; line++) {
      for (var ch = 0; ch < 4; ch++) {
        var sum = 0;
        for (var k = 0; k <= 2 * radius; k++) sum += from[at(line, clamp(k - radius)) + ch];
        for (var i = 0; i < side; i++) {
          to[at(line, i) + ch] = sum / span;
          sum += from[at(line, Math.min(i + radius + 1, side - 1)) + ch] - from[at(line, clamp(i - radius)) + ch];
        }
      }
    }
  }

  // ---------------- words: measured and laid out as DirectWrite lays them ----------------
  var mctx = document.createElement("canvas").getContext("2d");
  function fontCss(font) { return font.weight + " " + font.size + "px " + SANS; }
  var widths = new Map();
  // `text` in runs as each is drawn: the CJK face has no weight between its regular and its bold, and DirectWrite takes
  // its regular up to a semibold asked for (where a browser takes its bold), so CJK runs of such a weight are set apart.
  function runsOf(text, font) {
    if (!(font.weight > 500 && font.weight <= 600)) return [[text, font]];
    var runs = [], regular = weighed(font, 500);
    for (var i = 0; i < text.length; i++) {
      var f = wide(text.charCodeAt(i)) ? regular : font, last = runs[runs.length - 1];
      if (last && last[1] === f) last[0] += text[i]; else runs.push([text[i], f]);
    }
    return runs.length ? runs : [[text, font]];
  }
  function measure(text, font) {
    var key = font.size + "|" + font.weight + "|" + text, w = widths.get(key);
    if (w == null) {
      w = 0;
      runsOf(text, font).forEach(function (run) { mctx.font = fontCss(run[1]); w += mctx.measureText(run[0]).width; });
      if (widths.size > 6000) widths.clear();
      widths.set(key, w);
    }
    return w;
  }
  // `text` drawn from its baseline's start at (x, y), in runs (see `runsOf`).
  function fill(c, text, font, x, y) {
    runsOf(text, font).forEach(function (run) { c.font = fontCss(run[1]); c.fillText(run[0], x, y); x += measure(run[0], run[1]); });
  }
  // Written in a CJK face (Chinese, Japanese, Korean, their punctuation and full-width forms).
  function wide(code) { return (code >= 0x2E80 && code <= 0x9FFF) || (code >= 0xAC00 && code <= 0xD7AF) || (code >= 0xF900 && code <= 0xFAFF) || (code >= 0xFF00 && code <= 0xFFEF); }
  function space(ch) { return /\s/.test(ch); }
  // A face's ascent and descent, of its size (the line box DirectWrite gives a run in it).
  var faces = {};
  function face(cjk) {
    var key = cjk ? "cjk" : "latin";
    if (!faces[key]) {
      mctx.font = "400 100px " + (cjk ? CJK_FACE : SANS);
      var m = mctx.measureText(cjk ? "国" : "H");
      faces[key] = { asc: m.fontBoundingBoxAscent / 100, desc: m.fontBoundingBoxDescent / 100 };
    }
    return faces[key];
  }
  // Where a line may break before unit `i` (between words; between characters of the scripts written without spaces,
  // not before closing marks nor after opening ones).
  var CLOSING = "，。、；：！？）」』〉》”’,.!?;:)]}…ー";
  var OPENING = "（「『〈《“‘([{";
  function mayBreak(text, i) {
    var a = text[i - 1], b = text[i];
    if (space(b)) return false;
    if (space(a)) return true;
    if (CLOSING.indexOf(b) >= 0 || OPENING.indexOf(a) >= 0) return false;
    return wide(a.charCodeAt(0)) || wide(b.charCodeAt(0));
  }
  function trimEnd(s) { return s.replace(/\s+$/, ""); }
  // `text` laid out in rows `width` wide (`wrap`: broken where it may, a word wider than a row broken where it must;
  // else one row): each row's units, its top, height and baseline, and where each unit of it begins.
  var layouts = new Map();
  function laid(text, font, width, wrap) {
    var key = font.size + "|" + font.weight + "|" + width + "|" + (wrap ? 1 : 0) + "|" + text;
    var hit = layouts.get(key);
    if (hit) return hit;
    var spans = [], n = text.length, start = 0;
    var fits = function (a, b) { return measure(trimEnd(text.slice(a, b)), font) <= width; };
    if (!wrap) spans.push([0, n]);
    while (wrap && start < n) {
      if (fits(start, n)) { spans.push([start, n]); break; }
      var best = -1;
      for (var b = start + 1; b < n; b++) {
        if (!mayBreak(text, b)) continue;
        if (fits(start, b)) best = b; else break;
      }
      if (best < 0) {
        best = start + 1;
        while (best < n && fits(start, best + 1)) best++;
      }
      spans.push([start, best]);
      start = best;
    }
    if (!spans.length) spans.push([0, 0]);
    var top = 0, most = 0;
    var rows = spans.map(function (sp) {
      var cjk = false, other = false;
      for (var i = sp[0]; i < sp[1]; i++) {
        var code = text.charCodeAt(i);
        if (wide(code)) cjk = true; else if (!space(text[i])) other = true;
      }
      if (!cjk) other = true;
      var asc = 0, desc = 0;
      [[cjk, true], [other, false]].forEach(function (f) { if (f[0]) { var m = face(f[1]); asc = Math.max(asc, m.asc); desc = Math.max(desc, m.desc); } });
      var xs = [];
      for (var k = 0; k <= sp[1] - sp[0]; k++) xs.push(measure(text.slice(sp[0], sp[0] + k), font));
      var row = { from: sp[0], to: sp[1], top: top, height: (asc + desc) * font.size, asc: asc * font.size, xs: xs };
      top += row.height;
      most = Math.max(most, measure(trimEnd(text.slice(sp[0], sp[1])), font));
      return row;
    });
    var out = { text: text, rows: rows, height: top, width: most };
    if (layouts.size > 1500) layouts.clear();
    layouts.set(key, out);
    return out;
  }
  function wrappedHeight(text, font, width) { return laid(text, font, width, true).height; }
  // Where the first row's baseline falls below its top, and how far its descent reaches below it.
  function baseline(font) { var r = laid("0", font, 1e4, false).rows[0]; return [r.asc, r.height - r.asc]; }
  // How far below the top of its line box `text`'s ink starts and ends.
  function ink(text, font) {
    mctx.font = fontCss(font);
    var m = mctx.measureText(text), b = baseline(font)[0];
    return [b - m.actualBoundingBoxAscent, b + m.actualBoundingBoxDescent];
  }

  // `text`, or as much of its start as is no wider than `room` with an ellipsis after it.
  function fitted(text, font, room) {
    if (measure(text, font) <= room) return text;
    var kept = Array.from(text);
    for (;;) {
      kept.pop();
      var cut = trimEnd(kept.join("")) + "…";
      if (!kept.length || measure(cut, font) <= room) return cut;
    }
  }
  // As DirectWrite trims a row too long for its box: as many characters as fit with the ellipsis after them.
  function trimmed(text, font, room) {
    if (measure(text, font) <= room + 0.01) return text;
    var kept = Array.from(text);
    while (kept.length && measure(kept.join("") + "…", font) > room) kept.pop();
    return kept.join("") + "…";
  }
  function pen(c, font, color) { c.font = fontCss(font); c.fillStyle = css(color); c.textAlign = "left"; c.textBaseline = "alphabetic"; }
  // `text` in one row, its box's corner at (x, y), `width` wide (cut short where too long; `end`: to its right side).
  function text1(c, text, font, color, x, y, width, end) {
    var shown = trimmed(text, font, width), row = laid(shown, font, 1e5, false).rows[0];
    pen(c, font, color);
    fill(c, shown, font, end ? x + width - measure(trimEnd(shown), font) : x, y + row.asc);
  }
  // `text` in rows `width` wide, the first row's box at (x, y).
  function textWrapped(c, text, font, color, x, y, width) {
    var L = laid(text, font, width, true);
    pen(c, font, color);
    L.rows.forEach(function (r) { fill(c, text.slice(r.from, r.to), font, x, y + r.top + r.asc); });
  }

  // ---------------- drawing ----------------
  function stretch(m) { return Math.max(1e-3, Math.sqrt(Math.abs(m.a * m.d - m.b * m.c))); }
  // Canvases of the caller's size kept for drawing apart (a layer, a glow), one per depth.
  var spare = [];
  function scratch(depth, w, h) {
    var s = spare[depth];
    if (!s) { var cv = document.createElement("canvas"); s = spare[depth] = { cv: cv, c: cv.getContext("2d") }; }
    if (s.cv.width < w) s.cv.width = w;
    if (s.cv.height < h) s.cv.height = h;
    s.c.setTransform(1, 0, 0, 1, 0, 0);
    s.c.globalAlpha = 1; s.c.filter = "none"; s.c.globalCompositeOperation = "source-over";
    s.c.clearRect(0, 0, w, h);
    return s;
  }
  var depth = 0;
  // What `paint` draws within (x, y, w, h) of the present place, drawn apart (on a canvas whose corner is that box's,
  // on the device): handed that canvas, placed as `c` is, then given back to be put on `c`.
  function apart(c, box, pad, paint, put) {
    var m = c.getTransform(), cv = c.canvas;
    var xs = [], ys = [];
    [[box[0], box[1]], [box[0] + box[2], box[1]], [box[0], box[1] + box[3]], [box[0] + box[2], box[1] + box[3]]].forEach(function (p) {
      xs.push(m.a * p[0] + m.c * p[1] + m.e); ys.push(m.b * p[0] + m.d * p[1] + m.f);
    });
    var left = Math.max(0, Math.floor(Math.min.apply(null, xs) - pad)), top = Math.max(0, Math.floor(Math.min.apply(null, ys) - pad));
    var right = Math.min(cv.width, Math.ceil(Math.max.apply(null, xs) + pad)), bottom = Math.min(cv.height, Math.ceil(Math.max.apply(null, ys) + pad));
    if (right <= left || bottom <= top) return;
    var w = right - left, h = bottom - top, s = scratch(depth, w, h);
    depth++;
    s.c.setTransform(m.a, m.b, m.c, m.d, m.e - left, m.f - top);
    paint(s.c);
    depth--;
    s.c.setTransform(1, 0, 0, 1, 0, 0);
    c.save();
    c.setTransform(1, 0, 0, 1, 0, 0);
    put(s, left, top, w, h);
    c.restore();
  }
  var EVERYTHING = [-1e5, -1e5, 2e5, 2e5];
  // What `paint` draws, `opacity` as opaque as a whole (a layer).
  function layer(c, opacity, paint) {
    if (opacity <= 0) return;
    apart(c, EVERYTHING, 0, paint, function (s, x, y, w, h) { c.globalAlpha = opacity; c.drawImage(s.cv, 0, 0, w, h, x, y, w, h); });
  }
  // A glow of `color` round what `paint` draws within `box` (which is not itself drawn): its shape blurred by a
  // deviation of `blur` DIPs.
  function glow(c, color, blur, box, paint) {
    if (color.a <= 0) return;
    var sigma = blur * stretch(c.getTransform());
    apart(c, box, sigma * 3 + 2, paint, function (s, x, y, w, h) {
      s.c.globalCompositeOperation = "source-in";
      s.c.fillStyle = css(Color(color.r, color.g, color.b, 1));
      s.c.fillRect(0, 0, w, h);
      s.c.globalCompositeOperation = "source-over";
      c.globalAlpha = color.a;
      c.filter = "blur(" + sigma + "px)";
      c.drawImage(s.cv, 0, 0, w, h, x, y, w, h);
    });
  }
  // A dark edge all round what `paint` draws (as words are drawn with no ground), `a` as strong.
  function edge(c, a, box, paint) { glow(c, hex(0x000000, 0.7 * a), 1.2, box, paint); }
  function roundPath(c, x, y, w, h, r) {
    r = Math.max(0, Math.min(r, w / 2, h / 2));
    c.beginPath();
    c.roundRect(x, y, w, h, r);
  }
  function fillRounded(c, color, x, y, w, h, r) {
    if (w <= 0 || h <= 0) return;
    c.fillStyle = css(color);
    roundPath(c, x, y, w, h, r);
    c.fill();
  }
  function fillRect(c, color, x, y, w, h) { c.fillStyle = css(color); c.fillRect(x, y, w, h); }
  function circle(c, color, x, y, r) { c.fillStyle = css(color); c.beginPath(); c.arc(x, y, Math.max(0, r), 0, Math.PI * 2); c.fill(); }
  function ring(c, color, x, y, r, line) { c.strokeStyle = css(color); c.lineWidth = line; c.beginPath(); c.arc(x, y, Math.max(0, r), 0, Math.PI * 2); c.stroke(); }
  // The shadow a rounded rectangle casts (`drop` below it), kept outside it.
  function shadow(c, color, x, y, w, h, r, blur, drop) {
    var m = c.getTransform(), far = 1e5;
    c.save();
    c.beginPath();
    c.rect(-far, -far, 2 * far, 2 * far);
    c.roundRect(x, y, w, h, Math.max(0, Math.min(r, w / 2, h / 2)));
    c.clip("evenodd");
    // (The shape drawn far off, its shadow cast back where it falls.)
    c.setTransform(m.a, m.b, m.c, m.d, m.e - far, m.f);
    c.shadowColor = css(color);
    c.shadowBlur = 2 * blur * stretch(m);
    c.shadowOffsetX = far + m.c * drop;
    c.shadowOffsetY = m.d * drop;
    fillRounded(c, hex(0, 1), x, y, w, h, r);
    c.restore();
  }
  function icon(c, kind, cx, cy, size, color) {
    c.save();
    c.translate(cx - size / 2, cy - size / 2);
    c.scale(size / 24, size / 24);
    c.strokeStyle = css(color); c.lineWidth = 2; c.lineCap = "round"; c.lineJoin = "round";
    PATHS[kind].forEach(function (p) { c.stroke(p); });
    c.restore();
  }
  function stroke(c, points, color, width) {
    c.strokeStyle = css(color); c.lineWidth = width; c.lineCap = "round"; c.lineJoin = "round";
    c.beginPath();
    points.forEach(function (p, k) { if (k) c.lineTo(p[0], p[1]); else c.moveTo(p[0], p[1]); });
    c.stroke();
  }
  function picture(c, img, x, y, w, h, r) {
    c.save();
    roundPath(c, x, y, w, h, r);
    c.clip();
    c.imageSmoothingEnabled = true; c.imageSmoothingQuality = "low";
    var side = Math.max(w, h);
    c.drawImage(img, x - (side - w) / 2, y - (side - h) / 2, side, side);
    c.restore();
  }
  function scaledAbout(c, x, y, k, paint) { c.save(); c.translate(x, y); c.scale(k, k); c.translate(-x, -y); paint(); c.restore(); }

  // Words, edged where there is no ground (`clear`) to be read on whatever is behind.
  function words(c, text, font, color, x, y, width, end, clear) {
    if (clear) edge(c, color.a, [x, y, width, font.size * 2], function (o) { text1(o, text, font, hex(0xFFFFFF, 1), x, y, width, end); });
    text1(c, text, font, color, x, y, width, end);
  }
  function outlined(c, text, font, color, x, y, width) { words(c, text, font, color, x, y, width, false, true); }
  // `text` written down (see `downWrap`) from (x, y), `wrap` wide; edged, with no ground.
  function wordsDown(c, text, font, color, x, y, wrap, clear) {
    if (clear) edge(c, color.a, [x, y, wrap, wrappedHeight(text, font, wrap)], function (o) { textWrapped(o, text, font, hex(0xFFFFFF, 1), x, y, wrap); });
    textWrapped(c, text, font, color, x, y, wrap);
  }

  // ---------------- the line sung, part by part ----------------
  // How a line is sung: each part's place in the text (UTF-16, from and to) and its time. Word by word where that is
  // known; else each character of Chinese or Japanese, each word of other writing, sharing nine tenths of its time.
  function parts(line, next) {
    var text = "", out = [];
    if (line.words.length) {
      line.words.forEach(function (w) { var from = text.length; text += w.text; out.push([from, text.length, w.at, Math.max(w.end, w.at + 0.01)]); });
      return { text: text, parts: out };
    }
    text = line.text;
    var end = Math.max(line.end != null ? line.end : next != null ? next : line.at + 4, line.at + 0.3), span = (end - line.at) * 0.9;
    var pieces = [], at = 0, run = null;
    var close = function (to) { if (run != null) { pieces.push([run, to, 1 + 0.15 * (to - run)]); run = null; } };
    Array.from(text).forEach(function (ch) {
      var units = ch.length, code = ch.codePointAt(0);
      var cjk = (code >= 0x3040 && code <= 0x30FF) || (code >= 0x3400 && code <= 0x4DBF) || (code >= 0x4E00 && code <= 0x9FFF) || (code >= 0xAC00 && code <= 0xD7AF) || (code >= 0xF900 && code <= 0xFAFF);
      if (cjk) { close(at); pieces.push([at, at + units, 1]); } else if (space(ch)) close(at); else if (run == null) run = at;
      at += units;
    });
    close(at);
    pieces.forEach(function (p, k) { p[1] = k + 1 < pieces.length ? pieces[k + 1][0] : at; });
    var weight = Math.max(1e-6, pieces.reduce(function (a, p) { return a + p[2]; }, 0)), t = line.at;
    pieces.forEach(function (p) { var took = span * p[2] / weight; out.push([p[0], p[1], t, t + took]); t += took; });
    return { text: text, parts: out };
  }
  // How far the text drawn is sung at `t`: all before `from`, and `part` of the way across from `from` to `to`.
  function sungTo(ps, t) {
    var done = { from: 0, to: 0, part: 0 };
    for (var i = 0; i < ps.length; i++) {
      var p = ps[i];
      if (t < p[2]) break;
      done = t >= p[3] ? { from: p[1], to: p[1], part: 0 } : { from: p[0], to: p[1], part: (t - p[2]) / (p[3] - p[2]) };
    }
    return done;
  }
  // How far through the music between lines it is (0 to 1) at `t`.
  function between(line, next, t) {
    var end = Math.max(line.end != null ? line.end : next != null ? next : line.at + 4, line.at + 0.3);
    return Math.max(0, Math.min(1, (t - line.at) / (end - line.at)));
  }
  // The music between lines: three dots in `dim`, each coming to `bright` (and a little larger) as its third goes.
  function interlude(c, x, y, size, done, dim, bright) {
    var radius = size * INTERLUDE_DOT, cy = y + size * 0.68;
    for (var i = 0; i < 3; i++) {
      var lit = Math.max(0, Math.min(1, done * 3 - i));
      lit = lit * lit * (3 - 2 * lit);
      var col = Color(dim.r + (bright.r - dim.r) * lit, dim.g + (bright.g - dim.g) * lit, dim.b + (bright.b - dim.b) * lit, dim.a + (bright.a - dim.a) * lit);
      if (col.a > 0) circle(c, col, x + radius * 1.25 + i * size * INTERLUDE_STEP, cy, radius * (1 + 0.25 * lit));
    }
  }
  // As much of a line as is sung, in `bright`, its edge soft: row by row as `L` breaks it, the row being sung up to
  // the place sung (whole, then fading over the soft edge in steps), the rows before it whole.
  function sungPart(c, L, text, font, x, y, width, sung, bright, soft) {
    if (sung.to === 0) return;
    var rows = L.rows, top, at;
    var place = function (unit) {
      for (var i = 0; i < rows.length; i++) { var r = rows[i]; if (unit >= r.from && unit < r.to) return [r.xs[unit - r.from], r.top]; }
      var last = rows[rows.length - 1];
      return [last.xs[Math.min(unit, last.to) - last.from], last.top];
    };
    var pieces = [];
    rows.forEach(function (r) {
      var a = Math.max(sung.from, r.from), b = Math.min(sung.to, r.to);
      if (b > a) pieces.push({ top: r.top, left: r.xs[a - r.from], width: r.xs[b - r.from] - r.xs[a - r.from] });
    });
    if (!pieces.length) {
      var p = place(sung.from);
      top = p[1]; at = p[0];
    } else {
      var left = pieces.reduce(function (s, q) { return s + q.width; }, 0) * sung.part, on = [pieces[0].top, pieces[0].left];
      for (var k = 0; k < pieces.length; k++) {
        var q = pieces[k];
        on = [q.top, q.left + Math.min(left, q.width)];
        if (left <= q.width) break;
        left -= q.width;
      }
      top = on[0]; at = on[1];
    }
    for (var i = 0; i < rows.length; i++) {
      var r = rows[i];
      if (r.top + r.height <= top + 0.5) {
        c.save(); c.beginPath(); c.rect(x - 4, y + r.top, width + 8, r.height); c.clip();
        textWrapped(c, text, font, bright, x, y, width);
        c.restore();
      } else if (r.top <= top + 0.5) {
        c.save(); c.beginPath(); c.rect(x - 4, y + r.top, at + 4, r.height); c.clip();
        textWrapped(c, text, font, bright, x, y, width);
        c.restore();
        for (var step = 0; step < 4; step++) {
          var from = at + soft * step / 4;
          c.save(); c.beginPath(); c.rect(x + from, y + r.top, soft / 4 + 0.5, r.height); c.clip();
          textWrapped(c, text, font, alpha(bright, 1 - (step + 0.5) / 4), x, y, width);
          c.restore();
        }
        break;
      }
    }
  }
  // A line drawn in `dim`, and over it in `bright` as far as it is sung.
  function filledRaw(c, L, text, font, x, y, width, sung, dim, bright, soft) {
    textWrapped(c, text, font, dim, x, y, width);
    sungPart(c, L, text, font, x, y, width, sung, bright, soft);
  }

  // The spectrum moving with the music: as many bars as fit `SPECTRUM_PITCH` apart across `width` from `x`,
  // standing on `floor`, `tall` at most, shaded from the accent to the tint, `a` of it shown.
  function spectrum(c, mo, accent, tint, x, floor, width, tall, a) {
    var bars = Math.max(8, Math.round(width / SPECTRUM_PITCH)), bar = width / bars, b = mo.bands;
    for (var i = 0; i < bars; i++) {
      var at = i / (bars - 1) * (BANDS - 1), low = Math.floor(at), frac = at - low;
      var value = b[low] * (1 - frac) + b[Math.min(low + 1, BANDS - 1)] * frac;
      var height = 2 + Math.pow(value, 1.6) * (tall - 2);
      fillRounded(c, colour(toward(accent, tint, i / bars), (0.3 + 0.45 * value) * a), x + i * bar + bar * 0.2, floor - height, bar * 0.6, height, bar * 0.3);
    }
  }

  // ---------------- layouts ----------------
  function more(shows) { return shows.controls && shows.spectrum ? SPECTRUM_ROW + ROW_GAP : 0; }
  function stripUnder(m) { return (STRIP_MOST + PLAYER_FROM + m) / 2; }
  function playerZoom(h, m) { return Math.min(1, h / (PLAYER_FROM + m)); }
  function zoomAt(layout, size, shows) { return layout === "player" ? playerZoom(size[1], more(shows)) : 1; }
  // The layout for a room `w` × `h` showing `shows`: the one it had while the room is within `HOLD` of where the next
  // begins, so that a room sized about a threshold does not turn back and forth.
  function forRoom(size, shows, had) {
    var w = size[0], h = size[1], m = more(shows);
    var pick = function (slack) {
      if (w < COLUMN_UNDER + slack && h >= w) return "column";
      if (h < stripUnder(m) + slack || w < STRIP_UNDER + slack) return "strip";
      if (h < CARD_FROM + m + slack) return "player";
      if (shows.cover && w >= STAGE_FROM + slack && h >= CARD_FROM + slack) return "stage";
      if (w >= TALL_UNDER + slack) return "card";
      return "tall";
    };
    var plain = pick(0);
    if (had && had !== plain && (pick(-HOLD) === had || pick(HOLD) === had)) return had;
    return plain;
  }
  // The size a music widget sized to `room` comes to rest at: the room, within the range its layout is right in.
  function restsAt(room, shows) {
    room = [Math.max(room[0], LEAST[0]), Math.max(room[1], LEAST[1])];
    var m = more(shows);
    switch (forRoom(room, shows, null)) {
      case "strip": return [room[0], Math.min(room[1], STRIP_MOST)];
      case "player": return [room[0], Math.max(PLAYER_FROM + m, Math.min(PLAYER_MOST + m, room[1]))];
      default: return room;
    }
  }
  function pad(layout) { return layout === "strip" || layout === "column" ? 10 : layout === "player" ? 18 : layout === "card" || layout === "tall" ? 22 : 30; }
  function padOf(layout, shows) { return shows.inset != null ? shows.inset : pad(layout); }
  function rowsTall(shows) { return CONTROL + (shows.controls && shows.spectrum ? ROW_GAP + SPECTRUM_ROW : 0); }
  function stageLine(h) { return Math.max(18, Math.min(NOW_FONT.size, h / 16)); }
  function coverCorner(side) { return side < 50 ? 6 : 9; }
  // The stage's record: its middle and how wide it is.
  function stageRecord(size, shows) {
    var p = pad("stage");
    var rows = shows.controls && shows.spectrum ? CONTROL + ROW_GAP + STAGE_SPECTRUM : !shows.controls && shows.spectrum ? STAGE_SPECTRUM : CONTROL;
    var above = size[1] - p - STAGE_FOOT - rows - 14;
    var side = Math.max(0, Math.min(above, size[0] * 0.4));
    return [[p + side / 2, p + Math.max(0, above) / 2], side];
  }
  function playerSide(size, shows) {
    var p = padOf("player", shows);
    return Math.max(0, Math.min(Math.max(0, size[1] - 2 * p - rowsTall(shows) - 10), size[0] * 0.36));
  }
  // Where its parts go, as `layout` lays them out in `size` showing `shows`.
  function placed(layout, size, shows) {
    var w = size[0], h = size[1], p = padOf(layout, shows), P = { cover: null, round: 0, record: 0, title: null, titleZoom: 1, below: 0, left: 0, width: 0 };
    var side, top, left;
    switch (layout) {
      case "strip": {
        var m = more(shows), zoom = playerZoom(stripUnder(m), m);
        var player = playerSide([w / zoom, PLAYER_FROM + m], shows) * zoom;
        side = Math.max(0, Math.min(h - 2 * p, shows.inset != null ? HOSTED_COVER : Infinity, player));
        P.cover = shows.cover ? [p, (h - side) / 2, side] : null;
        P.left = shows.cover ? p + side + 12 : p + 4;
        P.width = Math.max(0, w - p - P.left);
        return P;
      }
      case "player": {
        var room = Math.max(0, h - 2 * p - rowsTall(shows) - 10);
        side = playerSide(size, shows);
        top = shows.cover ? p + (room - side) / 2 : p;
        P.cover = shows.cover ? [p, top, side] : null;
        P.left = shows.cover ? p + side + 16 : p;
        P.width = Math.max(0, w - P.left - p);
        P.title = shows.title ? [P.left, top - 2, P.width, false] : null;
        P.below = shows.title ? top + 46 : top + 4;
        return P;
      }
      case "card": {
        P.cover = shows.cover ? [p, p, CARD_COVER] : null;
        var tx = shows.cover ? p + CARD_COVER + 14 : p;
        P.title = shows.title ? [tx, p + (shows.cover ? 12 : 0), Math.max(0, w - tx - p), false] : null;
        P.below = p + (shows.cover ? CARD_COVER + 12 : shows.title ? 44 : 0);
        P.left = p; P.width = Math.max(0, w - 2 * p);
        return P;
      }
      case "stage": {
        var rec = stageRecord(size, shows), cx = rec[0][0], cy = rec[0][1];
        side = rec[1]; top = cy - side / 2;
        var label = side / 2 * RECORD_LABEL;
        P.cover = [cx - label, cy - label, 2 * label];
        P.round = 1; P.record = 1;
        P.left = p + side + side * 0.28 + 12;
        P.width = Math.max(0, w - P.left - p);
        P.title = shows.title ? [P.left, top + 6, P.width, false] : null;
        P.titleZoom = stageLine(h) / 18;
        P.below = top + (shows.title ? 58 * P.titleZoom : 0);
        return P;
      }
      case "column": {
        var across = Math.max(0, w - 2 * p);
        side = Math.max(0, Math.min(across, h * 0.4));
        P.cover = shows.cover ? [(w - side) / 2, p, side] : null;
        P.below = p + (shows.cover ? side + 10 : 0);
        P.left = p; P.width = across;
        return P;
      }
      default: {
        var wide = Math.max(0, w - 2 * p);
        side = Math.min(wide, h * 0.3);
        P.cover = shows.cover ? [(w - side) / 2, p, side] : null;
        var y = p + (shows.cover ? side + 12 : 0);
        P.title = shows.title ? [p, y, wide, true] : null;
        if (shows.title) y += 44;
        P.below = y; P.left = p; P.width = wide;
        return P;
      }
    }
  }
  function scaledPlaced(P, k) {
    return { cover: P.cover && [P.cover[0] * k, P.cover[1] * k, P.cover[2] * k], round: P.round, record: P.record,
      title: P.title && [P.title[0] * k, P.title[1] * k, P.title[2] * k, P.title[3]], titleZoom: P.titleZoom * k, below: P.below * k, left: P.left * k, width: P.width * k };
  }
  // Its parts `t` of the way from where they were to where they go: each there is in both moved, the others where they go.
  function tween(a, b, t) {
    var at = function (p, q) { return p + (q - p) * t; };
    return { cover: a.cover && b.cover ? [at(a.cover[0], b.cover[0]), at(a.cover[1], b.cover[1]), at(a.cover[2], b.cover[2])] : b.cover,
      title: a.title && b.title ? [at(a.title[0], b.title[0]), at(a.title[1], b.title[1]), at(a.title[2], b.title[2]), b.title[3]] : b.title,
      round: at(a.round, b.round), record: at(a.record, b.record), titleZoom: at(a.titleZoom, b.titleZoom), below: at(a.below, b.below), left: at(a.left, b.left), width: at(a.width, b.width) };
  }
  // Whether the player shows the lines round the one sung now (else the line sung now and what is under it).
  function playerShowsLines(size, shows) {
    var at = placed("player", size, shows), bottom = size[1] - pad("player") - rowsTall(shows) - 8, line = PLAYER_LINE.size * 1.35;
    return bottom - at.below >= line * (1 + 2 * 0.68) + 2 * 10;
  }
  // What a strip's row ends with: where its keys and its spectrum go, where its words end, and, centred, how far in
  // from either side they keep.
  function stripEnds(w, start, p, shows, centred) {
    var keysWide = 3 * CONTROL + 2 * KEY_GAP, end = w - p, ends = { keys: null, spectrum: null, end: end, side: 0 };
    if (centred) {
      var side = keysWide + 14;
      if (w - 2 * (p + side) >= 0) { ends.keys = shows.controls ? end - keysWide : p; ends.side = side; }
      return ends;
    }
    var fits = function (e, taken) { return e - taken >= start; };
    if (shows.controls) {
      if (fits(end, keysWide + 10)) {
        ends.keys = end - keysWide;
        end -= keysWide + 10;
        if (shows.spectrum && fits(end, 80)) { ends.spectrum = [end - 70, 70]; end -= 80; }
      }
    } else {
      var wide = Math.max(keysWide, 84);
      if (fits(end, wide + 12)) {
        ends.spectrum = shows.spectrum ? [end - wide, wide] : null;
        ends.keys = end - keysWide;
        end -= wide + 12;
      }
    }
    ends.end = end;
    return ends;
  }
  function ease(t) { return 1 - Math.pow(1 - Math.max(0, Math.min(1, t)), 4); }
  function clock(s) { var m = Math.floor(s / 60), r = s % 60; return Math.floor(m / 60) > 0 ? Math.floor(m / 60) + ":" + String(m % 60).padStart(2, "0") + ":" + String(r).padStart(2, "0") : m + ":" + String(r).padStart(2, "0"); }
  function easedToward(now, to, step) { var by = Math.min(step, 1 / 30) / LIGHT_UP; return to > now ? Math.min(now + by, to) : Math.max(now - by, to); }
  // Whether `text` is in a script written down a character under another.
  function writtenDown(text) { for (var i = 0; i < text.length; i++) { var c = text.charCodeAt(i); if ((c >= 0x2E80 && c <= 0x9FFF) || (c >= 0xAC00 && c <= 0xD7AF) || (c >= 0xF900 && c <= 0xFAFF) || (c >= 0xFF00 && c <= 0xFFEF)) return true; } return false; }
  function downWrap(text, font, column, down) { return down && writtenDown(text) ? Math.min(font.size * DOWN_ROW, column) : column; }
  function rowOf(text, font, wrap) { var L = laid(text, font, wrap, true); return L.height / Math.max(1, L.rows.length); }
  function unbroken(text, font, column, down) { return (down && writtenDown(text)) || text.split(/\s+/).filter(Boolean).every(function (w) { return measure(w, font) <= column; }); }
  function sizedDown(text, font, column, room, down) {
    var widest = Math.max(1, down && writtenDown(text) ? Math.min(font.size, column / DOWN_ROW) : font.size), least = Math.min(STRIP_LEAST, widest);
    for (var size = widest; size >= least; size -= 1) {
      var f = sized(font, size);
      if (unbroken(text, f, column, down) && wrappedHeight(text, f, downWrap(text, f, column, down)) <= room) return f;
    }
    return sized(font, least);
  }

  // ---------------- one frame drawn ----------------
  function makeMusic(song, cover, lang) {
    // The cover as Glance keeps it: squared at its side, its colours, blurred for behind it.
    var copy = document.createElement("canvas");
    copy.width = copy.height = SIDE;
    var cc = copy.getContext("2d");
    cc.imageSmoothingQuality = "high";
    var cw = cover.naturalWidth || cover.width, ch = cover.naturalHeight || cover.height, cs = Math.min(cw, ch);
    cc.drawImage(cover, (cw - cs) / 2, (ch - cs) / 2, cs, cs, 0, 0, SIDE, SIDE);
    var pixels = cc.getImageData(0, 0, SIDE, SIDE).data;
    var cols = coverColours(pixels);
    var groundPixels = blurred(pixels, SIDE, 9);
    var backdrop = document.createElement("canvas");
    backdrop.width = backdrop.height = SIDE;
    backdrop.getContext("2d").putImageData(new ImageData(groundPixels, SIDE, SIDE), 0, 0);

    // What is heard of it, every `step` seconds.
    var bytes = function (b64) { var s = atob(b64), out = new Uint8Array(s.length); for (var i = 0; i < s.length; i++) out[i] = s.charCodeAt(i); return out; };
    var heard = song.heard, spectrumBytes = bytes(heard.spectrum);
    var heardAt = function (at) {
      var out = new Array(BANDS).fill(0);
      if (at < heard.step) return out;
      var index = Math.min(Math.floor(at / heard.step) - 1, heard.count - 1);
      for (var b = 0; b < BANDS; b++) out[b] = spectrumBytes[index * heard.bands + b] / 255;
      return out;
    };
    var en = lang === "en";
    var lines = song.lines.map(function (l) {
      return { at: l.at, end: l.end, text: l.text, translation: en ? l.en : null, words: l.words.map(function (w) { return { at: w[0], end: w[1], text: w[2] }; }) };
    });
    var EARLY = 0.2;
    var sungAt = function (position) { for (var i = lines.length - 1; i >= 0; i--) if (lines[i].at <= position + EARLY) return i; return null; };

    var shows = { cover: true, title: true, translation: en, spectrum: true, controls: false, clear: false, inset: null };
    // What it keeps from frame to frame. (Begun as it is once it has played a while, as the studio films it after
    // drawing it for a while: its colours the cover's, its record's arm on it.)
    var S = {
      layout: null, rail: -1, railSpeed: 0, fresh: true, lastFrame: null,
      tint: readable(cols[0], 0.72), accent: readable(cols[1], 0.62),
      controlsIn: 0, soundOpen: false, soundIn: 0, trackLit: 0, soundLit: 0, easing: false, soundLeft: null,
      bands: new Array(BANDS).fill(0), anchor: null, step: 0, anchoring: false, turnedBy: 0, arm: 1,
      turning: null, shown: null, hits: [], drawnAt: null
    };
    function controlsShown() { return S.controlsIn * S.controlsIn * (3 - 2 * S.controlsIn); }
    function sameShows(a, b) { return ["cover", "title", "translation", "spectrum", "controls", "clear", "inset"].every(function (k) { return a[k] === b[k]; }); }
    function copyShows(s) { return { cover: !!s.cover, title: !!s.title, translation: !!s.translation, spectrum: !!s.spectrum, controls: !!s.controls, clear: !!s.clear, inset: s.inset == null ? null : s.inset }; }
    function turnedT(now, since) { var t = (now - since) / TURN; return t < 1 ? ease(t) : null; }

    // The colour its own ground shows most along its edges (for the buttons set on it).
    function groundColour(w, h) {
      var laidSide = Math.max(w, h, 1) * BACKDROP_GROWN, hx = w / 2 / laidSide, hy = h / 2 / laidSide, band = Math.min(w, h) * 0.15 / laidSide;
      var sum = [0, 0, 0], count = 0;
      for (var y = 0; y < SIDE; y++) {
        var v = (y + 0.5) / SIDE - 0.5;
        for (var x = 0; x < SIDE; x++) {
          var u = (x + 0.5) / SIDE - 0.5;
          if (Math.abs(u) <= hx && Math.abs(v) <= hy && (Math.abs(u) >= hx - band || Math.abs(v) >= hy - band)) {
            var p = (y * SIDE + x) * 4;
            sum[0] += groundPixels[p]; sum[1] += groundPixels[p + 1]; sum[2] += groundPixels[p + 2]; count++;
          }
        }
      }
      return sum.map(function (s, i) { return s / Math.max(1, count) / 255 * (1 - BACKDROP_DARKENED) + S.accent[i] * 0.06; });
    }
    var groundCache = { key: "", value: null };
    function groundAt(w, h) { var k = w + "x" + h; if (groundCache.key !== k) groundCache = { key: k, value: groundColour(w, h) }; return groundCache.value; }

    // ---------------- the parts of a frame (D: the frame being drawn) ----------------
    function placedD(D, layout) { var to = placed(layout, D.size, D.shows); return D.turn ? tween(D.turn.from, to, D.turn.t) : to; }
    function clip(D, x, y, w, h) { D.c.save(); D.c.beginPath(); D.c.rect(x, y, w, h); D.c.clip(); D.clips.push([[x, y, w, h], D.hits.length]); }
    function unclip(D) {
      D.c.restore();
      var last = D.clips.pop(), r = last[0];
      var within = D.hits.splice(last[1]).map(function (b) {
        var l = Math.max(b[0], r[0]), t = Math.max(b[1], r[1]), rr = Math.min(b[0] + b[2], r[0] + r[2]), bb = Math.min(b[1] + b[3], r[1] + r[3]);
        return rr > l && bb > t ? [l, t, rr - l, bb - t, b[4]] : null;
      }).filter(Boolean);
      Array.prototype.push.apply(D.hits, within);
    }
    function came(D) { return D.mo.since == null ? 1 : 1 - Math.pow(1 - Math.min(1, D.mo.since / 0.28), 3); }
    function lineColours(D, eased) {
      var dim = D.edged ? 0.6 : 0.36;
      return [hex(D.ink, dim * eased), colour(toward(D.tint, [1, 1, 1], 0.65), eased)];
    }
    function under(D, ls, now) {
      var t = D.shows.translation ? ls[now].translation : null;
      if (t != null) return t;
      return ls[now + 1] ? ls[now + 1].text : "";
    }
    function filled(D, c, L, text, font, x, y, width, sung, dim, bright, soft) {
      if (D.edged) edge(c, bright.a, [x, y, width, L.height], function (o) { textWrapped(o, text, font, hex(0xFFFFFF, 1), x, y, width); });
      filledRaw(c, L, text, font, x, y, width, sung, dim, bright, soft);
    }

    // Its ground (the cover blurred under a wash of its colour, inside its rounded corners, with a shadow; none,
    // clear), the cover and what plays.
    function ground(D, layout) {
      var c = D.c, w = D.size[0], h = D.size[1], at = placedD(D, layout), own = !D.shows.clear, radius = Math.min(RADIUS, h / 2);
      var on = function () {
        if (at.cover && at.record <= 0) {
          var x = at.cover[0], y = at.cover[1], side = at.cover[2], corner = coverCorner(side);
          if (D.shows.inset == null) shadow(c, hex(0, 0.35), x, y, side, side, corner, 10, 3);
          picture(c, copy, x, y, side, side, corner);
        }
        if (at.title) {
          var zoom = at.titleZoom, tf = sized(FONTS.title, FONTS.title.size * zoom), ab = sized(FONTS.about, FONTS.about.size * zoom);
          [[song.title, tf, 0.95, 0], [song.artist, ab, 0.62, 21 * zoom]].forEach(function (one) {
            var shown = fitted(one[0], one[1], at.title[2]);
            var from = at.title[3] ? at.title[0] + (at.title[2] - measure(shown, one[1])) / 2 : at.title[0];
            words(c, shown, one[1], hex(D.ink, one[2]), from, at.title[1] + one[3], at.title[2] + 2, false, D.edged);
          });
        }
      };
      if (!own) { on(); return; }
      shadow(c, hex(0, 0.4), 0, 0, w, h, radius, 8, 4);
      c.save();
      roundPath(c, 0, 0, w, h, radius);
      c.clip();
      fillRect(c, hex(0x101014, 1), 0, 0, w, h);
      var b = Math.max(w, h) * BACKDROP_GROWN;
      c.imageSmoothingEnabled = true; c.imageSmoothingQuality = "low";
      c.drawImage(backdrop, (w - b) / 2, (h - b) / 2, b, b);
      fillRect(c, hex(0, BACKDROP_DARKENED), 0, 0, w, h);
      var g = c.createLinearGradient(0, 0, 0, h);
      g.addColorStop(0, css(colour(S.accent, 0.12)));
      g.addColorStop(1, css(colour(S.tint, 0)));
      c.fillStyle = g;
      c.fillRect(0, 0, w, h);
      on();
      c.restore();
    }

    // The record round the cover, as much of it as the layout shows (the stage's, or growing in or going as a layout
    // turns into the stage or out of it), the cover in its middle turned as the record is, as far as it is round.
    function disc(D, layout) {
      var at = placedD(D, layout);
      if (!at.cover || at.record <= 0) return;
      var c = D.c, x = at.cover[0], y = at.cover[1], side = at.cover[2], cx = x + side / 2, cy = y + side / 2, label = side / 2, r = label / RECORD_LABEL;
      var faded = function (paint) { if (at.record < 1) layer(D.c, at.record, function (o) { var was = D.c; D.c = o; paint(); D.c = was; }); else paint(); };
      faded(function () { recordUnder(D, cx, cy, r); });
      var corner = coverCorner(side) + (label - coverCorner(side)) * at.round;
      var angle = S.turnedBy * at.round * Math.PI / 180;
      c.save(); c.translate(cx, cy); c.rotate(angle); c.translate(-cx, -cy);
      picture(c, copy, x, y, side, side, corner);
      c.restore();
      faded(function () { recordOver(D, cx, cy, r); });
    }
    function recordUnder(D, cx, cy, r) {
      var c = D.c, side = 2 * r, label = r * RECORD_LABEL;
      shadow(c, hex(0, 0.45), cx - r, cy - r, side, side, r, 14, 5);
      circle(c, hex(0x0E0E10, 1), cx, cy, r);
      // Its grooves: fine rings, lighter toward its edge.
      for (var g = label + 3; g < r - 3; g += 4) ring(c, hex(0xFFFFFF, 0.035 + 0.05 * (g - label) / (r - label)), cx, cy, g, 0.8);
      ring(c, hex(0xFFFFFF, 0.12), cx, cy, r - 0.5, 1);
    }
    function recordOver(D, cx, cy, r) {
      var c = D.c, label = r * RECORD_LABEL;
      ring(c, colour(S.accent, 0.9), cx, cy, label + 2, 2.5);
      for (var sheen = r - 6; sheen > label + 8; sheen -= 11) ring(c, colour(S.accent, 0.05 * (sheen - label) / (r - label)), cx, cy, sheen, 1.5);
      circle(c, hex(0x0E0E10, 1), cx, cy, Math.max(r * 0.04, 3));
      armOf(D, cx, cy, r);
    }
    // The record's arm: its pivot past the record's top right, bent a little as a tonearm is, to its headshell, on the
    // grooves while something plays and swung off it while nothing does.
    function armOf(D, cx, cy, r) {
      var c = D.c, pivot = [cx + r * 0.98, cy - r * 0.98], reach = r * 1.35;
      var swung = S.arm * S.arm * (3 - 2 * S.arm), angle = (ARM_OFF + (ARM_ON - ARM_OFF) * swung) * Math.PI / 180;
      var head = [pivot[0] + reach * Math.cos(angle), pivot[1] + reach * Math.sin(angle)];
      var dx = head[0] - pivot[0], dy = head[1] - pivot[1];
      var bend = [(pivot[0] + head[0]) / 2 - dy * 0.16, (pivot[1] + head[1]) / 2 + dx * 0.16];
      var curve = [];
      for (var s = 0; s <= 24; s++) {
        var t = s / 24, a = (1 - t) * (1 - t), b = 2 * (1 - t) * t, q = t * t;
        curve.push([a * pivot[0] + b * bend[0] + q * head[0], a * pivot[1] + b * bend[1] + q * head[1]]);
      }
      var thick = Math.max(r * 0.028, 2.5);
      stroke(c, curve.map(function (p) { return [p[0] + 1.5, p[1] + 2.5]; }), hex(0, 0.28), thick + 1);
      stroke(c, curve, hex(0xF4F4F6, 1), thick);
      var along = Math.atan2(head[1] - bend[1], head[0] - bend[0]);
      var long = Math.max(r * 0.2, 16), wide = Math.max(r * 0.075, 7);
      c.save(); c.translate(head[0], head[1]); c.rotate(along); c.translate(-head[0], -head[1]);
      shadow(c, hex(0, 0.35), head[0] - long * 0.3, head[1] - wide / 2, long, wide, wide * 0.3, 4, 2);
      fillRounded(c, hex(0xF4F4F6, 1), head[0] - long * 0.3, head[1] - wide / 2, long, wide, wide * 0.3);
      fillRounded(c, hex(0x2A2A2E, 0.85), head[0] + long * 0.12, head[1] - wide * 0.32, long * 0.08, wide * 0.64, 1);
      fillRounded(c, colour(S.accent, 1), head[0] + long * 0.32, head[1] - wide * 0.18, long * 0.34, wide * 0.36, wide * 0.18);
      c.restore();
      var rr = Math.max(r * 0.075, 7);
      shadow(c, hex(0, 0.4), pivot[0] - rr, pivot[1] - rr, 2 * rr, 2 * rr, rr, 5, 2);
      circle(c, hex(0xF4F4F6, 1), pivot[0], pivot[1], rr);
      circle(c, hex(0x2A2A2E, 1), pivot[0], pivot[1], rr * 0.48);
      circle(c, hex(0xFFFFFF, 0.5), pivot[0] - rr * 0.15, pivot[1] - rr * 0.15, rr * 0.14);
    }

    // The rows along the bottom, from `x`, `wide`, up from `edge`: the controls kept showing in a row of their own with
    // the spectrum's row above them; else the spectrum, the controls coming over it as the pointer does.
    function rows(D, x, edgeY, wide, beside) {
      var row = edgeY - CONTROL;
      if (D.shows.controls) {
        bottomRow(D, x, x + wide, row, 1, beside);
        if (!D.shows.spectrum) return row;
        spectrum(D.c, D.mo, D.accent, D.tint, x, row - ROW_GAP, wide, SPECTRUM_ROW, 0.85);
        return row - ROW_GAP - SPECTRUM_ROW;
      }
      var shownIn = controlsShown();
      if (shownIn < 1 && D.shows.spectrum) spectrum(D.c, D.mo, D.accent, D.tint, x, edgeY - 2, wide, CONTROL - 4, 1 - shownIn);
      if (shownIn > 0) bottomRow(D, x, x + wide, row, shownIn, beside);
      return row;
    }
    function bottomRow(D, x, right, row, fade, beside) {
      var gap = beside ? Math.max(KEY_GAP, (beside[0] - x + 2 * KEY_LEAD - 3 * CONTROL) / 2) : KEY_GAP;
      var end = keys(D, x - KEY_LEAD, gap, row, fade);
      var from = beside ? Math.max(beside[1], end + 6) : end + 10;
      soundKey(D, from, right + SOUND_TRAIL - CONTROL, row, fade);
    }
    function keys(D, x, gap, row, fade) {
      ["prev", "play", "next"].forEach(function (k) { key(D, k, x, row, fade); x += CONTROL + gap; });
      return x - gap;
    }
    function key(D, k, x, row, fade) {
      var c = D.c, kind = k === "prev" ? "skipBack" : k === "next" ? "skipForward" : D.mo.playing ? "pause" : "play";
      var p = D.pointer, on = p && p[0] >= x && p[0] < x + CONTROL && p[1] >= row && p[1] < row + CONTROL;
      if (on) circle(c, hex(D.ink, 0.14 * fade), x + CONTROL / 2, row + CONTROL / 2, CONTROL / 2);
      var side = k === "play" ? TOGGLE_ICON : KEY_ICON, col = hex(D.ink, (on ? 1 : 0.85) * fade), cx = x + CONTROL / 2, cy = row + CONTROL / 2;
      if (D.edged) icon(c, kind, cx + 0.6, cy + 1, side, hex(0, 0.45 * col.a));
      icon(c, kind, cx, cy, side, col);
      if (S.controlsIn >= 0.5) D.hits.push([x, row, CONTROL, CONTROL, k]);
    }
    // The sound's key at the row's end, the track before it from `from`; with the pointer on it, the key moving left and
    // a slider of how loud the sound is shown to its right as it goes, the track giving way.
    function soundKey(D, from, home, row, fade) {
      var c = D.c, scale = D.scale, whole = function (v) { return Math.round(v * scale) / scale; };
      home = whole(home);
      var level = SOUND[0], muted = SOUND[1], p = D.pointer;
      var inside = function (x, y, w, h) { return p && p[0] >= x && p[0] < x + w && p[1] >= y && p[1] < y + h; };
      var room = SOUND_WIDE + 8, x = home - room * S.soundIn;
      var over = inside(x, row - 6, home + CONTROL - x, CONTROL + 12), onHome = inside(home, row, CONTROL, CONTROL);
      var wants = onHome || (S.soundOpen && over);
      S.soundLeft = wants ? null : S.soundLeft == null ? D.now : S.soundLeft;
      var lingering = S.soundOpen && S.soundLeft != null && D.now - S.soundLeft < SOUND_LINGER;
      S.soundOpen = fade > 0 && (wants || lingering);
      var to = S.soundOpen ? 1 : 0, fadeStep = Math.min(S.step, 1 / 30) / SOUND_OPEN;
      S.soundIn = to > S.soundIn ? Math.min(S.soundIn + fadeStep, to) : Math.max(S.soundIn - fadeStep, to);
      var opened = S.soundIn * S.soundIn * (3 - 2 * S.soundIn);
      x = whole(home - room * opened);
      progress(D, from, x - 4, row, fade, 1);
      var on = inside(x, row, CONTROL, CONTROL);
      if (on) circle(c, hex(D.ink, 0.14 * fade), x + CONTROL / 2, row + CONTROL / 2, CONTROL / 2);
      var col = hex(D.ink, (on || S.soundOpen ? 1 : 0.85) * fade), cx = x + CONTROL / 2, cy = row + CONTROL / 2;
      var kind = muted || level === 0 ? "muted" : "volume";
      if (D.edged) icon(c, kind, cx + 0.6, cy + 1, KEY_ICON, hex(0, 0.45 * col.a));
      icon(c, kind, cx, cy, KEY_ICON, col);
      if (S.controlsIn >= 0.5) D.hits.push([x, row, CONTROL, CONTROL, "mute"]);
      var slider = whole(home - room + CONTROL + 4);
      var litTo = opened >= 1 && inside(slider - 4, row, SOUND_WIDE + 8, CONTROL) ? 1 : 0;
      S.soundLit = easedToward(S.soundLit, litTo, S.step);
      S.easing = S.easing || lingering || S.soundIn !== to || S.soundLit !== litTo;
      if (opened <= 0) return;
      var fraction = muted ? 0 : level / 100, lit = S.soundLit, thick = 3 + 2 * lit, a = fade * opened;
      c.save(); c.beginPath(); c.rect(x + CONTROL, row, home + CONTROL - (x + CONTROL), CONTROL); c.clip();
      fillRounded(c, hex(D.ink, 0.25 * a), slider, cy - thick / 2, SOUND_WIDE, thick, thick / 2);
      fillRounded(c, colour(D.accent, 0.95 * a), slider, cy - thick / 2, SOUND_WIDE * fraction, thick, thick / 2);
      circle(c, hex(D.ink, a), slider + SOUND_WIDE * fraction, cy, 4.5 + 1.5 * lit);
      c.restore();
      if (S.controlsIn >= 0.5 && opened >= 1) D.hits.push([slider, row, SOUND_WIDE, CONTROL, "volume"]);
    }
    // What plays's track, from `from` to `to` on `row`, with how long is left at its end, `fade` of it shown. (The
    // song is played as it is told, not from a player: it cannot be moved along, so the track never lights.)
    function progress(D, from, to, row, fade, timeShown) {
      var length = song.length;
      if (!(length > 0)) return;
      var c = D.c, position = D.mo.position;
      var leftOver = "-" + clock(Math.round(Math.max(0, length - position)));
      var timeWide = measure(leftOver, FONTS.time) + 2;
      var track = Math.max(0, to - (timeWide + 10) * timeShown - from);
      if (track < 24) return;
      var middle = row + CONTROL / 2, fraction = Math.max(0, Math.min(1, position / length));
      S.trackLit = easedToward(S.trackLit, 0, S.step);
      S.easing = S.easing || S.trackLit !== 0;
      var shine = S.trackLit, thick = 3 + 2 * shine;
      fillRounded(c, hex(D.ink, 0.2 * fade), from, middle - thick / 2, track, thick, thick / 2);
      fillRounded(c, colour(D.accent, 0.95 * fade), from, middle - thick / 2, track * fraction, thick, thick / 2);
      if (shine > 0) circle(c, hex(D.ink, fade * shine), from + track * fraction, middle, 4 + 2 * shine);
      if (timeShown > 0) {
        var at = Math.round((to - timeWide) * D.scale) / D.scale;
        words(c, leftOver, FONTS.time, hex(D.ink, 0.6 * fade * timeShown), at, middle - FONTS.time.size * 0.7, timeWide, true, D.edged);
      }
    }

    // The strip: the cover small at its start, the line sung now, and at its end the spectrum or the controls. With no
    // ground and no cover, the line large in its middle and what is under it below.
    function strip(D) {
      var c = D.c, w = D.size[0], h = D.size[1], p = padOf("strip", D.shows), at = placedD(D, "strip");
      var row = (h - CONTROL) / 2, centred = D.edged && !D.shows.cover, shownIn = controlsShown();
      var ends = stripEnds(w, at.left, p, D.shows, centred);
      if (ends.spectrum) {
        var a = D.shows.controls ? 0.85 : 1 - shownIn;
        if (a > 0) spectrum(c, D.mo, D.accent, D.tint, ends.spectrum[0], h / 2 + 10, ends.spectrum[1], 20, a);
      }
      if (ends.keys != null) {
        var fade = D.shows.controls ? 1 : shownIn;
        if (fade > 0) keys(D, ends.keys, KEY_GAP, row, fade);
      }
      if (centred) {
        var side = D.shows.controls ? ends.side : ends.side * shownIn;
        linesCentred(D, p + side, w - p - side, h);
        return;
      }
      var left = at.left, width = Math.max(0, ends.end - left);
      clip(D, left - 4, 0, width + 8, h);
      var ls = lines, now = D.mo.now, F = FONTS;
      if (now != null) {
        var line = ls[now], next = ls[now + 1] ? ls[now + 1].at : null, pt = parts(line, next), text = pt.text;
        var eased = came(D), lc = lineColours(D, eased);
        if (!text.trim()) {
          interlude(D.c, left, h / 2 - F.strip.size * 0.7 + (1 - eased) * 8, F.strip.size, between(line, next, D.mo.position), lc[0], lc[1]);
        } else {
          // In one row, as large as it fits (down to the least); too long for that, in two rows where the strip is
          // tall enough, as large as they fit; else one row cut short with an ellipsis.
          var room = h - 2 * padOf("strip", D.shows);
          var fills = F.strip.size * width / Math.max(1, measure(text, F.strip));
          var font = sized(F.strip, Math.max(STRIP_LEAST, Math.min(F.strip.size, fills)));
          var oneRow = fills >= STRIP_LEAST, twoRows = null;
          if (!oneRow) {
            for (var size = F.strip.size; size >= STRIP_LEAST; size -= 1) {
              var f = sized(F.strip, size), tall0 = wrappedHeight(text, f, width);
              if (tall0 <= room && tall0 <= 2 * wrappedHeight("M", f, width) + 0.5) { twoRows = f; break; }
            }
          }
          var wrap, zoom, shown;
          if (twoRows) { font = twoRows; shown = text; wrap = width; zoom = 1; }
          else if (oneRow) { zoom = font.size / F.strip.size; font = F.strip; shown = text; wrap = 100000; }
          else { shown = fitted(text, font, width); wrap = 100000; zoom = 1; }
          var tall = wrappedHeight(shown, font, wrap) * zoom;
          var y = (h - tall) / 2 + (1 - eased) * 8;
          var L = laid(shown, font, wrap, true), sung = sungTo(pt.parts, D.mo.position);
          scaledAbout(D.c, left, y, zoom, function () { filled(D, D.c, L, shown, font, left, y, wrap, sung, lc[0], lc[1], 12 / zoom); });
        }
      } else {
        var first = ls.length ? ls[0].at : 0, done = first > 0 ? Math.max(0, Math.min(1, D.mo.position / first)) : 0, lc1 = lineColours(D, 1);
        interlude(D.c, left, h / 2 - F.strip.size * 0.7, F.strip.size, done, lc1[0], lc1[1]);
      }
      unclip(D);
    }

    // The strip down: the cover at its top, the line sung now under it written down (and what is under it beside it,
    // where two go across), the key to play or pause at its foot where there is room.
    function column(D) {
      var c = D.c, w = D.size[0], h = D.size[1], p = padOf("column", D.shows), at = placedD(D, "column"), F = FONTS;
      var fade = D.shows.controls ? 1 : controlsShown();
      var keyed = h - p - at.below >= CONTROL + 8 + 2 * STRIP_LEAST;
      var foot = keyed ? h - p - CONTROL : h - p;
      if (keyed && fade > 0) key(D, "play", (w - CONTROL) / 2, foot, fade);
      var room = Math.max(0, foot - 8 - at.below);
      if (room < STRIP_LEAST || at.width < STRIP_LEAST) return;
      var left = at.left, top = at.below, width = at.width, down = room > width * 1.2, ls = lines, now = D.mo.now;
      if (now != null) {
        var line = ls[now], next = ls[now + 1] ? ls[now + 1].at : null, pt = parts(line, next), text = pt.text;
        var eased = came(D), lc = lineColours(D, eased), und = under(D, ls, now);
        if (!text.trim()) {
          var isz = Math.min(F.strip.size, width / INTERLUDE_WIDE);
          interlude(c, left + (width - isz * INTERLUDE_WIDE) / 2, top + (1 - eased) * 8, isz, between(line, next, D.mo.position), lc[0], lc[1]);
          return;
        }
        var gap = 6, half = (width - gap) / 2, least = sized(F.strip, STRIP_LEAST);
        var two = down && und && unbroken(text, least, half, down) && unbroken(und, least, half, down);
        var col = two ? half : width;
        var font = sizedDown(text, F.strip, col, room, down), wrap = downWrap(text, font, col, down);
        var x = two ? left + width - col + (col - wrap) / 2 : left + (width - wrap) / 2;
        var y = top + (1 - eased) * 8, rowH = rowOf(text, font, wrap), tall = wrappedHeight(text, font, wrap);
        clip(D, left - 2, top, width + 4, Math.min(tall, Math.floor(room / rowH) * rowH) + 2);
        var L = laid(text, font, wrap, true);
        filled(D, D.c, L, text, font, x, y, wrap, sungTo(pt.parts, D.mo.position), lc[0], lc[1], 6);
        unclip(D);
        var small = { size: Math.max(font.size * 0.82, Math.min(STRIP_LEAST, font.size)), weight: 500 }, ucol = hex(D.ink, 0.6 * eased);
        if (two) {
          var uw = downWrap(und, small, col, down), ur = rowOf(und, small, uw);
          clip(D, left - 2, top, col + 4, Math.floor(room / ur) * ur + 2);
          wordsDown(D.c, und, small, ucol, left + (col - uw) / 2, y, uw, D.edged);
          unclip(D);
        } else if (!down && und) {
          var below = y + tall + 4, ur2 = rowOf(und, small, width), n = Math.floor((top + room - below) / ur2);
          if (n >= 1) {
            clip(D, left - 2, below, width + 4, n * ur2 + 2);
            wordsDown(D.c, und, small, ucol, left, below, width, D.edged);
            unclip(D);
          }
        }
      } else {
        var first = ls.length ? ls[0].at : 0, done = first > 0 ? Math.max(0, Math.min(1, D.mo.position / first)) : 0, lc1 = lineColours(D, 1);
        var sz = Math.min(F.strip.size, width / INTERLUDE_WIDE);
        interlude(c, left + (width - sz * INTERLUDE_WIDE) / 2, top, sz, done, lc1[0], lc1[1]);
      }
    }

    // The line sung now large in the middle between `from` and `to` (floating lyrics), what is under it below where
    // there is room: glowing in the cover's colour, a dark haze round it to be read on anything.
    function linesCentred(D, from, to, h) {
      var c = D.c, room = Math.max(0, to - from), F = FONTS;
      var size = Math.max(STRIP_LEAST, Math.min(F.float.size, h * 0.34)), font = sized(F.float, size);
      var underFont = { size: Math.max(size * FLOAT_UNDER, 11), weight: 600 };
      var withUnder = h >= size * 1.4 + size * FLOAT_UNDER * 1.4 + 16;
      var top = withUnder ? (h - size * 1.4 - underFont.size * 1.4) / 2 : (h - size * 1.4) / 2;
      var now = D.mo.now;
      if (now == null) return;
      var line = lines[now], next = lines[now + 1] ? lines[now + 1].at : null, pt = parts(line, next), text = pt.text;
      var eased = came(D), y = top + (1 - eased) * 14;
      var dim = hex(0xFFFFFF, 0.6 * eased), bright = colour(D.tint, eased);
      if (!text.trim()) {
        var x0 = from + (room - font.size * INTERLUDE_WIDE) / 2, done = between(line, next, D.mo.position), box = [x0, y, font.size * INTERLUDE_WIDE, font.size * 1.4];
        glow(c, hex(0, 0.55 * eased), 3, box, function (o) { interlude(o, x0, y + 1, font.size, 0, hex(0xFFFFFF, 1), hex(0xFFFFFF, 1)); });
        glow(c, colour(D.accent, 0.75 * eased), 10, box, function (o) { interlude(o, x0, y, font.size, done, hex(0, 0), hex(0xFFFFFF, 1)); });
        interlude(c, x0, y, font.size, done, dim, bright);
      } else {
        var wideT = Math.min(measure(text, font), room), x = from + (room - wideT) / 2, width = wideT + 2;
        var L = laid(text, font, width, true), sung = sungTo(pt.parts, D.mo.position), box2 = [x - 4, y, width + 8, L.height + 1];
        glow(c, hex(0, 0.55 * eased), 3, box2, function (o) { textWrapped(o, text, font, hex(0xFFFFFF, 1), x, y + 1, width); });
        glow(c, colour(D.accent, 0.75 * eased), 10, box2, function (o) { sungPart(o, L, text, font, x, y, width, sung, hex(0xFFFFFF, 1), 14); });
        filledRaw(c, L, text, font, x, y, width, sung, dim, bright, 14);
      }
      if (withUnder) {
        var und = under(D, lines, now);
        if (und) {
          var uw = Math.min(measure(und, underFont), room);
          outlined(c, fitted(und, underFont, room), underFont, hex(0xFFFFFF, 0.78 * eased), from + (room - uw) / 2, y + size * 1.45, uw + 2);
        }
      }
    }

    // The player: the cover at the start, what plays beside it, the line sung now (rising in as it comes, filled as it
    // is sung, wrapped where it is too long) and what is under it, the rows along the bottom.
    function player(D, showLines) {
      var c = D.c, h = D.size[1], p = padOf("player", D.shows), at = placedD(D, "player"), left = at.left, width = at.width, F = FONTS;
      var free = rows(D, p, h - p, D.size[0] - 2 * p, at.cover ? [at.cover[0] + at.cover[2], at.left] : null);
      var lineTop = at.below, bottom = free - 8, underTall = F.under.size * 1.35;
      if (bottom <= lineTop) return;
      if (showLines) {
        var middle = at.cover ? at.cover[1] + at.cover[2] / 2 : (lineTop + bottom) / 2, row = F.line.size * 1.6;
        var anchor = Math.max(lineTop + row * 1.2, Math.min(Math.max(bottom - row * 1.2, lineTop + row * 1.2), middle));
        railOfLines(D, left, lineTop, bottom, width, anchor, F.line.size);
        return;
      }
      clip(D, left - 4, lineTop - 4, width + 8, bottom - lineTop + 8);
      var ls = lines, now = D.mo.now;
      if (now != null) {
        var line = ls[now], next = ls[now + 1] ? ls[now + 1].at : null, pt = parts(line, next), text = pt.text;
        var eased = came(D), und = under(D, ls, now), room = bottom - lineTop;
        // A line's size: wrapped, at the largest size (down to that of what is under it) at which all its rows fit
        // with what is under it; else all its rows alone; else one row at the least, cut short.
        var fit = function (t, withUnder) {
          var wantsUnder = withUnder ? 2 + underTall : 0, size, f;
          for (size = F.line.size; size >= F.under.size; size -= 1) { f = sized(F.line, size); if (wrappedHeight(t, f, width) + wantsUnder <= room) return [f, wrappedHeight(t, f, width), false]; }
          for (size = F.line.size; size >= F.under.size; size -= 1) { f = sized(F.line, size); if (wrappedHeight(t, f, width) <= room) return [f, wrappedHeight(t, f, width), false]; }
          f = sized(F.line, F.under.size);
          return [f, laid("M", f, 100000, true).height, true];
        };
        var shownOf = function (t, f, cut) { return cut ? fitted(t, f, width) : t; };
        var before = null;
        if (now > 0 && eased < 1 && text.trim() && ls[now - 1].text.trim() && under(D, ls, now - 1) === line.text) before = now - 1;
        var lineY, tall;
        if (before != null) {
          var o = fit(ls[before].text, true), sc = lineColours(D, 1)[1];
          textWrapped(c, shownOf(ls[before].text, o[0], o[2]), o[0], alpha(sc, 1 - eased), left, lineTop - eased * o[1] * 0.7, width);
          var nf = fit(text, !!und), shown = shownOf(text, nf[0], nf[2]);
          var fromY = lineTop + o[1] + 2, y = fromY + (lineTop - fromY) * eased, size2 = F.under.size + (nf[0].size - F.under.size) * eased;
          words(c, shown, sized(F.under, size2), hex(D.ink, 0.5 * (1 - eased)), left, y, width + 2, false, D.edged);
          var grown = sized(nf[0], size2), L = laid(shown, grown, width, true), lc = lineColours(D, eased);
          filled(D, c, L, shown, grown, left, y, width, sungTo(pt.parts, D.mo.position), lc[0], lc[1], 12);
          lineY = y; tall = Math.max(L.height, nf[1] * eased);
        } else {
          var step = 18;
          if (now > 0 && eased < 1) {
            var was = ls[now - 1];
            if (was.text.trim()) {
              var of = fit(was.text, true), wasUnder = under(D, ls, now - 1);
              var withU = !!wasUnder && lineTop + of[1] + 2 + underTall <= bottom;
              step = of[1] + 4 + (withU ? underTall : 0);
              var up = lineTop - eased * step, going = Math.max(0, 1 - eased * 1.8), sc2 = lineColours(D, 1)[1];
              textWrapped(c, shownOf(was.text, of[0], of[2]), of[0], alpha(sc2, going), left, up, width);
              if (withU) words(c, fitted(wasUnder, F.under, width), F.under, hex(D.ink, 0.5 * going), left, up + of[1] + 2, width + 2, false, D.edged);
            }
          }
          var y2 = lineTop + (1 - eased) * step, lc2 = lineColours(D, eased);
          if (!text.trim()) {
            interlude(c, left, y2, F.line.size, between(line, next, D.mo.position), lc2[0], lc2[1]);
            lineY = y2; tall = F.line.size * 1.4;
          } else {
            var nf2 = fit(text, !!und);
            if (nf2[1] <= room) {
              var shown2 = shownOf(text, nf2[0], nf2[2]), L2 = laid(shown2, nf2[0], width, true);
              filled(D, c, L2, shown2, nf2[0], left, y2, width, sungTo(pt.parts, D.mo.position), lc2[0], lc2[1], 12);
            }
            lineY = y2; tall = nf2[1];
          }
        }
        if (und && lineTop + tall + 2 + underTall <= bottom) words(c, fitted(und, F.under, width), F.under, hex(D.ink, 0.5 * eased), left, lineY + tall + 2, width + 2, false, D.edged);
      } else {
        var first = ls.length ? ls[0].at : 0, done = first > 0 ? Math.max(0, Math.min(1, D.mo.position / first)) : 0, lc1 = lineColours(D, 1);
        interlude(c, left, lineTop, F.line.size, done, lc1[0], lc1[1]);
      }
      unclip(D);
    }

    // The stage: the record at the start, what plays and the lines round the one sung now beside it, and along the
    // bottom the spectrum over the controls and the track.
    function stage(D) {
      var w = D.size[0], h = D.size[1], p = padOf("stage", D.shows), at = placedD(D, "stage"), cy = stageRecord(D.size, D.shows)[0][1];
      var foot = h - STAGE_FOOT, row = foot - CONTROL, floor;
      if (D.shows.controls) {
        bottomRow(D, p, w - p, row, 1, null);
        if (D.shows.spectrum) spectrum(D.c, D.mo, D.accent, D.tint, p, row - ROW_GAP, w - 2 * p, STAGE_SPECTRUM, 0.85);
        floor = row - ROW_GAP - (D.shows.spectrum ? STAGE_SPECTRUM + 10 : 0);
      } else {
        var shownIn = controlsShown();
        if (shownIn < 1 && D.shows.spectrum) spectrum(D.c, D.mo, D.accent, D.tint, p, foot - 2, w - 2 * p, STAGE_SPECTRUM, 0.85 * (1 - shownIn));
        if (shownIn > 0) bottomRow(D, p, w - p, row, shownIn, null);
        floor = foot - (D.shows.spectrum ? STAGE_SPECTRUM : CONTROL) - 10;
      }
      if (floor <= at.below + 20) return;
      railOfLines(D, at.left, at.below, floor, at.width, cy, stageLine(h));
    }

    // The card (and the card stood on end): what plays at the top, the lines round the one sung now gliding past it,
    // the rows along the bottom.
    function card(D, layout) {
      var h = D.size[1], p = padOf(layout, D.shows), at = placedD(D, layout), x = at.left, width = at.width;
      var free = rows(D, x, h - p, width, null), top = at.below, bottom = free - 10;
      if (bottom <= top + 20) return;
      railOfLines(D, x, top, bottom, width, null, Math.max(18, Math.min(FONTS.now.size, width / 16)));
    }

    // The lines before and after the one sung now, smaller and fainter the farther they are from it, the rail gliding;
    // the one sung now filled as it is sung, its translation under it. `size`: the line sung now's.
    function railOfLines(D, x, top, bottom, width, anchor, size) {
      var c = D.c, ls = lines;
      if (anchor == null) anchor = top + (bottom - top) * (bottom - top < 220 ? 0.5 : 0.38);
      var fadeTop = Math.min((bottom - top) * 0.12, 28), fadeBottom = Math.min((bottom - top) * 0.15, 34);
      var current = D.mo.now, translated = D.shows.translation;
      var scaleAt = function (d) { d = Math.abs(d); return d <= 1 ? 1 - 0.32 * d : Math.max(0.5, Math.min(0.68, 0.68 * Math.pow(0.9, d - 1))); };
      var opacityAt = function (d) {
        if (d >= 0) return d <= 1 ? 1 - 0.28 * d : Math.max(0.3, 0.72 - 0.18 * (d - 1));
        d = -d;
        return d <= 1 ? 1 - 0.48 * d : Math.max(0.22, 0.52 - 0.12 * (d - 1));
      };
      var gap = 10, full = FONTS.now, zoom = function (s) { return s * size / full.size; };
      var atRest = width / zoom(scaleAt(0));
      var heightOf = function (i, s) {
        var l = ls[i];
        return wrappedHeight(l.text.trim() ? l.text : "M", full, atRest) * zoom(s) + (translated && l.translation != null ? TRANSLATION_ROW * s : 0);
      };
      var rail = S.rail, base = Math.max(-1, Math.floor(rail));
      var first = Math.max(0, base - 4), last = Math.min(ls.length, Math.max(0, base + 7));
      if (first >= last) return;
      var middles = [], y = 0;
      for (var i = first; i < last; i++) {
        var s = scaleAt(i - rail), tall = heightOf(i, s);
        middles.push([i, y + tall / 2, s, tall]);
        y += tall + gap;
      }
      var below = (middles.find(function (m) { return m[0] >= rail; }) || middles[middles.length - 1])[1];
      var aboveM = null;
      for (var k = middles.length - 1; k >= 0; k--) if (middles[k][0] <= rail) { aboveM = middles[k]; break; }
      var above = aboveM ? aboveM[1] : below, frac = rail - Math.floor(rail);
      var atRail = rail < first ? below : above + (below - above) * frac;
      // Where it is asked for, as far as the lines next to the one aimed at stay within it: one before, one after.
      var aimed = current != null ? current : Math.round(rail);
      var tallAt = function (index, d) { return index >= 0 && index < ls.length ? heightOf(index, scaleAt(d)) : null; };
      var aimedHalf = (tallAt(aimed, 0) || 0) / 2;
      var tb = tallAt(aimed - 1, -1), ta = tallAt(aimed + 1, 1);
      var least = tb == null ? -Infinity : top + fadeTop + aimedHalf + gap + tb;
      var most = ta == null ? Infinity : bottom + 12 - fadeBottom - aimedHalf - gap - ta;
      anchor = least <= most ? Math.max(least, Math.min(most, anchor)) : (least + most) / 2;
      // Moving there as the rail glides (from where it was; a rail come anew, there at once).
      var to = anchor - top, aim = current != null ? current : -1, was = S.anchor && Math.abs(S.anchor.shown - to) < bottom - top ? S.anchor : null, state;
      if (was && was.aimed === aim && Math.abs(was.to - to) <= 0.5) state = was;
      else if (was) state = { from: was.shown, fromRail: rail, aimed: aim, to: to, shown: was.shown };
      else state = { from: to, fromRail: rail, aimed: aim, to: to, shown: to };
      var span = aim - state.fromRail, along = Math.abs(span) > 1e-3 ? Math.max(0, Math.min(1, (rail - state.fromRail) / span)) : 1;
      state.shown = state.from + (state.to - state.from) * along;
      S.anchoring = along < 1 && Math.abs(state.shown - to) > 0.3;
      S.anchor = state;
      anchor = top + state.shown;
      var tint = D.tint, clear = D.edged;
      clip(D, x - 8, top, width + 16, bottom - top + 12);
      middles.forEach(function (m) {
        var index = m[0], d = index - rail, s = m[2], tall = m[3], yy = anchor + m[1] - atRail - tall / 2;
        if (yy + tall < top - 4 || yy > bottom + 12) return;
        var edgeA = Math.min(Math.max(0, Math.min(1, (yy - top) / fadeTop)), Math.max(0, Math.min(1, (bottom + 12 - (yy + tall)) / fadeBottom)));
        var a = opacityAt(d) * edgeA, line = ls[index], font = sized(FONTS.now, size * s);
        if (index === current) {
          var next = ls[index + 1] ? ls[index + 1].at : null, pt = parts(line, next), text = pt.text;
          var bright = colour(toward([1, 1, 1], tint, 0.35), a), dim = hex(D.ink, (clear ? 0.55 : 0.34) * a);
          if (!text.trim()) {
            var done = between(line, next, D.mo.position);
            glow(c, colour(tint, 0.5 * a), 6, [x, yy, font.size * INTERLUDE_WIDE, font.size * 1.4], function (o) { interlude(o, x, yy, font.size, done, hex(0, 0), bright); });
            interlude(c, x, yy, font.size, done, dim, bright);
          } else {
            var L = laid(text, full, atRest, true), sung = sungTo(pt.parts, D.mo.position);
            var soft = Math.max(6, Math.min(16, font.size * 0.45)) / zoom(s);
            scaledAbout(c, x, yy, zoom(s), function () {
              glow(c, colour(tint, 0.5 * a), 6, [x - 4, yy, atRest + 8, L.height], function (o) { sungPart(o, L, text, full, x, yy, atRest, sung, bright, soft); });
              filled(D, c, L, text, full, x, yy, atRest, sung, dim, bright, soft);
            });
          }
          if (translated && line.translation != null) {
            var underY = !text.trim() ? font.size * 1.4 : wrappedHeight(text, full, atRest) * zoom(s);
            var small = sized(FONTS.under_font, FONTS.under_font.size * s);
            words(c, fitted(line.translation, small, width), small, hex(D.ink, 0.62 * a), x, yy + underY + 2 * s, width, false, clear);
          }
        } else {
          var shown = hex(D.ink, (clear ? 0.7 : 0.5) * a);
          if (!line.text.trim()) interlude(c, x, yy, font.size, 0, shown, shown);
          else scaledAbout(c, x, yy, zoom(s), function () { if (clear) wordsWrappedEdged(c, line.text, full, shown, x, yy, atRest); else textWrapped(c, line.text, full, shown, x, yy, atRest); });
          if (translated && line.translation != null && line.text.trim()) {
            var uy = wrappedHeight(line.text, full, atRest) * zoom(s), sm = sized(FONTS.under_font, FONTS.under_font.size * s);
            words(c, fitted(line.translation, sm, width), sm, hex(D.ink, 0.42 * a), x, yy + uy + 2 * s, width, false, clear);
          }
        }
      });
      unclip(D);
    }
    // The lines next to the one sung now with no ground: as `outlined` draws them, a row from the corner, `width` wide.
    function wordsWrappedEdged(c, text, font, color, x, y, width) {
      words(c, text, font, color, x, y, width, false, true);
    }

    function content(D, layout, showLines) {
      if (layout === "strip") strip(D);
      else if (layout === "player") player(D, showLines);
      else if (layout === "card" || layout === "tall") card(D, layout);
      else if (layout === "stage") stage(D);
      else column(D);
    }

    // ---------------- a frame ----------------
    function paintLaid(c, size, shws, pointer, now, mo, zoom) {
      var step = S.lastFrame == null ? 0 : Math.max(0, Math.min(0.1, (now - S.lastFrame) / 1000));
      S.lastFrame = now; S.step = step; S.easing = false;
      // The rail, sprung toward the line sung now (quick, then settling).
      var target = mo.now != null ? mo.now : -1;
      if (S.fresh) { S.fresh = false; S.rail = target; S.railSpeed = 0; }
      for (var left = step; left > 0; left -= 1 / 240) {
        var dt = Math.min(left, 1 / 240), force = -142 * (S.rail - target) - 28 * S.railSpeed;
        S.railSpeed += force / 0.82 * dt;
        S.rail += S.railSpeed * dt;
      }
      var settling = Math.abs(S.rail - target) > 0.002 || Math.abs(S.railSpeed) > 0.002;
      if (!settling) { S.rail = target; S.railSpeed = 0; }
      // The controls, in toward the pointer over it and out after it (kept showing, always in).
      var controlsTo = pointer || shws.controls ? 1 : 0, fadeStep = Math.min(step, 1 / 30);
      S.controlsIn = controlsTo > S.controlsIn ? Math.min(S.controlsIn + fadeStep / CONTROLS_FADE, controlsTo) : Math.max(S.controlsIn - fadeStep / CONTROLS_FADE, controlsTo);
      // The spectrum, falling where the sound has fallen.
      for (var b = 0; b < BANDS; b++) S.bands[b] = mo.heard[b] >= S.bands[b] ? mo.heard[b] : Math.max(mo.heard[b], S.bands[b] - step * SPECTRUM_FALL);
      mo.bands = S.bands;
      if (mo.playing) S.turnedBy = (S.turnedBy + step * 360 / RECORD_TURN) % 360;
      var armTo = mo.playing ? 1 : 0;
      S.arm = armTo > S.arm ? Math.min(S.arm + step / ARM_SWING, armTo) : Math.max(S.arm - step / ARM_SWING, armTo);
      var layout = forRoom(size, shws, S.layout);
      S.layout = layout;
      // Laid out anew: its parts move from where they were (from where they had got to if they were moving still).
      var lns = layout === "player" && playerShowsLines(size, shws), drawn = [size[0] * zoom, size[1] * zoom];
      if (S.shown && !(S.shown[0] === layout && sameShows(S.shown[1], shws) && S.shown[2] === lns)) {
        var wasZoom = zoomAt(S.shown[0], drawn, S.shown[1]);
        var wasAt = scaledPlaced(placed(S.shown[0], [drawn[0] / wasZoom, drawn[1] / wasZoom], S.shown[1]), wasZoom);
        var t0 = S.turning ? turnedT(now, S.turning.since) : null;
        S.turning = { since: now, from: t0 != null ? tween(S.turning.from, wasAt, t0) : wasAt, was: [S.shown[0], S.shown[2]] };
      }
      S.shown = [layout, copyShows(shws), lns];
      var tt = S.turning ? turnedT(now, S.turning.since) : null;
      var turn = tt != null ? { from: scaledPlaced(S.turning.from, 1 / zoom), t: tt } : null;
      var was = S.turning ? S.turning.was : null;
      if (!turn) S.turning = null;
      var D = { c: c, now: now, mo: mo, shows: shws, ink: 0xFFFFFF, edged: !!shws.clear, tint: S.tint, accent: S.accent, scale: stretch(c.getTransform()), pointer: pointer, size: size, turn: turn, hits: [], clips: [] };
      ground(D, layout);
      disc(D, layout);
      // Turning from another layout: what that one showed off its ground fades out as this one's comes in.
      if (turn && was && !(was[0] === layout && was[1] === lns)) {
        var kept = { anchor: S.anchor, anchoring: S.anchoring, easing: S.easing, sound: [S.soundOpen, S.soundIn, S.soundLit, S.soundLeft], trackLit: S.trackLit };
        var hitsN = D.hits.length, saved = D.turn;
        D.turn = null;
        layer(c, 1 - turn.t, function (o) { D.c = o; content(D, was[0], was[1]); D.c = c; });
        D.turn = saved;
        D.hits.length = hitsN;
        S.anchor = kept.anchor; S.anchoring = kept.anchoring; S.easing = kept.easing; S.trackLit = kept.trackLit;
        S.soundOpen = kept.sound[0]; S.soundIn = kept.sound[1]; S.soundLit = kept.sound[2]; S.soundLeft = kept.sound[3];
      }
      if (turn) layer(c, turn.t, function (o) { D.c = o; content(D, layout, lns); D.c = c; });
      else content(D, layout, lns);
      return D.hits;
    }

    var m = {
      shows: shows,
      least: LEAST.slice(),
      margin: MARGIN,
      reach: REACH,
      row: ROW,
      layoutFor: function (w, h) { return forRoom([w, h], m.shows, S.layout); },
      restsAt: function (w, h) { return restsAt([w, h], m.shows); },
      // `rack`: as a widget, the side its buttons' row is on ("above", "under", "left" or "right"); in a panel, none.
      draw: function (c, w, h, t, now, pointer, playing, rack) {
        var shws = copyShows(m.shows), size = [w, h];
        var at = sungAt(t), since = at != null ? Math.max(0, t - lines[at].at) : null;
        var mo = { now: at, since: since, position: t, playing: !!playing, heard: playing ? heardAt(t) : new Array(BANDS).fill(0) };
        var p = pointer && pointer.x >= 0 && pointer.y >= 0 && pointer.x < w && pointer.y < h ? [pointer.x, pointer.y] : null;
        c.save();
        // (Within its box and `MARGIN` round it, as Glance keeps it in a layer of that room: its shadow reaches there.)
        c.beginPath(); c.rect(-MARGIN, -MARGIN, w + 2 * MARGIN, h + 2 * MARGIN); c.clip();
        // A player less tall than the least it is: that least, smaller as a whole.
        var zoom = zoomAt(forRoom(size, shws, S.layout), size, shws), hits;
        if (zoom >= 1) hits = paintLaid(c, size, shws, p, now, mo, 1);
        else {
          c.scale(zoom, zoom);
          hits = paintLaid(c, [w / zoom, h / zoom], shws, p && [p[0] / zoom, p[1] / zoom], now, mo, zoom);
          hits = hits.map(function (b) { return [b[0] * zoom, b[1] * zoom, b[2] * zoom, b[3] * zoom, b[4]]; });
        }
        c.restore();
        var buttons = [];
        var g = groundAt(w, h), plate = function () {
          var lighter = function (v) { return v + (1 - v) * 0.12; };
          return Color(lighter(g[0]), lighter(g[1]), lighter(g[2]), 0.94);
        };
        // A widget's row, off its edge, the pointer over it or its row: lit only while on, as none of these is here.
        if (rack && pointer) {
          var r = BUTTON / 2, step = BUTTON + BUTTON_GAP;
          RACK.forEach(function (name, place) {
            var cx, cy;
            if (rack === "above" || rack === "under") {
              cx = w - r - (RACK.length - 1 - place) * step;
              cy = rack === "under" ? h + BUTTON_INSET + r : -BUTTON_INSET - r;
            } else {
              cx = rack === "right" ? w + BUTTON_INSET + r : -BUTTON_INSET - r;
              cy = r + place * step;
            }
            circle(c, plate(), cx, cy, r);
            ring(c, hex(0xFFFFFF, 0.22), cx, cy, (BUTTON - 1) / 2, 1);
            icon(c, { full: "maximize", through: "pointer" }[name] || name, cx, cy, 13, hex(0xFFFFFF, 1));
            buttons.push([cx - r, cy - r, BUTTON, BUTTON, name]);
          });
        }
        // In a panel: its pin, its settings and the full screen, at its top right while the pointer is over it.
        if (p && !rack) {
          var top = CORNER, right = w - CORNER;
          [["pin", "pin"], ["settings", "settings"], ["maximize", "full"]].forEach(function (one, index) {
            var bx = right - BUTTON - index * (BUTTON + BUTTON_GAP), by = top;
            var lit = p[0] >= bx && p[0] < bx + BUTTON && p[1] >= by && p[1] < by + BUTTON;
            var ground = plate(), mark = hex(0xFFFFFF, 1);
            var cx = bx + BUTTON / 2, cy = by + BUTTON / 2;
            circle(c, lit ? mark : ground, cx, cy, BUTTON / 2);
            ring(c, hex(0xFFFFFF, 0.22), cx, cy, (BUTTON - 1) / 2, 1);
            icon(c, one[0], cx, cy, 13, lit ? ground : mark);
            buttons.push([bx, by, BUTTON, BUTTON, one[1]]);
          });
        }
        S.hits = buttons.concat(hits);
        S.drawnAt = [w, h];
      },
      // Where a control is at `w` × `h` showing `shows` (DIPs, from its corner), however shown or not just now: the full
      // screen's button (in its corner, or with `rack` in a widget's row), or the key to play or pause, go back or
      // skip, as that layout puts them; null where it has none.
      place: function (w, h, what, rack) {
        var shws = copyShows(m.shows);
        if (what === "full" && rack) {
          var at = 0, step = BUTTON + BUTTON_GAP;
          var across = rack === "above" || rack === "under";
          return {
            x: across ? w - BUTTON - (RACK.length - 1 - at) * step : rack === "right" ? w + BUTTON_INSET : -BUTTON_INSET - BUTTON,
            y: across ? (rack === "under" ? h + BUTTON_INSET : -BUTTON_INSET - BUTTON) : at * step,
            w: BUTTON, h: BUTTON
          };
        }
        if (what === "full") return { x: w - CORNER - BUTTON - 2 * (BUTTON + BUTTON_GAP), y: CORNER, w: BUTTON, h: BUTTON };
        var order = ["prev", "play", "next"].indexOf(what);
        if (order < 0) return null;
        var layout = forRoom([w, h], shws, S.layout), zoom = zoomAt(layout, [w, h], shws), size = [w / zoom, h / zoom];
        var p = padOf(layout, shws), at = placed(layout, size, shws), x, row, gap = KEY_GAP;
        switch (layout) {
          case "strip": {
            var ends = stripEnds(size[0], at.left, p, shws, shws.clear && !shws.cover);
            if (ends.keys == null) return null;
            x = ends.keys; row = (size[1] - CONTROL) / 2;
            break;
          }
          case "column": {
            if (what !== "play" || size[1] - p - at.below < CONTROL + 8 + 2 * STRIP_LEAST) return null;
            return { x: (size[0] - CONTROL) / 2, y: size[1] - p - CONTROL, w: CONTROL, h: CONTROL };
          }
          case "player":
            x = p - KEY_LEAD; row = size[1] - p - CONTROL;
            if (at.cover) gap = Math.max(KEY_GAP, (at.cover[0] + at.cover[2] - p + 2 * KEY_LEAD - 3 * CONTROL) / 2);
            break;
          case "stage":
            x = p - KEY_LEAD; row = size[1] - STAGE_FOOT - CONTROL;
            break;
          default:
            x = at.left - KEY_LEAD; row = size[1] - p - CONTROL;
        }
        return { x: (x + order * (CONTROL + gap)) * zoom, y: row * zoom, w: CONTROL * zoom, h: CONTROL * zoom };
      },
      hit: function (w, h, x, y) {
        // (Where a press does something as it was last drawn, at that size.)
        if (!S.drawnAt || S.drawnAt[0] !== w || S.drawnAt[1] !== h) return null;
        for (var i = 0; i < S.hits.length; i++) {
          var b = S.hits[i];
          if (x >= b[0] && x < b[0] + b[2] && y >= b[1] && y < b[1] + b[3]) return ["full", "play", "prev", "next"].indexOf(b[4]) >= 0 ? b[4] : null;
        }
        return null;
      }
    };
    return m;
  }

  GD.music = makeMusic;
})();
