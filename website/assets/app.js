// The phone app's page (app/index.html): its language, and the newest app to download.
(function () {
  "use strict";
  // The page in the reader's language: Chinese unless the browser reads none.
  var languages = navigator.languages && navigator.languages.length ? navigator.languages : [navigator.language || ""];
  var zh = languages.some(function (l) { return /^zh\b/i.test(l); });
  if (!zh) {
    document.documentElement.lang = "en";
    document.title = "Glance for your phone · Download";
    Array.prototype.forEach.call(document.querySelectorAll("[data-zh]"), function (e) { e.hidden = true; });
    Array.prototype.forEach.call(document.querySelectorAll("[data-en]"), function (e) { e.hidden = false; });
  }
  // Inside another app's own browser a download goes nowhere: said first.
  if (/MicroMessenger|xhsdiscover|XiaoHongShu|Weibo|QQ\/|DingTalk|AlipayClient/i.test(navigator.userAgent)) document.getElementById("inapp").hidden = false;

  // The newest release that has the app, from the site this reader reaches best; the other site is offered beside it.
  var REPO = "lulu-loopp/glance";
  var SOURCES = {
    github: { api: "https://api.github.com/repos/" + REPO + "/releases?per_page=10", all: "https://github.com/" + REPO + "/releases/latest" },
    gitee: { api: "https://gitee.com/api/v5/repos/" + REPO + "/releases?per_page=10&page=1&direction=desc", all: "https://gitee.com/" + REPO + "/releases" }
  };
  var primary = zh ? "gitee" : "github", second = zh ? "github" : "gitee";
  var get = document.getElementById("get"), other = document.getElementById(zh ? "other" : "other-en");
  get.href = SOURCES[primary].all;
  other.href = SOURCES[second].all;
  function newest(source, done) {
    fetch(SOURCES[source].api).then(function (answer) { return answer.ok ? answer.json() : []; }).then(function (releases) {
      for (var i = 0; i < releases.length; i++) {
        var assets = releases[i].assets || [];
        for (var j = 0; j < assets.length; j++) {
          if (/^Glance_[0-9.]+\.apk$/.test(assets[j].name || "") && /^https:\/\//.test(assets[j].browser_download_url || "")) return done(assets[j].browser_download_url, assets[j].name.replace(/^Glance_|\.apk$/g, ""));
        }
      }
    }).catch(function () { /* not reached: the link stays on that site's releases */ });
  }
  function offer(url, version) {
    get.href = url;
    Array.prototype.forEach.call(document.querySelectorAll("[data-version]"), function (e) { e.textContent = " " + version; });
  }
  // The app from this site first (reached where GitHub is not; see /api/app), else the release's on the reader's site.
  fetch("/api/app").then(function (answer) { return answer.ok ? answer.json() : {}; }).then(function (told) {
    if (told.apk && /^https:\/\/glancepc\.com\/dl\/Glance_[0-9.]+\.apk$/.test(told.apk)) offer(told.apk, told.version);
    else newest(primary, offer);
  }).catch(function () { newest(primary, offer); });
  newest(second, function (url) { other.href = url; });
  var from = "";
  try { from = new URLSearchParams(location.search).get("from") || ""; } catch (e) { /* none */ }
  if (navigator.sendBeacon) navigator.sendBeacon("/api/e", JSON.stringify({ k: "view", p: location.pathname, r: document.referrer, l: zh ? "zh" : "en", f: from }));
  get.addEventListener("click", function () {
    var said = JSON.stringify({ k: "download", p: location.pathname, s: get.href.indexOf("glancepc.com") >= 0 ? "site" : get.href.indexOf("gitee.com") >= 0 ? "gitee" : "github", l: zh ? "zh" : "en" });
    if (navigator.sendBeacon) navigator.sendBeacon("/api/e", said);
  });
})();
