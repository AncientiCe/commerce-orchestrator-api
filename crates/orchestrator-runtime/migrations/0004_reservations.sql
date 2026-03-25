CREATE TABLE IF NOT EXISTS reservations (
  reservation_key TEXT PRIMARY KEY,
  record JSONB NOT NULL
);
