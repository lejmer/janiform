use janiform::calibration::{
    CalibrationMemoryCheckpointStore, CalibrationStage, CalibrationStagedExecutionInput,
    MarketDataNormalizedTickOutput,
};
use pyo3::exceptions::PyValueError;
use pyo3::{PyResult, pyclass, pymethods};
use std::sync::Arc;

#[pyclass(module = "janiform._native", name = "CalibrationMemoryCheckpointStore")]
pub(crate) struct PyCalibrationMemoryCheckpointStore {
    pub(crate) store: CalibrationMemoryCheckpointStore,
}

#[pyclass(
    module = "janiform._native",
    name = "PreparedCalibrationExecutionHandle"
)]
#[derive(Clone)]
pub(crate) struct PyPreparedCalibrationExecutionHandle {
    pub(crate) execution_input: CalibrationStagedExecutionInput,
}

#[pyclass(module = "janiform._native", name = "NormalizedMarketDataHandle")]
#[derive(Clone)]
pub(crate) struct PyNormalizedMarketDataHandle {
    pub(crate) normalized_output: Arc<MarketDataNormalizedTickOutput>,
}

#[pymethods]
impl PyCalibrationMemoryCheckpointStore {
    #[new]
    fn py_new() -> Self {
        Self {
            store: CalibrationMemoryCheckpointStore::new(),
        }
    }

    fn checkpoint_count_for_run(&self, run_id: &str) -> usize {
        self.store.checkpoint_count_for_run(run_id)
    }

    fn checkpoint_reference_for_stage(
        &self,
        run_id: &str,
        stage: &str,
    ) -> PyResult<Option<String>> {
        let stage = parse_calibration_stage(stage)?;
        Ok(self
            .store
            .checkpoint_for_stage(run_id, stage)
            .map(|checkpoint| checkpoint.checkpoint_reference.clone()))
    }

    fn last_checkpoint_reference_for_run(&self, run_id: &str) -> Option<String> {
        self.store
            .last_checkpoint_for_run(run_id)
            .map(|checkpoint| checkpoint.checkpoint_reference.clone())
    }
}

#[pymethods]
impl PyPreparedCalibrationExecutionHandle {
    #[getter]
    fn schema_version(&self) -> &str {
        &self.execution_input.schema_version
    }

    #[getter]
    fn run_id(&self) -> &str {
        &self.execution_input.request.run_id
    }

    fn serialize_execution_input_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.execution_input).map_err(|source| {
            PyValueError::new_err(format!(
                "failed to serialize prepared calibration execution input: {source}"
            ))
        })
    }
}

#[pymethods]
impl PyNormalizedMarketDataHandle {
    fn serialize_summary_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.normalized_output.tick_data_summary).map_err(|source| {
            PyValueError::new_err(format!(
                "failed to serialize normalized market-data summary: {source}"
            ))
        })
    }
}

fn parse_calibration_stage(raw_stage: &str) -> PyResult<CalibrationStage> {
    match raw_stage {
        "seasonality" => Ok(CalibrationStage::Seasonality),
        "hawkes" => Ok(CalibrationStage::Hawkes),
        "return_components" => Ok(CalibrationStage::ReturnComponents),
        "mean_reversion" => Ok(CalibrationStage::MeanReversion),
        _ => Err(PyValueError::new_err(
            "stage must be one of: seasonality, hawkes, return_components, mean_reversion",
        )),
    }
}
