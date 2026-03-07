use janiform_core::calibration::{CalibrationMemoryCheckpointStore, CalibrationStage};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

#[pyclass(module = "janiform._native", name = "CalibrationMemoryCheckpointStore")]
pub(crate) struct PyCalibrationMemoryCheckpointStore {
    pub(crate) store: CalibrationMemoryCheckpointStore,
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

fn parse_calibration_stage(raw_stage: &str) -> PyResult<CalibrationStage> {
    match raw_stage {
        "seasonality" => Ok(CalibrationStage::Seasonality),
        "hawkes" => Ok(CalibrationStage::Hawkes),
        "return_components" => Ok(CalibrationStage::ReturnComponents),
        _ => Err(PyValueError::new_err(
            "stage must be one of: seasonality, hawkes, return_components",
        )),
    }
}
