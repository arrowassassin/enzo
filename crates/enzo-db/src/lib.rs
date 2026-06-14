//! Database query engine for Enzo.
//!
//! Provides an async connection abstraction (`Connection`) that returns
//! Apache Arrow `RecordBatch` results, a lightweight connection pool, and
//! helpers for pretty-printing query results.
//!
//! Concrete drivers (`SQLite`, `PostgreSQL`, `DuckDB`, …) implement the
//! [`Driver`] trait and are registered at startup.

pub mod driver;
pub mod pool;
pub mod pretty;
pub mod schema;
