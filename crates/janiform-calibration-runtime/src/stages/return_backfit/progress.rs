use std::sync::mpsc::Sender;

use serde::Serialize;

use crate::stages::progress::{StageProgressUpdate, emit_stage_progress, progress_percent};

pub(crate) const RETURN_BACKFIT_STAGE_PROGRESS_UNITS: usize = 6;
const PERCENT_SCALE: usize = 100;

/// Progress phases emitted by the return-backfit stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReturnBackfitStageProgressPhase {
    InputPreflight,
    ContinuousSamplePreparation,
    DynamicClockReplay,
    DeltaVarianceHandoff,
    MarketPressureRefit,
    ReportAssembly,
}

/// Progress update emitted by the return-backfit stage.
pub type ReturnBackfitStageProgressUpdate = StageProgressUpdate<ReturnBackfitStageProgressPhase>;

pub(crate) fn emit_return_backfit_progress(
    progress_sender: Option<&Sender<ReturnBackfitStageProgressUpdate>>,
    phase: ReturnBackfitStageProgressPhase,
    completed_units: usize,
    total_units: usize,
    current_item_label: Option<&str>,
) {
    let mut progress_update =
        ReturnBackfitStageProgressUpdate::new(phase, completed_units, total_units);

    progress_update.current_item_label = current_item_label.map(str::to_string);

    emit_stage_progress(progress_sender, progress_update);
}

pub(crate) fn emit_return_backfit_subphase_progress(
    progress_sender: Option<&Sender<ReturnBackfitStageProgressUpdate>>,
    phase: ReturnBackfitStageProgressPhase,
    completed_stage_units_before_phase: usize,
    completed_units: usize,
    total_units: usize,
    current_item_label: Option<&str>,
) {
    let mut progress_update =
        ReturnBackfitStageProgressUpdate::new(phase, completed_units, total_units);

    progress_update.aggregate_progress_percent = Some(return_backfit_phase_progress_percent(
        completed_stage_units_before_phase,
        progress_percent(completed_units, total_units),
    ));

    progress_update.current_item_label = current_item_label.map(str::to_string);

    emit_stage_progress(progress_sender, progress_update);
}

pub(crate) fn return_backfit_phase_progress_percent(
    completed_stage_units_before_phase: usize,
    phase_progress_percent: usize,
) -> usize {
    let completed_hundredths = completed_stage_units_before_phase
        .saturating_mul(PERCENT_SCALE)
        .saturating_add(phase_progress_percent.min(PERCENT_SCALE));

    let total_hundredths = RETURN_BACKFIT_STAGE_PROGRESS_UNITS.saturating_mul(PERCENT_SCALE);

    progress_percent(completed_hundredths, total_hundredths)
}
