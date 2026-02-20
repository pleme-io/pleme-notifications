//! Dependency health probe functions
//!
//! Provides check functions for common dependencies (PostgreSQL, Redis, NATS).
//! Each function returns a `DependencyCheck` with connection status and latency.
//!
//! These are designed to be called with the service's existing connection handles,
//! keeping the dependency on database/redis/nats crates in the service, not here.
//! The functions below are helper utilities and type-erased wrappers.

use crate::startup::DependencyCheck;
use std::time::{Duration, Instant};

/// Check PostgreSQL by executing a probe query.
///
/// Usage:
/// ```ignore
/// let check = check_postgres("database", || async {
///     sqlx::query("SELECT 1").execute(&pool).await.map(|_| ())
///         .map_err(|e| e.to_string())
/// }).await;
/// ```
pub async fn check_with_probe<F, Fut>(name: &str, probe: F) -> DependencyCheck
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<Option<String>, String>>,
{
    let start = Instant::now();
    match tokio::time::timeout(Duration::from_secs(5), probe()).await {
        Ok(Ok(detail)) => DependencyCheck {
            name: name.to_string(),
            connected: true,
            latency: start.elapsed(),
            detail,
        },
        Ok(Err(e)) => DependencyCheck {
            name: name.to_string(),
            connected: false,
            latency: start.elapsed(),
            detail: Some(e),
        },
        Err(_) => DependencyCheck {
            name: name.to_string(),
            connected: false,
            latency: start.elapsed(),
            detail: Some("timeout (5s)".to_string()),
        },
    }
}
