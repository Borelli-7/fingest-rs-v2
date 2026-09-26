//! In-memory doubles for wallets use-case tests.
//!
//! The fake transaction buffers its operations and applies them only on `commit`, so
//! dropping it without committing really does discard the work. That makes the atomicity
//! guarantee testable without a database.

use std::{
    collections::HashSet,
    sync::{
        Mutex,
        atomic::{AtomicI32, Ordering},
    },
};

use async_trait::async_trait;
use fingest_kernel::{CategoryRef, DateRange, EventEnvelope, Money, PortError};

use crate::{
    expense::Expense,
    port::{UnitOfWork, WalletReader, WalletTx},
    wallet::Wallet,
};

#[derive(Debug, Clone)]
struct OwnedWallet {
    owner: String,
    wallet: Wallet,
}

#[derive(Debug, Clone)]
struct StoredExpense {
    wallet_id: i32,
    expense: Expense,
}

#[derive(Default)]
pub struct InMemoryStore {
    owners: Mutex<HashSet<String>>,
    categories: Mutex<HashSet<(String, bool)>>,
    wallets: Mutex<Vec<OwnedWallet>>,
    expenses: Mutex<Vec<StoredExpense>>,
    events: Mutex<Vec<EventEnvelope>>,
    next_wallet_id: AtomicI32,
    next_expense_id: AtomicI32,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self {
            next_wallet_id: AtomicI32::new(1),
            next_expense_id: AtomicI32::new(1),
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

    /// Seeds a wallet directly, bypassing the transaction.
    pub fn with_wallet(self, owner: &str, wallet: Wallet) -> Self {
        let id = wallet
            .id
            .unwrap_or_else(|| self.next_wallet_id.fetch_add(1, Ordering::SeqCst));
        self.wallets
            .lock()
            .expect("lock poisoned")
            .push(OwnedWallet {
                owner: owner.to_owned(),
                wallet: wallet.with_id(id),
            });
        self
    }

    pub fn with_expense(self, wallet_id: i32, expense: Expense) -> Self {
        let id = expense
            .id
            .unwrap_or_else(|| self.next_expense_id.fetch_add(1, Ordering::SeqCst));
        self.expenses
            .lock()
            .expect("lock poisoned")
            .push(StoredExpense {
                wallet_id,
                expense: expense.with_id(id),
            });
        self
    }

    pub fn balance_of(&self, wallet_id: i32) -> Option<Money> {
        self.wallets
            .lock()
            .expect("lock poisoned")
            .iter()
            .find(|row| row.wallet.id == Some(wallet_id))
            .map(|row| row.wallet.amount.clone())
    }

    pub fn expense_count(&self) -> usize {
        self.expenses.lock().expect("lock poisoned").len()
    }

    pub fn wallet_count(&self) -> usize {
        self.wallets.lock().expect("lock poisoned").len()
    }

    pub fn recorded_events(&self) -> Vec<String> {
        self.events
            .lock()
            .expect("lock poisoned")
            .iter()
            .map(|e| e.event_type.to_owned())
            .collect()
    }

    fn find_owned_now(&self, login: &str, wallet_id: i32) -> Option<Wallet> {
        self.wallets
            .lock()
            .expect("lock poisoned")
            .iter()
            .find(|row| row.owner == login && row.wallet.id == Some(wallet_id))
            .map(|row| row.wallet.clone())
    }
}

#[async_trait]
impl WalletReader for InMemoryStore {
    async fn owner_exists(&self, login: &str) -> Result<bool, PortError> {
        Ok(self.owners.lock().expect("lock poisoned").contains(login))
    }

    async fn list_for_owner(&self, login: &str) -> Result<Vec<Wallet>, PortError> {
        Ok(self
            .wallets
            .lock()
            .expect("lock poisoned")
            .iter()
            .filter(|row| row.owner == login)
            .map(|row| row.wallet.clone())
            .collect())
    }

    async fn find_owned(&self, login: &str, wallet_id: i32) -> Result<Option<Wallet>, PortError> {
        Ok(self.find_owned_now(login, wallet_id))
    }

    async fn wallet_exists(&self, wallet_id: i32) -> Result<bool, PortError> {
        Ok(self
            .wallets
            .lock()
            .expect("lock poisoned")
            .iter()
            .any(|row| row.wallet.id == Some(wallet_id)))
    }

    async fn list_expenses(
        &self,
        wallet_id: i32,
        range: &DateRange,
    ) -> Result<Vec<Expense>, PortError> {
        Ok(self
            .expenses
            .lock()
            .expect("lock poisoned")
            .iter()
            .filter(|row| row.wallet_id == wallet_id && range.contains_date(row.expense.date))
            .map(|row| row.expense.clone())
            .collect())
    }

    async fn highest_expense(
        &self,
        wallet_id: i32,
        range: &DateRange,
    ) -> Result<Option<Expense>, PortError> {
        Ok(self
            .expenses
            .lock()
            .expect("lock poisoned")
            .iter()
            .filter(|row| {
                row.wallet_id == wallet_id
                    && range.contains_date(row.expense.date)
                    && !row.expense.is_income()
            })
            .max_by(|a, b| {
                a.expense
                    .amount
                    .partial_cmp(&b.expense.amount)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|row| row.expense.clone()))
    }

    async fn find_expense(
        &self,
        wallet_id: i32,
        expense_id: i32,
    ) -> Result<Option<Expense>, PortError> {
        Ok(self
            .expenses
            .lock()
            .expect("lock poisoned")
            .iter()
            .find(|row| row.wallet_id == wallet_id && row.expense.id == Some(expense_id))
            .map(|row| row.expense.clone()))
    }
}

enum Op {
    InsertWallet { owner: String, wallet: Wallet },
    RenameWallet { wallet_id: i32, name: String },
    DeleteWallet(i32),
    InsertExpense { wallet_id: i32, expense: Expense },
    UpdateExpense(Expense),
    DeleteExpense { wallet_id: i32, expense_id: i32 },
    AdjustBalance { wallet_id: i32, delta: Money },
    AppendEvents(Vec<EventEnvelope>),
}

pub struct InMemoryUnitOfWork {
    store: std::sync::Arc<InMemoryStore>,
}

impl InMemoryUnitOfWork {
    pub fn new(store: std::sync::Arc<InMemoryStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl UnitOfWork for InMemoryUnitOfWork {
    async fn begin(&self) -> Result<Box<dyn WalletTx>, PortError> {
        Ok(Box::new(InMemoryTx {
            store: std::sync::Arc::clone(&self.store),
            ops: Vec::new(),
        }))
    }
}

pub struct InMemoryTx {
    store: std::sync::Arc<InMemoryStore>,
    ops: Vec<Op>,
}

#[async_trait]
impl WalletTx for InMemoryTx {
    async fn category_exists(&mut self, category: &CategoryRef) -> Result<bool, PortError> {
        Ok(self
            .store
            .categories
            .lock()
            .expect("lock poisoned")
            .contains(&(category.name.clone(), category.profit)))
    }

    async fn find_owned(
        &mut self,
        login: &str,
        wallet_id: i32,
    ) -> Result<Option<Wallet>, PortError> {
        Ok(self.store.find_owned_now(login, wallet_id))
    }

    async fn insert_wallet(&mut self, login: &str, wallet: &Wallet) -> Result<i32, PortError> {
        let id = self.store.next_wallet_id.fetch_add(1, Ordering::SeqCst);
        self.ops.push(Op::InsertWallet {
            owner: login.to_owned(),
            wallet: wallet.clone().with_id(id),
        });
        Ok(id)
    }

    async fn rename_wallet(&mut self, wallet_id: i32, name: &str) -> Result<u64, PortError> {
        self.ops.push(Op::RenameWallet {
            wallet_id,
            name: name.to_owned(),
        });
        Ok(1)
    }

    async fn delete_wallet(&mut self, wallet_id: i32) -> Result<u64, PortError> {
        let exists = self
            .store
            .wallets
            .lock()
            .expect("lock poisoned")
            .iter()
            .any(|row| row.wallet.id == Some(wallet_id));

        self.ops.push(Op::DeleteWallet(wallet_id));
        Ok(u64::from(exists))
    }

    async fn insert_expense(
        &mut self,
        wallet_id: i32,
        expense: &Expense,
    ) -> Result<i32, PortError> {
        let id = self.store.next_expense_id.fetch_add(1, Ordering::SeqCst);
        self.ops.push(Op::InsertExpense {
            wallet_id,
            expense: expense.clone().with_id(id),
        });
        Ok(id)
    }

    async fn update_expense(&mut self, expense: &Expense) -> Result<u64, PortError> {
        self.ops.push(Op::UpdateExpense(expense.clone()));
        Ok(1)
    }

    async fn delete_expense(&mut self, wallet_id: i32, expense_id: i32) -> Result<u64, PortError> {
        let exists = self
            .store
            .expenses
            .lock()
            .expect("lock poisoned")
            .iter()
            .any(|row| row.wallet_id == wallet_id && row.expense.id == Some(expense_id));

        self.ops.push(Op::DeleteExpense {
            wallet_id,
            expense_id,
        });
        Ok(u64::from(exists))
    }

    async fn adjust_balance(&mut self, wallet_id: i32, delta: &Money) -> Result<(), PortError> {
        self.ops.push(Op::AdjustBalance {
            wallet_id,
            delta: delta.clone(),
        });
        Ok(())
    }

    async fn append_events(&mut self, events: &[EventEnvelope]) -> Result<(), PortError> {
        self.ops.push(Op::AppendEvents(events.to_vec()));
        Ok(())
    }

    async fn commit(self: Box<Self>) -> Result<(), PortError> {
        let mut wallets = self.store.wallets.lock().expect("lock poisoned");
        let mut expenses = self.store.expenses.lock().expect("lock poisoned");
        let mut events = self.store.events.lock().expect("lock poisoned");

        for op in self.ops {
            match op {
                Op::InsertWallet { owner, wallet } => wallets.push(OwnedWallet { owner, wallet }),
                Op::RenameWallet { wallet_id, name } => {
                    if let Some(row) = wallets.iter_mut().find(|r| r.wallet.id == Some(wallet_id)) {
                        row.wallet.name = name;
                    }
                }
                Op::DeleteWallet(id) => wallets.retain(|r| r.wallet.id != Some(id)),
                Op::InsertExpense { wallet_id, expense } => {
                    expenses.push(StoredExpense { wallet_id, expense });
                }
                Op::UpdateExpense(updated) => {
                    if let Some(row) = expenses.iter_mut().find(|r| r.expense.id == updated.id) {
                        row.expense = updated;
                    }
                }
                Op::DeleteExpense {
                    wallet_id,
                    expense_id,
                } => expenses
                    .retain(|r| !(r.wallet_id == wallet_id && r.expense.id == Some(expense_id))),
                Op::AdjustBalance { wallet_id, delta } => {
                    if let Some(row) = wallets.iter_mut().find(|r| r.wallet.id == Some(wallet_id)) {
                        row.wallet.amount = row
                            .wallet
                            .amount
                            .add(&delta)
                            .map_err(|e| PortError::Storage(e.to_string()))?;
                    }
                }
                Op::AppendEvents(envelopes) => events.extend(envelopes),
            }
        }

        Ok(())
    }
}
