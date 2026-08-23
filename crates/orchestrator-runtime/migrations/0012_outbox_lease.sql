-- Outbox delivery leases.
--
-- Dequeue used to DELETE the row and hand the message to the caller, so a process
-- that died between the delete and the delivery lost the event outright. A claim
-- now leases the row (locked_until) and the row is only removed once delivery has
-- been acknowledged, which also lets a crashed replica's claim expire and be
-- picked up by another.
ALTER TABLE outbox ADD COLUMN IF NOT EXISTS locked_until TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_outbox_locked_until ON outbox (locked_until);
