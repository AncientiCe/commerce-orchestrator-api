//! File-backed persistent stores for production durability.

mod file_backed;

pub use file_backed::open_persistent_stores;
pub use file_backed::PersistentStores;
