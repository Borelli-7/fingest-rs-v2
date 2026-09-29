use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use fingest_kernel::{EventEnvelope, EventPublisher, PortError};

use crate::outbox::OutboxReader;

/// How often [`OutboxRelay::run`] purges old published rows when retention is set.
const PURGE_EVERY: Duration = Duration::from_secs(3600);

/// Upper bound on batches per tick, so a pass under sustained load still ends and the
/// purge still gets its turn. With the default batch size that is 10,000 events a tick.
const MAX_BATCHES_PER_PASS: usize = 100;

/// Drains the outbox and hands events to a publisher.
///
/// Delivery is **at-least-once**: if publishing succeeds but the row cannot be marked, the
/// event is re-delivered once its claim lapses. Subscribers must be idempotent. Several
/// relays may run against one database; each claims a disjoint batch.
pub struct OutboxRelay {
    reader: Arc<dyn OutboxReader>,
    publisher: Arc<dyn EventPublisher>,
    batch_size: i64,
    max_batches_per_pass: usize,
    retention: Option<Duration>,
}

impl OutboxRelay {
    pub fn new(reader: Arc<dyn OutboxReader>, publisher: Arc<dyn EventPublisher>) -> Self {
        Self {
            reader,
            publisher,
            batch_size: 100,
            max_batches_per_pass: MAX_BATCHES_PER_PASS,
            retention: None,
        }
    }

    pub fn with_batch_size(mut self, batch_size: i64) -> Self {
        self.batch_size = batch_size;
        self
    }

    pub fn with_max_batches_per_pass(mut self, max_batches: usize) -> Self {
        self.max_batches_per_pass = max_batches.max(1);
        self
    }

    /// Published rows older than `retention` are deleted; without this the table grows
    /// forever.
    pub fn with_retention(mut self, retention: Duration) -> Self {
        self.retention = Some(retention);
        self
    }

    /// Publishes one batch. Returns how many events were delivered.
    ///
    /// Rows are marked only after the publish succeeds, so a failing broker leaves them
    /// pending rather than silently dropping them.
    pub async fn drain_once(&self) -> Result<usize, PortError> {
        let pending = self.reader.claim_unpublished(self.batch_size).await?;
        if pending.is_empty() {
            return Ok(0);
        }

        let (ids, envelopes): (Vec<i64>, Vec<EventEnvelope>) = pending
            .into_iter()
            .map(|event| (event.id, event.envelope))
            .unzip();

        if let Err(err) = self.publisher.publish(&envelopes).await {
            // Best effort: if this fails too, the lease expires and the rows come back.
            if let Err(release_err) = self.reader.release(&ids).await {
                tracing::warn!(error = %release_err, "could not release outbox claim");
            }
            return Err(err);
        }
        self.reader.mark_published(&ids).await?;

        Ok(envelopes.len())
    }

    /// Publishes batches until one comes back short, up to the per-pass cap. Returns how
    /// many events were delivered.
    ///
    /// Draining one batch per tick capped throughput at `batch_size / interval` however
    /// large the backlog. An error ends the pass, so a failing publisher backs off to the
    /// tick interval instead of spinning.
    pub async fn drain_pass(&self) -> Result<usize, PortError> {
        let mut delivered = 0;

        for _ in 0..self.max_batches_per_pass {
            let count = match self.drain_once().await {
                Ok(count) => count,
                Err(err) => {
                    if delivered > 0 {
                        tracing::debug!(delivered, "outbox pass stopped after partial progress");
                    }
                    return Err(err);
                }
            };
            delivered += count;

            if (count as i64) < self.batch_size {
                break;
            }
            // Other tasks on this worker get a turn between full batches.
            tokio::task::yield_now().await;
        }

        Ok(delivered)
    }

    /// Deletes published rows past the retention window. A no-op when none is set.
    pub async fn purge_once(&self) -> Result<u64, PortError> {
        match self.retention {
            Some(retention) => self.reader.purge_published(retention).await,
            None => Ok(0),
        }
    }

    /// Polls until the process ends. Errors are logged and retried on the next tick, so a
    /// broker outage never takes the API down with it.
    pub async fn run(self, interval: Duration) {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_purge: Option<Instant> = None;

        loop {
            ticker.tick().await;

            match self.drain_pass().await {
                Ok(0) => {}
                Ok(count) => tracing::debug!(count, "outbox events published"),
                Err(err) => tracing::warn!(error = %err, "outbox relay pass failed; will retry"),
            }

            if last_purge.is_none_or(|at| at.elapsed() >= PURGE_EVERY) {
                last_purge = Some(Instant::now());
                match self.purge_once().await {
                    Ok(0) => {}
                    Ok(count) => tracing::debug!(count, "published outbox rows purged"),
                    Err(err) => tracing::warn!(error = %err, "outbox purge failed; will retry"),
                }
            }
        }
    }
}
