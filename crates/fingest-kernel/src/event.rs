use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{error::PortError, money::Money};

/// Facts that have already happened. Emitted by aggregates, written to the outbox in the
/// same transaction as the state change they describe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DomainEvent {
    AccountRegistered {
        login: String,
        admin: bool,
    },
    WalletCreated {
        login: String,
        wallet_id: i32,
    },
    ExpenseRecorded {
        wallet_id: i32,
        expense_id: i32,
        amount: Money,
        category_name: String,
        category_profit: bool,
    },
    ExpenseUpdated {
        wallet_id: i32,
        expense_id: i32,
    },
    ExpenseRemoved {
        wallet_id: i32,
        expense_id: i32,
    },
    WalletBalanceAdjusted {
        wallet_id: i32,
        delta: Money,
        balance: Money,
    },
    BudgetCreated {
        login: String,
        budget_id: i32,
    },
}

impl DomainEvent {
    /// Outbox `aggregate` column.
    pub fn aggregate(&self) -> &'static str {
        match self {
            Self::AccountRegistered { .. } => "account",
            Self::WalletCreated { .. }
            | Self::WalletBalanceAdjusted { .. }
            | Self::ExpenseRecorded { .. }
            | Self::ExpenseUpdated { .. }
            | Self::ExpenseRemoved { .. } => "wallet",
            Self::BudgetCreated { .. } => "budget",
        }
    }

    /// Outbox `event_type` column.
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::AccountRegistered { .. } => "AccountRegistered",
            Self::WalletCreated { .. } => "WalletCreated",
            Self::ExpenseRecorded { .. } => "ExpenseRecorded",
            Self::ExpenseUpdated { .. } => "ExpenseUpdated",
            Self::ExpenseRemoved { .. } => "ExpenseRemoved",
            Self::WalletBalanceAdjusted { .. } => "WalletBalanceAdjusted",
            Self::BudgetCreated { .. } => "BudgetCreated",
        }
    }
}

/// An event plus the metadata the outbox table and any downstream broker need.
///
/// Fields are owned rather than `&'static str` so a row read back out of the outbox can be
/// turned into an envelope again.
#[derive(Debug, Clone, PartialEq)]
pub struct EventEnvelope {
    pub aggregate: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub occurred_at: DateTime<Utc>,
}

impl EventEnvelope {
    pub fn new(event: &DomainEvent, occurred_at: DateTime<Utc>) -> Result<Self, PortError> {
        Ok(Self {
            aggregate: event.aggregate().to_owned(),
            event_type: event.event_type().to_owned(),
            payload: serde_json::to_value(event).map_err(|e| PortError::Encoding(e.to_string()))?,
            occurred_at,
        })
    }
}

/// Outbound port. The default adapter appends to the outbox inside the caller's transaction;
/// a broker-backed adapter can replace it without any change above this line.
#[async_trait::async_trait]
pub trait EventPublisher: Send + Sync {
    async fn publish(&self, events: &[EventEnvelope]) -> Result<(), PortError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::currency::Currency;
    use bigdecimal::BigDecimal;

    fn at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-05T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn expense_events_belong_to_the_wallet_aggregate() {
        let event = DomainEvent::ExpenseRemoved {
            wallet_id: 1,
            expense_id: 2,
        };
        assert_eq!(event.aggregate(), "wallet");
        assert_eq!(event.event_type(), "ExpenseRemoved");
    }

    #[test]
    fn envelope_carries_a_tagged_payload() {
        let event = DomainEvent::BudgetCreated {
            login: "user1".into(),
            budget_id: 7,
        };
        let envelope = EventEnvelope::new(&event, at()).unwrap();
        assert_eq!(envelope.aggregate, "budget");
        assert_eq!(envelope.payload["type"], "BudgetCreated");
        assert_eq!(envelope.payload["budget_id"], 7);
        assert_eq!(envelope.occurred_at, at());
    }

    #[test]
    fn envelope_round_trips_money() {
        let event = DomainEvent::WalletBalanceAdjusted {
            wallet_id: 3,
            delta: Money::new(BigDecimal::from(-50), Currency::default()),
            balance: Money::new(BigDecimal::from(950), Currency::default()),
        };
        let envelope = EventEnvelope::new(&event, at()).unwrap();
        let decoded: DomainEvent = serde_json::from_value(envelope.payload).unwrap();
        assert_eq!(decoded, event);
    }
}
