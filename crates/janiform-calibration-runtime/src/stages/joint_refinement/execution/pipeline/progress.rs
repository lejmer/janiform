use std::sync::mpsc::Sender;

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageProgressPhase,
    JointRefinementStageProgressUpdate,
};
use crate::stages::progress::progress_percent;

pub(super) fn emit_final_assembly_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_units: usize,
    total_units: usize,
    current_item_label: &str,
) {
    emit_joint_refinement_progress(
        progress_sender,
        JointRefinementStageProgressPhase::FinalAssembly,
        completed_units,
        total_units,
        current_item_label,
    );
}

fn emit_single_pass_joint_refinement_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_phase: JointRefinementStageProgressPhase,
    completed_units: usize,
    current_item_label: &str,
) {
    emit_joint_refinement_progress(
        progress_sender,
        progress_phase,
        completed_units,
        1,
        current_item_label,
    );
}

pub(super) fn run_single_pass_joint_refinement_progress_phase<T>(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_phase: JointRefinementStageProgressPhase,
    current_item_label: &str,
    work: impl FnOnce() -> Result<T, JointRefinementStageExecutionFailure>,
) -> Result<T, JointRefinementStageExecutionFailure> {
    emit_single_pass_joint_refinement_progress(
        progress_sender,
        progress_phase,
        0,
        current_item_label,
    );

    let output = work()?;

    emit_single_pass_joint_refinement_progress(
        progress_sender,
        progress_phase,
        1,
        current_item_label,
    );

    Ok(output)
}

pub(super) fn emit_candidate_screening_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_units: usize,
    total_units: usize,
    current_item_label: &str,
) {
    emit_joint_refinement_progress(
        progress_sender,
        JointRefinementStageProgressPhase::CandidateScreening,
        completed_units,
        total_units,
        current_item_label,
    );
}

pub(super) fn emit_coarse_consistency_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    completed_units: usize,
    total_units: usize,
    current_item_label: &str,
) {
    emit_joint_refinement_progress(
        progress_sender,
        JointRefinementStageProgressPhase::BaselineCoarseConsistencyEvaluation,
        completed_units,
        total_units,
        current_item_label,
    );
}

pub(super) fn emit_joint_refinement_subphase_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_phase: JointRefinementStageProgressPhase,
    completed_units: usize,
    total_units: usize,
    current_item_label: &str,
) {
    if let Some(progress_sender) = progress_sender {
        let _ = progress_sender.send(JointRefinementStageProgressUpdate {
            aggregate_progress_percent: Some(progress_percent(completed_units, total_units)),
            current_item_label: Some(current_item_label.to_string()),
            ..JointRefinementStageProgressUpdate::new(progress_phase, completed_units, total_units)
        });
    }
}

fn emit_joint_refinement_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_phase: JointRefinementStageProgressPhase,
    completed_units: usize,
    total_units: usize,
    current_item_label: &str,
) {
    if let Some(progress_sender) = progress_sender {
        let _ = progress_sender.send(JointRefinementStageProgressUpdate {
            current_item_label: Some(current_item_label.to_string()),
            ..JointRefinementStageProgressUpdate::new(progress_phase, completed_units, total_units)
        });
    }
}
