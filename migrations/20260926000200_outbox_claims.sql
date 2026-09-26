-- Relays on several replicas used to read the same pending rows and publish each one once
-- per replica. A row is now claimed for a lease before publishing; an expired lease (a relay
-- that died mid-batch) makes the row claimable again.
ALTER TABLE outbox ADD COLUMN IF NOT EXISTS claimed_until TIMESTAMPTZ;

-- Supports the retention purge of published rows.
CREATE INDEX IF NOT EXISTS idx_outbox_published_at
    ON outbox (published_at)
    WHERE published_at IS NOT NULL;
