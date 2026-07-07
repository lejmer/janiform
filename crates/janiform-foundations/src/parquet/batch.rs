//! Shared Parquet record-batch helpers.

use arrow_array::{Array, RecordBatch};

/// Return the position of a named column in a projected Parquet record batch.
pub fn column_index(batch: &RecordBatch, column_name: &str) -> Option<usize> {
    batch.schema_ref().index_of(column_name).ok()
}

/// Return a named column from a projected Parquet record batch.
pub fn column<'a>(batch: &'a RecordBatch, column_name: &str) -> Option<&'a dyn Array> {
    let column_index = column_index(batch, column_name)?;

    Some(batch.column(column_index).as_ref())
}
