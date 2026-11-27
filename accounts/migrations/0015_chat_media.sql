-- Photos and voice messages in a conversation. The file lives in the MEDIA bucket (removed after 30 days); the message keeps its key.
ALTER TABLE messages ADD COLUMN media_key TEXT;
ALTER TABLE messages ADD COLUMN media_type TEXT;
ALTER TABLE messages ADD COLUMN media_seconds INTEGER NOT NULL DEFAULT 0;
ALTER TABLE messages ADD COLUMN media_size INTEGER NOT NULL DEFAULT 0;
CREATE INDEX messages_media ON messages(sender, created_at) WHERE media_key IS NOT NULL;
