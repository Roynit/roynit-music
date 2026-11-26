-- Replies and emoji reactions on direct messages.
ALTER TABLE messages ADD COLUMN reply_to INTEGER;

-- One reaction per person per message (a chat has two people, so a message has at most two).
CREATE TABLE message_reactions (
  message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  user_id TEXT NOT NULL,
  emoji TEXT NOT NULL,
  PRIMARY KEY (message_id, user_id)
);
