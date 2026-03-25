CREATE TABLE IF NOT EXISTS idempotency (
  idempotency_key TEXT PRIMARY KEY,
  state JSONB NOT NULL
);
