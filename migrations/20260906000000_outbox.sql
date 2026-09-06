-- Transactional outbox. Rows are written in the same transaction as the state change they
-- describe; a relay publishes them and stamps published_at.
CREATE TABLE IF NOT EXISTS outbox (
    id BIGSERIAL PRIMARY KEY,
    aggregate TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    published_at TIMESTAMPTZ
);

-- Partial index: the relay only ever scans unpublished rows.
CREATE INDEX IF NOT EXISTS idx_outbox_unpublished
    ON outbox (id)
    WHERE published_at IS NULL;
