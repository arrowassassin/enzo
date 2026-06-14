//! Table viewer + row editor — view and edit values directly in the data grid.
//!
//! Builds parameter-free but injection-safe SQL for the common grid operations:
//! browse a table page, update a single cell/row, delete a row, and insert a
//! row. Identifiers are quoted via [`quote_ident`]; literal values are escaped
//! through [`sql_literal`]. Primary keys (from [`crate::introspect`]) anchor the
//! `WHERE` clause so edits target exactly one row.

use crate::introspect::quote_ident;
use crate::paginate::{Page, count_query, paged_query};

/// Escape a string value into a single-quoted SQL literal.
///
/// `NULL` (Rust `None`) becomes the keyword `NULL`. Embedded single-quotes are
/// doubled, the standard SQL escaping that `SQLite`, `PostgreSQL`, and `DuckDB` share.
#[must_use]
pub fn sql_literal(value: Option<&str>) -> String {
    match value {
        None => "NULL".to_owned(),
        Some(v) => format!("'{}'", v.replace('\'', "''")),
    }
}

/// Build SQL to browse one page of a table, newest natural order.
#[must_use]
pub fn browse_page_sql(table: &str, page: Page) -> String {
    let base = format!("SELECT * FROM {}", quote_ident(table));
    paged_query(&base, page)
}

/// Build SQL to count the rows in a table (for scrollbar extent).
#[must_use]
pub fn count_table_sql(table: &str) -> String {
    count_query(&format!("SELECT * FROM {}", quote_ident(table)))
}

/// A column/value assignment for an `UPDATE` or `INSERT`.
#[derive(Debug, Clone)]
pub struct Cell {
    /// Column name.
    pub column: String,
    /// New value (`None` = SQL `NULL`).
    pub value: Option<String>,
}

/// Build an `UPDATE` statement setting `cells`, keyed by `pk` columns.
///
/// Returns an error if `pk` is empty (refuse to update without a unique anchor,
/// which would risk modifying every row).
///
/// # Errors
/// Returns an error if no primary-key columns are supplied or `cells` is empty.
pub fn update_row_sql(table: &str, cells: &[Cell], pk: &[Cell]) -> anyhow::Result<String> {
    if pk.is_empty() {
        anyhow::bail!("refusing to UPDATE without a primary-key WHERE clause");
    }
    if cells.is_empty() {
        anyhow::bail!("no cells to update");
    }
    let set = cells
        .iter()
        .map(|c| {
            format!(
                "{} = {}",
                quote_ident(&c.column),
                sql_literal(c.value.as_deref())
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let where_clause = build_where(pk);
    Ok(format!(
        "UPDATE {} SET {set} WHERE {where_clause}",
        quote_ident(table)
    ))
}

/// Build a `DELETE` statement keyed by `pk` columns.
///
/// # Errors
/// Returns an error if no primary-key columns are supplied.
pub fn delete_row_sql(table: &str, pk: &[Cell]) -> anyhow::Result<String> {
    if pk.is_empty() {
        anyhow::bail!("refusing to DELETE without a primary-key WHERE clause");
    }
    Ok(format!(
        "DELETE FROM {} WHERE {}",
        quote_ident(table),
        build_where(pk)
    ))
}

/// Build an `INSERT` statement from `cells`.
///
/// # Errors
/// Returns an error if `cells` is empty.
pub fn insert_row_sql(table: &str, cells: &[Cell]) -> anyhow::Result<String> {
    if cells.is_empty() {
        anyhow::bail!("no cells to insert");
    }
    let cols = cells
        .iter()
        .map(|c| quote_ident(&c.column))
        .collect::<Vec<_>>()
        .join(", ");
    let vals = cells
        .iter()
        .map(|c| sql_literal(c.value.as_deref()))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(
        "INSERT INTO {} ({cols}) VALUES ({vals})",
        quote_ident(table)
    ))
}

/// Build a `col = value AND …` clause from key cells.
fn build_where(pk: &[Cell]) -> String {
    pk.iter()
        .map(|c| match &c.value {
            None => format!("{} IS NULL", quote_ident(&c.column)),
            Some(v) => format!("{} = {}", quote_ident(&c.column), sql_literal(Some(v))),
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::AnyPool;

    fn cell(col: &str, val: Option<&str>) -> Cell {
        Cell {
            column: col.to_owned(),
            value: val.map(str::to_owned),
        }
    }

    #[test]
    fn sql_literal_escapes_quotes() {
        assert_eq!(sql_literal(Some("o'brien")), "'o''brien'");
        assert_eq!(sql_literal(None), "NULL");
    }

    #[test]
    fn update_row_builds_set_and_where() {
        let sql = update_row_sql(
            "users",
            &[cell("name", Some("alice")), cell("age", None)],
            &[cell("id", Some("1"))],
        )
        .unwrap();
        assert!(sql.contains("UPDATE \"users\" SET"));
        assert!(sql.contains("\"name\" = 'alice'"));
        assert!(sql.contains("\"age\" = NULL"));
        assert!(sql.contains("WHERE \"id\" = '1'"));
    }

    #[test]
    fn update_without_pk_refused() {
        let err = update_row_sql("users", &[cell("name", Some("x"))], &[]);
        assert!(err.is_err());
    }

    #[test]
    fn update_without_cells_refused() {
        let err = update_row_sql("users", &[], &[cell("id", Some("1"))]);
        assert!(err.is_err());
    }

    #[test]
    fn delete_row_builds_where() {
        let sql = delete_row_sql("users", &[cell("id", Some("5"))]).unwrap();
        assert_eq!(sql, "DELETE FROM \"users\" WHERE \"id\" = '5'");
    }

    #[test]
    fn delete_without_pk_refused() {
        assert!(delete_row_sql("users", &[]).is_err());
    }

    #[test]
    fn insert_row_builds_columns_and_values() {
        let sql = insert_row_sql(
            "users",
            &[cell("id", Some("3")), cell("name", Some("cara"))],
        )
        .unwrap();
        assert!(sql.starts_with("INSERT INTO \"users\" (\"id\", \"name\") VALUES ('3', 'cara')"));
    }

    #[test]
    fn build_where_handles_null_key() {
        let w = build_where(&[cell("a", None), cell("b", Some("2"))]);
        assert_eq!(w, "\"a\" IS NULL AND \"b\" = '2'");
    }

    #[test]
    fn browse_page_sql_paginates() {
        let sql = browse_page_sql("users", Page::new(1, 50));
        assert!(sql.contains("SELECT * FROM \"users\""));
        assert!(sql.contains("LIMIT 50"));
        assert!(sql.contains("OFFSET 50"));
    }

    /// End-to-end: build + run an edit cycle against a real `SQLite` DB.
    #[tokio::test]
    async fn edit_cycle_against_sqlite() {
        let pool = AnyPool::sqlite(":memory:").unwrap();
        pool.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)")
            .await
            .unwrap();

        // INSERT
        let ins = insert_row_sql("t", &[cell("id", Some("1")), cell("name", Some("a"))]).unwrap();
        pool.execute(&ins).await.unwrap();

        // UPDATE
        let upd =
            update_row_sql("t", &[cell("name", Some("b"))], &[cell("id", Some("1"))]).unwrap();
        pool.execute(&upd).await.unwrap();

        // Verify via browse
        let page = browse_page_sql("t", Page::new(0, 10));
        let batches = pool.query(&page).await.unwrap();
        let json = crate::pretty::batches_to_json(&batches).unwrap();
        assert_eq!(json["rows"][0][1], "b");

        // DELETE
        let del = delete_row_sql("t", &[cell("id", Some("1"))]).unwrap();
        pool.execute(&del).await.unwrap();
        let count = count_table_sql("t");
        let batches = pool.query(&count).await.unwrap();
        let json = crate::pretty::batches_to_json(&batches).unwrap();
        assert_eq!(json["rows"][0][0], "0");
    }
}
