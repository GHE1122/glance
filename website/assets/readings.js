// The monitor torn off the desk's panel as a widget, drawn in a canvas as
// Glance draws it: the panel's lanes as they were, then, as the hand sizes
// it, whatever of them fits the room (arrange.rs `Ladder`), from all of each
// lane in columns down to a few rings (forms.rs), each form turning into the
// next, on the ground each look gives it (skins.rs). Its readings are the
// panel's own (demo.js), running on.
(function () {
  "use strict";

  var GD = window.GlanceDesk = window.GlanceDesk || {};
  var root = document.documentElement;
  var TAU = Math.PI * 2;

  // ---------------- measures (view.rs, forms.rs, arrange.rs, widget.rs) ----------------
  // A lane: its column's width, the label column and the gap after it, the head and the room under it, a chart's
  // height (a rate chart's), how much further right rate charts start, a line, the room between facts, a table's row.
  var COLUMN_WIDTH = 356, LABEL = 96, LABEL_GAP = 12, HEAD = 17, HEAD_GAP = 8, PLOT = 40, RATE_PLOT = 36, RATE_INDENT = 26;
  var LINE = 16, FACT_GAP = 3, TABLE_ROW = 22, COMPACT_PROCESSES = 3;
  // The processes' measures, each column's width (view.rs `PROCESS_COLUMNS`: a share, a size, a share).
  var PROCESS_WIDTHS = [40, 52, 36];
  // The small layouts: a tile, the room round the tiles, a strip's line, a tall corner's rows, a rail's width and row,
  // how wide a rail sets what goes with a figure beside it, the widest name in a legend (forms.rs).
  var TILE = [156, 112], TILES_PAD = 12, STRIP = 44, TALL_CHIPS = 40, TALL_PAIR = 34, RAIL = 96, RAIL_ROW = 36, RAIL_WIDE = 150;
  var LEGEND_NAME = 120;
  // Columns a form spreads over at most, the most it is scaled up, the least its lanes are scaled down to, how much
  // more of the room another form of its level must cover to be taken, and how much more than its least a richer
  // level must fit at to come back (arrange.rs).
  var MAX_COLUMNS = 3, MOST_SCALE = 1.3, LANES_FLOOR = 0.8, STICK = 1.25, UP = 1.15;
  // Closest a form is laid to the ends of the screen it is on (arrange.rs `GAP`), and that screen's work area: the
  // desk's 1440 × 900, its taskbar off it (desk.js).
  var GAP = 12, WORK = [1440, 852];
  // Sized by hand, how far past what its form fills it follows the hand; how long a form takes to turn into the next
  // (widget.rs `GIVE`, `MORPH`).
  var GIVE = 0.5, MORPH = 260;
  // Its buttons (buttons.rs `ALL`, `SIZE`, `GAP`; widget.rs `BUTTON_INSET`), in a row off its edge.
  var RACK = ["game", "settings", "pin", "through", "close"];
  var BUTTON = 22, BUTTON_GAP = 4, BUTTON_INSET = 6;
  var REACH = BUTTON + 2 * BUTTON_INSET, ROW = RACK.length * BUTTON + (RACK.length - 1) * BUTTON_GAP;
  // A load past which it runs hot, and a temperature (prefs.rs `HOT_LOAD`, `hot_temp`).
  var HOT_LOAD = 85, HOT_TEMP = 85;
  // How frosted glass is over the desk's wallpaper, as the panel there is (site.css `--gp-frost`).
  var FROST = 0.16;

  // ---------------- icons (Lucide, ISC: as src/ui/icons.rs has them) ----------------
  var ICONS = {
    cpu: ["M12 20v2", "M12 2v2", "M17 20v2", "M17 2v2", "M2 12h2", "M2 17h2", "M2 7h2", "M20 12h2", "M20 17h2", "M20 7h2", "M7 20v2", "M7 2v2",
      "M6 4h12a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z", "M9 8h6a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H9a1 1 0 0 1-1-1V9a1 1 0 0 1 1-1z"],
    gpu: ["M2 17h18a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2H2", "M2 21V3", "M7 17v3a1 1 0 0 0 1 1h5a1 1 0 0 0 1-1v-3", "M18 11a2 2 0 1 1-4 0a2 2 0 1 1 4 0z", "M10 11a2 2 0 1 1-4 0a2 2 0 1 1 4 0z"],
    memory: ["M12 12v-2", "M12 18v-2", "M16 12v-2", "M16 18v-2", "M2 11h1.5", "M20 18v-2", "M20.5 11H22", "M4 18v-2", "M8 12v-2", "M8 18v-2", "M4 6h16a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2z"],
    network: ["m3 16 4 4 4-4", "M7 20V4", "m21 8-4-4-4 4", "M17 4v16"],
    game: ["M6 11h4", "M8 9v4", "M15 12h.01", "M18 10h.01", "M17.32 5H6.68a4 4 0 0 0-3.978 3.59c-.006.052-.01.101-.017.152C2.604 9.416 2 14.456 2 16a3 3 0 0 0 3 3c1 0 1.5-.5 2-1l1.414-1.414A2 2 0 0 1 9.828 16h4.344a2 2 0 0 1 1.414.586L17 18c.5.5 1 1 2 1a3 3 0 0 0 3-3c0-1.545-.604-6.584-.685-7.258-.007-.05-.011-.1-.017-.151A4 4 0 0 0 17.32 5z"],
    settings: ["M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915", "M15 12a3 3 0 1 1-6 0a3 3 0 1 1 6 0z"],
    pin: ["M12 17v5", "M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z"],
    through: ["M4.037 4.688a.495.495 0 0 1 .651-.651l16 6.5a.5.5 0 0 1-.063.947l-6.124 1.58a2 2 0 0 0-1.438 1.435l-1.579 6.126a.5.5 0 0 1-.947.063z"],
    close: ["M18 6 6 18", "m6 6 12 12"]
  };
  var PATHS = {};
  Object.keys(ICONS).forEach(function (k) { PATHS[k] = ICONS[k].map(function (d) { return new Path2D(d); }); });

  // ---------------- the looks, dark (theme.rs) ----------------
  function Color(rgb, a) { return { rgb: rgb, a: a }; }
  function alpha(c, a) { return Color(c.rgb, c.a * a); }
  function css(c) { return "rgba(" + (c.rgb >> 16 & 255) + "," + (c.rgb >> 8 & 255) + "," + (c.rgb & 255) + "," + c.a + ")"; }
  // One colour at several strengths (theme.rs `ink`).
  function ink(rgb, second, wash, washEnd) { var t = Color(rgb, 1); return { trace: t, trace2: alpha(t, second), wash: alpha(t, wash), washEnd: alpha(t, washEnd) }; }
  var CJK = '"Microsoft YaHei UI", "Microsoft YaHei", "PingFang SC", "Hiragino Sans GB", "Noto Sans CJK SC", sans-serif';
  // A face: its family, size and weight, how wide (a share of its normal width) and how far apart its letters (em).
  function Face(family, size, weight, width, tracking) { return { family: family, size: size, weight: weight, width: width || 100, tracking: tracking || 0 }; }
  function sized(f, size, weight) { return Face(f.family, size, weight, f.width, f.tracking); }

  function paper() {
    var A = '"Archivo Variable", ' + CJK, text = 0xE7EEE7, plain = { trace: Color(text, 1), trace2: Color(0x6B7B73, 1), wash: Color(text, 0.08), washEnd: Color(text, 0.08) };
    return {
      skin: "paper", text: Color(text, 1), text2: Color(0xA1B0A8, 1), text3: Color(0x6B7B73, 1), rule: Color(0x263732, 1), track: Color(text, 0.08),
      signal: Color(0xFF7B4A, 1), inks: {}, ink: plain,
      body: Face(A, 13, 500), small: Face(A, 12, 500), title: Face(A, 13, 650), figure: Face(A, 42, 620, 66, -0.01), figureLine: 0.84,
      unit: Face(A, 15, 500, 80), value: Face(A, 13, 500, 85),
      padX: 18, padTop: 12, padBottom: 12, outer: [0, 0, 0], laneGap: 0, columnGap: 0, barGap: 0, barHeight: 44, barPad: [18, 10], ruled: true,
      meterHeight: 3, meterRadius: 0, threadRadius: 0, radius: 16, inset: 0,
      shadows: [{ y: 18, blur: 40, spread: -12, color: Color(0, 0.6) }, { y: 2, blur: 6, spread: 0, color: Color(0, 0.3) }],
      paper: Color(0x14231F, 1), stroke: Color(0x263732, 1)
    };
  }
  // The user's accent, Windows' own blue where none is said (theme.rs `accent`).
  function fluent() {
    var S = '"Segoe UI Variable Text", "Segoe UI", ' + CJK, D = '"Segoe UI Variable Display", "Segoe UI", ' + CJK;
    return {
      skin: "fluent", text: Color(0xFFFFFF, 1), text2: Color(0xFFFFFF, 0.76), text3: Color(0xFFFFFF, 0.5), rule: Color(0xFFFFFF, 0.06), track: Color(0xFFFFFF, 0.14),
      signal: Color(0xFF99A4, 1), inks: {}, ink: ink(0x99EBFF, 0.5, 0.22, 0),
      body: Face(S, 13, 500), small: Face(S, 12, 500), title: Face(S, 14, 600), figure: Face(D, 34, 600, 100, -0.02), figureLine: 0.86,
      unit: Face(S, 15, 500), value: Face(S, 13, 500),
      padX: 16, padTop: 10, padBottom: 12, outer: [10, 6, 6], laneGap: 0, columnGap: 0, barGap: 0, barHeight: 52, barPad: [22, 14], ruled: false,
      meterHeight: 4, meterRadius: 2, threadRadius: 1, radius: 8, inset: 12,
      shadows: [{ y: 8, blur: 32, spread: 0, color: Color(0, 0.4) }, { y: 2, blur: 8, spread: 0, color: Color(0, 0.25) }],
      luminosity: Color(0x2C2C2C, 0.96), tint: Color(0x2C2C2C, 0.15), stroke: Color(0xFFFFFF, 0.09), footer: Color(0, 0.18)
    };
  }
  function glass() {
    var I = '"Inter", ' + CJK, lane = function (rgb) { return ink(rgb, 0.55, 0.34, 0); };
    return {
      skin: "glass", text: Color(0xFFFFFF, 0.95), text2: Color(0xFFFFFF, 0.68), text3: Color(0xFFFFFF, 0.46), rule: Color(0xFFFFFF, 0.1), track: Color(0xFFFFFF, 0.16),
      signal: Color(0xFF6961, 1), inks: { game: lane(0x63E6C8), gpu: lane(0x30D158), memory: lane(0xFF9F0A), network: lane(0xC77DFF), disk: lane(0x5AC8F5), storage: lane(0x5AC8F5) },
      ink: lane(0x3D9BFF),
      body: Face(I, 13, 500), small: Face(I, 12, 500), title: Face(I, 13, 650), figure: Face(I, 40, 600, 100, -0.035), figureLine: 0.84,
      unit: Face(I, 15, 500), value: Face(I, 13, 500),
      padX: 18, padTop: 14, padBottom: 14, outer: [0, 0, 0], laneGap: 8, columnGap: 8, barGap: 8, barHeight: 48, barPad: [20, 8], ruled: false,
      meterHeight: 6, meterRadius: 3, threadRadius: 2, radius: 24, inset: 10,
      shadows: [{ y: 10, blur: 30, spread: 0, color: Color(0, 0.3) }, { y: 1, blur: 3, spread: 0, color: Color(0, 0.2) }],
      glass: Color(0x14141A, 1), glassClear: 0.16, glassFrosted: 0.68, legibility: Color(0, 0.5)
    };
  }
  var LOOKS = { paper: paper, win11: fluent, glass: glass };
  function themed(look) {
    var T = (LOOKS[look] || glass)();
    // The ink of module `id`'s lane, and whether two are drawn in the same.
    T.inkOf = function (id) { return T.inks[id.split(":")[0]] || T.ink; };
    T.sameInk = function (a, b) { var x = T.inkOf(a).trace, y = T.inkOf(b).trace; return x.rgb === y.rgb && x.a === y.a; };
    // Room its shadows need round it (theme.rs `room`).
    T.margin = Math.ceil(T.shadows.reduce(function (m, s) { return Math.max(m, s.blur + s.spread + Math.abs(s.y)); }, 0));
    return T;
  }

  // ---------------- measuring ----------------
  var mctx = document.createElement("canvas").getContext("2d");
  // Canvas widths are keywords: each face's width as the nearest of them.
  function stretchOf(width) {
    var keys = [[50, "ultra-condensed"], [62.5, "extra-condensed"], [75, "condensed"], [87.5, "semi-condensed"], [100, "normal"]];
    return keys.reduce(function (a, b) { return Math.abs(b[0] - width) < Math.abs(a[0] - width) ? b : a; })[1];
  }
  function face(c, f) {
    c.font = f.weight + " " + f.size + "px " + f.family;
    try { c.fontStretch = stretchOf(f.width); } catch (e) { /* (an older canvas: its normal width) */ }
    try { c.letterSpacing = f.tracking * f.size + "px"; } catch (e) { /* (an older canvas: no tracking) */ }
  }
  var metrics = {};
  function measured(text, f) { face(mctx, f); return mctx.measureText(text); }
  function measure(text, f) { return text ? measured(text, f).width : 0; }
  // The face's ascent and descent (its line box's, as DirectWrite has them).
  function baseline(f) {
    var key = f.family + f.size + f.weight + f.width;
    if (!metrics[key]) { var m = measured("Hg", f); metrics[key] = [m.fontBoundingBoxAscent, m.fontBoundingBoxDescent]; }
    return metrics[key];
  }
  // How far the ink of `text` reaches, from its line box's top (canvas.rs `ink`).
  function inkOf(text, f) { var m = measured(text, f), a = baseline(f)[0]; return [a - m.actualBoundingBoxAscent, a + m.actualBoundingBoxDescent]; }
  // `text`, or as much of its start as is no wider than `room` with an ellipsis after it.
  function fitted(text, f, room) {
    if (measure(text, f) <= room) return text;
    var kept = Array.from(text);
    for (;;) {
      kept.pop();
      var cut = kept.join("").replace(/\s+$/, "") + "…";
      if (!kept.length || measure(cut, f) <= room) return cut;
    }
  }

  // ---------------- what it draws, recorded ----------------
  // As Glance records a layout to turn it into the next (morph.rs): each thing drawn named by its part ("cpu.value"),
  // the same part in two layouts the same thing, moved and resized between them. Recorded `k` times as large (the
  // zoom it is drawn at), from `at`.
  function Rec(T, k, at) {
    var els = [], key = "", seen = {};
    function push(e) {
      e.key = key;
      // (Several things in one part: each by its kind and its place among them.)
      var id = key + "|" + e.t, n = seen[id] = (seen[id] || 0) + 1;
      e.id = key ? id + "|" + n : null;
      e.k = k;
      els.push(e);
    }
    var X = function (x) { return at[0] + x * k; }, Y = function (y) { return at[1] + y * k; };
    var r = {
      els: els, T: T,
      key: function (name) { key = name; },
      // `text` on one line from (`x`, `top`), its line box's top there, at the start or end of `width`, cut short to fit.
      text: function (text, f, color, x, top, width, end) {
        if (!text) return;
        var s = fitted(text, f, Math.max(0, width)), w = measure(s, f);
        push({ t: "text", text: s, x: X(end ? x + width - w : x), y: Y(top + baseline(f)[0]), f: sized(f, f.size * k, f.weight), color: color });
      },
      // `text` standing on `base`; how wide it is.
      onLine: function (text, f, color, x, base) {
        if (!text) return 0;
        push({ t: "text", text: text, x: X(x), y: Y(base), f: sized(f, f.size * k, f.weight), color: color });
        return measure(text, f);
      },
      fill: function (color, x, y, w, h, radius) { push({ t: "fill", x: X(x), y: Y(y), w: w * k, h: h * k, r: (radius || 0) * k, color: color }); },
      // A track `w` long filled `frac` of the way in `color`; `up`, from its foot.
      meter: function (x, y, w, h, radius, frac, color, up) { push({ t: "meter", x: X(x), y: Y(y), w: w * k, h: h * k, r: radius * k, frac: frac, color: color, track: T.track, up: !!up }); },
      ring: function (x, y, radius, share, color, icon) { push({ t: "ring", x: X(x), y: Y(y), r: radius * k, share: share, color: color, track: T.track, icon: icon }); },
      // A lane's chart (view.rs `paint_plot`), or a small layout's (forms.rs `chart`).
      plot: function (x, y, w, h, o) { push(Object.assign({ t: "plot", x: X(x), y: Y(y), w: w * k, h: h * k, rule: T.rule, signal: T.signal }, o)); },
      chart: function (x, y, w, h, o) { push(Object.assign({ t: "chart", x: X(x), y: Y(y), w: w * k, h: h * k }, o)); }
    };
    return r;
  }

  // ---------------- the readings, as the panel has them ----------------
  function words() { return window.GlanceDemo.words; }
  function parts() { return window.GlanceDemo.parts; }
  function latest() { return window.GlanceDemo.read(); }
  var MB = 1048576;
  // A rate written out (text.rs `rate`), whole or as a figure and its unit, short (one letter of the unit).
  function rateText(bytes) {
    var units = ["B/s", "KB/s", "MB/s", "GB/s"], u = 0, v = bytes;
    while (v >= 1000 && u < units.length - 1) { v /= 1024; u++; }
    return v.toFixed(u === 0 || v >= 100 ? 0 : 1) + " " + units[u];
  }
  function rateParts(bytes, short) {
    var w = rateText(bytes).split(" "), unit = w[1];
    if (short) unit = unit.length > 3 && /[A-Z]/.test(unit[0]) ? unit[0] : "";
    return [w[0], unit];
  }
  function percent(v) { return v < 10 ? v.toFixed(1) + "%" : v.toFixed(0) + "%"; }
  // A share of a process's, as the panel's table writes it (demo.js).
  function share(v) { return (v < 10 ? v.toFixed(1) : Math.round(v)) + "%"; }
  // A chart's top for rates, rounded up to 1, 2 or 5 of a unit (view.rs `full_scale`, `round_up_rate`).
  function fullScale(series) {
    var peak = series.reduce(function (m, s) { return s.reduce(function (a, v) { return Math.max(a, v); }, m); }, 10 * 1024);
    var unit = Math.pow(1024, Math.floor(Math.log(peak) / Math.log(1024))), mag = Math.pow(10, Math.floor(Math.log10(peak / unit))), lead = peak / unit / mag;
    return (lead <= 1 ? 1 : lead <= 2 ? 2 : lead <= 5 ? 5 : 10) * mag * unit;
  }
  function bytes(series) { return series.map(function (v) { return v * MB; }); }

  // The panel's lanes (view.rs `lane`), all of each or compact: its blocks.
  function lanesOf(R, full) {
    var W = words(), P = parts(), ccd = W.ccd, rate = window.GlanceDemo.rate;
    var readout = function (v, series) { return { k: "readout", figure: String(Math.round(v)), unit: "%", hot: v > HOT_LOAD, series: [series], max: 100, hotAt: HOT_LOAD }; };
    var facts = function (rows) { return { k: "facts", gap: 10, rows: full ? rows : [] }; };
    var cpu = [
      { k: "head", title: W.cpu, device: P.cpu, aside: Math.round(R.cpuTemp) + " °C", hot: R.cpuTemp > HOT_TEMP },
      readout(R.cpu, R.cpuHist),
      facts([[W.clock, R.clock.toFixed(2) + " GHz"], [W.power, R.power.toFixed(1) + " W"],
        [ccd + " 1", Math.round(R.ccd1) + " °C", R.ccd1 > HOT_TEMP], [ccd + " 2", Math.round(R.ccd2) + " °C", R.ccd2 > HOT_TEMP]])
    ];
    if (full) cpu.push({ k: "threads", loads: R.threads });
    var gpu = [
      { k: "head", title: W.gpu, device: P.gpu, aside: Math.round(R.gpuTemp) + " °C", hot: R.gpuTemp > HOT_TEMP },
      readout(R.gpu, R.gpuHist),
      { k: "meter", label: W.vram, frac: R.vram / 16, value: R.vram.toFixed(1) + " / 16 GB", gap: 8 }
    ];
    if (full) gpu.push(facts([[W.clock, Math.round(R.gpuClock) + " MHz"], [W.power, R.gpuPower.toFixed(1) + " W"], [W.fan, Math.round(R.fan) + " RPM"]]));
    var down = bytes(R.downHist), up = bytes(R.upHist);
    return [
      { id: "cpu", blocks: cpu },
      { id: "gpu:0", blocks: gpu },
      { id: "memory", blocks: [{ k: "head", title: W.memory, device: P.memory, aside: (R.mem / 100 * 61.6).toFixed(1) + " / 61.6 GB" }, readout(R.mem, R.memHist), facts([])] },
      { id: "network", blocks: [{ k: "head", title: W.network, device: P.network, aside: "" },
        { k: "rates", rows: [[W.down, rate(R.down)], [W.up, rate(R.up)]], series: [down, up], max: fullScale([down, up]) }, facts([])] },
      { id: "processes", blocks: [{ k: "head", title: W.processes, device: "", aside: "" },
        { k: "table", headings: W.cols, rows: R.procs.slice(0, full ? 5 : COMPACT_PROCESSES).map(function (p) { return [p.name, share(p.cpu), p.mem.toFixed(1) + " GB", share(p.gpu)]; }) }] }
    ];
  }
  function blockHeight(b) {
    switch (b.k) {
      case "head": return HEAD + HEAD_GAP;
      case "readout": return PLOT;
      case "rates": return RATE_PLOT;
      case "threads": return 10 + 14;
      case "meter": return b.gap + LINE;
      case "facts": return b.rows.length ? b.gap + b.rows.length * LINE + (b.rows.length - 1) * FACT_GAP : 0;
      case "table": return b.rows.length * TABLE_ROW;
    }
    return 0;
  }
  function laneHeight(T, lane) { return T.padTop + T.padBottom + lane.blocks.reduce(function (a, b) { return a + blockHeight(b); }, 0); }

  // The readings the small layouts show (forms.rs `readings`): the CPU, the card, memory, the network both ways.
  function readingsOf(R) {
    var W = words();
    var chip = function (id, module, icon, label, v, temp, series) {
      return { id: id, module: module, icon: icon, label: label, share: v / 100, figure: String(Math.round(v)), unit: "%",
        beside: temp == null ? "" : Math.round(temp) + "°", detail: temp == null ? "" : Math.round(temp) + " °C",
        hot: v > HOT_LOAD, warm: temp != null && temp > HOT_TEMP, hotAt: HOT_LOAD, series: series, most: 100 };
    };
    var list = [
      chip("cpu", "cpu", "cpu", "CPU", R.cpu, R.cpuTemp, R.cpuHist),
      chip("gpu:0", "gpu", "gpu", "GPU", R.gpu, R.gpuTemp, R.gpuHist),
      chip("memory", "memory", "memory", W.memory, R.mem, null, R.memHist)
    ];
    list[2].detail = (R.mem / 100 * 61.6).toFixed(1) + " GB";
    var down = bytes(R.downHist), up = bytes(R.upHist);
    list.push({ id: "network", module: "network", icon: "network", label: W.network, share: null, figure: "", unit: "", beside: "", detail: "",
      hot: false, warm: false, hotAt: null, series: down, second: up,
      most: down.concat(up).reduce(function (a, v) { return Math.max(a, v); }, 10 * 1024),
      pair: [["↓", R.down], ["↑", R.up]].map(function (p) { return { mark: p[0], full: rateParts(p[1] * MB, false), short: rateParts(p[1] * MB, true) }; }) });
    return list;
  }
  // The busiest process and its share (forms.rs `busiest`).
  function busiest(R) {
    var p = R.procs.reduce(function (a, b) { return b.cpu > a.cpu ? b : a; });
    return [p.name, percent(p.cpu)];
  }

  // ---------------- the lanes laid out (view.rs `Layout`) ----------------
  // Where each column starts, and the tallest column, the lanes split into `columns` so the tallest is least.
  function split(hs, columns, gap) {
    var n = hs.length, best = [], from = [];
    var span = function (a, b) { var s = 0; for (var i = a; i < b; i++) s += hs[i]; return s + Math.max(0, b - a - 1) * gap; };
    for (var k = 0; k <= columns; k++) { best.push(new Array(n + 1).fill(Infinity)); from.push(new Array(n + 1).fill(0)); }
    best[0][0] = 0;
    for (k = 1; k <= columns; k++) for (var i = k; i <= n; i++) for (var j = k - 1; j < i; j++) {
      var c = Math.max(best[k - 1][j], span(j, i));
      if (c < best[k][i]) { best[k][i] = c; from[k][i] = j; }
    }
    var cuts = new Array(columns).fill(0), at = n;
    for (k = columns; k >= 1; k--) { cuts[k - 1] = from[k][at]; at = from[k][at]; }
    return { cuts: cuts, tallest: best[columns][n] };
  }
  function Layout(T, heights, columns) {
    columns = Math.max(1, Math.min(columns, heights.length));
    var s = split(heights, columns, T.laneGap);
    return { T: T, columns: columns, cuts: s.cuts, heights: heights, lanesHeight: s.tallest, columnWidth: COLUMN_WIDTH, grow: 0 };
  }
  // `by` wider and taller: the width shared out among the columns, the height given to each, for its charts.
  function stretched(L, by) {
    return Object.assign({}, L, { columnWidth: L.columnWidth + Math.max(0, by[0]) / L.columns, lanesHeight: L.lanesHeight + Math.max(0, by[1]), grow: Math.max(0, by[1]) });
  }
  function widthOf(L) { return L.columns * L.columnWidth + (L.columns - 1) * L.T.columnGap; }
  function heightOf(L) { var T = L.T; return T.outer[0] + L.lanesHeight + T.outer[2] + T.barGap + T.barHeight; }
  function inner(L) { return (widthOf(L) - 2 * L.T.outer[1] - (L.columns - 1) * L.T.columnGap) / L.columns; }
  function columnX(L, col) { return L.T.outer[1] + col * (inner(L) + L.T.columnGap); }
  function ends(L, col) { return [L.cuts[col], col + 1 < L.columns ? L.cuts[col + 1] : L.heights.length]; }
  // Each lane's box, a shorter column's lanes sharing what is left over, so every column ends at the same line.
  function boxes(L) {
    var out = [], gap = L.T.laneGap;
    for (var col = 0; col < L.columns; col++) {
      var e = ends(L, col), count = e[1] - e[0], natural = 0;
      for (var i = e[0]; i < e[1]; i++) natural += L.heights[i];
      natural += Math.max(0, count - 1) * gap;
      var extra = (L.lanesHeight - natural) / Math.max(1, count), y = L.T.outer[0];
      for (i = e[0]; i < e[1]; i++) {
        out.push({ x: columnX(L, col), y: y, w: inner(L), h: L.heights[i] + extra });
        y += L.heights[i] + extra + gap;
      }
    }
    return out;
  }
  function barOf(L) { var T = L.T; return { x: 0, y: T.outer[0] + L.lanesHeight + T.outer[2] + T.barGap, w: widthOf(L), h: T.barHeight }; }
  // How much each lane's charts may grow: its column's share of what the layout was stretched by.
  function grows(L) {
    var out = [];
    for (var col = 0; col < L.columns; col++) { var e = ends(L, col); for (var i = e[0]; i < e[1]; i++) out.push(L.grow / Math.max(1, e[1] - e[0])); }
    return out;
  }

  // ---------------- drawing the lanes (view.rs `paint`, `paint_lane`, `paint_bar`) ----------------
  // `text`'s baseline where a CSS line box `line` tall, aligned to `bottom`, would put it (view.rs `text_on_line`).
  function textOnLine(r, text, f, color, x, bottom, line, width) {
    var m = baseline(f);
    r.text(text, f, color, x, bottom - line / 2 + (m[0] - m[1]) / 2 - m[0], width);
  }
  function paintLane(r, R, lane, area, grow) {
    var T = r.T, ink = T.inkOf(lane.id);
    var left = area.x + T.padX, width = area.w - 2 * T.padX, plotLeft = left + LABEL + LABEL_GAP, plotWidth = left + width - plotLeft;
    var y = area.y + T.padTop, id = lane.id;
    var charts = lane.blocks.filter(function (b) { return b.k === "readout" || b.k === "rates"; }).length;
    var taller = charts ? Math.min(grow / charts, RATE_PLOT) : 0;
    var kinds = {};
    var name = function (part) { r.key(id + "." + part); };
    lane.blocks.forEach(function (b) {
      var nth = kinds[b.k] = (kinds[b.k] == null ? 0 : kinds[b.k] + 1);
      var part = function (what) { name(b.k + nth + "." + what); };
      var height = blockHeight(b) + (b.k === "readout" || b.k === "rates" ? taller : 0);
      switch (b.k) {
        case "head": {
          var tw = measure(b.title, T.title), row = y + (HEAD - T.title.size) / 2 - 1;
          name("label");
          r.text(b.title, T.title, T.text, left, row, width);
          var aw = b.aside ? measure(b.aside, T.body) : 0;
          name("device");
          r.text(b.device, T.small, T.text3, left + tw + 8, row + 1, width - tw - 8 - (aw > 0 ? aw + 8 : 0));
          name("detail");
          r.text(b.aside, T.body, b.hot ? T.signal : T.text2, left, row, width, true);
          break;
        }
        case "readout": {
          var fw = measure(b.figure, T.figure), bottom = y + height;
          name("value");
          textOnLine(r, b.figure, T.figure, b.hot ? T.signal : T.text, left, bottom, T.figure.size * T.figureLine, LABEL);
          name("unit");
          textOnLine(r, b.unit, T.unit, T.text2, left + fw + 3, bottom, T.unit.size, Math.max(LABEL - fw - 3, 30));
          name("chart");
          r.plot(plotLeft, y, plotWidth, height, { series: b.series, max: b.max, hotAt: b.hotAt, ink: ink, steps: R.steps });
          break;
        }
        case "rates": {
          b.rows.forEach(function (row, i) {
            var ry = y + height - (b.rows.length - i) * (LINE + 2) + 1, which = i ? "b" : "a";
            name(which + ".mark");
            r.fill(i ? ink.trace2 : ink.trace, left, ry + LINE / 2, 10, i ? 1 : 2);
            r.text(row[0], T.small, T.text2, left + 15, ry + 1, 40);
            name(which + ".value");
            r.text(row[1], T.value, T.text, left, ry, LABEL + RATE_INDENT, true);
          });
          name("chart");
          r.plot(plotLeft + RATE_INDENT, y, plotWidth - RATE_INDENT, height, { series: b.series, max: b.max, hotAt: null, ink: ink, steps: R.steps });
          break;
        }
        case "threads": {
          part("cells");
          var count = Math.max(1, b.loads.length), cell = (width - 2 * (count - 1)) / count;
          b.loads.forEach(function (load, i) {
            r.meter(left + i * (cell + 2), y + 10, cell, 14, T.threadRadius, load, load * 100 > HOT_LOAD ? T.signal : ink.trace, true);
          });
          break;
        }
        case "meter": {
          var my = y + b.gap;
          part("meter");
          r.text(b.label, T.small, T.text2, left, my, LABEL);
          var vw = measure(b.value, T.small);
          r.text(b.value, T.small, T.text2, left, my, width, true);
          r.meter(plotLeft, my + (LINE - T.meterHeight) / 2, left + width - vw - LABEL_GAP - plotLeft, T.meterHeight, T.meterRadius, b.frac, ink.trace);
          break;
        }
        case "facts": {
          b.rows.forEach(function (row, i) {
            var ry = y + b.gap + i * (LINE + FACT_GAP);
            part(String(i));
            r.text(row[0], T.small, T.text2, left, ry, LABEL);
            r.text(row[1], T.small, row[2] ? T.signal : T.text, plotLeft, ry, left + width - plotLeft);
          });
          break;
        }
        case "table": {
          part("rows");
          // The headings share the lane's head row, after the title, each one's ink centred on the title's.
          var headY = y - HEAD - HEAD_GAP, measures = PROCESS_WIDTHS.reduce(function (a, w) { return a + w + 6; }, 0);
          var cols = [[left, width - measures]], cx = left + width - measures + 6;
          PROCESS_WIDTHS.forEach(function (w) { cols.push([cx, w]); cx += w + 6; });
          var mid = function (text, f) { var i = inkOf(text, f); return (i[0] + i[1]) / 2; };
          var centre = headY + (HEAD - T.title.size) / 2 - 1 + mid(words().processes, T.title);
          b.headings.forEach(function (label, i) {
            // (Sorted by the CPU's share, its heading the chosen one.)
            var chosen = i === 0, f = chosen ? sized(T.small, T.small.size, 650) : T.small;
            r.text(label, f, chosen ? T.text : T.text2, cols[i + 1][0], centre - mid(label, f), cols[i + 1][1], true);
          });
          b.rows.forEach(function (row, i) {
            var ty = y + i * TABLE_ROW + (TABLE_ROW - LINE) / 2;
            row.forEach(function (cell, j) {
              if (j === 0) r.text(cell, T.body, T.text, cols[0][0], ty, cols[0][1]);
              else r.text(cell, T.small, T.text2, cols[j][0], ty, cols[j][1], true);
            });
          });
          break;
        }
      }
      y += height;
    });
  }
  function paintLanes(r, R, L, lanes) {
    var T = r.T, bx = boxes(L), gr = grows(L);
    lanes.forEach(function (lane, i) {
      paintLane(r, R, lane, bx[i], gr[i]);
      // Chart paper rules each lane off below.
      if (T.ruled) { r.key(""); r.fill(T.rule, bx[i].x, bx[i].y + bx[i].h - 1, bx[i].w, 1); }
    });
    r.key("");
    if (T.ruled) for (var col = 1; col < L.columns; col++) r.fill(T.rule, columnX(L, col), 0, 1, L.lanesHeight);
    // The bar: how long the machine has been up (a widget's has no buttons of the panel's: its own are off its edge).
    var bar = barOf(L);
    r.key("bar");
    if (T.skin === "fluent") { r.fill(T.footer, bar.x, bar.y, bar.w, bar.h); r.fill(T.rule, bar.x, bar.y, bar.w, 1); }
    var start = bar.x + T.barPad[0];
    r.text(words().uptime(R.uptime), T.small, T.text2, start, bar.y + (bar.h - LINE) / 2, bar.w - 2 * start);
    r.key("");
  }

  // ---------------- drawing the small layouts (forms.rs) ----------------
  // How far in from its edges a layout is laid, clear of corners `radius` round.
  function inside(radius, pad) { return Math.max(radius * 0.75, pad); }
  function Painter(r, R, list) {
    var T = r.T, P;
    var font = function (size, weight) { return sized(T.body, size, weight); };
    var figureFont = function (size, weight) { return sized(T.figure, size, weight); };
    var name = function (reading, part) { r.key(reading.id + "." + part); };
    var warmth = function (reading, otherwise) { return reading.warm ? T.signal : otherwise; };
    var inkOfReading = function (reading) { return reading.hot ? T.signal : T.inkOf(reading.module).trace; };
    // The baseline that puts the ink of a figure in `f` centred on `middle`.
    var baselineAt = function (f, middle) { var i = inkOf("8", f); return middle - (i[0] + i[1]) / 2 + baseline(f)[0]; };
    // `text` with its ink centred on `middle`, from `x` (or ending there); how wide it is.
    var level = function (text, f, color, x, middle, end) {
      var i = inkOf(text, f), w = measure(text, f);
      r.text(text, f, color, end ? x - w : x, middle - (i[0] + i[1]) / 2, w + 2);
      return w;
    };
    // How wide `draw` draws, drawing nothing.
    var wide = function (draw) { return draw(Painter(Rec(T, 1, [0, 0]), R, list)); };
    P = {
      ring: function (reading, cx, cy, radius) { name(reading, "icon"); r.ring(cx, cy, radius, reading.share, inkOfReading(reading), reading.icon); },
      figure: function (reading, x, base, f, unit, unitColor) {
        name(reading, "value");
        var w = r.onLine(reading.figure, f, reading.hot ? T.signal : T.text, x, base);
        if (!reading.unit) return w;
        var gap = /^[A-Za-z]/.test(reading.unit) ? f.size * 0.25 : 1;
        name(reading, "unit");
        return w + gap + r.onLine(reading.unit, unit, unitColor, x + w + gap, base);
      },
      rate: function (reading, second, x, base, f, unit, short) {
        var rate = reading.pair[second ? 1 : 0], which = second ? "b" : "a", fu = short ? rate.short : rate.full;
        var i = inkOf(fu[0], f), middle = base - baseline(f)[0] + (i[0] + i[1]) / 2;
        name(reading, which + ".mark");
        var w = level(rate.mark, sized(T.body, f.size * 0.86, 500), T.text3, x, middle) + 3;
        name(reading, which + ".value");
        w += r.onLine(fu[0], f, second ? T.text2 : T.text, x + w, base);
        if (fu[1]) { name(reading, which + ".unit"); w += 1 + r.onLine(fu[1], unit, T.text2, x + w + 1, base); }
        return w;
      },
      chip: function (reading, x, middle, size, beside, radius) {
        P.ring(reading, x + radius, middle, radius);
        var start = x + 2 * radius + 6, f = font(size, 600), base = baselineAt(f, middle);
        if (reading.pair) {
          var first = P.rate(reading, false, start, base, f, font(size * 0.78, 500), true);
          var second = P.rate(reading, true, start + first + 6, base, f, font(size * 0.78, 500), true);
          return start + first + 6 + second - x;
        }
        var end = start + P.figure(reading, start, base, f, f, reading.hot ? T.signal : T.text);
        if (beside && reading.beside) { name(reading, "beside"); end += 5 + r.onLine(reading.beside, font(size, 500), warmth(reading, T.text3), end + 5, base); }
        return end - x;
      },
      // Chips in a row from `left` to `right`, the space between shared; what goes beside a figure only where it fits.
      row: function (readings, left, right, middle, size, beside, radius) {
        var withBeside = function (b) { return readings.map(function (rd) { return wide(function (p) { return p.chip(rd, 0, middle, size, b, radius); }); }); };
        var widths = withBeside(beside), sum = function (a) { return a.reduce(function (s, v) { return s + v; }, 0); };
        beside = beside && sum(widths) + Math.max(0, readings.length - 1) * 8 <= right - left;
        if (!beside) widths = withBeside(false);
        var gap = readings.length > 1 ? (right - left - sum(widths)) / (readings.length - 1) : 0, x = left;
        readings.forEach(function (rd, i) { P.chip(rd, x, middle, size, beside, radius); x += widths[i] + gap; });
      },
      // A reading's chart, ruled, filled below its line, a second line dashed over it, and where the look gives both
      // the same ink, a key to which is which.
      chart: function (reading, second, box, legend) {
        if (box[2] < 2 || box[3] < 2) return;
        name(reading, "chart");
        var ink = T.inkOf(reading.module), lines = [{ series: reading.series, most: reading.most, color: ink.trace, width: 1.5, fill: ink.wash, hotAt: reading.hotAt }];
        var key = null;
        if (second === "pair") lines.push({ series: reading.second, most: reading.most, color: ink.trace2, width: 1.2, dashed: true });
        else if (second) {
          var alike = T.sameInk(reading.module, second.module), color = alike ? T.text3 : alpha(T.inkOf(second.module).trace, 0.9);
          lines.push({ series: second.series, most: second.most, color: color, width: 1.2, dashed: true });
          if (legend && alike && box[3] >= 40) key = [[reading.label, ink.trace, false], [second.label, color, true]];
        }
        r.chart(box[0], box[1], box[2], box[3], { lines: lines, legend: key, rule: T.rule, signal: T.signal, small: font(10.5, 500), text2: T.text2 });
      },
      history: function (area, withCard, legend) {
        var main = list.filter(function (rd) { return rd.id === "cpu"; })[0];
        var card = withCard ? list.filter(function (rd) { return rd.module === "gpu"; })[0] : null;
        P.chart(main, card, area, legend);
      },
      tiles: function (across, down, w, h) {
        var tw = (w - 2 * TILES_PAD) / across, th = (h - 2 * TILES_PAD) / down;
        list.slice(0, across * down).forEach(function (rd, i) {
          var x = TILES_PAD + (i % across) * tw, y = TILES_PAD + Math.floor(i / across) * th, below;
          r.key("");
          if (i % across > 0) r.fill(T.rule, x, y + 8, 1, th - 16);
          if (i >= across) r.fill(T.rule, x + 8, y, tw - 16, 1);
          P.ring(rd, x + 22, y + 22, 11);
          name(rd, "label");
          var beyond = rd.detail ? measure(rd.detail, T.small) + 10 : 0;
          level(fitted(rd.label, T.small, tw - 40 - 12 - beyond), T.small, T.text2, x + 40, y + 22);
          if (rd.detail) { name(rd, "detail"); level(rd.detail, T.small, warmth(rd, T.text3), x + tw - 12, y + 22, true); }
          if (rd.pair) {
            var f = figureFont(17, 620), u = font(11, 500);
            P.rate(rd, false, x + 12, y + 58, f, u, false);
            P.rate(rd, true, x + 12, y + 78, f, u, false);
            below = y + 86;
          } else {
            P.figure(rd, x + 12, y + 70, figureFont(30, T.figure.weight), font(12, 500), T.text2);
            below = y + 78;
          }
          P.chart(rd, rd.pair ? "pair" : null, [x + 12, below, tw - 24, Math.max(y + th - 10 - below, 14)], false);
        });
      },
      corner: function (w, h) {
        var pad = inside(T.radius, 14), base = 14;
        P.history([pad, pad, w - 2 * pad, h - pad - base - 70], true, true);
        P.row(list.filter(function (rd) { return !rd.pair; }).slice(0, 3), pad, w - pad, h - base - 46, 14, true, 10);
        P.row(list.filter(function (rd) { return rd.pair; }), pad, w - pad, h - base - 12, 13, false, 10);
      },
      tall: function (w, h) {
        var pad = inside(T.radius, 14), base = 14;
        var chips = list.filter(function (rd) { return !rd.pair; }).slice(0, 4), pairs = list.filter(function (rd) { return rd.pair; });
        var rows = Math.ceil(chips.length / 2), below = rows * TALL_CHIPS + pairs.length * TALL_PAIR;
        P.history([pad, pad, w - 2 * pad, h - pad - base - 8 - below], true, true);
        var top = h - base - below, half = (w - 2 * pad) / 2;
        chips.forEach(function (rd, i) {
          var middle = top + Math.floor(i / 2) * TALL_CHIPS + TALL_CHIPS / 2;
          var beside = wide(function (p) { return p.chip(rd, 0, middle, 16, true, 11); }) <= half - 8;
          P.chip(rd, pad + (i % 2) * half, middle, 16, beside, 11);
        });
        pairs.forEach(function (rd, i) { P.row([rd], pad, w - pad, top + rows * TALL_CHIPS + i * TALL_PAIR + TALL_PAIR / 2, 15, false, 11); });
      },
      strip: function (rich, w, h) {
        var middle = h / 2, widths = [], sum = function () { return widths.reduce(function (s, v) { return s + v; }, 0); };
        if (rich) widths.push(84);
        list.forEach(function (rd) { widths.push(wide(function (p) { return p.chip(rd, 0, middle, 14, rich, 8.5); })); });
        var most = rich ? busiest(R) : null, pad = inside(T.radius, 12);
        if (most) {
          var whole = measure(most[0], T.body) + 8 + measure(most[1], T.body), room = w - 2 * pad - sum() - 6 * widths.length;
          widths.push(Math.min(whole, Math.max(room, 0)));
        }
        var gap = Math.max((w - 2 * pad - sum()) / (Math.max(widths.length, 2) - 1), 6), x = pad, at = 0;
        if (rich) { P.history([x, 9, widths[0], h - 18], false, false); x += widths[at++] + gap; }
        list.forEach(function (rd) { P.chip(rd, x, middle, 14, rich, 8.5); x += widths[at++] + gap; });
        if (most) {
          r.key("busiest");
          var shareWidth = measure(most[1], T.body), i = inkOf(most[0], T.body), left = Math.max(widths[at] - shareWidth - 8, 0);
          r.text(most[0], T.body, T.text2, x, middle - (i[0] + i[1]) / 2, left);
          level(most[1], T.body, T.text2, x + Math.min(measure(most[0], T.body), left) + 8, middle);
        }
      },
      rail: function (w, h) {
        var pad = inside(T.radius, 10), base = 10, rows = list.length;
        var chart = Math.max(h - pad - base - rows * RAIL_ROW - 8, 36);
        P.history([pad, pad, w - 2 * pad, chart], true, true);
        var gap = Math.max((h - pad - base - chart - 8 - rows * RAIL_ROW) / Math.max(rows, 1), 0), wide = w >= RAIL_WIDE;
        var f = font(13, 600), small = font(11, 500);
        list.forEach(function (rd, i) {
          var top = pad + chart + 8 + i * (RAIL_ROW + gap) + gap / 2, middle = top + 11;
          P.ring(rd, pad + 9, middle, 8.5);
          var x = pad + 24, base = baselineAt(f, middle), under = baselineAt(small, middle + 15);
          if (rd.pair) {
            var first = P.rate(rd, false, x, base, f, font(10, 500), true);
            if (wide) P.rate(rd, true, x + first + 6, base, f, font(10, 500), true);
            else P.rate(rd, true, x, under, small, font(9, 500), true);
            return;
          }
          var width = P.figure(rd, x, base, f, f, rd.hot ? T.signal : T.text);
          if (rd.beside) {
            name(rd, "beside");
            var fits = x + width + 5 + measure(rd.beside, font(13, 500)) <= w - pad;
            if (wide && fits) r.onLine(rd.beside, font(13, 500), warmth(rd, T.text3), x + width + 5, base);
            else if (x + measure(rd.beside, small) <= w - pad) r.onLine(rd.beside, small, warmth(rd, T.text3), x, under);
          }
        });
      },
      micro: function (w, h) {
        var chips = list.filter(function (rd) { return rd.share != null; }).slice(0, 3), cell = (w - 16) / Math.max(1, chips.length);
        chips.forEach(function (rd, i) { P.ring(rd, 8 + cell * (i + 0.5), h / 2, Math.min(h / 2 - 5, 13)); });
      }
    };
    return P;
  }

  // ---------------- the forms, and the ladder that chooses among them (arrange.rs) ----------------
  // A form by the name Glance keeps it under: "full-2", "compact-1", "tiles-3x2", "corner", "corner-tall", "strip",
  // "strip-short", "rail", "micro".
  function levelOf(form) {
    if (/^full-/.test(form)) return 0;
    if (/^compact-/.test(form)) return 1;
    if (/^tiles-/.test(form)) return 2;
    return { corner: 3, "corner-tall": 3, strip: 4, rail: 4, "strip-short": 5, micro: 6 }[form];
  }
  // Each small layout's size as designed for `count` readings, how much it stretches, the least it is scaled to, and
  // the room it keeps round what it lays out, where a chart by a rounded corner keeps clear of it (forms.rs).
  function small(form, count) {
    var n = Math.max(1, count), t = /^tiles-(\d)x(\d)$/.exec(form);
    if (t) { var a = +t[1], d = +t[2]; return { size: [a * TILE[0] + 2 * TILES_PAD, d * TILE[1] + 2 * TILES_PAD], stretch: [36 * a, 24 * d], floor: 0.9 }; }
    return {
      corner: { size: [300, 176], stretch: [200, 170], floor: 0.85, pad: [14, true] },
      "corner-tall": { size: [240, 28 + 110 + 8 + 2 * TALL_CHIPS + 2 * TALL_PAIR], stretch: [120, 220], floor: 0.85, pad: [14, true] },
      strip: { size: [24 + 84 + n * 96 + 90, STRIP], stretch: [260, 10], floor: 0.76, pad: [12, false] },
      "strip-short": { size: [24 + n * 70, STRIP], stretch: [120, 10], floor: 0.76, pad: [12, false] },
      rail: { size: [RAIL, 120 + n * RAIL_ROW], stretch: [96, 160], floor: 0.76, pad: [10, true] },
      micro: { size: [16 + 34 * Math.min(n, 3), 38], stretch: [30, 6], floor: 0.9 }
    }[form];
  }
  function Ladder(T, R) {
    var rungs = [], lanes = { full: lanesOf(R, true), compact: lanesOf(R, false) };
    ["full", "compact"].forEach(function (detail) {
      var hs = lanes[detail].map(function (l) { return laneHeight(T, l); });
      for (var columns = Math.min(MAX_COLUMNS, hs.length); columns >= 1; columns--) {
        var L = Layout(T, hs, columns);
        rungs.push({ form: detail + "-" + columns, size: [widthOf(L), heightOf(L)], stretch: [120 * columns, 60], floor: LANES_FLOOR, layout: L, detail: detail });
      }
    });
    var count = readingsOf(R).length, tiles = [3, 2].map(function (a) { return "tiles-" + a + "x" + Math.ceil(count / a); });
    tiles.concat(["corner", "corner-tall", "strip", "strip-short", "rail", "micro"]).forEach(function (form) {
      var s = small(form, count), size = s.size.slice();
      // Grown by how much further in than designed it is laid, clear of the look's corners (forms.rs `size_within`).
      if (s.pad) { var grown = inside(T.radius, s.pad[0]) - s.pad[0]; size[0] += 2 * grown; if (s.pad[1]) size[1] += grown; }
      rungs.push({ form: form, size: size, stretch: s.stretch, floor: s.floor, small: true });
    });
    var rung = function (form) { return rungs.filter(function (r) { return r.form === form; })[0]; };
    // `rung` at `scale`, stretched as far as it may into `room`.
    var fill = function (r, room, scale) {
      return { form: r.form, scale: scale, extra: [Math.min(Math.max(room[0] / scale - r.size[0], 0), r.stretch[0]), Math.min(Math.max(room[1] / scale - r.size[1], 0), r.stretch[1])] };
    };
    var L = {
      rungs: rungs, rung: rung, fill: fill,
      drawn: function (c) { var r = rung(c.form); return [(r.size[0] + c.extra[0]) * c.scale, (r.size[1] + c.extra[1]) * c.scale]; },
      stretched: function (c) { var r = rung(c.form); return [r.size[0] + c.extra[0], r.size[1] + c.extra[1]]; },
      // What shows in `room`: of the forms that fit it at their least scale or larger, those of the level showing the
      // most; of these, the one covering the most of it, though the one shown stays unless another covers clearly
      // more. A richer level than the one shown counts only where it fits at `UP` times its least.
      choose: function (room, shown) {
        var area = Math.max(room[0] * room[1], 1), fitting = [];
        rungs.forEach(function (r) {
          var fit = Math.min(room[0] / r.size[0], room[1] / r.size[1]);
          var richer = shown && levelOf(r.form) < levelOf(shown.form);
          if (fit >= r.floor * (richer ? UP : 1)) {
            var c = fill(r, room, Math.min(fit, MOST_SCALE)), d = L.drawn(c);
            fitting.push([c, d[0] * d[1] / area]);
          }
        });
        if (!fitting.length) { var least = rungs[rungs.length - 1]; return { form: least.form, scale: least.floor, extra: [0, 0] }; }
        var level = Math.min.apply(null, fitting.map(function (f) { return levelOf(f[0].form); }));
        var mine = fitting.filter(function (f) { return levelOf(f[0].form) === level; });
        var best = mine.reduce(function (b, f) { return b && b[1] >= f[1] ? b : f; }, null);
        var kept = shown && mine.filter(function (f) { return f[0].form === shown.form; })[0];
        return kept && best[1] < kept[1] * STICK ? kept[0] : best[0];
      },
      // `c` held to its form's measures here; a form not offered gives way to the first.
      keep: function (c) {
        var r = rung(c.form) || rungs[0];
        return { form: r.form, scale: Math.min(Math.max(c.scale, r.floor), MOST_SCALE), extra: [Math.min(Math.max(c.extra[0], 0), r.stretch[0]), Math.min(Math.max(c.extra[1], 0), r.stretch[1])] };
      }
    };
    return L;
  }

  // ---------------- the ground each look gives it (skins.rs) ----------------
  function roundRect(c, x, y, w, h, radius) { c.roundRect(x, y, w, h, Math.max(0, Math.min(radius, w / 2, h / 2))); }
  function grow(b, by) { return { x: b.x - by, y: b.y - by, w: b.w + 2 * by, h: b.h + 2 * by }; }
  // Box shadows cast by `pieces`, only outside each, as CSS casts a box's (skins.rs `shadows_outside`). (The canvas
  // draws a shadow in device pixels: `k` of them a DIP here.)
  function shadows(c, pieces, specs, radius, k) {
    var far = 1e5;
    pieces.forEach(function (p) {
      c.save();
      c.beginPath(); c.rect(-far, -far, 2 * far, 2 * far); roundRect(c, p.x, p.y, p.w, p.h, radius); c.clip("evenodd");
      specs.forEach(function (s) {
        var g = grow(p, s.spread);
        // (The shape itself well away, only its shadow where it falls.)
        c.shadowColor = css(s.color); c.shadowBlur = s.blur * k; c.shadowOffsetX = far * k; c.shadowOffsetY = s.y * k;
        c.fillStyle = "#000";
        c.beginPath(); roundRect(c, g.x - far, g.y, g.w, g.h, radius + s.spread); c.fill();
      });
      c.restore();
    });
  }
  // A shadow cast inward from a piece's edge, as CSS's `inset` box shadow.
  function insetShadow(c, p, radius, y, blur, color, k) {
    c.save();
    c.beginPath(); roundRect(c, p.x, p.y, p.w, p.h, radius); c.clip();
    c.shadowColor = css(color); c.shadowBlur = blur * k; c.shadowOffsetY = y * k;
    c.fillStyle = "#000";
    c.beginPath(); c.rect(p.x - 100, p.y - 100, p.w + 200, p.h + 200); roundRect(c, p.x, p.y, p.w, p.h, radius); c.fill("evenodd");
    c.restore();
  }
  // Light along a piece of glass's edge, brightest at its top left, again faintly at its bottom right.
  function rim(c, p, radius) {
    var length = (p.w + p.h) * Math.SQRT1_2, cx = p.x + p.w / 2, cy = p.y + p.h / 2, step = length / 2 * Math.SQRT1_2, RIM_WIDTH = 1.25;
    var g = c.createLinearGradient(cx - step, cy - step, cx + step, cy + step);
    [[0, 0.75], [0.22, 0.2], [0.5, 0.05], [0.78, 0.08], [1, 0.22]].forEach(function (s) { g.addColorStop(s[0], "rgba(255,255,255," + s[1] + ")"); });
    c.fillStyle = g;
    c.beginPath(); roundRect(c, p.x, p.y, p.w, p.h, radius); roundRect(c, p.x + RIM_WIDTH, p.y + RIM_WIDTH, p.w - 2 * RIM_WIDTH, p.h - 2 * RIM_WIDTH, radius - RIM_WIDTH);
    c.fill("evenodd");
  }
  function fillRound(c, color, b, radius) { c.fillStyle = css(color); c.beginPath(); roundRect(c, b.x, b.y, b.w, b.h, radius); c.fill(); }
  // What of the desktop `behind` it is within `area` (the desktop's picture, its corner and size from its own corner),
  // drawn where it is.
  function backdrop(c, b, area) {
    var iw = b.image.naturalWidth || b.image.width, ih = b.image.naturalHeight || b.image.height;
    var x0 = Math.max(area.x, b.x), y0 = Math.max(area.y, b.y), x1 = Math.min(area.x + area.w, b.x + b.w), y1 = Math.min(area.y + area.h, b.y + b.h);
    if (x1 <= x0 || y1 <= y0) return;
    c.drawImage(b.image, (x0 - b.x) / b.w * iw, (y0 - b.y) / b.h * ih, (x1 - x0) / b.w * iw, (y1 - y0) / b.h * ih, x0, y0, x1 - x0, y1 - y0);
  }
  // Windows 11's acrylic: the desktop behind the sheet blurred wide and a little more saturated (skins.rs `acrylic`),
  // made at an eighth of its pixels (blurred that far, it is the same). `k`: pixels a DIP.
  var ACRYLIC_BLUR = 30, ACRYLIC_SATURATION = 1.25, FROST_BLUR = 9, GLASS_SATURATION = 1.3;
  var made = document.createElement("canvas");
  function acrylic(c, b, sheet, k) {
    var px = k / 8, area = grow(sheet, 3 * ACRYLIC_BLUR), w = Math.max(1, Math.ceil(area.w * px)), h = Math.max(1, Math.ceil(area.h * px));
    made.width = w; made.height = h;
    var o = made.getContext("2d");
    o.setTransform(px, 0, 0, px, -area.x * px, -area.y * px);
    o.filter = "blur(" + ACRYLIC_BLUR * px + "px) saturate(" + ACRYLIC_SATURATION + ")";
    backdrop(o, b, area);
    c.drawImage(made, area.x, area.y, w / px, h / px);
  }
  // Its ground `size` large (DIPs), its lanes' and bar's boxes in it, or none (a small layout, one piece); `behind`,
  // the desktop under it, where the look sees it.
  function ground(c, T, size, pieces, k, behind) {
    var whole = { x: 0, y: 0, w: size[0], h: size[1] };
    if (T.skin === "paper") {
      // One slab, free all round off the screen's edge, its rule just outside it.
      shadows(c, [whole], T.shadows, T.radius, k);
      fillRound(c, T.stroke, grow(whole, 1), T.radius + 1);
      fillRound(c, T.paper, whole, T.radius);
    } else if (T.skin === "fluent") {
      // One acrylic sheet, its hairline round it: the desktop blurred, a luminosity layer that evens out how bright it
      // is (its hue and saturation, the layer's lightness), a thin tint. With no picture of the desktop, the layer
      // alone, as over a backdrop of its own colour.
      shadows(c, [whole], T.shadows, T.radius, k);
      fillRound(c, T.stroke, grow(whole, 1), T.radius + 1);
      fillRound(c, Color(T.luminosity.rgb, 1), whole, T.radius);
      if (behind) {
        c.save();
        c.beginPath(); roundRect(c, 0, 0, whole.w, whole.h, T.radius); c.clip();
        acrylic(c, behind, whole, k);
        c.globalCompositeOperation = "luminosity";
        c.globalAlpha = T.luminosity.a;
        c.fillStyle = css(Color(T.luminosity.rgb, 1)); c.fillRect(0, 0, whole.w, whole.h);
        c.restore();
      }
      fillRound(c, T.tint, whole, T.radius);
    } else {
      // A piece of glass for each lane and the bar: the desktop through it, frosted as much as it needs and a little
      // more saturated, its tint, light just inside its edge, its rim.
      var list = pieces || [whole];
      shadows(c, list, T.shadows, T.radius, k);
      if (behind) {
        c.save();
        c.beginPath(); list.forEach(function (p) { roundRect(c, p.x, p.y, p.w, p.h, T.radius); }); c.clip();
        c.filter = "blur(" + FROST * FROST_BLUR * k + "px) saturate(" + GLASS_SATURATION + ")";
        backdrop(c, behind, grow(whole, 3 * FROST * FROST_BLUR));
        c.restore();
      }
      list.forEach(function (p) {
        fillRound(c, alpha(T.glass, T.glassClear + T.glassFrosted * FROST), p, T.radius);
        insetShadow(c, p, T.radius, 1, 2, Color(0xFFFFFF, 0.5), k);
        insetShadow(c, p, T.radius, -1, 2, Color(0xFFFFFF, 0.12), k);
        insetShadow(c, p, T.radius, 0, 18, Color(0xFFFFFF, 0.07), k);
        rim(c, p, T.radius);
      });
    }
  }

  // ---------------- drawing what was recorded ----------------
  function icon(c, kind, cx, cy, size, color) {
    c.save();
    c.translate(cx - size / 2, cy - size / 2);
    c.scale(size / 24, size / 24);
    c.strokeStyle = css(color); c.lineWidth = 2; c.lineCap = "round"; c.lineJoin = "round";
    PATHS[kind].forEach(function (p) { c.stroke(p); });
    c.restore();
  }
  function polyline(c, pts) { c.beginPath(); pts.forEach(function (p, i) { if (i) c.lineTo(p[0], p[1]); else c.moveTo(p[0], p[1]); }); }
  // A lane's chart: the rules travelling with the readings (one each ten), the wash under the trace fading as the
  // look's does, the line, red above the load it runs hot at, and the pen's tip at its end.
  function drawPlot(c, e) {
    var x = e.x, y = e.y, w = e.w, h = e.h, k = e.k, bottom = y + h, ink = e.ink, n = e.series[0].length;
    var px = function (i) { return x + w * i / (n - 1); }, py = function (v) { return bottom - Math.min(1, Math.max(0, v / e.max)) * (h - 2 * k); };
    c.fillStyle = css(e.rule);
    for (var i = 0; i < n; i++) if ((e.steps - n + i) % 10 === 0) c.fillRect(px(i), y, k, h);
    c.fillRect(x, bottom, w, k);
    c.save();
    c.beginPath(); c.rect(x, y - 2 * k, w, h + 2 * k); c.clip();
    e.series.forEach(function (s, order) {
      var pts = s.map(function (v, i) { return [px(i), py(v)]; });
      if (order === 0) {
        polyline(c, pts); c.lineTo(x + w, bottom); c.lineTo(x, bottom); c.closePath();
        if (ink.wash.a === ink.washEnd.a) c.fillStyle = css(ink.wash);
        else { var g = c.createLinearGradient(0, y, 0, bottom); g.addColorStop(0, css(ink.wash)); g.addColorStop(1, css(ink.washEnd)); c.fillStyle = g; }
        c.fill();
      }
      var width = (order ? 1 : 1.5) * k;
      c.lineWidth = width; c.lineJoin = "round"; c.lineCap = "butt";
      c.strokeStyle = css(order ? ink.trace2 : ink.trace); polyline(c, pts); c.stroke();
      if (order === 0 && e.hotAt != null) {
        c.save(); c.beginPath(); c.rect(x, y - 2 * k, w, py(e.hotAt) - y + 2 * k); c.clip();
        c.strokeStyle = css(e.signal); polyline(c, pts); c.stroke(); c.restore();
      }
    });
    c.restore();
    var last = e.series[0][n - 1];
    c.fillStyle = css(e.hotAt != null && last > e.hotAt ? e.signal : ink.trace);
    c.beginPath(); c.arc(x + w, py(last), 2.5 * k, 0, TAU); c.fill();
  }
  // A small layout's chart: ruled in four, each line its last samples at about three DIPs a sample.
  function drawChart(c, e) {
    var x = e.x, y = e.y, w = e.w, h = e.h, k = e.k;
    c.fillStyle = css(e.rule);
    for (var i = 0; i <= 4; i++) c.fillRect(x + w * i / 4, y, k, h);
    e.lines.forEach(function (l) {
      var room = Math.max(Math.floor(w / k / 3), 20), s = l.series.slice(-room), step = w / Math.max(1, s.length - 1);
      var pts = s.map(function (v, j) { return [x + j * step, y + h - h * Math.min(1, Math.max(0, v / l.most))]; });
      if (pts.length < 2) return;
      if (l.fill) { polyline(c, pts); c.lineTo(pts[pts.length - 1][0], y + h); c.lineTo(pts[0][0], y + h); c.closePath(); c.fillStyle = css(l.fill); c.fill(); }
      c.lineWidth = l.width * k; c.lineJoin = "round"; c.lineCap = "butt";
      c.setLineDash(l.dashed ? [2 * l.width * k, 2 * l.width * k] : []);
      c.strokeStyle = css(l.color); polyline(c, pts); c.stroke();
      if (l.hotAt != null && l.hotAt < l.most) {
        c.save(); c.beginPath(); c.rect(x, y - 2 * k, w, h - h * l.hotAt / l.most + 2 * k); c.clip();
        c.strokeStyle = css(e.signal); polyline(c, pts); c.stroke(); c.restore();
      }
      c.setLineDash([]);
    });
    if (!e.legend) return;
    // Which line is which, on a plate of their own, where there is room.
    var f = e.small, pad = 5, sample = 12, gap = 4, apart = 9;
    var names = e.legend.map(function (l) { return fitted(l[0], f, LEGEND_NAME); }), widths = names.map(function (n) { return measure(n, f); });
    var width = widths[0] + widths[1] + 2 * (sample + gap) + apart + 2 * pad, height = f.size + 2 * pad - 2;
    if (width * k > w - 8 * k) return;
    var bx = x + 4 * k, by = y + 4 * k, middle = by + height * k / 2, at = bx + pad * k;
    c.fillStyle = "rgba(0,0,0,0.35)"; c.beginPath(); roundRect(c, bx, by, width * k, height * k, height * k / 2); c.fill();
    e.legend.forEach(function (l, i) {
      c.setLineDash(l[2] ? [2.4 * k, 2.4 * k] : []); c.lineWidth = (l[2] ? 1.2 : 1.5) * k; c.strokeStyle = css(l[1]);
      c.beginPath(); c.moveTo(at, middle); c.lineTo(at + sample * k, middle); c.stroke(); c.setLineDash([]);
      at += (sample + gap) * k;
      var fk = sized(f, f.size * k, f.weight), ink = inkOf(names[i], fk);
      face(c, fk); c.fillStyle = css(e.text2); c.fillText(names[i], at, middle - (ink[0] + ink[1]) / 2 + baseline(fk)[0]);
      at += (widths[i] + apart) * k;
    });
  }
  function drawEls(c, els) {
    els.forEach(function (e) {
      var a = e.alpha == null ? 1 : e.alpha;
      if (a <= 0.003) return;
      c.globalAlpha = a;
      switch (e.t) {
        case "text":
          face(c, e.f); c.fillStyle = css(e.color); c.textAlign = "left"; c.textBaseline = "alphabetic"; c.fillText(e.text, e.x, e.y);
          break;
        case "fill":
          c.fillStyle = css(e.color);
          if (e.r) { c.beginPath(); roundRect(c, e.x, e.y, e.w, e.h, e.r); c.fill(); } else c.fillRect(e.x, e.y, e.w, e.h);
          break;
        case "meter":
          c.fillStyle = css(e.track); c.beginPath(); roundRect(c, e.x, e.y, e.w, e.h, e.r); c.fill();
          var f = Math.min(1, Math.max(0, e.frac));
          if (f > 0) {
            c.save(); c.beginPath();
            if (e.up) c.rect(e.x, e.y + e.h * (1 - f), e.w, e.h * f); else c.rect(e.x, e.y, e.w * f, e.h);
            c.clip();
            c.fillStyle = css(e.color); c.beginPath(); roundRect(c, e.x, e.y, e.w, e.h, e.r); c.fill();
            c.restore();
          }
          break;
        case "ring": {
          // (Round from the top, as far as its share; with none, a disc.)
          if (e.share != null) {
            var lw = Math.max(e.r * 0.16, 1.5 * e.k);
            c.lineWidth = lw; c.lineCap = "butt";
            c.strokeStyle = css(e.track); c.beginPath(); c.arc(e.x, e.y, e.r, 0, TAU); c.stroke();
            var s = Math.min(1, Math.max(0, e.share));
            if (s > 0) { c.strokeStyle = css(e.color); c.beginPath(); c.arc(e.x, e.y, e.r, -Math.PI / 2, -Math.PI / 2 + TAU * s); c.stroke(); }
          } else { c.fillStyle = css(e.track); c.beginPath(); c.arc(e.x, e.y, e.r * 1.1, 0, TAU); c.fill(); }
          icon(c, e.icon, e.x, e.y, e.r * 1.3, e.color);
          break;
        }
        case "plot": drawPlot(c, e); break;
        case "chart": drawChart(c, e); break;
      }
    });
    c.globalAlpha = 1;
  }

  // ---------------- turning from one form into the next (morph.rs) ----------------
  function lerp(a, b, t) { return a + (b - a) * t; }
  // An ease that starts at once and settles gently (morph.rs `ease`).
  function ease(t) { return 1 - Math.pow(1 - Math.min(1, Math.max(0, t)), 4); }
  var MOVED = ["x", "y", "w", "h", "r", "k"];
  function between(from, to, t) {
    var mine = {};
    from.forEach(function (e) { if (e.id) mine[e.id] = e; });
    var out = [], used = {};
    to.forEach(function (b) {
      var a = b.id && mine[b.id];
      if (!a) { out.push(Object.assign({}, b, { alpha: t })); return; }
      used[b.id] = true;
      var m = Object.assign({}, b);
      MOVED.forEach(function (key) { if (a[key] != null && b[key] != null) m[key] = lerp(a[key], b[key], t); });
      if (b.t === "text") {
        m.f = sized(b.f, lerp(a.f.size, b.f.size, t), b.f.weight);
        // Other words or another face: the one fading out as the other fades in, on their way.
        if (a.text !== b.text || a.f.family !== b.f.family || a.f.weight !== b.f.weight || a.f.width !== b.f.width) {
          out.push(Object.assign({}, a, { x: m.x, y: m.y, f: sized(a.f, m.f.size, a.f.weight), alpha: 1 - t }));
          m.alpha = t;
        }
      }
      out.push(m);
    });
    from.forEach(function (a) { if (!a.id || !used[a.id]) out.push(Object.assign({}, a, { alpha: (a.alpha == null ? 1 : a.alpha) * (1 - t) })); });
    return out;
  }

  // ---------------- the widget ----------------
  GD.readings = function (lang) {
    var look = null, T = null, ladder = null;
    // What it shows (a form, at a scale, stretched), the form shown and the room it was taken in, its zoom and size,
    // the sides its content keeps to as it is sized, and what it drew last, and is turning from.
    var choice = null, taken = null, zoom = 1, size = [0, 0], keeps = { left: false, top: false };
    var last = [], turning = null, drawnAt = null;

    // The look on the page, as it is now (it is changed live).
    function looked() {
      var name = root.getAttribute("data-look") || "glass";
      if (name === look) return;
      look = name;
      T = themed(name);
      ladder = Ladder(T, latest());
      // What it showed, held to this look's measures (arrange.rs `Want::Kept`).
      if (choice) opened(ladder.keep(choice));
    }
    // Laid out as `kept` within the screen it is on, the zoom that holds it there (arrange.rs `Opening::new`): too
    // large for the screen's room, what best fills that room.
    function opened(kept) {
      var room = [WORK[0] - GAP - T.inset, WORK[1] - 2 * GAP], d = ladder.drawn(kept);
      choice = d[0] <= room[0] + 0.5 && d[1] <= room[1] + 0.5 ? kept : ladder.choose(room, { form: kept.form, taken: room });
      var s = ladder.stretched(choice);
      zoom = choice.scale * Math.min(room[0] / (s[0] * choice.scale), room[1] / (s[1] * choice.scale), 1);
      size = s;
    }
    // What it shows, recorded at its zoom, its corner at `at` (DIPs on the screen).
    function record(R, at) {
      var r = Rec(T, zoom, at), rung = ladder.rung(choice.form);
      if (rung.small) {
        var P = Painter(r, R, readingsOf(R)), w = size[0], h = size[1], t = /^tiles-(\d)x(\d)$/.exec(choice.form);
        if (t) P.tiles(+t[1], +t[2], w, h);
        else ({ corner: function () { P.corner(w, h); }, "corner-tall": function () { P.tall(w, h); }, strip: function () { P.strip(true, w, h); },
          "strip-short": function () { P.strip(false, w, h); }, rail: function () { P.rail(w, h); }, micro: function () { P.micro(w, h); } })[choice.form]();
      } else paintLanes(r, R, stretched(rung.layout, choice.extra), lanesOf(R, rung.detail === "full"));
      return r.els;
    }
    // Its buttons' centres, off its edge on `rack`'s side, from its corner (widget.rs `buttons`).
    function buttons(w, h, rack) {
      var r = BUTTON / 2, step = BUTTON + BUTTON_GAP;
      return RACK.map(function (name, place) {
        if (rack === "above" || rack === "under") return [w - r - (RACK.length - 1 - place) * step, rack === "under" ? h + BUTTON_INSET + r : -BUTTON_INSET - r, name];
        return [rack === "right" ? w + BUTTON_INSET + r : -BUTTON_INSET - r, r + place * step, name];
      });
    }
    function scaleOf(c) { var m = c.getTransform(); return Math.hypot(m.a, m.b); }

    looked();
    var api = {
      // It was just torn off a panel `w` × `h` (DIPs) showing `columns` columns: all of each lane in as many, at that
      // size (the panel's lanes stretched into it, as a form fills its room).
      torn: function (w, h, columns) {
        looked();
        var rung = ladder.rung("full-" + Math.max(1, Math.min(columns, MAX_COLUMNS)));
        var fit = Math.min(w / rung.size[0], h / rung.size[1]);
        opened(ladder.keep(ladder.fill(rung, [w, h], Math.min(fit, MOST_SCALE))));
        taken = { form: choice.form, taken: [w, h] };
        turning = null;
        last = [];
        keeps = { left: false, top: false };
        return api.rest();
      },
      // The hand gives it `w` × `h` (DIPs): what fills that room (arrange.rs `Ladder::choose`, the form shown kept as
      // the hand goes to and fro), and the size its surface is drawn at, past what that form fills following the hand
      // only part of the way. `keep`: the sides its content keeps to, those not dragged ({ left, top }).
      room: function (w, h, now, keep) {
        looked();
        if (keep) keeps = { left: !!keep.left, top: !!keep.top };
        var shown = taken && taken.form === choice.form ? taken : { form: choice.form, taken: [w, h] };
        var was = choice.form;
        opened(ladder.choose([w, h], shown));
        taken = taken && taken.form === choice.form ? taken : { form: choice.form, taken: [w, h] };
        // Another form: what showed turns into it, from where it is now if it was still turning.
        if (choice.form !== was) turning = { at: now, from: last };
        var give = function (have, fill) { return have <= fill ? fill : fill + (have - fill) * GIVE; };
        return { w: give(w / zoom, size[0]) * zoom, h: give(h / zoom, size[1]) * zoom };
      },
      // The size it settles to let go: what its form fills.
      rest: function () { looked(); return { w: size[0] * zoom, h: size[1] * zoom }; },
      // Drawn in `c`, its box's corner at the origin, `w` × `h` (DIPs): its ground, what it shows, and with the pointer
      // over it, its buttons in their row on `rack`'s side ("above", "under", "left" or "right"). `behind`, where the
      // look sees the desktop through it (Windows 11, glass): `{ image, x, y, w, h }`, the desktop's picture drawn
      // `w` × `h` with its corner at (`x`, `y`) from the box's (DIPs), as Glance's ground is given what it captured.
      draw: function (c, w, h, now, pointer, rack, behind) {
        looked();
        var R = latest(), surface = [w / zoom, h / zoom];
        // What it shows keeps to the sides not dragged.
        var off = [keeps.left ? w - size[0] * zoom : 0, keeps.top ? h - size[1] * zoom : 0];
        var els = record(R, off);
        if (turning) {
          var t = (now - turning.at) / MORPH;
          if (t >= 1) turning = null; else els = between(turning.from, els, ease(t));
        }
        last = els;
        var k = scaleOf(c);
        c.save();
        c.scale(zoom, zoom);
        // A small layout is one piece; lanes, a piece each and the bar.
        var rung = ladder.rung(choice.form), pieces = null;
        if (!rung.small) {
          var L = stretched(rung.layout, choice.extra), o = [off[0] / zoom, off[1] / zoom];
          pieces = boxes(L).concat([barOf(L)]).map(function (b) { return { x: b.x + o[0], y: b.y + o[1], w: b.w, h: b.h }; });
        }
        var seen = behind && { image: behind.image, x: behind.x / zoom, y: behind.y / zoom, w: behind.w / zoom, h: behind.h / zoom };
        ground(c, T, surface, pieces, k * zoom, seen);
        c.restore();
        // On glass, a faint halo round what it shows keeps it legible over anything (render.rs, gfx.rs `glow`).
        c.save();
        if (T.legibility) { c.shadowColor = css(T.legibility); c.shadowBlur = k; }
        drawEls(c, els);
        c.restore();
        drawnAt = { w: w, h: h, rack: pointer && rack ? rack : null };
        if (!drawnAt.rack) return;
        // Its buttons, each on a plate of its own, dark on the dark (buttons.rs `paint`).
        c.save();
        c.scale(zoom, zoom);
        buttons(surface[0], surface[1], rack).forEach(function (b) {
          c.fillStyle = "rgba(30,30,30,0.94)"; c.beginPath(); c.arc(b[0], b[1], BUTTON / 2, 0, TAU); c.fill();
          c.strokeStyle = css(alpha(T.text3, 0.5)); c.lineWidth = 1; c.beginPath(); c.arc(b[0], b[1], (BUTTON - 1) / 2, 0, TAU); c.stroke();
          icon(c, b[2], b[0], b[1], 13, Color(0xFFFFFF, 1));
        });
        c.restore();
      },
      // Which of its buttons is at (`x`, `y`) from its box's corner, as it was last drawn `w` × `h`; or null.
      hit: function (w, h, x, y) {
        if (!drawnAt || !drawnAt.rack || drawnAt.w !== w || drawnAt.h !== h) return null;
        var found = buttons(w / zoom, h / zoom, drawnAt.rack).filter(function (b) { return Math.hypot(b[0] - x / zoom, b[1] - y / zoom) <= BUTTON / 2 + 2; })[0];
        return found ? found[2] : null;
      }
    };
    // The room round its box its canvas needs (each side): its shadow's, or its buttons' row's, the larger; how far
    // the row reaches past its edge, and how long it is; the least it is sized down to. (In DIPs on the screen, at
    // its zoom, as its window has them.)
    Object.defineProperties(api, {
      margin: { get: function () { looked(); return Math.ceil(Math.max(T.margin, REACH) * zoom); } },
      reach: { get: function () { return REACH * zoom; } },
      row: { get: function () { return ROW * zoom; } },
      // The form it shows, by the name Glance keeps it under ("full-2", "tiles-3x2", "rail", ...).
      form: { get: function () { return choice.form; } },
      least: { get: function () { looked(); var m = ladder.rungs[ladder.rungs.length - 1]; return [m.size[0] * m.floor, m.size[1] * m.floor]; } }
    });
    // (Shown first as the panel it is torn from shows itself, two columns, until it is torn.)
    api.torn(720, 480, 2);
    return api;
  };
})();
