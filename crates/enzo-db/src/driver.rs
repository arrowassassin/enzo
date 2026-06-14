//! Driver trait and in-memory reference implementation.

use std::sync::Arc;

use arrow::array::{ArrayRef, Float64Array, Int64Array, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};

/// The result of a query — a sequence of Arrow record batches.
pub type QueryResult = Vec<RecordBatch>;

/// An async database driver.
///
/// Implementations wrap a specific database (`SQLite`, `PostgreSQL`, `DuckDB`, …)
/// and translate queries into Arrow record batches.
#[allow(
    async_fn_in_trait,
    reason = "callers are always Send; dyn Driver is not needed"
)]
pub trait Driver: Send + Sync {
    /// Execute a SQL statement and return Arrow record batches.
    async fn query(&self, sql: &str) -> anyhow::Result<QueryResult>;

    /// Execute a SQL statement that produces no result set (INSERT / DDL).
    async fn execute(&self, sql: &str) -> anyhow::Result<u64>;

    /// Return the driver name (used for logging / diagnostics).
    fn name(&self) -> &'static str;
}

// ── In-memory mock driver ────────────────────────────────────────────────────

/// A no-op driver useful for testing.  Returns one hard-coded batch for
/// `SELECT` statements and 0 for everything else.
pub struct MemDriver;

impl Driver for MemDriver {
    async fn query(&self, sql: &str) -> anyhow::Result<QueryResult> {
        if !sql
            .trim_ascii_start()
            .to_ascii_uppercase()
            .starts_with("SELECT")
        {
            return Ok(vec![]);
        }
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("name", DataType::Utf8, false),
            Field::new("value", DataType::Float64, true),
        ]));
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![
                Arc::new(Int64Array::from(vec![1, 2, 3])) as ArrayRef,
                Arc::new(StringArray::from(vec!["alpha", "beta", "gamma"])) as ArrayRef,
                Arc::new(Float64Array::from(vec![1.1, 2.2, 3.3])) as ArrayRef,
            ],
        )?;
        Ok(vec![batch])
    }

    async fn execute(&self, _sql: &str) -> anyhow::Result<u64> {
        Ok(0)
    }

    fn name(&self) -> &'static str {
        "mem"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mem_driver_select_returns_batch() {
        let d = MemDriver;
        let batches = d.query("SELECT 1").await.unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].num_rows(), 3);
        assert_eq!(batches[0].num_columns(), 3);
    }

    #[tokio::test]
    async fn mem_driver_insert_returns_empty() {
        let d = MemDriver;
        let batches = d.query("INSERT INTO t VALUES (1)").await.unwrap();
        assert!(batches.is_empty());
    }

    #[tokio::test]
    async fn mem_driver_execute_returns_zero() {
        let d = MemDriver;
        let rows = d.execute("CREATE TABLE t (id INT)").await.unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn mem_driver_name() {
        assert_eq!(MemDriver.name(), "mem");
    }
}
