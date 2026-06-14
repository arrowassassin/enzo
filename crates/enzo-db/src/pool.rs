//! Async connection pool.
//!
//! Wraps a `Driver` in an `Arc` so it can be shared across tasks.
//! For drivers that maintain per-connection state, callers can use
//! `tokio::sync::Semaphore` to bound concurrency externally.

use std::sync::Arc;

use crate::driver::{Driver, QueryResult};

/// A cheaply-cloneable handle to a shared driver instance.
pub struct Pool<D: Driver> {
    inner: Arc<D>,
}

impl<D: Driver> Clone for Pool<D> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<D: Driver> Pool<D> {
    /// Wrap a driver in a pool.
    pub fn new(driver: D) -> Self {
        Self {
            inner: Arc::new(driver),
        }
    }

    /// Execute a SQL query and return Arrow record batches.
    pub async fn query(&self, sql: &str) -> anyhow::Result<QueryResult> {
        self.inner.query(sql).await
    }

    /// Execute a SQL statement that produces no result set.
    pub async fn execute(&self, sql: &str) -> anyhow::Result<u64> {
        self.inner.execute(sql).await
    }

    /// Return the underlying driver name.
    #[must_use]
    pub fn driver_name(&self) -> &'static str {
        self.inner.name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::MemDriver;

    #[tokio::test]
    async fn pool_query_delegates_to_driver() {
        let pool = Pool::new(MemDriver);
        let batches = pool.query("SELECT 1").await.unwrap();
        assert_eq!(batches.len(), 1);
    }

    #[tokio::test]
    async fn pool_clone_shares_driver() {
        let pool = Pool::new(MemDriver);
        let pool2 = pool.clone();
        assert_eq!(pool.driver_name(), pool2.driver_name());
    }

    #[test]
    fn pool_driver_name() {
        let pool = Pool::new(MemDriver);
        assert_eq!(pool.driver_name(), "mem");
    }
}
