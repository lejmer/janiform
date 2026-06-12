mod calendar;
mod calibration;
mod hourly;
mod seasonality;
mod simulation;

use pyo3::prelude::{Bound, PyResult};
use pyo3::types::{PyModule, PyModuleMethods};

pub(crate) use calendar::PyCalendarRegistry;
pub(crate) use calibration::{
    PyCalibrationMemoryCheckpointStore, PyNormalizedMarketDataHandle,
    PyPreparedCalibrationExecutionHandle,
};
pub(crate) use hourly::{
    PyHourlyFactorContractResult, PyHourlyNewsIntensityMaterializationResult,
    PyHourlySeasonalityMaterializationResult,
};
pub(crate) use seasonality::PyTimestampEventPreprocessingResult;
pub(crate) use simulation::{
    PySeededFinePathChunkColumns, PySeededTickEventTapeChunkColumns,
    PySeededTickSimulationNativeResult,
};

pub(crate) fn register_classes(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyCalibrationMemoryCheckpointStore>()?;
    module.add_class::<PyNormalizedMarketDataHandle>()?;
    module.add_class::<PyPreparedCalibrationExecutionHandle>()?;
    module.add_class::<PyCalendarRegistry>()?;
    module.add_class::<PyTimestampEventPreprocessingResult>()?;
    module.add_class::<PyHourlyFactorContractResult>()?;
    module.add_class::<PyHourlySeasonalityMaterializationResult>()?;
    module.add_class::<PyHourlyNewsIntensityMaterializationResult>()?;
    module.add_class::<PySeededFinePathChunkColumns>()?;
    module.add_class::<PySeededTickEventTapeChunkColumns>()?;
    module.add_class::<PySeededTickSimulationNativeResult>()?;

    Ok(())
}
