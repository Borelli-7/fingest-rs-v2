//! In-memory doubles for planning use-case tests.

use std::{
    collections::HashSet,
    sync::{
        Mutex,
        atomic::{AtomicI32, Ordering},
    },
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use fingest_kernel::{CategoryRef, DateRange, DomainEvent, EventEnvelope, Money, PortError};

use crate::{
    budget::{Budget, BudgetWithSpent},
    port::BudgetRepository,
};

struct OwnedBudget {
    owner: String,
    budget: Budget,
    spent: Option<Money>,
}

#[derive(Default)]
pub struct InMemoryBudgetRepository {
    owners: Mutex<HashSet<String>>,
    categories: Mutex<HashSet<(String, bool)>>,
    rows: Mutex<Vec<OwnedBudget>>,
    events: Mutex<Vec<EventEnvelope>>,
    next_id: AtomicI32,
}

impl InMemoryBudgetRepository {
    pub fn new() -> Self {
        Self {
            next_id: AtomicI32::new(1),
            ..Default::default()
        }
    }

    pub fn with_owner(self, login: &str) -> Self {
        self.owners
            .lock()
            .expect("lock poisoned")
            .insert(login.to_owned());
        self
    }

    pub fn with_category(self, name: &str, profit: bool) -> Self {
        self.categories
            .lock()
            .expect("lock poisoned")
            .insert((name.to_owned(), profit));
        self
    }

    /// Stands in for the spending sum the real query computes.
    pub fn set_spent(&self, budget_id: i32, spent: Money) {
        if let Some(row) = self
            .rows
            .lock()
            .expect("lock poisoned")
            .iter_mut()
            .find(|r| r.budget.id == Some(budget_id))
        {
            row.spent = Some(spent);
        }
    }

    pub fn len(&self) -> usize {
        self.rows.lock().expect("lock poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn event_types(&self) -> Vec<String> {
        self.events
            .lock()
            .expect("lock poisoned")
            .iter()
            .map(|e| e.event_type.clone())
            .collect()
    }
}

#[async_trait]
impl BudgetRepository for InMemoryBudgetRepository {
    async fn owner_exists(&self, login: &str) -> Result<bool, PortError> {
        Ok(self.owners.lock().expect("lock poisoned").contains(login))
    }

    async fn category_exists(&self, category: &CategoryRef) -> Result<bool, PortError> {
        Ok(self
            .categories
            .lock()
            .expect("lock poisoned")
            .contains(&(category.name.clone(), category.profit)))
    }

    async fn list_with_spent(
        &self,
        login: &str,
        _start_window: &DateRange,
        _end_window: &DateRange,
    ) -> Result<Vec<BudgetWithSpent>, PortError> {
        Ok(self
            .rows
            .lock()
            .expect("lock poisoned")
            .iter()
            .filter(|row| row.owner == login)
            .map(|row| BudgetWithSpent {
                budget: row.budget.clone(),
                spent: row
                    .spent
                    .clone()
                    .unwrap_or_else(|| Money::zero(row.budget.total.currency.clone())),
            })
            .collect())
    }

    async fn find_with_owner(&self, budget_id: i32) -> Result<Option<(Budget, String)>, PortError> {
        Ok(self
            .rows
            .lock()
            .expect("lock poisoned")
            .iter()
            .find(|row| row.budget.id == Some(budget_id))
            .map(|row| (row.budget.clone(), row.owner.clone())))
    }

    async fn insert(
        &self,
        login: &str,
        budget: &Budget,
        occurred_at: DateTime<Utc>,
    ) -> Result<i32, PortError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        self.rows.lock().expect("lock poisoned").push(OwnedBudget {
            owner: login.to_owned(),
            budget: budget.clone().with_id(id),
            spent: None,
        });

        let event = DomainEvent::BudgetCreated {
            login: login.to_owned(),
            budget_id: id,
        };
        self.events
            .lock()
            .expect("lock poisoned")
            .push(EventEnvelope::new(&event, occurred_at)?);

        Ok(id)
    }

    async fn update(&self, budget: &Budget) -> Result<u64, PortError> {
        let mut rows = self.rows.lock().expect("lock poisoned");
        let Some(row) = rows.iter_mut().find(|r| r.budget.id == budget.id) else {
            return Ok(0);
        };
        row.budget = budget.clone();
        Ok(1)
    }

    async fn delete(&self, budget_id: i32) -> Result<u64, PortError> {
        let mut rows = self.rows.lock().expect("lock poisoned");
        let before = rows.len();
        rows.retain(|row| row.budget.id != Some(budget_id));
        Ok((before - rows.len()) as u64)
    }
}
