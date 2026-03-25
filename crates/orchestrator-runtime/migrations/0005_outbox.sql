CREATE TABLE IF NOT EXISTS outbox (
  seq BIGSERIAL PRIMARY KEY,
  message_id TEXT UNIQUE NOT NULL,
  message JSONB NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_outbox_seq ON outbox (seq);
