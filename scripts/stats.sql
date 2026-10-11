-- The site's own count (see _worker.js): one row for each page shown and
-- each installer taken; the salts that tell one day's visitors apart.
CREATE TABLE IF NOT EXISTS events (
  ts INTEGER NOT NULL,
  day TEXT NOT NULL,
  kind TEXT NOT NULL,
  path TEXT NOT NULL,
  ref TEXT NOT NULL,
  country TEXT NOT NULL,
  visitor TEXT NOT NULL,
  source TEXT NOT NULL,
  lang TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS events_day ON events (day, kind);
CREATE TABLE IF NOT EXISTS salts (day TEXT PRIMARY KEY, salt TEXT NOT NULL);

-- What users of Glance sent of their own PCs (see /api/report in _worker.js):
-- a short name each, the day, which of that day's senders, and the text.
CREATE TABLE IF NOT EXISTS reports (
  id TEXT PRIMARY KEY,
  ts INTEGER NOT NULL,
  day TEXT NOT NULL,
  sender TEXT NOT NULL,
  version TEXT NOT NULL,
  country TEXT NOT NULL,
  text TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS reports_day ON reports (day, sender);
