//! Error type for store persistence failures (e.g. file-backed save).

#[derive(Debug, thiserror::Error)]
#[error("store persistence failed: {0}")]
pub struct StoreError(#[from] pub std::io::Error);
