-- Orbs: the in-app currency, spent in a shop that comes later. Everyone starts at zero.
ALTER TABLE users ADD COLUMN orbs INTEGER NOT NULL DEFAULT 0;
