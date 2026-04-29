//! Shared CSV record helpers.

use ::csv::StringRecord;

/// Return the position of a named CSV column in a header record.
pub fn column_position(headers: &StringRecord, column_name: &str) -> Option<usize> {
    headers.iter().position(|header| header == column_name)
}
