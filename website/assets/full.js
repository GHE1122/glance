// The music full screen, as Glance draws it (src/fullscreen.rs, src/ui/smoke.rs):
// over the whole of a screen, the smoke flowing in the cover's colours (noise
// warped by noise, by a time of its own, faster as the music swells), and the
// line sung now large at the left, each character coming up out of a blur as
// it is sung, lighting and swelling with the music; the next line small under
// it. Drawn into a host the page gives it, as large as the host is.
//
//   var f = GlanceDesk.full(host, song, cover, lang);
//   f.frame(t, now)   song time t (s), now = performance.now() (ms)
//   f.resize()        the host's size changed
//   f.destroy()
(function () {
  "use strict";

  window.GlanceDesk = window.GlanceDesk || {};

  // The smoke's colours with no cover, as it begins.
  var TINT = [0.40, 0.95, 0.88], TINT2 = [0.84, 0.97, 0.92];
  // How long the line before takes to leave as the next comes (s).
  var LEAVING = 0.7;
  // Each character's size and tilt as it stands in its line.
  var SIZES = [1.15, 0.78, 1.0, 1.32, 0.86, 1.08, 0.72, 1.22];
  var TILTS = [-0.09, 0.06, -0.03, 0.08, -0.07, 0.04, -0.05, 0.07];
  // The least share of its size a line is drawn at to fit in one row, else
  // more rows, at most so many; how far apart rows are, of their size.
  var LEAST_FIT = 0.6, MOST_ROWS = 3, ROW_STEP = 1.3;
  // How early a line is shown (s).
  var EARLY = 0.2;
  // How many of the screen's pixels the smoke is drawn with, each way.
  var SCALE = 0.5;
  // The side a cover is squared to before its colours are read.
  var SIDE = 192;

  // ---------------- the serif face ----------------
  // Noto Serif SC Black, cut to the song's characters (studio/site_serif.py);
  // the lines are in YaHei till it is had, as Glance's are.
  var SERIF = '"Glance Serif"', YAHEI = '"Microsoft YaHei UI", "Microsoft YaHei", sans-serif';
  var script = document.currentScript;
  var face = typeof FontFace === "function" && script
    ? new FontFace("Glance Serif", "url(" + new URL("fonts/serif-song.woff", script.src).href + ")", { weight: "100 900" }) : null;
  if (face) document.fonts.add(face);
  function family() { return face && face.status === "loaded" ? SERIF : YAHEI; }
  function want() { if (face && face.status === "unloaded") face.load().catch(function () {}); }
  function font(size, f) { return "700 " + size + "px " + f; }

  // ---------------- the smoke ----------------
  var VERTEX = "attribute vec2 at; void main() { gl_Position = vec4(at, 0.0, 1.0); }";
  var PIXEL = [
    "precision highp float;",
    "uniform float flow;   // its own time: how far it has flowed",
    "uniform float swell;  // the music's slow-following swell (0 to 1)",
    "uniform float aspect; // width over height",
    "uniform float dots;   // the screen of dots' pitch, in its pixels",
    "uniform vec3 tint;    // the cover's colour",
    "uniform vec3 tint2;   // its second",
    "uniform vec2 size;    // its pixels",
    "float hash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }",
    "float noise(vec2 p) {",
    "  vec2 i = floor(p), f = fract(p), u = f * f * (3.0 - 2.0 * f);",
    "  return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x), u.y);",
    "}",
    "float fbm(vec2 p) {",
    "  float v = 0.0, a = 0.5;",
    "  for (int k = 0; k < 6; k++) { v += a * noise(p); p = p * 2.03 + 17.1; a *= 0.5; }",
    "  return v;",
    "}",
    // (smoothstep with its edges the wrong way round, as HLSL takes it.)
    "float smooth(float e0, float e1, float x) { float t = clamp((x - e0) / (e1 - e0), 0.0, 1.0); return t * t * (3.0 - 2.0 * t); }",
    "void main() {",
    "  vec2 uv = gl_FragCoord.xy / size;",
    "  vec2 position = vec2(gl_FragCoord.x, size.y - gl_FragCoord.y);",
    "  vec2 p = vec2(uv.x * aspect, uv.y);",
    "  vec2 q = vec2(fbm(p * 1.6 + flow * 0.05), fbm(p * 1.6 + vec2(5.2, 1.3) - flow * 0.04));",
    "  vec2 r = vec2(fbm(p * 1.8 + 3.0 * q + vec2(1.7, 9.2) + flow * 0.09), fbm(p * 1.8 + 3.0 * q + vec2(8.3, 2.8) + flow * 0.07));",
    "  float f = fbm(p * 1.4 + 2.5 * r);",
    "  vec3 c = mix(tint * 0.1, tint * 0.5, smooth(0.25, 0.85, f));",
    "  c = mix(c, tint2 * 0.95, smooth(0.62, 1.0, f * f * 1.6) * 0.75);",
    "  c *= 0.8 + 0.35 * swell;",
    "  vec2 cell = fract(position / dots) - 0.5;",
    "  c += 0.035 * smooth(0.25, 0.0, length(cell));",
    "  c *= 1.0 - 0.45 * length(uv - 0.5);",
    "  gl_FragColor = vec4(c, 1.0);",
    "}"
  ].join("\n");

  // The shader made on `canvas`, or null where WebGL cannot be had.
  function Smoke(canvas) {
    var gl = canvas.getContext("webgl", { alpha: false, antialias: false, depth: false, stencil: false, premultipliedAlpha: false });
    if (!gl) return null;
    function shader(kind, source) {
      var s = gl.createShader(kind);
      gl.shaderSource(s, source);
      gl.compileShader(s);
      return gl.getShaderParameter(s, gl.COMPILE_STATUS) ? s : null;
    }
    var vs = shader(gl.VERTEX_SHADER, VERTEX), ps = shader(gl.FRAGMENT_SHADER, PIXEL);
    if (!vs || !ps) return null;
    var program = gl.createProgram();
    gl.attachShader(program, vs);
    gl.attachShader(program, ps);
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) return null;
    gl.useProgram(program);
    // One triangle covering the target.
    gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
    var at = gl.getAttribLocation(program, "at");
    gl.enableVertexAttribArray(at);
    gl.vertexAttribPointer(at, 2, gl.FLOAT, false, 0, 0);
    var u = {};
    ["flow", "swell", "aspect", "dots", "tint", "tint2", "size"].forEach(function (n) { u[n] = gl.getUniformLocation(program, n); });
    return {
      // Drawn as it stands: how far it has flowed, the swell, the two colours.
      render: function (flow, swell, tint, tint2) {
        var w = canvas.width, h = canvas.height;
        gl.viewport(0, 0, w, h);
        gl.uniform1f(u.flow, flow);
        gl.uniform1f(u.swell, swell);
        gl.uniform1f(u.aspect, w / h);
        gl.uniform1f(u.dots, 4 * SCALE);
        gl.uniform3fv(u.tint, tint);
        gl.uniform3fv(u.tint2, tint2);
        gl.uniform2f(u.size, w, h);
        gl.drawArrays(gl.TRIANGLES, 0, 3);
      },
      lose: function () { var lose = gl.getExtension("WEBGL_lose_context"); if (lose) lose.loseContext(); }
    };
  }

  // ---------------- the cover's colours (src/cover.rs) ----------------
  // A colour's hue (degrees), saturation and lightness (0 to 1).
  function hsl(c) {
    var r = c[0], g = c[1], b = c[2], most = Math.max(r, g, b), least = Math.min(r, g, b), l = (most + least) / 2;
    if (most - least < 1e-6) return [0, 0, l];
    var span = most - least, s = span / Math.max(1 - Math.abs(2 * l - 1), 1e-6), hue;
    if (most === r) hue = 60 * ((((g - b) / span) % 6 + 6) % 6);
    else if (most === g) hue = 60 * ((b - r) / span + 2);
    else hue = 60 * ((r - g) / span + 4);
    return [hue, Math.min(s, 1), l];
  }

  // A cover's main colours and how much of it each is: its pixels (four bits
  // a channel) split, box by box, along the widest range of the box that holds
  // the most range and weight, into `count` boxes, each by its weighted mean.
  function medianCut(px, count) {
    var counted = new Uint32Array(4096), i;
    for (i = 0; i < px.length; i += 4) counted[(px[i] >> 4) << 8 | (px[i + 1] >> 4) << 4 | px[i + 2] >> 4]++;
    var colours = [];
    for (i = 0; i < 4096; i++) if (counted[i]) colours.push([[i >> 8, i >> 4 & 15, i & 15], counted[i]]);
    function range(cs, ch) {
      var hi = 0, lo = 255;
      cs.forEach(function (c) { hi = Math.max(hi, c[0][ch]); lo = Math.min(lo, c[0][ch]); });
      return cs.length ? hi - lo : 0;
    }
    function weight(cs) { return cs.reduce(function (a, c) { return a + c[1]; }, 0); }
    function worth(cs) { return Math.max(range(cs, 0), range(cs, 1), range(cs, 2)) * Math.sqrt(weight(cs)); }
    var boxes = [colours];
    while (boxes.length < count) {
      // (The last of equals, as Rust's max_by takes.)
      var at = -1, best = -Infinity;
      boxes.forEach(function (b, k) { if (b.length > 1 && worth(b) >= best) { best = worth(b); at = k; } });
      if (at < 0) break;
      var split = boxes[at];
      boxes[at] = boxes[boxes.length - 1];
      boxes.pop();
      var channel = 0;
      for (var ch = 1; ch < 3; ch++) if (range(split, ch) >= range(split, channel)) channel = ch;
      split.sort(function (a, b) { return a[0][channel] - b[0][channel]; });
      var total = weight(split), soFar = 0, middle = 0;
      for (i = 0; i < split.length; i++) { soFar += split[i][1]; if (soFar * 2 >= total) { middle = i; break; } }
      middle = Math.min(Math.max(middle, 1), split.length - 1);
      boxes.push(split.slice(0, middle));
      boxes.push(split.slice(middle));
    }
    var all = Math.max(1, boxes.reduce(function (a, b) { return a + weight(b); }, 0));
    return boxes.map(function (cs) {
      var w = weight(cs);
      function mean(ch) { return cs.reduce(function (a, c) { return a + (c[0][ch] * 17 + 8) * c[1]; }, 0) / Math.max(w, 1) / 255; }
      return [[mean(0), mean(1), mean(2)], w / all];
    });
  }

  // The most telling of a cover's main colours (vivid, neither near black nor
  // near white): the smoke's colour, as Glance takes it.
  function baseOf(img) {
    var c = document.createElement("canvas");
    c.width = c.height = SIDE;
    var x = c.getContext("2d", { willReadFrequently: true });
    var iw = img.naturalWidth || img.width, ih = img.naturalHeight || img.height, side = Math.min(iw, ih);
    x.fillStyle = "#000";
    x.fillRect(0, 0, SIDE, SIDE);
    x.imageSmoothingQuality = "high";
    x.drawImage(img, (iw - side) / 2, (ih - side) / 2, side, side, 0, 0, SIDE, SIDE);
    function telling(rgb) { var s = hsl(rgb); return s[1] * (1 - Math.abs(s[2] - 0.5) * 1.2); }
    var base = [0.5, 0.5, 0.5], best = -Infinity;
    medianCut(x.getImageData(0, 0, SIDE, SIDE).data, 8).forEach(function (m) { if (telling(m[0]) >= best) { best = telling(m[0]); base = m[0]; } });
    return base;
  }

  // ---------------- what is heard (src/sound.rs, as studio.rs reads it) ----------------
  function bytes(b64) {
    var s = atob(b64), out = new Uint8Array(s.length);
    for (var i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
    return out;
  }
  function Heard(h) {
    var level = bytes(h.level), beat = bytes(h.beat), spectrum = bytes(h.spectrum), n = h.bands;
    // What is heard at `at` seconds (the last look before it: nothing before the first).
    return function (at) {
      var bands = new Float32Array(n);
      if (at < h.step) return { level: 0, bands: bands, beat: 0 };
      var i = Math.min(Math.floor(at / h.step) - 1, h.count - 1);
      for (var b = 0; b < n; b++) bands[b] = spectrum[i * n + b] / 255;
      return { level: level[i] / 255, bands: bands, beat: beat[i] / 255 };
    };
  }

  // The mean of the bands `from` to `to` of `of`.
  function part(from, to, of) { var s = 0; for (var i = from; i < to; i++) s += of[i]; return s / (to - from); }
  function ease(x) { x = Math.min(Math.max(x, 0), 1); return 1 - Math.pow(1 - x, 3); }

  // ---------------- the lines (src/lyrics.rs, src/fullscreen.rs) ----------------
  // The line sung at `position`: the last begun (or about to be).
  function sungAt(lines, position) {
    for (var i = lines.length - 1; i >= 0; i--) if (lines[i].at <= position + EARLY) return i;
    return null;
  }

  // Each character of `line` with when it is sung and for how long: by its
  // words' times (each word's characters sharing it evenly), else its time
  // shared evenly (to the next line, or four seconds on).
  function timed(line, next) {
    var out = [];
    if (line.words.length) {
      line.words.forEach(function (word) {
        var chars = Array.from(word[2]), sung = Math.max(1, Array.from(word[2].replace(/\s+$/, "")).length);
        var each = Math.max(0, word[1] - word[0]) / sung;
        chars.forEach(function (ch, k) { out.push([ch, word[0] + each * Math.min(k, sung - 1), each]); });
      });
      return out;
    }
    var chars = Array.from(line.text);
    var end = line.end != null ? line.end : next != null ? next : line.at + 4;
    var each = Math.max(0, end - line.at) * 0.85 / Math.max(1, chars.length);
    chars.forEach(function (ch, k) { out.push([ch, line.at + each * k, each]); });
    return out;
  }

  // Whether `ch` is of a script whose characters stand each on its own as
  // they are read (Chinese, Japanese, Korean, their marks and full-width forms).
  function isCjk(ch) {
    var c = ch.codePointAt(0);
    return (c >= 0x1100 && c <= 0x11FF) || (c >= 0x2E80 && c <= 0x2FDF) || (c >= 0x3000 && c <= 0x303F) || (c >= 0x3040 && c <= 0x30FF) ||
      (c >= 0x3100 && c <= 0x31FF) || (c >= 0x3400 && c <= 0x4DBF) || (c >= 0x4E00 && c <= 0x9FFF) || (c >= 0xAC00 && c <= 0xD7AF) ||
      (c >= 0xF900 && c <= 0xFAFF) || (c >= 0xFE30 && c <= 0xFE4F) || (c >= 0xFF00 && c <= 0xFFEF) || (c >= 0x20000 && c <= 0x2FFFF);
  }
  function blank(s) { return s.trim() === ""; }

  // A line's characters as they move together: each CJK character on its own,
  // of other scripts each word whole; spaces as they are.
  function units(chars) {
    var out = [], joining = false;
    chars.forEach(function (c) {
      var alone = /\s/.test(c[0]) || isCjk(c[0]), last = out[out.length - 1];
      if (last && joining && !alone) { last[0] += c[0]; last[2] += c[2]; }
      else out.push([c[0], c[1], c[2]]);
      joining = !alone;
    });
    return out;
  }

  // The fewest rows (at most MOST_ROWS) a line whose units take `rooms` is
  // drawn in, `wide` across, keeping it at least LEAST_FIT of its size; each
  // row as long as the others as near as may be; and how much smaller it is
  // drawn to fit.
  function rowsOf(rooms, blanks, wide) {
    function sum(from, to) { var s = 0; for (var i = from; i < to; i++) s += rooms[i]; return s; }
    function long(row) {
      var from = row[0], to = row[1];
      while (from < to && blanks[from]) from++;
      while (to > from && blanks[to - 1]) to--;
      return sum(from, to);
    }
    var total = sum(0, rooms.length), chosen = [[[0, rooms.length]], 1];
    for (var count = 1; count <= Math.min(MOST_ROWS, Math.max(rooms.length, 1)); count++) {
      // Each break where the room before it comes nearest to an even share.
      var rows = [], start = 0, before = 0;
      for (var row = 1; row < count; row++) {
        var share = total * row / count, end = start + 1;
        while (end < rooms.length - (count - row) && before + sum(start, end) + rooms[end] / 2 < share) end++;
        before += sum(start, end);
        rows.push([start, end]);
        start = end;
      }
      rows.push([start, rooms.length]);
      var longest = rows.reduce(function (a, r) { return Math.max(a, long(r)); }, 1);
      chosen = [rows, Math.min(wide / longest, 1)];
      if (chosen[1] >= LEAST_FIT) break;
    }
    return chosen;
  }

  // ---------------- the full screen ----------------
  GlanceDesk.full = function (host, song, cover, lang) {
    want();
    var lines = song.lines, heardAt = Heard(song.heard);
    // (Glance's full screen shows no translation under its lines: `lang` changes nothing drawn.)

    var ground = document.createElement("canvas"), words = document.createElement("canvas"), glowing = document.createElement("canvas");
    [ground, words].forEach(function (c) {
      c.style.cssText = "position:absolute;left:0;top:0;width:100%;height:100%;display:block;pointer-events:none";
      host.appendChild(c);
    });
    var smoke = Smoke(ground);
    var ctx = words.getContext("2d"), gctx = glowing.getContext("2d");
    // (Blurs as CSS blurs, where the canvas has filters: a shadow's blur is not
    // the deviation the standard says in every browser.)
    var FILTERS = typeof ctx.filter === "string";

    // The cover's colour and its lighter second, toward which the smoke's ease.
    var base = baseOf(cover), top = Math.max(0.01, base[0], base[1], base[2]);
    var towardTint = base.map(function (c) { return Math.min(c / top * 0.96, 1); });
    var towardTint2 = towardTint.map(function (c) { return c + (1 - c) * 0.55; });

    // What it shows as the music goes (fullscreen::Shown), its lines' motions
    // timed from when it opened, as Glance's are.
    var S = { smooth: new Float32Array(24), swell: 0, flow: 0, beat: 0, last: null, began: performance.now(),
      tint: TINT.slice(), tint2: TINT2.slice(), now: null, leaving: null };
    // The host's size (DIPs) and how many pixels a DIP is drawn with.
    var w = 1, h = 1, k = 1;

    function resize() {
      w = Math.max(1, host.clientWidth);
      h = Math.max(1, host.clientHeight);
      var rect = host.getBoundingClientRect(), dpr = window.devicePixelRatio || 1;
      var pw = Math.max(1, Math.round(rect.width * dpr)), ph = Math.max(1, Math.round(rect.height * dpr));
      k = pw / w;
      words.width = glowing.width = pw;
      words.height = glowing.height = ph;
      ground.width = Math.max(1, Math.floor(pw * SCALE));
      ground.height = Math.max(1, Math.floor(ph * SCALE));
    }

    // Followed to `now` (ms): what is heard, the colours, the line sung now.
    function follow(now, heard, sungNow) {
      var dt = S.last == null ? 0 : Math.min((now - S.last) / 1000, 0.1);
      S.last = now;
      var kk = 1 - Math.exp(-dt / 0.22);
      for (var i = 0; i < S.smooth.length; i++) S.smooth[i] += (heard.bands[i] - S.smooth[i]) * kk;
      S.swell += (part(0, 4, heard.bands) - S.swell) * (1 - Math.exp(-dt / 0.45));
      S.flow += dt * (0.55 + S.swell * 0.9);
      var beat = Math.min(Math.max(heard.beat, 0), 1);
      S.beat += (beat - S.beat) * (1 - Math.exp(-dt / (beat > S.beat ? 0.04 : 0.3)));
      var e = 1 - Math.exp(-dt / 0.6);
      for (i = 0; i < 3; i++) {
        S.tint[i] += (towardTint[i] - S.tint[i]) * e;
        S.tint2[i] += (towardTint2[i] - S.tint2[i]) * e;
      }
      if (sungNow !== S.now) S.leaving = S.now == null ? null : [S.now, now];
      S.now = sungNow;
    }

    function measure(text, size, f) { ctx.font = font(size, f); return ctx.measureText(text).width; }
    function rgba(r, g, b, a) { return "rgba(" + r * 255 + "," + g * 255 + "," + b * 255 + "," + a + ")"; }

    // A glow of `colour` ([r, g, b, a]) round what `paint` draws into the glow layer (which is
    // not itself drawn): its shape blurred by a deviation of `blur` DIPs.
    function glow(colour, blur, paint) {
      gctx.setTransform(1, 0, 0, 1, 0, 0);
      gctx.clearRect(0, 0, glowing.width, glowing.height);
      gctx.setTransform(k, 0, 0, k, 0, 0);
      paint(gctx);
      ctx.save();
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      if (FILTERS) {
        // Its shape in the colour, blurred as CSS blurs (by the deviation).
        gctx.setTransform(1, 0, 0, 1, 0, 0);
        gctx.globalCompositeOperation = "source-in";
        gctx.fillStyle = rgba(colour[0], colour[1], colour[2], 1);
        gctx.fillRect(0, 0, glowing.width, glowing.height);
        gctx.globalCompositeOperation = "source-over";
        ctx.filter = "blur(" + blur * k + "px)";
        ctx.globalAlpha = colour[3];
        ctx.drawImage(glowing, 0, 0);
      } else {
        // Its shadow alone (a deviation of half the shadow's blur): the layer
        // drawn a whole width off to the left, its shadow cast as far back.
        ctx.shadowColor = rgba(colour[0], colour[1], colour[2], colour[3]);
        ctx.shadowBlur = 2 * blur * k;
        ctx.shadowOffsetX = glowing.width;
        ctx.drawImage(glowing, -glowing.width, 0);
      }
      ctx.restore();
    }

    // One line, each unit its size and tilt along a slowly drifting wave,
    // coming up out of a blur as it is sung, lit and swelling with the music
    // while it is; `out` of the way left (rising and fading).
    function drawLine(line, next, which, position, t, music, out, left, baseline, base) {
      var chars = units(timed(line, next));
      if (chars.every(function (c) { return blank(c[0]); })) return;
      var f = family();
      var sizes = chars.map(function (c, i) { return base * SIZES[(i + which * 3) % 8]; });
      var blanks = chars.map(function (c) { return blank(c[0]); });
      var rooms = chars.map(function (c, i) { return blanks[i] ? sizes[i] * 0.4 : measure(c[0], sizes[i], f) + base * 0.08; });
      var laid = rowsOf(rooms, blanks, w - 2 * left), rows = laid[0], fit = laid[1];
      sizes = sizes.map(function (s) { return s * fit; });
      rooms = rooms.map(function (r) { return r * fit; });
      function rowOf(i) { for (var r = 0; r < rows.length; r++) if (i >= rows[r][0] && i < rows[r][1]) return r; return 0; }
      function rowBaseline(r) { return baseline - (rows.length - 1 - r) * base * fit * ROW_STEP; }
      var sway = Math.sin(t * 0.4) * 0.04;
      var glyphs = [], x = left;
      chars.forEach(function (c, i) {
        // (A row begins at the left; a space at its start takes no room.)
        if (rows.some(function (r) { return r[0] === i; })) x = left;
        var room = rooms[i], size = sizes[i];
        if (blanks[i]) { x += x === left ? 0 : room; return; }
        var middle = x + room / 2;
        x += room;
        var age = position - c[1];
        if (age < -0.25) return;
        var y = rowBaseline(rowOf(i)) + Math.sin(i * 0.9 + t * 0.8 + which) * base * fit * 0.18 + (middle - w / 2) * sway;
        var came = ease((age + 0.25) / 0.55);
        // How lit it is: up as it is sung, down after.
        var lit = ease((age + 0.08) / 0.18) * (1 - ease((age - c[2] - 0.1) / 0.5)) * (1 - out);
        var breathe = 1 + 0.02 * Math.sin(t * 1.7 + i * 1.3) * (0.5 + music.swell);
        var rise = (1 - came) * 26 - out * 60 - music.beat * 3;
        glyphs.push({
          said: c[0], font: font(size, f), middle: middle, base: y + rise, pivot: y - size * 0.35 + rise,
          tilt: (TILTS[(i + which) % 8] + Math.sin(t * 0.6 + i) * 0.025) * (0.4 + 0.6 * came),
          swell: breathe * (1 + lit * (0.14 * music.bass + 0.04 * music.beat)),
          came: came, lit: lit, alpha: Math.min(came * 1.4, 1) * (1 - out), room: room
        });
      });
      // Each drawn on `c` in white, so much of it as `of` says.
      function draw(c, of, ink) {
        glyphs.forEach(function (g) {
          var a = of(g);
          if (a <= 0.004) return;
          var i = ink ? ink(g) : 1;
          c.save();
          c.translate(g.middle, g.pivot);
          c.rotate(g.tilt);
          c.scale(g.swell, g.swell);
          c.translate(-g.middle, -g.pivot);
          c.font = g.font;
          c.fillStyle = rgba(i, i * 0.985, i * 0.965, a);
          c.fillText(g.said, g.middle - g.room / 2, g.base);
          c.restore();
        });
      }
      // The glows, each for the whole line at once: every character's,
      // softly; the one being sung's, wider and stronger with the highs; and
      // those still coming, out of a blur.
      glow([1, 1, 1, 0.85], (10 + 8 * music.swell) / 2, function (c) { draw(c, function (g) { return g.alpha; }); });
      if (glyphs.some(function (g) { return g.lit > 0.01; }))
        glow([1, 1, 1, 0.85], (24 + 30 * music.high) / 2, function (c) { draw(c, function (g) { return g.alpha * g.lit; }); });
      if (glyphs.some(function (g) { return g.came < 1; }))
        glow([1, 1, 1, 1], 4, function (c) { draw(c, function (g) { return g.alpha * (1 - g.came); }); });
      draw(ctx, function (g) { return g.alpha * g.came * g.came; }, function (g) { return 0.92 + 0.08 * g.lit; });
    }

    // `text` on one line `width` wide at most, its box's top at `top`, ending
    // in an ellipsis where it is too long.
    function textIn(text, size, colour, x, top, width) {
      var f = family();
      ctx.font = font(size, f);
      var said = text;
      if (ctx.measureText(said).width > width) {
        var chars = Array.from(text), n = chars.length;
        while (n > 0 && ctx.measureText(chars.slice(0, n).join("").replace(/\s+$/, "") + "…").width > width) n--;
        said = chars.slice(0, n).join("").replace(/\s+$/, "") + "…";
      }
      // (Its baseline as far below the top as the face's ascent: measured
      // large, as browsers give it in whole pixels.)
      ctx.font = font(1000, f);
      var ascent = ctx.measureText("0").fontBoundingBoxAscent * size / 1000;
      ctx.font = font(size, f);
      ctx.fillStyle = colour;
      ctx.fillText(said, x, top + ascent);
    }

    // The line sung now (and the one before it as it leaves) large at the
    // left, and the next line small under it.
    function drawLines(position, t, now, music) {
      var left = w * 0.081, baseline = h * 0.62, base = h * 0.133;
      if (S.leaving && (now - S.leaving[1]) / 1000 < LEAVING && lines[S.leaving[0]]) {
        var before = S.leaving[0], after = lines[before + 1];
        drawLine(lines[before], after ? after.at : null, before, position, t, music, ease((now - S.leaving[1]) / 1000 / LEAVING), left, baseline, base);
      }
      if (S.now == null) return;
      var line = lines[S.now], next = lines[S.now + 1];
      drawLine(line, next ? next.at : null, S.now, position, t, music, 0, left, baseline, base);
      // The next line, small under it, coming in a moment after.
      if (next && !blank(next.text)) {
        var shown = Math.min(Math.max(position - line.at - 0.3, 0), 1) * 0.5;
        textIn(next.text, Math.max(h * 0.029, 14), rgba(1, 1, 1, shown), left + 4, baseline + h * 0.09, w - 2 * left);
      }
    }

    function frame(t, now) {
      follow(now, heardAt(t), sungAt(lines, t));
      var position = Math.max(0, t);
      if (smoke) smoke.render(S.flow, S.swell, S.tint, S.tint2);
      else host.style.background = rgba(S.tint[0] * 0.1, S.tint[1] * 0.1, S.tint[2] * 0.1, 1);
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.clearRect(0, 0, words.width, words.height);
      ctx.setTransform(k, 0, 0, k, 0, 0);
      var music = { swell: S.swell, beat: S.beat, high: part(14, 20, S.smooth), bass: part(0, 4, S.smooth) };
      drawLines(position, (now - S.began) / 1000, now, music);
    }

    resize();
    return {
      frame: frame,
      resize: resize,
      destroy: function () {
        if (smoke) smoke.lose();
        [ground, words].forEach(function (c) { if (c.parentNode) c.parentNode.removeChild(c); });
      }
    };
  };
})();
