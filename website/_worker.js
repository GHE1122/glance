// The site's own count of who comes and who downloads: the pages are served
// as they are (env.ASSETS); a browser showing one says so, and says so as
// its visitor takes the installer, at /api/e. No cookie is set and no
// address kept: a visitor is told from another by a hash of their address
// and browser with that day's salt, and a day's salt is thrown away once the
// day is over, so no visitor can be followed from one day to the next.
// /api/stats gives the counts to whoever has the key (STATS_KEY).
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
  const [daily, countries, sources, refs, pages] = await Promise.all([
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
  ]);
  return json({ since, days, daily, countries, sources, refs, pages });
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.pathname === "/api/e" && request.method === "POST") return record(request, env);
    if (url.pathname === "/api/stats" && request.method === "GET") return stats(request, env);
    if (url.pathname === "/api/report" && request.method === "POST") return report(request, env);
    if (url.pathname === "/api/reports" && request.method === "GET") return reports(request, env);
    return env.ASSETS.fetch(request);
  },
};
