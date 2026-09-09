//! Catalog bounded context.
//!
//! Owns the `Category` aggregate, the `CategoryRepository` port and the four use cases
//! behind `/resources/categories`. Contains no I/O and no framework types.

pub mod category;
pub mod error;
pub mod port;
pub mod service;

#[cfg(any(test, feature = "test-support"))]
pub mod testing;

pub use category::Category;
pub use error::CatalogError;
pub use port::CategoryRepository;
pub use service::CategoryService;
