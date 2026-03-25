CREATE TABLE IF NOT EXISTS dead_letter (
  message_id TEXT PRIMARY KEY,
  message JSONB NOT NULL
);
