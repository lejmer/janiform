use std::sync::Mutex;
use std::sync::mpsc::Sender;

use super::super::super::contracts::{
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
};
pub(super) fn emit_completed_candidate_screening_unit(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_screening_units: &Mutex<usize>,
    total_screening_units: usize,
    current_item_label: &str,
) {
    let Ok(mut completed_units_guard) = completed_screening_units.lock() else {
        return;
    };

    *completed_units_guard = completed_units_guard.saturating_add(1);

    emit_candidate_screening_progress(
        progress_sender,
        *completed_units_guard,
        total_screening_units,
        Some(current_item_label),
    );
}

pub(super) fn emit_candidate_screening_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_units: usize,
    total_units: usize,
    current_item_label: Option<&str>,
) {
    let Some(progress_sender) = progress_sender else {
        return;
    };

    let _ = progress_sender.send(JointRefinementStageProgressUpdate {
        current_item_label: current_item_label.map(str::to_string),
        ..JointRefinementStageProgressUpdate::new(
            JointRefinementStageProgressPhase::CandidateScreening,
            completed_units,
            total_units,
        )
    });
}

pub(super) fn emit_candidate_seed_generation_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_units: usize,
    total_units: usize,
    current_item_label: Option<&str>,
) {
    let Some(progress_sender) = progress_sender else {
        return;
    };

    let _ = progress_sender.send(JointRefinementStageProgressUpdate {
        current_item_label: current_item_label.map(str::to_string),
        ..JointRefinementStageProgressUpdate::new(
            JointRefinementStageProgressPhase::CandidateSeedGeneration,
            completed_units,
            total_units,
        )
    });
}
