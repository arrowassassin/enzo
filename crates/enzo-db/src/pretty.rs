//! Pretty-print Arrow record batches as UTF-8 tables.

use arrow::array::RecordBatch;
use arrow::util::display::{ArrayFormatter, FormatOptions};
use arrow::util::pretty::pretty_format_batches;
use serde_json::Value;

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

/// Serialise record batches as a JSON object with `columns` and `rows` arrays.
///
/// ```json
/// { "columns": ["id","name"], "rows": [["1","alice"],["2","bob"]] }
/// ```
pub fn batches_to_json(batches: &[RecordBatch]) -> anyhow::Result<Value> {
    if batches.is_empty() {
        return Ok(serde_json::json!({ "columns": [], "rows": [] }));
    }
    let col_names: Vec<String> = batches[0]
        .schema()
        .fields()
        .iter()
        .map(|f| f.name().clone())
        .collect();

    let opts = FormatOptions::default();
    let mut all_rows: Vec<Vec<String>> = Vec::new();

    for batch in batches {
        let formatters: Vec<ArrayFormatter<'_>> = batch
            .columns()
            .iter()
            .map(|col| ArrayFormatter::try_new(col.as_ref(), &opts))
            .collect::<Result<_, _>>()
            .map_err(|e| anyhow::anyhow!("array formatter: {e}"))?;

        for row_idx in 0..batch.num_rows() {
            let vals: Vec<String> = formatters
                .iter()
                .map(|f| f.value(row_idx).to_string())
                .collect();
            all_rows.push(vals);
        }
    }

    Ok(serde_json::json!({ "columns": col_names, "rows": all_rows }))
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
