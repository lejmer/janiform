use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, Utc};

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
    resolve_calibration_stage_seed_metadata, stage_solver_settings_for_stage,
};
use janiform_model_kernels::news::{
    CoupledGapMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration, NewsProcessConfiguration, SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

use super::constants::*;
use super::continuous_fit;
use super::contracts::*;

struct ReturnComponentsStageCandidateGenerationOutput {
    candidate_inputs: Vec<ReturnComponentsStageCandidateInput>,
    candidate_family_counts: ReturnComponentsStageCandidateFamilyCounts,
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
    candidate_generation_input.validate()?;

    let sorted_signals = sort_candidate_generation_signals(
        candidate_generation_input,
        derived_close_open_gap_candidates,
    );
    let (summary_statistics, continuous_return_calibration_sample) =
        summarize_candidate_generation_signals(
            candidate_generation_input,
            &sorted_signals,
            activity_calibration_context,
        );

    let jump_abs_return_bin_edges = deterministic_quantile_bins(
        sorted_signals.surprise_open_jump_magnitudes.as_slice(),
        &RETURN_COMPONENTS_STAGE_JUMP_BIN_QUANTILES,
    );
    let gap_abs_return_bin_edges = deterministic_quantile_bins(
        sorted_signals.combined_gap_magnitudes.as_slice(),
        &RETURN_COMPONENTS_STAGE_GAP_BIN_QUANTILES,
    );

    let candidate_family_sets = build_candidate_family_sets(
        candidate_generation_input,
        &sorted_signals,
        summary_statistics,
        &continuous_return_calibration_sample,
        stage_seed,
        stage_solver_settings,
        jump_abs_return_bin_edges.as_slice(),
        gap_abs_return_bin_edges.as_slice(),
    );

    let candidate_provenance_template = build_candidate_provenance_template(
        sorted_signals.source_window,
        stage_seed,
        jump_abs_return_bin_edges,
        gap_abs_return_bin_edges,
        summary_statistics,
        close_open_gap_detection_filter,
    );
    let candidate_family_counts = build_candidate_family_counts(&candidate_family_sets);
    let generated_candidates = enumerate_candidate_inputs(
        candidate_generation_input,
        &candidate_family_sets,
        summary_statistics,
        &continuous_return_calibration_sample,
        stage_seed,
        &candidate_provenance_template,
    )?;

    Ok(ReturnComponentsStageCandidateGenerationOutput {
        candidate_inputs: generated_candidates,
        candidate_family_counts,
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
    pub(super) close_open_gap_candidates: Vec<ReturnComponentsDetectedCloseOpenGapCandidate>,
    pub(super) known_sample_magnitudes: Vec<f64>,
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
    nig_candidates: Vec<NigSharedPlusShiftConfiguration>,
    known_arrival_families: Vec<ReturnComponentsKnownNewsArrivalCandidateFamily>,
    surprise_arrival_families: Vec<ReturnComponentsSurpriseNewsArrivalCandidateFamily>,
    return_jump_mark_scale_families: Vec<ReturnComponentsReturnJumpMarkScaleCandidateFamily>,
    activity_coupling_families: Vec<ReturnComponentsActivityCouplingCandidateFamily>,
}

#[derive(Debug, Clone, Copy)]
struct ReturnComponentsStageReturnDistributionSummary {
    mean_return: f64,
    volatility: f64,
    skewness: f64,
    excess_kurtosis: f64,
}

fn sort_candidate_generation_signals(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    derived_close_open_gap_candidates: &[ReturnComponentsDetectedCloseOpenGapCandidate],
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

    let known_sample_magnitudes: Vec<f64> = known_return_samples
        .iter()
        .map(|sample| {
            sample
                .open_window_jump_return
                .abs()
                .max(sample.closed_window_gap_return.abs())
        })
        .collect();

    let surprise_open_jump_magnitudes: Vec<f64> = intraday_jump_candidates
        .iter()
        .map(|candidate| candidate.jump_return.abs())
        .collect();

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

    let jump_candidate_timestamps: BTreeSet<i64> = intraday_jump_candidates
        .iter()
        .map(|candidate| candidate.jump_timestamp_utc_nanos)
        .collect();
    let continuous_filter_threshold = (median_abs(
        observed_open_session_returns
            .iter()
            .map(|observation| observation.log_return)
            .collect::<Vec<_>>()
            .as_slice(),
    )
        * RETURN_COMPONENTS_STAGE_CONTINUOUS_RETURN_FILTER_MULTIPLIER)
        .max(1.0e-8);

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
                ) && !jump_candidate_timestamps.contains(&observation.return_timestamp_utc_nanos)
                    && observation.log_return.abs() <= continuous_filter_threshold
            })
            .collect();
    if continuous_fit_returns.is_empty() {
        continuous_fit_returns = observed_open_session_returns
            .iter()
            .copied()
            .filter(|observation| observation.log_return.abs() <= continuous_filter_threshold)
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
        close_open_gap_candidates,
        known_sample_magnitudes,
        surprise_open_jump_magnitudes,
        combined_gap_magnitudes,
        source_window: candidate_generation_input.known_news_calendar.source_window,
    }
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
    let mean_continuous_business_time_scale =
        arithmetic_mean(continuous_fit_business_time_scales.as_slice()).max(1.0e-8);
    let continuous_return_calibration_sample =
        continuous_fit::build_continuous_return_calibration_sample(
            sorted_signals,
            continuous_fit_business_time_scales.as_slice(),
            candidate_generation_input.open_session_sell_share_proxy,
        );
    let continuous_distribution = summarize_return_distribution(
        sorted_signals
            .continuous_fit_returns
            .iter()
            .map(|observation| observation.log_return)
            .collect::<Vec<_>>()
            .as_slice(),
        continuous_fit_business_time_scales.as_slice(),
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

    (
        ReturnComponentsStageObservedSignalSummaryStatistics {
            known_calendar_event_count: candidate_generation_input
                .known_news_calendar
                .scheduled_event_timestamps_utc_nanos
                .len(),
            known_realized_sample_count: sorted_signals.known_return_samples.len(),
            source_window_duration_hours,
            observed_open_session_return_count: sorted_signals.observed_open_session_returns.len(),
            continuous_fit_return_count: sorted_signals.continuous_fit_returns.len(),
            intraday_jump_candidate_count: sorted_signals.intraday_jump_candidates.len(),
            close_open_gap_candidate_count: sorted_signals.close_open_gap_candidates.len(),
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
                sorted_signals.surprise_open_jump_magnitudes.as_slice(),
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
                sorted_signals
                    .intraday_jump_candidates
                    .iter()
                    .map(|candidate| candidate.post_shock_activity_lift)
                    .chain(
                        sorted_signals
                            .close_open_gap_candidates
                            .iter()
                            .map(|candidate| candidate.post_shock_activity_lift),
                    )
                    .collect::<Vec<_>>()
                    .as_slice(),
            ),
            known_news_coverage_ratio: sorted_signals.known_return_samples.len() as f64
                / candidate_generation_input
                    .known_news_calendar
                    .scheduled_event_timestamps_utc_nanos
                    .len()
                    .max(1) as f64,
            surprise_arrival_intensity_per_hour: (sorted_signals.intraday_jump_candidates.len()
                + sorted_signals.close_open_gap_candidates.len())
                as f64
                / source_window_duration_hours,
        },
        continuous_return_calibration_sample,
    )
}

fn build_candidate_family_sets(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    sorted_signals: &ReturnComponentsStageSortedSignalInputs,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    continuous_return_calibration_sample: &continuous_fit::ReturnComponentsStageContinuousReturnCalibrationSample,
    stage_seed: u64,
    stage_solver_settings: &CalibrationOptimizerSettings,
    jump_abs_return_bin_edges: &[f64],
    gap_abs_return_bin_edges: &[f64],
) -> ReturnComponentsStageCandidateFamilySets {
    let nig_candidates = rank_and_truncate_candidates(
        continuous_fit::build_nig_search_candidates(
            summary_statistics,
            continuous_return_calibration_sample,
            stage_seed,
            stage_solver_settings,
        ),
        RETURN_COMPONENTS_STAGE_MAX_NIG_CANDIDATES,
        |candidate| {
            continuous_fit::evaluate_continuous_nig_candidate(
                *candidate,
                continuous_return_calibration_sample,
                summary_statistics,
                stage_seed,
            )
            .objective_score
        },
    );
    let known_arrival_families = build_known_arrival_families(
        candidate_generation_input,
        sorted_signals.known_sample_magnitudes.as_slice(),
        sorted_signals.known_return_samples.as_slice(),
    );
    let surprise_arrival_families = build_surprise_arrival_families(
        summary_statistics.surprise_arrival_intensity_per_hour,
        summary_statistics.intraday_jump_candidate_count
            + summary_statistics.close_open_gap_candidate_count,
    );
    let return_jump_mark_scale_families = rank_and_truncate_candidates(
        build_return_jump_mark_scale_families(
            candidate_generation_input,
            summary_statistics,
            jump_abs_return_bin_edges,
            gap_abs_return_bin_edges,
        ),
        RETURN_COMPONENTS_STAGE_MAX_RETURN_JUMP_MARK_SCALE_FAMILIES,
        |family| {
            rank_return_jump_mark_scale_family(
                summary_statistics,
                candidate_generation_input.shared_mark_law_template,
                family,
            )
        },
    );
    let activity_coupling_families = rank_and_truncate_candidates(
        build_activity_coupling_families(summary_statistics),
        RETURN_COMPONENTS_STAGE_MAX_ACTIVITY_COUPLING_FAMILIES,
        |family| rank_activity_coupling_family(summary_statistics, family),
    );

    ReturnComponentsStageCandidateFamilySets {
        nig_candidates,
        known_arrival_families,
        surprise_arrival_families,
        return_jump_mark_scale_families,
        activity_coupling_families,
    }
}

fn build_known_arrival_families(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    known_sample_magnitudes: &[f64],
    known_return_samples: &[ReturnComponentsRealizedKnownNewsReturnSample],
) -> Vec<ReturnComponentsKnownNewsArrivalCandidateFamily> {
    let known_magnitude_by_timestamp: BTreeMap<i64, f64> = known_return_samples
        .iter()
        .map(|sample| {
            (
                sample.event_timestamp_utc_nanos,
                sample
                    .open_window_jump_return
                    .abs()
                    .max(sample.closed_window_gap_return.abs()),
            )
        })
        .collect();

    let known_arrival_thresholds = deterministic_quantile_bins(
        known_sample_magnitudes,
        &RETURN_COMPONENTS_STAGE_KNOWN_RETENTION_QUANTILES,
    );

    known_arrival_thresholds
        .into_iter()
        .map(|threshold| {
            let retained_known_event_timestamps_utc_nanos = candidate_generation_input
                .known_news_calendar
                .scheduled_event_timestamps_utc_nanos
                .iter()
                .copied()
                .filter(|timestamp_utc_nanos| {
                    known_magnitude_by_timestamp
                        .get(timestamp_utc_nanos)
                        .copied()
                        .unwrap_or(f64::MAX)
                        >= threshold
                })
                .collect();

            ReturnComponentsKnownNewsArrivalCandidateFamily {
                minimum_abs_known_return_threshold: threshold,
                retained_known_event_timestamps_utc_nanos,
            }
        })
        .collect()
}

fn build_surprise_arrival_families(
    surprise_arrival_intensity_per_hour: f64,
    surprise_evidence_count: usize,
) -> Vec<ReturnComponentsSurpriseNewsArrivalCandidateFamily> {
    if surprise_evidence_count < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT {
        return vec![ReturnComponentsSurpriseNewsArrivalCandidateFamily {
            baseline_intensity: 0.0,
        }];
    }

    let mut surprise_intensity_candidates = deduplicate_sorted_f64_values(vec![
        round_candidate_value(surprise_arrival_intensity_per_hour * 0.75),
        round_candidate_value(surprise_arrival_intensity_per_hour),
        round_candidate_value(surprise_arrival_intensity_per_hour * 1.25),
    ]);
    if surprise_intensity_candidates.is_empty() {
        surprise_intensity_candidates.push(0.0);
    }

    surprise_intensity_candidates
        .into_iter()
        .map(
            |baseline_intensity| ReturnComponentsSurpriseNewsArrivalCandidateFamily {
                baseline_intensity,
            },
        )
        .collect()
}

fn build_return_jump_mark_scale_families(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    jump_abs_return_bin_edges: &[f64],
    gap_abs_return_bin_edges: &[f64],
) -> Vec<ReturnComponentsReturnJumpMarkScaleCandidateFamily> {
    let total_shock_evidence_count = summary_statistics.known_realized_sample_count
        + summary_statistics.intraday_jump_candidate_count
        + summary_statistics.close_open_gap_candidate_count;
    if total_shock_evidence_count < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT {
        return vec![ReturnComponentsReturnJumpMarkScaleCandidateFamily { sigma_g: 0.0 }];
    }

    let weighted_shock_magnitude_sum = summary_statistics.known_realized_sample_count as f64
        * summary_statistics.mean_abs_known_open_jump_return
        + summary_statistics.known_realized_sample_count as f64
            * summary_statistics.mean_abs_known_closed_gap_return
        + summary_statistics.intraday_jump_candidate_count as f64
            * arithmetic_mean(jump_abs_return_bin_edges)
                .max(summary_statistics.mean_abs_surprise_open_jump_return)
        + summary_statistics.close_open_gap_candidate_count as f64
            * arithmetic_mean(gap_abs_return_bin_edges)
                .max(summary_statistics.mean_abs_surprise_closed_gap_return);
    let weighted_shock_magnitude_count = (summary_statistics.known_realized_sample_count * 2
        + summary_statistics.intraday_jump_candidate_count
        + summary_statistics.close_open_gap_candidate_count)
        .max(1) as f64;
    let observed_mean_abs_shock_magnitude =
        weighted_shock_magnitude_sum / weighted_shock_magnitude_count;
    let shared_mark_multiplier = shared_mark_law_mean_abs_jump_multiplier(
        candidate_generation_input.shared_mark_law_template,
    )
    .max(1.0e-8);
    let sigma_g_target = observed_mean_abs_shock_magnitude / shared_mark_multiplier;
    let mut sigma_g_values = scale_candidates_from_target(sigma_g_target, false);
    if sigma_g_values.is_empty() {
        sigma_g_values.push(round_candidate_value(
            candidate_generation_input
                .shared_mark_law_template
                .coupled_gap_marks
                .sigma_g,
        ));
    }

    sigma_g_values
        .into_iter()
        .map(|sigma_g| ReturnComponentsReturnJumpMarkScaleCandidateFamily { sigma_g })
        .collect()
}

fn build_activity_coupling_families(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
) -> Vec<ReturnComponentsActivityCouplingCandidateFamily> {
    let known_activity_values =
        scale_candidates_from_target(summary_statistics.mean_known_post_shock_activity_lift, true);
    let surprise_activity_values = if summary_statistics.intraday_jump_candidate_count
        + summary_statistics.close_open_gap_candidate_count
        < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT
    {
        vec![0.0]
    } else {
        scale_candidates_from_target(
            summary_statistics.mean_surprise_post_shock_activity_lift,
            true,
        )
    };

    let mut families = Vec::new();
    for known_activity_coupling_scale in &known_activity_values {
        for surprise_activity_coupling_scale in &surprise_activity_values {
            families.push(ReturnComponentsActivityCouplingCandidateFamily {
                known_activity_coupling_scale: *known_activity_coupling_scale,
                surprise_activity_coupling_scale: *surprise_activity_coupling_scale,
            });
        }
    }

    families
}

fn build_candidate_provenance_template(
    source_window: ReturnComponentsStageSignalSourceWindow,
    stage_seed: u64,
    jump_abs_return_bin_edges: Vec<f64>,
    gap_abs_return_bin_edges: Vec<f64>,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    close_open_gap_detection_filter: &str,
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
            "exclude_detected_material_shock_returns_from_continuous_nig_fit".to_string(),
            "reject_non_finite_intraday_jump_returns".to_string(),
            "reject_non_finite_and_non_positive_close_open_boundary_prices".to_string(),
            "enforce_non_negative_post_shock_activity_lift".to_string(),
            "sort_observed_open_session_returns_by_timestamp".to_string(),
            "sort_intraday_jumps_by_jump_timestamp".to_string(),
            "validate_close_open_session_boundary_transitions".to_string(),
            close_open_gap_detection_filter.to_string(),
            "sort_close_open_gaps_by_close_then_open_timestamp".to_string(),
            "deterministic_nearest_rank_quantile_binning".to_string(),
        ],
        jump_abs_return_bin_edges,
        gap_abs_return_bin_edges,
        summary_statistics,
        deterministic_seed_metadata: ReturnComponentsStageDeterministicSeedMetadata {
            stage_seed,
            candidate_generation_seed: stage_seed,
            deterministic_seed_policy: RETURN_COMPONENTS_STAGE_DETERMINISTIC_SEED_POLICY,
        },
    }
}

fn build_candidate_family_counts(
    candidate_family_sets: &ReturnComponentsStageCandidateFamilySets,
) -> ReturnComponentsStageCandidateFamilyCounts {
    ReturnComponentsStageCandidateFamilyCounts {
        nig_candidate_count: candidate_family_sets.nig_candidates.len(),
        known_news_arrival_family_count: candidate_family_sets.known_arrival_families.len(),
        surprise_news_arrival_family_count: candidate_family_sets.surprise_arrival_families.len(),
        return_jump_mark_scale_family_count: candidate_family_sets
            .return_jump_mark_scale_families
            .len(),
        activity_coupling_family_count: candidate_family_sets.activity_coupling_families.len(),
    }
}

fn rank_and_truncate_candidates<T, F>(
    candidates: Vec<T>,
    maximum_count: usize,
    mut score_candidate: F,
) -> Vec<T>
where
    F: FnMut(&T) -> f64,
{
    let retained_count = maximum_count.max(1);
    let mut ranked_candidates = candidates
        .into_iter()
        .enumerate()
        .map(|(position, candidate)| (score_candidate(&candidate), position, candidate))
        .collect::<Vec<_>>();
    ranked_candidates.sort_by(|left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)));
    ranked_candidates.truncate(retained_count);
    ranked_candidates
        .into_iter()
        .map(|(_, _, candidate)| candidate)
        .collect()
}

fn rank_return_jump_mark_scale_family(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
    family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
) -> f64 {
    let fitted_mean_abs_jump_return = fitted_mean_abs_jump_return(shared_mark_law_template, family);

    zero_aware_relative_error(
        fitted_mean_abs_jump_return,
        summary_statistics.mean_abs_known_open_jump_return,
        1.0e-6,
    ) + zero_aware_relative_error(
        fitted_mean_abs_jump_return,
        if summary_statistics.intraday_jump_candidate_count
            < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT
        {
            0.0
        } else {
            summary_statistics.mean_abs_surprise_open_jump_return
        },
        1.0e-6,
    ) + zero_aware_relative_error(
        fitted_mean_abs_jump_return,
        if summary_statistics.close_open_gap_candidate_count
            < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT
        {
            0.0
        } else {
            summary_statistics.mean_abs_surprise_closed_gap_return
        },
        1.0e-6,
    ) + zero_aware_relative_error(
        fitted_mean_abs_jump_return,
        summary_statistics.mean_abs_known_closed_gap_return,
        1.0e-6,
    )
}

fn rank_activity_coupling_family(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    family: &ReturnComponentsActivityCouplingCandidateFamily,
) -> f64 {
    let surprise_evidence_count = summary_statistics.intraday_jump_candidate_count
        + summary_statistics.close_open_gap_candidate_count;

    zero_aware_relative_error(
        family.known_activity_coupling_scale,
        summary_statistics.mean_known_post_shock_activity_lift,
        1.0e-6,
    ) + zero_aware_relative_error(
        family.surprise_activity_coupling_scale,
        if surprise_evidence_count < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT {
            0.0
        } else {
            summary_statistics.mean_surprise_post_shock_activity_lift
        },
        1.0e-6,
    )
}

fn scale_candidates_from_target(target_value: f64, include_zero: bool) -> Vec<f64> {
    if target_value <= 0.0 || !target_value.is_finite() {
        return vec![0.0];
    }

    let mut values = deduplicate_sorted_f64_values(vec![
        if include_zero {
            0.0
        } else {
            target_value * 0.75
        },
        round_candidate_value(target_value * 0.75),
        round_candidate_value(target_value),
        round_candidate_value(target_value * 1.25),
    ]);
    if values.is_empty() {
        values.push(round_candidate_value(target_value));
    }

    values
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

fn summarize_return_distribution(
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
) -> bool {
    if known_news_timestamps.is_empty() {
        return false;
    }

    let window_nanos =
        Duration::minutes(RETURN_COMPONENTS_STAGE_KNOWN_NEWS_EXCLUSION_WINDOW_MINUTES)
            .num_nanoseconds()
            .expect("known-news exclusion window must fit in i64");
    let lower_bound = candidate_timestamp_utc_nanos - window_nanos;
    let upper_bound = candidate_timestamp_utc_nanos + window_nanos;
    let lower_index = known_news_timestamps.partition_point(|timestamp| *timestamp < lower_bound);

    lower_index < known_news_timestamps.len() && known_news_timestamps[lower_index] <= upper_bound
}

fn median_abs(values: &[f64]) -> f64 {
    let mut retained_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .map(f64::abs)
        .collect();
    if retained_values.is_empty() {
        return 0.0;
    }

    retained_values.sort_by(f64::total_cmp);
    let midpoint = retained_values.len() / 2;
    if retained_values.len() % 2 == 0 {
        (retained_values[midpoint - 1] + retained_values[midpoint]) / 2.0
    } else {
        retained_values[midpoint]
    }
}

fn prepare_candidate_generation_input(
    exchange_code: &str,
    calendar_registry: &ExchangeCalendarRegistry,
    candidate_generation_input: ReturnComponentsStageCandidateGenerationInput,
) -> Result<
    ReturnComponentsStagePreparedCandidateGenerationInput,
    ReturnComponentsStageCandidateGenerationInputError,
> {
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
    continuous_return_calibration_sample: &continuous_fit::ReturnComponentsStageContinuousReturnCalibrationSample,
    stage_seed: u64,
    candidate_provenance_template: &ReturnComponentsStageCandidateProvenance,
) -> Result<
    Vec<ReturnComponentsStageCandidateInput>,
    ReturnComponentsStageCandidateGenerationInputError,
> {
    let mut generated_candidates = Vec::new();

    for (nig_position, nig_candidate) in candidate_family_sets.nig_candidates.iter().enumerate() {
        for (known_arrival_position, known_arrival_family) in candidate_family_sets
            .known_arrival_families
            .iter()
            .enumerate()
        {
            for (surprise_arrival_position, surprise_arrival_family) in candidate_family_sets
                .surprise_arrival_families
                .iter()
                .enumerate()
            {
                for (return_jump_position, return_jump_family) in candidate_family_sets
                    .return_jump_mark_scale_families
                    .iter()
                    .enumerate()
                {
                    for (activity_coupling_position, activity_coupling_family) in
                        candidate_family_sets
                            .activity_coupling_families
                            .iter()
                            .enumerate()
                    {
                        let candidate_families = ReturnComponentsStageCandidateFamilies {
                            known_news_arrival_controls: known_arrival_family.clone(),
                            surprise_news_arrival_controls: *surprise_arrival_family,
                            return_jump_mark_scale_controls: *return_jump_family,
                            news_to_hawkes_activity_coupling_controls: *activity_coupling_family,
                        };

                        let continuous_return_evaluation =
                            continuous_fit::evaluate_continuous_nig_candidate(
                                *nig_candidate,
                                continuous_return_calibration_sample,
                                summary_statistics,
                                stage_seed,
                            );
                        let return_distribution_diagnostic =
                            continuous_return_evaluation.diagnostic;

                        let jump_diagnostic = build_jump_diagnostic(
                            summary_statistics,
                            surprise_arrival_family,
                            return_jump_family,
                            candidate_generation_input.shared_mark_law_template,
                        );
                        let gap_diagnostic = build_gap_diagnostic(
                            summary_statistics,
                            return_jump_family,
                            candidate_generation_input.shared_mark_law_template,
                        );
                        let activity_coupling_diagnostic = build_activity_coupling_diagnostic(
                            summary_statistics,
                            activity_coupling_family,
                        );
                        let price_range_sanity_diagnostic = build_price_range_sanity_diagnostic(
                            summary_statistics,
                            return_distribution_diagnostic,
                            surprise_arrival_family,
                            return_jump_family,
                            candidate_generation_input.shared_mark_law_template,
                        );
                        let objective_metric_buckets = score_candidate_metric_buckets(
                            continuous_return_evaluation.objective_score,
                            &jump_diagnostic,
                            &gap_diagnostic,
                            &activity_coupling_diagnostic,
                            &price_range_sanity_diagnostic,
                        );
                        let dominant_scale_driver =
                            dominant_scale_driver_from_metric_buckets(objective_metric_buckets)
                                .to_string();
                        let objective_terms = build_candidate_objective_terms(
                            candidate_generation_input.objective_metric_weights,
                            objective_metric_buckets,
                        );
                        let candidate_id = format!(
                            "generated_nig_{:03}_known_{:03}_surprise_{:03}_mark_{:03}_coupling_{:03}",
                            nig_position + 1,
                            known_arrival_position + 1,
                            surprise_arrival_position + 1,
                            return_jump_position + 1,
                            activity_coupling_position + 1,
                        );

                        let news_process_configuration =
                            build_news_process_configuration_from_candidate_families(
                                candidate_generation_input,
                                known_arrival_family,
                                surprise_arrival_family,
                                return_jump_family,
                                activity_coupling_family,
                            )?;

                        generated_candidates.push(ReturnComponentsStageCandidateInput {
                            candidate_id,
                            nig_shared_plus_shift_configuration: *nig_candidate,
                            news_process_configuration,
                            objective_metric_buckets,
                            return_distribution_diagnostic,
                            jump_diagnostic,
                            gap_diagnostic,
                            activity_coupling_diagnostic,
                            price_range_sanity_diagnostic,
                            dominant_scale_driver,
                            objective_terms,
                            candidate_families,
                            candidate_provenance: candidate_provenance_template.clone(),
                        });
                    }
                }
            }
        }
    }

    Ok(generated_candidates)
}

fn build_candidate_objective_terms(
    objective_metric_weights: ReturnComponentsStageObjectiveMetricWeights,
    objective_metric_buckets: ReturnComponentsStageObjectiveMetricBuckets,
) -> Vec<ReturnComponentsStageObjectiveTermInput> {
    vec![
        ReturnComponentsStageObjectiveTermInput {
            term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_CONTINUOUS_DISTRIBUTION_FIT.to_string(),
            weight: objective_metric_weights.continuous_distribution_fit_weight,
            observed_score: objective_metric_buckets.continuous_distribution_fit,
        },
        ReturnComponentsStageObjectiveTermInput {
            term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_OPEN_JUMP_FIT.to_string(),
            weight: objective_metric_weights.jump_fit_weight,
            observed_score: objective_metric_buckets.open_jump_fit,
        },
        ReturnComponentsStageObjectiveTermInput {
            term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_CLOSED_GAP_FIT.to_string(),
            weight: objective_metric_weights.gap_fit_weight,
            observed_score: objective_metric_buckets.closed_gap_fit,
        },
        ReturnComponentsStageObjectiveTermInput {
            term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_ACTIVITY_COUPLING_FIT.to_string(),
            weight: objective_metric_weights.activity_coupling_fit_weight,
            observed_score: objective_metric_buckets.activity_coupling_fit,
        },
        ReturnComponentsStageObjectiveTermInput {
            term_id: RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_PRICE_RANGE_SANITY_FIT.to_string(),
            weight: objective_metric_weights.price_range_sanity_fit_weight,
            observed_score: objective_metric_buckets.price_range_sanity_fit,
        },
    ]
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

fn score_candidate_metric_buckets(
    continuous_distribution_fit: f64,
    jump_diagnostic: &ReturnComponentsStageJumpDiagnostic,
    gap_diagnostic: &ReturnComponentsStageGapDiagnostic,
    activity_coupling_diagnostic: &ReturnComponentsStageActivityCouplingDiagnostic,
    price_range_sanity_diagnostic: &ReturnComponentsStagePriceRangeSanityDiagnostic,
) -> ReturnComponentsStageObjectiveMetricBuckets {
    let open_jump_fit = zero_aware_relative_error(
        jump_diagnostic.fitted_surprise_arrival_intensity_per_hour,
        jump_diagnostic.observed_jump_frequency_per_hour,
        1.0e-8,
    ) + zero_aware_relative_error(
        jump_diagnostic.fitted_mean_abs_jump_return,
        jump_diagnostic.observed_mean_abs_jump_return,
        1.0e-6,
    );
    let closed_gap_fit = zero_aware_relative_error(
        gap_diagnostic.fitted_mean_abs_gap_return,
        gap_diagnostic.observed_mean_abs_gap_return,
        1.0e-6,
    );
    let activity_coupling_fit = zero_aware_relative_error(
        activity_coupling_diagnostic.fitted_known_activity_coupling_scale,
        activity_coupling_diagnostic.observed_known_post_shock_activity_lift,
        1.0e-6,
    ) + zero_aware_relative_error(
        activity_coupling_diagnostic.fitted_surprise_activity_coupling_scale,
        activity_coupling_diagnostic.observed_surprise_post_shock_activity_lift,
        1.0e-6,
    );

    ReturnComponentsStageObjectiveMetricBuckets {
        continuous_distribution_fit: round_candidate_value(continuous_distribution_fit),
        open_jump_fit: round_candidate_value(open_jump_fit),
        closed_gap_fit: round_candidate_value(closed_gap_fit),
        activity_coupling_fit: round_candidate_value(activity_coupling_fit),
        price_range_sanity_fit: round_candidate_value(
            if price_range_sanity_diagnostic.is_flagged {
                price_range_sanity_diagnostic.expansion_ratio
                    / price_range_sanity_diagnostic.threshold_ratio.max(1.0)
            } else {
                0.0
            },
        ),
    }
}

fn build_jump_diagnostic(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    surprise_arrival_family: &ReturnComponentsSurpriseNewsArrivalCandidateFamily,
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
) -> ReturnComponentsStageJumpDiagnostic {
    let fitted_mean_abs_jump_return =
        fitted_mean_abs_jump_return(shared_mark_law_template, return_jump_family);

    ReturnComponentsStageJumpDiagnostic {
        evidence_state: evidence_state_from_count(summary_statistics.intraday_jump_candidate_count)
            .to_string(),
        observed_jump_count: summary_statistics.intraday_jump_candidate_count,
        observed_jump_frequency_per_hour: if summary_statistics.intraday_jump_candidate_count
            < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT
        {
            0.0
        } else {
            summary_statistics.surprise_arrival_intensity_per_hour
        },
        observed_mean_abs_jump_return: if summary_statistics.intraday_jump_candidate_count
            < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT
        {
            0.0
        } else {
            summary_statistics.mean_abs_surprise_open_jump_return
        },
        fitted_mean_abs_jump_return,
        fitted_sigma_g: return_jump_family.sigma_g,
        fitted_surprise_arrival_intensity_per_hour: surprise_arrival_family.baseline_intensity,
    }
}

fn build_gap_diagnostic(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
) -> ReturnComponentsStageGapDiagnostic {
    let fitted_mean_abs_gap_return =
        fitted_mean_abs_jump_return(shared_mark_law_template, return_jump_family);

    ReturnComponentsStageGapDiagnostic {
        evidence_state: evidence_state_from_count(
            summary_statistics.close_open_gap_candidate_count,
        )
        .to_string(),
        observed_gap_count: summary_statistics.close_open_gap_candidate_count,
        observed_gap_frequency_per_hour: if summary_statistics.close_open_gap_candidate_count
            < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT
        {
            0.0
        } else {
            summary_statistics.close_open_gap_candidate_count as f64
                / summary_statistics.known_calendar_event_count.max(1) as f64
        },
        observed_mean_abs_gap_return: if summary_statistics.close_open_gap_candidate_count
            < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT
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
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    activity_coupling_family: &ReturnComponentsActivityCouplingCandidateFamily,
) -> ReturnComponentsStageActivityCouplingDiagnostic {
    let surprise_evidence_count = summary_statistics.intraday_jump_candidate_count
        + summary_statistics.close_open_gap_candidate_count;

    ReturnComponentsStageActivityCouplingDiagnostic {
        evidence_state: evidence_state_from_count(surprise_evidence_count).to_string(),
        observed_known_post_shock_activity_lift: summary_statistics
            .mean_known_post_shock_activity_lift,
        observed_surprise_post_shock_activity_lift: if surprise_evidence_count
            < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT
        {
            0.0
        } else {
            summary_statistics.mean_surprise_post_shock_activity_lift
        },
        fitted_known_activity_coupling_scale: activity_coupling_family
            .known_activity_coupling_scale,
        fitted_surprise_activity_coupling_scale: activity_coupling_family
            .surprise_activity_coupling_scale,
    }
}

fn build_price_range_sanity_diagnostic(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    return_distribution_diagnostic: ReturnComponentsStageReturnDistributionDiagnostic,
    surprise_arrival_family: &ReturnComponentsSurpriseNewsArrivalCandidateFamily,
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
) -> ReturnComponentsStagePriceRangeSanityDiagnostic {
    let continuous_reference_span = 6.0
        * return_distribution_diagnostic.fitted_return_volatility
        * (summary_statistics.observed_open_session_return_count as f64
            * summary_statistics.mean_continuous_business_time_scale)
            .sqrt()
            .max(1.0);
    let fitted_mean_abs_jump_return =
        fitted_mean_abs_jump_return(shared_mark_law_template, return_jump_family);
    let fitted_reference_log_price_span = continuous_reference_span
        + (summary_statistics.known_realized_sample_count as f64
            + surprise_arrival_family.baseline_intensity
                * summary_statistics.source_window_duration_hours)
            * fitted_mean_abs_jump_return
        + summary_statistics.close_open_gap_candidate_count as f64 * fitted_mean_abs_jump_return;
    let expansion_ratio =
        fitted_reference_log_price_span / summary_statistics.observed_log_price_span.max(1.0e-8);
    let is_flagged = expansion_ratio > RETURN_COMPONENTS_STAGE_PRICE_RANGE_SANITY_THRESHOLD_RATIO
        && fitted_reference_log_price_span > summary_statistics.observed_log_price_span + 0.05;

    ReturnComponentsStagePriceRangeSanityDiagnostic {
        is_flagged,
        observed_log_price_span: summary_statistics.observed_log_price_span,
        fitted_reference_log_price_span,
        expansion_ratio,
        threshold_ratio: RETURN_COMPONENTS_STAGE_PRICE_RANGE_SANITY_THRESHOLD_RATIO,
    }
}

fn dominant_scale_driver_from_metric_buckets(
    objective_metric_buckets: ReturnComponentsStageObjectiveMetricBuckets,
) -> &'static str {
    let mut dominant_driver = "nig_scale";
    let mut dominant_value = objective_metric_buckets.continuous_distribution_fit;

    for (driver, value) in [
        ("jump_scales", objective_metric_buckets.open_jump_fit),
        ("gap_behavior", objective_metric_buckets.closed_gap_fit),
        (
            "activity_coupling",
            objective_metric_buckets.activity_coupling_fit,
        ),
        (
            "price_range_sanity",
            objective_metric_buckets.price_range_sanity_fit,
        ),
    ] {
        if value > dominant_value {
            dominant_driver = driver;
            dominant_value = value;
        }
    }

    dominant_driver
}

fn evidence_state_from_count(event_count: usize) -> &'static str {
    if event_count < RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT {
        "insufficient_evidence"
    } else {
        "identified_component"
    }
}

pub(super) fn theoretical_net_process_distribution_diagnostic(
    configuration: NigSharedPlusShiftConfiguration,
    open_session_sell_share_proxy: f64,
) -> Option<ReturnComponentsStageReturnDistributionDiagnostic> {
    let (mean_return, variance, skewness, excess_kurtosis) =
        theoretical_net_process_moments(configuration, open_session_sell_share_proxy)?;
    let fitted_return_volatility = variance.sqrt();

    Some(ReturnComponentsStageReturnDistributionDiagnostic {
        sample_count: 0,
        observed_mean_return: 0.0,
        fitted_mean_return: mean_return,
        observed_return_volatility: 0.0,
        fitted_return_volatility,
        observed_skewness: 0.0,
        fitted_skewness: skewness,
        observed_excess_kurtosis: 0.0,
        fitted_excess_kurtosis: excess_kurtosis,
    })
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

fn zero_aware_relative_error(observed: f64, target: f64, zero_floor: f64) -> f64 {
    let denominator = if target.abs() <= zero_floor {
        zero_floor
    } else {
        target.abs()
    };
    ((observed - target).abs() / denominator).max(0.0)
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

    (value * 1_000_000.0).round() / 1_000_000.0
}

pub(super) fn round_negative_candidate_value(value: f64) -> f64 {
    if !value.is_finite() {
        return -0.05;
    }

    ((value.min(-1.0e-6)) * 1_000_000.0).round() / 1_000_000.0
}

pub(super) fn arithmetic_mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    let sum: f64 = values.iter().copied().sum();
    if !sum.is_finite() {
        return 0.0;
    }

    (sum / values.len() as f64).max(0.0)
}

fn deterministic_quantile_bins(values: &[f64], quantiles: &[f64]) -> Vec<f64> {
    if values.is_empty() {
        return vec![0.0];
    }

    let mut sorted_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .collect();
    sorted_values.sort_by(f64::total_cmp);

    if sorted_values.is_empty() {
        return vec![0.0];
    }

    let mut bins = Vec::with_capacity(quantiles.len());
    for quantile in quantiles {
        let clamped_quantile = quantile.clamp(0.0, 1.0);
        let quantile_position =
            (clamped_quantile * (sorted_values.len().saturating_sub(1)) as f64).round() as usize;
        bins.push(round_candidate_value(sorted_values[quantile_position]));
    }

    deduplicate_sorted_f64_values(bins)
}

fn round_candidate_value(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }

    let rounded = (value * 1_000_000.0).round() / 1_000_000.0;
    if rounded < 0.0 { 0.0 } else { rounded }
}

fn deduplicate_sorted_f64_values(mut values: Vec<f64>) -> Vec<f64> {
    values.retain(|value| value.is_finite() && *value >= 0.0);
    values.sort_by(f64::total_cmp);
    values.dedup_by(|left, right| (*left - *right).abs() <= 1e-12);
    values
}

fn collect_return_components_stage_constraint_violations(
    candidate_input: &ReturnComponentsStageCandidateInput,
) -> Vec<ReturnComponentsStageConstraintViolationDiagnostic> {
    let mut violations = Vec::new();

    if !candidate_input
        .nig_shared_plus_shift_configuration
        .delta_beta
        .is_finite()
        || candidate_input
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
        candidate_input.nig_shared_plus_shift_configuration.alpha,
        candidate_input.nig_shared_plus_shift_configuration.beta_0,
        candidate_input.nig_shared_plus_shift_configuration.delta,
        candidate_input.nig_shared_plus_shift_configuration.mu,
        candidate_input
            .nig_shared_plus_shift_configuration
            .delta_beta,
    ) {
        violations.push(ReturnComponentsStageConstraintViolationDiagnostic {
            field: "nig_shared_plus_shift_configuration".to_string(),
            message: source.to_string(),
        });
    }

    if let Err(source) = NewsProcessConfiguration::try_new(
        candidate_input.news_process_configuration.arrivals.clone(),
        candidate_input.news_process_configuration.shared_mark_law,
        candidate_input
            .news_process_configuration
            .known_component_mark_scales,
        candidate_input
            .news_process_configuration
            .surprise_component_mark_scales,
    ) {
        violations.push(ReturnComponentsStageConstraintViolationDiagnostic {
            field: "news_process_configuration".to_string(),
            message: source.to_string(),
        });
    }

    violations
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
            close_open_gap_candidate_count: 0,
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
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 1),
                    log_return: -1.0e-6,
                    post_shock_activity_lift: 1.0,
                },
            ],
            continuous_fit_returns: vec![
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 0),
                    log_return: 1.0e-6,
                    post_shock_activity_lift: 1.0,
                },
                ReturnComponentsObservedOpenSessionReturn {
                    return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 0, 1),
                    log_return: -1.0e-6,
                    post_shock_activity_lift: 1.0,
                },
            ],
            intraday_jump_candidates: Vec::new(),
            close_open_gap_candidates: Vec::new(),
            known_sample_magnitudes: Vec::new(),
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
            CalibrationOptimizerSettings::try_new("adaptive_search", 32, 1.0e-8, 1.0e-8, 1.0)
                .expect("solver settings should validate");

        let nig_candidates = continuous_fit::build_nig_search_candidates(
            summary_statistics,
            &calibration_sample,
            17,
            &solver_settings,
        );

        assert!(!nig_candidates.is_empty());
        assert!(nig_candidates.iter().all(|candidate| candidate.delta
            >= RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER));
    }

    #[test]
    fn return_distribution_summary_normalizes_each_return_by_business_time_scale() {
        let summary = summarize_return_distribution(&[0.01, 0.04], &[1.0, 4.0]);

        assert!((summary.mean_return - 0.01).abs() <= 1.0e-12);
        assert!((summary.volatility - 0.007071067811865475).abs() <= 1.0e-12);
        assert!(summary.volatility < sample_standard_deviation(&[0.01, 0.04]));
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
