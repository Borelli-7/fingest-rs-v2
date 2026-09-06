//! In-memory doubles for use-case tests. No mocking framework: these are real
//! implementations of the port, so they stay honest about behaviour rather than
//! about call sequences.

use std::sync::Mutex;

use async_trait::async_trait;
use fingest_kernel::PortError;

use crate::{category::Category, port::CategoryRepository};

#[derive(Default)]
pub struct InMemoryCategoryRepository {
    rows: Mutex<Vec<Category>>,
}

impl InMemoryCategoryRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn seeded(categories: impl IntoIterator<Item = Category>) -> Self {
        Self {
            rows: Mutex::new(categories.into_iter().collect()),
        }
    }

    pub fn contains(&self, name: &str, profit: bool) -> bool {
        self.rows
            .lock()
            .expect("lock poisoned")
            .iter()
            .any(|c| c.name == name && c.profit == profit)
    }

    pub fn len(&self) -> usize {
        self.rows.lock().expect("lock poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[async_trait]
impl CategoryRepository for InMemoryCategoryRepository {
    async fn list(&self) -> Result<Vec<Category>, PortError> {
        Ok(self.rows.lock().expect("lock poisoned").clone())
    }

    async fn find(&self, name: &str, profit: bool) -> Result<Option<Category>, PortError> {
        Ok(self
            .rows
            .lock()
            .expect("lock poisoned")
            .iter()
            .find(|c| c.name == name && c.profit == profit)
            .cloned())
    }

    async fn find_conflict(
        &self,
        name: &str,
        profit: bool,
        excluding: Option<&str>,
    ) -> Result<Option<Category>, PortError> {
        Ok(self
            .rows
            .lock()
            .expect("lock poisoned")
            .iter()
            .find(|c| {
                c.profit == profit
                    && c.name.eq_ignore_ascii_case(name)
                    && excluding != Some(c.name.as_str())
            })
            .cloned())
    }

    async fn insert(&self, category: &Category) -> Result<Category, PortError> {
        self.rows
            .lock()
            .expect("lock poisoned")
            .push(category.clone());
        Ok(category.clone())
    }

    async fn rename(
        &self,
        name: &str,
        profit: bool,
        new_name: &str,
    ) -> Result<Category, PortError> {
        let mut rows = self.rows.lock().expect("lock poisoned");
        let row = rows
            .iter_mut()
            .find(|c| c.name == name && c.profit == profit)
            .ok_or_else(|| PortError::Storage("row disappeared".into()))?;
        row.name = new_name.to_owned();
        Ok(row.clone())
    }

    async fn delete(&self, name: &str, profit: bool) -> Result<u64, PortError> {
        let mut rows = self.rows.lock().expect("lock poisoned");
        let before = rows.len();
        rows.retain(|c| !(c.name == name && c.profit == profit));
        Ok((before - rows.len()) as u64)
    }
}

/// Port double that always fails, for checking error propagation.
pub struct FailingCategoryRepository(pub PortError);

#[async_trait]
impl CategoryRepository for FailingCategoryRepository {
    async fn list(&self) -> Result<Vec<Category>, PortError> {
        Err(self.0.clone())
    }
    async fn find(&self, _: &str, _: bool) -> Result<Option<Category>, PortError> {
        Err(self.0.clone())
    }
    async fn find_conflict(
        &self,
        _: &str,
        _: bool,
        _: Option<&str>,
    ) -> Result<Option<Category>, PortError> {
        Err(self.0.clone())
    }
    async fn insert(&self, _: &Category) -> Result<Category, PortError> {
        Err(self.0.clone())
    }
    async fn rename(&self, _: &str, _: bool, _: &str) -> Result<Category, PortError> {
        Err(self.0.clone())
    }
    async fn delete(&self, _: &str, _: bool) -> Result<u64, PortError> {
        Err(self.0.clone())
    }
}
