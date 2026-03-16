mod calendar;
mod calibration;
mod hourly;
mod seasonality;
mod simulation;

use pyo3::prelude::*;
use pyo3::types::PyModule;

pub(crate) use calendar::PyCalendarRegistry;
pub(crate) use calibration::PyCalibrationMemoryCheckpointStore;
pub(crate) use hourly::{
    PyHourlyFactorContractResult, PyHourlyNewsIntensityMaterializationResult,
    PyHourlySeasonalityMaterializationResult,
};
pub(crate) use seasonality::{
    PyDayOfWeekSeasonalityEstimateResult, PyDayOfYearSeasonalityEstimateResult,
    PyDeterministicSeasonalityCompositionResult, PyIntradaySeasonalityEstimateResult,
    PyTimestampEventPreprocessingResult,
};
pub(crate) use simulation::PySeededTickSimulationNativeResult;

pub(crate) fn register_classes(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyCalibrationMemoryCheckpointStore>()?;
    module.add_class::<PyCalendarRegistry>()?;
    module.add_class::<PyTimestampEventPreprocessingResult>()?;
    module.add_class::<PyIntradaySeasonalityEstimateResult>()?;
    module.add_class::<PyDayOfWeekSeasonalityEstimateResult>()?;
    module.add_class::<PyDayOfYearSeasonalityEstimateResult>()?;
    module.add_class::<PyDeterministicSeasonalityCompositionResult>()?;
    module.add_class::<PyHourlyFactorContractResult>()?;
    module.add_class::<PyHourlySeasonalityMaterializationResult>()?;
    module.add_class::<PyHourlyNewsIntensityMaterializationResult>()?;
    module.add_class::<PySeededTickSimulationNativeResult>()?;
    Ok(())
}
