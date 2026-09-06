//! Event publishing and the transactional outbox relay.
//!
//! Writers append to the `outbox` table inside their own transaction; this crate moves
//! those rows to a publisher afterwards. Delivery is at-least-once.

mod outbox;
mod publisher;
mod relay;

pub use outbox::{OutboxReader, PendingEvent, PgOutboxReader};
pub use publisher::{FailingPublisher, InProcessPublisher, RecordingPublisher, TracingPublisher};
pub use relay::OutboxRelay;

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use fingest_kernel::{DomainEvent, EventEnvelope, EventPublisher};
    use sqlx::PgPool;
    use std::sync::Arc;

    fn envelope(login: &str) -> EventEnvelope {
        let event = DomainEvent::AccountRegistered {
            login: login.to_owned(),
            admin: false,
        };
        EventEnvelope::new(&event, Utc::now()).unwrap()
    }

    async fn append(pool: &PgPool, envelope: &EventEnvelope) {
        sqlx::query!(
            r#"INSERT INTO outbox (aggregate, event_type, payload, occurred_at)
               VALUES ($1, $2, $3, $4)"#,
            envelope.aggregate.as_str(),
            envelope.event_type.as_str(),
            envelope.payload,
            envelope.occurred_at
        )
        .execute(pool)
        .await
        .unwrap();
    }

    async fn unpublished_count(pool: &PgPool) -> i64 {
        sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count!" FROM outbox WHERE published_at IS NULL"#)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    fn relay(pool: &PgPool, publisher: Arc<dyn EventPublisher>) -> OutboxRelay {
        OutboxRelay::new(Arc::new(PgOutboxReader::new(pool.clone())), publisher)
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn an_empty_outbox_publishes_nothing(pool: PgPool) {
        let publisher = Arc::new(RecordingPublisher::new());

        let count = relay(&pool, publisher.clone()).drain_once().await.unwrap();

        assert_eq!(count, 0);
        assert!(publisher.event_types().is_empty());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn pending_events_are_published_and_marked(pool: PgPool) {
        append(&pool, &envelope("alice")).await;
        append(&pool, &envelope("bob")).await;
        let publisher = Arc::new(RecordingPublisher::new());

        let count = relay(&pool, publisher.clone()).drain_once().await.unwrap();

        assert_eq!(count, 2);
        assert_eq!(
            publisher.event_types(),
            vec!["AccountRegistered", "AccountRegistered"]
        );
        assert_eq!(unpublished_count(&pool).await, 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_second_pass_does_not_republish(pool: PgPool) {
        append(&pool, &envelope("alice")).await;
        let publisher = Arc::new(RecordingPublisher::new());
        let relay = relay(&pool, publisher.clone());

        assert_eq!(relay.drain_once().await.unwrap(), 1);
        assert_eq!(relay.drain_once().await.unwrap(), 0);
        assert_eq!(publisher.event_types().len(), 1);
    }

    /// A broker outage must leave the row pending, never silently drop it.
    #[sqlx::test(migrations = "../../migrations")]
    async fn a_failing_publisher_leaves_rows_pending(pool: PgPool) {
        append(&pool, &envelope("alice")).await;
        let publisher = Arc::new(FailingPublisher("broker down".into()));

        let err = relay(&pool, publisher).drain_once().await.unwrap_err();

        assert!(matches!(err, fingest_kernel::PortError::Unavailable(_)));
        assert_eq!(unpublished_count(&pool).await, 1, "row must stay pending");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn recovery_after_an_outage_delivers_the_backlog(pool: PgPool) {
        append(&pool, &envelope("alice")).await;

        let _ = relay(&pool, Arc::new(FailingPublisher("down".into())))
            .drain_once()
            .await;

        let publisher = Arc::new(RecordingPublisher::new());
        let count = relay(&pool, publisher.clone()).drain_once().await.unwrap();

        assert_eq!(count, 1);
        assert_eq!(unpublished_count(&pool).await, 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn the_batch_size_is_respected(pool: PgPool) {
        for login in ["a", "b", "c"] {
            append(&pool, &envelope(login)).await;
        }
        let publisher = Arc::new(RecordingPublisher::new());
        let relay = relay(&pool, publisher.clone()).with_batch_size(2);

        assert_eq!(relay.drain_once().await.unwrap(), 2);
        assert_eq!(relay.drain_once().await.unwrap(), 1);
        assert_eq!(unpublished_count(&pool).await, 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn the_payload_survives_the_round_trip(pool: PgPool) {
        append(&pool, &envelope("alice")).await;
        let publisher = Arc::new(RecordingPublisher::new());
        let reader = PgOutboxReader::new(pool.clone());

        let pending = reader.fetch_unpublished(10).await.unwrap();
        let decoded: DomainEvent =
            serde_json::from_value(pending[0].envelope.payload.clone()).unwrap();

        assert_eq!(
            decoded,
            DomainEvent::AccountRegistered {
                login: "alice".into(),
                admin: false
            }
        );
        let _ = publisher;
    }

    #[tokio::test]
    async fn in_process_subscribers_receive_published_events() {
        let publisher = InProcessPublisher::new(8);
        let mut subscriber = publisher.subscribe();

        publisher.publish(&[envelope("alice")]).await.unwrap();

        let received = subscriber.recv().await.unwrap();
        assert_eq!(received.event_type, "AccountRegistered");
    }

    #[tokio::test]
    async fn publishing_without_subscribers_is_not_an_error() {
        let publisher = InProcessPublisher::new(8);
        assert!(publisher.publish(&[envelope("alice")]).await.is_ok());
    }
}
