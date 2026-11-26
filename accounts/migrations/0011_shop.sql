-- Daily listening quests, the Orbs shop and what each person wears.
ALTER TABLE users ADD COLUMN equipped TEXT NOT NULL DEFAULT '{}';

CREATE TABLE inventory (
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  item TEXT NOT NULL,
  variant TEXT NOT NULL,
  acquired_at INTEGER NOT NULL,
  PRIMARY KEY (user_id, item, variant)
);

-- One row per person per day (Vietnam time). The server counts the listening itself from presence heartbeats.
CREATE TABLE quest_day (
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  day INTEGER NOT NULL,
  seconds INTEGER NOT NULL DEFAULT 0,
  songs TEXT NOT NULL DEFAULT '[]',
  artists TEXT NOT NULL DEFAULT '[]',
  per_song TEXT NOT NULL DEFAULT '{}',
  last_at INTEGER NOT NULL DEFAULT 0,
  last_key TEXT NOT NULL DEFAULT '',
  last_position REAL NOT NULL DEFAULT 0,
  claimed TEXT NOT NULL DEFAULT '[]',
  PRIMARY KEY (user_id, day)
);

-- Every change to the Orbs balance, so the numbers can always be explained.
CREATE TABLE orb_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  delta INTEGER NOT NULL,
  reason TEXT NOT NULL,
  at INTEGER NOT NULL
);
CREATE INDEX orb_log_user ON orb_log(user_id, id);
