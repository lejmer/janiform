use std::fs;
use std::path::Path;

use janiform_foundations::parallelism::available_cpu_count;
use serde_json::to_string_pretty;

use crate::staged_runner::CalibrationStagedRunStageReport;
use crate::stages::hawkes::HawkesStageFittedPayload;
use crate::stages::joint_refinement::{
    JointRefinementStageCandidateDiagnostic, JointRefinementStageCoarseTickConsistencyMetric,
    JointRefinementStageFittedPayload, JointRefinementStageSimulationSummary,
};
use crate::stages::return_components::{
    ReturnComponentsStageCandidateDiagnostic, ReturnComponentsStageFittedPayload,
};

use super::types::{
    CalibrationDiagnosticArtifactBundle, CalibrationDiagnosticArtifactReference,
    CalibrationDiagnosticCandidateFrontierArtifact, CalibrationDiagnosticCandidateFrontierEntry,
    CalibrationDiagnosticCoarseTickConsistencyArtifact, CalibrationDiagnosticConsistencyMetric,
    CalibrationDiagnosticEvaluationTier, CalibrationDiagnosticFamilyMetric,
    CalibrationDiagnosticMetricEligibility, CalibrationDiagnosticMetricRole,
    CalibrationDiagnosticObservedTargetEntry, CalibrationDiagnosticObservedTargetsArtifact,
    CalibrationDiagnosticRuntimeProfileArtifact, CalibrationDiagnosticRuntimeProfileEntry,
    CalibrationDiagnosticSearchSurfaceManifestArtifact, CalibrationDiagnosticSearchSurfacePoint,
    CalibrationDiagnosticSelectionDecision, CalibrationDiagnosticSelectionRationaleArtifact,
    CalibrationDiagnosticSimulationSummaryArtifact, CalibrationDiagnosticSimulationSummaryEntry,
};

const OBSERVED_TARGETS_FILENAME: &str = "observed_targets.json";
const CANDIDATE_FRONTIER_FILENAME: &str = "candidate_frontier.json";
const COARSE_TICK_CONSISTENCY_FILENAME: &str = "coarse_tick_consistency.json";
const SELECTION_RATIONALE_FILENAME: &str = "selection_rationale.json";
const SIMULATION_SUMMARY_FILENAME: &str = "simulation_summary.json";
const SEARCH_SURFACE_MANIFEST_FILENAME: &str = "search_surface_manifest.json";
const RUNTIME_PROFILE_FILENAME: &str = "runtime_profile.json";

pub(crate) fn build_diagnostic_artifact_bundle(
    stage_reports: &[CalibrationStagedRunStageReport],
    runtime_entries: Vec<CalibrationDiagnosticRuntimeProfileEntry>,
) -> Option<CalibrationDiagnosticArtifactBundle> {
    let hawkes_payload = stage_reports
        .iter()
        .find_map(|stage_report| match stage_report {
            CalibrationStagedRunStageReport::Hawkes(report) => report.fitted_stage_payload.as_ref(),
            _ => None,
        })?;
    let return_payload = stage_reports
        .iter()
        .find_map(|stage_report| match stage_report {
            CalibrationStagedRunStageReport::ReturnComponents(report) => {
                report.fitted_stage_payload.as_ref()
            }
            _ => None,
        })?;
    let joint_payload = stage_reports
        .iter()
        .find_map(|stage_report| match stage_report {
            CalibrationStagedRunStageReport::JointRefinement(report) => {
                report.fitted_stage_payload.as_ref()
            }
            _ => None,
        })?;

    let observed_targets = build_observed_targets_artifact(hawkes_payload, return_payload);
    let candidate_frontier =
        build_candidate_frontier_artifact(hawkes_payload, return_payload, joint_payload);
    let coarse_tick_consistency = build_coarse_tick_consistency_artifact(joint_payload);
    let selection_rationale = build_selection_rationale_artifact(joint_payload);
    let simulation_summary = build_simulation_summary_artifact(joint_payload);
    let search_surface_manifest =
        build_search_surface_manifest_artifact(return_payload, joint_payload);

    let runtime_profile = CalibrationDiagnosticRuntimeProfileArtifact {
        schema_version: "calibration.diagnostics.runtime_profile.v1",
        cpu_count: available_cpu_count(),
        entries: runtime_entries,
    };

    Some(CalibrationDiagnosticArtifactBundle {
        observed_targets,
        candidate_frontier,
        coarse_tick_consistency,
        selection_rationale,
        simulation_summary,
        search_surface_manifest,
        runtime_profile,
    })
}

pub(crate) fn persist_diagnostic_artifact_bundle(
    run_directory: &Path,
    artifact_bundle: &CalibrationDiagnosticArtifactBundle,
) -> Result<Vec<CalibrationDiagnosticArtifactReference>, String> {
    let artifacts_directory = run_directory.join("diagnostics");

    fs::create_dir_all(&artifacts_directory)
        .map_err(|source| format!("failed to create diagnostics directory: {source}"))?;

    let families = [
        ("observed_targets", OBSERVED_TARGETS_FILENAME),
        ("candidate_frontier", CANDIDATE_FRONTIER_FILENAME),
        ("coarse_tick_consistency", COARSE_TICK_CONSISTENCY_FILENAME),
        ("selection_rationale", SELECTION_RATIONALE_FILENAME),
        ("simulation_summary", SIMULATION_SUMMARY_FILENAME),
        ("search_surface_manifest", SEARCH_SURFACE_MANIFEST_FILENAME),
        ("runtime_profile", RUNTIME_PROFILE_FILENAME),
    ];

    let payloads = [
        to_string_pretty(&artifact_bundle.observed_targets),
        to_string_pretty(&artifact_bundle.candidate_frontier),
        to_string_pretty(&artifact_bundle.coarse_tick_consistency),
        to_string_pretty(&artifact_bundle.selection_rationale),
        to_string_pretty(&artifact_bundle.simulation_summary),
        to_string_pretty(&artifact_bundle.search_surface_manifest),
        to_string_pretty(&artifact_bundle.runtime_profile),
    ];

    let mut references = Vec::with_capacity(families.len());
    for ((artifact_family, file_name), payload_result) in
        families.into_iter().zip(payloads.into_iter())
    {
        let payload = payload_result.map_err(|source| {
            format!("failed to serialize {artifact_family} diagnostics: {source}")
        })?;
        let artifact_path = artifacts_directory.join(file_name);

        fs::write(&artifact_path, payload)
            .map_err(|source| format!("failed to write {artifact_family} diagnostics: {source}"))?;

        references.push(CalibrationDiagnosticArtifactReference {
            artifact_family: artifact_family.to_string(),
            relative_path: format!("diagnostics/{file_name}"),
        });
    }

    Ok(references)
}

fn build_observed_targets_artifact(
    hawkes_payload: &HawkesStageFittedPayload,
    return_payload: &ReturnComponentsStageFittedPayload,
) -> CalibrationDiagnosticObservedTargetsArtifact {
    let mut targets = Vec::new();
    if let Some(selected_hawkes_candidate) = hawkes_payload
        .candidate_diagnostics
        .iter()
        .find(|candidate| candidate.candidate_id == hawkes_payload.selected_candidate_id)
    {
        for objective_term in &selected_hawkes_candidate.objective_terms {
            targets.push(CalibrationDiagnosticObservedTargetEntry {
                target_id: format!("hawkes.{}", objective_term.term_id),
                target_family: classify_metric_family(objective_term.term_id.as_str()).to_string(),
                definition: format!("Observed Hawkes target `{}`", objective_term.term_id),
                estimation_method: "deterministic_open_session_activity_summary.v1".to_string(),
                effective_sample_size: selected_hawkes_candidate
                    .source_regime
                    .open_session_tick_count as f64,
                uncertainty_scale: objective_term.scale.abs().max(1.0e-8),
                stability_score: 1.0,
                evaluation_eligibility: CalibrationDiagnosticMetricEligibility::ExactEligible,
                admissibility_role: CalibrationDiagnosticMetricRole::Admissibility,
                ranking_role: CalibrationDiagnosticMetricRole::Ranking,
                horizon_label: None,
                conditioning_label: None,
                units: "normalized".to_string(),
                observed_value: objective_term.observed_value,
            });
        }
    }

    if let Some(selected_return_candidate) = return_payload
        .candidate_diagnostics
        .iter()
        .find(|candidate| candidate.candidate_id == return_payload.selected_candidate_id)
    {
        let summary_statistics = &selected_return_candidate
            .candidate_provenance
            .summary_statistics;
        let return_distribution = &selected_return_candidate.return_distribution_diagnostic;
        let surprise_arrival = &selected_return_candidate.surprise_arrival_diagnostic;
        let jump = &selected_return_candidate.jump_diagnostic;
        let gap = &selected_return_candidate.gap_diagnostic;
        let activity_coupling = &selected_return_candidate.activity_coupling_diagnostic;
        let price_range = &selected_return_candidate.price_range_sanity_diagnostic;

        targets.extend([
            build_observed_target_entry(ObservedTargetEntryBuildInput {
                target_id: "return_components.mean_return",
                target_family: "return_center_mass",
                definition: "Observed open-session mean block return",
                estimation_method: "deterministic_open_session_return_summary.v1",
                effective_sample_size: return_distribution.sample_count as f64,
                uncertainty_scale: return_distribution.observed_mean_return.abs().max(1.0e-8),
                evaluation_eligibility: CalibrationDiagnosticMetricEligibility::ExactEligible,
                admissibility_role: CalibrationDiagnosticMetricRole::Ranking,
                ranking_role: CalibrationDiagnosticMetricRole::Ranking,
                units: "log_return",
                observed_value: return_distribution.observed_mean_return,
            }),
            build_observed_target_entry(ObservedTargetEntryBuildInput {
                target_id: "return_components.return_volatility",
                target_family: "return_scale",
                definition: "Observed open-session return volatility",
                estimation_method: "deterministic_open_session_return_summary.v1",
                effective_sample_size: return_distribution.sample_count as f64,
                uncertainty_scale: return_distribution
                    .observed_return_volatility
                    .abs()
                    .max(1.0e-8),
                evaluation_eligibility: CalibrationDiagnosticMetricEligibility::ExactEligible,
                admissibility_role: CalibrationDiagnosticMetricRole::Admissibility,
                ranking_role: CalibrationDiagnosticMetricRole::Ranking,
                units: "log_return",
                observed_value: return_distribution.observed_return_volatility,
            }),
            build_observed_target_entry(ObservedTargetEntryBuildInput {
                target_id: "return_components.skewness",
                target_family: "return_asymmetry",
                definition: "Observed open-session return skewness",
                estimation_method: "deterministic_open_session_return_summary.v1",
                effective_sample_size: return_distribution.sample_count as f64,
                uncertainty_scale: 0.1,
                evaluation_eligibility: CalibrationDiagnosticMetricEligibility::ExactEligible,
                admissibility_role: CalibrationDiagnosticMetricRole::Ranking,
                ranking_role: CalibrationDiagnosticMetricRole::Ranking,
                units: "unitless",
                observed_value: return_distribution.observed_skewness,
            }),
            build_observed_target_entry(ObservedTargetEntryBuildInput {
                target_id: "return_components.excess_kurtosis",
                target_family: "return_tails",
                definition: "Observed open-session return excess kurtosis",
                estimation_method: "deterministic_open_session_return_summary.v1",
                effective_sample_size: return_distribution.sample_count as f64,
                uncertainty_scale: 0.25,
                evaluation_eligibility: CalibrationDiagnosticMetricEligibility::ExactEligible,
                admissibility_role: CalibrationDiagnosticMetricRole::Ranking,
                ranking_role: CalibrationDiagnosticMetricRole::Ranking,
                units: "unitless",
                observed_value: return_distribution.observed_excess_kurtosis,
            }),
            build_observed_target_entry(ObservedTargetEntryBuildInput {
                target_id: "return_components.surprise_arrival_intensity_per_hour",
                target_family: "event_conditioned_return_behavior",
                definition: "Observed surprise jump arrival intensity per hour",
                estimation_method: "surprise_episode_detection_summary.v1",
                effective_sample_size: summary_statistics.selected_surprise_jump_episode_count
                    as f64,
                uncertainty_scale: surprise_arrival
                    .observed_jump_frequency_per_hour
                    .abs()
                    .max(1.0e-8),
                evaluation_eligibility: CalibrationDiagnosticMetricEligibility::ExactEligible,
                admissibility_role: CalibrationDiagnosticMetricRole::Ranking,
                ranking_role: CalibrationDiagnosticMetricRole::Ranking,
                units: "events_per_hour",
                observed_value: surprise_arrival.observed_jump_frequency_per_hour,
            }),
            build_observed_target_entry(ObservedTargetEntryBuildInput {
                target_id: "return_components.open_jump_abs_return",
                target_family: "event_conditioned_return_behavior",
                definition: "Observed mean absolute open-session jump return",
                estimation_method: "jump_episode_summary.v1",
                effective_sample_size: jump.observed_jump_count as f64,
                uncertainty_scale: jump.observed_mean_abs_jump_return.abs().max(1.0e-8),
                evaluation_eligibility: CalibrationDiagnosticMetricEligibility::ExactEligible,
                admissibility_role: CalibrationDiagnosticMetricRole::Ranking,
                ranking_role: CalibrationDiagnosticMetricRole::Ranking,
                units: "absolute_log_return",
                observed_value: jump.observed_mean_abs_jump_return,
            }),
            build_observed_target_entry(ObservedTargetEntryBuildInput {
                target_id: "return_components.closed_gap_abs_return",
                target_family: "event_conditioned_return_behavior",
                definition: "Observed mean absolute close-open gap return",
                estimation_method: "gap_episode_summary.v1",
                effective_sample_size: gap.observed_gap_count as f64,
                uncertainty_scale: gap.observed_mean_abs_gap_return.abs().max(1.0e-8),
                evaluation_eligibility: CalibrationDiagnosticMetricEligibility::ExactEligible,
                admissibility_role: CalibrationDiagnosticMetricRole::Ranking,
                ranking_role: CalibrationDiagnosticMetricRole::Ranking,
                units: "absolute_log_return",
                observed_value: gap.observed_mean_abs_gap_return,
            }),
            build_observed_target_entry(ObservedTargetEntryBuildInput {
                target_id: "return_components.known_post_shock_activity_lift",
                target_family: "event_conditioned_activity_response",
                definition: "Observed known-news post-shock activity lift",
                estimation_method: "event_conditioned_activity_summary.v1",
                effective_sample_size: summary_statistics.known_realized_sample_count as f64,
                uncertainty_scale: activity_coupling
                    .observed_known_post_shock_activity_lift
                    .abs()
                    .max(1.0e-8),
                evaluation_eligibility: CalibrationDiagnosticMetricEligibility::ExactEligible,
                admissibility_role: CalibrationDiagnosticMetricRole::Ranking,
                ranking_role: CalibrationDiagnosticMetricRole::Ranking,
                units: "activity_lift",
                observed_value: activity_coupling.observed_known_post_shock_activity_lift,
            }),
            build_observed_target_entry(ObservedTargetEntryBuildInput {
                target_id: "return_components.surprise_post_shock_activity_lift",
                target_family: "event_conditioned_activity_response",
                definition: "Observed surprise-event post-shock activity lift",
                estimation_method: "event_conditioned_activity_summary.v1",
                effective_sample_size: summary_statistics.selected_surprise_jump_episode_count
                    as f64,
                uncertainty_scale: activity_coupling
                    .observed_surprise_post_shock_activity_lift
                    .abs()
                    .max(1.0e-8),
                evaluation_eligibility: CalibrationDiagnosticMetricEligibility::ExactEligible,
                admissibility_role: CalibrationDiagnosticMetricRole::Ranking,
                ranking_role: CalibrationDiagnosticMetricRole::Ranking,
                units: "activity_lift",
                observed_value: activity_coupling.observed_surprise_post_shock_activity_lift,
            }),
            build_observed_target_entry(ObservedTargetEntryBuildInput {
                target_id: "return_components.windowed_log_price_span",
                target_family: "diagnostic",
                definition:
                    "Observed reference log-price span summary used only for admissibility diagnostics",
                estimation_method: "path_functional_reference_summary.v1",
                effective_sample_size: price_range.effective_continuous_sample_count as f64,
                uncertainty_scale: price_range.observed_log_price_span.abs().max(1.0e-8),
                evaluation_eligibility: CalibrationDiagnosticMetricEligibility::CoarseEligible,
                admissibility_role: CalibrationDiagnosticMetricRole::Diagnostic,
                ranking_role: CalibrationDiagnosticMetricRole::Diagnostic,
                units: "log_price_span",
                observed_value: price_range.observed_log_price_span,
            }),
        ]);
    }

    CalibrationDiagnosticObservedTargetsArtifact {
        schema_version: "calibration.diagnostics.observed_targets.v1",
        target_builder_version: "staged_payload_target_extraction.v1",
        targets,
    }
}

fn build_candidate_frontier_artifact(
    hawkes_payload: &HawkesStageFittedPayload,
    return_payload: &ReturnComponentsStageFittedPayload,
    joint_payload: &JointRefinementStageFittedPayload,
) -> CalibrationDiagnosticCandidateFrontierArtifact {
    let mut frontier_entries = Vec::new();
    let mut hawkes_candidates = hawkes_payload
        .candidate_diagnostics
        .iter()
        .collect::<Vec<_>>();

    hawkes_candidates.sort_by(|left, right| {
        left.penalized_objective_value
            .total_cmp(&right.penalized_objective_value)
            .then_with(|| left.candidate_id.cmp(&right.candidate_id))
    });

    for (position, candidate) in hawkes_candidates.into_iter().enumerate() {
        let admissible = candidate.constraint_violation_count == 0;

        frontier_entries.push(CalibrationDiagnosticCandidateFrontierEntry {
            candidate_id: candidate.candidate_id.clone(),
            stage: "hawkes".to_string(),
            evaluation_tier: CalibrationDiagnosticEvaluationTier::ExactBlock,
            valid: admissible,
            admissible,
            pareto_rank: position + 1,
            lexicographic_rank: position + 1,
            scalar_tie_break: candidate.penalized_objective_value,
            rejection_reason: (!admissible).then_some("constraint_violation".to_string()),
            family_metrics: candidate
                .objective_terms
                .iter()
                .map(|term| CalibrationDiagnosticFamilyMetric {
                    metric_id: term.term_id.clone(),
                    target_id: format!("hawkes.{}", term.term_id),
                    target_family: classify_metric_family(term.term_id.as_str()).to_string(),
                    evaluation_tier: CalibrationDiagnosticEvaluationTier::ExactBlock,
                    normalized_distance: term.normalized_error,
                    admissible,
                    simulated_value: term.model_implied_value,
                    observed_value: term.observed_value,
                    uncertainty_scale: term.scale.abs().max(1.0e-8),
                })
                .collect(),
            canonical_coordinates: Vec::new(),
        });
    }

    let mut return_candidates = return_payload
        .candidate_diagnostics
        .iter()
        .collect::<Vec<_>>();

    return_candidates.sort_by(|left, right| {
        left.penalized_objective_value
            .total_cmp(&right.penalized_objective_value)
            .then_with(|| left.candidate_id.cmp(&right.candidate_id))
    });

    for (position, candidate) in return_candidates.into_iter().enumerate() {
        frontier_entries.push(build_return_candidate_frontier_entry(
            candidate,
            position + 1,
        ));
    }

    frontier_entries.extend(
        joint_payload
            .candidate_frontier
            .iter()
            .map(build_joint_candidate_frontier_entry),
    );

    CalibrationDiagnosticCandidateFrontierArtifact {
        schema_version: "calibration.diagnostics.candidate_frontier.v1",
        frontier_entries,
    }
}

fn build_return_candidate_frontier_entry(
    candidate: &ReturnComponentsStageCandidateDiagnostic,
    rank: usize,
) -> CalibrationDiagnosticCandidateFrontierEntry {
    let admissible = candidate.constraint_violation_count == 0
        && !candidate.price_range_sanity_diagnostic.is_flagged;

    CalibrationDiagnosticCandidateFrontierEntry {
        candidate_id: candidate.candidate_id.clone(),
        stage: "return_components".to_string(),
        evaluation_tier: CalibrationDiagnosticEvaluationTier::ExactBlock,
        valid: candidate.constraint_violation_count == 0,
        admissible,
        pareto_rank: rank,
        lexicographic_rank: rank,
        scalar_tie_break: candidate.penalized_objective_value,
        rejection_reason: (!admissible)
            .then_some("constraint_or_admissibility_failure".to_string()),
        family_metrics: vec![
            CalibrationDiagnosticFamilyMetric {
                metric_id: "continuous_distribution_fit".to_string(),
                target_id: "return_components.return_volatility".to_string(),
                target_family: "return_scale".to_string(),
                evaluation_tier: CalibrationDiagnosticEvaluationTier::ExactBlock,
                normalized_distance: candidate
                    .objective_metric_buckets
                    .continuous_distribution_fit,
                admissible,
                simulated_value: candidate
                    .return_distribution_diagnostic
                    .fitted_return_volatility,
                observed_value: candidate
                    .return_distribution_diagnostic
                    .observed_return_volatility,
                uncertainty_scale: candidate
                    .return_distribution_diagnostic
                    .observed_return_volatility
                    .abs()
                    .max(1.0e-8),
            },
            CalibrationDiagnosticFamilyMetric {
                metric_id: "continuous_center_mass_fit".to_string(),
                target_id: "return_components.mean_return".to_string(),
                target_family: "return_center_mass".to_string(),
                evaluation_tier: CalibrationDiagnosticEvaluationTier::ExactBlock,
                normalized_distance: candidate
                    .continuous_distribution_fit_breakdown
                    .mean_return_fit,
                admissible,
                simulated_value: candidate.return_distribution_diagnostic.fitted_mean_return,
                observed_value: candidate
                    .return_distribution_diagnostic
                    .observed_mean_return,
                uncertainty_scale: candidate
                    .return_distribution_diagnostic
                    .observed_mean_return
                    .abs()
                    .max(1.0e-8),
            },
            CalibrationDiagnosticFamilyMetric {
                metric_id: "continuous_skewness_fit".to_string(),
                target_id: "return_components.skewness".to_string(),
                target_family: "return_asymmetry".to_string(),
                evaluation_tier: CalibrationDiagnosticEvaluationTier::ExactBlock,
                normalized_distance: candidate
                    .continuous_distribution_fit_breakdown
                    .bowley_skewness_fit,
                admissible,
                simulated_value: candidate.return_distribution_diagnostic.fitted_skewness,
                observed_value: candidate.return_distribution_diagnostic.observed_skewness,
                uncertainty_scale: 0.1,
            },
            CalibrationDiagnosticFamilyMetric {
                metric_id: "continuous_tail_fit".to_string(),
                target_id: "return_components.excess_kurtosis".to_string(),
                target_family: "return_tails".to_string(),
                evaluation_tier: CalibrationDiagnosticEvaluationTier::ExactBlock,
                normalized_distance: candidate
                    .continuous_distribution_fit_breakdown
                    .l_moment_tau3_fit,
                admissible,
                simulated_value: candidate
                    .return_distribution_diagnostic
                    .fitted_excess_kurtosis,
                observed_value: candidate
                    .return_distribution_diagnostic
                    .observed_excess_kurtosis,
                uncertainty_scale: 0.25,
            },
            CalibrationDiagnosticFamilyMetric {
                metric_id: "surprise_arrival_fit".to_string(),
                target_id: "return_components.surprise_arrival_intensity_per_hour".to_string(),
                target_family: "event_conditioned_return_behavior".to_string(),
                evaluation_tier: CalibrationDiagnosticEvaluationTier::ExactBlock,
                normalized_distance: candidate.objective_metric_buckets.surprise_arrival_fit,
                admissible,
                simulated_value: candidate
                    .surprise_arrival_diagnostic
                    .fitted_surprise_arrival_intensity_per_hour,
                observed_value: candidate
                    .surprise_arrival_diagnostic
                    .observed_jump_frequency_per_hour,
                uncertainty_scale: candidate
                    .surprise_arrival_diagnostic
                    .observed_jump_frequency_per_hour
                    .abs()
                    .max(1.0e-8),
            },
            CalibrationDiagnosticFamilyMetric {
                metric_id: "open_jump_fit".to_string(),
                target_id: "return_components.open_jump_abs_return".to_string(),
                target_family: "event_conditioned_return_behavior".to_string(),
                evaluation_tier: CalibrationDiagnosticEvaluationTier::ExactBlock,
                normalized_distance: candidate.objective_metric_buckets.open_jump_fit,
                admissible,
                simulated_value: candidate.jump_diagnostic.fitted_mean_abs_jump_return,
                observed_value: candidate.jump_diagnostic.observed_mean_abs_jump_return,
                uncertainty_scale: candidate
                    .jump_diagnostic
                    .observed_mean_abs_jump_return
                    .abs()
                    .max(1.0e-8),
            },
            CalibrationDiagnosticFamilyMetric {
                metric_id: "closed_gap_fit".to_string(),
                target_id: "return_components.closed_gap_abs_return".to_string(),
                target_family: "event_conditioned_return_behavior".to_string(),
                evaluation_tier: CalibrationDiagnosticEvaluationTier::ExactBlock,
                normalized_distance: candidate.objective_metric_buckets.closed_gap_fit,
                admissible,
                simulated_value: candidate.gap_diagnostic.fitted_mean_abs_gap_return,
                observed_value: candidate.gap_diagnostic.observed_mean_abs_gap_return,
                uncertainty_scale: candidate
                    .gap_diagnostic
                    .observed_mean_abs_gap_return
                    .abs()
                    .max(1.0e-8),
            },
            CalibrationDiagnosticFamilyMetric {
                metric_id: "activity_coupling_fit".to_string(),
                target_id: "return_components.known_post_shock_activity_lift".to_string(),
                target_family: "event_conditioned_activity_response".to_string(),
                evaluation_tier: CalibrationDiagnosticEvaluationTier::ExactBlock,
                normalized_distance: candidate.objective_metric_buckets.activity_coupling_fit,
                admissible,
                simulated_value: candidate
                    .activity_coupling_diagnostic
                    .fitted_known_activity_coupling_scale,
                observed_value: candidate
                    .activity_coupling_diagnostic
                    .observed_known_post_shock_activity_lift,
                uncertainty_scale: candidate
                    .activity_coupling_diagnostic
                    .observed_known_post_shock_activity_lift
                    .abs()
                    .max(1.0e-8),
            },
            CalibrationDiagnosticFamilyMetric {
                metric_id: "price_range_sanity_fit".to_string(),
                target_id: "return_components.windowed_log_price_span".to_string(),
                target_family: "diagnostic".to_string(),
                evaluation_tier: CalibrationDiagnosticEvaluationTier::ExactBlock,
                normalized_distance: candidate.objective_metric_buckets.price_range_sanity_fit,
                admissible,
                simulated_value: candidate
                    .price_range_sanity_diagnostic
                    .fitted_reference_log_price_span,
                observed_value: candidate
                    .price_range_sanity_diagnostic
                    .observed_log_price_span,
                uncertainty_scale: candidate
                    .price_range_sanity_diagnostic
                    .observed_log_price_span
                    .abs()
                    .max(1.0e-8),
            },
        ],
        canonical_coordinates: Vec::new(),
    }
}

fn build_joint_candidate_frontier_entry(
    candidate: &JointRefinementStageCandidateDiagnostic,
) -> CalibrationDiagnosticCandidateFrontierEntry {
    CalibrationDiagnosticCandidateFrontierEntry {
        candidate_id: candidate.candidate_id.clone(),
        stage: "joint_refinement".to_string(),
        evaluation_tier: CalibrationDiagnosticEvaluationTier::TickJoint,
        valid: candidate.valid,
        admissible: candidate.admissible,
        pareto_rank: candidate.pareto_rank,
        lexicographic_rank: candidate.lexicographic_rank,
        scalar_tie_break: candidate.scalar_tie_break,
        rejection_reason: candidate.invalid_reason.clone(),
        family_metrics: candidate
            .objective_terms
            .iter()
            .map(|term| CalibrationDiagnosticFamilyMetric {
                metric_id: term.metric_id.clone(),
                target_id: term.target_id.clone(),
                target_family: term.target_family.clone(),
                evaluation_tier: CalibrationDiagnosticEvaluationTier::TickJoint,
                normalized_distance: term.normalized_error,
                admissible: candidate.admissible,
                simulated_value: term.simulated_value,
                observed_value: term.observed_value,
                uncertainty_scale: term.uncertainty_scale,
            })
            .collect(),
        canonical_coordinates: candidate.canonical_coordinates.clone(),
    }
}

fn build_coarse_tick_consistency_artifact(
    joint_payload: &JointRefinementStageFittedPayload,
) -> CalibrationDiagnosticCoarseTickConsistencyArtifact {
    CalibrationDiagnosticCoarseTickConsistencyArtifact {
        schema_version: "calibration.diagnostics.coarse_tick_consistency.v1",
        metrics: joint_payload
            .coarse_tick_consistency
            .iter()
            .map(build_consistency_metric)
            .collect(),
    }
}

fn build_consistency_metric(
    metric: &JointRefinementStageCoarseTickConsistencyMetric,
) -> CalibrationDiagnosticConsistencyMetric {
    CalibrationDiagnosticConsistencyMetric {
        metric_id: format!("{}.{}", metric.horizon_label, metric.metric_id),
        horizon_label: metric.horizon_label.clone(),
        coarse_value: metric.coarse_value,
        tick_value: metric.tick_value,
        absolute_delta: metric.absolute_delta,
        normalized_delta: metric.normalized_delta,
    }
}

fn build_selection_rationale_artifact(
    joint_payload: &JointRefinementStageFittedPayload,
) -> CalibrationDiagnosticSelectionRationaleArtifact {
    let admissible_candidate_count = joint_payload
        .candidate_frontier
        .iter()
        .filter(|candidate| candidate.admissible)
        .count();
    let pareto_frontier_count = joint_payload
        .candidate_frontier
        .iter()
        .filter(|candidate| candidate.pareto_rank == 1)
        .count();

    CalibrationDiagnosticSelectionRationaleArtifact {
        schema_version: "calibration.diagnostics.selection_rationale.v1",
        decision: CalibrationDiagnosticSelectionDecision {
            selection_stage: "joint_refinement".to_string(),
            selected_candidate_id: joint_payload.selected_candidate_id.clone(),
            selection_policy: "validity_then_admissibility_then_pareto_then_lexicographic_tie_break.v1".to_string(),
            validity_screened_candidate_count: joint_payload.candidate_frontier.len(),
            admissible_candidate_count,
            pareto_frontier_count,
            tie_break_candidate_count: joint_payload
                .candidate_frontier
                .iter()
                .filter(|candidate| candidate.lexicographic_rank <= 2)
                .count(),
            decisive_metric_ids: vec![
                "joint.return_scale".to_string(),
                "joint.return_asymmetry.central_skewness".to_string(),
                "joint.activity_level".to_string(),
                "joint.rv_activity_coupling".to_string(),
            ],
            rationale: "Selected the joint candidate after validity screening, admissibility gating, and lexicographic ordering on the admissible Pareto frontier.".to_string(),
        },
    }
}

fn build_simulation_summary_artifact(
    joint_payload: &JointRefinementStageFittedPayload,
) -> CalibrationDiagnosticSimulationSummaryArtifact {
    CalibrationDiagnosticSimulationSummaryArtifact {
        schema_version: "calibration.diagnostics.simulation_summary.v1",
        summaries: joint_payload
            .simulation_summaries
            .iter()
            .map(build_simulation_summary_entry)
            .collect(),
    }
}

fn build_simulation_summary_entry(
    summary: &JointRefinementStageSimulationSummary,
) -> CalibrationDiagnosticSimulationSummaryEntry {
    CalibrationDiagnosticSimulationSummaryEntry {
        simulation_seed: summary.simulation_seed,
        evaluation_tier: CalibrationDiagnosticEvaluationTier::TickJoint,
        horizon_label: summary.horizon_label.clone(),
        event_count: summary.event_count,
        realized_variance: summary.realized_variance,
        mean_absolute_return: summary.mean_absolute_return,
    }
}

fn build_search_surface_manifest_artifact(
    return_payload: &ReturnComponentsStageFittedPayload,
    joint_payload: &JointRefinementStageFittedPayload,
) -> CalibrationDiagnosticSearchSurfaceManifestArtifact {
    let mut surfaces = Vec::new();
    if let Some(selected_return_candidate) = return_payload
        .candidate_diagnostics
        .iter()
        .find(|candidate| candidate.candidate_id == return_payload.selected_candidate_id)
    {
        surfaces.extend(
            selected_return_candidate
                .continuous_distribution_fit_breakdown
                .optimizer_basin_diagnostics
                .iter()
                .map(|basin| CalibrationDiagnosticSearchSurfacePoint {
                    surface_id: "return_components.nig.log_alpha_vs_log_delta_offset".to_string(),
                    x_label: "log_alpha".to_string(),
                    x_value: basin.final_log_alpha,
                    y_label: Some("log_delta_offset".to_string()),
                    y_value: Some(basin.final_log_delta_offset),
                    objective_value: basin.final_objective,
                    admissible: true,
                }),
        );
    }

    surfaces.extend(joint_payload.candidate_frontier.iter().map(|candidate| {
        CalibrationDiagnosticSearchSurfacePoint {
            surface_id: "joint_refinement.frontier.weighted_objective".to_string(),
            x_label: "lexicographic_rank".to_string(),
            x_value: candidate.lexicographic_rank as f64,
            y_label: None,
            y_value: None,
            objective_value: candidate.objective_breakdown.weighted_objective,
            admissible: candidate.admissible,
        }
    }));

    CalibrationDiagnosticSearchSurfaceManifestArtifact {
        schema_version: "calibration.diagnostics.search_surface_manifest.v1",
        surfaces,
    }
}

fn build_observed_target_entry(
    input: ObservedTargetEntryBuildInput<'_>,
) -> CalibrationDiagnosticObservedTargetEntry {
    let ObservedTargetEntryBuildInput {
        target_id,
        target_family,
        definition,
        estimation_method,
        effective_sample_size,
        uncertainty_scale,
        evaluation_eligibility,
        admissibility_role,
        ranking_role,
        units,
        observed_value,
    } = input;

    CalibrationDiagnosticObservedTargetEntry {
        target_id: target_id.to_string(),
        target_family: target_family.to_string(),
        definition: definition.to_string(),
        estimation_method: estimation_method.to_string(),
        effective_sample_size,
        uncertainty_scale,
        stability_score: 1.0,
        evaluation_eligibility,
        admissibility_role,
        ranking_role,
        horizon_label: None,
        conditioning_label: None,
        units: units.to_string(),
        observed_value,
    }
}

struct ObservedTargetEntryBuildInput<'a> {
    target_id: &'a str,
    target_family: &'a str,
    definition: &'a str,
    estimation_method: &'a str,
    effective_sample_size: f64,
    uncertainty_scale: f64,
    evaluation_eligibility: CalibrationDiagnosticMetricEligibility,
    admissibility_role: CalibrationDiagnosticMetricRole,
    ranking_role: CalibrationDiagnosticMetricRole,
    units: &'a str,
    observed_value: f64,
}

fn classify_metric_family(metric_id: &str) -> &'static str {
    if metric_id.contains("activity") {
        "activity_level"
    } else if metric_id.contains("cluster") || metric_id.contains("gap") {
        "activity_clustering"
    } else if metric_id.contains("volatility") || metric_id.contains("scale") {
        "return_scale"
    } else if metric_id.contains("skew") || metric_id.contains("asymmetry") {
        "return_asymmetry"
    } else if metric_id.contains("tail") || metric_id.contains("kurtosis") {
        "return_tails"
    } else if metric_id.contains("jump") || metric_id.contains("news") {
        "event_conditioned_return_behavior"
    } else if metric_id.contains("coupling") {
        "activity_vs_volatility_coupling"
    } else {
        "diagnostic"
    }
}
