CREATE TABLE IF NOT EXISTS webhooks (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  url TEXT NOT NULL,
  secret TEXT NOT NULL,
  event_filter JSONB,
  active BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE INDEX IF NOT EXISTS idx_webhooks_tenant_id ON webhooks (tenant_id);
CREATE INDEX IF NOT EXISTS idx_webhooks_active ON webhooks (active);
