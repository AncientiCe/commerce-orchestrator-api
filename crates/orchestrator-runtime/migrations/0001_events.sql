CREATE TABLE IF NOT EXISTS cart_events (
  cart_id TEXT PRIMARY KEY,
  events JSONB NOT NULL
);

CREATE TABLE IF NOT EXISTS cart_snapshots (
  cart_id TEXT PRIMARY KEY,
  snapshot JSONB NOT NULL
);

CREATE TABLE IF NOT EXISTS cart_states (
  cart_id TEXT PRIMARY KEY,
  state JSONB NOT NULL
);
