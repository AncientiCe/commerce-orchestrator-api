//! Task-local correlation ID context.

tokio::task_local! {
    static CORRELATION_ID: String;
}

pub async fn with_correlation_id<F, R>(correlation_id: String, future: F) -> R
where
    F: std::future::Future<Output = R>,
{
    CORRELATION_ID.scope(correlation_id, future).await
}

pub fn current_correlation_id() -> Option<String> {
    CORRELATION_ID.try_with(|value| value.clone()).ok()
}
