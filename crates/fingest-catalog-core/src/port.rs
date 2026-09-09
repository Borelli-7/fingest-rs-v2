use async_trait::async_trait;
use fingest_kernel::PortError;

use crate::category::Category;

/// Outbound port for category persistence.
///
/// Declared with `async_trait` so it stays dyn-compatible: native `async fn` in traits is
/// stable but not usable behind `dyn` as of Rust 1.98.1.
#[async_trait]
pub trait CategoryRepository: Send + Sync {
    async fn list(&self) -> Result<Vec<Category>, PortError>;

    /// Exact `(name, profit)` lookup.
    async fn find(&self, name: &str, profit: bool) -> Result<Option<Category>, PortError>;

    /// Case-insensitive duplicate lookup. `excluding` skips one exact name, which a rename
    /// needs so a category never conflicts with itself.
    async fn find_conflict(
        &self,
        name: &str,
        profit: bool,
        excluding: Option<&str>,
    ) -> Result<Option<Category>, PortError>;

    async fn insert(&self, category: &Category) -> Result<Category, PortError>;

    async fn rename(&self, name: &str, profit: bool, new_name: &str)
    -> Result<Category, PortError>;

    /// Returns the number of rows removed.
    async fn delete(&self, name: &str, profit: bool) -> Result<u64, PortError>;
}
