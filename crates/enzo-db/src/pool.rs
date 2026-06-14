//! Async connection pool.
//!
//! Wraps a `Driver` in an `Arc` so it can be shared across tasks.
//! For drivers that maintain per-connection state, callers can use
//! `tokio::sync::Semaphore` to bound concurrency externally.

use std::sync::Arc;

use crate::driver::{Driver, MemDriver, QueryResult};
use crate::duckdb::DuckDbDriver;
use crate::sqlite::SqliteDriver;

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

// ── AnyPool ───────────────────────────────────────────────────────────────────

/// Type-erased pool for use where the concrete driver is not known at compile time.
///
/// Avoids `dyn Driver` (which is not object-safe due to async trait methods) by
/// using enum dispatch.
#[derive(Clone)]
pub enum AnyPool {
    /// `SQLite` connection pool.
    Sqlite(Pool<SqliteDriver>),
    /// `DuckDB` connection pool.
    DuckDb(Pool<DuckDbDriver>),
    /// In-memory mock pool (testing / demo).
    Mem(Pool<MemDriver>),
}

impl AnyPool {
    /// Open a `SQLite` pool at `path` (use `":memory:"` for a transient DB).
    pub fn sqlite(path: &str) -> anyhow::Result<Self> {
        Ok(Self::Sqlite(Pool::new(SqliteDriver::open(path)?)))
    }

    /// Open a `DuckDB` pool at `path` (use `":memory:"` for a transient DB).
    pub fn duckdb(path: &str) -> anyhow::Result<Self> {
        Ok(Self::DuckDb(Pool::new(DuckDbDriver::open(path)?)))
    }

    /// Open a pool for the named `driver` (`"sqlite"` | `"duckdb"`) at `path`.
    /// An unknown/empty driver name is inferred from the path extension, then
    /// falls back to `SQLite`.
    pub fn open(driver: &str, path: &str) -> anyhow::Result<Self> {
        match driver.to_ascii_lowercase().as_str() {
            "duckdb" | "duck" => Self::duckdb(path),
            "sqlite" | "sqlite3" => Self::sqlite(path),
            _ => match driver_from_path(path) {
                "duckdb" => Self::duckdb(path),
                _ => Self::sqlite(path),
            },
        }
    }

    /// Create an in-memory mock pool.
    #[must_use]
    pub fn mem() -> Self {
        Self::Mem(Pool::new(MemDriver))
    }

    /// Execute a SQL query and return Arrow record batches.
    pub async fn query(&self, sql: &str) -> anyhow::Result<QueryResult> {
        match self {
            Self::Sqlite(p) => p.query(sql).await,
            Self::DuckDb(p) => p.query(sql).await,
            Self::Mem(p) => p.query(sql).await,
        }
    }

    /// Execute a SQL statement that produces no result set.
    pub async fn execute(&self, sql: &str) -> anyhow::Result<u64> {
        match self {
            Self::Sqlite(p) => p.execute(sql).await,
            Self::DuckDb(p) => p.execute(sql).await,
            Self::Mem(p) => p.execute(sql).await,
        }
    }

    /// Return the underlying driver name.
    #[must_use]
    pub fn driver_name(&self) -> &'static str {
        match self {
            Self::Sqlite(p) => p.driver_name(),
            Self::DuckDb(p) => p.driver_name(),
            Self::Mem(p) => p.driver_name(),
        }
    }
}

/// Infer a driver name from a database file's extension.
fn driver_from_path(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".duckdb") || lower.ends_with(".ddb") {
        "duckdb"
    } else {
        "sqlite"
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
