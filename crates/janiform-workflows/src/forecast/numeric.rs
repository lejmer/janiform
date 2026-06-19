use crate::error::WorkflowError;

use super::request::validation_error;

pub(super) fn usize_to_f64(value: usize, field: &'static str) -> Result<f64, WorkflowError> {
    janiform_foundations::numeric::usize_to_f64_exact(value, field)
        .map_err(|source| validation_error(field, source.to_string()))
}
