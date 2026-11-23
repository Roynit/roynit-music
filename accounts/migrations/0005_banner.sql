-- Profile cover picture, shown behind the avatar.
ALTER TABLE users ADD COLUMN banner_set INTEGER NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN banner_version INTEGER NOT NULL DEFAULT 0;
CREATE TABLE banners (
  user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
  content_type TEXT NOT NULL,
  data BLOB NOT NULL
);
