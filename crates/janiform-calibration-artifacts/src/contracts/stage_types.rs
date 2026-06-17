use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationStage {
    Seasonality,
    Hawkes,
    ObservationModels,
    ReturnComponents,
    LongVariance,
    NewsShocks,
    ReturnBackfit,
    MeanReversion,
    JointRefinement,
    OptionRefinement,
    Finalization,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationExecutionMode {
    Memory,
    Disk,
}

/// Originating caller surface used for execution-mode defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationCallerSurface {
    Cli,
    PythonImportableApi,
}

/// Resolve the default execution mode for one caller surface.
pub const fn default_calibration_execution_mode_for_surface(
    caller_surface: CalibrationCallerSurface,
) -> CalibrationExecutionMode {
    match caller_surface {
        CalibrationCallerSurface::Cli => CalibrationExecutionMode::Disk,
        CalibrationCallerSurface::PythonImportableApi => CalibrationExecutionMode::Memory,
    }
}

/// Resolve execution mode using caller-surface defaults and optional override.
pub const fn resolve_calibration_execution_mode_for_surface(
    caller_surface: CalibrationCallerSurface,
    execution_mode_override: Option<CalibrationExecutionMode>,
) -> CalibrationExecutionMode {
    match execution_mode_override {
        Some(execution_mode) => execution_mode,
        None => default_calibration_execution_mode_for_surface(caller_surface),
    }
}
