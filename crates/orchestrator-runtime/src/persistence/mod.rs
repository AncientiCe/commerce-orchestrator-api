//! File-backed persistent stores for production durability.

mod file_backed;
mod postgres_backed;

pub use file_backed::open_persistent_stores;
pub use file_backed::PersistentStores;
pub use postgres_backed::open_postgres_stores;
pub use postgres_backed::PostgresStores;
