CREATE TABLE IF NOT EXISTS mandate_dedupe (
  mandate_id TEXT PRIMARY KEY,
  expires_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mandate_dedupe_expires_at ON mandate_dedupe (expires_at);
