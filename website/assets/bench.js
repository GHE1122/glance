// Glance's workbench drawn in the page, as each of its skins draws it in the
// dark (the page's look: html[data-look]): quick launch as a rail at the
// panel's edge, the time, the station and the notes beside it, holding the
// made-up things Glance's studio films it with. Laid out in Glance's DIPs at
// panel size 1 (view.rs: the lanes' padding, heads, blocks and the rail's
// cells), at its natural size, and laid out anew as the look changes. Where it
// stands is the page's to say (see desk.js).
(function () {
  "use strict";

  var SVG = "http://www.w3.org/2000/svg";

  // ---- Glance's measures (DIPs) ----
  var HEAD = 17, HEAD_GAP = 8, SHELF_GAP = 6, LINE = 16;
  var COLUMN = 356, RAIL_W = 60;
  var PLOT = 40, STATION_TALL = 284, NOTES_TALL = 250;
  var RAIL_CELL = 46, RAIL_SIDE = 32, BUTTON = 32, BUTTON_GAP = 4, RAIL_FOOT = 8 + 2 * BUTTON + BUTTON_GAP;
  var TILE_W = 72, TILE_GAP = 8, PLATE = 0.74, CARD_WORDS = 5 + 2 * LINE, CARDS_LEAD = 4;
  var NOTE_STRIP = 30, NOTE_ROW = 40, NOTE_CARD = 104, NOTE_CARDS_GAP = 8;
  var MEDIA_ROW = 24, TAB_H = 24;
  var MINUTE = 60000, DAY = 24 * 60 * MINUTE;

  // ---- what each look lays out differently, as its skin's theme has it in the dark (theme.rs `paper`, `glass`,
  // `fluent`): a lane's padding, the gap between the lanes and beside the rail, the room round them all (top, sides,
  // bottom), the time's figure (its size, and its line as a share of it), the titles' size, the station's tabs' room
  // round their names and the gap between them (view.rs `paint_tabs`) ----
  var LOOKS = {
    paper: { padX: 18, padTop: 12, padBottom: 12, gap: 0, outer: [0, 0, 0], figure: 42, figureLine: 0.84, title: 13, tabPad: 0, tabGap: 16 },
    glass: { padX: 18, padTop: 14, padBottom: 14, gap: 8, outer: [0, 0, 0], figure: 40, figureLine: 0.84, title: 13, tabPad: 8, tabGap: 2 },
    win11: { padX: 16, padTop: 10, padBottom: 12, gap: 0, outer: [10, 6, 6], figure: 34, figureLine: 0.86, title: 14, tabPad: 8, tabGap: 2 }
  };
  // Chart paper's sheet and ink (theme.rs `paper`), and Windows 11's note card's sheet (view.rs `fluent_note_ground`).
  var PAPER = [20, 35, 31], PAPER_INK = [231, 238, 231], FLUENT_NOTE = [40, 42, 46];

  // Lucide's paths, as Glance keeps them (src/ui/icons.rs).
  var ICONS = {
    layers: ["M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83z",
      "M2 12a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 12",
      "M2 17a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 17"],
    grid: ["M4 3h5a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z",
      "M15 3h5a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1h-5a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z",
      "M15 14h5a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1h-5a1 1 0 0 1-1-1v-5a1 1 0 0 1 1-1z",
      "M4 14h5a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1v-5a1 1 0 0 1 1-1z"],
    list: ["M3 5h.01", "M3 12h.01", "M3 19h.01", "M8 5h13", "M8 12h13", "M8 19h13"],
    add: ["M5 12h14", "M12 5v14"],
    chevron: ["m6 9 6 6 6-6"],
    pin: ["M12 17v5", "M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z"],
    settings: ["M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915",
      "M15 12a3 3 0 1 1-6 0a3 3 0 1 1 6 0z"],
    link: ["M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71", "M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"],
    clipboard: ["M9 2h6a1 1 0 0 1 1 1v2a1 1 0 0 1-1 1H9a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1z", "M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"],
    file: ["M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7z", "M14 2v4a2 2 0 0 0 2 2h4"]
  };

  // A note's colours on glass: its tint, the ink written on it (view.rs `note_glass`), and its mark (view.rs
  // `note_colour`): its dot, its band, a little of it in a Windows 11 card.
  var COLOURS = {
    blue: { tint: "55,105,245", ink: "24,66,142", mark: [91, 141, 239] },
    yellow: { tint: "242,247,102", ink: "90,94,18", mark: [232, 182, 74] },
    green: { tint: "104,189,129", ink: "31,94,51", mark: [79, 191, 130] }
  };

  // ---- the words, in each language (Glance's own) ----
  var WORDS = {
    zh: {
      time: "时间", station: "中转站", copied: "最近复制", notes: "便签", quick: "快捷 App", pin: "固定", settings: "设置",
      add: "添加", newNote: "新建便签", shows: ["叠放", "卡片", "列表"], view: "显示方式",
      inkProbe: "中转站最近复制最近文件",
      copiedEmpty: "复制的文字、图片和文件会出现在这里",
      file: "文件", files: function (n) { return n + " 个文件"; }, link: "链接", chars: function (n) { return n + " 字"; },
      justNow: "刚刚", ago: function (m) { return m + " 分钟前"; }, yesterday: "昨天",
      date: function (d) { return (d.getMonth() + 1) + "月" + d.getDate() + "日 周" + "日一二三四五六".charAt(d.getDay()); },
      day: function (d) { return (d.getMonth() + 1) + "月" + d.getDate() + "日"; },
      apps: ["文件资源管理器", "浏览器", "计算器", "记事本", "命令提示符"],
      things: { folder: "项目资料", sheet: "第三季度报表.xlsx", text: "会议改到周四下午三点\n地点：二楼小会议室", picture: "桌面壁纸.jpg" },
      notesHeld: [
        { colour: "blue", title: "本周要做的事", lines: ["交季度报表", "订周六的餐厅", "给妈妈打电话"], ago: 3 * MINUTE },
        { colour: "yellow", title: "显卡价格记录", lines: ["5070 Ti 京东 6299"], ago: 20 * 60 * MINUTE },
        { colour: "green", title: "周末开车听的歌", lines: ["晴天", "后来"], ago: 50 * 60 * MINUTE }
      ]
    },
    en: {
      time: "Time", station: "Station", copied: "Copied", notes: "Notes", quick: "Quick launch", pin: "Pin", settings: "Settings",
      add: "Add", newNote: "New note", shows: ["Stack", "Cards", "List"], view: "View",
      inkProbe: "StationCopiedRecent",
      copiedEmpty: "What you copy shows here",
      file: "File", files: function (n) { return n + " files"; }, link: "Link", chars: function (n) { return n + " characters"; },
      justNow: "Just now", ago: function (m) { return m + " min ago"; }, yesterday: "Yesterday",
      date: function (d) { return "SunMonTueWedThuFriSat".substr(d.getDay() * 3, 3) + ", " + MONTHS.substr(d.getMonth() * 3, 3) + " " + d.getDate(); },
      day: function (d) { return MONTHS.substr(d.getMonth() * 3, 3) + " " + d.getDate(); },
      apps: ["File Explorer", "Browser", "Calculator", "Notepad", "Command Prompt"],
      things: { folder: "Project files", sheet: "Q3 report.xlsx", text: "Meeting moved to Thursday, 3 pm\nRoom 2B, second floor", picture: "Wallpaper.jpg" },
      notesHeld: [
        { colour: "blue", title: "This week", lines: ["Send the Q3 report", "Book Saturday's table", "Call Mum"], ago: 3 * MINUTE },
        { colour: "yellow", title: "GPU prices", lines: ["5070 Ti 549 USD"], ago: 20 * 60 * MINUTE },
        { colour: "green", title: "Songs for the drive", lines: ["Here Comes the Sun"], ago: 50 * 60 * MINUTE }
      ]
    }
  };
  var MONTHS = "JanFebMarAprMayJunJulAugSepOctNovDec";

  // ---- drawing ----
  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }
  function svg(tag, attrs) {
    var e = document.createElementNS(SVG, tag);
    for (var k in attrs) e.setAttribute(k, attrs[k]);
    return e;
  }
  function place(e, x, y, w, h) {
    e.style.left = x + "px";
    e.style.top = y + "px";
    if (w != null) e.style.width = w + "px";
    if (h != null) e.style.height = h + "px";
    return e;
  }
  // Words with their line's top at (`x`, `y`), cut short with an ellipsis past `w` where one is given.
  function text(words, cls, x, y, w) {
    return place(el("div", "tx " + (cls || ""), words), x, y, w);
  }
  // A Lucide icon `size` wide with its middle at (`cx`, `cy`).
  function icon(name, cx, cy, size) {
    var s = svg("svg", { viewBox: "0 0 24 24", "class": "ic", "aria-hidden": "true", width: size, height: size });
    ICONS[name].forEach(function (d) { s.appendChild(svg("path", { d: d })); });
    s.style.left = (cx - size / 2) + "px";
    s.style.top = (cy - size / 2) + "px";
    return s;
  }
  function button(cls, label) {
    var b = el("button", cls);
    b.type = "button";
    b.setAttribute("aria-label", label);
    return b;
  }
  function piece(cls, width, height) {
    var p = el("div", "gb-piece " + cls);
    p.style.width = width + "px";
    p.style.height = height + "px";
    return p;
  }
  // A colour ([r, g, b]) for CSS, at `alpha` where one is given.
  function rgb(c, alpha) {
    return alpha == null ? "rgb(" + c.join(",") + ")" : "rgba(" + c.join(",") + "," + alpha + ")";
  }
  // `b` laid `k` over `a` (view.rs `over`).
  function mix(a, b, k) {
    return a.map(function (v, i) { return Math.round(v + (b[i] - v) * k); });
  }

  // How long ago `then` was at `now` (ms), as Glance says it (text.rs `since`): just now; minutes ago, within the hour;
  // the time of day, today; yesterday; else the day.
  function since(then, now, w) {
    var ago = now - then;
    if (ago < MINUTE) return w.justNow;
    if (ago < 60 * MINUTE) return w.ago(Math.floor(ago / MINUTE));
    var at = new Date(then), today = new Date(now), yesterday = new Date(now - DAY);
    var same = function (a, b) { return a.getMonth() === b.getMonth() && a.getDate() === b.getDate(); };
    if (same(at, today) && ago < DAY) return clock(at);
    if (same(at, yesterday) && ago < 2 * DAY) return w.yesterday;
    return w.day(at);
  }
  function two(n) { return (n < 10 ? "0" : "") + n; }
  function clock(d) { return two(d.getHours()) + ":" + two(d.getMinutes()); }

  // ---- the pictures of things: neutral marks in the spirit of the shell's icons ----
  function art(kind, side) {
    var s = svg("svg", { viewBox: "0 0 32 32", width: side, height: side, "aria-hidden": "true" });
    var add = function (tag, attrs) { s.appendChild(svg(tag, attrs)); };
    var id = "gb" + kind + Math.random().toString(36).slice(2, 7);
    var defs = svg("defs", {});
    s.appendChild(defs);
    var grad = function (name, a, b, vertical) {
      var g = svg("linearGradient", { id: id + name, x1: 0, y1: 0, x2: vertical ? 0 : 1, y2: 1 });
      g.appendChild(svg("stop", { offset: 0, "stop-color": a }));
      g.appendChild(svg("stop", { offset: 1, "stop-color": b }));
      defs.appendChild(g);
      return "url(#" + id + name + ")";
    };
    if (kind === "folder") {
      add("path", { d: "M3 8.5a2.5 2.5 0 0 1 2.5-2.5h6.3a2 2 0 0 1 1.5.7l1.6 1.8h11.6A2.5 2.5 0 0 1 29 11v1H3z", fill: "#e2a42b" });
      add("rect", { x: 3, y: 10.5, width: 26, height: 16, rx: 2.4, fill: grad("f", "#ffd862", "#f6b73c", true) });
      add("rect", { x: 3, y: 10.5, width: 26, height: 1.4, rx: 0.7, fill: "#fff1b8", opacity: 0.7 });
    } else if (kind === "browser") {
      add("circle", { cx: 16, cy: 16, r: 13, fill: grad("b", "#4fc3f7", "#1565c0", false) });
      add("path", { d: "M3 16h26M16 3c-4 3.6-5.6 8-5.6 13s1.6 9.4 5.6 13M16 3c4 3.6 5.6 8 5.6 13S20 25.4 16 29M5.2 9.5h21.6M5.2 22.5h21.6", fill: "none", stroke: "#e3f4ff", "stroke-width": 1.3, opacity: 0.85 });
    } else if (kind === "calculator") {
      add("rect", { x: 6, y: 3, width: 20, height: 26, rx: 3, fill: grad("c", "#6b7684", "#454e5a", true) });
      add("rect", { x: 9, y: 6, width: 14, height: 5.5, rx: 1.2, fill: "#bfe6f5" });
      for (var r = 0; r < 3; r++) for (var c = 0; c < 3; c++) {
        add("rect", { x: 9 + c * 5, y: 14 + r * 4.6, width: 4, height: 3.4, rx: 0.8, fill: c === 2 && r === 2 ? "#ff9f43" : "#e7ebf0" });
      }
    } else if (kind === "notebook") {
      add("rect", { x: 6, y: 4, width: 20, height: 25, rx: 2.4, fill: grad("n", "#e9f6ff", "#c6e3f7", true) });
      add("rect", { x: 6, y: 4, width: 20, height: 5, rx: 2.4, fill: "#3d8bd9" });
      add("path", { d: "M10 14h12M10 18h12M10 22h8", stroke: "#3d8bd9", "stroke-width": 1.5, "stroke-linecap": "round" });
      add("path", { d: "M11 2.5v4M16 2.5v4M21 2.5v4", stroke: "#5c6b7a", "stroke-width": 1.5, "stroke-linecap": "round" });
    } else if (kind === "terminal") {
      add("rect", { x: 3, y: 5, width: 26, height: 22, rx: 3, fill: "#1d2027" });
      add("rect", { x: 3, y: 5, width: 26, height: 5, rx: 3, fill: "#3a3f4b" });
      add("rect", { x: 3, y: 8, width: 26, height: 2, fill: "#3a3f4b" });
      add("path", { d: "M8 15l4 3-4 3M14 22h7", fill: "none", stroke: "#e8ecf2", "stroke-width": 1.7, "stroke-linecap": "round", "stroke-linejoin": "round" });
    } else if (kind === "sheet") {
      add("path", { d: "M8 3h11l7 7v17a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z", fill: "#f7f9fb" });
      add("path", { d: "M19 3v5a2 2 0 0 0 2 2h5z", fill: "#cfd7df" });
      add("rect", { x: 9, y: 13, width: 14, height: 12, rx: 1, fill: "#2fa36b" });
      add("path", { d: "M9 17h14M9 21h14M14 13v12M18.5 13v12", stroke: "#f7f9fb", "stroke-width": 1 });
    }
    return s;
  }

  function GlanceDeskBench(lang) {
    var w = WORDS[lang === "en" ? "en" : "zh"];
    var made = Date.now();
    // How old each made-up thing is said to be, by where it is shown ([element, age (ms), what comes before]): said
    // again as time goes, and anew as its page is.
    var ages = { shelf: [], notes: [] };
    var root = el("div", "gb");
    root.setAttribute("lang", lang === "en" ? "en" : "zh-CN");
    // The page and the notes' view shown, kept as it is laid out anew for another look.
    var shown = 0, notesShown = 0;
    // The look it is laid out for, its measures, and what each part is laid out with (made anew with the look).
    var look, t, glass, column, left, width, row, tall, height;
    var figure, date, tabs, pill, tabButtons, view, probe, page, station, room, shows, showPill, showButtons;

    // ---- the look's ways, as view.rs draws each skin's ----
    // How round a note's card is (view.rs `note_radius`).
    function noteRadius(small) {
      return glass ? (small ? 10 : 12) : 6;
    }
    // How round a thing's plate is, its smaller side `small` (view.rs `face_round`).
    function faceRound(small) {
      return glass ? Math.max(4, Math.min(10, small / 5)) : look === "win11" ? 4 : 3;
    }
    // What is written on a note's card (view.rs `note_inks`): on glass the tint's ink, else the sheet's own.
    function inks(colour) {
      if (!glass) return { title: "var(--gb-text)", words: "var(--gb-text2)", faint: "var(--gb-text3)" };
      var c = COLOURS[colour].ink;
      return { title: "rgb(" + c + ")", words: "rgba(" + c + ",0.84)", faint: "rgba(" + c + ",0.7)" };
    }
    // A note's colour as a dot, its middle `middle` down from `y`, its left at `x` (view.rs `note_dot`), and the room it
    // takes before its title.
    function dot(host, colour, x, middle) {
      var d = place(el("i", "gb-dot"), x, middle - 3.5);
      d.style.background = rgb(COLOURS[colour].mark);
      host.appendChild(d);
      return 15;
    }

    // ---- laid out for the look ----
    function build() {
      look = document.documentElement.getAttribute("data-look");
      t = LOOKS[look];
      glass = look === "glass";
      root.textContent = "";
      ages = { shelf: [], notes: [] };
      // The lanes' column beside the rail, inside the room round them all (view.rs `Layout::lanes`).
      column = COLUMN - 2 * t.outer[1];
      left = t.padX;
      width = column - 2 * t.padX;
      // A head's title's line (view.rs `paint_lane`, the head).
      row = t.padTop + (HEAD - t.title) / 2 - 1;
      // The lanes' heights, as their blocks add up (view.rs `Block::height`, `Lane::height`).
      var head = HEAD + HEAD_GAP;
      tall = {
        time: t.padTop + head + PLOT + t.padBottom,
        station: t.padTop + head + SHELF_GAP + STATION_TALL + t.padBottom,
        notes: t.padTop + head + SHELF_GAP + NOTES_TALL + t.padBottom
      };
      height = tall.time + t.gap + tall.station + t.gap + tall.notes;
      root.style.padding = t.outer[0] + "px " + t.outer[1] + "px " + t.outer[2] + "px";
      root.style.gap = t.gap + "px";

      rail();
      var col = el("div", "gb-col");
      col.style.width = column + "px";
      col.style.gap = t.gap + "px";
      root.appendChild(col);
      col.appendChild(timeLane());
      col.appendChild(stationLane());
      col.appendChild(notesLane());

      // (Till the page has laid the names out: about where the title's words' ink stands.)
      layHead(row + 0.7 * t.title, row + 1.08 * t.title);
      showPage(shown);
      showNotes(notesShown);
      tick();
      measureInk();
    }

    // A lane's head: its title, and what is said after it, small.
    function headline(lane, title) {
      var h = place(el("div", "gb-headline"), left, row);
      h.style.right = left + "px";
      h.appendChild(el("span", "title", title));
      var aside = el("span", "small");
      h.appendChild(aside);
      lane.appendChild(h);
      return aside;
    }

    // ---- quick launch, as the rail along the panel's edge ----
    function rail() {
      var bar = piece("gb-rail", RAIL_W, height);
      bar.setAttribute("role", "group");
      bar.setAttribute("aria-label", w.quick);
      var apps = place(el("div", "gb-apps"), 0, 8, RAIL_W, height - 16 - RAIL_FOOT);
      ["folder", "browser", "calculator", "notebook", "terminal"].forEach(function (kind, at) {
        var b = button("gb-app", w.apps[at]);
        b.title = w.apps[at];
        place(b, (RAIL_W - RAIL_CELL) / 2, at * RAIL_CELL);
        b.appendChild(art(kind, RAIL_SIDE));
        apps.appendChild(b);
      });
      // The place to add more: a plus in a dashed ring, its corners rounded as the skin rounds a small square
      // (view.rs `paint_adder`).
      var adder = button("gb-adder", w.add);
      var inset = (RAIL_CELL - RAIL_SIDE) / 2;
      place(adder, inset + (RAIL_W - RAIL_CELL) / 2, 5 * RAIL_CELL + inset);
      var ring = RAIL_SIDE * 0.92, round = glass ? RAIL_SIDE * 0.26 : look === "win11" ? 4 : 6, ringAt = (RAIL_SIDE - ring) / 2;
      var dashed = svg("svg", { width: RAIL_SIDE, height: RAIL_SIDE, "aria-hidden": "true", "class": "ic" });
      dashed.style.left = "0px"; dashed.style.top = "0px";
      dashed.appendChild(svg("rect", { x: ringAt, y: ringAt, width: ring, height: ring, rx: round, fill: "none", stroke: "currentColor", "stroke-width": 1.2, "stroke-dasharray": "2.4 2.4", "stroke-linecap": "butt" }));
      adder.appendChild(dashed);
      var plus = icon("add", RAIL_SIDE / 2, RAIL_SIDE / 2, RAIL_SIDE * 0.45);
      plus.setAttribute("class", "ic plus");
      adder.appendChild(plus);
      apps.appendChild(adder);
      bar.appendChild(apps);
      // Its foot: the pin and the settings, as the bar has them.
      [["pin", w.pin], ["settings", w.settings]].forEach(function (b, at) {
        var foot = button("gb-btn gb-foot", b[1]);
        place(foot, (RAIL_W - BUTTON) / 2, height - 8 - (RAIL_FOOT - 8) + at * (BUTTON + BUTTON_GAP));
        foot.appendChild(icon(b[0], BUTTON / 2, BUTTON / 2, 16));
        bar.appendChild(foot);
      });
      root.appendChild(bar);
    }

    // ---- the time: its name and the day, the time of day standing on the lane's foot ----
    function timeLane() {
      var lane = piece("gb-lane", column, tall.time);
      date = headline(lane, w.time);
      figure = el("div", "gb-figure");
      // (Its line box `figureLine` of its size, on the bottom of the readout's room: view.rs `text_on_line`.)
      var line = t.figure * t.figureLine;
      place(figure, left, t.padTop + HEAD + HEAD_GAP + PLOT - line);
      figure.style.fontSize = t.figure + "px";
      figure.style.lineHeight = line + "px";
      lane.appendChild(figure);
      return lane;
    }

    // ---- the station ----
    function stationLane() {
      station = piece("gb-lane", column, tall.station);
      var headTall = t.padTop + HEAD + HEAD_GAP;
      tabs = place(el("div", "gb-tabs"), left - t.tabPad, 0, null, headTall);
      tabs.style.gap = t.tabGap + "px";
      tabs.setAttribute("role", "tablist");
      pill = el("div", "gb-pill");
      tabs.appendChild(pill);
      var pages = [{ name: w.station, count: "5" }, { name: w.copied, count: "" }];
      tabButtons = pages.map(function (one, at) {
        var b = el("button", "gb-tab");
        b.type = "button";
        b.setAttribute("role", "tab");
        b.style.height = headTall + "px";
        b.style.padding = row + "px " + t.tabPad + "px 0";
        b.appendChild(el("span", "n", one.name));
        if (one.count) b.appendChild(el("span", "c", one.count));
        b.addEventListener("click", function () { showPage(at); });
        tabs.appendChild(b);
        return b;
      });
      station.appendChild(tabs);
      view = place(button("gb-view", w.view), left + width - (MEDIA_ROW + 12), 0, MEDIA_ROW + 12, headTall);
      view.appendChild(icon("grid", MEDIA_ROW / 2, 0, 16));
      view.appendChild(icon("chevron", MEDIA_ROW + 3, 0.5, 10));
      station.appendChild(view);
      // (The ink of the pages' names, measured where the page has laid them out: the pill and the view button stand
      // about its middle, the mark under the chosen one so far under its foot, as Glance's do (see `paint_tabs`).)
      probe = text(w.inkProbe, "title", left, row);
      probe.style.visibility = "hidden";
      station.appendChild(probe);
      page = place(el("div", "gb-page"), left - 4, headTall + SHELF_GAP - 2, width + 8, STATION_TALL + 4);
      station.appendChild(page);
      return station;
    }

    function showPage(at) {
      shown = at;
      tabButtons.forEach(function (b, i) { b.setAttribute("aria-selected", i === at ? "true" : "false"); });
      placePill();
      page.textContent = "";
      ages.shelf = [];
      if (at === 0) shelf(); else copies();
    }
    // The chosen page's mark, as the skin marks what is chosen (view.rs `glass_choice`, `choice_mark`): glass's a pill
    // round it, Windows 11's a short bar of its accent under its middle, chart paper's a line of ink under all of it.
    function placePill() {
      var b = tabButtons[shown];
      if (!b.offsetWidth) return;
      var x = b.offsetLeft, wide = b.offsetWidth;
      if (look === "win11") { x += wide / 2 - 8; wide = 16; }
      pill.style.left = x + "px";
      pill.style.width = wide + "px";
    }
    function measureInk() {
      if (!probe.offsetHeight) return;
      // The baseline, from the line's top; the ink about it, as the canvas measures the same words in the same face.
      var mark = el("span");
      mark.style.cssText = "display:inline-block;width:0;height:0;vertical-align:baseline";
      probe.appendChild(mark);
      var baseline = mark.offsetTop;
      probe.removeChild(mark);
      var ctx = document.createElement("canvas").getContext("2d");
      var style = getComputedStyle(probe);
      ctx.font = style.fontWeight + " " + style.fontSize + " " + style.fontFamily;
      var m = ctx.measureText(w.inkProbe);
      layHead(row + baseline + (m.actualBoundingBoxDescent - m.actualBoundingBoxAscent) / 2, row + baseline + m.actualBoundingBoxDescent);
    }
    // The pill and the view button about `middle`, the middle of the names' ink, the mark under them so far under
    // `foot`, the ink's foot (view.rs `paint_tabs`).
    function layHead(middle, foot) {
      station.style.setProperty("--gb-row", (middle - TAB_H / 2) + "px");
      pill.style.top = (glass ? middle - TAB_H / 2 : foot + 4) + "px";
      Array.prototype.forEach.call(view.querySelectorAll(".ic"), function (s, i) {
        var size = i ? 10 : 16;
        s.style.top = (middle + (i ? 0.5 : 0) - size / 2) + "px";
      });
      placePill();
    }

    // What is on the station, newest first: a card each, three across.
    var things = [
      { face: "folder", name: w.things.folder, kind: w.file, ago: 12 },
      { face: "link", name: "glancepc.com", kind: w.link, ago: 30 },
      { face: "sheet", name: w.things.sheet, kind: w.files(2), ago: 45, more: 1 },
      { face: "text", text: w.things.text, ago: 180, pinned: true },
      { face: "picture", name: w.things.picture, kind: w.file, ago: 300 }
    ];
    function shelf() {
      var across = Math.max(1, Math.min(4, Math.floor((width + TILE_GAP) / (TILE_W + 24 + TILE_GAP))));
      var cw = (width - (across - 1) * TILE_GAP) / across;
      var plate = cw * PLATE;
      var ch = plate + CARD_WORDS;
      var gapX = across > 1 ? (width - across * cw) / (across - 1) : 0;
      things.forEach(function (thing, at) {
        // (Within the page's clip, which reaches 4 past the room's sides and 2 above it.)
        var tile = place(el("div", "gb-tile"), 4 + (at % across) * (cw + gapX), 2 + CARDS_LEAD + Math.floor(at / across) * (ch + TILE_GAP), cw, ch);
        // (Under the pointer, a plate round it, concentric with its face's on glass: view.rs `plate_round`.)
        if (glass) tile.style.setProperty("--gb-round", (faceRound(Math.min(cw, plate)) + 4) + "px");
        var face = place(el("div", "gb-face"), 0, 0, cw, plate);
        tile.appendChild(face);
        paintFace(face, thing, cw, plate);
        var name = thing.face === "text" ? thing.text.split(/\s+/).join(" ") : thing.name;
        var kind = thing.face === "text" ? w.chars(thing.text.replace(/\s/g, "").length) : thing.kind;
        tile.title = name;
        tile.appendChild(text(name, "small gb-name", 0, plate + 5, cw));
        var about = text("", "small faint", 0, plate + 5 + LINE, cw);
        ages.shelf.push([about, thing.ago * MINUTE, kind + " · "]);
        tile.appendChild(about);
        page.appendChild(tile);
      });
      sayAges();
    }
    // A thing's face (view.rs `paint_face`): a picture filling its plate, words on it, a link's mark on it; a folder's or
    // another file's icon standing on the card; how many more files are with it; a pin where it is pinned.
    function paintFace(face, thing, fw, fh) {
      var small = Math.min(fw, fh);
      var radius = faceRound(small);
      var plate = function () {
        var p = el("div", "gb-plate");
        p.style.borderRadius = radius + "px";
        face.appendChild(p);
        return p;
      };
      if (thing.face === "folder" || thing.face === "sheet") {
        var side = Math.min(small * 0.62, 96);
        var mark = art(thing.face, side);
        mark.style.cssText = "position:absolute;left:" + (fw - side) / 2 + "px;top:" + (fh - side) / 2 + "px";
        face.appendChild(mark);
        if (thing.more && small >= 40) {
          // On the icon's corner, in the sheet's ink.
          var badge = el("div", "gb-badge", "+" + thing.more);
          var cornerX = fw / 2 + side / 2 + 4, cornerY = fh / 2 + side / 2;
          var middle = Math.min(cornerY - 8, fh - 8);
          badge.style.right = (fw - Math.min(cornerX, fw)) + "px";
          badge.style.top = (middle - 8) + "px";
          // (Figures stand from their line's top to its base: their middle is so far down it.)
          badge.style.paddingTop = (8 - 10.5 * 0.71) + "px";
          face.appendChild(badge);
        }
      } else if (thing.face === "link") {
        plate();
        var mark2 = icon("link", fw / 2, fh / 2, Math.min(small * 0.42, 28));
        mark2.style.color = "var(--gb-trace)";
        face.appendChild(mark2);
      } else if (thing.face === "picture") {
        var img = el("img");
        img.src = BASE + "img/bench-picture.webp";
        img.alt = "";
        img.draggable = false;
        plate().appendChild(img);
      } else if (thing.face === "text") {
        var p = plate();
        var size = small > 80 ? 12 : 11, step = size * 1.28;
        var lines = Math.floor((fh - 10) / step);
        thing.text.split("\n").filter(function (l) { return l.trim(); }).slice(0, lines).forEach(function (words, at) {
          var room = fw - 10 - (at === 0 && thing.pinned ? 14 : 0);
          var said = text(words.trim(), "", 5, 5 + at * step, room);
          said.style.fontSize = size + "px";
          p.appendChild(said);
        });
      }
      if (thing.pinned && small >= 40) {
        var pin = place(el("div", "gb-pinned"), fw - 9 - 8, 9 - 8);
        pin.appendChild(icon("pin", 8, 8, 10));
        face.appendChild(pin);
      }
    }
    // What was copied lately: nothing yet.
    function copies() {
      var middle = 2 + STATION_TALL / 2;
      var mark = icon("clipboard", 4 + width / 2, middle - 12, 22);
      mark.style.color = "var(--gb-text3)";
      page.appendChild(mark);
      var said = text(w.copiedEmpty, "small gb-empty", 4, middle + 4);
      said.style.width = width + "px";
      page.appendChild(said);
    }

    // ---- the notes ----
    // (Glass's cards' shadows reach past them: room for them either side, and under (view.rs `paint_notes`).)
    var reach;
    function notesLane() {
      var lane = piece("gb-lane", column, tall.notes);
      headline(lane, w.notes).textContent = String(w.notesHeld.length);
      var buttonTop = t.padTop + (HEAD - MEDIA_ROW) / 2;
      var cell = MEDIA_ROW + 2;
      var x0 = left + width - MEDIA_ROW - 8 - (3 * cell + 4);
      shows = place(el("div", "gb-shows"), x0 + 2, buttonTop, 3 * cell, MEDIA_ROW);
      shows.setAttribute("role", "group");
      showPill = el("div", "gb-pill");
      shows.appendChild(showPill);
      showButtons = ["layers", "grid", "list"].map(function (name, at) {
        var b = button("gb-show", w.shows[at]);
        b.title = w.shows[at];
        b.style.left = (at * cell) + "px";
        b.appendChild(icon(name, cell / 2, MEDIA_ROW / 2, 16));
        b.addEventListener("click", function () { showNotes(at); });
        shows.appendChild(b);
        return b;
      });
      lane.appendChild(shows);
      var newNote = place(button("gb-btn gb-new", w.newNote), left + width - MEDIA_ROW, buttonTop, MEDIA_ROW, MEDIA_ROW);
      newNote.appendChild(icon("add", MEDIA_ROW / 2, MEDIA_ROW / 2, 15));
      lane.appendChild(newNote);
      reach = glass ? 12 : 4;
      var notesTop = t.padTop + HEAD + HEAD_GAP + SHELF_GAP;
      room = place(el("div", "gb-notes"), left - reach, notesTop - 2, width + 2 * reach, NOTES_TALL + reach);
      lane.appendChild(room);
      return lane;
    }

    // The notes, the last written first.
    var notes = w.notesHeld;
    function showNotes(at) {
      notesShown = at;
      showButtons.forEach(function (b, i) { b.setAttribute("aria-pressed", i === at ? "true" : "false"); });
      // The view shown marked as the station's page is (view.rs `paint_lane`, the notes' head): on glass a pill round
      // it, else a mark under it (`choice_mark`, its bar 14 long, chart paper's line 4 in from its ends).
      var cell = MEDIA_ROW + 2;
      if (glass) {
        showPill.style.left = (at * cell) + "px";
        showPill.style.width = cell + "px";
        showPill.style.top = "0px";
      } else {
        showPill.style.left = (look === "win11" ? at * cell + cell / 2 - 7 : at * cell + 4) + "px";
        showPill.style.width = (look === "win11" ? 14 : cell - 8) + "px";
        showPill.style.top = (MEDIA_ROW + 1) + "px";
      }
      paintNotes();
    }
    // A note's coloured glass: a pane of white under its tint (in the dark), as view.rs `note_card_fill` lays it.
    function ground(colour) {
      var c = COLOURS[colour].tint;
      return "linear-gradient(rgba(" + c + ",0.58),rgba(" + c + ",0.58)),rgba(255,255,255,0.55)";
    }
    // A card at (x, y), w × h, in the notes' room, as the skin draws it (view.rs `note_card_ground`): glass's of the
    // note's coloured glass, over it, frosted, the plain panes of `below` (each {colour, x, y, w, h}). Windows 11's its
    // sheet a little of the colour, a band of it along its top. Chart paper's its paper.
    function card(note, x, y, cw, chh, small, below) {
      var c = place(el("div", "gb-card"), x + reach, y + 2, cw, chh);
      c.style.borderRadius = noteRadius(small) + "px";
      c.style.setProperty("--mark", COLOURS[note.colour].mark.join(","));
      if (glass && below && below.length) {
        var under = place(el("div", "under"), 0, 0, cw, chh);
        below.forEach(function (b) {
          var pane = place(el("div"), b.x - x, b.y - y, b.w, b.h);
          pane.style.background = ground(b.colour);
          under.appendChild(pane);
        });
        c.appendChild(under);
      }
      var tint = el("div", "tint");
      if (glass) tint.style.background = ground(note.colour);
      else if (look === "win11") tint.style.backgroundColor = rgb(mix(FLUENT_NOTE, COLOURS[note.colour].mark, 0.16), 0.94);
      else tint.style.backgroundColor = rgb(mix(PAPER, PAPER_INK, 0.08));
      c.appendChild(tint);
      if (look === "win11") {
        var band = el("div", "band");
        band.style.background = rgb(COLOURS[note.colour].mark);
        c.appendChild(band);
      }
      return c;
    }
    function paintNotes() {
      room.textContent = "";
      ages.notes = [];
      if (notesShown === 0) deck();
      else if (notesShown === 1) cards();
      else list();
      sayAges();
    }
    // (Under Windows 11's band, what is on its card a little lower.)
    function banded() { return look === "win11" ? 2 : 0; }
    // The others' strips stacked over the last written (on glass and chart paper each narrower the farther back it
    // lies), the last written whole.
    function deck() {
      var others = notes.length - 1;
      var stack = Math.min(others * NOTE_STRIP, Math.max(0, NOTES_TALL - NOTE_CARD));
      var radius = noteRadius(false), foot = radius + 6;
      var recede = glass ? radius * 2 / 3 : look === "paper" ? 8 : 0;
      var stripY = function (at) { return (at - 1) * NOTE_STRIP; };
      var across = function (at) {
        var back = recede * Math.max(0, (stack - stripY(at)) / NOTE_STRIP);
        return { x: back, w: width - 2 * back };
      };
      var plain = function (at) { var a = across(at); return { colour: notes[at].colour, x: a.x, y: stripY(at), w: a.w, h: NOTE_STRIP + foot }; };
      for (var at = 1; at <= others; at++) {
        if (stripY(at) > stack) break;
        var a = across(at);
        var below = [];
        for (var b = 1; b < at; b++) below.push(plain(b));
        var strip = card(notes[at], a.x, stripY(at), a.w, NOTE_STRIP + foot, false, below);
        strip.classList.add("strip");
        var ink = inks(notes[at].colour);
        var line = el("div", "gb-strip ink");
        line.style.top = ((NOTE_STRIP - LINE) / 2 + banded()) + "px";
        if (look === "paper") dot(line, notes[at].colour, 14, LINE / 2);
        var title = el("b", null, notes[at].title);
        title.style.color = ink.title;
        if (look === "paper") title.style.marginLeft = "15px";
        var when = el("span");
        when.style.color = ink.faint;
        ages.notes.push([when, notes[at].ago, ""]);
        line.appendChild(title);
        line.appendChild(when);
        strip.appendChild(line);
        strip.title = notes[at].title;
        // On glass, up to the pane lying on it: under that it is seen only through it, frosted.
        if (glass) {
          var covered = at + 1 < notes.length ? stripY(at + 1) : stack;
          var clip = place(el("div", "gb-clip"), 0, 0, width + 2 * reach, covered + 2);
          clip.appendChild(strip);
          room.appendChild(clip);
        } else room.appendChild(strip);
      }
      var shownStrips = [];
      for (var s = 1; s <= others && stripY(s) <= stack; s++) shownStrips.push(plain(s));
      var first = card(notes[0], 0, stack, width, NOTES_TALL - stack, false, shownStrips);
      var fi = inks(notes[0].colour);
      // (view.rs `paint_front_note`: before when it was written, chart paper's dot, Windows 11's a note's mark.)
      var x = glass ? 16 : 14, top = (glass ? 14 : 12) + banded(), words = width - x - 14;
      var lead = 0;
      if (look === "paper") lead = dot(first, notes[0].colour, x, top + LINE / 2);
      else if (look === "win11") {
        var mark = icon("file", x + 6, top + LINE / 2, 12);
        mark.style.color = "var(--gb-text3)";
        first.appendChild(mark);
        lead = 18;
      }
      var said = text("", "small ink", x + lead, top + 1, words - lead);
      said.style.color = look === "win11" ? "var(--gb-text2)" : fi.faint;
      ages.notes.push([said, notes[0].ago, ""]);
      first.appendChild(said);
      var size = t.title + 1;
      var heading = text(notes[0].title, "ink", x, top + LINE + 6, words);
      heading.style.cssText += ";font-size:" + size + "px;font-weight:700;color:" + fi.title;
      first.appendChild(heading);
      notes[0].lines.forEach(function (l, i) {
        var y = top + LINE + 6 + size * 1.4 + 4 + i * (LINE + 4);
        if (y + LINE > NOTES_TALL - stack - 8) return;
        var lt = text(l, "ink", x, y, words);
        lt.style.color = fi.words;
        first.appendChild(lt);
      });
      room.appendChild(first);
    }
    // Two across, each its title (after its dot, on chart paper), its first lines and when it was written.
    function cards() {
      var cw = (width - NOTE_CARDS_GAP) / 2;
      notes.forEach(function (note, at) {
        var c = card(note, (at % 2) * (cw + NOTE_CARDS_GAP), Math.floor(at / 2) * (NOTE_CARD + NOTE_CARDS_GAP), cw, NOTE_CARD, true);
        var ink = inks(note.colour);
        var inner = glass ? 13 : 11, top = 10 + banded(), wordsW = cw - inner - 10, whenY = NOTE_CARD - LINE - 8;
        var lead = look === "paper" ? dot(c, note.colour, inner, top + LINE / 2) : 0;
        var tt = text(note.title, "ink", inner + lead, top, wordsW - lead);
        tt.style.cssText += ";font-weight:700;color:" + ink.title;
        c.appendChild(tt);
        note.lines.forEach(function (l, i) {
          var y = top + LINE + 6 + i * (LINE + 1);
          if (y + LINE > whenY - 2) return;
          var lt = text(l, "small ink", inner, y, wordsW);
          lt.style.color = ink.words;
          c.appendChild(lt);
        });
        var when = text("", "small ink", inner, whenY, wordsW);
        when.style.color = ink.faint;
        ages.notes.push([when, note.ago, ""]);
        c.appendChild(when);
        c.title = note.title;
        room.appendChild(c);
      });
    }
    // A row each: its colour (Windows 11's a band of it, the others' a dot), its title over its first line, when it was
    // written at its end.
    function list() {
      notes.forEach(function (note, at) {
        var r = place(el("div", "gb-row"), reach, 2 + at * NOTE_ROW, width, NOTE_ROW);
        if (look === "win11") {
          var band = place(el("div", "band"), 0, 7, 4, NOTE_ROW - 14);
          band.style.background = rgb(COLOURS[note.colour].mark);
          r.appendChild(band);
        } else dot(r, note.colour, 0, (note.lines.length ? 4 : (NOTE_ROW - LINE) / 2) + LINE / 2);
        var words = el("div", "words");
        words.style.left = (look === "win11" ? 14 : 15) + "px";
        var lines = el("div", "lines");
        lines.appendChild(el("div", null, note.title));
        if (note.lines.length) lines.appendChild(el("div", null, note.lines[0]));
        else words.style.top = ((NOTE_ROW - LINE) / 2) + "px";
        words.appendChild(lines);
        var when = el("div", "when");
        if (!note.lines.length) when.style.marginTop = "0px";
        ages.notes.push([when, note.ago, ""]);
        words.appendChild(when);
        r.appendChild(words);
        r.title = note.title;
        room.appendChild(r);
      });
    }

    // ---- time going: the clock, the day, and how long ago each thing came ----
    function sayAges() {
      var now = Date.now();
      // (The made-up things keep their ages: made as the page was, they stay as old.)
      ages.shelf.concat(ages.notes).forEach(function (a) { a[0].textContent = a[2] + since(now - a[1], now, w); });
    }
    function tick() {
      var now = new Date();
      figure.textContent = clock(now);
      date.textContent = w.date(now);
      sayAges();
    }
    build();
    setTimeout(function step() {
      tick();
      setTimeout(step, 1000 - (Date.now() % 1000) + 5);
    }, 1000 - (made % 1000) + 5);

    // Laid out once it is on the page, and again as its faces come, and anew as the look changes.
    if (window.ResizeObserver) new ResizeObserver(function () { measureInk(); }).observe(root);
    if (document.fonts) {
      document.fonts.ready.then(measureInk);
      document.fonts.addEventListener("loadingdone", measureInk);
    }
    new MutationObserver(build).observe(document.documentElement, { attributes: true, attributeFilter: ["data-look"] });

    return { node: root };
  }

  // Where the site's assets are, from this script's own address.
  var BASE = (function () {
    var s = document.currentScript && document.currentScript.src;
    return s ? s.replace(/[^/]*$/, "") : "assets/";
  })();

  window.GlanceDesk = window.GlanceDesk || {};
  window.GlanceDesk.bench = function (lang) { return GlanceDeskBench(lang); };
  // A program's picture, `side` DIPs square, as the rail shows it (the desk's taskbar has the same ones), and their
  // names.
  window.GlanceDesk.app = function (kind, side) { return art(kind, side); };
  window.GlanceDesk.apps = { kinds: ["folder", "browser", "calculator", "notebook", "terminal"], names: function (lang) { return WORDS[lang === "en" ? "en" : "zh"].apps; } };
  // How frosted dark glass is over a desktop whose luma (0 to 1, as the eye weighs the picture's own values) is `mean`
  // on average, spread by `spread` (its standard deviation), as Glance works it out (skins.rs `frost`): more where what
  // is behind is busy, and more where it is brighter than dark glass reads over. For the root node's --gb-frost.
  window.GlanceDesk.frost = function (mean, spread) {
    var busy = Math.min(1, Math.max(0, (spread - 0.03) / (0.14 - 0.03)));
    return Math.max(busy, Math.min(1, Math.max(0, (mean - 0.4) / 0.35)));
  };
})();
