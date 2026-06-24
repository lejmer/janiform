use std::error::Error;
use std::fmt::{Display, Formatter};

use super::loading::MarketDataLoadingProgressEvent;
use super::timestamp_batching_surface_diagnostics::CalibrationTimestampBatchingSurfaceDiagnostic;

/// Market-data preparation phase reported after source loading.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarketDataPreparationProgressPhase {
    /// Calibration input contracts are being resolved from loaded market data.
    PreparingCalibrationInputs,
    /// Calibration run metadata and solver settings are being assembled.
    BuildingRunSpec,
    /// Staged execution input is being materialized from prepared data.
    BuildingExecutionInput,
    /// Market-data preparation diagnostics are being summarized.
    BuildingDiagnostics,
    /// Market-data provenance is being assembled.
    BuildingProvenance,
}

impl MarketDataPreparationProgressPhase {
    /// Stable phase identifier used by progress events.
    pub const fn phase_id(self) -> &'static str {
        match self {
            Self::PreparingCalibrationInputs => "preparing_calibration_inputs",
            Self::BuildingRunSpec => "building_run_spec",
            Self::BuildingExecutionInput => "building_execution_input",
            Self::BuildingDiagnostics => "building_diagnostics",
            Self::BuildingProvenance => "building_provenance",
        }
    }

    /// Human-readable phase label used by progress displays.
    pub const fn phase_label(self) -> &'static str {
        match self {
            Self::PreparingCalibrationInputs => "Preparing calibration inputs",
            Self::BuildingRunSpec => "Building calibration run spec",
            Self::BuildingExecutionInput => "Materializing staged execution input",
            Self::BuildingDiagnostics => "Summarizing market data diagnostics",
            Self::BuildingProvenance => "Building market data provenance",
        }
    }

    /// Human-readable latest signal for terminal progress displays.
    pub const fn latest_signal(self) -> &'static str {
        match self {
            Self::PreparingCalibrationInputs => "preparing calibration input contracts",
            Self::BuildingRunSpec => "building calibration run specification",
            Self::BuildingExecutionInput => "materializing staged execution input",
            Self::BuildingDiagnostics => "summarizing prepared market data",
            Self::BuildingProvenance => "building provenance payload",
        }
    }
}

/// Progress emitted while market-data preparation runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MarketDataPreparationProgressEvent {
    /// Source loading progress.
    Loading(MarketDataLoadingProgressEvent),
    /// Post-loading preparation phase progress.
    Preparing(MarketDataPreparationProgressPhase),
    /// Timestamp batching surface diagnostic surfaced during preparation.
    TimestampBatchingSurface(CalibrationTimestampBatchingSurfaceDiagnostic),
}

impl MarketDataPreparationProgressEvent {
    /// Human-readable latest signal for terminal progress displays.
    pub fn latest_signal(&self) -> String {
        match self {
            Self::Loading(event) => event.latest_signal(),
            Self::Preparing(phase) => phase.latest_signal().to_string(),
            Self::TimestampBatchingSurface(_) => {
                "recording timestamp batching surface diagnostics".to_string()
            }
        }
    }
}

/// Error returned when a preparation progress observer fails.
#[derive(Debug)]
pub struct MarketDataPreparationProgressError {
    message: String,
}

impl MarketDataPreparationProgressError {
    /// Build an observer error from a boundary-specific failure message.
    pub fn observer(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for MarketDataPreparationProgressError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "progress observer failed: {}", self.message)
    }
}

impl Error for MarketDataPreparationProgressError {}

/// Callback used by preparation APIs that expose market-data preparation progress.
pub type MarketDataPreparationProgressObserver<'a> =
    &'a mut dyn FnMut(
        MarketDataPreparationProgressEvent,
    ) -> Result<(), MarketDataPreparationProgressError>;

pub(crate) struct MarketDataPreparationProgressContext<'a> {
    observer: Option<MarketDataPreparationProgressObserver<'a>>,
}

impl<'a> MarketDataPreparationProgressContext<'a> {
    pub(crate) const fn disabled() -> Self {
        Self { observer: None }
    }

    pub(crate) fn observed(observer: MarketDataPreparationProgressObserver<'a>) -> Self {
        Self {
            observer: Some(observer),
        }
    }

    pub(crate) fn emit_loading(
        &mut self,
        event: MarketDataLoadingProgressEvent,
    ) -> Result<(), MarketDataPreparationProgressError> {
        self.emit(MarketDataPreparationProgressEvent::Loading(event))
    }

    pub(crate) fn emit_phase(
        &mut self,
        phase: MarketDataPreparationProgressPhase,
    ) -> Result<(), MarketDataPreparationProgressError> {
        self.emit(MarketDataPreparationProgressEvent::Preparing(phase))
    }

    pub(crate) fn emit_timestamp_batching_surface(
        &mut self,
        diagnostic: CalibrationTimestampBatchingSurfaceDiagnostic,
    ) -> Result<(), MarketDataPreparationProgressError> {
        self.emit(MarketDataPreparationProgressEvent::TimestampBatchingSurface(diagnostic))
    }

    fn emit(
        &mut self,
        event: MarketDataPreparationProgressEvent,
    ) -> Result<(), MarketDataPreparationProgressError> {
        let Some(observer) = self.observer.as_mut() else {
            return Ok(());
        };

        observer(event)
    }
}

#[cfg(test)]
mod tests {
    use janiform_model_kernels::timestamp_batching_surface::TimestampBatchingSurface;

    use super::super::timestamp_batching_surface_diagnostics::{
        CALIBRATION_TIMESTAMP_BATCHING_SURFACE_DIAGNOSTIC_POLICY,
        CALIBRATION_TIMESTAMP_BATCHING_SURFACE_DIAGNOSTIC_SCHEMA_VERSION,
        CalibrationTimestampBatchingSurfaceDiagnostic,
        CalibrationTimestampBatchingSurfaceDiagnosticState,
    };
    use super::{MarketDataPreparationProgressEvent, MarketDataPreparationProgressPhase};

    #[test]
    fn preparation_progress_phase_exposes_stable_id_and_signal() {
        assert_eq!(
            MarketDataPreparationProgressPhase::BuildingExecutionInput.phase_id(),
            "building_execution_input"
        );
        assert_eq!(
            MarketDataPreparationProgressPhase::BuildingExecutionInput.latest_signal(),
            "materializing staged execution input"
        );
    }

    #[test]
    fn preparation_progress_event_uses_preparation_signal() {
        assert_eq!(
            MarketDataPreparationProgressEvent::Preparing(
                MarketDataPreparationProgressPhase::BuildingDiagnostics
            )
            .latest_signal(),
            "summarizing prepared market data"
        );
    }

    #[test]
    fn preparation_progress_event_uses_active_timestamp_batching_diagnostic_signal() {
        let event = MarketDataPreparationProgressEvent::TimestampBatchingSurface(
            CalibrationTimestampBatchingSurfaceDiagnostic {
                schema_version: CALIBRATION_TIMESTAMP_BATCHING_SURFACE_DIAGNOSTIC_SCHEMA_VERSION
                    .to_string(),
                diagnostic_policy: CALIBRATION_TIMESTAMP_BATCHING_SURFACE_DIAGNOSTIC_POLICY
                    .to_string(),
                state:
                    CalibrationTimestampBatchingSurfaceDiagnosticState::DisabledNoDetectableBatching,
                return_components_stage_selected: true,
                timestamp_batching_surface: TimestampBatchingSurface::disabled(),
                message: "timestamp batching surface evaluated".to_string(),
            },
        );

        assert_eq!(
            event.latest_signal(),
            "recording timestamp batching surface diagnostics"
        );
    }
}
