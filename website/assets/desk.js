// The desk: a screen in the page on which Glance is shown as it is used. It
// plays itself (the monitor pushed out of the right edge, the workbench out
// of the left, music pulled down from the top and torn onto the desktop,
// sized through its layouts and sent full screen) until the pointer comes
// onto it: then the edges, the music widget and its full screen are the
// visitor's, as Glance's are, and it plays again a while after the pointer
// leaves or rests. Everything on it is in DIPs of a 1440 × 900 screen,
// scaled to the page.
(function () {
  "use strict";

  var desk = document.querySelector("[data-desk]");
  if (!desk) return;
  var base = document.currentScript.src.replace(/[^\/]*$/, "");
  var root = document.documentElement;
  var lang = root.lang.indexOf("zh") === 0 ? "zh" : "en", zh = lang === "zh";
  var GD = window.GlanceDesk = window.GlanceDesk || {};

  var SW = 1440, SH = 900, BAR = 48, WORK = SH - BAR, GAP = 10;
  // How long an edge's panel takes to slide out and back (s), and how long
  // the pointer may be away from it before it goes (ms), as Glance's.
  var SLIDE = 0.3, AWAY = 220;
  // The music widget pulled from the top edge, as Glance's panel holds it there.
  var TOP = { w: 640, h: 200 };
  // Pressed this near the widget's edge (corner) it is sized, else moved; it
  // is taken only once moved this far.
  var EDGE = 8, CORNER = 20, MOVE_FROM = 4, SETTLE = 0.26;
  // Back to playing itself this long after the pointer leaves, or rests on it (ms).
  var BACK_AFTER = 2500, IDLE = 10000;

  var screen = desk.querySelector(".screen");
  var edges = { right: desk.querySelector('[data-at="right"]'), left: desk.querySelector('[data-at="left"]') };
  var mw = desk.querySelector(".mw"), mwCanvas = mw.querySelector("canvas"), mctx = mwCanvas.getContext("2d");
  var fullHost = desk.querySelector(".full"), escKey = desk.querySelector(".esc"), pointer = desk.querySelector(".pointer");
  var say = document.querySelector("[data-say]");
  var chapters = document.querySelector("[data-chapters]");
  var soundButton = document.querySelector("[data-sound]");
  var still = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  if (GD.bench) edges.left.appendChild(GD.bench(lang).node);

  // ---------------- easing ----------------
  function clamp(v) { return Math.min(1, Math.max(0, v)); }
  function smooth(t) { t = clamp(t); return t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2; }
  function out(t) { return 1 - Math.pow(1 - clamp(t), 3); }
  // From 0 to 1 over `long` seconds from `from`.
  function rise(t, from, long) { return clamp((t - from) / long); }
  // Values eased between keys [t, a, b, ...], held past the ends.
  function keyed(keys, t) {
    if (t <= keys[0][0]) return keys[0].slice(1);
    for (var i = 1; i < keys.length; i++) {
      if (t < keys[i][0]) {
        var a = keys[i - 1], b = keys[i], e = smooth((t - a[0]) / (b[0] - a[0]));
        return a.slice(1).map(function (v, j) { return v + (b[j + 1] - v) * e; });
      }
    }
    return keys[keys.length - 1].slice(1);
  }
  function mod(a, n) { return ((a % n) + n) % n; }

  // ---------------- what the desk plays ----------------
  // The music widget: pulled from the top at 17.1 s, taken at 19.3 s by the
  // pointer 320 × 98 DIPs into it, let go at 20.5 s and sized by its corner
  // through a strip, a player, a card and the stage; full screen from 36.35 s
  // to 45.45 s; gone by the end, as the desk begins again.
  var LOOP = 48, SONG_FROM = 16;
  var TAKEN = [320, 98];
  var DRAG = [[19.3, 720, 110], [20.5, 520, 430]];
  var BOX = [[20.5, 200, 332, 640, 200], [22.0, 200, 332, 640, 200], [23.0, 200, 332, 560, 72], [24.6, 200, 332, 560, 72],
    [25.6, 200, 332, 560, 196], [27.4, 200, 332, 560, 196], [28.6, 200, 332, 400, 380], [30.6, 200, 332, 400, 380], [31.8, 200, 332, 600, 380]];
  var FULL = [36.35, 45.0], ESC = [44.4, 44.9];
  var WAY = [[0, 880, 560], [0.8, 880, 560], [1.8, 1439, 440], [2.6, 1439, 440], [3.4, 1170, 360], [5.4, 1080, 520], [6.6, 760, 600],
    [8.0, 760, 600], [9.0, 0, 420], [9.8, 0, 420], [10.8, 300, 300], [13.4, 420, 560], [14.6, 760, 600],
    [16.0, 760, 600], [17.0, 720, 0], [17.8, 720, 0], [18.8, 720, 110]];
  var SAYS = zh ? [
    [0, "鼠标推到屏幕右边缘，系统监控滑出来，移开就收回。"],
    [8, "左边缘放着工作台：常用程序、时间、中转站和便签。"],
    [16, "音乐放在上边缘。按住它往下拖，就成了桌面上的小组件。"],
    [21.4, "拖动边角改大小，排版跟着空间变：横条、播放器、卡片，够宽就放唱片。"],
    [35.0, "点右上角的全屏按钮，歌词铺满整个屏幕，按 Esc 退出。"]
  ] : [
    [0, "Push the pointer against the right edge and the monitor slides out. Move away and it slides back."],
    [8, "The left edge holds the workbench: programs at hand, the time, the station and notes."],
    [16, "Music sits at the top edge. Drag it down and it becomes a widget on the desktop."],
    [21.4, "Drag a corner to size it and the layout follows the room: a strip, a player, a card, and a record once it is wide."],
    [35.0, "The full screen button fills the screen with the lyrics. Esc leaves it."]
  ];
  var LIVE_SAY = zh ? "现在由你操作：推这块屏幕的左、右、上三条边，拖动音乐小组件、拖它的边角改大小，或者点全屏。鼠标移开后，演示会继续。"
    : "It is yours now: push this screen's left, right and top edges, drag the music widget and its corners, or take it full screen. Move the pointer away and the demo carries on.";
  var CHAPTERS = [0, 8, 16, 35.0];

  // Where the full screen button is on the stage the desk sizes music to
  // (found once music is loaded, from the widget's own hit test).
  var fullButton = null;

  // The desk at `t` seconds into what it plays.
  function scene(t) {
    var s = { right: rise(t, 1.85, SLIDE) - rise(t, 6.3, SLIDE), left: rise(t, 9.05, SLIDE) - rise(t, 14.2, SLIDE),
      music: null, full: 0, esc: 0, escDown: false, cursor: null, song: mod(t - SONG_FROM, song ? song.length : 1), playing: true };
    var c, down = false;
    if (t < DRAG[0][0]) c = keyed(WAY, t);
    else if (t < DRAG[1][0]) { c = keyed(DRAG, t); down = true; }
    else if (t < 21.4) c = keyed([[20.5, 520, 430], [21.4, 837, 529]], t);
    else if (t < 35.0) {
      var b = keyed(BOX, t);
      c = [b[0] + b[2] - 3, b[1] + b[3] - 3];
      down = t >= 21.7 && t < 31.9 && !((t > 23.1 && t < 24.5) || (t > 25.7 && t < 27.3) || (t > 28.7 && t < 30.5));
    } else {
      var at = BOX[BOX.length - 1], f = fullButton;
      var aim = [at[1] + f[0], at[2] + f[1]];
      c = keyed([[35.0, at[1] + at[3] - 3, at[2] + at[4] - 3], [35.8, aim[0], aim[1]], [45.4, aim[0], aim[1]], [46.6, 880, 560]], t);
      down = t >= 36.15 && t < 36.3;
    }
    var shown = t < FULL[0] || t >= 45.4;
    s.cursor = { x: c[0], y: c[1], down: down, alpha: shown ? 1 - rise(t, 47.2, 0.6) : 0 };
    // Music: at the top until taken, then on the desktop.
    if (t < DRAG[0][0]) s.music = { at: "top", p: rise(t, 17.1, SLIDE), box: null, alpha: 1 };
    else {
      var box;
      if (t < DRAG[1][0]) { var d = keyed(DRAG, t); box = [d[0] - TAKEN[0], d[1] - TAKEN[1], TOP.w, TOP.h]; }
      else box = keyed(BOX, t);
      s.music = { at: "desk", p: 1, box: { x: box[0], y: box[1], w: box[2], h: box[3] }, alpha: 1 - rise(t, 47.2, 0.7) };
    }
    s.full = rise(t, FULL[0], 0.45) - rise(t, FULL[1], 0.45);
    s.esc = rise(t, ESC[0], 0.2) - rise(t, ESC[1] + 0.3, 0.25);
    s.escDown = t >= ESC[1] && t < ESC[1] + 0.15;
    return s;
  }
  function sayAt(t) {
    var said = SAYS[0][1];
    SAYS.forEach(function (x) { if (t >= x[0]) said = x[1]; });
    return said;
  }
  function chapterAt(t) {
    var n = 0;
    CHAPTERS.forEach(function (c, i) { if (t >= c) n = i; });
    return n;
  }

  // ---------------- the song, the widget and its full screen ----------------
  var song = null, cover = null, music = null, full = null;
  fetch(base + "song/song.json").then(function (r) { return r.json(); }).then(function (data) {
    var img = new Image();
    img.onload = function () {
      song = data;
      cover = img;
      if (GD.music) {
        music = GD.music(song, cover, lang);
        var at = BOX[BOX.length - 1], f = music.place(at[3], at[4], "full");
        fullButton = [f.x + f.w / 2, f.y + f.h / 2];
      }
      request();
    };
    img.src = base + "song/cover.webp";
  });

  // ---------------- the sound ----------------
  var audio = null, sound = false;
  function setSound(on) {
    sound = on;
    soundButton.setAttribute("aria-pressed", on ? "true" : "false");
    if (on && !audio) { audio = new Audio(base + "song/song.mp3"); audio.loop = true; audio.preload = "auto"; }
    if (audio && !on) audio.pause();
  }
  // Kept with the song as the desk shows it.
  function hear(s) {
    if (!audio) return;
    var want = sound && s.playing && visible;
    if (!want) { if (!audio.paused) audio.pause(); return; }
    if (Math.abs(audio.currentTime - s.song) > 0.3) audio.currentTime = s.song;
    if (audio.paused) audio.play().catch(function () { setSound(false); });
  }
  if (soundButton) soundButton.addEventListener("click", function () { setSound(!sound); request(); });

  // ---------------- drawing ----------------
  var k = 1;
  function fit() {
    k = desk.clientWidth / SW;
    screen.style.transform = "scale(" + k + ")";
    if (full) full.resize();
  }
  function placeEdge(side, p) {
    var el = edges[side], w = el.offsetWidth, h = el.offsetHeight, e = out(p);
    var x = side === "right" ? SW - (w + GAP) * e : -w + (w + GAP) * e;
    el.style.transform = "translate(" + x.toFixed(2) + "px," + Math.max(GAP, (WORK - h) / 2).toFixed(2) + "px)";
    el.style.visibility = p > 0 ? "visible" : "hidden";
  }
  function musicBox(m) {
    if (m.at === "top") return { x: (SW - TOP.w) / 2, y: -TOP.h + (TOP.h + GAP) * out(m.p), w: TOP.w, h: TOP.h };
    return m.box;
  }
  function draw(s, now) {
    placeEdge("right", s.right);
    placeEdge("left", s.left);
    var m = s.music, box = m && musicBox(m), on = music && box && m.alpha > 0 && (m.at === "desk" || m.p > 0);
    mw.hidden = !on;
    if (on) {
      // (Its canvas has room round it for its shadow, as Glance's window has.)
      var g = music.margin, cw = box.w + 2 * g, ch = box.h + 2 * g;
      var dpr = window.devicePixelRatio || 1, px = Math.round(cw * k * dpr), py = Math.round(ch * k * dpr);
      mw.style.transform = "translate(" + (box.x - g).toFixed(2) + "px," + (box.y - g).toFixed(2) + "px)";
      mw.style.width = cw + "px";
      mw.style.height = ch + "px";
      mw.style.opacity = m.alpha;
      if (mwCanvas.width !== px || mwCanvas.height !== py) { mwCanvas.width = px; mwCanvas.height = py; }
      mctx.setTransform(1, 0, 0, 1, 0, 0);
      mctx.clearRect(0, 0, px, py);
      mctx.setTransform(px / cw, 0, 0, py / ch, g * px / cw, g * py / ch);
      var c = s.cursor, over = c && c.alpha > 0 && c.x >= box.x && c.x < box.x + box.w && c.y >= box.y && c.y < box.y + box.h;
      music.draw(mctx, box.w, box.h, s.song, now, over ? { x: c.x - box.x, y: c.y - box.y } : null, s.playing);
    }
    // Opened, the full screen begins as Glance's does; closed, it is gone.
    if (s.full > 0 && song && GD.full && !full) { full = GD.full(fullHost, song, cover, lang); full.resize(); }
    else if (s.full === 0 && full) { full.destroy(); full = null; }
    fullHost.classList.toggle("on", s.full > 0);
    fullHost.style.opacity = s.full;
    if (full && s.full > 0) full.frame(s.song, now);
    escKey.style.opacity = s.esc;
    escKey.style.transform = "translateX(-50%)" + (s.escDown ? " translateY(5px)" : "");
    escKey.style.boxShadow = s.escDown ? "0 1px 0 rgba(170, 168, 200, 0.9), 0 6px 18px rgba(0, 0, 0, 0.4)" : "";
    var c2 = s.cursor;
    pointer.style.opacity = live || !c2 ? 0 : c2.alpha;
    if (c2) pointer.style.transform = "translate(" + c2.x.toFixed(2) + "px," + c2.y.toFixed(2) + "px) scale(" + (c2.down ? 0.88 : 1) + ")";
  }

  // ---------------- the clock ----------------
  // Playing itself: `t` runs on and loops. The visitor's: `state` is theirs,
  // its song running on by itself.
  var t = 0, live = false, state = null, last = null, visible = false;
  // Held at one moment (see GlanceDesk.hold), as a check looks at it.
  var held = null;
  function step(now) {
    var dt = last == null ? 0 : Math.min(0.1, (now - last) / 1000);
    last = now;
    if (held != null) { t = held; dt = 0; }
    if (!live) {
      t = mod(t + dt, LOOP);
      state = scene(t);
      setSay(sayAt(t));
      setChapter(chapterAt(t));
    } else liveStep(dt, now);
    hear(state);
    draw(state, now);
  }

  var said = null;
  function setSay(text) { if (text !== said && say) { said = text; say.textContent = text; } }
  var chapter = -1;
  function setChapter(n) {
    if (n === chapter || !chapters) return;
    chapter = n;
    Array.prototype.forEach.call(chapters.querySelectorAll("button"), function (b, i) {
      b.setAttribute("aria-pressed", i === n ? "true" : "false");
      if (i === n) { chapters.style.setProperty("--x", b.offsetLeft + "px"); chapters.style.setProperty("--w", b.offsetWidth + "px"); }
    });
  }
  if (chapters) chapters.addEventListener("click", function (e) {
    var b = e.target.closest("button");
    if (!b) return;
    var n = Array.prototype.indexOf.call(chapters.querySelectorAll("button"), b);
    play(CHAPTERS[n]);
  });

  // ---------------- the visitor's ----------------
  var want = { right: false, left: false, top: false }, away = { right: null, left: null, top: null };
  var drag = null, settle = null, hover = null, back = null, idle = null;
  function enter() {
    if (live) return;
    live = true;
    want = { right: false, left: false, top: false };
    if (state.music && state.music.at === "desk") state.music.alpha = 1;
    state.cursor = null;
    setSay(LIVE_SAY);
    setChapter(-1);
    request();
  }
  // Playing itself again, from `from` seconds.
  function play(from) {
    if (still) { show(from); return; }
    live = false;
    drag = settle = hover = null;
    t = from;
    if (back) { clearTimeout(back); back = null; }
    if (idle) { clearTimeout(idle); idle = null; }
    chapter = -1;
    request();
  }
  // Asked for less motion, a part shown standing: the desk as it is in the
  // middle of it, the visitor's from there.
  var MIDDLES = [4, 12, 34, 40];
  function show(from) {
    var n = CHAPTERS.indexOf(from), s = scene(MIDDLES[n < 0 ? 2 : n]);
    s.cursor = null;
    s.fullOn = s.full > 0.5;
    s.full = s.fullOn ? 1 : 0;
    want = { right: s.right > 0.5, left: s.left > 0.5, top: false };
    state = s;
    live = true;
    setSay(LIVE_SAY);
    setChapter(n);
    request();
  }
  function liveStep(dt, now) {
    var s = state;
    ["right", "left"].forEach(function (side) { s[side] = clamp(s[side] + (want[side] ? 1 : -1) * dt / SLIDE); });
    if (s.music.at === "top") s.music.p = clamp(s.music.p + (want.top ? 1 : -1) * dt / SLIDE);
    if (settle) {
      var e = out((now - settle.at) / 1000 / SETTLE), b = s.music.box;
      b.w = settle.from.w + (settle.to.w - settle.from.w) * e;
      b.h = settle.from.h + (settle.to.h - settle.from.h) * e;
      if (settle.right != null) b.x = settle.right - b.w;
      if (settle.bottom != null) b.y = settle.bottom - b.h;
      if (e >= 1) settle = null;
    }
    s.full = clamp(s.full + (s.fullOn ? 1 : -1) * dt / 0.45);
    s.esc = 0;
    if (s.playing && song) s.song = mod(s.song + dt, song.length);
    s.cursor = hover ? { x: hover.x, y: hover.y, down: false, alpha: 1 } : null;
  }
  function dip(e) {
    var r = desk.getBoundingClientRect();
    return { x: (e.clientX - r.left) / k, y: (e.clientY - r.top) / k };
  }
  function rectOf(el) {
    var tr = new DOMMatrixReadOnly(getComputedStyle(el).transform);
    return { x: tr.m41, y: tr.m42, w: el.offsetWidth, h: el.offsetHeight };
  }
  function near(p, r, by) { return p.x >= r.x - by && p.x < r.x + r.w + by && p.y >= r.y - by && p.y < r.y + r.h + by; }
  function within(p, b) { return b && p.x >= b.x && p.x < b.x + b.w && p.y >= b.y && p.y < b.y + b.h; }
  function edgesAt(p, b) {
    var x = p.x - b.x, y = p.y - b.y, l = x < EDGE, r = x > b.w - EDGE, t0 = y < EDGE, bo = y > b.h - EDGE;
    var e = { l: l || (t0 || bo) && x < CORNER, r: r || (t0 || bo) && x > b.w - CORNER, t: t0 || (l || r) && y < CORNER, b: bo || (l || r) && y > b.h - CORNER };
    return e.l || e.r || e.t || e.b ? e : null;
  }
  function cursorFor(e) {
    if (!e) return "grab";
    if (e.l && e.t || e.r && e.b) return "nwse-resize";
    if (e.r && e.t || e.l && e.b) return "nesw-resize";
    return e.l || e.r ? "ew-resize" : "ns-resize";
  }
  function shownBox() { var m = state.music; return m && music && (m.at === "desk" || m.p > 0.9) ? musicBox(m) : null; }
  function mouse(e) { return e.pointerType === "mouse" || e.pointerType === "pen"; }
  function rested() {
    if (idle) clearTimeout(idle);
    idle = setTimeout(function () { if (!drag) play(0); }, IDLE);
  }

  desk.addEventListener("pointerenter", function (e) {
    if (!mouse(e)) return;
    if (back) { clearTimeout(back); back = null; }
    enter();
    rested();
  });
  desk.addEventListener("pointerleave", function (e) {
    if (!mouse(e) || drag) return;
    hover = null;
    want = { right: false, left: false, top: false };
    back = setTimeout(function () { back = null; play(0); }, BACK_AFTER);
  });
  desk.addEventListener("pointermove", function (e) {
    if (!mouse(e)) return;
    if (!live) enter();
    rested();
    var p = dip(e), s = state, b = shownBox(), now = performance.now();
    hover = p;
    if (drag) { dragTo(p, e); return; }
    // An edge pushed: what is at it slides out.
    if (!s.fullOn) {
      if (p.x >= SW - 4 && p.y < WORK) want.right = true;
      if (p.x <= 3 && p.y < WORK) want.left = true;
      if (p.y <= 3 && s.music.at === "top") want.top = true;
    }
    // Away from what slid out a moment: it goes.
    ["right", "left", "top"].forEach(function (side) {
      if (!want[side]) return;
      var r = side === "top" ? musicBox(s.music) : rectOf(edges[side]);
      if (near(p, r, 32) || (side === "right" ? p.x >= SW - 4 : side === "left" ? p.x <= 3 : p.y <= 3)) away[side] = null;
      else if (away[side] == null) away[side] = now;
      else if (now - away[side] >= AWAY) { want[side] = false; away[side] = null; }
    });
    screen.style.cursor = within(p, b) ? cursorFor(s.music.at === "desk" ? edgesAt(p, b) : null) : "";
    request();
  });
  desk.addEventListener("pointerdown", function (e) {
    if (!mouse(e) || e.button !== 0) return;
    if (!live) enter();
    var p = dip(e), s = state;
    if (s.fullOn) { s.fullOn = false; e.preventDefault(); return; }
    var b = shownBox();
    if (!within(p, b)) return;
    var ed = s.music.at === "desk" ? edgesAt(p, b) : null;
    drag = { from: p, box: { x: b.x, y: b.y, w: b.w, h: b.h }, edges: ed, moved: false };
    settle = null;
    desk.setPointerCapture(e.pointerId);
    e.preventDefault();
  });
  function dragTo(p) {
    var dx = p.x - drag.from.x, dy = p.y - drag.from.y, s = state, m = s.music, ed = drag.edges;
    if (!drag.moved && Math.hypot(dx, dy) < MOVE_FROM) return;
    drag.moved = true;
    if (!ed) {
      // Taken from the top edge: on the desktop from now on.
      if (m.at === "top") { m.at = "desk"; want.top = false; }
      var x = Math.max(0, Math.min(drag.box.x + dx, SW - drag.box.w)), y = Math.max(0, Math.min(drag.box.y + dy, WORK - drag.box.h));
      m.box = { x: x, y: y, w: drag.box.w, h: drag.box.h };
    } else {
      var o = drag.box, least = music.least;
      var w = o.w + (ed.r ? dx : ed.l ? -dx : 0), h = o.h + (ed.b ? dy : ed.t ? -dy : 0);
      w = Math.max(least[0], Math.min(w, ed.l ? o.x + o.w : SW - o.x));
      h = Math.max(least[1], Math.min(h, ed.t ? o.y + o.h : WORK - o.y));
      m.box = { x: ed.l ? o.x + o.w - w : o.x, y: ed.t ? o.y + o.h - h : o.y, w: w, h: h };
    }
    request();
  }
  function letGo(e) {
    if (!drag) return;
    var s = state, m = s.music, d = drag;
    drag = null;
    if (!d.moved) {
      var b = shownBox(), what = b && music.hit(b.w, b.h, d.from.x - b.x, d.from.y - b.y);
      if (what === "full") { s.fullOn = true; want = { right: false, left: false, top: false }; }
      else if (what === "play") s.playing = !s.playing;
      else if (what === "prev" || what === "next") s.song = turn(s.song, what === "next" ? 1 : -1);
    } else if (d.edges) {
      // Let go, it comes to rest at the size its layout is right at.
      var to = music.restsAt(m.box.w, m.box.h);
      // (Sized by its left or top edge, its right or bottom stays.)
      settle = { at: performance.now(), from: { w: m.box.w, h: m.box.h }, to: { w: to[0], h: to[1] },
        right: d.edges.l ? m.box.x + m.box.w : null, bottom: d.edges.t ? m.box.y + m.box.h : null };
    }
    request();
  }
  desk.addEventListener("pointerup", letGo);
  desk.addEventListener("pointercancel", letGo);
  document.addEventListener("keydown", function (e) {
    if (e.key === "Escape" && live && state.fullOn) state.fullOn = false;
  });
  // The line before or after the one sung at `at`, from its start.
  function turn(at, by) {
    var lines = song.lines, i = -1;
    lines.forEach(function (l, j) { if (at >= l.at - 0.05) i = j; });
    var next = Math.max(0, Math.min(lines.length - 1, i + by));
    return lines[next].at;
  }

  // The desk held at `at` seconds into what it plays
  // (null: playing again), for checking a moment of it against Glance's own.
  GD.hold = function (at) { held = at; live = false; request(); };

  // ---------------- running ----------------
  var queued = false;
  function frame(now) {
    queued = false;
    if (!visible || document.hidden) { last = null; return; }
    step(now);
    request();
  }
  function request() { if (!queued) { queued = true; requestAnimationFrame(frame); } }
  new IntersectionObserver(function (entries) {
    visible = entries[0].isIntersecting;
    if (visible) request();
  }).observe(desk);
  document.addEventListener("visibilitychange", function () { if (!document.hidden) request(); });
  window.addEventListener("resize", function () { fit(); request(); });
  fit();

  // Asked for less motion: nothing plays itself. The widget rests on the
  // desktop as the stage, the edges and the full screen the visitor's.
  if (still) show(16);
  else state = scene(0);
  request();
})();
