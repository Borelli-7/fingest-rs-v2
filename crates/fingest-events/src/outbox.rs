use std::time::Duration;

use async_trait::async_trait;
use fingest_kernel::{EventEnvelope, PortError};
use sqlx::PgPool;

/// A pending outbox row and the envelope it carries.
pub struct PendingEvent {
    pub id: i64,
    pub envelope: EventEnvelope,
}

#[async_trait]
pub trait OutboxReader: Send + Sync {
    /// Claims up to `limit` pending rows for this relay, oldest first. A claimed row is
    /// invisible to other relays until it is published, released or its lease expires.
    async fn claim_unpublished(&self, limit: i64) -> Result<Vec<PendingEvent>, PortError>;

    async fn mark_published(&self, ids: &[i64]) -> Result<u64, PortError>;

    /// Gives up a claim after a failed publish so the rows are retried on the next pass.
    async fn release(&self, ids: &[i64]) -> Result<u64, PortError>;

    /// Deletes rows published longer ago than `older_than`.
    async fn purge_published(&self, older_than: Duration) -> Result<u64, PortError>;
}

pub struct PgOutboxReader {
    pool: PgPool,
    lease: Duration,
}

impl PgOutboxReader {
    /// Long enough for any publish to finish; short enough that a crashed relay's batch
    /// is picked up again promptly.
    pub const DEFAULT_LEASE: Duration = Duration::from_secs(60);

    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            lease: Self::DEFAULT_LEASE,
        }
    }
}

fn seconds(duration: Duration) -> f64 {
    duration.as_secs_f64()
}

#[async_trait]
impl OutboxReader for PgOutboxReader {
    /// `FOR UPDATE SKIP LOCKED` inside a single `UPDATE ... RETURNING`: concurrent relays
    /// take disjoint batches without holding a transaction open while they publish.
    async fn claim_unpublished(&self, limit: i64) -> Result<Vec<PendingEvent>, PortError> {
        let mut rows = sqlx::query!(
            r#"UPDATE outbox
               SET claimed_until = now() + make_interval(secs => $2)
               WHERE id IN (
                   SELECT id FROM outbox
                   WHERE published_at IS NULL
                     AND (claimed_until IS NULL OR claimed_until < now())
                   ORDER BY id
                   LIMIT $1
                   FOR UPDATE SKIP LOCKED
               )
               RETURNING id, aggregate, event_type, payload, occurred_at"#,
            limit,
            seconds(self.lease)
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| PortError::Storage(e.to_string()))?;

        // RETURNING carries no ordering guarantee.
        rows.sort_by_key(|row| row.id);

        Ok(rows
            .into_iter()
            .map(|row| PendingEvent {
                id: row.id,
                envelope: EventEnvelope {
                    aggregate: row.aggregate,
                    event_type: row.event_type,
                    payload: row.payload,
                    occurred_at: row.occurred_at,
                },
            })
            .collect())
    }

    /// Idempotent: marking an already-published row is a no-op, so a retried mark after
    /// an ambiguous failure cannot move `published_at`.
    async fn mark_published(&self, ids: &[i64]) -> Result<u64, PortError> {
        sqlx::query!(
            r#"UPDATE outbox SET published_at = now(), claimed_until = NULL
               WHERE id = ANY($1) AND published_at IS NULL"#,
            ids
        )
        .execute(&self.pool)
        .await
        .map(|r| r.rows_affected())
        .map_err(|e| PortError::Storage(e.to_string()))
    }

    async fn release(&self, ids: &[i64]) -> Result<u64, PortError> {
        sqlx::query!(
            r#"UPDATE outbox SET claimed_until = NULL
               WHERE id = ANY($1) AND published_at IS NULL"#,
            ids
        )
        .execute(&self.pool)
        .await
        .map(|r| r.rows_affected())
        .map_err(|e| PortError::Storage(e.to_string()))
    }

    async fn purge_published(&self, older_than: Duration) -> Result<u64, PortError> {
        sqlx::query!(
            r#"DELETE FROM outbox
               WHERE published_at IS NOT NULL
                 AND published_at < now() - make_interval(secs => $1)"#,
            seconds(older_than)
        )
        .execute(&self.pool)
        .await
        .map(|r| r.rows_affected())
        .map_err(|e| PortError::Storage(e.to_string()))
    }
}
