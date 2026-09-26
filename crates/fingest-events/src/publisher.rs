use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fingest_kernel::{EventEnvelope, EventPublisher, PortError};
use tokio::sync::broadcast;

/// Fan-out to in-process subscribers.
///
/// The default backend: no broker to operate, and the port means a NATS or Kafka adapter
/// can replace it without touching anything above.
pub struct InProcessPublisher {
    sender: broadcast::Sender<EventEnvelope>,
}

impl InProcessPublisher {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EventEnvelope> {
        self.sender.subscribe()
    }
}

impl Default for InProcessPublisher {
    fn default() -> Self {
        Self::new(1024)
    }
}

#[async_trait]
impl EventPublisher for InProcessPublisher {
    /// With no subscriber the events would reach nobody, yet the relay would mark them
    /// delivered. Failing keeps them pending until a subscriber is attached.
    async fn publish(&self, events: &[EventEnvelope]) -> Result<(), PortError> {
        for event in events {
            self.sender.send(event.clone()).map_err(|_| {
                PortError::Unavailable("no in-process subscriber is attached".to_owned())
            })?;
        }
        Ok(())
    }
}

/// Writes each event to the log. Useful on its own, and as a subscriber-free default.
pub struct TracingPublisher;

#[async_trait]
impl EventPublisher for TracingPublisher {
    async fn publish(&self, events: &[EventEnvelope]) -> Result<(), PortError> {
        for event in events {
            tracing::info!(
                aggregate = %event.aggregate,
                event_type = %event.event_type,
                occurred_at = %event.occurred_at,
                "domain event published"
            );
        }
        Ok(())
    }
}

/// Records everything it is given, for tests.
#[derive(Default)]
pub struct RecordingPublisher {
    published: Mutex<Vec<EventEnvelope>>,
}

impl RecordingPublisher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn event_types(&self) -> Vec<String> {
        self.published
            .lock()
            .expect("lock poisoned")
            .iter()
            .map(|e| e.event_type.clone())
            .collect()
    }
}

#[async_trait]
impl EventPublisher for RecordingPublisher {
    async fn publish(&self, events: &[EventEnvelope]) -> Result<(), PortError> {
        self.published
            .lock()
            .expect("lock poisoned")
            .extend(events.iter().cloned());
        Ok(())
    }
}

/// Fails every publish, so the relay's retry behaviour can be exercised.
pub struct FailingPublisher(pub Arc<str>);

#[async_trait]
impl EventPublisher for FailingPublisher {
    async fn publish(&self, _: &[EventEnvelope]) -> Result<(), PortError> {
        Err(PortError::Unavailable(self.0.to_string()))
    }
}
