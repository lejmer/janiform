use std::path::PathBuf;

use janiform_workflows::simulation::{
    DEFAULT_SIMULATION_EXPORT_BATCH_SIZE, SimulationOutputColumns, export_coarse_simulation,
    export_seeded_tick_simulation,
};
use pyo3::prelude::{Bound, PyResult, Python};
use pyo3::types::{PyAny, PyModule, PyModuleMethods};
use pyo3::{pyfunction, wrap_pyfunction};

use crate::SimulationExecutionError;
use crate::python_value::py_any_to_json_value;
use crate::types::{PyCalendarRegistry, PySeededTickSimulationNativeResult};

use super::output::workflow_error_to_simulation_error;

pub(super) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(export_coarse_simulation_artifact, module)?)?;
    module.add_function(wrap_pyfunction!(
        export_seeded_tick_simulation_artifact,
        module
    )?)?;

    Ok(())
}

#[pyfunction]
#[pyo3(signature = (native_result, output_path, reference_price, timestamp_column, price_column, side_column = None, batch_size = DEFAULT_SIMULATION_EXPORT_BATCH_SIZE))]
#[expect(
    clippy::too_many_arguments,
    reason = "PyO3 boundary mirrors the public Python keyword API."
)]
fn export_seeded_tick_simulation_artifact(
    py: Python<'_>,
    native_result: &PySeededTickSimulationNativeResult,
    output_path: &str,
    reference_price: f64,
    timestamp_column: &str,
    price_column: &str,
    side_column: Option<&str>,
    batch_size: usize,
) -> PyResult<usize> {
    let columns = simulation_output_columns(timestamp_column, price_column, side_column);

    let output_path = PathBuf::from(output_path);

    let summary = py
        .detach(|| {
            export_seeded_tick_simulation(
                &native_result.output,
                output_path.as_path(),
                reference_price,
                &columns,
                batch_size,
            )
        })
        .map_err(workflow_error_to_simulation_error)?;

    Ok(summary.record_count)
}

#[pyfunction]
#[pyo3(signature = (path_payload, output_path, reference_price, timestamp_column, price_column, calendar_registry, exchange_code, batch_size = DEFAULT_SIMULATION_EXPORT_BATCH_SIZE))]
#[expect(
    clippy::too_many_arguments,
    reason = "PyO3 boundary mirrors the public Python keyword API."
)]
fn export_coarse_simulation_artifact(
    py: Python<'_>,
    path_payload: &Bound<'_, PyAny>,
    output_path: &str,
    reference_price: f64,
    timestamp_column: &str,
    price_column: &str,
    calendar_registry: &PyCalendarRegistry,
    exchange_code: &str,
    batch_size: usize,
) -> PyResult<usize> {
    let coarse_path: janiform::simulation::CoarseSimulationPath =
        serde_json::from_value(py_any_to_json_value(path_payload)?).map_err(|source| {
            SimulationExecutionError::new_err(format!(
                "failed to deserialize coarse simulation path: {source}"
            ))
        })?;

    let columns = simulation_output_columns(timestamp_column, price_column, None);

    let output_path = PathBuf::from(output_path);

    let registry = calendar_registry.registry.clone();

    let exchange_code = exchange_code.to_string();

    let summary = py
        .detach(move || {
            export_coarse_simulation(
                &coarse_path,
                output_path.as_path(),
                reference_price,
                &columns,
                &registry,
                exchange_code.as_str(),
                batch_size,
            )
        })
        .map_err(workflow_error_to_simulation_error)?;

    Ok(summary.record_count)
}

fn simulation_output_columns(
    timestamp_column: &str,
    price_column: &str,
    side_column: Option<&str>,
) -> SimulationOutputColumns {
    SimulationOutputColumns {
        timestamp_column: timestamp_column.to_string(),
        price_column: price_column.to_string(),
        side_column: side_column.map(str::to_string),
    }
}
