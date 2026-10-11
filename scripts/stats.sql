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

-- Glance's own askings of the site (see /api/latest in _worker.js): one row for each time a Glance asked for
-- the latest version ('ask') or took an installer ('take'); which of that day's PCs, the version asking (or
-- taken), the version it came from, whether it was a new install's first asking, and how many weeks Glance
-- has been on that PC.
CREATE TABLE IF NOT EXISTS asks (
  ts INTEGER NOT NULL,
  day TEXT NOT NULL,
  kind TEXT NOT NULL,
  machine TEXT NOT NULL,
  version TEXT NOT NULL,
  came_from TEXT NOT NULL,
  fresh INTEGER NOT NULL,
  weeks INTEGER,
  country TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS asks_day ON asks (day, machine);
