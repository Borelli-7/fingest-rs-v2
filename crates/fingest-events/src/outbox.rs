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
    async fn fetch_unpublished(&self, limit: i64) -> Result<Vec<PendingEvent>, PortError>;

    async fn mark_published(&self, ids: &[i64]) -> Result<u64, PortError>;
}

pub struct PgOutboxReader {
    pool: PgPool,
}

impl PgOutboxReader {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl OutboxReader for PgOutboxReader {
    async fn fetch_unpublished(&self, limit: i64) -> Result<Vec<PendingEvent>, PortError> {
        let rows = sqlx::query!(
            r#"SELECT id, aggregate, event_type, payload, occurred_at
               FROM outbox
               WHERE published_at IS NULL
               ORDER BY id
               LIMIT $1"#,
            limit
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| PortError::Storage(e.to_string()))?;

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

    /// Guarded by `published_at IS NULL` so a concurrent relay cannot double-count.
    async fn mark_published(&self, ids: &[i64]) -> Result<u64, PortError> {
        sqlx::query!(
            r#"UPDATE outbox SET published_at = now()
               WHERE id = ANY($1) AND published_at IS NULL"#,
            ids
        )
        .execute(&self.pool)
        .await
        .map(|r| r.rows_affected())
        .map_err(|e| PortError::Storage(e.to_string()))
    }
}
