-- Blend: friends can mix their songs with yours. On by default, and each person can switch it off.
ALTER TABLE users ADD COLUMN share_taste INTEGER NOT NULL DEFAULT 1;
