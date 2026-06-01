use std::error::Error;
use std::fmt::{Display, Formatter};

use super::loading::MarketDataLoadingProgressEvent;

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
    /// Market-data preparation finished.
    Completed,
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
            Self::Completed => "completed",
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
            Self::Completed => "Prepared market data plan",
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
            Self::Completed => "market-data calibration plan prepared",
        }
    }
}

/// Progress emitted while market-data preparation runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarketDataPreparationProgressEvent {
    /// Source loading progress.
    Loading(MarketDataLoadingProgressEvent),
    /// Post-loading preparation phase progress.
    Preparing(MarketDataPreparationProgressPhase),
}

impl MarketDataPreparationProgressEvent {
    /// Human-readable latest signal for terminal progress displays.
    pub fn latest_signal(self) -> String {
        match self {
            Self::Loading(event) => event.latest_signal(),
            Self::Preparing(phase) => phase.latest_signal().to_string(),
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
}
