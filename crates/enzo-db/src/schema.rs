//! Schema introspection helpers.

use arrow::array::RecordBatch;
use arrow::datatypes::{DataType, SchemaRef};

/// A description of one column returned from introspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnInfo {
    /// Column name.
    pub name: String,
    /// Arrow data type.
    pub data_type: DataType,
    /// Whether the column allows null values.
    pub nullable: bool,
}

/// Extract column metadata from the schema of the first batch.
#[must_use]
pub fn describe(batches: &[RecordBatch]) -> Vec<ColumnInfo> {
    let Some(batch) = batches.first() else {
        return vec![];
    };
    describe_schema(batch.schema_ref())
}

/// Extract column metadata from an Arrow `Schema`.
#[must_use]
pub fn describe_schema(schema: &SchemaRef) -> Vec<ColumnInfo> {
    schema
        .fields()
        .iter()
        .map(|f| ColumnInfo {
            name: f.name().clone(),
            data_type: f.data_type().clone(),
            nullable: f.is_nullable(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::{Driver, MemDriver};

    #[tokio::test]
    async fn describe_mem_driver_columns() {
        let d = MemDriver;
        let batches = d.query("SELECT 1").await.unwrap();
        let cols = describe(&batches);
        assert_eq!(cols.len(), 3);
        assert_eq!(cols[0].name, "id");
        assert_eq!(cols[0].data_type, DataType::Int64);
        assert!(!cols[0].nullable);
        assert_eq!(cols[1].name, "name");
        assert_eq!(cols[2].data_type, DataType::Float64);
        assert!(cols[2].nullable);
    }

    #[test]
    fn describe_empty_batches() {
        let cols = describe(&[]);
        assert!(cols.is_empty());
    }
}
