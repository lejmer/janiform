use janiform::simulation::SeededTickSimulationRunResult;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

#[pyclass(module = "janiform._native", name = "SeededTickSimulationNativeResult")]
pub(crate) struct PySeededTickSimulationNativeResult {
    pub(crate) output: SeededTickSimulationRunResult,
}

#[pymethods]
impl PySeededTickSimulationNativeResult {
    #[getter]
    fn schema_version(&self) -> &str {
        self.output.schema_version
    }

    #[getter]
    fn transition_count(&self) -> usize {
        self.output.transition_count()
    }

    #[getter]
    fn event_count(&self) -> usize {
        self.output.event_count()
    }

    #[getter]
    fn estimated_native_retained_bytes(&self) -> usize {
        self.output
            .native_diagnostics()
            .estimated_total_retained_bytes
    }

    fn fine_path_metadata_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.output.fine_path_metadata).map_err(|source| {
            PyRuntimeError::new_err(format!(
                "failed to serialize seeded fine-path metadata payload: {source}"
            ))
        })
    }

    fn event_tape_metadata_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.output.event_tape_metadata).map_err(|source| {
            PyRuntimeError::new_err(format!(
                "failed to serialize seeded event-tape metadata payload: {source}"
            ))
        })
    }

    fn get_fine_path_chunk_json(&self, start_index: usize, max_records: usize) -> PyResult<String> {
        serde_json::to_string(self.output.fine_path_chunk(start_index, max_records)).map_err(
            |source| {
                PyRuntimeError::new_err(format!(
                    "failed to serialize seeded fine-path chunk payload: {source}"
                ))
            },
        )
    }

    fn get_event_tape_chunk_json(
        &self,
        start_index: usize,
        max_records: usize,
    ) -> PyResult<String> {
        serde_json::to_string(&self.output.event_chunk(start_index, max_records)).map_err(
            |source| {
                PyRuntimeError::new_err(format!(
                    "failed to serialize seeded event-tape chunk payload: {source}"
                ))
            },
        )
    }
}
