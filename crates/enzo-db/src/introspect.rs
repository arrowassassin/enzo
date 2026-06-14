//! Schema introspection — power the DB schema browser tree.
//!
//! Reads `SQLite`'s `sqlite_master` and `PRAGMA` interfaces to enumerate
//! tables, views, columns, and indexes. Results are plain serializable structs
//! the daemon forwards over ATP for the schema sidebar.

use serde::{Deserialize, Serialize};

use crate::pool::AnyPool;
use crate::pretty::batches_to_json;

/// A table or view in the database.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableInfo {
    /// Object name.
    pub name: String,
    /// `"table"` or `"view"`.
    pub kind: String,
}

/// One column of a table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColumnDef {
    /// Column name.
    pub name: String,
    /// Declared SQL type (e.g. `INTEGER`, `TEXT`).
    pub sql_type: String,
    /// `true` if the column is `NOT NULL`.
    pub not_null: bool,
    /// `true` if the column is part of the primary key.
    pub primary_key: bool,
    /// Default value expression, if any.
    pub default: Option<String>,
}

/// One index on a table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexInfo {
    /// Index name.
    pub name: String,
    /// `true` if the index enforces uniqueness.
    pub unique: bool,
}

/// Quote a SQL identifier by doubling embedded double-quotes.
///
/// Prevents identifier injection when interpolating table/column names that
/// originate from prior introspection (never from raw user text directly).
#[must_use]
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// List all tables and views, ordered by name.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn list_tables(pool: &AnyPool) -> anyhow::Result<Vec<TableInfo>> {
    let batches = pool
        .query(
            "SELECT name, type FROM sqlite_master \
             WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' \
             ORDER BY name",
        )
        .await?;
    let json = batches_to_json(&batches)?;
    let rows = json["rows"].as_array().cloned().unwrap_or_default();
    Ok(rows
        .iter()
        .filter_map(|r| {
            let cols = r.as_array()?;
            Some(TableInfo {
                name: cols.first()?.as_str()?.to_owned(),
                kind: cols.get(1)?.as_str()?.to_owned(),
            })
        })
        .collect())
}

/// List the columns of `table` via `PRAGMA table_info`.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn columns(pool: &AnyPool, table: &str) -> anyhow::Result<Vec<ColumnDef>> {
    // PRAGMA table_info returns: cid, name, type, notnull, dflt_value, pk
    let sql = format!("PRAGMA table_info({})", quote_ident(table));
    let batches = pool.query(&sql).await?;
    let json = batches_to_json(&batches)?;
    let rows = json["rows"].as_array().cloned().unwrap_or_default();
    Ok(rows
        .iter()
        .filter_map(|r| {
            let cols = r.as_array()?;
            let name = cols.get(1)?.as_str()?.to_owned();
            let sql_type = cols.get(2)?.as_str().unwrap_or("").to_owned();
            let not_null = cols.get(3)?.as_str().is_some_and(|s| s == "1");
            let default = cols
                .get(4)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            let primary_key = cols
                .get(5)
                .and_then(|v| v.as_str())
                .is_some_and(|s| s != "0");
            Some(ColumnDef {
                name,
                sql_type,
                not_null,
                primary_key,
                default,
            })
        })
        .collect())
}

/// List the indexes on `table` via `PRAGMA index_list`.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn indexes(pool: &AnyPool, table: &str) -> anyhow::Result<Vec<IndexInfo>> {
    // PRAGMA index_list returns: seq, name, unique, origin, partial
    let sql = format!("PRAGMA index_list({})", quote_ident(table));
    let batches = pool.query(&sql).await?;
    let json = batches_to_json(&batches)?;
    let rows = json["rows"].as_array().cloned().unwrap_or_default();
    Ok(rows
        .iter()
        .filter_map(|r| {
            let cols = r.as_array()?;
            Some(IndexInfo {
                name: cols.get(1)?.as_str()?.to_owned(),
                unique: cols
                    .get(2)
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| s == "1"),
            })
        })
        .collect())
}

/// Return the primary-key column names of `table`, in declaration order.
///
/// Used by the table editor to build safe `WHERE` clauses for row updates.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn primary_keys(pool: &AnyPool, table: &str) -> anyhow::Result<Vec<String>> {
    Ok(columns(pool, table)
        .await?
        .into_iter()
        .filter(|c| c.primary_key)
        .map(|c| c.name)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn seeded() -> AnyPool {
        let pool = AnyPool::sqlite(":memory:").unwrap();
        pool.execute(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER)",
        )
        .await
        .unwrap();
        pool.execute("CREATE UNIQUE INDEX idx_name ON users(name)")
            .await
            .unwrap();
        pool.execute("CREATE VIEW adults AS SELECT * FROM users WHERE age >= 18")
            .await
            .unwrap();
        pool.execute("INSERT INTO users VALUES (1,'alice',30),(2,'bob',25)")
            .await
            .unwrap();
        pool
    }

    #[test]
    fn quote_ident_escapes_quotes() {
        assert_eq!(quote_ident("users"), "\"users\"");
        assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
    }

    #[tokio::test]
    async fn list_tables_returns_table_and_view() {
        let pool = seeded().await;
        let tables = list_tables(&pool).await.unwrap();
        let names: Vec<&str> = tables.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"users"));
        assert!(names.contains(&"adults"));
        let users = tables.iter().find(|t| t.name == "users").unwrap();
        assert_eq!(users.kind, "table");
        let adults = tables.iter().find(|t| t.name == "adults").unwrap();
        assert_eq!(adults.kind, "view");
    }

    #[tokio::test]
    async fn columns_reports_types_and_constraints() {
        let pool = seeded().await;
        let cols = columns(&pool, "users").await.unwrap();
        assert_eq!(cols.len(), 3);
        let id = &cols[0];
        assert_eq!(id.name, "id");
        assert!(id.primary_key);
        let name = &cols[1];
        assert_eq!(name.name, "name");
        assert!(name.not_null);
        assert!(!name.primary_key);
    }

    #[tokio::test]
    async fn primary_keys_extracted() {
        let pool = seeded().await;
        let pks = primary_keys(&pool, "users").await.unwrap();
        assert_eq!(pks, vec!["id".to_owned()]);
    }

    #[tokio::test]
    async fn indexes_lists_unique_index() {
        let pool = seeded().await;
        let idx = indexes(&pool, "users").await.unwrap();
        assert!(idx.iter().any(|i| i.name == "idx_name" && i.unique));
    }
}
