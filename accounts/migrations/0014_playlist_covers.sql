-- A picture chosen for a playlist, instead of the mosaic of its songs' covers. The version grows with every change.
CREATE TABLE playlist_covers (
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  playlist_id TEXT NOT NULL,
  content_type TEXT NOT NULL,
  data BLOB NOT NULL,
  version INTEGER NOT NULL,
  PRIMARY KEY (user_id, playlist_id)
);
