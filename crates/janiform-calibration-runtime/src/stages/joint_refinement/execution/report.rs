use std::collections::BTreeMap;
use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::{
    CALIBRATION_JOINT_REFINEMENT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_JOINT_REFINEMENT_STAGE_HANDOFF_POLICY, CalibrationRunSpec, CalibrationStage,
    JointRefinementStageLabeledCount, JointRefinementStageRuntimeProfile,
};

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageExecutionReport,
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
    JointRefinementStageSimulationSummary,
};
use crate::stages::seasonality::CalibrationStageExecutionStatus;

use super::super::setup::{
    CandidateSeedSummariesCacheInput, cached_seed_summaries_for_candidate_seeds,
};
use super::{
    JointRefinementCandidateOutcome, JointRefinementContext, JointRefinementSeedSummary,
    ordered_parameter_bits,
};

pub(crate) fn build_joint_candidate_diagnostics(
    candidate_outcomes: &[JointRefinementCandidateOutcome],
) -> janiform_calibration_artifacts::JointRefinementStageCandidateDiagnostics {
    let candidate_count = candidate_outcomes.len();

    let valid_candidate_count = candidate_outcomes
        .iter()
        .filter(|candidate_outcome| {
            candidate_outcome.local_evaluation.invalid_reason.is_none()
                && candidate_outcome.final_evaluation.invalid_reason.is_none()
        })
        .count();

    let invalid_candidate_count = candidate_count.saturating_sub(valid_candidate_count);

    let terminal_candidate_counts = candidate_outcomes.iter().fold(
        BTreeMap::<Vec<u64>, usize>::new(),
        |mut counts, candidate_outcome| {
            *counts
                .entry(ordered_parameter_bits(
                    candidate_outcome.parameter_vector.as_slice(),
                ))
                .or_default() += 1;
            counts
        },
    );

    let unique_terminal_candidate_count = terminal_candidate_counts.len();

    let terminal_collision_count = candidate_count.saturating_sub(unique_terminal_candidate_count);

    let terminal_collision_share = if candidate_count == 0 {
        0.0
    } else {
        terminal_collision_count as f64 / candidate_count as f64
    };

    janiform_calibration_artifacts::JointRefinementStageCandidateDiagnostics {
        candidate_count,
        valid_candidate_count,
        invalid_candidate_count,
        unique_terminal_candidate_count,
        terminal_collision_count,
        terminal_collision_share,
        stop_reason_frequencies: build_labeled_count_frequencies(
            candidate_outcomes
                .iter()
                .map(|candidate_outcome| candidate_outcome.stop_reason.clone()),
        ),
        invalid_reason_frequencies: build_labeled_count_frequencies(
            candidate_outcomes.iter().filter_map(|candidate_outcome| {
                candidate_outcome
                    .final_evaluation
                    .invalid_reason
                    .clone()
                    .or_else(|| candidate_outcome.local_evaluation.invalid_reason.clone())
            }),
        ),
    }
}

pub(crate) fn build_joint_finalist_seed_summaries(
    context: &JointRefinementContext,
    parameter_bits: Option<&[u64]>,
    fitted_parameters: &janiform_calibration_artifacts::JointRefinementStageFittedParameters,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Vec<(u64, JointRefinementSeedSummary)> {
    cached_seed_summaries_for_candidate_seeds(CandidateSeedSummariesCacheInput {
        parameter_bits,
        fitted_parameters,
        context,
        simulation_seeds: context.final_simulation_seeds.as_slice(),
        progress_sender,
        progress_phase: Some(JointRefinementStageProgressPhase::FinalSeedEvaluation),
        completed_units_before_seed_summaries: 0,
        total_units: context.final_simulation_seeds.len(),
    })
    .unwrap_or_default()
}

pub(crate) fn build_joint_simulation_summaries(
    seed_summaries: &[(u64, JointRefinementSeedSummary)],
) -> Vec<JointRefinementStageSimulationSummary> {
    let mut summaries = Vec::with_capacity(seed_summaries.len() * 2);
    for (simulation_seed, seed_summary) in seed_summaries {
        summaries.push(JointRefinementStageSimulationSummary {
            simulation_seed: *simulation_seed,
            horizon_label: "5m".to_string(),
            event_count: seed_summary
                .simulated_5m_intervals
                .iter()
                .map(|interval| interval.tick_count)
                .sum(),
            realized_variance: seed_summary
                .simulated_5m_return_scale_summary
                .realized_variance,
            mean_absolute_return: seed_summary
                .simulated_5m_return_scale_summary
                .mean_absolute_return,
        });

        summaries.push(JointRefinementStageSimulationSummary {
            simulation_seed: *simulation_seed,
            horizon_label: "15m".to_string(),
            event_count: seed_summary
                .simulated_15m_intervals
                .iter()
                .map(|interval| interval.tick_count)
                .sum(),
            realized_variance: seed_summary
                .simulated_15m_return_scale_summary
                .realized_variance,
            mean_absolute_return: seed_summary
                .simulated_15m_return_scale_summary
                .mean_absolute_return,
        });
    }

    summaries
}

pub(crate) fn simulation_infrastructure_failure_message(
    context: &JointRefinementContext,
) -> Option<String> {
    context
        .simulation_infrastructure_failure
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
}

pub(crate) fn option_pricing_execution_failure_message(
    context: &JointRefinementContext,
) -> Option<String> {
    context
        .option_pricing_execution_failure
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
}

pub(crate) fn build_joint_refinement_stage_failure_report(
    run_spec: &CalibrationRunSpec,
    failure: JointRefinementStageExecutionFailure,
) -> JointRefinementStageExecutionReport {
    build_joint_refinement_stage_failure_report_with_runtime_profile(run_spec, failure, None)
}

pub(crate) fn build_joint_refinement_stage_failure_report_with_runtime_profile(
    run_spec: &CalibrationRunSpec,
    failure: JointRefinementStageExecutionFailure,
    runtime_profile: Option<JointRefinementStageRuntimeProfile>,
) -> JointRefinementStageExecutionReport {
    build_joint_refinement_like_stage_failure_report_with_runtime_profile(
        run_spec,
        CalibrationStage::JointRefinement,
        failure,
        runtime_profile,
    )
}

pub(crate) fn build_joint_refinement_like_stage_failure_report_with_runtime_profile(
    run_spec: &CalibrationRunSpec,
    stage: CalibrationStage,
    failure: JointRefinementStageExecutionFailure,
    runtime_profile: Option<JointRefinementStageRuntimeProfile>,
) -> JointRefinementStageExecutionReport {
    JointRefinementStageExecutionReport {
        schema_version: CALIBRATION_JOINT_REFINEMENT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage,
        status: CalibrationStageExecutionStatus::Failure,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_JOINT_REFINEMENT_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: None,
        metadata: None,
        runtime_profile,
        warnings: Vec::new(),
        failure: Some(failure),
        runtime_handoff: None,
    }
}

fn build_labeled_count_frequencies(
    labels: impl Iterator<Item = String>,
) -> Vec<JointRefinementStageLabeledCount> {
    labels
        .fold(BTreeMap::<String, usize>::new(), |mut counts, label| {
            *counts.entry(label).or_default() += 1;
            counts
        })
        .into_iter()
        .map(|(label, count)| JointRefinementStageLabeledCount { label, count })
        .collect()
}
