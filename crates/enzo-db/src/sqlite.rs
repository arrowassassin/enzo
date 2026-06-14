//! `SQLite` driver — wraps `rusqlite` in a `spawn_blocking` async adapter.

use std::sync::{Arc, Mutex};

use arrow::array::{ArrayRef, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};

use crate::driver::{Driver, QueryResult};

// ── SqliteDriver ─────────────────────────────────────────────────────────────

/// Async `SQLite` driver backed by `rusqlite` (bundled `SQLite`).
///
/// Each driver holds a single connection protected by a `Mutex`; concurrent
/// queries are serialised.  For higher concurrency, wrap multiple drivers in a
/// pool or use separate files.
pub struct SqliteDriver {
    conn: Arc<Mutex<rusqlite::Connection>>,
}

impl SqliteDriver {
    /// Open (or create) a database file at `path`.
    ///
    /// Pass `":memory:"` for a transient in-process database.
    pub fn open(path: &str) -> anyhow::Result<Self> {
        let conn = rusqlite::Connection::open(path)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

impl Driver for SqliteDriver {
    async fn query(&self, sql: &str) -> anyhow::Result<QueryResult> {
        let conn = Arc::clone(&self.conn);
        let sql = sql.to_owned();
        tokio::task::spawn_blocking(move || {
            let guard = conn
                .lock()
                .map_err(|e| anyhow::anyhow!("sqlite mutex poisoned: {e}"))?;
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
                .map_err(|e| anyhow::anyhow!("sqlite mutex poisoned: {e}"))?;
            let n = guard.execute(&sql, [])?;
            u64::try_from(n).map_err(|e| anyhow::anyhow!("row count overflow: {e}"))
        })
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking panicked: {e}"))?
    }

    fn name(&self) -> &'static str {
        "sqlite"
    }
}

// ── Sync query helper ─────────────────────────────────────────────────────────

/// Execute `sql` on `conn` synchronously, returning Arrow record batches.
fn query_sync(conn: &rusqlite::Connection, sql: &str) -> anyhow::Result<QueryResult> {
    let mut stmt = conn.prepare(sql)?;
    let ncols = stmt.column_count();

    let col_names: Vec<String> = (0..ncols)
        .map(|i| {
            stmt.column_name(i)
                .map_or_else(|_| format!("col{i}"), str::to_owned)
        })
        .collect();

    let fields: Vec<Field> = col_names
        .iter()
        .map(|name| Field::new(name, DataType::Utf8, true))
        .collect();
    let schema = Arc::new(Schema::new(fields));

    let rows: Vec<Vec<Option<String>>> = stmt
        .query_map([], |row| {
            let vals: Vec<Option<String>> = (0..ncols).map(|i| value_to_string(row, i)).collect();
            Ok(vals)
        })?
        .filter_map(std::result::Result::ok)
        .collect();

    if rows.is_empty() {
        return Ok(vec![]);
    }

    let arrays: Vec<ArrayRef> = (0..ncols)
        .map(|ci| {
            let col: Vec<Option<String>> = rows.iter().map(|row| row[ci].clone()).collect();
            Arc::new(StringArray::from(col)) as ArrayRef
        })
        .collect();

    let batch = RecordBatch::try_new(schema, arrays)?;
    Ok(vec![batch])
}

/// Convert column `i` of `row` to an `Option<String>`, stringifying any `SQLite`
/// type (integer, real, text, blob) so heterogeneous columns — including the
/// integer columns returned by `PRAGMA` introspection — survive the all-`Utf8`
/// result model. `NULL` maps to `None`.
fn value_to_string(row: &rusqlite::Row<'_>, i: usize) -> Option<String> {
    use rusqlite::types::ValueRef;
    match row.get_ref(i).ok()? {
        ValueRef::Null => None,
        ValueRef::Integer(n) => Some(n.to_string()),
        ValueRef::Real(f) => Some(f.to_string()),
        ValueRef::Text(t) => Some(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Some(format!("<{} bytes>", b.len())),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::Driver;

    #[tokio::test]
    async fn sqlite_create_and_insert() {
        let driver = SqliteDriver::open(":memory:").unwrap();
        driver
            .execute("CREATE TABLE t (id INTEGER, name TEXT)")
            .await
            .unwrap();
        let n = driver
            .execute("INSERT INTO t VALUES (1, 'alice')")
            .await
            .unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn sqlite_query_returns_batch() {
        let driver = SqliteDriver::open(":memory:").unwrap();
        driver
            .execute("CREATE TABLE t (id INTEGER, name TEXT)")
            .await
            .unwrap();
        driver
            .execute("INSERT INTO t VALUES (1, 'alice'), (2, 'bob')")
            .await
            .unwrap();
        let batches = driver.query("SELECT id, name FROM t").await.unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].num_rows(), 2);
        assert_eq!(batches[0].num_columns(), 2);
    }

    #[tokio::test]
    async fn sqlite_query_empty_result() {
        let driver = SqliteDriver::open(":memory:").unwrap();
        driver.execute("CREATE TABLE t (id INTEGER)").await.unwrap();
        let batches = driver.query("SELECT * FROM t").await.unwrap();
        assert!(batches.is_empty());
    }

    #[test]
    fn sqlite_driver_name() {
        let driver = SqliteDriver::open(":memory:").unwrap();
        assert_eq!(driver.name(), "sqlite");
    }
}
