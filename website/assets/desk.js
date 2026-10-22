// The desk: a screen in the page on which Glance is shown as it is used. It
// plays itself (the monitor pushed out of the right edge, torn off onto the
// desktop, sized through its layouts and stuck to the edge, the workbench
// pushed out of the left, music pulled down from the top and torn onto the
// desktop, sized through its layouts and sent full screen) until the pointer
// comes onto it: then the edges, the widgets and the full screen are the
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
  // The gap between an edge's panel and the screen edge, in each look (theme.rs `inset`): chart paper's slab lies flush
  // with it.
  var INSET = { paper: 0, glass: 10, win11: 12 };
  // How long an edge's panel takes to slide out and back (s), and how long
  // the pointer may be away from it before it goes (ms), as Glance's.
  var SLIDE = 0.3, AWAY = 220;
  // The music widget pulled from the top edge, as Glance's panel holds it there.
  var TOP = { w: 640, h: 200 };
  // Pressed this near a widget's edge (corner) it is sized, else moved; it
  // is taken only once moved this far.
  var EDGE = 8, CORNER = 20, MOVE_FROM = 4, SETTLE = 0.26;
  // Let go this near a screen edge, a widget sticks to it; pulled this far
  // from it, it is free again; from this near, where it would stick shows
  // (widget.rs `STICK`, `UNSTICK`, `HINT_SEEN`). Stuck, it snaps flush in SNAP s.
  var STICK = 36, UNSTICK = 52, HINT_SEEN = 64, SNAP = 0.15;
  // Back to playing itself this long after the pointer leaves, or rests on it (ms).
  var BACK_AFTER = 2500, IDLE = 10000;

  var screen = desk.querySelector(".screen");
  var edges = { right: desk.querySelector('[data-at="right"]'), left: desk.querySelector('[data-at="left"]') };
  var fullHost = desk.querySelector(".full"), escKey = desk.querySelector(".esc"), pointer = desk.querySelector(".pointer");
  // The widgets' windows: a canvas each, the music's above the monitor's.
  var mw = desk.querySelector(".mw"), monw = mw.cloneNode(true);
  monw.classList.add("mon");
  screen.insertBefore(monw, mw);
  // Where a widget let go would stick, as the look draws it.
  var hint = document.createElement("div");
  hint.className = "hint";
  screen.insertBefore(hint, monw);
  // The taskbar's programs: the workbench rail's own pictures.
  var taskbar = desk.querySelector(".taskbar"), names = GD.apps.names(lang);
  GD.apps.kinds.forEach(function (kind, at) {
    var one = document.createElement("i");
    one.title = names[at];
    one.appendChild(GD.app(kind, 28));
    taskbar.appendChild(one);
  });
  var say = document.querySelector("[data-say]");
  var chapters = document.querySelector("[data-chapters]");
  var soundButton = document.querySelector("[data-sound]");
  var still = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  if (GD.bench) edges.left.appendChild(GD.bench(lang).node);
  // The monitor torn off as a widget (readings.js), and the desktop's picture, which its acrylic and glass see behind
  // them as Glance's do (the screen's ground: see desk.css).
  var mon = GD.readings ? GD.readings(lang) : null;
  var wall = new Image();
  wall.onload = function () { request(); };
  wall.src = base + "img/desk-wall.webp?v=2";
  function behind(box) { return wall.complete && wall.naturalWidth ? { image: wall, x: -box.x, y: -box.y, w: SW, h: SH } : null; }

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
  function lerp(a, b, e) { return a + (b - a) * e; }

  // ---------------- what the desk plays ----------------
  // The monitor: pushed out at 1.35 s, taken by its bar and torn off at 2.8 s, put down at 4.1 s, sized by its corner
  // from 5.0 s to 7.9 s, moved to the right edge from 8.95 s and let go there at 10.1 s, stuck to it from then on.
  var M = { press: 2.7, tear: 2.8, put: 4.0, letGo: 4.1, corner: 4.9, size: 5.0, sized: 7.9, move: 8.95, moved: 10.0, stuck: 10.1, away: 11.0 };
  // Where it is put down, and the rooms the hand gives it (as shares of the panel's, then DIPs).
  var PUT = { x: 300, y: 140 }, SHRUNK = [0.6, 0.62], SMALL = { w: 340, h: 196 };
  // The music widget: pulled from the top at 18.35 s, taken at 19.9 s by the
  // pointer 320 × 98 DIPs into it, let go at 20.9 s and sized by its corner
  // through a strip, a player, a card and the stage; full screen from 31.7 s
  // to 39.3 s; gone by the end, as the desk begins again. The song runs from
  // SONG_FROM, so that the full screen opens on its last line.
  var LOOP = 42.5, SONG_FROM = 11.35;
  var TAKEN = [320, 98];
  var DRAG = [[19.9, 720, 110], [20.9, 520, 430]];
  var BOX = [[20.9, 200, 332, 640, 200], [22.1, 200, 332, 640, 200], [22.9, 200, 332, 560, 72], [24.18, 200, 332, 560, 72],
    [24.98, 200, 332, 560, 196], [26.42, 200, 332, 560, 196], [27.38, 200, 332, 400, 380], [28.98, 200, 332, 400, 380], [29.94, 200, 332, 600, 380]];
  var FULL = [31.7, 39.3], ESC = [38.7, 39.2], GONE = 41.4;
  // The pointer from the monitor's corner on: to the workbench, then music.
  var WAY = [[11.0, 760, 600], [11.8, 0, 420], [12.4, 0, 420], [13.2, 300, 300], [15.0, 420, 560], [16.0, 760, 600],
    [17.5, 760, 600], [18.3, 720, 0], [18.9, 720, 0], [19.6, 720, 110]];
  var SAYS = zh ? [
    [0, "鼠标推到屏幕右边缘，系统监控滑出来，移开就收回。"],
    [M.press, "按住面板拖到桌面上，它就成了小组件。拖动边角改大小，排版跟着空间变化。"],
    [M.sized + 0.3, "拖到屏幕边缘松手，小组件会贴靠在这条边上。"],
    [M.away, "左边缘放着工作台：常用程序、时间、中转站和便签。"],
    [17.5, "音乐放在上边缘。按住它往下拖，就成了桌面上的小组件。"],
    [21.6, "拖动边角改大小，排版跟着空间变：横条、播放器、卡片，够宽就放唱片。"],
    [30.6, "点右上角的全屏按钮，歌词铺满整个屏幕，按 Esc 退出。"]
  ] : [
    [0, "Push the pointer against the right edge and the monitor slides out. Move away and it slides back."],
    [M.press, "Drag the panel onto the desktop and it becomes a widget. Drag a corner to size it and the layout follows the room."],
    [M.sized + 0.3, "Let a widget go at a screen edge and it sticks to that edge."],
    [M.away, "The left edge holds the workbench: programs at hand, the time, the station and notes."],
    [17.5, "Music sits at the top edge. Drag it down and it becomes a widget on the desktop."],
    [21.6, "Drag a corner to size it and the layout follows the room: a strip, a player, a card, and a record once it is wide."],
    [30.6, "The full screen button fills the screen with the lyrics. Esc leaves it."]
  ];
  var LIVE_SAY = zh ? "现在由你操作：推这块屏幕的左、右、上三条边，把监控面板或音乐拖到桌面上，拖动小组件的边角改大小，拖到屏幕边缘贴靠，或者点全屏。鼠标移开后，演示会继续。"
    : "It is yours now: push this screen's left, right and top edges, drag the monitor or music onto the desktop, size a widget by its corners, stick it to an edge, or take music full screen. Move the pointer away and the demo carries on.";
  var CHAPTERS = [0, M.away, 17.5, 30.6];

  // Where the full screen button is on the stage the desk sizes music to
  // (found once music is loaded, from the widget's own hit test).
  var fullButton = null;

  // The monitor panel at the right edge, out: its corner and size, and how many columns it shows.
  function panelAt() {
    var el = edges.right, w = el.offsetWidth, h = el.offsetHeight;
    return { x: SW - w - INSET[root.getAttribute("data-look")], y: Math.max(GAP, (WORK - h) / 2), w: w, h: h,
      columns: el.querySelectorAll(".gp .col").length || 1 };
  }
  // Torn off and come to rest in this play of the desk: the monitor widget's state there is its own, made anew each
  // time it is torn off.
  var monTorn = null, monRest = null, lastT = 0;

  // The desk at `t` seconds into what it plays.
  function scene(t, now) {
    var s = { right: t < M.tear ? rise(t, 1.35, SLIDE) : 0, left: rise(t, 11.85, SLIDE) - rise(t, 15.6, SLIDE),
      music: null, mon: null, hint: null, full: 0, esc: 0, escDown: false, cursor: null, song: mod(t - SONG_FROM, song ? song.length : 1), playing: true };
    if (t < lastT || t < M.tear) { monTorn = null; monRest = null; }
    lastT = t;
    var P = panelAt(), grab = [P.x + P.w / 2, P.y + P.h - 22], put = [PUT.x + P.w / 2, PUT.y + P.h - 22];
    var shrunk = { w: Math.round(P.w * SHRUNK[0]), h: Math.round(P.h * SHRUNK[1]) };
    var rooms = [[M.size, P.w, P.h], [5.9, shrunk.w, shrunk.h], [6.7, shrunk.w, shrunk.h], [7.6, SMALL.w, SMALL.h]];
    var c, down = false;

    // ---- the monitor, torn off and stuck ----
    if (mon && t >= M.tear) {
      // (As large as the panel was, as its form lays it out.)
      if (!monTorn) monTorn = mon.torn(P.w, P.h, P.columns);
      var box;
      if (t < M.put) {
        var hand = keyed([[M.tear, grab[0], grab[1]], [M.put, put[0], put[1]]], t);
        box = { x: P.x + hand[0] - grab[0], y: P.y + hand[1] - grab[1], w: monTorn.w, h: monTorn.h };
      } else if (t < M.size) box = { x: PUT.x, y: PUT.y, w: monTorn.w, h: monTorn.h };
      else if (t < M.sized) {
        var room = keyed(rooms, t), drawn = mon.room(room[0], room[1], now);
        box = { x: PUT.x, y: PUT.y, w: drawn.w, h: drawn.h };
      } else {
        if (!monRest) { var last = rooms[rooms.length - 1], from = mon.room(last[1], last[2], now); monRest = { from: from, to: mon.rest() }; }
        var e = out((t - M.sized) / SETTLE), size = { w: lerp(monRest.from.w, monRest.to.w, e), h: lerp(monRest.from.h, monRest.to.h, e) };
        var mid = [PUT.x + monRest.to.w / 2, PUT.y + monRest.to.h / 2], by = [SW - 16 - (PUT.x + monRest.to.w), 30];
        var moved = keyed([[M.move, 0, 0], [M.moved, by[0], by[1]]], t);
        box = { x: PUT.x + moved[0], y: PUT.y + moved[1], w: size.w, h: size.h };
        if (t >= M.stuck) box.x = lerp(box.x, SW - box.w, out((t - M.stuck) / SNAP));
        // Brought near the edge: where it would stick, firmer the nearer.
        else if (t >= M.move) s.hint = hintFor(box, "right", SW - box.x - box.w);
      }
      s.mon = { box: box, alpha: 1 - rise(t, GONE, 0.7), stuck: t >= M.stuck ? "right" : null };
    }

    // ---- the pointer ----
    if (t < M.press) c = keyed([[0, 880, 560], [0.5, 880, 560], [1.3, 1439, 440], [1.9, 1439, 440], [2.6, grab[0], grab[1]]], t);
    else if (t < M.letGo) { c = keyed([[M.tear, grab[0], grab[1]], [M.put, put[0], put[1]]], t); down = true; }
    else if (t < M.size) c = keyed([[M.letGo, put[0], put[1]], [M.corner, PUT.x + P.w - 3, PUT.y + P.h - 3]], t);
    else if (t < M.sized) { var r = keyed(rooms, t); c = [PUT.x + r[0] - 3, PUT.y + r[1] - 3]; down = true; }
    else if (t < M.away && monRest) {
      var rest = monRest.to, grip = [PUT.x + rest.w / 2, PUT.y + rest.h / 2], gone = [SW - 16 - (PUT.x + rest.w), 30];
      c = keyed([[M.sized, PUT.x + SMALL.w - 3, PUT.y + SMALL.h - 3], [8.8, grip[0], grip[1]], [M.move, grip[0], grip[1]],
        [M.moved, grip[0] + gone[0], grip[1] + gone[1]], [M.stuck, grip[0] + gone[0], grip[1] + gone[1]], [M.away, 760, 600]], t);
      down = t >= 8.9 && t < M.stuck;
    } else if (t < M.away) c = [760, 600];
    else if (t < DRAG[0][0]) c = keyed(WAY, t);
    else if (t < DRAG[1][0]) { c = keyed(DRAG, t); down = true; }
    else if (t < 21.6) c = keyed([[20.9, 520, 430], [21.6, 837, 529]], t);
    else if (t < 30.6) {
      var b = keyed(BOX, t);
      c = [b[0] + b[2] - 3, b[1] + b[3] - 3];
      down = t >= 21.86 && t < 30.02 && !((t > 22.98 && t < 24.1) || (t > 25.06 && t < 26.34) || (t > 27.46 && t < 28.9));
    } else {
      var at = BOX[BOX.length - 1], f = fullButton;
      var aim = [at[1] + f[0], at[2] + f[1]];
      c = keyed([[30.6, at[1] + at[3] - 3, at[2] + at[4] - 3], [31.3, aim[0], aim[1]], [39.7, aim[0], aim[1]], [40.9, 880, 560]], t);
      down = t >= 31.45 && t < 31.6;
    }
    var shown = t < FULL[0] || t >= 39.7;
    s.cursor = { x: c[0], y: c[1], down: down, alpha: shown ? 1 - rise(t, GONE, 0.6) : 0 };

    // ---- music: at the top until taken, then on the desktop ----
    if (t < DRAG[0][0]) s.music = { at: "top", p: rise(t, 18.35, SLIDE), box: null, alpha: 1, stuck: null };
    else {
      var mbox;
      if (t < DRAG[1][0]) { var d = keyed(DRAG, t); mbox = [d[0] - TAKEN[0], d[1] - TAKEN[1], TOP.w, TOP.h]; }
      else mbox = keyed(BOX, t);
      s.music = { at: "desk", p: 1, box: { x: mbox[0], y: mbox[1], w: mbox[2], h: mbox[3] }, alpha: 1 - rise(t, GONE, 0.7), stuck: null };
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
        var at = BOX[BOX.length - 1], rest = { x: at[1], y: at[2], w: at[3], h: at[4] }, f = music.place(at[3], at[4], "full", rackFor(rest, music));
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
  // (Placed by its left and top, not moved by a transform: a transform would make the edge a group of its own, and
  // Windows 11's sheet's luminosity layer would blend with nothing under it but the sheet, not the desktop.)
  function placeEdge(side, p) {
    var el = edges[side], w = el.offsetWidth, h = el.offsetHeight, e = out(p);
    var inset = INSET[root.getAttribute("data-look")];
    var x = side === "right" ? SW - (w + inset) * e : -w + (w + inset) * e;
    el.style.left = x.toFixed(2) + "px";
    el.style.top = Math.max(GAP, (WORK - h) / 2).toFixed(2) + "px";
    el.style.visibility = p > 0 ? "visible" : "hidden";
  }
  // Where a widget at `box` has its buttons' row, as Glance puts it (widget.rs `rack`): over its right end (under it,
  // where the screen has no room above); narrower than the row and tall enough, down its right side (its left, where
  // the screen has no room on the right).
  function rackFor(box, of) {
    if (box.w >= of.row || box.h < of.row) return box.y - of.reach < 0 ? "under" : "above";
    return box.x + box.w + of.reach <= SW ? "right" : "left";
  }
  // What a widget at `box` takes of the pointer: itself; and, the pointer over it already, its buttons' row too
  // (widget.rs `taking`).
  function takes(p, box, rack, over, of) {
    if (within(p, box)) return true;
    if (!over || !rack) return false;
    var r = of.reach;
    return within(p, {
      x: box.x - (rack === "left" ? r : 0), y: box.y - (rack === "above" ? r : 0),
      w: box.w + (rack === "left" || rack === "right" ? r : 0), h: box.h + (rack === "above" || rack === "under" ? r : 0)
    });
  }
  function musicBox(m) {
    if (m.at === "top") return { x: (SW - TOP.w) / 2, y: -TOP.h + (TOP.h + GAP) * out(m.p), w: TOP.w, h: TOP.h };
    return m.box;
  }
  // A widget's window: its canvas, `box` with its margin round it, the pointer over it or not (kept in `view.over`).
  function views(host) {
    var canvas = host.querySelector("canvas");
    return { host: host, canvas: canvas, ctx: canvas.getContext("2d"), over: false };
  }
  var musicView = views(mw), monView = views(monw);
  function drawWidget(view, of, box, alpha, s, paint, desktop) {
    var g = of.margin, cw = box.w + 2 * g, ch = box.h + 2 * g;
    var dpr = window.devicePixelRatio || 1, px = Math.round(cw * k * dpr), py = Math.round(ch * k * dpr);
    view.host.style.transform = "translate(" + (box.x - g).toFixed(2) + "px," + (box.y - g).toFixed(2) + "px)";
    view.host.style.width = cw + "px";
    view.host.style.height = ch + "px";
    view.host.style.opacity = alpha;
    if (view.canvas.width !== px || view.canvas.height !== py) { view.canvas.width = px; view.canvas.height = py; }
    var c = view.ctx;
    c.setTransform(1, 0, 0, 1, 0, 0);
    c.clearRect(0, 0, px, py);
    c.setTransform(px / cw, 0, 0, py / ch, g * px / cw, g * py / ch);
    var p = s.cursor, rack = desktop ? rackFor(box, of) : null;
    // (Playing itself, its pointer comes to the row only from over it: what shows is the moment's own.)
    view.over = !!p && p.alpha > 0 && takes(p, box, rack, view.over || !live, of);
    paint(c, view.over ? { x: p.x - box.x, y: p.y - box.y } : null, rack);
  }
  function draw(s, now) {
    placeEdge("right", s.right);
    placeEdge("left", s.left);
    var m = s.music, box = m && musicBox(m), on = music && box && m.alpha > 0 && (m.at === "desk" || m.p > 0);
    mw.hidden = !on;
    if (on) drawWidget(musicView, music, box, m.alpha, s, function (c, at, rack) {
      music.draw(c, box.w, box.h, s.song, now, at, s.playing, rack);
    }, m.at === "desk");
    var n = s.mon, shown = mon && n && n.alpha > 0;
    monw.hidden = !shown;
    if (shown) drawWidget(monView, mon, n.box, n.alpha, s, function (c, at, rack) {
      mon.draw(c, n.box.w, n.box.h, now, at, rack, behind(n.box));
    }, true);
    var h = s.hint;
    hint.classList.toggle("on", !!h);
    if (h) {
      hint.style.transform = "translate(" + h.x.toFixed(2) + "px," + h.y.toFixed(2) + "px)";
      hint.style.width = h.w + "px";
      hint.style.height = h.h + "px";
      hint.style.opacity = h.strength;
      hint.classList.toggle("firm", h.firm);
      hint.classList.toggle("own", !!h.ground);
      if (h.ground) hint.style.setProperty("--ground", h.ground.map(function (v) { return Math.round(v * 255); }).join(","));
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

  // ---------------- sticking to an edge ----------------
  // How far `box` is from each side of the work area, nearest first (widget.rs `nearest_side`).
  function nearest(box) {
    return [["left", box.x], ["right", SW - box.x - box.w], ["top", box.y], ["bottom", WORK - box.y - box.h]]
      .reduce(function (a, b) { return b[1] < a[1] ? b : a; });
  }
  function awayFrom(box, side) {
    return { left: box.x, right: SW - box.x - box.w, top: box.y, bottom: WORK - box.y - box.h }[side];
  }
  // Where `box` stuck to `side` is: flush with it, its middle where it was along it (widget.rs `stuck_at`).
  function stuckAt(box, side) {
    var x = side === "left" ? 0 : side === "right" ? SW - box.w : box.x;
    var y = side === "top" ? 0 : side === "bottom" ? WORK - box.h : box.y;
    return { x: Math.max(0, Math.min(x, SW - box.w)), y: Math.max(0, Math.min(y, WORK - box.h)), w: box.w, h: box.h };
  }
  // Where `box`, `away` from `side`, would stick there, shown firmer the nearer (widget.rs, the hint's strength).
  function hintFor(box, side, away) {
    if (away >= HINT_SEEN) return null;
    var firm = away < STICK, at = stuckAt(box, side);
    return { x: at.x, y: at.y, w: at.w, h: at.h, firm: firm,
      strength: firm ? 1 : Math.round((0.25 + 0.5 * (1 - (away - STICK) / (HINT_SEEN - STICK))) * 20) / 20 };
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
      state = scene(t, now);
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
    // (None of them, while the visitor has the desk: its mark goes too.)
    if (n < 0) chapters.style.setProperty("--w", "0px");
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
  var drag = null, settle = null, snap = null, hover = null, back = null, idle = null;
  function enter() {
    if (live) return;
    live = true;
    want = { right: false, left: false, top: false };
    if (state.music && state.music.at === "desk") state.music.alpha = 1;
    if (state.mon) state.mon.alpha = 1;
    state.hint = null;
    state.cursor = null;
    setSay(LIVE_SAY);
    setChapter(-1);
    request();
  }
  // Playing itself again, from `from` seconds.
  function play(from) {
    if (still) { show(from); return; }
    live = false;
    drag = settle = snap = hover = null;
    t = from;
    if (back) { clearTimeout(back); back = null; }
    if (idle) { clearTimeout(idle); idle = null; }
    chapter = -1;
    request();
  }
  // Asked for less motion, a part shown standing: the desk as it is in the
  // middle of it, the visitor's from there.
  var MIDDLES = [2.0, 12.8, 30.3, 34];
  function show(from) {
    var n = CHAPTERS.indexOf(from), s = scene(MIDDLES[n < 0 ? 2 : n], performance.now());
    s.cursor = null;
    s.hint = null;
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
      var e = out((now - settle.at) / 1000 / SETTLE), b = settle.widget.box;
      b.w = lerp(settle.from.w, settle.to.w, e);
      b.h = lerp(settle.from.h, settle.to.h, e);
      if (settle.right != null) b.x = settle.right - b.w;
      if (settle.bottom != null) b.y = settle.bottom - b.h;
      if (e >= 1) settle = null;
    }
    if (snap) {
      var f = out((now - snap.at) / 1000 / SNAP), sb = snap.widget.box;
      sb.x = lerp(snap.from.x, snap.to.x, f);
      sb.y = lerp(snap.from.y, snap.to.y, f);
      if (f >= 1) snap = null;
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
    return { x: parseFloat(el.style.left), y: parseFloat(el.style.top), w: el.offsetWidth, h: el.offsetHeight };
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
  function mouse(e) { return e.pointerType === "mouse" || e.pointerType === "pen"; }
  function rested() {
    if (idle) clearTimeout(idle);
    idle = setTimeout(function () { if (!drag) play(0); }, IDLE);
  }
  // The widgets on the desktop, the one in front first: each with what draws it, its view and its state.
  function widgets() {
    var s = state, list = [];
    if (music && s.music && (s.music.at === "desk" || s.music.p > 0.9)) list.push({ of: music, view: musicView, w: s.music, box: musicBox(s.music), desktop: s.music.at === "desk" });
    if (mon && s.mon) list.push({ of: mon, view: monView, w: s.mon, box: s.mon.box, desktop: true });
    return list;
  }
  // The widget the pointer at `p` is on, and whether on its buttons' row.
  function widgetAt(p) {
    var list = widgets();
    for (var i = 0; i < list.length; i++) {
      var x = list[i];
      if (within(p, x.box)) return { x: x, row: false };
      if (x.desktop && takes(p, x.box, rackFor(x.box, x.of), x.view.over, x.of)) return { x: x, row: true };
    }
    return null;
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
    var p = dip(e), s = state, now = performance.now();
    hover = p;
    if (drag) { dragTo(p, now); return; }
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
    var on = s.right > 0.9 && within(p, rectOf(edges.right)) ? null : widgetAt(p);
    screen.style.cursor = on && !on.row ? cursorFor(on.x.desktop ? edgesAt(p, on.x.box) : null) : "";
    request();
  });
  desk.addEventListener("pointerdown", function (e) {
    if (!mouse(e) || e.button !== 0) return;
    if (!live) enter();
    var p = dip(e), s = state;
    if (s.fullOn) { s.fullOn = false; e.preventDefault(); return; }
    // The monitor panel, out, over everything: pulled, it is torn off whole.
    var panel = rectOf(edges.right);
    if (mon && s.right > 0.9 && within(p, panel)) drag = { panel: panel, from: p, moved: false };
    else {
      var on = widgetAt(p);
      if (!on) return;
      var x = on.x, ed = x.desktop && !on.row ? edgesAt(p, x.box) : null;
      drag = { x: x, from: p, box: { x: x.box.x, y: x.box.y, w: x.box.w, h: x.box.h }, edges: ed, moved: false, row: on.row,
        torn: x.w.at === "top" ? "top" : null };
    }
    settle = snap = null;
    desk.setPointerCapture(e.pointerId);
    e.preventDefault();
  });
  function dragTo(p, now) {
    var dx = p.x - drag.from.x, dy = p.y - drag.from.y, s = state;
    if (drag.row || !drag.moved && Math.hypot(dx, dy) < MOVE_FROM) return;
    drag.moved = true;
    // Pulled far enough: the panel as it shows, a widget under the hand (panel.rs `dragged`), the panel gone.
    if (drag.panel) {
      var P = panelAt(), r = drag.panel;
      var made = mon.torn(r.w, r.h, P.columns);
      r = { x: r.x, y: r.y, w: made.w, h: made.h };
      s.mon = { box: { x: r.x, y: r.y, w: r.w, h: r.h }, alpha: 1, stuck: null };
      s.right = 0;
      want.right = false;
      drag = { x: widgets().filter(function (w) { return w.of === mon; })[0], from: drag.from, box: { x: r.x, y: r.y, w: r.w, h: r.h },
        edges: null, moved: true, row: false, torn: "right" };
    }
    var x = drag.x, w = x.w, ed = drag.edges;
    if (!ed) {
      // Taken from the top edge: on the desktop from now on.
      if (w === s.music && w.at === "top") { w.at = "desk"; want.top = false; }
      var o = drag.box, box = { x: Math.max(0, Math.min(o.x + dx, SW - o.w)), y: Math.max(0, Math.min(o.y + dy, WORK - o.h)), w: o.w, h: o.h };
      w.box = box;
      // Pulled away from the edge it was stuck to: free again. Torn off and well clear of the edge it came from: in
      // hand as any widget is.
      if (w.stuck && awayFrom(box, w.stuck) > UNSTICK) w.stuck = null;
      if (drag.torn && awayFrom(box, drag.torn) > UNSTICK) drag.torn = null;
      var side = nearest(box);
      s.hint = !w.stuck && !drag.torn ? hintFor(box, side[0], side[1]) : null;
      if (s.hint && x.of === music) s.hint.ground = music.ground(box.w, box.h);
    } else {
      var a = drag.box, least = x.of.least;
      var rw = Math.max(least[0], Math.min(a.w + (ed.r ? dx : ed.l ? -dx : 0), ed.l ? a.x + a.w : SW - a.x));
      var rh = Math.max(least[1], Math.min(a.h + (ed.b ? dy : ed.t ? -dy : 0), ed.t ? a.y + a.h : WORK - a.y));
      // (The music draws at the room the hand gives it, the monitor at what its form makes of that room.)
      var size = x.of === mon ? mon.room(rw, rh, now, { left: ed.l, top: ed.t }) : { w: rw, h: rh };
      w.box = { x: ed.l ? a.x + a.w - size.w : a.x, y: ed.t ? a.y + a.h - size.h : a.y, w: size.w, h: size.h };
      w.stuck = null;
    }
    request();
  }
  function letGo() {
    if (!drag) return;
    var s = state, d = drag;
    drag = null;
    s.hint = null;
    if (d.panel || !d.x) { request(); return; }
    var w = d.x.w, of = d.x.of;
    if (!d.moved) {
      var b = d.x.box, what = of.hit(b.w, b.h, d.from.x - b.x, d.from.y - b.y);
      if (of === music) {
        if (what === "full") { s.fullOn = true; want = { right: false, left: false, top: false }; }
        else if (what === "play") s.playing = !s.playing;
        else if (what === "prev" || what === "next") s.song = turn(s.song, what === "next" ? 1 : -1);
      }
    } else if (d.edges) {
      // Let go, it comes to rest at the size its layout is right at.
      var to = of === mon ? mon.rest() : (function (r) { return { w: r[0], h: r[1] }; })(music.restsAt(w.box.w, w.box.h));
      // (Sized by its left or top edge, its right or bottom stays.)
      settle = { widget: w, at: performance.now(), from: { w: w.box.w, h: w.box.h }, to: to,
        right: d.edges.l ? w.box.x + w.box.w : null, bottom: d.edges.t ? w.box.y + w.box.h : null };
    } else if (w.at !== "top") {
      // Let go near an edge: stuck to it, flush with it (widget.rs `stick`). Torn off and still at the panel's edge,
      // it is put down where it is let go.
      var side = nearest(w.box), stuck = !d.torn && side[1] < STICK ? side[0] : w.stuck;
      if (stuck) {
        w.stuck = stuck;
        snap = { widget: w, at: performance.now(), from: { x: w.box.x, y: w.box.y }, to: stuckAt(w.box, stuck) };
      }
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
  if (still) show(CHAPTERS[2]);
  else state = scene(0, performance.now());
  request();
})();
