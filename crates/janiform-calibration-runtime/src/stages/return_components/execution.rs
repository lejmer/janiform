use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, Utc};
use janiform_foundations::numeric::arithmetic_mean_or_zero as arithmetic_mean;

use crate::calendar::{ExchangeCalendarRegistry, MarketState};
use crate::optimizer_adapter::{
    CalibrationOptimizerAdapterCandidateInput, CalibrationOptimizerAdapterConfiguration,
    CalibrationOptimizerAdapterError, CalibrationOptimizerCandidateParameterValue,
    CalibrationOptimizerParameterBound, run_constrained_optimizer_adapter,
};
use crate::stages::seasonality::CalibrationStageExecutionStatus;
use janiform_calibration_artifacts::{
    CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_RETURN_COMPONENTS_STAGE_HANDOFF_POLICY,
    CALIBRATION_RETURN_COMPONENTS_STAGE_OBJECTIVE_WEIGHTING_POLICY,
    CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_STAGE_SEED_DERIVATION_POLICY, CalibrationExecutionMode,
    CalibrationOptimizerSettings, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, ReturnComponentsStageFittedParameters,
    SurpriseNewsDeterministicIntensityProfile, resolve_calibration_stage_seed_metadata,
    stage_solver_settings_for_stage,
};
use janiform_model_kernels::news::{
    CoupledGapMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration, NewsProcessConfiguration, SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

use super::activity_coupling_fit;
use super::constants::*;
use super::continuous_fit;
use super::continuous_layer_fit;
use super::contracts::*;
use super::diagnostic_policy_fit;
use super::known_arrival_fit;
use super::mark_layer_fit;
use super::objective_scoring_policy_fit;
use super::price_range_policy_fit;
use super::quantile_binning_fit;
use super::signal_partition_fit;
use super::surprise_episode_selection_fit;
use super::surprise_intensity_fit;
use crate::shock_mark_fitting::fit_shared_news_mark_law_from_shock_samples;

struct ReturnComponentsStageCandidateGenerationOutput {
    candidate_inputs: Vec<ReturnComponentsStageCandidateInput>,
    candidate_family_counts: ReturnComponentsStageCandidateFamilyCounts,
    fitted_surprise_arrival_layer: surprise_intensity_fit::FittedSurpriseNewsArrivalLayer,
}

/// Execute the return-components stage with NIG and coupled-gap parameter constraints.
pub fn run_return_components_calibration_stage(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: ReturnComponentsStageExecutionInput,
) -> ReturnComponentsStageExecutionReport {
    if let Err(source) = run_spec.validate() {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::InvalidRunSpec {
                error_message: format!("{source} (classification={:?})", source.classification()),
            },
        );
    }

    if !run_spec
        .selected_stages
        .iter()
        .copied()
        .any(|selected_stage| selected_stage == CalibrationStage::ReturnComponents)
    {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::StageIsNotSelectedInRunSpec,
        );
    }

    let Some(stage_solver_settings) =
        stage_solver_settings_for_stage(run_spec, CalibrationStage::ReturnComponents)
    else {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::MissingStageSolverSettings,
        );
    };

    if let Err(source) = stage_input.candidate_generation_input.validate() {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: source.to_string(),
            },
        );
    }

    let prepared_candidate_generation_input = match prepare_candidate_generation_input(
        &run_spec.data_inputs.exchange_code,
        calendar_registry,
        stage_input.candidate_generation_input.clone(),
    ) {
        Ok(prepared_candidate_generation_input) => prepared_candidate_generation_input,
        Err(source) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::InvalidCandidateGenerationInput {
                    error_message: source.to_string(),
                },
            );
        }
    };

    let expected_activity_handoff = run_spec.stage_handoffs.iter().find(|stage_handoff| {
        stage_handoff.from_stage == CalibrationStage::Hawkes
            && stage_handoff.to_stage == CalibrationStage::ReturnComponents
    });
    let expected_transport = expected_activity_handoff
        .map(|stage_handoff| stage_handoff.transport())
        .unwrap_or(match run_spec.execution_mode {
            CalibrationExecutionMode::Memory => CalibrationStageHandoffTransport::InMemoryOutput,
            CalibrationExecutionMode::Disk => {
                CalibrationStageHandoffTransport::PersistedArtifactReference
            }
        });
    let provided_transport = stage_input.activity_input.transport();

    if provided_transport != expected_transport {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::ActivityInputTransportMismatch {
                expected_transport,
                provided_transport,
            },
        );
    }

    if provided_transport == CalibrationStageHandoffTransport::PersistedArtifactReference {
        let Some(provided_artifact_reference) = stage_input
            .activity_input
            .artifact_reference()
            .map(ToOwned::to_owned)
        else {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::MissingArtifactReferenceInActivityInput,
            );
        };

        if provided_artifact_reference.trim().is_empty() {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::InvalidArtifactReferenceInActivityInput {
                    artifact_reference: provided_artifact_reference,
                },
            );
        }

        if let Some(expected_stage_handoff) = expected_activity_handoff
            && let Some(expected_artifact_reference) = expected_stage_handoff.artifact_reference()
            && expected_artifact_reference != provided_artifact_reference
        {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::ActivityArtifactReferenceMismatch {
                    expected_artifact_reference: expected_artifact_reference.to_string(),
                    provided_artifact_reference,
                },
            );
        }
    }

    let resolved_stage_seed =
        resolve_calibration_stage_seed_metadata(run_spec, CalibrationStage::ReturnComponents);
    let stage_seed = resolved_stage_seed.stage_seed;
    let candidate_generation_seed = stage_seed;

    let generated_candidates = match generate_return_components_stage_candidate_inputs(
        &prepared_candidate_generation_input.candidate_generation_input,
        prepared_candidate_generation_input
            .derived_close_open_gap_candidates
            .as_slice(),
        prepared_candidate_generation_input
            .close_open_gap_detection_filter
            .as_str(),
        resolve_activity_calibration_context(&stage_input.activity_input),
        candidate_generation_seed,
        &stage_solver_settings,
    ) {
        Ok(output) => output,

        Err(source) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::CandidateGenerationFailed {
                    error_message: source.to_string(),
                },
            );
        }
    };

    if generated_candidates.candidate_inputs.is_empty() {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::CandidateGenerationFailed {
                error_message: "candidate generation completed but produced zero candidate_inputs"
                    .to_string(),
            },
        );
    }

    let optimizer_adapter_configuration = match CalibrationOptimizerAdapterConfiguration::try_new(
        "candidate_enumeration.v1",
        stage_seed,
        stage_solver_settings.clone(),
        return_components_stage_optimizer_bounds(),
    ) {
        Ok(configuration) => configuration,

        Err(source) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
                    candidate_id: "optimizer_adapter_configuration".to_string(),
                    error_message: source.to_string(),
                },
            );
        }
    };

    let mut seen_candidate_ids = BTreeSet::new();
    let mut external_constraint_diagnostics =
        Vec::<Vec<ReturnComponentsStageConstraintViolationDiagnostic>>::with_capacity(
            generated_candidates.candidate_inputs.len(),
        );
    let mut adapter_candidate_inputs =
        Vec::with_capacity(generated_candidates.candidate_inputs.len());
    for candidate_input in &generated_candidates.candidate_inputs {
        if !seen_candidate_ids.insert(candidate_input.candidate_id.clone()) {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::DuplicateCandidateId {
                    candidate_id: candidate_input.candidate_id.clone(),
                },
            );
        }

        if let Err(source) = candidate_input.validate() {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::InvalidCandidateInput {
                    candidate_id: candidate_input.candidate_id.clone(),
                    error_message: source.to_string(),
                },
            );
        }

        let constraint_violations =
            collect_return_components_stage_constraint_violations(candidate_input);
        let external_constraint_violation_count = constraint_violations.len();
        external_constraint_diagnostics.push(constraint_violations);

        adapter_candidate_inputs.push(CalibrationOptimizerAdapterCandidateInput {
            candidate_id: candidate_input.candidate_id.clone(),
            objective_terms: candidate_input.objective_terms.clone(),
            parameter_values: vec![CalibrationOptimizerCandidateParameterValue {
                parameter_name: "nig_shared_plus_shift_configuration.delta_beta".to_string(),
                value: candidate_input
                    .nig_shared_plus_shift_configuration
                    .delta_beta,
            }],
            external_constraint_violation_count,
        });
    }

    let optimizer_adapter_output = match run_constrained_optimizer_adapter(
        &optimizer_adapter_configuration,
        &adapter_candidate_inputs,
    ) {
        Ok(output) => output,

        Err(CalibrationOptimizerAdapterError::InvalidCandidateInput {
            candidate_id,
            source,
        }) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::InvalidCandidateInput {
                    candidate_id,
                    error_message: source.to_string(),
                },
            );
        }

        Err(CalibrationOptimizerAdapterError::InvalidObjectiveComposition {
            candidate_id,
            source,
        }) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::InvalidCandidateInput {
                    candidate_id,
                    error_message: source.to_string(),
                },
            );
        }

        Err(CalibrationOptimizerAdapterError::DuplicateCandidateId { candidate_id }) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::DuplicateCandidateId { candidate_id },
            );
        }

        Err(source) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
                    candidate_id: "optimizer_adapter".to_string(),
                    error_message: source.to_string(),
                },
            );
        }
    };

    let mut candidate_diagnostics: Vec<ReturnComponentsStageCandidateDiagnostic> =
        Vec::with_capacity(generated_candidates.candidate_inputs.len());
    for ((candidate_input, constraint_violations), optimizer_diagnostic) in generated_candidates
        .candidate_inputs
        .iter()
        .zip(external_constraint_diagnostics.into_iter())
        .zip(optimizer_adapter_output.candidate_diagnostics.iter())
    {
        let mut combined_constraint_violations = constraint_violations;
        for bound_violation in &optimizer_diagnostic.bound_violations {
            combined_constraint_violations.push(
                ReturnComponentsStageConstraintViolationDiagnostic {
                    field: bound_violation.parameter_name.clone(),
                    message: format!(
                        "value {} violated inclusive bounds [{}, {}] ({:?})",
                        bound_violation.value,
                        bound_violation.lower_inclusive,
                        bound_violation.upper_inclusive,
                        bound_violation.violation_kind
                    ),
                },
            );
        }

        candidate_diagnostics.push(ReturnComponentsStageCandidateDiagnostic {
            candidate_id: candidate_input.candidate_id.clone(),
            raw_objective_value: optimizer_diagnostic.raw_objective_value,
            constraint_penalty: optimizer_diagnostic.constraint_penalty,
            penalized_objective_value: optimizer_diagnostic.penalized_objective_value,
            constraint_violation_count: optimizer_diagnostic.total_constraint_violation_count,
            constraint_violations: combined_constraint_violations,
            objective_metric_buckets: candidate_input.objective_metric_buckets,
            objective_terms: optimizer_diagnostic
                .objective_composition
                .objective_terms
                .clone(),
            candidate_families: candidate_input.candidate_families.clone(),
            candidate_provenance: candidate_input.candidate_provenance.clone(),
            return_distribution_diagnostic: candidate_input.return_distribution_diagnostic,
            surprise_arrival_diagnostic: candidate_input.surprise_arrival_diagnostic.clone(),
            jump_diagnostic: candidate_input.jump_diagnostic.clone(),
            gap_diagnostic: candidate_input.gap_diagnostic.clone(),
            activity_coupling_diagnostic: candidate_input.activity_coupling_diagnostic.clone(),
            price_range_sanity_diagnostic: candidate_input.price_range_sanity_diagnostic.clone(),
            dominant_scale_driver: candidate_input.dominant_scale_driver.clone(),
        });
    }

    let Some(selected_candidate_id) = optimizer_adapter_output.selected_candidate_id.as_ref()
    else {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::ConstraintViolationsForAllCandidates {
                candidate_diagnostics,
            },
        );
    };

    let Some(selected_candidate_position) = generated_candidates
        .candidate_inputs
        .iter()
        .position(|candidate| candidate.candidate_id == *selected_candidate_id)
    else {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: "optimizer_adapter_selection".to_string(),
                error_message: "selected candidate id was not found in generated candidate_inputs"
                    .to_string(),
            },
        );
    };

    let selected_candidate_input =
        &generated_candidates.candidate_inputs[selected_candidate_position];
    let selected_candidate_diagnostic = &candidate_diagnostics[selected_candidate_position];
    let nig_configuration = match NigSharedPlusShiftConfiguration::try_new(
        selected_candidate_input
            .nig_shared_plus_shift_configuration
            .alpha,
        selected_candidate_input
            .nig_shared_plus_shift_configuration
            .beta_0,
        selected_candidate_input
            .nig_shared_plus_shift_configuration
            .delta,
        selected_candidate_input
            .nig_shared_plus_shift_configuration
            .mu,
        selected_candidate_input
            .nig_shared_plus_shift_configuration
            .delta_beta,
    ) {
        Ok(configuration) => configuration,
        Err(source) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
                    candidate_id: selected_candidate_input.candidate_id.clone(),
                    error_message: source.to_string(),
                },
            );
        }
    };

    if !nig_configuration.delta_beta.is_finite() || nig_configuration.delta_beta >= 0.0 {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: selected_candidate_input.candidate_id.clone(),
                error_message:
                    "nig_shared_plus_shift_configuration.delta_beta must be finite and strictly negative for return skew asymmetry".to_string(),
            },
        );
    }

    let news_process_configuration = match NewsProcessConfiguration::try_new(
        selected_candidate_input
            .news_process_configuration
            .arrivals
            .clone(),
        selected_candidate_input
            .news_process_configuration
            .shared_mark_law,
        selected_candidate_input
            .news_process_configuration
            .known_component_mark_scales,
        selected_candidate_input
            .news_process_configuration
            .surprise_component_mark_scales,
    ) {
        Ok(configuration) => configuration,
        Err(source) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
                    candidate_id: selected_candidate_input.candidate_id.clone(),
                    error_message: source.to_string(),
                },
            );
        }
    };

    let valid_candidate_count = optimizer_adapter_output
        .candidate_diagnostics
        .iter()
        .filter(|candidate_diagnostic| candidate_diagnostic.total_constraint_violation_count == 0)
        .count();

    let selected_candidate_raw_objective = selected_candidate_diagnostic.raw_objective_value;
    let selected_candidate_penalized_objective =
        selected_candidate_diagnostic.penalized_objective_value;

    let fitted_stage_payload = ReturnComponentsStageFittedPayload {
        schema_version: CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
        selected_candidate_id: selected_candidate_input.candidate_id.clone(),
        fitted_parameters: ReturnComponentsStageFittedParameters {
            nig_shared_plus_shift_configuration: nig_configuration,
            news_process_configuration,
            surprise_deterministic_intensity_profile: SurpriseNewsDeterministicIntensityProfile {
                weekly_hour_utc_multipliers: selected_candidate_input
                    .candidate_families
                    .surprise_news_arrival_controls
                    .weekly_hour_utc_multipliers
                    .clone(),
                profile_resolution_hours: selected_candidate_input
                    .candidate_families
                    .surprise_news_arrival_controls
                    .profile_resolution_hours,
                fit_policy: generated_candidates
                    .fitted_surprise_arrival_layer
                    .deterministic_intensity_profile
                    .fit_policy
                    .clone(),
                normalization_policy: generated_candidates
                    .fitted_surprise_arrival_layer
                    .deterministic_intensity_profile
                    .normalization_policy
                    .clone(),
            },
        },
        candidate_diagnostics,
        candidate_generation_policy: RETURN_COMPONENTS_STAGE_CANDIDATE_GENERATION_POLICY,
        objective_weighting_policy: CALIBRATION_RETURN_COMPONENTS_STAGE_OBJECTIVE_WEIGHTING_POLICY,
        objective_metric_bucket_policy: RETURN_COMPONENTS_STAGE_OBJECTIVE_METRIC_BUCKET_POLICY,
        candidate_family_counts: generated_candidates.candidate_family_counts.clone(),
        activity_input_transport: stage_input.activity_input.transport(),
        activity_selected_hawkes_candidate_id: match stage_input.activity_input.transport() {
            CalibrationStageHandoffTransport::InMemoryOutput => stage_input
                .activity_input
                .selected_hawkes_candidate_id()
                .map(ToOwned::to_owned),
            CalibrationStageHandoffTransport::PersistedArtifactReference => None,
        },
        activity_artifact_reference: stage_input
            .activity_input
            .artifact_reference()
            .map(ToOwned::to_owned),
    };

    ReturnComponentsStageExecutionReport {
        schema_version: CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::ReturnComponents,
        status: CalibrationStageExecutionStatus::Success,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_RETURN_COMPONENTS_STAGE_HANDOFF_POLICY,
        handoffs: build_return_components_stage_handoff_outputs(run_spec),
        fitted_stage_payload: Some(fitted_stage_payload),
        metadata: Some(ReturnComponentsStageExecutionMetadata {
            stage_output_schema_version: CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
            run_global_seed: resolved_stage_seed.run_global_seed,
            stage_seed,
            stage_seed_source: resolved_stage_seed.stage_seed_source,
            stage_seed_derivation_policy: CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
            candidate_generation_seed,
            solver_settings: stage_solver_settings,
            optimizer_adapter_metadata: optimizer_adapter_output.metadata.clone(),
            input_candidate_count: generated_candidates.candidate_inputs.len(),
            valid_candidate_count,
            selected_candidate_raw_objective,
            selected_candidate_penalized_objective,
            objective_weighting_policy:
                CALIBRATION_RETURN_COMPONENTS_STAGE_OBJECTIVE_WEIGHTING_POLICY,
            objective_metric_bucket_policy: RETURN_COMPONENTS_STAGE_OBJECTIVE_METRIC_BUCKET_POLICY,
            candidate_generation_policy: RETURN_COMPONENTS_STAGE_CANDIDATE_GENERATION_POLICY,
            candidate_family_counts: generated_candidates.candidate_family_counts,
        }),
        failure: None,
    }
}

pub fn evaluate_return_components_exact_candidate(
    candidate_generation_input: ReturnComponentsStageCandidateGenerationInput,
    activity_calibration_context: Option<ReturnComponentsStageActivityCalibrationContextSnapshot>,
    observed_summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    fitted_parameters: ReturnComponentsStageFittedParameters,
    stage_seed: u64,
) -> Result<
    ReturnComponentsStageExactCandidateEvaluation,
    ReturnComponentsStageExactCandidateEvaluationError,
> {
    if let Err(source) = candidate_generation_input.validate() {
        return Err(
            ReturnComponentsStageExactCandidateEvaluationError::InvalidCandidateGenerationInput {
                error_message: source.to_string(),
            },
        );
    }

    if let Err(source) = NigSharedPlusShiftConfiguration::try_new(
        fitted_parameters.nig_shared_plus_shift_configuration.alpha,
        fitted_parameters.nig_shared_plus_shift_configuration.beta_0,
        fitted_parameters.nig_shared_plus_shift_configuration.delta,
        fitted_parameters.nig_shared_plus_shift_configuration.mu,
        fitted_parameters
            .nig_shared_plus_shift_configuration
            .delta_beta,
    ) {
        return Err(
            ReturnComponentsStageExactCandidateEvaluationError::InvalidFittedParameters {
                error_message: source.to_string(),
            },
        );
    }

    if let Err(source) = fitted_parameters.news_process_configuration.validate() {
        return Err(
            ReturnComponentsStageExactCandidateEvaluationError::InvalidFittedParameters {
                error_message: source.to_string(),
            },
        );
    }

    let activity_calibration_context = activity_calibration_context.map(|context| {
        ReturnComponentsStageActivityCalibrationContext {
            modeled_open_event_rate_per_hour: context.modeled_open_event_rate_per_hour,
            representative_interarrival_hours: context.representative_interarrival_hours,
        }
    });
    let fitted_signal_partition_layer = signal_partition_fit::fit_signal_partition_layer(
        candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
    );
    let fitted_surprise_episode_selection_layer =
        surprise_episode_selection_fit::fit_surprise_episode_selection_layer(
            candidate_generation_input
                .detected_intraday_jump_candidates
                .as_slice(),
            fitted_signal_partition_layer.continuous_return_filter_threshold,
        );
    let sorted_signals = sort_candidate_generation_signals(
        &candidate_generation_input,
        &[],
        fitted_signal_partition_layer,
        &fitted_surprise_episode_selection_layer,
    );
    let continuous_fit_business_time_scales =
        derive_continuous_fit_business_time_scales(&sorted_signals, activity_calibration_context);
    let continuous_return_calibration_sample =
        continuous_fit::build_continuous_return_calibration_sample(
            &sorted_signals,
            continuous_fit_business_time_scales.as_slice(),
            candidate_generation_input.open_session_sell_share_proxy,
        );

    let continuous_return_evaluation = continuous_fit::evaluate_continuous_nig_candidate(
        fitted_parameters.nig_shared_plus_shift_configuration,
        &continuous_return_calibration_sample,
        observed_summary_statistics,
        stage_seed,
    );

    let surprise_arrival_family = ReturnComponentsSurpriseNewsArrivalCandidateFamily {
        baseline_intensity: fitted_parameters
            .news_process_configuration
            .arrivals
            .surprise
            .baseline_intensity,
        weekly_hour_utc_multipliers: fitted_parameters
            .surprise_deterministic_intensity_profile
            .weekly_hour_utc_multipliers
            .clone(),
        profile_resolution_hours: fitted_parameters
            .surprise_deterministic_intensity_profile
            .profile_resolution_hours,
    };
    let return_jump_family = ReturnComponentsReturnJumpMarkScaleCandidateFamily {
        sigma_g: fitted_parameters
            .news_process_configuration
            .shared_mark_law
            .coupled_gap_marks
            .sigma_g,
    };
    let activity_coupling_family = ReturnComponentsActivityCouplingCandidateFamily {
        known_activity_coupling_scale: fitted_parameters
            .news_process_configuration
            .known_component_mark_scales
            .activity_coupling_scale,
        surprise_activity_coupling_scale: fitted_parameters
            .news_process_configuration
            .surprise_component_mark_scales
            .activity_coupling_scale,
    };
    let fitted_diagnostic_policy = diagnostic_policy_fit::fit_diagnostic_policy(
        RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT,
    );
    let fitted_objective_scoring_policy =
        objective_scoring_policy_fit::fit_objective_scoring_policy(
            RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER,
        );
    let jump_diagnostic = build_jump_diagnostic(
        &fitted_diagnostic_policy,
        observed_summary_statistics,
        &return_jump_family,
        candidate_generation_input.shared_mark_law_template,
    );
    let surprise_arrival_diagnostic = build_surprise_arrival_diagnostic(
        &fitted_diagnostic_policy,
        observed_summary_statistics,
        &surprise_arrival_family,
        &fitted_parameters.surprise_deterministic_intensity_profile,
    );
    let gap_diagnostic = build_gap_diagnostic(
        &fitted_diagnostic_policy,
        observed_summary_statistics,
        &return_jump_family,
        candidate_generation_input.shared_mark_law_template,
    );
    let activity_coupling_diagnostic = build_activity_coupling_diagnostic(
        &fitted_diagnostic_policy,
        observed_summary_statistics,
        &activity_coupling_family,
    );
    let fitted_price_range_sanity_policy = price_range_policy_fit::fit_price_range_sanity_policy();
    let price_range_sanity_diagnostic = build_price_range_sanity_diagnostic(
        &fitted_diagnostic_policy,
        observed_summary_statistics,
        continuous_return_evaluation.diagnostic,
        &surprise_arrival_family,
        &return_jump_family,
        candidate_generation_input.shared_mark_law_template,
        &fitted_price_range_sanity_policy,
    );
    let objective_metric_buckets = objective_scoring_policy_fit::score_candidate_metric_buckets(
        &fitted_objective_scoring_policy,
        continuous_return_evaluation.objective_score,
        &surprise_arrival_diagnostic,
        &jump_diagnostic,
        &gap_diagnostic,
        &activity_coupling_diagnostic,
        &price_range_sanity_diagnostic,
        fitted_diagnostic_policy.shock_evidence_min_count,
    );
    let dominant_scale_driver =
        objective_scoring_policy_fit::dominant_scale_driver_from_metric_buckets(
            &fitted_objective_scoring_policy,
            objective_metric_buckets,
        );
    let raw_objective_value = objective_scoring_policy_fit::weighted_objective_value(
        candidate_generation_input.objective_metric_weights,
        objective_metric_buckets,
    );
    let constraint_violations =
        collect_return_components_exact_candidate_constraint_violations(&fitted_parameters);

    Ok(ReturnComponentsStageExactCandidateEvaluation {
        raw_objective_value,
        objective_metric_buckets,
        return_distribution_diagnostic: continuous_return_evaluation.diagnostic,
        continuous_return_sample_diagnostic:
            ReturnComponentsStageContinuousReturnSampleDiagnostic {
                observed_returns: continuous_return_calibration_sample
                    .observed_returns
                    .clone(),
                fitted_returns: continuous_return_evaluation.fitted_returns.clone(),
                business_time_scales: continuous_return_calibration_sample
                    .business_time_scales
                    .clone(),
            },
        continuous_distribution_fit_breakdown: continuous_return_evaluation.fit_breakdown,
        surprise_arrival_diagnostic,
        jump_diagnostic,
        gap_diagnostic,
        activity_coupling_diagnostic,
        price_range_sanity_diagnostic,
        constraint_violation_count: constraint_violations.len(),
        constraint_violations,
        dominant_scale_driver,
    })
}

fn generate_return_components_stage_candidate_inputs(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    derived_close_open_gap_candidates: &[ReturnComponentsDetectedCloseOpenGapCandidate],
    close_open_gap_detection_filter: &str,
    activity_calibration_context: Option<ReturnComponentsStageActivityCalibrationContext>,
    stage_seed: u64,
    stage_solver_settings: &CalibrationOptimizerSettings,
) -> Result<
    ReturnComponentsStageCandidateGenerationOutput,
    ReturnComponentsStageCandidateGenerationInputError,
> {
    let fitted_signal_partition_layer = signal_partition_fit::fit_signal_partition_layer(
        candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
    );
    let fitted_surprise_episode_selection_layer =
        surprise_episode_selection_fit::fit_surprise_episode_selection_layer(
            candidate_generation_input
                .detected_intraday_jump_candidates
                .as_slice(),
            fitted_signal_partition_layer.continuous_return_filter_threshold,
        );

    candidate_generation_input.validate()?;

    let sorted_signals = sort_candidate_generation_signals(
        candidate_generation_input,
        derived_close_open_gap_candidates,
        fitted_signal_partition_layer,
        &fitted_surprise_episode_selection_layer,
    );
    let (summary_statistics, continuous_return_calibration_sample) =
        summarize_candidate_generation_signals(
            candidate_generation_input,
            &sorted_signals,
            activity_calibration_context,
        );

    let fitted_quantile_binning_policy = quantile_binning_fit::fit_quantile_binning_policy(
        sorted_signals.surprise_open_jump_magnitudes.as_slice(),
        sorted_signals.combined_gap_magnitudes.as_slice(),
    );
    let fitted_surprise_arrival_layer = surprise_intensity_fit::fit_surprise_news_arrival_layer(
        sorted_signals.source_window.window_start_utc_nanos,
        sorted_signals.source_window.window_end_utc_nanos,
        &sorted_signals
            .selected_surprise_jump_episodes
            .iter()
            .map(|episode| episode.terminal_timestamp_utc_nanos)
            .collect::<Vec<_>>(),
    );
    let fitted_return_jump_mark_layer = mark_layer_fit::fit_return_jump_mark_layer(
        candidate_generation_input.shared_mark_law_template,
        summary_statistics,
    );
    let fitted_activity_coupling_layer =
        activity_coupling_fit::fit_activity_coupling_layer(summary_statistics);
    let fitted_continuous_return_layer = continuous_layer_fit::fit_continuous_return_layer(
        summary_statistics,
        &continuous_return_calibration_sample,
        stage_seed,
        stage_solver_settings,
    );
    let fitted_diagnostic_policy = diagnostic_policy_fit::fit_diagnostic_policy(
        RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT,
    );
    let fitted_objective_scoring_policy =
        objective_scoring_policy_fit::fit_objective_scoring_policy(
            RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER,
        );
    let fitted_price_range_sanity_policy = price_range_policy_fit::fit_price_range_sanity_policy();

    let candidate_family_sets = build_candidate_family_sets(
        candidate_generation_input,
        fitted_continuous_return_layer,
        &fitted_surprise_arrival_layer,
        &fitted_return_jump_mark_layer,
        &fitted_activity_coupling_layer,
    );

    let candidate_provenance_template = build_candidate_provenance_template(
        sorted_signals.source_window,
        stage_seed,
        summary_statistics,
        close_open_gap_detection_filter,
        fitted_signal_partition_layer,
        fitted_surprise_episode_selection_layer.policy.clone(),
        fitted_quantile_binning_policy,
        fitted_diagnostic_policy.clone(),
        fitted_objective_scoring_policy.clone(),
        fitted_price_range_sanity_policy.clone(),
    );
    let candidate_family_counts = build_candidate_family_counts();
    let generated_candidates = enumerate_candidate_inputs(
        candidate_generation_input,
        &candidate_family_sets,
        summary_statistics,
        &continuous_return_calibration_sample,
        &fitted_surprise_arrival_layer.deterministic_intensity_profile,
        stage_seed,
        &candidate_provenance_template,
        &fitted_diagnostic_policy,
        &fitted_objective_scoring_policy,
        &fitted_price_range_sanity_policy,
    )?;

    Ok(ReturnComponentsStageCandidateGenerationOutput {
        candidate_inputs: generated_candidates,
        candidate_family_counts,
        fitted_surprise_arrival_layer,
    })
}

fn resolve_activity_calibration_context(
    activity_input: &ReturnComponentsStageActivityInput,
) -> Option<ReturnComponentsStageActivityCalibrationContext> {
    let hawkes_payload = activity_input.hawkes_payload()?;
    let selected_candidate = hawkes_payload
        .candidate_diagnostics
        .iter()
        .find(|candidate| candidate.candidate_id == hawkes_payload.selected_candidate_id)?;
    let modeled_open_event_rate_per_hour = selected_candidate
        .implied_metrics
        .model_implied_open_event_rate_per_hour;
    let representative_interarrival_hours = selected_candidate
        .source_regime
        .open_session_mean_interarrival_seconds
        / 3_600.0;

    if !modeled_open_event_rate_per_hour.is_finite()
        || modeled_open_event_rate_per_hour <= 0.0
        || !representative_interarrival_hours.is_finite()
        || representative_interarrival_hours <= 0.0
    {
        return None;
    }

    Some(ReturnComponentsStageActivityCalibrationContext {
        modeled_open_event_rate_per_hour,
        representative_interarrival_hours,
    })
}

#[derive(Debug, Clone)]
pub(super) struct ReturnComponentsStageSortedSignalInputs {
    pub(super) known_return_samples: Vec<ReturnComponentsRealizedKnownNewsReturnSample>,
    pub(super) observed_open_session_returns: Vec<ReturnComponentsObservedOpenSessionReturn>,
    pub(super) continuous_fit_returns: Vec<ReturnComponentsObservedOpenSessionReturn>,
    pub(super) intraday_jump_candidates: Vec<ReturnComponentsDetectedIntradayJumpCandidate>,
    pub(super) surprise_jump_episodes: Vec<ReturnComponentsSurpriseJumpEpisode>,
    pub(super) selected_surprise_jump_episodes: Vec<ReturnComponentsSurpriseJumpEpisode>,
    pub(super) close_open_gap_candidates: Vec<ReturnComponentsDetectedCloseOpenGapCandidate>,
    pub(super) surprise_open_jump_magnitudes: Vec<f64>,
    pub(super) combined_gap_magnitudes: Vec<f64>,
    pub(super) source_window: ReturnComponentsStageSignalSourceWindow,
}

#[derive(Debug, Clone)]
struct ReturnComponentsStagePreparedCandidateGenerationInput {
    candidate_generation_input: ReturnComponentsStageCandidateGenerationInput,
    derived_close_open_gap_candidates: Vec<ReturnComponentsDetectedCloseOpenGapCandidate>,
    close_open_gap_detection_filter: String,
}

#[derive(Debug, Clone, Copy)]
struct ReturnComponentsStageActivityCalibrationContext {
    modeled_open_event_rate_per_hour: f64,
    representative_interarrival_hours: f64,
}

#[derive(Debug, Clone)]
struct ReturnComponentsStageCandidateFamilySets {
    fitted_continuous_return_layer: continuous_layer_fit::FittedContinuousReturnLayer,
    fitted_known_arrival_layer: known_arrival_fit::FittedKnownNewsArrivalLayer,
    fitted_surprise_arrival_layer: ReturnComponentsSurpriseNewsArrivalCandidateFamily,
    fitted_return_jump_mark_layer: ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    fitted_activity_coupling_layer: ReturnComponentsActivityCouplingCandidateFamily,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ReturnComponentsStageReturnDistributionSummary {
    mean_return: f64,
    volatility: f64,
    skewness: f64,
    excess_kurtosis: f64,
}

fn sort_candidate_generation_signals(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    derived_close_open_gap_candidates: &[ReturnComponentsDetectedCloseOpenGapCandidate],
    fitted_signal_partition_layer: signal_partition_fit::FittedSignalPartitionLayer,
    fitted_surprise_episode_selection_layer: &surprise_episode_selection_fit::FittedSurpriseEpisodeSelectionLayer,
) -> ReturnComponentsStageSortedSignalInputs {
    let mut known_return_samples = candidate_generation_input
        .realized_known_news_return_samples
        .clone();
    known_return_samples.sort_by_key(|sample| sample.event_timestamp_utc_nanos);

    let mut observed_open_session_returns = candidate_generation_input
        .observed_open_session_returns
        .clone();
    observed_open_session_returns.sort_by_key(|observation| observation.return_timestamp_utc_nanos);

    let mut intraday_jump_candidates = candidate_generation_input
        .detected_intraday_jump_candidates
        .clone();
    intraday_jump_candidates.sort_by_key(|candidate| candidate.jump_timestamp_utc_nanos);

    let mut close_open_gap_candidates = derived_close_open_gap_candidates.to_vec();
    close_open_gap_candidates.sort_by_key(|candidate| {
        (
            candidate.close_timestamp_utc_nanos,
            candidate.open_timestamp_utc_nanos,
        )
    });

    let combined_gap_magnitudes = {
        let mut magnitudes =
            Vec::with_capacity(known_return_samples.len() + close_open_gap_candidates.len());
        magnitudes.extend(
            known_return_samples
                .iter()
                .map(|sample| sample.closed_window_gap_return.abs()),
        );
        magnitudes.extend(
            close_open_gap_candidates
                .iter()
                .map(|candidate| candidate.gap_return.abs()),
        );
        magnitudes
    };

    let surprise_jump_episodes = fitted_surprise_episode_selection_layer
        .surprise_jump_episodes
        .clone();
    let selected_surprise_jump_episodes = fitted_surprise_episode_selection_layer
        .selected_surprise_jump_episodes
        .clone();
    let exogenous_jump_timestamps: BTreeSet<i64> = selected_surprise_jump_episodes
        .iter()
        .flat_map(|episode| episode.member_jump_timestamps_utc_nanos.iter().copied())
        .collect();

    let mut continuous_fit_returns: Vec<ReturnComponentsObservedOpenSessionReturn> =
        observed_open_session_returns
            .iter()
            .copied()
            .filter(|observation| {
                !is_within_known_news_exclusion_window(
                    observation.return_timestamp_utc_nanos,
                    candidate_generation_input
                        .known_news_calendar
                        .scheduled_event_timestamps_utc_nanos
                        .as_slice(),
                    fitted_signal_partition_layer.known_news_exclusion_window_minutes,
                ) && !exogenous_jump_timestamps.contains(&observation.return_timestamp_utc_nanos)
                    && observation.log_return.abs()
                        <= fitted_signal_partition_layer.continuous_return_filter_threshold
            })
            .collect();
    if continuous_fit_returns.is_empty() {
        continuous_fit_returns = observed_open_session_returns
            .iter()
            .copied()
            .filter(|observation| {
                observation.log_return.abs()
                    <= fitted_signal_partition_layer.continuous_return_filter_threshold
            })
            .collect();
    }
    if continuous_fit_returns.is_empty() {
        continuous_fit_returns = observed_open_session_returns.clone();
    }

    ReturnComponentsStageSortedSignalInputs {
        known_return_samples,
        observed_open_session_returns,
        continuous_fit_returns,
        intraday_jump_candidates,
        surprise_jump_episodes,
        selected_surprise_jump_episodes: selected_surprise_jump_episodes.clone(),
        close_open_gap_candidates,
        surprise_open_jump_magnitudes: selected_surprise_jump_episodes
            .iter()
            .map(|episode| episode.cumulative_jump_return.abs())
            .collect(),
        combined_gap_magnitudes,
        source_window: candidate_generation_input.known_news_calendar.source_window,
    }
}

fn activity_coupling_target_from_post_shock_lift(post_shock_activity_lift: f64) -> f64 {
    if !post_shock_activity_lift.is_finite() {
        return 0.0;
    }

    (post_shock_activity_lift - 1.0).max(0.0)
}

fn summarize_candidate_generation_signals(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    sorted_signals: &ReturnComponentsStageSortedSignalInputs,
    activity_calibration_context: Option<ReturnComponentsStageActivityCalibrationContext>,
) -> (
    ReturnComponentsStageObservedSignalSummaryStatistics,
    continuous_fit::ReturnComponentsStageContinuousReturnCalibrationSample,
) {
    let source_window_duration_hours = sorted_signals
        .source_window
        .duration_hours()
        .max(f64::EPSILON);
    let continuous_fit_business_time_scales =
        derive_continuous_fit_business_time_scales(sorted_signals, activity_calibration_context);
    let continuous_return_calibration_sample =
        continuous_fit::build_continuous_return_calibration_sample(
            sorted_signals,
            continuous_fit_business_time_scales.as_slice(),
            candidate_generation_input.open_session_sell_share_proxy,
        );
    let mean_continuous_business_time_scale =
        continuous_fit::mean_continuous_return_calibration_business_time_scale(
            &continuous_return_calibration_sample,
        )
        .max(1.0e-8);
    let continuous_distribution =
        continuous_fit::summarize_observed_continuous_return_calibration_sample(
            &continuous_return_calibration_sample,
        );
    let observed_log_price_span = compute_observed_log_price_span(
        sorted_signals
            .observed_open_session_returns
            .iter()
            .map(|observation| {
                (
                    observation.return_timestamp_utc_nanos,
                    observation.log_return,
                )
            })
            .chain(
                sorted_signals
                    .close_open_gap_candidates
                    .iter()
                    .map(|candidate| (candidate.open_timestamp_utc_nanos, candidate.gap_return)),
            )
            .collect::<Vec<_>>()
            .as_slice(),
    );
    let known_activity_coupling_target = arithmetic_mean(
        &sorted_signals
            .known_return_samples
            .iter()
            .map(|sample| {
                activity_coupling_target_from_post_shock_lift(sample.post_shock_activity_lift)
            })
            .collect::<Vec<_>>(),
    );
    let surprise_activity_coupling_target = arithmetic_mean(
        &sorted_signals
            .selected_surprise_jump_episodes
            .iter()
            .map(|episode| {
                activity_coupling_target_from_post_shock_lift(episode.post_shock_activity_lift)
            })
            .collect::<Vec<_>>(),
    );

    (
        ReturnComponentsStageObservedSignalSummaryStatistics {
            known_calendar_event_count: candidate_generation_input
                .known_news_calendar
                .scheduled_event_timestamps_utc_nanos
                .len(),
            known_realized_sample_count: sorted_signals.known_return_samples.len(),
            source_window_duration_hours,
            observed_open_session_return_count: sorted_signals.observed_open_session_returns.len(),
            continuous_fit_return_count: continuous_fit::continuous_return_calibration_sample_count(
                &continuous_return_calibration_sample,
            ),
            intraday_jump_candidate_count: sorted_signals.intraday_jump_candidates.len(),
            intraday_jump_episode_count: sorted_signals.surprise_jump_episodes.len(),
            selected_surprise_jump_episode_count: sorted_signals
                .selected_surprise_jump_episodes
                .len(),
            close_open_gap_candidate_count: sorted_signals.close_open_gap_candidates.len(),
            close_open_gap_episode_count: sorted_signals.close_open_gap_candidates.len(),
            open_session_sell_share_proxy: candidate_generation_input.open_session_sell_share_proxy,
            mean_continuous_business_time_scale,
            continuous_return_mean: continuous_distribution.mean_return,
            continuous_return_volatility: continuous_distribution.volatility,
            continuous_return_skewness: continuous_distribution.skewness,
            continuous_return_excess_kurtosis: continuous_distribution.excess_kurtosis,
            observed_log_price_span,
            mean_abs_known_open_jump_return: arithmetic_mean(
                sorted_signals
                    .known_return_samples
                    .iter()
                    .map(|sample| sample.open_window_jump_return.abs())
                    .collect::<Vec<_>>()
                    .as_slice(),
            ),
            mean_abs_known_closed_gap_return: arithmetic_mean(
                sorted_signals
                    .known_return_samples
                    .iter()
                    .map(|sample| sample.closed_window_gap_return.abs())
                    .collect::<Vec<_>>()
                    .as_slice(),
            ),
            mean_abs_surprise_open_jump_return: arithmetic_mean(
                &sorted_signals
                    .selected_surprise_jump_episodes
                    .iter()
                    .map(|episode| episode.cumulative_jump_return.abs())
                    .collect::<Vec<_>>(),
            ),
            mean_abs_surprise_closed_gap_return: arithmetic_mean(
                sorted_signals
                    .close_open_gap_candidates
                    .iter()
                    .map(|candidate| candidate.gap_return.abs())
                    .collect::<Vec<_>>()
                    .as_slice(),
            ),
            mean_known_post_shock_activity_lift: arithmetic_mean(
                sorted_signals
                    .known_return_samples
                    .iter()
                    .map(|sample| sample.post_shock_activity_lift)
                    .collect::<Vec<_>>()
                    .as_slice(),
            ),
            mean_surprise_post_shock_activity_lift: arithmetic_mean(
                &sorted_signals
                    .selected_surprise_jump_episodes
                    .iter()
                    .map(|episode| episode.post_shock_activity_lift)
                    .collect::<Vec<_>>(),
            ),
            known_activity_coupling_target,
            surprise_activity_coupling_target,
            known_news_coverage_ratio: sorted_signals.known_return_samples.len() as f64
                / candidate_generation_input
                    .known_news_calendar
                    .scheduled_event_timestamps_utc_nanos
                    .len()
                    .max(1) as f64,
            surprise_arrival_intensity_per_hour: sorted_signals
                .selected_surprise_jump_episodes
                .len() as f64
                / source_window_duration_hours,
        },
        continuous_return_calibration_sample,
    )
}

fn build_candidate_family_sets(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    fitted_continuous_return_layer: continuous_layer_fit::FittedContinuousReturnLayer,
    fitted_surprise_arrival_layer: &surprise_intensity_fit::FittedSurpriseNewsArrivalLayer,
    fitted_return_jump_mark_layer: &mark_layer_fit::FittedReturnJumpMarkLayer,
    fitted_activity_coupling_layer: &activity_coupling_fit::FittedActivityCouplingLayer,
) -> ReturnComponentsStageCandidateFamilySets {
    ReturnComponentsStageCandidateFamilySets {
        fitted_continuous_return_layer,
        fitted_known_arrival_layer: known_arrival_fit::fit_known_news_arrival_layer(
            &candidate_generation_input.known_news_calendar,
        ),
        fitted_surprise_arrival_layer: build_surprise_arrival_families(
            fitted_surprise_arrival_layer,
        ),
        fitted_return_jump_mark_layer: build_return_jump_mark_scale_families(
            fitted_return_jump_mark_layer,
        ),
        fitted_activity_coupling_layer: build_activity_coupling_families(
            fitted_activity_coupling_layer,
        ),
    }
}

fn build_surprise_arrival_families(
    fitted_surprise_arrival_layer: &surprise_intensity_fit::FittedSurpriseNewsArrivalLayer,
) -> ReturnComponentsSurpriseNewsArrivalCandidateFamily {
    ReturnComponentsSurpriseNewsArrivalCandidateFamily {
        baseline_intensity: round_candidate_value(
            fitted_surprise_arrival_layer.baseline_intensity_per_hour,
        ),
        weekly_hour_utc_multipliers: fitted_surprise_arrival_layer
            .deterministic_intensity_profile
            .weekly_hour_utc_multipliers
            .clone(),
        profile_resolution_hours: fitted_surprise_arrival_layer
            .deterministic_intensity_profile
            .profile_resolution_hours,
    }
}

fn build_return_jump_mark_scale_families(
    fitted_return_jump_mark_layer: &mark_layer_fit::FittedReturnJumpMarkLayer,
) -> ReturnComponentsReturnJumpMarkScaleCandidateFamily {
    ReturnComponentsReturnJumpMarkScaleCandidateFamily {
        sigma_g: fitted_return_jump_mark_layer.sigma_g,
    }
}

fn build_activity_coupling_families(
    fitted_activity_coupling_layer: &activity_coupling_fit::FittedActivityCouplingLayer,
) -> ReturnComponentsActivityCouplingCandidateFamily {
    ReturnComponentsActivityCouplingCandidateFamily {
        known_activity_coupling_scale: fitted_activity_coupling_layer.known_activity_coupling_scale,
        surprise_activity_coupling_scale: fitted_activity_coupling_layer
            .surprise_activity_coupling_scale,
    }
}

fn build_candidate_provenance_template(
    source_window: ReturnComponentsStageSignalSourceWindow,
    stage_seed: u64,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    close_open_gap_detection_filter: &str,
    fitted_signal_partition_layer: signal_partition_fit::FittedSignalPartitionLayer,
    fitted_surprise_episode_selection_policy: ReturnComponentsStageSurpriseEpisodeSelectionPolicy,
    fitted_quantile_binning_policy: ReturnComponentsStageQuantileBinningPolicy,
    fitted_diagnostic_policy: ReturnComponentsStageDiagnosticPolicy,
    fitted_objective_scoring_policy: ReturnComponentsStageObjectiveScoringPolicy,
    fitted_price_range_sanity_policy: ReturnComponentsStagePriceRangeSanityPolicy,
) -> ReturnComponentsStageCandidateProvenance {
    ReturnComponentsStageCandidateProvenance {
        source_windows: vec![
            ReturnComponentsStageCandidateSourceWindow {
                source_name: "known_news_calendar".to_string(),
                window_start_utc_nanos: source_window.window_start_utc_nanos,
                window_end_utc_nanos: source_window.window_end_utc_nanos,
            },
            ReturnComponentsStageCandidateSourceWindow {
                source_name: "realized_known_news_return_samples".to_string(),
                window_start_utc_nanos: source_window.window_start_utc_nanos,
                window_end_utc_nanos: source_window.window_end_utc_nanos,
            },
            ReturnComponentsStageCandidateSourceWindow {
                source_name: "observed_open_session_returns".to_string(),
                window_start_utc_nanos: source_window.window_start_utc_nanos,
                window_end_utc_nanos: source_window.window_end_utc_nanos,
            },
            ReturnComponentsStageCandidateSourceWindow {
                source_name: "detected_intraday_jump_candidates".to_string(),
                window_start_utc_nanos: source_window.window_start_utc_nanos,
                window_end_utc_nanos: source_window.window_end_utc_nanos,
            },
            ReturnComponentsStageCandidateSourceWindow {
                source_name: "observed_close_open_boundary_observations".to_string(),
                window_start_utc_nanos: source_window.window_start_utc_nanos,
                window_end_utc_nanos: source_window.window_end_utc_nanos,
            },
        ],
        preprocessing_filter_policy: RETURN_COMPONENTS_STAGE_SIGNAL_PREPROCESSING_POLICY,
        preprocessing_filters: vec![
            "reject_non_finite_known_news_returns".to_string(),
            "reject_non_finite_observed_open_session_returns".to_string(),
            "exclude_known_news_windows_from_continuous_nig_fit".to_string(),
            format!(
                "known_news_exclusion_window_minutes={}",
                fitted_signal_partition_layer.known_news_exclusion_window_minutes
            ),
            "exclude_selected_exogenous_surprise_episodes_from_continuous_nig_fit".to_string(),
            format!(
                "continuous_return_filter_multiplier={:.6}",
                fitted_signal_partition_layer.continuous_return_filter_multiplier
            ),
            format!(
                "continuous_return_filter_threshold={:.12}",
                fitted_signal_partition_layer.continuous_return_filter_threshold
            ),
            "reject_non_finite_intraday_jump_returns".to_string(),
            "reject_non_finite_and_non_positive_close_open_boundary_prices".to_string(),
            "enforce_non_negative_post_shock_activity_lift".to_string(),
            "sort_observed_open_session_returns_by_timestamp".to_string(),
            "sort_intraday_jumps_by_jump_timestamp".to_string(),
            "validate_close_open_session_boundary_transitions".to_string(),
            close_open_gap_detection_filter.to_string(),
            "sort_close_open_gaps_by_close_then_open_timestamp".to_string(),
        ],
        surprise_episode_selection_policy: fitted_surprise_episode_selection_policy,
        quantile_binning_policy: fitted_quantile_binning_policy,
        diagnostic_policy: fitted_diagnostic_policy,
        objective_scoring_policy: fitted_objective_scoring_policy,
        price_range_sanity_policy: fitted_price_range_sanity_policy,
        summary_statistics,
        deterministic_seed_metadata: ReturnComponentsStageDeterministicSeedMetadata {
            stage_seed,
            candidate_generation_seed: stage_seed,
            deterministic_seed_policy: RETURN_COMPONENTS_STAGE_DETERMINISTIC_SEED_POLICY,
        },
    }
}

fn build_candidate_family_counts() -> ReturnComponentsStageCandidateFamilyCounts {
    ReturnComponentsStageCandidateFamilyCounts {
        nig_candidate_count: 1,
        known_news_arrival_family_count: 1,
        surprise_news_arrival_family_count: 1,
        return_jump_mark_scale_family_count: 1,
        activity_coupling_family_count: 1,
    }
}

fn derive_continuous_fit_business_time_scales(
    sorted_signals: &ReturnComponentsStageSortedSignalInputs,
    activity_calibration_context: Option<ReturnComponentsStageActivityCalibrationContext>,
) -> Vec<f64> {
    let Some(activity_calibration_context) = activity_calibration_context else {
        return vec![1.0; sorted_signals.continuous_fit_returns.len()];
    };

    let representative_interarrival_hours = activity_calibration_context
        .representative_interarrival_hours
        .max(1.0e-8);
    let maximum_elapsed_hours = representative_interarrival_hours * 8.0;
    let fallback_business_time_scale = (activity_calibration_context
        .modeled_open_event_rate_per_hour
        * representative_interarrival_hours)
        .max(1.0e-8);
    let mut business_time_scale_by_timestamp = BTreeMap::new();
    let mut previous_timestamp_utc_nanos: Option<i64> = None;

    for observation in &sorted_signals.observed_open_session_returns {
        let elapsed_hours = previous_timestamp_utc_nanos
            .map(|previous_timestamp_utc_nanos_value| {
                (observation.return_timestamp_utc_nanos - previous_timestamp_utc_nanos_value) as f64
                    / 3_600_000_000_000.0
            })
            .filter(|elapsed_hours_value| {
                elapsed_hours_value.is_finite()
                    && *elapsed_hours_value > 0.0
                    && *elapsed_hours_value <= maximum_elapsed_hours
            })
            .unwrap_or(representative_interarrival_hours);
        let business_time_scale = (activity_calibration_context.modeled_open_event_rate_per_hour
            * elapsed_hours)
            .max(1.0e-8);

        business_time_scale_by_timestamp
            .insert(observation.return_timestamp_utc_nanos, business_time_scale);

        previous_timestamp_utc_nanos = Some(observation.return_timestamp_utc_nanos);
    }

    sorted_signals
        .continuous_fit_returns
        .iter()
        .map(|observation| {
            business_time_scale_by_timestamp
                .get(&observation.return_timestamp_utc_nanos)
                .copied()
                .unwrap_or(fallback_business_time_scale)
        })
        .collect()
}

fn shared_mark_law_mean_abs_jump_multiplier(
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
) -> f64 {
    let severity_scale_proxy = (shared_mark_law_template.gamma_severity_marks.shape_k
        * shared_mark_law_template.gamma_severity_marks.scale_theta)
        .sqrt();
    let base_shock_mean_absolute_value = shared_mark_law_template
        .double_exponential_base_shock
        .probability_positive
        / shared_mark_law_template
            .double_exponential_base_shock
            .eta_plus
        + (1.0
            - shared_mark_law_template
                .double_exponential_base_shock
                .probability_positive)
            / shared_mark_law_template
                .double_exponential_base_shock
                .eta_minus;

    severity_scale_proxy * base_shock_mean_absolute_value
}

fn fitted_mean_abs_jump_return(
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
) -> f64 {
    return_jump_family.sigma_g * shared_mark_law_mean_abs_jump_multiplier(shared_mark_law_template)
}

pub(super) fn summarize_return_distribution(
    values: &[f64],
    business_time_scales: &[f64],
) -> ReturnComponentsStageReturnDistributionSummary {
    let normalized_mean_values: Vec<f64> = values
        .iter()
        .zip(business_time_scales.iter())
        .filter_map(|(value, business_time_scale)| {
            if !value.is_finite() || !business_time_scale.is_finite() || *business_time_scale <= 0.0
            {
                return None;
            }

            Some(*value / business_time_scale.max(1.0e-8))
        })
        .collect();
    let normalized_variance_values: Vec<f64> = values
        .iter()
        .zip(business_time_scales.iter())
        .filter_map(|(value, business_time_scale)| {
            if !value.is_finite() || !business_time_scale.is_finite() || *business_time_scale <= 0.0
            {
                return None;
            }

            Some(*value / business_time_scale.max(1.0e-8).sqrt())
        })
        .collect();

    ReturnComponentsStageReturnDistributionSummary {
        mean_return: arithmetic_mean_signed(normalized_mean_values.as_slice()),
        volatility: sample_standard_deviation(normalized_variance_values.as_slice()),
        skewness: sample_adjusted_skewness(normalized_variance_values.as_slice()).unwrap_or(0.0),
        excess_kurtosis: sample_adjusted_excess_kurtosis(normalized_variance_values.as_slice())
            .unwrap_or(0.0),
    }
}

fn compute_observed_log_price_span(timestamped_returns: &[(i64, f64)]) -> f64 {
    if timestamped_returns.is_empty() {
        return 0.0;
    }

    let mut sorted_returns = timestamped_returns.to_vec();
    sorted_returns.sort_by_key(|(timestamp_utc_nanos, _)| *timestamp_utc_nanos);

    let mut cumulative_return = 0.0_f64;
    let mut minimum_cumulative_return = 0.0_f64;
    let mut maximum_cumulative_return = 0.0_f64;
    for (_, return_increment) in sorted_returns {
        cumulative_return += return_increment;
        minimum_cumulative_return = minimum_cumulative_return.min(cumulative_return);
        maximum_cumulative_return = maximum_cumulative_return.max(cumulative_return);
    }

    (maximum_cumulative_return - minimum_cumulative_return).max(0.0)
}

fn is_within_known_news_exclusion_window(
    candidate_timestamp_utc_nanos: i64,
    known_news_timestamps: &[i64],
    exclusion_window_minutes: i64,
) -> bool {
    if known_news_timestamps.is_empty() {
        return false;
    }

    let window_nanos = Duration::minutes(exclusion_window_minutes)
        .num_nanoseconds()
        .expect("known-news exclusion window must fit in i64");
    let lower_bound = candidate_timestamp_utc_nanos - window_nanos;
    let upper_bound = candidate_timestamp_utc_nanos + window_nanos;
    let lower_index = known_news_timestamps.partition_point(|timestamp| *timestamp < lower_bound);

    lower_index < known_news_timestamps.len() && known_news_timestamps[lower_index] <= upper_bound
}

fn prepare_candidate_generation_input(
    exchange_code: &str,
    calendar_registry: &ExchangeCalendarRegistry,
    mut candidate_generation_input: ReturnComponentsStageCandidateGenerationInput,
) -> Result<
    ReturnComponentsStagePreparedCandidateGenerationInput,
    ReturnComponentsStageCandidateGenerationInputError,
> {
    let fitted_signal_partition_layer = signal_partition_fit::fit_signal_partition_layer(
        candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
    );
    let derived_close_open_gap_candidates = derive_close_open_gap_candidates(
        exchange_code,
        calendar_registry,
        candidate_generation_input.known_news_calendar.source_window,
        candidate_generation_input
            .observed_close_open_boundary_observations
            .as_slice(),
        candidate_generation_input.close_open_gap_detection_policy,
    )?;
    let close_open_gap_detection_filter = format!(
        "derive_close_open_log_gap_candidates_abs_gt_{:.6}",
        candidate_generation_input
            .close_open_gap_detection_policy
            .material_gap_absolute_return_tolerance
    );
    let fitted_surprise_episode_selection_layer =
        surprise_episode_selection_fit::fit_surprise_episode_selection_layer(
            candidate_generation_input
                .detected_intraday_jump_candidates
                .as_slice(),
            fitted_signal_partition_layer.continuous_return_filter_threshold,
        );
    let open_shock_returns: Vec<f64> = candidate_generation_input
        .realized_known_news_return_samples
        .iter()
        .map(|sample| sample.open_window_jump_return)
        .chain(
            fitted_surprise_episode_selection_layer
                .selected_surprise_jump_episodes
                .iter()
                .map(|episode| episode.cumulative_jump_return),
        )
        .collect();
    let gap_returns: Vec<f64> = candidate_generation_input
        .realized_known_news_return_samples
        .iter()
        .map(|sample| sample.closed_window_gap_return)
        .chain(
            derived_close_open_gap_candidates
                .iter()
                .map(|candidate| candidate.gap_return),
        )
        .collect();
    let fallback_absolute_returns: Vec<f64> = candidate_generation_input
        .observed_open_session_returns
        .iter()
        .map(|observation| observation.log_return.abs())
        .chain(
            candidate_generation_input
                .observed_close_open_boundary_observations
                .iter()
                .map(|observation| {
                    f64::abs(f64::ln(observation.open_price / observation.close_price))
                }),
        )
        .collect();
    candidate_generation_input.shared_mark_law_template =
        fit_shared_news_mark_law_from_shock_samples(
            open_shock_returns.as_slice(),
            gap_returns.as_slice(),
            fallback_absolute_returns.as_slice(),
        );

    Ok(ReturnComponentsStagePreparedCandidateGenerationInput {
        candidate_generation_input,
        derived_close_open_gap_candidates,
        close_open_gap_detection_filter,
    })
}

fn derive_close_open_gap_candidates(
    exchange_code: &str,
    calendar_registry: &ExchangeCalendarRegistry,
    source_window: ReturnComponentsStageSignalSourceWindow,
    observed_close_open_boundary_observations: &[ReturnComponentsObservedCloseOpenBoundaryObservation],
    detection_policy: ReturnComponentsCloseOpenGapDetectionPolicy,
) -> Result<
    Vec<ReturnComponentsDetectedCloseOpenGapCandidate>,
    ReturnComponentsStageCandidateGenerationInputError,
> {
    let mut derived_close_open_gap_candidates = Vec::new();

    for observed_boundary in observed_close_open_boundary_observations {
        observed_boundary.validate(source_window)?;
        validate_close_open_boundary_transition(
            exchange_code,
            calendar_registry,
            observed_boundary.close_timestamp_utc_nanos,
            observed_boundary.open_timestamp_utc_nanos,
        )?;

        let gap_return = (observed_boundary.open_price / observed_boundary.close_price).ln();
        if gap_return.abs() <= detection_policy.material_gap_absolute_return_tolerance {
            continue;
        }

        let derived_candidate = ReturnComponentsDetectedCloseOpenGapCandidate {
            close_timestamp_utc_nanos: observed_boundary.close_timestamp_utc_nanos,
            open_timestamp_utc_nanos: observed_boundary.open_timestamp_utc_nanos,
            gap_return,
            post_shock_activity_lift: observed_boundary.post_shock_activity_lift,
        };
        derived_candidate.validate(source_window)?;
        derived_close_open_gap_candidates.push(derived_candidate);
    }

    Ok(derived_close_open_gap_candidates)
}

fn validate_close_open_boundary_transition(
    exchange_code: &str,
    calendar_registry: &ExchangeCalendarRegistry,
    close_timestamp_utc_nanos: i64,
    open_timestamp_utc_nanos: i64,
) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
    let close_previous_state = resolve_market_state_at_nanos(
        calendar_registry,
        exchange_code,
        close_timestamp_utc_nanos.saturating_sub(1),
    )?;
    let close_boundary_state =
        resolve_market_state_at_nanos(calendar_registry, exchange_code, close_timestamp_utc_nanos)?;
    if close_previous_state != MarketState::Open || close_boundary_state != MarketState::Closed {
        return Err(
            ReturnComponentsStageCandidateGenerationInputError::InvalidCloseBoundaryTransition {
                exchange_code: exchange_code.to_string(),
                close_timestamp_utc_nanos,
                previous_state: close_previous_state,
                boundary_state: close_boundary_state,
            },
        );
    }

    let open_previous_state = resolve_market_state_at_nanos(
        calendar_registry,
        exchange_code,
        open_timestamp_utc_nanos.saturating_sub(1),
    )?;
    let open_boundary_state =
        resolve_market_state_at_nanos(calendar_registry, exchange_code, open_timestamp_utc_nanos)?;
    if open_previous_state != MarketState::Closed || open_boundary_state != MarketState::Open {
        return Err(
            ReturnComponentsStageCandidateGenerationInputError::InvalidOpenBoundaryTransition {
                exchange_code: exchange_code.to_string(),
                open_timestamp_utc_nanos,
                previous_state: open_previous_state,
                boundary_state: open_boundary_state,
            },
        );
    }

    Ok(())
}

fn resolve_market_state_at_nanos(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    timestamp_utc_nanos: i64,
) -> Result<MarketState, ReturnComponentsStageCandidateGenerationInputError> {
    let timestamp_utc = DateTime::<Utc>::from_timestamp_nanos(timestamp_utc_nanos);

    calendar_registry
        .resolve_market_state_at_utc(exchange_code, timestamp_utc)
        .map_err(|source: crate::calendar::ExchangeCalendarResolveError| {
            ReturnComponentsStageCandidateGenerationInputError::CloseOpenBoundaryStateResolutionFailed {
                exchange_code: exchange_code.to_string(),
                timestamp_utc_nanos,
                error_message: source.to_string(),
            }
        })
}

fn enumerate_candidate_inputs(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    candidate_family_sets: &ReturnComponentsStageCandidateFamilySets,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    _continuous_return_calibration_sample: &continuous_fit::ReturnComponentsStageContinuousReturnCalibrationSample,
    fitted_surprise_deterministic_intensity_profile: &SurpriseNewsDeterministicIntensityProfile,
    _stage_seed: u64,
    candidate_provenance_template: &ReturnComponentsStageCandidateProvenance,
    fitted_diagnostic_policy: &ReturnComponentsStageDiagnosticPolicy,
    fitted_objective_scoring_policy: &ReturnComponentsStageObjectiveScoringPolicy,
    fitted_price_range_sanity_policy: &ReturnComponentsStagePriceRangeSanityPolicy,
) -> Result<
    Vec<ReturnComponentsStageCandidateInput>,
    ReturnComponentsStageCandidateGenerationInputError,
> {
    let nig_candidate = candidate_family_sets
        .fitted_continuous_return_layer
        .nig_configuration;
    let continuous_return_evaluation = &candidate_family_sets
        .fitted_continuous_return_layer
        .evaluation;
    let known_arrival_family = candidate_family_sets
        .fitted_known_arrival_layer
        .as_candidate_family();
    let surprise_arrival_family = &candidate_family_sets.fitted_surprise_arrival_layer;
    let return_jump_family = &candidate_family_sets.fitted_return_jump_mark_layer;
    let activity_coupling_family = &candidate_family_sets.fitted_activity_coupling_layer;
    let candidate_families = ReturnComponentsStageCandidateFamilies {
        known_news_arrival_controls: known_arrival_family.clone(),
        surprise_news_arrival_controls: surprise_arrival_family.clone(),
        return_jump_mark_scale_controls: *return_jump_family,
        news_to_hawkes_activity_coupling_controls: *activity_coupling_family,
    };
    let return_distribution_diagnostic = continuous_return_evaluation.diagnostic;
    let jump_diagnostic = build_jump_diagnostic(
        fitted_diagnostic_policy,
        summary_statistics,
        return_jump_family,
        candidate_generation_input.shared_mark_law_template,
    );
    let surprise_arrival_diagnostic = build_surprise_arrival_diagnostic(
        fitted_diagnostic_policy,
        summary_statistics,
        surprise_arrival_family,
        fitted_surprise_deterministic_intensity_profile,
    );
    let gap_diagnostic = build_gap_diagnostic(
        fitted_diagnostic_policy,
        summary_statistics,
        return_jump_family,
        candidate_generation_input.shared_mark_law_template,
    );
    let activity_coupling_diagnostic = build_activity_coupling_diagnostic(
        fitted_diagnostic_policy,
        summary_statistics,
        activity_coupling_family,
    );
    let price_range_sanity_diagnostic = build_price_range_sanity_diagnostic(
        fitted_diagnostic_policy,
        summary_statistics,
        return_distribution_diagnostic,
        surprise_arrival_family,
        return_jump_family,
        candidate_generation_input.shared_mark_law_template,
        fitted_price_range_sanity_policy,
    );
    let objective_metric_buckets = objective_scoring_policy_fit::score_candidate_metric_buckets(
        fitted_objective_scoring_policy,
        continuous_return_evaluation.objective_score,
        &surprise_arrival_diagnostic,
        &jump_diagnostic,
        &gap_diagnostic,
        &activity_coupling_diagnostic,
        &price_range_sanity_diagnostic,
        fitted_diagnostic_policy.shock_evidence_min_count,
    );
    let dominant_scale_driver =
        objective_scoring_policy_fit::dominant_scale_driver_from_metric_buckets(
            fitted_objective_scoring_policy,
            objective_metric_buckets,
        );
    let objective_terms = objective_scoring_policy_fit::build_candidate_objective_terms(
        candidate_generation_input.objective_metric_weights,
        objective_metric_buckets,
    );
    let news_process_configuration = build_news_process_configuration_from_candidate_families(
        candidate_generation_input,
        &known_arrival_family,
        surprise_arrival_family,
        return_jump_family,
        activity_coupling_family,
    )?;

    Ok(vec![ReturnComponentsStageCandidateInput {
        candidate_id: "generated_nig_001_known_001_surprise_001_mark_001_coupling_001".to_string(),
        nig_shared_plus_shift_configuration: nig_candidate,
        news_process_configuration,
        objective_metric_buckets,
        return_distribution_diagnostic,
        surprise_arrival_diagnostic,
        jump_diagnostic,
        gap_diagnostic,
        activity_coupling_diagnostic,
        price_range_sanity_diagnostic,
        dominant_scale_driver,
        objective_terms,
        candidate_families,
        candidate_provenance: candidate_provenance_template.clone(),
    }])
}

fn build_news_process_configuration_from_candidate_families(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    known_arrival_family: &ReturnComponentsKnownNewsArrivalCandidateFamily,
    surprise_arrival_family: &ReturnComponentsSurpriseNewsArrivalCandidateFamily,
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    activity_coupling_family: &ReturnComponentsActivityCouplingCandidateFamily,
) -> Result<NewsProcessConfiguration, ReturnComponentsStageCandidateGenerationInputError> {
    let mut shared_mark_law = candidate_generation_input.shared_mark_law_template;
    shared_mark_law.coupled_gap_marks =
        CoupledGapMarkConfiguration::new(return_jump_family.sigma_g);

    NewsProcessConfiguration::try_new(
        NewsArrivalConfiguration {
            known: KnownNewsArrivalConfiguration {
                scheduled_event_timestamps_utc_nanos: known_arrival_family
                    .retained_known_event_timestamps_utc_nanos
                    .clone(),
            },
            surprise: SurpriseNewsArrivalConfiguration {
                baseline_intensity: surprise_arrival_family.baseline_intensity,
            },
        },
        shared_mark_law,
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: activity_coupling_family.known_activity_coupling_scale,
            return_jump_scale: 1.0,
        },
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: activity_coupling_family.surprise_activity_coupling_scale,
            return_jump_scale: 1.0,
        },
    )
    .map_err(|source| {
        ReturnComponentsStageCandidateGenerationInputError::InvalidSharedMarkLawTemplate {
            error_message: source.to_string(),
        }
    })
}

fn build_jump_diagnostic(
    diagnostic_policy: &ReturnComponentsStageDiagnosticPolicy,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
) -> ReturnComponentsStageJumpDiagnostic {
    let fitted_mean_abs_jump_return =
        fitted_mean_abs_jump_return(shared_mark_law_template, return_jump_family);

    ReturnComponentsStageJumpDiagnostic {
        evidence_state: diagnostic_policy_fit::evidence_state_from_count(
            diagnostic_policy,
            summary_statistics.selected_surprise_jump_episode_count,
        )
        .to_string(),
        observed_jump_count: summary_statistics.selected_surprise_jump_episode_count,
        observed_mean_abs_jump_return: if summary_statistics.selected_surprise_jump_episode_count
            < diagnostic_policy.shock_evidence_min_count
        {
            0.0
        } else {
            summary_statistics.mean_abs_surprise_open_jump_return
        },
        fitted_mean_abs_jump_return,
        fitted_sigma_g: return_jump_family.sigma_g,
    }
}

fn build_surprise_arrival_diagnostic(
    diagnostic_policy: &ReturnComponentsStageDiagnosticPolicy,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    surprise_arrival_family: &ReturnComponentsSurpriseNewsArrivalCandidateFamily,
    fitted_surprise_deterministic_intensity_profile: &SurpriseNewsDeterministicIntensityProfile,
) -> ReturnComponentsStageSurpriseArrivalDiagnostic {
    let candidate_surprise_profile = SurpriseNewsDeterministicIntensityProfile {
        weekly_hour_utc_multipliers: surprise_arrival_family.weekly_hour_utc_multipliers.clone(),
        profile_resolution_hours: surprise_arrival_family.profile_resolution_hours,
        fit_policy: fitted_surprise_deterministic_intensity_profile
            .fit_policy
            .clone(),
        normalization_policy: fitted_surprise_deterministic_intensity_profile
            .normalization_policy
            .clone(),
    };

    ReturnComponentsStageSurpriseArrivalDiagnostic {
        evidence_state: diagnostic_policy_fit::evidence_state_from_count(
            diagnostic_policy,
            summary_statistics.selected_surprise_jump_episode_count,
        )
        .to_string(),
        observed_jump_count: summary_statistics.selected_surprise_jump_episode_count,
        observed_jump_frequency_per_hour: if summary_statistics.selected_surprise_jump_episode_count
            < diagnostic_policy.shock_evidence_min_count
        {
            0.0
        } else {
            summary_statistics.selected_surprise_jump_episode_count as f64
                / summary_statistics
                    .source_window_duration_hours
                    .max(f64::EPSILON)
        },
        fitted_surprise_arrival_intensity_per_hour: surprise_arrival_family.baseline_intensity,
        surprise_arrival_profile_fit_error: if summary_statistics
            .selected_surprise_jump_episode_count
            < diagnostic_policy.shock_evidence_min_count
        {
            0.0
        } else {
            surprise_intensity_fit::profile_mean_absolute_error(
                &candidate_surprise_profile,
                fitted_surprise_deterministic_intensity_profile,
            )
        },
    }
}

fn build_gap_diagnostic(
    diagnostic_policy: &ReturnComponentsStageDiagnosticPolicy,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
) -> ReturnComponentsStageGapDiagnostic {
    let fitted_mean_abs_gap_return =
        fitted_mean_abs_jump_return(shared_mark_law_template, return_jump_family);

    ReturnComponentsStageGapDiagnostic {
        evidence_state: diagnostic_policy_fit::evidence_state_from_count(
            diagnostic_policy,
            summary_statistics.close_open_gap_episode_count,
        )
        .to_string(),
        observed_gap_count: summary_statistics.close_open_gap_episode_count,
        observed_gap_frequency_per_hour: if summary_statistics.close_open_gap_episode_count
            < diagnostic_policy.shock_evidence_min_count
        {
            0.0
        } else {
            summary_statistics.close_open_gap_episode_count as f64
                / summary_statistics
                    .source_window_duration_hours
                    .max(f64::EPSILON)
        },
        observed_mean_abs_gap_return: if summary_statistics.close_open_gap_episode_count
            < diagnostic_policy.shock_evidence_min_count
        {
            0.0
        } else {
            summary_statistics.mean_abs_surprise_closed_gap_return
        },
        fitted_mean_abs_gap_return,
        fitted_sigma_g: return_jump_family.sigma_g,
    }
}

fn build_activity_coupling_diagnostic(
    diagnostic_policy: &ReturnComponentsStageDiagnosticPolicy,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    activity_coupling_family: &ReturnComponentsActivityCouplingCandidateFamily,
) -> ReturnComponentsStageActivityCouplingDiagnostic {
    ReturnComponentsStageActivityCouplingDiagnostic {
        evidence_state: diagnostic_policy_fit::evidence_state_from_count(
            diagnostic_policy,
            summary_statistics.selected_surprise_jump_episode_count,
        )
        .to_string(),
        observed_known_post_shock_activity_lift: summary_statistics.known_activity_coupling_target,
        observed_surprise_post_shock_activity_lift: if summary_statistics
            .selected_surprise_jump_episode_count
            < diagnostic_policy.shock_evidence_min_count
        {
            0.0
        } else {
            summary_statistics.surprise_activity_coupling_target
        },
        fitted_known_activity_coupling_scale: activity_coupling_family
            .known_activity_coupling_scale,
        fitted_surprise_activity_coupling_scale: activity_coupling_family
            .surprise_activity_coupling_scale,
    }
}

fn build_price_range_sanity_diagnostic(
    diagnostic_policy: &ReturnComponentsStageDiagnosticPolicy,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    return_distribution_diagnostic: ReturnComponentsStageReturnDistributionDiagnostic,
    surprise_arrival_family: &ReturnComponentsSurpriseNewsArrivalCandidateFamily,
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
    price_range_sanity_policy: &ReturnComponentsStagePriceRangeSanityPolicy,
) -> ReturnComponentsStagePriceRangeSanityDiagnostic {
    let effective_continuous_sample_count = summary_statistics.continuous_fit_return_count.max(1);
    let effective_mean_business_time_scale = summary_statistics
        .mean_continuous_business_time_scale
        .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER);
    let continuous_reference_span = price_range_sanity_policy.continuous_reference_span_multiplier
        * return_distribution_diagnostic.fitted_return_volatility
        * (effective_continuous_sample_count as f64 * effective_mean_business_time_scale)
            .sqrt()
            .max(1.0);
    let fitted_mean_abs_jump_return =
        fitted_mean_abs_jump_return(shared_mark_law_template, return_jump_family);
    let fitted_reference_log_price_span = continuous_reference_span
        + (summary_statistics.known_realized_sample_count as f64
            + surprise_arrival_family.baseline_intensity
                * summary_statistics.source_window_duration_hours)
            * fitted_mean_abs_jump_return
        + if summary_statistics.close_open_gap_episode_count
            >= diagnostic_policy.shock_evidence_min_count
        {
            summary_statistics.close_open_gap_candidate_count as f64 * fitted_mean_abs_jump_return
        } else {
            0.0
        };
    let expansion_ratio =
        fitted_reference_log_price_span / summary_statistics.observed_log_price_span.max(1.0e-8);
    let is_flagged = expansion_ratio > price_range_sanity_policy.threshold_ratio;

    ReturnComponentsStagePriceRangeSanityDiagnostic {
        policy: price_range_sanity_policy.clone(),
        is_flagged,
        observed_log_price_span: summary_statistics.observed_log_price_span,
        fitted_reference_log_price_span,
        continuous_reference_log_price_span: continuous_reference_span,
        effective_continuous_sample_count,
        effective_mean_business_time_scale,
        fitted_mean_abs_jump_return,
        expansion_ratio,
    }
}

pub(super) fn theoretical_net_process_moments(
    configuration: NigSharedPlusShiftConfiguration,
    open_session_sell_share_proxy: f64,
) -> Option<(f64, f64, f64, f64)> {
    let buy_unit_cumulants = nig_unit_cumulants(
        configuration.alpha,
        configuration.beta_0,
        configuration.delta,
        configuration.mu,
    )?;
    let sell_unit_cumulants = nig_unit_cumulants(
        configuration.alpha,
        configuration.derived_sell_beta(),
        configuration.delta,
        configuration.mu,
    )?;
    let sell_weight = open_session_sell_share_proxy.clamp(0.0, 1.0);
    let buy_weight = 1.0 - sell_weight;
    let mean_return = buy_weight * buy_unit_cumulants.0 - sell_weight * sell_unit_cumulants.0;
    let variance = buy_weight * buy_unit_cumulants.1 + sell_weight * sell_unit_cumulants.1;
    if !variance.is_finite() || variance <= 1.0e-12 {
        return None;
    }

    let third_cumulant = buy_weight * buy_unit_cumulants.2 - sell_weight * sell_unit_cumulants.2;
    let fourth_cumulant = buy_weight * buy_unit_cumulants.3 + sell_weight * sell_unit_cumulants.3;
    let skewness = third_cumulant / variance.powf(1.5);
    let excess_kurtosis = fourth_cumulant / (variance * variance);

    Some((mean_return, variance, skewness, excess_kurtosis))
}

fn nig_unit_cumulants(alpha: f64, beta: f64, delta: f64, mu: f64) -> Option<(f64, f64, f64, f64)> {
    let gamma_squared = alpha * alpha - beta * beta;
    if !gamma_squared.is_finite() || gamma_squared <= 0.0 {
        return None;
    }
    let gamma = gamma_squared.sqrt();
    let kappa_1 = mu + delta * beta / gamma;
    let kappa_2 = delta * alpha * alpha / gamma.powi(3);
    let kappa_3 = 3.0 * delta * beta * alpha * alpha / gamma.powi(5);
    let kappa_4 = 3.0 * delta * alpha * alpha * (alpha * alpha + 4.0 * beta * beta) / gamma.powi(7);

    Some((kappa_1, kappa_2, kappa_3, kappa_4))
}

pub(super) fn arithmetic_mean_signed(values: &[f64]) -> f64 {
    let finite_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    if finite_values.is_empty() {
        return 0.0;
    }

    finite_values.iter().sum::<f64>() / finite_values.len() as f64
}

pub(super) fn sample_standard_deviation(values: &[f64]) -> f64 {
    let finite_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    if finite_values.len() < 2 {
        return finite_values.first().copied().unwrap_or(0.0).abs();
    }

    let sample_mean = arithmetic_mean_signed(finite_values.as_slice());
    let sum_squared_deviation = finite_values
        .iter()
        .map(|value| {
            let centered_value = *value - sample_mean;
            centered_value * centered_value
        })
        .sum::<f64>();

    (sum_squared_deviation / (finite_values.len() - 1) as f64)
        .max(0.0)
        .sqrt()
}

pub(super) fn sample_adjusted_skewness(values: &[f64]) -> Option<f64> {
    let finite_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    let sample_count = finite_values.len();
    if sample_count < 3 {
        return None;
    }

    let sample_mean = arithmetic_mean_signed(finite_values.as_slice());
    let (sum_squared_deviation, sum_cubed_deviation) =
        finite_values
            .iter()
            .fold((0.0, 0.0), |(running_sum2, running_sum3), value| {
                let centered_value = *value - sample_mean;
                (
                    running_sum2 + centered_value * centered_value,
                    running_sum3 + centered_value * centered_value * centered_value,
                )
            });
    let variance_population = sum_squared_deviation / sample_count as f64;
    if !variance_population.is_finite() || variance_population <= 1.0e-12 {
        return Some(0.0);
    }

    let unadjusted_skewness =
        (sum_cubed_deviation / sample_count as f64) / variance_population.powf(1.5);
    if !unadjusted_skewness.is_finite() {
        return None;
    }

    let adjustment =
        ((sample_count * (sample_count - 1)) as f64).sqrt() / (sample_count - 2) as f64;
    Some(adjustment * unadjusted_skewness)
}

pub(super) fn sample_adjusted_excess_kurtosis(values: &[f64]) -> Option<f64> {
    let finite_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    let sample_count = finite_values.len();
    if sample_count < 4 {
        return None;
    }

    let sample_mean = arithmetic_mean_signed(finite_values.as_slice());
    let (sum_squared_deviation, sum_fourth_deviation) =
        finite_values
            .iter()
            .fold((0.0, 0.0), |(running_sum2, running_sum4), value| {
                let centered_value = *value - sample_mean;
                let centered_square = centered_value * centered_value;
                (
                    running_sum2 + centered_square,
                    running_sum4 + centered_square * centered_square,
                )
            });
    let variance_population = sum_squared_deviation / sample_count as f64;
    if !variance_population.is_finite() || variance_population <= 1.0e-12 {
        return Some(0.0);
    }

    let unadjusted_excess_kurtosis = (sum_fourth_deviation / sample_count as f64)
        / (variance_population * variance_population)
        - 3.0;
    if !unadjusted_excess_kurtosis.is_finite() {
        return None;
    }

    let n = sample_count as f64;
    Some(((n - 1.0) / ((n - 2.0) * (n - 3.0))) * ((n + 1.0) * unadjusted_excess_kurtosis + 6.0))
}

pub(super) fn round_positive_candidate_value(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }

    let rounded = round_candidate_value(value.max(0.0));
    if value > 0.0 && rounded <= 0.0 {
        RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER
    } else {
        rounded
    }
}

pub(super) fn round_signed_candidate_value(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }

    (value * 1_000_000_000_000.0).round() / 1_000_000_000_000.0
}

pub(super) fn round_negative_candidate_value(value: f64) -> f64 {
    if !value.is_finite() {
        return -RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER;
    }

    ((value.min(-RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER))
        * 1_000_000_000_000.0)
        .round()
        / 1_000_000_000_000.0
}

fn round_candidate_value(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }

    let rounded = (value * 1_000_000_000_000.0).round() / 1_000_000_000_000.0;
    if rounded < 0.0 { 0.0 } else { rounded }
}

fn collect_return_components_stage_constraint_violations(
    candidate_input: &ReturnComponentsStageCandidateInput,
) -> Vec<ReturnComponentsStageConstraintViolationDiagnostic> {
    collect_return_components_exact_candidate_constraint_violations(
        &ReturnComponentsStageFittedParameters {
            nig_shared_plus_shift_configuration: candidate_input
                .nig_shared_plus_shift_configuration,
            news_process_configuration: candidate_input.news_process_configuration.clone(),
            surprise_deterministic_intensity_profile:
                default_surprise_deterministic_intensity_profile(),
        },
    )
}

fn collect_return_components_exact_candidate_constraint_violations(
    fitted_parameters: &ReturnComponentsStageFittedParameters,
) -> Vec<ReturnComponentsStageConstraintViolationDiagnostic> {
    let mut violations = Vec::new();

    if !fitted_parameters
        .nig_shared_plus_shift_configuration
        .delta_beta
        .is_finite()
        || fitted_parameters
            .nig_shared_plus_shift_configuration
            .delta_beta
            >= 0.0
    {
        violations.push(ReturnComponentsStageConstraintViolationDiagnostic {
            field: "nig_shared_plus_shift_configuration.delta_beta".to_string(),
            message:
                "delta_beta must be finite and strictly negative to preserve sell-side skew asymmetry"
                    .to_string(),
        });
    }

    if let Err(source) = NigSharedPlusShiftConfiguration::try_new(
        fitted_parameters.nig_shared_plus_shift_configuration.alpha,
        fitted_parameters.nig_shared_plus_shift_configuration.beta_0,
        fitted_parameters.nig_shared_plus_shift_configuration.delta,
        fitted_parameters.nig_shared_plus_shift_configuration.mu,
        fitted_parameters
            .nig_shared_plus_shift_configuration
            .delta_beta,
    ) {
        violations.push(ReturnComponentsStageConstraintViolationDiagnostic {
            field: "nig_shared_plus_shift_configuration".to_string(),
            message: source.to_string(),
        });
    }

    if let Err(source) = NewsProcessConfiguration::try_new(
        fitted_parameters
            .news_process_configuration
            .arrivals
            .clone(),
        fitted_parameters.news_process_configuration.shared_mark_law,
        fitted_parameters
            .news_process_configuration
            .known_component_mark_scales,
        fitted_parameters
            .news_process_configuration
            .surprise_component_mark_scales,
    ) {
        violations.push(ReturnComponentsStageConstraintViolationDiagnostic {
            field: "news_process_configuration".to_string(),
            message: source.to_string(),
        });
    }

    if fitted_parameters
        .surprise_deterministic_intensity_profile
        .profile_resolution_hours
        != RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_RESOLUTION_HOURS
    {
        violations.push(ReturnComponentsStageConstraintViolationDiagnostic {
            field: "surprise_deterministic_intensity_profile.profile_resolution_hours".to_string(),
            message: format!(
                "profile_resolution_hours must equal {}",
                RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_RESOLUTION_HOURS
            ),
        });
    }
    if fitted_parameters
        .surprise_deterministic_intensity_profile
        .weekly_hour_utc_multipliers
        .len()
        != RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_WEEKLY_PROFILE_BUCKET_COUNT
    {
        violations.push(ReturnComponentsStageConstraintViolationDiagnostic {
            field: "surprise_deterministic_intensity_profile.weekly_hour_utc_multipliers"
                .to_string(),
            message: format!(
                "weekly_hour_utc_multipliers must contain exactly {} values",
                RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_WEEKLY_PROFILE_BUCKET_COUNT
            ),
        });
    }
    for (bucket_index, multiplier) in fitted_parameters
        .surprise_deterministic_intensity_profile
        .weekly_hour_utc_multipliers
        .iter()
        .copied()
        .enumerate()
    {
        if !multiplier.is_finite() || multiplier <= 0.0 {
            violations.push(ReturnComponentsStageConstraintViolationDiagnostic {
                field: format!(
                    "surprise_deterministic_intensity_profile.weekly_hour_utc_multipliers[{bucket_index}]"
                ),
                message: format!(
                    "weekly surprise intensity multiplier must be finite and strictly positive, found {multiplier}"
                ),
            });
        }
    }

    violations
}

fn default_surprise_deterministic_intensity_profile() -> SurpriseNewsDeterministicIntensityProfile {
    SurpriseNewsDeterministicIntensityProfile {
        weekly_hour_utc_multipliers: vec![
            1.0;
            RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_WEEKLY_PROFILE_BUCKET_COUNT
        ],
        profile_resolution_hours:
            RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_RESOLUTION_HOURS,
        fit_policy: RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_FIT_POLICY.to_string(),
        normalization_policy:
            RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_NORMALIZATION_POLICY.to_string(),
    }
}

fn return_components_stage_optimizer_bounds() -> Vec<CalibrationOptimizerParameterBound> {
    vec![CalibrationOptimizerParameterBound {
        parameter_name: "nig_shared_plus_shift_configuration.delta_beta".to_string(),
        lower_inclusive: -f64::MAX,
        upper_inclusive: 0.0,
    }]
}

fn build_return_components_stage_handoff_outputs(
    run_spec: &CalibrationRunSpec,
) -> Vec<ReturnComponentsStageHandoffOutput> {
    run_spec
        .stage_handoffs
        .iter()
        .filter(|stage_handoff| stage_handoff.from_stage == CalibrationStage::ReturnComponents)
        .map(|stage_handoff| match stage_handoff.transport() {
            CalibrationStageHandoffTransport::InMemoryOutput => {
                ReturnComponentsStageHandoffOutput {
                    from_stage: stage_handoff.from_stage,
                    to_stage: stage_handoff.to_stage,
                    transport: stage_handoff.transport(),
                    in_memory_payload_available: true,
                    artifact_reference: None,
                }
            }
            CalibrationStageHandoffTransport::PersistedArtifactReference => {
                ReturnComponentsStageHandoffOutput {
                    from_stage: stage_handoff.from_stage,
                    to_stage: stage_handoff.to_stage,
                    transport: stage_handoff.transport(),
                    in_memory_payload_available: false,
                    artifact_reference: stage_handoff.artifact_reference().map(str::to_string),
                }
            }
        })
        .collect()
}

fn build_return_components_stage_failure_report(
    run_spec: &CalibrationRunSpec,
    failure: ReturnComponentsStageExecutionFailure,
) -> ReturnComponentsStageExecutionReport {
    ReturnComponentsStageExecutionReport {
        schema_version: CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::ReturnComponents,
        status: CalibrationStageExecutionStatus::Failure,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_RETURN_COMPONENTS_STAGE_HANDOFF_POLICY,
        handoffs: Vec::new(),
        fitted_stage_payload: None,
        metadata: None,
        failure: Some(failure),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use janiform_model_kernels::news::{
        DoubleExponentialBaseShockConfiguration, GammaSeverityMarkConfiguration,
    };

    use super::*;
    use crate::calendar::ExchangeCalendarRegistryLoader;

    #[test]
    fn derived_close_open_gap_detection_filters_zero_gap_boundaries() {
        let calendar_registry = test_calendar_registry();

        let derived_candidates = derive_close_open_gap_candidates(
            "XNYS",
            &calendar_registry,
            test_source_window(),
            &[ReturnComponentsObservedCloseOpenBoundaryObservation {
                close_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 21, 0, 0),
                close_price: 100.0,
                open_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 14, 30, 0),
                open_price: 100.0,
                post_shock_activity_lift: 1.25,
            }],
            ReturnComponentsCloseOpenGapDetectionPolicy::new(0.005),
        )
        .expect("zero-gap boundary should validate");

        assert!(derived_candidates.is_empty());
    }

    #[test]
    fn derived_close_open_gap_detection_emits_overnight_gap_candidate() {
        let calendar_registry = test_calendar_registry();

        let derived_candidates = derive_close_open_gap_candidates(
            "XNYS",
            &calendar_registry,
            test_source_window(),
            &[ReturnComponentsObservedCloseOpenBoundaryObservation {
                close_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 21, 0, 0),
                close_price: 100.0,
                open_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 14, 30, 0),
                open_price: 100.0 * (-0.013_f64).exp(),
                post_shock_activity_lift: 1.40,
            }],
            ReturnComponentsCloseOpenGapDetectionPolicy::new(0.005),
        )
        .expect("overnight boundary should validate");

        assert_eq!(derived_candidates.len(), 1);
        assert_eq!(
            derived_candidates[0].close_timestamp_utc_nanos,
            utc_nanos(2026, 2, 2, 21, 0, 0)
        );
        assert_eq!(
            derived_candidates[0].open_timestamp_utc_nanos,
            utc_nanos(2026, 2, 3, 14, 30, 0)
        );
        assert!((derived_candidates[0].gap_return + 0.013).abs() <= 1e-12);
        assert_eq!(derived_candidates[0].post_shock_activity_lift, 1.40);
    }

    #[test]
    fn derived_close_open_gap_detection_emits_weekend_gap_candidate() {
        let calendar_registry = test_calendar_registry();

        let derived_candidates = derive_close_open_gap_candidates(
            "XNYS",
            &calendar_registry,
            test_source_window(),
            &[ReturnComponentsObservedCloseOpenBoundaryObservation {
                close_timestamp_utc_nanos: utc_nanos(2026, 2, 6, 21, 0, 0),
                close_price: 100.0,
                open_timestamp_utc_nanos: utc_nanos(2026, 2, 9, 14, 30, 0),
                open_price: 100.0 * 0.012_f64.exp(),
                post_shock_activity_lift: 1.15,
            }],
            ReturnComponentsCloseOpenGapDetectionPolicy::new(0.005),
        )
        .expect("weekend boundary should validate");

        assert_eq!(derived_candidates.len(), 1);
        assert_eq!(
            derived_candidates[0].close_timestamp_utc_nanos,
            utc_nanos(2026, 2, 6, 21, 0, 0)
        );
        assert_eq!(
            derived_candidates[0].open_timestamp_utc_nanos,
            utc_nanos(2026, 2, 9, 14, 30, 0)
        );
        assert!((derived_candidates[0].gap_return - 0.012).abs() <= 1e-12);
        assert_eq!(derived_candidates[0].post_shock_activity_lift, 1.15);
    }

    #[test]
    fn derived_close_open_gap_detection_rejects_non_boundary_timestamps() {
        let calendar_registry = test_calendar_registry();

        let error = derive_close_open_gap_candidates(
            "XNYS",
            &calendar_registry,
            test_source_window(),
            &[ReturnComponentsObservedCloseOpenBoundaryObservation {
                close_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 20, 0, 0),
                close_price: 100.0,
                open_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 14, 30, 0),
                open_price: 100.0 * (-0.013_f64).exp(),
                post_shock_activity_lift: 1.40,
            }],
            ReturnComponentsCloseOpenGapDetectionPolicy::new(0.005),
        )
        .expect_err("non-boundary close timestamp must fail");

        assert!(matches!(
            error,
            ReturnComponentsStageCandidateGenerationInputError::InvalidCloseBoundaryTransition { .. }
        ));
    }

    #[test]
    fn nig_search_candidates_remain_non_empty_for_tiny_fx_return_volatility() {
        let summary_statistics = ReturnComponentsStageObservedSignalSummaryStatistics {
            known_calendar_event_count: 3,
            known_realized_sample_count: 1,
            source_window_duration_hours: 12.0,
            observed_open_session_return_count: 2_000,
            continuous_fit_return_count: 1_980,
            intraday_jump_candidate_count: 0,
            intraday_jump_episode_count: 0,
            selected_surprise_jump_episode_count: 0,
            close_open_gap_candidate_count: 0,
            close_open_gap_episode_count: 0,
            open_session_sell_share_proxy: 0.51,
            mean_continuous_business_time_scale: 1.0,
            continuous_return_mean: 0.0,
            continuous_return_volatility: 2.5e-6,
            continuous_return_skewness: -0.15,
            continuous_return_excess_kurtosis: 1.2,
            observed_log_price_span: 0.012,
            mean_abs_known_open_jump_return: 0.0008,
            mean_abs_known_closed_gap_return: 0.0,
            mean_abs_surprise_open_jump_return: 0.0,
            mean_abs_surprise_closed_gap_return: 0.0,
            mean_known_post_shock_activity_lift: 1.05,
            mean_surprise_post_shock_activity_lift: 0.0,
            known_activity_coupling_target: 0.05,
            surprise_activity_coupling_target: 0.0,
            known_news_coverage_ratio: 1.0,
            surprise_arrival_intensity_per_hour: 0.0,
        };
        let sorted_signals = ReturnComponentsStageSortedSignalInputs {
            known_return_samples: Vec::new(),
            observed_open_session_returns: vec![
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 0),
                    log_return: 1.0e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 1),
                    log_return: -1.0e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
            ],
            continuous_fit_returns: vec![
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 0),
                    log_return: 1.0e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 1),
                    log_return: -1.0e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
            ],
            intraday_jump_candidates: Vec::new(),
            surprise_jump_episodes: Vec::new(),
            selected_surprise_jump_episodes: Vec::new(),
            close_open_gap_candidates: Vec::new(),
            surprise_open_jump_magnitudes: Vec::new(),
            combined_gap_magnitudes: Vec::new(),
            source_window: test_source_window(),
        };
        let calibration_sample = continuous_fit::build_continuous_return_calibration_sample(
            &sorted_signals,
            &[1.0, 1.0],
            summary_statistics.open_session_sell_share_proxy,
        );
        let solver_settings =
            CalibrationOptimizerSettings::try_new("adaptive_search", 32, 1, 1.0e-8, 1.0e-8, 1.0)
                .expect("solver settings should validate");

        let nig_candidates =
            continuous_fit::build_nig_search_candidates(&calibration_sample, 17, &solver_settings);

        assert!(!nig_candidates.is_empty());
        assert!(
            nig_candidates
                .iter()
                .all(|candidate| candidate.configuration.delta
                    >= RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER)
        );
    }

    #[test]
    fn return_distribution_summary_normalizes_each_return_by_business_time_scale() {
        let summary = summarize_return_distribution(&[0.01, 0.04], &[1.0, 4.0]);

        assert!((summary.mean_return - 0.01).abs() <= 1.0e-12);
        assert!((summary.volatility - 0.007071067811865475).abs() <= 1.0e-12);
        assert!(summary.volatility < sample_standard_deviation(&[0.01, 0.04]));
    }

    #[test]
    fn zero_inflated_open_returns_keep_small_non_zero_moves_in_continuous_fit_sample() {
        let candidate_generation_input = ReturnComponentsStageCandidateGenerationInput {
            known_news_calendar: ReturnComponentsKnownNewsCalendarInput {
                source_window: test_source_window(),
                scheduled_event_timestamps_utc_nanos: Vec::new(),
            },
            realized_known_news_return_samples: Vec::new(),
            observed_open_session_returns: vec![
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 0),
                    log_return: 0.0,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 1),
                    log_return: 0.0,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 2),
                    log_return: 1.0e-4,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.0,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 3),
                    log_return: -2.0e-4,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 1.0,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 4),
                    log_return: 3.0e-4,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.0,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 5),
                    log_return: 5.0e-3,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 1.0,
                },
            ],
            detected_intraday_jump_candidates: vec![
                ReturnComponentsDetectedIntradayJumpCandidate {
                    jump_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 5),
                    jump_return: 5.0e-3,
                    post_shock_activity_lift: 1.0,
                },
            ],
            observed_close_open_boundary_observations: Vec::new(),
            close_open_gap_detection_policy: ReturnComponentsCloseOpenGapDetectionPolicy::new(
                0.005,
            ),
            open_session_sell_share_proxy: 0.5,
            shared_mark_law_template: SharedNewsMarkLawConfiguration {
                gamma_severity_marks: GammaSeverityMarkConfiguration {
                    shape_k: 2.0,
                    scale_theta: 1.0e-4,
                },
                double_exponential_base_shock: DoubleExponentialBaseShockConfiguration {
                    probability_positive: 0.45,
                    eta_plus: 8.0,
                    eta_minus: 10.0,
                },
                coupled_gap_marks: CoupledGapMarkConfiguration { sigma_g: 0.001 },
            },
            objective_metric_weights: ReturnComponentsStageObjectiveMetricWeights {
                continuous_distribution_fit_weight: 1.0,
                surprise_arrival_fit_weight: 1.0,
                jump_fit_weight: 1.0,
                gap_fit_weight: 1.0,
                activity_coupling_fit_weight: 1.0,
                price_range_sanity_fit_weight: 0.1,
            },
        };

        let fitted_signal_partition_layer = signal_partition_fit::fit_signal_partition_layer(
            candidate_generation_input
                .observed_open_session_returns
                .as_slice(),
        );
        let fitted_surprise_episode_selection_layer =
            surprise_episode_selection_fit::fit_surprise_episode_selection_layer(
                candidate_generation_input
                    .detected_intraday_jump_candidates
                    .as_slice(),
                fitted_signal_partition_layer.continuous_return_filter_threshold,
            );
        let sorted_signals = sort_candidate_generation_signals(
            &candidate_generation_input,
            &[],
            fitted_signal_partition_layer,
            &fitted_surprise_episode_selection_layer,
        );
        let retained_non_zero_returns: Vec<f64> = sorted_signals
            .continuous_fit_returns
            .iter()
            .map(|observation| observation.log_return)
            .filter(|return_value| *return_value != 0.0)
            .collect();

        assert_eq!(sorted_signals.continuous_fit_returns.len(), 5);
        assert_eq!(retained_non_zero_returns, vec![1.0e-4, -2.0e-4, 3.0e-4]);
    }

    #[test]
    fn surprise_jump_episode_clustering_merges_adjacent_candidates() {
        let fitted_layer = surprise_episode_selection_fit::fit_surprise_episode_selection_layer(
            &[
                ReturnComponentsDetectedIntradayJumpCandidate {
                    jump_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 0),
                    jump_return: 2.0e-4,
                    post_shock_activity_lift: 1.05,
                },
                ReturnComponentsDetectedIntradayJumpCandidate {
                    jump_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 1),
                    jump_return: -1.0e-4,
                    post_shock_activity_lift: 1.25,
                },
                ReturnComponentsDetectedIntradayJumpCandidate {
                    jump_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 15, 0),
                    jump_return: 5.0e-4,
                    post_shock_activity_lift: 1.10,
                },
            ],
            1.0e-4,
        );
        let episodes = fitted_layer.surprise_jump_episodes;

        assert_eq!(episodes.len(), 2);
        assert!((episodes[0].cumulative_jump_return - 1.0e-4).abs() <= 1.0e-12);
        assert_eq!(episodes[0].post_shock_activity_lift, 1.25);
        assert!((episodes[1].cumulative_jump_return - 5.0e-4).abs() <= 1.0e-12);
    }

    #[test]
    fn exact_candidate_evaluation_matches_stage_bucket_construction() {
        let candidate_generation_input = ReturnComponentsStageCandidateGenerationInput {
            known_news_calendar: ReturnComponentsKnownNewsCalendarInput {
                source_window: test_source_window(),
                scheduled_event_timestamps_utc_nanos: Vec::new(),
            },
            realized_known_news_return_samples: Vec::new(),
            observed_open_session_returns: vec![
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 0),
                    log_return: 1.0e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.4,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 1),
                    log_return: -2.0e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.6,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 2),
                    log_return: 1.5e-6,
                    post_shock_activity_lift: 1.0,
                    sell_share_proxy: 0.5,
                },
            ],
            detected_intraday_jump_candidates: Vec::new(),
            observed_close_open_boundary_observations: Vec::new(),
            close_open_gap_detection_policy: ReturnComponentsCloseOpenGapDetectionPolicy::new(
                0.005,
            ),
            open_session_sell_share_proxy: 0.5,
            shared_mark_law_template: SharedNewsMarkLawConfiguration {
                gamma_severity_marks: GammaSeverityMarkConfiguration {
                    shape_k: 2.0,
                    scale_theta: 1.0e-4,
                },
                double_exponential_base_shock: DoubleExponentialBaseShockConfiguration {
                    probability_positive: 0.45,
                    eta_plus: 8.0,
                    eta_minus: 10.0,
                },
                coupled_gap_marks: CoupledGapMarkConfiguration { sigma_g: 0.001 },
            },
            objective_metric_weights: ReturnComponentsStageObjectiveMetricWeights {
                continuous_distribution_fit_weight: 1.0,
                surprise_arrival_fit_weight: 1.0,
                jump_fit_weight: 1.0,
                gap_fit_weight: 1.0,
                activity_coupling_fit_weight: 1.0,
                price_range_sanity_fit_weight: 0.1,
            },
        };
        let fitted_parameters = ReturnComponentsStageFittedParameters {
            nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration {
                alpha: 100.0,
                beta_0: 0.0,
                delta: 1.0e-6,
                mu: 0.0,
                delta_beta: -1.0,
            },
            news_process_configuration: NewsProcessConfiguration::try_new(
                NewsArrivalConfiguration {
                    known: KnownNewsArrivalConfiguration {
                        scheduled_event_timestamps_utc_nanos: Vec::new(),
                    },
                    surprise: SurpriseNewsArrivalConfiguration {
                        baseline_intensity: 0.0,
                    },
                },
                candidate_generation_input.shared_mark_law_template,
                NewsComponentMarkScaleConfiguration {
                    activity_coupling_scale: 0.0,
                    return_jump_scale: 1.0,
                },
                NewsComponentMarkScaleConfiguration {
                    activity_coupling_scale: 0.0,
                    return_jump_scale: 1.0,
                },
            )
            .expect("news process configuration should validate"),
            surprise_deterministic_intensity_profile:
                default_surprise_deterministic_intensity_profile(),
        };
        let fitted_signal_partition_layer = signal_partition_fit::fit_signal_partition_layer(
            candidate_generation_input
                .observed_open_session_returns
                .as_slice(),
        );
        let fitted_surprise_episode_selection_layer =
            surprise_episode_selection_fit::fit_surprise_episode_selection_layer(
                candidate_generation_input
                    .detected_intraday_jump_candidates
                    .as_slice(),
                fitted_signal_partition_layer.continuous_return_filter_threshold,
            );
        let sorted_signals = sort_candidate_generation_signals(
            &candidate_generation_input,
            &[],
            fitted_signal_partition_layer,
            &fitted_surprise_episode_selection_layer,
        );
        let (summary_statistics, calibration_sample) = summarize_candidate_generation_signals(
            &candidate_generation_input,
            &sorted_signals,
            None,
        );
        let exact_evaluation = evaluate_return_components_exact_candidate(
            candidate_generation_input.clone(),
            None,
            summary_statistics,
            fitted_parameters.clone(),
            17,
        )
        .expect("exact evaluation should succeed");
        let continuous_evaluation = continuous_fit::evaluate_continuous_nig_candidate(
            fitted_parameters.nig_shared_plus_shift_configuration,
            &calibration_sample,
            summary_statistics,
            17,
        );
        let surprise_arrival_family = ReturnComponentsSurpriseNewsArrivalCandidateFamily {
            baseline_intensity: fitted_parameters
                .news_process_configuration
                .arrivals
                .surprise
                .baseline_intensity,
            weekly_hour_utc_multipliers: fitted_parameters
                .surprise_deterministic_intensity_profile
                .weekly_hour_utc_multipliers
                .clone(),
            profile_resolution_hours: fitted_parameters
                .surprise_deterministic_intensity_profile
                .profile_resolution_hours,
        };
        let return_jump_family = ReturnComponentsReturnJumpMarkScaleCandidateFamily {
            sigma_g: fitted_parameters
                .news_process_configuration
                .shared_mark_law
                .coupled_gap_marks
                .sigma_g,
        };
        let activity_coupling_family = ReturnComponentsActivityCouplingCandidateFamily {
            known_activity_coupling_scale: fitted_parameters
                .news_process_configuration
                .known_component_mark_scales
                .activity_coupling_scale,
            surprise_activity_coupling_scale: fitted_parameters
                .news_process_configuration
                .surprise_component_mark_scales
                .activity_coupling_scale,
        };
        let fitted_diagnostic_policy = diagnostic_policy_fit::fit_diagnostic_policy(
            RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT,
        );
        let fitted_objective_scoring_policy =
            objective_scoring_policy_fit::fit_objective_scoring_policy(
                RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER,
            );
        let expected_jump_diagnostic = build_jump_diagnostic(
            &fitted_diagnostic_policy,
            summary_statistics,
            &return_jump_family,
            candidate_generation_input.shared_mark_law_template,
        );
        let expected_surprise_arrival_diagnostic = build_surprise_arrival_diagnostic(
            &fitted_diagnostic_policy,
            summary_statistics,
            &surprise_arrival_family,
            &fitted_parameters.surprise_deterministic_intensity_profile,
        );
        let expected_gap_diagnostic = build_gap_diagnostic(
            &fitted_diagnostic_policy,
            summary_statistics,
            &return_jump_family,
            candidate_generation_input.shared_mark_law_template,
        );
        let expected_activity_coupling_diagnostic = build_activity_coupling_diagnostic(
            &fitted_diagnostic_policy,
            summary_statistics,
            &activity_coupling_family,
        );
        let fitted_price_range_sanity_policy =
            price_range_policy_fit::fit_price_range_sanity_policy();
        let expected_price_range_sanity_diagnostic = build_price_range_sanity_diagnostic(
            &fitted_diagnostic_policy,
            summary_statistics,
            continuous_evaluation.diagnostic,
            &surprise_arrival_family,
            &return_jump_family,
            candidate_generation_input.shared_mark_law_template,
            &fitted_price_range_sanity_policy,
        );
        let expected_buckets = objective_scoring_policy_fit::score_candidate_metric_buckets(
            &fitted_objective_scoring_policy,
            continuous_evaluation.objective_score,
            &expected_surprise_arrival_diagnostic,
            &expected_jump_diagnostic,
            &expected_gap_diagnostic,
            &expected_activity_coupling_diagnostic,
            &expected_price_range_sanity_diagnostic,
            fitted_diagnostic_policy.shock_evidence_min_count,
        );

        assert_eq!(exact_evaluation.objective_metric_buckets, expected_buckets);
        assert_eq!(
            exact_evaluation.return_distribution_diagnostic,
            continuous_evaluation.diagnostic
        );
        assert_eq!(
            exact_evaluation.surprise_arrival_diagnostic,
            expected_surprise_arrival_diagnostic
        );
        assert_eq!(exact_evaluation.jump_diagnostic, expected_jump_diagnostic);
        assert_eq!(exact_evaluation.gap_diagnostic, expected_gap_diagnostic);
        assert_eq!(
            exact_evaluation.activity_coupling_diagnostic,
            expected_activity_coupling_diagnostic
        );
        assert_eq!(
            exact_evaluation.price_range_sanity_diagnostic,
            expected_price_range_sanity_diagnostic
        );
    }

    fn test_source_window() -> ReturnComponentsStageSignalSourceWindow {
        ReturnComponentsStageSignalSourceWindow {
            window_start_utc_nanos: utc_nanos(2026, 2, 2, 13, 0, 0),
            window_end_utc_nanos: utc_nanos(2026, 2, 10, 0, 0, 0),
        }
    }

    fn test_calendar_registry() -> ExchangeCalendarRegistry {
        ExchangeCalendarRegistryLoader::load_from_json(
            r#"
{
  "schema_version": "calendar.registry.v1",
  "artifact_date": "2026-01-31",
  "exchanges": [
    {
      "schema_version": "calendar.v1",
      "exchange_code": "XNYS",
      "exchange_name": "New York Stock Exchange",
      "timezone_name": "America/New_York",
      "rule_metadata": {
        "holiday_calendar_code": "US_EQUITIES",
        "early_close_rule_set": "US_EQUITIES_EARLY_CLOSE"
      },
      "regular_sessions": [
        {"weekday": "Monday", "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]},
        {"weekday": "Tuesday", "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]},
        {"weekday": "Wednesday", "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]},
        {"weekday": "Thursday", "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]},
        {"weekday": "Friday", "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]}
      ],
      "holiday_dates": [],
      "early_close_sessions": []
    }
  ]
}
"#,
        )
        .expect("calendar registry fixture should load")
    }

    fn utc_nanos(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
        Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
            .single()
            .expect("fixture timestamp should be valid")
            .timestamp_nanos_opt()
            .expect("fixture timestamp nanos should fit in i64")
    }
}
