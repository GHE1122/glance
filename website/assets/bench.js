// Glance's workbench drawn in the page, as its glass skin draws it in the dark:
// quick launch as a rail at the panel's edge, the time, the station and the
// notes beside it, holding the made-up things Glance's studio films it with.
// Laid out in Glance's DIPs at panel size 1 (view.rs: the lanes' padding, heads,
// blocks and the rail's cells), at its natural size; where it stands is the
// page's to say (see desk.js).
(function () {
  "use strict";

  var SVG = "http://www.w3.org/2000/svg";

  // ---- Glance's measures (DIPs) ----
  var PAD_X = 18, PAD_TOP = 14, PAD_BOTTOM = 14;
  var HEAD = 17, HEAD_GAP = 8, SHELF_GAP = 6, LINE = 16;
  var GAP = 8, COLUMN = 356, RAIL_W = 60;
  var PLOT = 40, STATION_TALL = 284, NOTES_TALL = 250;
  var RAIL_CELL = 46, RAIL_SIDE = 32, BUTTON = 32, BUTTON_GAP = 4, RAIL_FOOT = 8 + 2 * BUTTON + BUTTON_GAP;
  var TILE_W = 72, TILE_GAP = 8, PLATE = 0.74, CARD_WORDS = 5 + 2 * LINE, CARDS_LEAD = 4;
  var NOTE_STRIP = 30, NOTE_ROW = 40, NOTE_CARD = 104, NOTE_CARDS_GAP = 8, NOTE_REACH = 12;
  var MEDIA_ROW = 24, TAB_H = 24;
  var FIGURE = 40, FIGURE_LINE = 0.84;
  var MINUTE = 60000, DAY = 24 * 60 * MINUTE;

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
    clipboard: ["M9 2h6a1 1 0 0 1 1 1v2a1 1 0 0 1-1 1H9a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1z", "M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"]
  };

  // A note's colours on glass: its tint, the ink written on it (view.rs `note_glass`), and its mark in a list.
  var COLOURS = {
    blue: { tint: "55,105,245", ink: "24,66,142", mark: "#5b8def" },
    yellow: { tint: "242,247,102", ink: "90,94,18", mark: "#e8b64a" },
    green: { tint: "104,189,129", ink: "31,94,51", mark: "#4fbf82" }
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
  function piece(cls, height) {
    var p = el("section", "gb-piece " + cls);
    p.style.height = height + "px";
    return p;
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

    // ---- the lanes' heights, as their blocks add up (view.rs `Block::height`) ----
    var head = HEAD + HEAD_GAP;
    var tall = {
      time: PAD_TOP + head + PLOT + PAD_BOTTOM,
      station: PAD_TOP + head + SHELF_GAP + STATION_TALL + PAD_BOTTOM,
      notes: PAD_TOP + head + SHELF_GAP + NOTES_TALL + PAD_BOTTOM
    };
    var height = tall.time + GAP + tall.station + GAP + tall.notes;

    // ---- quick launch, as the rail along the panel's edge ----
    var rail = piece("gb-rail", height);
    rail.setAttribute("aria-label", w.quick);
    var apps = place(el("div", "gb-apps"), 0, 8, RAIL_W, height - 16 - RAIL_FOOT);
    ["folder", "browser", "calculator", "notebook", "terminal"].forEach(function (kind, at) {
      var b = button("gb-app", w.apps[at]);
      b.title = w.apps[at];
      place(b, (RAIL_W - RAIL_CELL) / 2, at * RAIL_CELL);
      b.appendChild(art(kind, RAIL_SIDE));
      apps.appendChild(b);
    });
    // The place to add more: a plus in a dashed round.
    var adder = button("gb-adder", w.add);
    var inset = (RAIL_CELL - RAIL_SIDE) / 2;
    place(adder, inset + (RAIL_W - RAIL_CELL) / 2, 5 * RAIL_CELL + inset);
    var ring = RAIL_SIDE * 0.92, round = RAIL_SIDE * 0.26, ringAt = (RAIL_SIDE - ring) / 2;
    var dashed = svg("svg", { width: RAIL_SIDE, height: RAIL_SIDE, "aria-hidden": "true", "class": "ic" });
    dashed.style.left = "0px"; dashed.style.top = "0px";
    dashed.appendChild(svg("rect", { x: ringAt, y: ringAt, width: ring, height: ring, rx: round, fill: "none", stroke: "currentColor", "stroke-width": 1.2, "stroke-dasharray": "2.4 2.4", "stroke-linecap": "butt" }));
    adder.appendChild(dashed);
    var plus = icon("add", RAIL_SIDE / 2, RAIL_SIDE / 2, RAIL_SIDE * 0.45);
    plus.setAttribute("class", "ic plus");
    adder.appendChild(plus);
    apps.appendChild(adder);
    rail.appendChild(apps);
    // Its foot: the pin and the settings, as the bar has them.
    [["pin", w.pin], ["settings", w.settings]].forEach(function (b, at) {
      var foot = button("gb-btn gb-foot", b[1]);
      place(foot, (RAIL_W - BUTTON) / 2, height - 8 - (RAIL_FOOT - 8) + at * (BUTTON + BUTTON_GAP));
      foot.appendChild(icon(b[0], BUTTON / 2, BUTTON / 2, 16));
      rail.appendChild(foot);
    });
    root.appendChild(rail);

    var col = el("div", "gb-col");
    root.appendChild(col);

    // ---- the time: its name and the day, the time of day standing on the lane's foot ----
    var time = piece("gb-lane", tall.time);
    var timeHead = el("div", "gb-headline");
    timeHead.appendChild(el("span", "title", w.time));
    var date = el("span", "small");
    timeHead.appendChild(date);
    time.appendChild(timeHead);
    var figure = el("div", "gb-figure");
    // (Its line box FIGURE_LINE of its size, on the bottom of the readout's room: view.rs `text_on_line`.)
    var line = FIGURE * FIGURE_LINE;
    figure.style.top = (PAD_TOP + head + PLOT - line) + "px";
    time.appendChild(figure);
    col.appendChild(time);

    // ---- the station ----
    var station = piece("gb-lane", tall.station);
    var tabs = el("div", "gb-tabs");
    tabs.setAttribute("role", "tablist");
    var pill = el("div", "gb-pill");
    tabs.appendChild(pill);
    var pages = [{ name: w.station, count: "5" }, { name: w.copied, count: "" }];
    var tabButtons = pages.map(function (page, at) {
      var b = el("button", "gb-tab");
      b.type = "button";
      b.setAttribute("role", "tab");
      b.appendChild(el("span", "n", page.name));
      if (page.count) b.appendChild(el("span", "c", page.count));
      b.addEventListener("click", function () { showPage(at); });
      tabs.appendChild(b);
      return b;
    });
    station.appendChild(tabs);
    var view = button("gb-view", w.view);
    view.appendChild(icon("grid", MEDIA_ROW / 2, 0, 16));
    view.appendChild(icon("chevron", MEDIA_ROW + 3, 0.5, 10));
    station.appendChild(view);
    // (The ink of the pages' names, measured where the page has laid them out: the pill and the view button stand about
    // its middle, as Glance's do; see `paint_tabs`.)
    var probe = text(w.inkProbe, "title", PAD_X, PAD_TOP + (HEAD - 13) / 2 - 1);
    probe.style.visibility = "hidden";
    station.appendChild(probe);
    var page = el("div", "gb-page");
    station.appendChild(page);
    col.appendChild(station);

    var shown = 0;
    function showPage(at) {
      shown = at;
      tabButtons.forEach(function (b, i) { b.setAttribute("aria-selected", i === at ? "true" : "false"); });
      placePill();
      page.textContent = "";
      ages.shelf = [];
      if (at === 0) shelf(); else copies();
    }
    function placePill() {
      var b = tabButtons[shown];
      if (!b.offsetWidth) return;
      pill.style.left = b.offsetLeft + "px";
      pill.style.width = b.offsetWidth + "px";
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
      layHead(PAD_TOP + (HEAD - 13) / 2 - 1 + baseline + (m.actualBoundingBoxDescent - m.actualBoundingBoxAscent) / 2);
    }
    // The pill and the view button about `middle`, the middle of the names' ink.
    function layHead(middle) {
      station.style.setProperty("--gb-row", (middle - TAB_H / 2) + "px");
      pill.style.top = (middle - TAB_H / 2) + "px";
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
      var width = COLUMN - 2 * PAD_X;
      var across = Math.max(1, Math.min(4, Math.floor((width + TILE_GAP) / (TILE_W + 24 + TILE_GAP))));
      var cw = (width - (across - 1) * TILE_GAP) / across;
      var plate = cw * PLATE;
      var ch = plate + CARD_WORDS;
      var gapX = across > 1 ? (width - across * cw) / (across - 1) : 0;
      things.forEach(function (thing, at) {
        // (Within the page's clip, which reaches 4 past the room's sides and 2 above it.)
        var tile = place(el("div", "gb-tile"), 4 + (at % across) * (cw + gapX), 2 + CARDS_LEAD + Math.floor(at / across) * (ch + TILE_GAP), cw, ch);
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
      var radius = Math.max(4, Math.min(10, small / 5));
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
          var t = text(words.trim(), "", 5, 5 + at * step, room);
          t.style.fontSize = size + "px";
          p.appendChild(t);
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
      var mark = icon("clipboard", 4 + (COLUMN - 2 * PAD_X) / 2, middle - 12, 22);
      mark.style.color = "var(--gb-text3)";
      page.appendChild(mark);
      var said = text(w.copiedEmpty, "small gb-empty", 4, middle + 4);
      said.style.width = (COLUMN - 2 * PAD_X) + "px";
      page.appendChild(said);
    }

    // ---- the notes ----
    var notesLane = piece("gb-lane", tall.notes);
    var notesHead = el("div", "gb-headline");
    notesHead.appendChild(el("span", "title", w.notes));
    notesHead.appendChild(el("span", "small", String(w.notesHeld.length)));
    notesLane.appendChild(notesHead);
    var left = PAD_X, width = COLUMN - 2 * PAD_X;
    var buttonTop = PAD_TOP + (HEAD - MEDIA_ROW) / 2;
    var cell = MEDIA_ROW + 2;
    var x0 = left + width - MEDIA_ROW - 8 - (3 * cell + 4);
    var shows = place(el("div", "gb-shows"), x0 + 2, buttonTop, 3 * cell, MEDIA_ROW);
    shows.setAttribute("role", "group");
    var showPill = el("div", "gb-pill");
    showPill.style.top = "0px";
    shows.appendChild(showPill);
    var showButtons = ["layers", "grid", "list"].map(function (name, at) {
      var b = button("gb-show", w.shows[at]);
      b.title = w.shows[at];
      b.style.left = (at * cell) + "px";
      b.appendChild(icon(name, cell / 2, MEDIA_ROW / 2, 16));
      b.addEventListener("click", function () { showNotes(at); });
      shows.appendChild(b);
      return b;
    });
    notesLane.appendChild(shows);
    var newNote = button("gb-btn gb-new", w.newNote);
    newNote.appendChild(icon("add", MEDIA_ROW / 2, MEDIA_ROW / 2, 15));
    notesLane.appendChild(newNote);
    // (The notes' clip: room either side and under for the cards' shadows.)
    var notesTop = PAD_TOP + head + SHELF_GAP;
    var room = place(el("div", "gb-notes"), left - NOTE_REACH, notesTop - 2, width + 2 * NOTE_REACH, NOTES_TALL + NOTE_REACH);
    notesLane.appendChild(room);
    col.appendChild(notesLane);

    // The notes, the last written first.
    var notes = w.notesHeld;
    var notesShown = 0;
    function showNotes(at) {
      notesShown = at;
      showButtons.forEach(function (b, i) { b.setAttribute("aria-pressed", i === at ? "true" : "false"); });
      showPill.style.left = (at * cell) + "px";
      showPill.style.width = cell + "px";
      paintNotes();
    }
    // A note's coloured glass: a pane of white under its tint (in the dark), as view.rs `note_card_fill` lays it.
    function ground(colour) {
      var c = COLOURS[colour].tint;
      return "linear-gradient(rgba(" + c + ",0.58),rgba(" + c + ",0.58)),rgba(255,255,255,0.55)";
    }
    function inks(colour) {
      var c = COLOURS[colour].ink;
      return { title: "rgb(" + c + ")", words: "rgba(" + c + ",0.84)", faint: "rgba(" + c + ",0.7)" };
    }
    // A card at (x, y), w × h, in the notes' room; over it, frosted, the plain panes of `below` (each {colour, x, y, w, h}).
    function card(note, x, y, cw, chh, small, below) {
      var c = place(el("div", "gb-card" + (small ? " small" : "")), x + NOTE_REACH, y + 2, cw, chh);
      if (below && below.length) {
        var under = place(el("div", "under"), 0, 0, cw, chh);
        below.forEach(function (b) {
          var pane = place(el("div"), b.x - x, b.y - y, b.w, b.h);
          pane.style.background = ground(b.colour);
          under.appendChild(pane);
        });
        c.appendChild(under);
      }
      var tint = el("div", "tint");
      tint.style.background = ground(note.colour);
      c.appendChild(tint);
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
    // The others' strips stacked over the last written, each narrower the farther back it lies; the last written whole.
    function deck() {
      var others = notes.length - 1;
      var stack = Math.min(others * NOTE_STRIP, Math.max(0, NOTES_TALL - NOTE_CARD));
      var radius = 12, foot = radius + 6, recede = radius * 2 / 3;
      var stripY = function (at) { return (at - 1) * NOTE_STRIP; };
      var across = function (at) {
        var back = recede * Math.max(0, (stack - stripY(at)) / NOTE_STRIP);
        return { x: back, w: width - 2 * back };
      };
      var plain = function (at) { var a = across(at); return { colour: notes[at].colour, x: a.x, y: stripY(at), w: a.w, h: NOTE_STRIP + foot }; };
      for (var at = 1; at <= others; at++) {
        if (stripY(at) > stack) break;
        // Up to the pane lying on it: under that it is seen only through it, frosted.
        var covered = at + 1 < notes.length ? stripY(at + 1) : stack;
        var clip = place(el("div", "gb-clip"), 0, 0, width + 2 * NOTE_REACH, covered + 2);
        var a = across(at);
        var below = [];
        for (var b = 1; b < at; b++) below.push(plain(b));
        var strip = card(notes[at], a.x, stripY(at), a.w, NOTE_STRIP + foot, false, below);
        strip.classList.add("strip");
        var ink = inks(notes[at].colour);
        var row = el("div", "gb-strip ink");
        var title = el("b", null, notes[at].title);
        title.style.color = ink.title;
        var when = el("span");
        when.style.color = ink.faint;
        ages.notes.push([when, notes[at].ago, ""]);
        row.appendChild(title);
        row.appendChild(when);
        strip.appendChild(row);
        strip.title = notes[at].title;
        clip.appendChild(strip);
        room.appendChild(clip);
      }
      var shownStrips = [];
      for (var s = 1; s <= others && stripY(s) <= stack; s++) shownStrips.push(plain(s));
      var first = card(notes[0], 0, stack, width, NOTES_TALL - stack, false, shownStrips);
      var fi = inks(notes[0].colour);
      var x = 16, top = 14, words = width - x - 14;
      var said = text("", "small ink", x, top + 1, words);
      said.style.color = fi.faint;
      ages.notes.push([said, notes[0].ago, ""]);
      first.appendChild(said);
      var heading = text(notes[0].title, "ink", x, top + LINE + 6, words);
      heading.style.cssText += ";font-size:14px;font-weight:700;color:" + fi.title;
      first.appendChild(heading);
      notes[0].lines.forEach(function (l, i) {
        var y = top + LINE + 6 + 14 * 1.4 + 4 + i * (LINE + 4);
        if (y + LINE > NOTES_TALL - stack - 8) return;
        var t = text(l, "ink", x, y, words);
        t.style.color = fi.words;
        first.appendChild(t);
      });
      room.appendChild(first);
    }
    // Two across, each its title, its first lines and when it was written.
    function cards() {
      var cw = (width - NOTE_CARDS_GAP) / 2;
      notes.forEach(function (note, at) {
        var c = card(note, (at % 2) * (cw + NOTE_CARDS_GAP), Math.floor(at / 2) * (NOTE_CARD + NOTE_CARDS_GAP), cw, NOTE_CARD, true);
        var ink = inks(note.colour);
        var inner = 13, top = 10, wordsW = cw - inner - 10, whenY = NOTE_CARD - LINE - 8;
        var t = text(note.title, "ink", inner, top, wordsW);
        t.style.cssText += ";font-weight:700;color:" + ink.title;
        c.appendChild(t);
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
    // A row each: its colour's dot, its title over its first line, when it was written at its end.
    function list() {
      notes.forEach(function (note, at) {
        var r = place(el("div", "gb-row"), NOTE_REACH, 2 + at * NOTE_ROW, width, NOTE_ROW);
        var dot = el("div", "dot");
        dot.style.background = COLOURS[note.colour].mark;
        dot.style.top = (4 + LINE / 2 - 3.5) + "px";
        r.appendChild(dot);
        var words = el("div", "words");
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
    tick();
    setTimeout(function step() {
      tick();
      setTimeout(step, 1000 - (Date.now() % 1000) + 5);
    }, 1000 - (made % 1000) + 5);

    // (Till the page has laid the names out: about where 13 DIP words' ink stands.)
    layHead(PAD_TOP + (HEAD - 13) / 2 - 1 + 9);
    showPage(0);
    showNotes(0);
    // Laid out once it is on the page, and again as its faces come.
    if (window.ResizeObserver) new ResizeObserver(measureInk).observe(tabs);
    if (document.fonts) {
      document.fonts.ready.then(measureInk);
      document.fonts.addEventListener("loadingdone", measureInk);
    }

    return { node: root, width: RAIL_W + GAP + COLUMN, height: height };
  }

  // Where the site's assets are, from this script's own address.
  var BASE = (function () {
    var s = document.currentScript && document.currentScript.src;
    return s ? s.replace(/[^/]*$/, "") : "assets/";
  })();

  window.GlanceDesk = window.GlanceDesk || {};
  window.GlanceDesk.bench = function (lang) { return GlanceDeskBench(lang); };
  // How frosted dark glass is over a desktop whose luma (0 to 1, as the eye weighs the picture's own values) is `mean`
  // on average, spread by `spread` (its standard deviation), as Glance works it out (skins.rs `frost`): more where what
  // is behind is busy, and more where it is brighter than dark glass reads over. For the root node's --gb-frost.
  window.GlanceDesk.frost = function (mean, spread) {
    var busy = Math.min(1, Math.max(0, (spread - 0.03) / (0.14 - 0.03)));
    return Math.max(busy, Math.min(1, Math.max(0, (mean - 0.4) / 0.35)));
  };
})();
