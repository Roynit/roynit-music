-- Birthday as YYYY-MM-DD, private to the owner: the apps use it to wish them a happy birthday.
ALTER TABLE users ADD COLUMN birthday TEXT;
