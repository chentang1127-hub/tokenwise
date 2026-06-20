//! Recording layer — per-call logging to local SQLite.

mod model;
pub mod store;

pub use model::CallRecord;
pub use store::Store;
