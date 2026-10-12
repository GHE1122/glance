(function () {
  "use strict";
  var $ = function (s) { return document.querySelector(s); };
  var KEY = "glance-stats-key";
  var store = {
    get: function (k) { try { return localStorage.getItem(k); } catch (e) { return null; } },
    set: function (k, v) { try { localStorage.setItem(k, v); } catch (e) { /* kept for this visit only */ } }
  };
  var key = store.get(KEY), days = 30;
  var names = (window.Intl && Intl.DisplayNames) ? new Intl.DisplayNames(["zh-CN"], { type: "region" }) : null;
  var place = function (code) { if (!code) return "未知"; try { return names ? names.of(code) : code; } catch (e) { return code; } };
  var fmt = function (n) { return (n || 0).toLocaleString("zh-CN"); };
  var pct = function (a, b) { return b ? (100 * a / b).toFixed(1) + "%" : "—"; };
  var esc = function (s) { return String(s).replace(/[&<>"]/g, function (c) { return { "&": "&amp;", "<": "&lt;", ">": "&gt;", "\"": "&quot;" }[c]; }); };

  function table(el, head, rows) {
    if (!rows.length) { el.innerHTML = "<tr><td class=\"empty\">还没有数据</td></tr>"; return; }
    el.innerHTML = "<tr>" + head.map(function (h, i) { return "<th" + (i ? " class=\"num\"" : "") + ">" + h + "</th>"; }).join("") + "</tr>" +
      rows.map(function (r) { return "<tr>" + r.map(function (c, i) { return "<td" + (i ? " class=\"num\"" : "") + ">" + esc(c) + "</td>"; }).join("") + "</tr>"; }).join("");
  }

  // Every day of the range, the empty ones too.
  function everyDay(since, n, rows, keys) {
    var by = {}; rows.forEach(function (r) { by[r.day] = r; });
    var out = [], t = Date.parse(since + "T00:00:00Z");
    for (var i = 0; i < n; i++) {
      var day = new Date(t + i * 86400000).toISOString().slice(0, 10), none = { day: day };
      keys.forEach(function (k) { none[k] = 0; });
      out.push(by[day] || none);
    }
    return out;
  }

  // The lines of `keys` over the days, and under the pointer what `told` says of that day.
  function chart(host, rows, keys, told) {
    var svg = host.querySelector("svg"), tip = host.querySelector(".tip");
    var W = host.clientWidth, H = 260, L = 36, R = 8, T = 8, B = 24;
    var most = Math.max(4, Math.max.apply(null, rows.map(function (r) { return Math.max.apply(null, keys.map(function (k) { return r[k] || 0; })); })));
    var step = Math.pow(10, Math.floor(Math.log10(most))); var top = Math.ceil(most / step) * step;
    var x = function (i) { return L + (rows.length > 1 ? (W - L - R) * i / (rows.length - 1) : (W - L - R) / 2); };
    var y = function (v) { return T + (H - T - B) * (1 - v / top); };
    var parts = [];
    for (var g = 0; g <= 4; g++) {
      var v = top * g / 4, gy = y(v);
      parts.push("<line class=\"grid\" x1=\"" + L + "\" x2=\"" + (W - R) + "\" y1=\"" + gy + "\" y2=\"" + gy + "\"/>");
      parts.push("<text class=\"axis\" x=\"" + (L - 6) + "\" y=\"" + (gy + 4) + "\" text-anchor=\"end\">" + fmt(Math.round(v)) + "</text>");
    }
    var every = Math.ceil(rows.length / 8);
    rows.forEach(function (r, i) {
      // Every few days, and the last; one too near the last gives way to it.
      var last = i === rows.length - 1;
      if (last || (i % every === 0 && rows.length - 1 - i >= every / 2)) parts.push("<text class=\"axis\" x=\"" + x(i) + "\" y=\"" + (H - 6) + "\" text-anchor=\"middle\">" + r.day.slice(5) + "</text>");
    });
    keys.forEach(function (k, s) {
      var d = rows.map(function (r, i) { return (i ? "L" : "M") + x(i).toFixed(1) + "," + y(r[k]).toFixed(1); }).join("");
      parts.push("<path class=\"l" + (s + 1) + "\" d=\"" + d + "\"/>");
    });
    parts.push("<line data-cross class=\"grid\" y1=\"" + T + "\" y2=\"" + (H - B) + "\"/>");
    parts.push("<g data-dots></g>");
    svg.setAttribute("viewBox", "0 0 " + W + " " + H);
    svg.innerHTML = parts.join("");
    var cross = svg.querySelector("[data-cross]"), dots = svg.querySelector("[data-dots]");
    cross.style.display = "none";
    svg.onmousemove = function (e) {
      var box = svg.getBoundingClientRect(), px = (e.clientX - box.left) * W / box.width;
      var i = Math.max(0, Math.min(rows.length - 1, Math.round((px - L) / Math.max(1, (W - L - R)) * (rows.length - 1))));
      var r = rows[i], cx = x(i);
      cross.setAttribute("x1", cx); cross.setAttribute("x2", cx); cross.style.display = "";
      dots.innerHTML = keys.map(function (k, s) { return "<circle class=\"d" + (s + 1) + "\" cx=\"" + cx + "\" cy=\"" + y(r[k]) + "\" r=\"4\"/>"; }).join("");
      tip.innerHTML = "<b>" + r.day + "</b><br>" + told(r);
      tip.style.display = "block";
      var left = cx * box.width / W + 12; if (left + tip.offsetWidth > box.width) left = cx * box.width / W - tip.offsetWidth - 12;
      tip.style.left = left + "px"; tip.style.top = "8px";
    };
    svg.onmouseleave = function () { tip.style.display = "none"; cross.style.display = "none"; dots.innerHTML = ""; };
  }

  var tiles = function (el, of) {
    el.innerHTML = of.map(function (t) { return "<div class=\"card tile\"><div class=\"l\">" + t[0] + "</div><div class=\"n\">" + t[1] + "</div><div class=\"s\">" + t[2] + "</div></div>"; }).join("");
  };
  var since = function (weeks) { return weeks === 0 ? "不到一周" : weeks + " 周"; };

  // Glance itself: what its askings of this site say.
  function glance(data) {
    var rows = everyDay(data.since, data.days, data.glance, ["running", "new_installs", "updates", "taken_here"]);
    var sum = function (k) { return rows.reduce(function (a, r) { return a + (r[k] || 0); }, 0); };
    var told = data.glance.length ? data.glance[data.glance.length - 1] : null;
    tiles($("[data-glance-tiles]"), [
      ["在运行的电脑", told ? fmt(told.running) : "—", told ? told.day + " 这一天" : "还没有数据"],
      ["新安装", fmt(sum("new_installs")), "这段时间里"],
      ["更新", fmt(sum("updates")), "这段时间里"],
      ["从官网取的安装包", fmt(sum("taken_here")), "更新时下载的次数"]
    ]);
    chart($("[data-glance-chart]"), rows, ["running", "new_installs", "updates"], function (r) {
      return "在运行 " + fmt(r.running) + "<br>新安装 " + fmt(r.new_installs) + " · 更新 " + fmt(r.updates);
    });
    table($("[data-glance-daily]"), ["日期", "在运行", "新安装", "更新", "从官网取的安装包"], rows.slice().reverse().map(function (r) { return [r.day, fmt(r.running), fmt(r.new_installs), fmt(r.updates), fmt(r.taken_here)]; }));
    var running = data.versions.reduce(function (a, v) { return a + v.running; }, 0);
    table($("[data-versions]"), ["版本", "电脑", "占比"], data.versions.map(function (v) { return [v.version, fmt(v.running), pct(v.running, running)]; }));
    table($("[data-updates]"), ["从", "到", "次数"], data.updates.map(function (u) { return [u.came_from === "pre" ? "0.3.3 之前" : u.came_from, u.version, fmt(u.updates)]; }));
    var days = data.staying.reduce(function (a, s) { return a + s.asking_days; }, 0);
    table($("[data-staying]"), ["已使用", "电脑·天", "占比"], data.staying.map(function (s) { return [since(s.weeks), fmt(s.asking_days), pct(s.asking_days, days)]; }));
  }

  // The reports users sent, the newest first; one pressed is shown whole.
  function reports() {
    var list = $("[data-reports]"), whole = $("[data-report]");
    fetch("/api/reports", { headers: { "x-stats-key": key || "" } }).then(function (r) { return r.ok ? r.json() : []; }).then(function (rows) {
      table(list, ["编号", "时间", "版本", "地区"], rows.map(function (r) { return [r.id, new Date(r.ts).toLocaleString("zh-CN"), r.version, place(r.country)]; }));
      Array.prototype.forEach.call(list.querySelectorAll("tr"), function (tr, i) {
        if (!i || !rows.length) return;
        tr.className = "pick";
        tr.addEventListener("click", function () {
          fetch("/api/reports?id=" + encodeURIComponent(rows[i - 1].id), { headers: { "x-stats-key": key || "" } }).then(function (r) { return r.json(); }).then(function (one) {
            whole.textContent = one ? one.text : ""; whole.hidden = !one;
          });
        });
      });
    });
  }

  function show(data) {
    glance(data); reports();
    var rows = everyDay(data.since, data.days, data.daily, ["views", "visitors", "downloads", "downloaders"]);
    var sum = function (k) { return rows.reduce(function (a, r) { return a + (r[k] || 0); }, 0); };
    var visitors = sum("visitors"), downloaders = sum("downloaders");
    tiles($("[data-tiles]"), [
      ["独立访客", fmt(visitors), "按天去重后相加"],
      ["浏览", fmt(sum("views")), "打开页面的次数"],
      ["下载点击", fmt(sum("downloads")), fmt(downloaders) + " 位访客"],
      ["转化率", pct(downloaders, visitors), "点了下载的访客 / 独立访客"]
    ]);
    chart($("[data-chart]"), rows, ["visitors", "downloaders"], function (r) {
      return "独立访客 " + fmt(r.visitors) + " · 浏览 " + fmt(r.views) + "<br>点了下载的访客 " + fmt(r.downloaders) + "（" + pct(r.downloaders, r.visitors) + "）";
    });
    table($("[data-daily]"), ["日期", "独立访客", "浏览", "下载点击", "点了下载的访客"], rows.slice().reverse().map(function (r) { return [r.day, fmt(r.visitors), fmt(r.views), fmt(r.downloads), fmt(r.downloaders)]; }));
    table($("[data-countries]"), ["国家和地区", "访客", "下载", "转化"], data.countries.map(function (c) { return [place(c.country), fmt(c.visitors), fmt(c.downloaders), pct(c.downloaders, c.visitors)]; }));
    table($("[data-sources]"), ["来源", "下载点击"], data.sources.map(function (s) { return [s.source === "gitee" ? "Gitee" : s.source === "github" ? "GitHub" : s.source || "未知", fmt(s.downloads)]; }));
    table($("[data-pages]"), ["页面", "浏览"], data.pages.map(function (p) { return [p.path === "/" ? "中文首页" : p.path === "/en/" ? "英文首页" : p.path, fmt(p.views)]; }));
    table($("[data-refs]"), ["来源", "浏览"], data.refs.map(function (r) { return [r.ref === "" ? "不带来源（App 内打开、直接访问等）" : r.ref.indexOf("tag:") === 0 ? "标记：" + r.ref.slice(4) : r.ref, fmt(r.views)]; }));
  }

  function load() {
    fetch("/api/stats?days=" + days, { headers: { "x-stats-key": key || "" } }).then(function (r) {
      if (r.status === 403) { $("[data-board]").hidden = true; $("[data-login]").hidden = false; return null; }
      return r.json();
    }).then(function (data) {
      if (!data) return;
      // The owner's browser: kept out of the count.
      store.set(KEY, key); store.set("glance-notrack", "1");
      $("[data-login]").hidden = true; $("[data-board]").hidden = false;
      show(data);
    });
  }

  $("[data-form]").addEventListener("submit", function (e) { e.preventDefault(); key = $("[data-key]").value.trim(); load(); });
  $("[data-days]").addEventListener("click", function (e) {
    var b = e.target.closest("button[data-d]"); if (!b) return;
    days = +b.getAttribute("data-d");
    Array.prototype.forEach.call(this.querySelectorAll("button"), function (x) { x.setAttribute("aria-pressed", x === b ? "true" : "false"); });
    load();
  });
  var redraw; window.addEventListener("resize", function () { clearTimeout(redraw); redraw = setTimeout(load, 200); });
  if (key) load(); else $("[data-login]").hidden = false;
})();
