-- Provider accounts (Google/YouTube, Spotify) linked to a Roynit ID. Refresh tokens are stored encrypted (AES-GCM, LINK_KEY).
CREATE TABLE links (
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  provider TEXT NOT NULL,
  label TEXT NOT NULL DEFAULT '',
  refresh TEXT NOT NULL,
  linked_at INTEGER NOT NULL,
  PRIMARY KEY (user_id, provider)
);
CREATE TABLE link_attempts (
  state_hash TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  provider TEXT NOT NULL,
  verifier TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending'
);
