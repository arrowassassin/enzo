//! Database query engine for Enzo.
//!
//! Provides an async connection abstraction (`Connection`) that returns
//! Apache Arrow `RecordBatch` results, a lightweight connection pool, and
//! helpers for pretty-printing query results.
//!
//! Concrete drivers (`SQLite`, `PostgreSQL`, `DuckDB`, …) implement the
//! [`Driver`] trait and are registered at startup.

pub mod driver;
pub mod duckdb;
pub mod introspect;
pub mod paginate;
pub mod pool;
pub mod pretty;
pub mod schema;
pub mod sqlite;
pub mod table;
pub mod tabs;

pub use introspect::{ColumnDef, IndexInfo, TableInfo};
pub use paginate::{Page, page_count};
pub use pool::AnyPool;
pub use pretty::batches_to_json;
pub use table::Cell;
pub use tabs::{QueryTab, TabManager};
