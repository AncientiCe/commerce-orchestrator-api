CREATE TABLE IF NOT EXISTS payment_state (
  transaction_id TEXT PRIMARY KEY,
  state JSONB NOT NULL
);
