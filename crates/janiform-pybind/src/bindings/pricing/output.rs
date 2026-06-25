use janiform_foundations::paths::format_path;
use janiform_workflows::error::WorkflowError;
use pyo3::prelude::{Py, PyErr, PyResult, Python};
use pyo3::types::PyAny;
use serde::Serialize;

use crate::PricingExecutionError;
use crate::python_value::json_value_to_py_object;

pub(super) fn pricing_result_to_py_object<TValue>(
    py: Python<'_>,
    value: &TValue,
    description: &str,
) -> PyResult<Py<PyAny>>
where
    TValue: Serialize,
{
    let serialized_value = serde_json::to_value(value).map_err(|source| {
        PricingExecutionError::new_err(format!("failed to serialize {description}: {source}"))
    })?;

    json_value_to_py_object(py, &serialized_value)
}

pub(super) fn workflow_error_to_pricing_error(error: WorkflowError) -> PyErr {
    match error {
        WorkflowError::PricingValidationFailed { field, message } => {
            PricingExecutionError::new_err(format!("{field}: {message}"))
        }
        WorkflowError::PricingArtifactWrite { path, source } => {
            PricingExecutionError::new_err(format!(
                "failed to write pricing artifact `{}`: {source}",
                format_path(path.as_path())
            ))
        }
        WorkflowError::ModelValidationFailed { field, message } => {
            PricingExecutionError::new_err(format!("{field}: {message}"))
        }
        _ => PricingExecutionError::new_err(error.to_string()),
    }
}
