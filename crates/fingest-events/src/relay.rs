use std::{sync::Arc, time::Duration};

use fingest_kernel::{EventEnvelope, EventPublisher, PortError};

use crate::outbox::OutboxReader;

/// Drains the outbox and hands events to a publisher.
///
/// Delivery is **at-least-once**: if publishing succeeds but the row cannot be marked, the
/// event is re-delivered on the next pass. Subscribers must be idempotent.
pub struct OutboxRelay {
    reader: Arc<dyn OutboxReader>,
    publisher: Arc<dyn EventPublisher>,
    batch_size: i64,
}

impl OutboxRelay {
    pub fn new(reader: Arc<dyn OutboxReader>, publisher: Arc<dyn EventPublisher>) -> Self {
        Self {
            reader,
            publisher,
            batch_size: 100,
        }
    }

    pub fn with_batch_size(mut self, batch_size: i64) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Publishes one batch. Returns how many events were delivered.
    ///
    /// Rows are marked only after the publish succeeds, so a failing broker leaves them
    /// pending rather than silently dropping them.
    pub async fn drain_once(&self) -> Result<usize, PortError> {
        let pending = self.reader.fetch_unpublished(self.batch_size).await?;
        if pending.is_empty() {
            return Ok(0);
        }

        let (ids, envelopes): (Vec<i64>, Vec<EventEnvelope>) = pending
            .into_iter()
            .map(|event| (event.id, event.envelope))
            .unzip();

        self.publisher.publish(&envelopes).await?;
        self.reader.mark_published(&ids).await?;

        Ok(envelopes.len())
    }

    /// Polls until the process ends. Errors are logged and retried on the next tick, so a
    /// broker outage never takes the API down with it.
    pub async fn run(self, interval: Duration) {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            ticker.tick().await;

            match self.drain_once().await {
                Ok(0) => {}
                Ok(count) => tracing::debug!(count, "outbox batch published"),
                Err(err) => tracing::warn!(error = %err, "outbox relay pass failed; will retry"),
            }
        }
    }
}
