// The site's own count of who comes and who downloads: the pages are served
// as they are (env.ASSETS); a browser showing one says so, and says so as
// its visitor takes the installer, at /api/e. No cookie is set and no
// address kept: a visitor is told from another by a hash of their address
// and browser with that day's salt, and a day's salt is thrown away once the
// day is over, so no visitor can be followed from one day to the next.
// /api/stats gives the counts to whoever has the key (STATS_KEY).
//
// /api/latest is where Glance itself asks, once a day, whether there is a
// newer version (GitHub and Gitee are asked besides, and serve where this
// site is out of reach): answered as those two answer, from latest.json,
// with the installer this site serves. Each asking is counted, with what it
// says of itself: its version, whether it is the first of a Glance new on
// its PC, which version it was updated from, and how many weeks it has been
// there. An installer taken by Glance (not from a page) is counted as an
// update. No PC is told from another beyond the day: the same hash of its
// address with that day's salt as for a page's visitors.
//
// /api/report takes what a user of Glance sends of their own PC when its
// hardware is not read or set as it should be (the text Glance showed them
// first: its parts and what Glance reads of them), and gives it a short
// name they can quote; no address is kept with it, only which day's sender
// it was, so that one sender cannot fill the table. /api/reports gives
// those taken to whoever has the key.

// Days as they are in China, where most visitors are.
const OFFSET_MS = 8 * 3600 * 1000;
const dayOf = (ms) => new Date(ms + OFFSET_MS).toISOString().slice(0, 10);

const json = (body, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json; charset=utf-8", "cache-control": "no-store" } });

async function hex(text) {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

// That day's salt, made the first time it is asked for; those of the days
// before, gone.
async function saltOf(db, day) {
  const fresh = crypto.randomUUID();
  await db.batch([
    db.prepare("INSERT OR IGNORE INTO salts (day, salt) VALUES (?, ?)").bind(day, fresh),
    db.prepare("DELETE FROM salts WHERE day < ?").bind(day),
  ]);
  const row = await db.prepare("SELECT salt FROM salts WHERE day = ?").bind(day).first();
  return row.salt;
}

const clip = (value, most) => (typeof value === "string" ? value.slice(0, most) : "");

// A version as Glance writes them (numbers and dots), or nothing.
const version = (value) => (/^[0-9]{1,4}(\.[0-9]{1,4}){1,3}$/.test(value || "") ? value : "");
// How many askings and takings one address may have counted in a day: the
// rest are answered all the same, and not counted.
const ASKS_A_DAY = 24;

// One asking or taking of Glance's own, counted.
async function count(request, env, kind, of, cameFrom, fresh, weeks) {
  const now = Date.now();
  const day = dayOf(now);
  const address = request.headers.get("cf-connecting-ip") || "";
  const machine = (await hex(`${await saltOf(env.STATS, day)}|${address}`)).slice(0, 16);
  await env.STATS.prepare(
    `INSERT INTO asks (ts, day, kind, machine, version, came_from, fresh, weeks, country)
     SELECT ?, ?, ?, ?, ?, ?, ?, ?, ? WHERE (SELECT COUNT(*) FROM asks WHERE day = ? AND machine = ?) < ?`,
  )
    .bind(now, day, kind, machine, of, cameFrom, fresh ? 1 : 0, weeks, request.cf?.country || "", day, machine, ASKS_A_DAY)
    .run();
}

// Glance asking for the latest release.
async function latest(request, env, url) {
  const asked = url.searchParams;
  const of = version(asked.get("v"));
  // Only Glance's own asking is counted (a browser's look at the address is not).
  if (of && /^Glance\//.test(request.headers.get("user-agent") || "")) {
    const cameFrom = asked.get("f") === "pre" ? "pre" : version(asked.get("f"));
    const weeks = /^[0-9]{1,4}$/.test(asked.get("w") || "") ? parseInt(asked.get("w"), 10) : null;
    try {
      await count(request, env, "ask", of, cameFrom, asked.get("n") === "1", weeks);
    } catch {
      // (The count failing is no reason to keep the answer from Glance.)
    }
  }
  const answer = await env.ASSETS.fetch(new Request(new URL("/latest.json", url), { method: "GET" }));
  return new Response(answer.body, { status: answer.status, headers: { "content-type": "application/json; charset=utf-8", "cache-control": "no-store" } });
}

// An installer taken by Glance itself (it says which version takes it).
async function taken(request, env, url) {
  const of = version((url.pathname.match(/^\/dl\/Glance_([0-9.]+)_x64-setup\.exe$/) || [])[1]);
  const cameFrom = version(url.searchParams.get("u"));
  if (of && cameFrom && /^Glance\//.test(request.headers.get("user-agent") || "")) {
    try {
      await count(request, env, "take", of, cameFrom, false, null);
    } catch {}
  }
  return env.ASSETS.fetch(new Request(new URL(url.pathname, url), request));
}

async function record(request, env) {
  let event;
  try {
    event = JSON.parse(await request.text());
  } catch {
    return new Response(null, { status: 400 });
  }
  const kind = event.k === "download" ? "download" : event.k === "view" ? "view" : null;
  const ua = request.headers.get("user-agent") || "";
  // Only what a browser sends.
  if (!kind || !/^Mozilla\//.test(ua)) return new Response(null, { status: 204 });
  const now = Date.now();
  const day = dayOf(now);
  const address = request.headers.get("cf-connecting-ip") || "";
  const visitor = (await hex(`${await saltOf(env.STATS, day)}|${address}|${ua}`)).slice(0, 16);
  // Where it came from: the name its link carried (?from=name), else the
  // other site's name alone, not the page.
  let from = "";
  const tag = clip(event.f, 24).toLowerCase();
  if (/^[a-z0-9_-]+$/.test(tag)) {
    from = `tag:${tag}`;
  } else {
    try {
      const ref = new URL(clip(event.r, 500));
      if (ref.hostname !== new URL(request.url).hostname) from = ref.hostname;
    } catch {}
  }
  await env.STATS.prepare(
    "INSERT INTO events (ts, day, kind, path, ref, country, visitor, source, lang) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
  )
    .bind(now, day, kind, clip(event.p, 100), from, request.cf?.country || "", visitor, kind === "download" ? clip(event.s, 10) : "", clip(event.l, 10))
    .run();
  return new Response(null, { status: 204 });
}

// The most of a report that is kept (its text, in characters) and the most
// that is read of what carries it (bytes), how many one sender may send in a day and how many are taken in a
// day from everyone, and how long one is kept.
const REPORT_MOST = 32 * 1024;
const REPORT_BYTES = 4 * REPORT_MOST;
const REPORTS_A_DAY = 6;
const REPORTS_A_DAY_IN_ALL = 500;
const REPORTS_KEPT_FOR = 180 * 86400000;

// What a request carries, as text, if it is no more than `most` bytes: read
// no further than that.
async function bodyWithin(request, most) {
  if (Number(request.headers.get("content-length")) > most || !request.body) return null;
  const reader = request.body.getReader();
  const parts = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > most) {
      await reader.cancel();
      return null;
    }
    parts.push(value);
  }
  const all = new Uint8Array(size);
  let at = 0;
  for (const part of parts) {
    all.set(part, at);
    at += part.byteLength;
  }
  return new TextDecoder().decode(all);
}

async function report(request, env) {
  // Only what Glance sends.
  if (!/^Glance\//.test(request.headers.get("user-agent") || "")) return json({ error: "nothing" }, 400);
  const body = await bodyWithin(request, REPORT_BYTES);
  if (body === null) return json({ error: "too long" }, 413);
  let sent;
  try {
    sent = JSON.parse(body);
  } catch {
    return json({ error: "not json" }, 400);
  }
  if (!sent || typeof sent !== "object" || typeof sent.text !== "string" || !sent.text) return json({ error: "nothing" }, 400);
  const now = Date.now();
  const day = dayOf(now);
  const sender = (await hex(`${await saltOf(env.STATS, day)}|${request.headers.get("cf-connecting-ip") || ""}`)).slice(0, 16);
  // A name of six letters and figures, none easily taken for another.
  const letters = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
  const id = [...crypto.getRandomValues(new Uint8Array(6))].map((b) => letters[b % letters.length]).join("");
  // Kept only while there is room for it today (this sender's, and everyone's): counted and kept as one, so that several sent at once are not each counted before any is kept.
  const kept = await env.STATS.prepare(
    `INSERT INTO reports (id, ts, day, sender, version, country, text)
     SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7
     WHERE (SELECT COUNT(*) FROM reports WHERE day = ?3 AND sender = ?4) < ?8 AND (SELECT COUNT(*) FROM reports WHERE day = ?3) < ?9`,
  )
    .bind(id, now, day, sender, clip(sent.version, 20), request.cf?.country || "", clip(sent.text, REPORT_MOST), REPORTS_A_DAY, REPORTS_A_DAY_IN_ALL)
    .run();
  if (!kept.meta.changes) return json({ error: "too many" }, 429);
  await env.STATS.prepare("DELETE FROM reports WHERE ts < ?").bind(now - REPORTS_KEPT_FOR).run();
  return json({ id });
}

async function reports(request, env) {
  if (!env.STATS_KEY || request.headers.get("x-stats-key") !== env.STATS_KEY) return json({ error: "key" }, 403);
  const url = new URL(request.url);
  const id = url.searchParams.get("id");
  if (id) return json(await env.STATS.prepare("SELECT id, ts, day, version, country, text FROM reports WHERE id = ?").bind(id.toUpperCase()).first());
  const listed = await env.STATS.prepare("SELECT id, ts, day, version, country, substr(text, 1, 400) AS head FROM reports ORDER BY ts DESC LIMIT 100").all();
  return json(listed.results);
}

async function stats(request, env) {
  if (!env.STATS_KEY || request.headers.get("x-stats-key") !== env.STATS_KEY) return json({ error: "key" }, 403);
  const days = Math.min(Math.max(parseInt(new URL(request.url).searchParams.get("days") || "30", 10) || 30, 1), 365);
  const since = dayOf(Date.now() - (days - 1) * 86400000);
  const all = (sql) => env.STATS.prepare(sql).bind(since).all().then((r) => r.results);
  const [daily, countries, sources, refs, pages, glance, versions, updates, staying] = await Promise.all([
    // Each day: pages shown, visitors, downloads, visitors who downloaded.
    all(`SELECT day,
           SUM(kind = 'view') AS views,
           COUNT(DISTINCT CASE WHEN kind = 'view' THEN visitor END) AS visitors,
           SUM(kind = 'download') AS downloads,
           COUNT(DISTINCT CASE WHEN kind = 'download' THEN visitor END) AS downloaders
         FROM events WHERE day >= ? GROUP BY day ORDER BY day`),
    // A visitor is one a day: counted per day, then added up.
    all(`SELECT country, SUM(visitors) AS visitors, SUM(downloaders) AS downloaders FROM (
           SELECT day, country,
             COUNT(DISTINCT CASE WHEN kind = 'view' THEN visitor END) AS visitors,
             COUNT(DISTINCT CASE WHEN kind = 'download' THEN visitor END) AS downloaders
           FROM events WHERE day >= ? GROUP BY day, country)
         GROUP BY country ORDER BY visitors DESC`),
    all(`SELECT source, COUNT(*) AS downloads FROM events WHERE day >= ? AND kind = 'download' GROUP BY source ORDER BY downloads DESC`),
    all(`SELECT ref, COUNT(*) AS views FROM events WHERE day >= ? AND kind = 'view' GROUP BY ref ORDER BY views DESC LIMIT 20`),
    all(`SELECT path, COUNT(*) AS views FROM events WHERE day >= ? AND kind = 'view' GROUP BY path ORDER BY views DESC LIMIT 20`),
    // Glance itself, each day: how many PCs asked (one address a day counts once), how many of them were new
    // installs' first asking, and how many installers Glance took (updates).
    all(`SELECT day,
           COUNT(DISTINCT CASE WHEN kind = 'ask' THEN machine END) AS running,
           COUNT(DISTINCT CASE WHEN kind = 'ask' AND fresh = 1 THEN machine END) AS new_installs,
           SUM(kind = 'take') AS updates
         FROM asks WHERE day >= ? GROUP BY day ORDER BY day`),
    // Which versions run, over the latest day there is.
    all(`SELECT version, COUNT(DISTINCT machine) AS running FROM asks
         WHERE day >= ? AND kind = 'ask' AND day = (SELECT MAX(day) FROM asks WHERE kind = 'ask') GROUP BY version ORDER BY running DESC`),
    // Updates, from which version to which.
    all(`SELECT came_from, version, COUNT(*) AS updates FROM asks WHERE day >= ? AND kind = 'take' GROUP BY came_from, version ORDER BY updates DESC LIMIT 40`),
    // Staying: of the PCs asking each day (counted per day, then added up), how many weeks Glance has been there.
    all(`SELECT weeks, COUNT(*) AS asking_days FROM (SELECT DISTINCT day, machine, weeks FROM asks WHERE day >= ? AND kind = 'ask' AND weeks IS NOT NULL)
         GROUP BY weeks ORDER BY weeks`),
  ]);
  return json({ since, days, daily, countries, sources, refs, pages, glance, versions, updates, staying });
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.pathname === "/api/e" && request.method === "POST") return record(request, env);
    if (url.pathname === "/api/stats" && request.method === "GET") return stats(request, env);
    if (url.pathname === "/api/report" && request.method === "POST") return report(request, env);
    if (url.pathname === "/api/reports" && request.method === "GET") return reports(request, env);
    if (url.pathname === "/api/latest" && request.method === "GET") return latest(request, env, url);
    if (url.pathname.startsWith("/dl/Glance_") && request.method === "GET" && url.searchParams.has("u")) return taken(request, env, url);
    return env.ASSETS.fetch(request);
  },
};
