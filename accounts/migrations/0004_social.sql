-- Friends, what each person is listening to, and direct messages.
-- A friendship is one row per pair, stored with the smaller id first, so it can never exist twice.
CREATE TABLE friendships (
  low TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  high TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  requester TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',
  created_at INTEGER NOT NULL,
  PRIMARY KEY (low, high),
  CHECK (low < high)
);
CREATE INDEX friendship_high ON friendships(high);
CREATE TABLE presence (
  user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
  invisible INTEGER NOT NULL DEFAULT 0,
  seen_at INTEGER NOT NULL,
  listening TEXT
);
CREATE TABLE messages (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  low TEXT NOT NULL,
  high TEXT NOT NULL,
  sender TEXT NOT NULL,
  body TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  read INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX messages_pair ON messages(low, high, id);
