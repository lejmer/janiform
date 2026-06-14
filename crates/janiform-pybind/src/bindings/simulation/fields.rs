use pyo3::prelude::{Bound, PyRef, PyResult};
use pyo3::types::{PyAny, PyAnyMethods, PyMapping, PyMappingMethods};
use serde::de::DeserializeOwned;

use crate::SeasonalityInputValidationError;
use crate::SimulationExecutionError;
use crate::python_value::py_any_to_json_value;
use crate::types::PyIntervalFactorGridContractResult;

pub(super) fn parse_required_field<'py>(
    parent_mapping: &Bound<'py, PyMapping>,
    parent_context: &str,
    field_name: &str,
) -> PyResult<Bound<'py, PyAny>> {
    parent_mapping
        .contains(field_name)?
        .then(|| parent_mapping.get_item(field_name))
        .ok_or_else(|| {
            SeasonalityInputValidationError::new_err(format!(
                "{parent_context} is missing required field '{field_name}'"
            ))
        })?
}

pub(super) fn parse_required_json_field<TValue>(
    parent_mapping: &Bound<'_, PyMapping>,
    parent_context: &str,
    field_name: &str,
) -> PyResult<TValue>
where
    TValue: DeserializeOwned,
{
    let field_value = parse_required_field(parent_mapping, parent_context, field_name)?;

    serde_json::from_value(py_any_to_json_value(&field_value)?).map_err(|source| {
        SimulationExecutionError::new_err(format!(
            "{parent_context}.{field_name} failed to deserialize: {source}"
        ))
    })
}

pub(super) fn parse_required_string_field(
    parent_mapping: &Bound<'_, PyMapping>,
    parent_context: &str,
    field_name: &str,
) -> PyResult<String> {
    let field_value = parse_required_field(parent_mapping, parent_context, field_name)?;

    field_value.extract::<String>().map_err(|_| {
        SimulationExecutionError::new_err(format!("{parent_context}.{field_name} must be a string"))
    })
}

pub(super) fn parse_required_interval_factor_grid<'py>(
    parent_mapping: &Bound<'py, PyMapping>,
    parent_context: &str,
) -> PyResult<PyRef<'py, PyIntervalFactorGridContractResult>> {
    let field_value = parse_required_field(parent_mapping, parent_context, "interval_factor_grid")?;

    field_value
        .extract::<PyRef<'py, PyIntervalFactorGridContractResult>>()
        .map_err(|_| {
            SimulationExecutionError::new_err(format!(
                "{parent_context}.interval_factor_grid must be a native interval factor grid contract"
            ))
        })
}
