//! `DuckDB` driver — wraps the `duckdb` crate in a `spawn_blocking` async adapter.
//!
//! `DuckDB` speaks Arrow natively, so queries are collected straight into the
//! same `arrow` record batches the rest of the engine uses (the workspace and
//! `duckdb` share a single `arrow` version, so the types unify). This is the
//! analytical engine that makes Enzo's DB surface a real Harlequin-style IDE
//! (local files, Parquet/CSV, window functions, …).

use std::sync::{Arc, Mutex};

use arrow::array::RecordBatch;

use crate::driver::{Driver, QueryResult};

/// Async `DuckDB` driver backed by bundled libduckdb.
///
/// A single connection is shared behind a `Mutex`; concurrent queries serialise
/// (DuckDB's connection object is not `Sync`).
pub struct DuckDbDriver {
    conn: Arc<Mutex<::duckdb::Connection>>,
}

impl DuckDbDriver {
    /// Open (or create) a `DuckDB` database at `path`.
    ///
    /// Pass `":memory:"` (or an empty string) for a transient in-process DB.
    pub fn open(path: &str) -> anyhow::Result<Self> {
        let conn = if path.is_empty() || path == ":memory:" {
            ::duckdb::Connection::open_in_memory()?
        } else {
            ::duckdb::Connection::open(path)?
        };
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

impl Driver for DuckDbDriver {
    async fn query(&self, sql: &str) -> anyhow::Result<QueryResult> {
        let conn = Arc::clone(&self.conn);
        let sql = sql.to_owned();
        tokio::task::spawn_blocking(move || {
            let guard = conn
                .lock()
                .map_err(|e| anyhow::anyhow!("duckdb mutex poisoned: {e}"))?;
            query_sync(&guard, &sql)
        })
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking panicked: {e}"))?
    }

    async fn execute(&self, sql: &str) -> anyhow::Result<u64> {
        let conn = Arc::clone(&self.conn);
        let sql = sql.to_owned();
        tokio::task::spawn_blocking(move || {
            let guard = conn
                .lock()
                .map_err(|e| anyhow::anyhow!("duckdb mutex poisoned: {e}"))?;
            let n = guard.execute(&sql, [])?;
            u64::try_from(n).map_err(|e| anyhow::anyhow!("row count overflow: {e}"))
        })
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking panicked: {e}"))?
    }

    fn name(&self) -> &'static str {
        "duckdb"
    }
}

/// Execute `sql` synchronously, collecting DuckDB's native Arrow output.
fn query_sync(conn: &::duckdb::Connection, sql: &str) -> anyhow::Result<QueryResult> {
    let mut stmt = conn.prepare(sql)?;
    let batches: Vec<RecordBatch> = stmt.query_arrow([])?.collect();
    // Match the SQLite driver's convention: an empty result is `vec![]`.
    if batches.iter().all(|b| b.num_rows() == 0) {
        return Ok(vec![]);
    }
    Ok(batches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::Driver;

    #[tokio::test]
    async fn duckdb_create_insert_query() {
        let driver = DuckDbDriver::open(":memory:").unwrap();
        driver
            .execute("CREATE TABLE t (id INTEGER, name TEXT)")
            .await
            .unwrap();
        let n = driver
            .execute("INSERT INTO t VALUES (1, 'alice'), (2, 'bob')")
            .await
            .unwrap();
        assert_eq!(n, 2);
        let batches = driver.query("SELECT id, name FROM t ORDER BY id").await.unwrap();
        assert_eq!(batches.iter().map(RecordBatch::num_rows).sum::<usize>(), 2);
    }

    #[tokio::test]
    async fn duckdb_empty_result_is_vec_empty() {
        let driver = DuckDbDriver::open(":memory:").unwrap();
        driver.execute("CREATE TABLE t (id INTEGER)").await.unwrap();
        let batches = driver.query("SELECT * FROM t").await.unwrap();
        assert!(batches.is_empty());
    }

    #[test]
    fn duckdb_driver_name() {
        let driver = DuckDbDriver::open(":memory:").unwrap();
        assert_eq!(driver.name(), "duckdb");
    }
}
