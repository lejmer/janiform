use std::collections::BTreeMap;
use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::{
    CALIBRATION_JOINT_REFINEMENT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_JOINT_REFINEMENT_STAGE_HANDOFF_POLICY, CalibrationRunSpec, CalibrationStage,
    JointRefinementStageLabeledCount,
};

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageExecutionReport,
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
    JointRefinementStageSimulationSummary,
};
use crate::stages::seasonality::CalibrationStageExecutionStatus;

use super::super::setup::cached_seed_summaries_for_candidate_seeds;
use super::{
    JointRefinementBaselineBuckets, JointRefinementCandidateEvaluation, JointRefinementContext,
    JointRefinementRestartOutcome, JointRefinementSeedSummary, ordered_parameter_bits,
};

pub(crate) fn build_joint_restart_diagnostics(
    restart_outcomes: &[JointRefinementRestartOutcome],
) -> janiform_calibration_artifacts::JointRefinementStageRestartDiagnostics {
    let restart_count = restart_outcomes.len();

    let valid_restart_count = restart_outcomes
        .iter()
        .filter(|restart_outcome| {
            restart_outcome.local_evaluation.invalid_reason.is_none()
                && restart_outcome.final_evaluation.invalid_reason.is_none()
        })
        .count();

    let invalid_restart_count = restart_count.saturating_sub(valid_restart_count);

    let terminal_candidate_counts = restart_outcomes.iter().fold(
        BTreeMap::<Vec<u64>, usize>::new(),
        |mut counts, restart_outcome| {
            *counts
                .entry(ordered_parameter_bits(
                    restart_outcome.parameter_vector.as_slice(),
                ))
                .or_default() += 1;
            counts
        },
    );

    let unique_terminal_candidate_count = terminal_candidate_counts.len();

    let terminal_collision_count = restart_count.saturating_sub(unique_terminal_candidate_count);

    let terminal_collision_share = if restart_count == 0 {
        0.0
    } else {
        terminal_collision_count as f64 / restart_count as f64
    };

    janiform_calibration_artifacts::JointRefinementStageRestartDiagnostics {
        restart_count,
        valid_restart_count,
        invalid_restart_count,
        unique_terminal_candidate_count,
        terminal_collision_count,
        terminal_collision_share,
        stop_reason_frequencies: build_labeled_count_frequencies(
            restart_outcomes
                .iter()
                .map(|restart_outcome| restart_outcome.stop_reason.to_string()),
        ),
        invalid_reason_frequencies: build_labeled_count_frequencies(
            restart_outcomes.iter().filter_map(|restart_outcome| {
                restart_outcome
                    .final_evaluation
                    .invalid_reason
                    .clone()
                    .or_else(|| restart_outcome.local_evaluation.invalid_reason.clone())
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
    cached_seed_summaries_for_candidate_seeds(
        parameter_bits,
        fitted_parameters,
        context,
        context.final_simulation_seeds.as_slice(),
        progress_sender,
        Some(JointRefinementStageProgressPhase::FinalSeedEvaluation),
        0,
        context.final_simulation_seeds.len(),
    )
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

pub(crate) fn baseline_buckets_from_evaluation(
    evaluation: &JointRefinementCandidateEvaluation,
) -> JointRefinementBaselineBuckets {
    let breakdown = &evaluation.breakdown;

    JointRefinementBaselineBuckets {
        aggregated_likelihood_fit: breakdown.aggregated_likelihood_fit,
        aggregated_return_shape_fit: breakdown.aggregated_return_shape_fit,
        return_asymmetry_fit: breakdown.return_asymmetry_fit,
        tick_return_asymmetry_fit: breakdown.tick_return_asymmetry_fit,
        aggregated_return_scale_fit: breakdown.aggregated_return_scale_fit,
        tick_return_scale_fit: breakdown.tick_return_scale_fit,
        business_time_return_scale_fit: breakdown.business_time_return_scale_fit,
        intraday_return_level_fit: breakdown.intraday_return_level_fit,
        activity_shape_fit: breakdown.activity_shape_fit,
        activity_level_fit: breakdown.activity_level_fit,
        rv_activity_coupling_fit: breakdown.rv_activity_coupling_fit,
        price_span_sanity_fit: breakdown.price_span_sanity_fit,
        option_spread_normalized_price_fit: breakdown.option_spread_normalized_price_fit,
    }
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
    JointRefinementStageExecutionReport {
        schema_version: CALIBRATION_JOINT_REFINEMENT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::JointRefinement,
        status: CalibrationStageExecutionStatus::Failure,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_JOINT_REFINEMENT_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: None,
        metadata: None,
        failure: Some(failure),
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
