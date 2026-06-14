//! Pretty-print Arrow record batches as UTF-8 tables.

use arrow::array::RecordBatch;
use arrow::util::pretty::pretty_format_batches;

/// Format a sequence of record batches into a human-readable table string.
///
/// Returns an empty string if `batches` is empty.
pub fn format_result(batches: &[RecordBatch]) -> anyhow::Result<String> {
    if batches.is_empty() {
        return Ok(String::new());
    }
    let display = pretty_format_batches(batches)?;
    Ok(display.to_string())
}

/// Return the total row count across all batches.
pub fn row_count(batches: &[RecordBatch]) -> usize {
    batches.iter().map(RecordBatch::num_rows).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::{Driver, MemDriver};

    #[tokio::test]
    async fn format_result_non_empty() {
        let d = MemDriver;
        let batches = d.query("SELECT 1").await.unwrap();
        let s = format_result(&batches).unwrap();
        assert!(s.contains("alpha"));
        assert!(s.contains("id"));
    }

    #[test]
    fn format_result_empty() {
        let s = format_result(&[]).unwrap();
        assert!(s.is_empty());
    }

    #[tokio::test]
    async fn row_count_correct() {
        let d = MemDriver;
        let batches = d.query("SELECT 1").await.unwrap();
        assert_eq!(row_count(&batches), 3);
    }

    #[test]
    fn row_count_empty() {
        assert_eq!(row_count(&[]), 0);
    }
}
