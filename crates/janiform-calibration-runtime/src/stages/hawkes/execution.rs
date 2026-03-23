use std::collections::BTreeSet;

use super::contracts::{
    HawkesStageCandidateDiagnostic, HawkesStageCandidateInput, HawkesStageExecutionFailure,
    HawkesStageExecutionInput, HawkesStageExecutionMetadata, HawkesStageExecutionReport,
    HawkesStageFittedPayload, HawkesStageHandoffOutput, HawkesStageImpliedMetricsDiagnostic,
    HawkesStageObjectiveTermDiagnostic, HawkesStageObservedSourceRegime,
};
use crate::adapters::precheck_structured_hawkes_kernel_parameter_domain;
use crate::calendar::ExchangeCalendarRegistry;
use crate::objective::CalibrationObjectiveTermInput;
use crate::optimizer_adapter::{
    CalibrationOptimizerAdapterCandidateInput, CalibrationOptimizerAdapterConfiguration,
    CalibrationOptimizerAdapterError, CalibrationOptimizerAdapterRunOutput,
    CalibrationOptimizerCandidateParameterValue, CalibrationOptimizerParameterBound,
    run_constrained_optimizer_adapter,
};
use crate::stages::seasonality::{
    CalibrationStageExecutionStatus, SeasonalityStageNormalizedPayload,
    maximum_runtime_hourly_seasonality_factor_from_payload,
};
use janiform_calibration_artifacts::{
    CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_HAWKES_STAGE_HANDOFF_POLICY, CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_STAGE_SEED_DERIVATION_POLICY, CalibrationExecutionMode, CalibrationRunSpec,
    CalibrationStage, CalibrationStageHandoffTransport, HawkesStageFittedParameters,
    resolve_calibration_stage_seed_metadata, stage_solver_settings_for_stage,
};
use janiform_foundations::numeric::{
    arithmetic_mean, coefficient_of_variation, round_positive_to_domain, round_to,
};
use janiform_model_kernels::hawkes::{
    HawkesKernelLoadings, HawkesKernelParameterValidationReport, HawkesKernelParameters,
    HawkesKernelValidationStatus, HawkesStructuredAlphaParameterization,
    HawkesStructuredRhoParameterization,
};
#[derive(Debug, Clone, Copy, PartialEq)]
struct HawkesObservedActivityTargets {
    deseasonalized_open_event_rate_per_hour: f64,
    open_factor_exposure_hours: f64,
    deseasonalized_mean_interarrival_seconds: f64,
    deseasonalized_interarrival_coefficient_of_variation: f64,
    deseasonalized_short_gap_threshold_seconds: f64,
    deseasonalized_short_gap_ratio: f64,
    deseasonalized_known_news_rate_per_hour: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SeasonalityScaledStationarityCheck {
    max_seasonality_factor: f64,
    spectral_radius_alpha_over_beta: f64,
    effective_branching_ratio: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct HawkesPreparedCandidate {
    adjusted_candidate_input: HawkesStageCandidateInput,
    validation_report: HawkesKernelParameterValidationReport,
    objective_terms: Vec<CalibrationObjectiveTermInput>,
    objective_term_diagnostics: Vec<HawkesStageObjectiveTermDiagnostic>,
    implied_metrics: HawkesStageImpliedMetricsDiagnostic,
    warnings: Vec<String>,
    external_constraint_violation_count: usize,
}

pub fn run_hawkes_calibration_stage(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: HawkesStageExecutionInput,
) -> HawkesStageExecutionReport {
    if let Err(source) = run_spec.validate() {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::InvalidRunSpec {
                error_message: format!("{source} (classification={:?})", source.classification()),
            },
        );
    }

    if !run_spec
        .selected_stages
        .iter()
        .copied()
        .any(|selected_stage| selected_stage == CalibrationStage::Hawkes)
    {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::StageIsNotSelectedInRunSpec,
        );
    }

    let Some(stage_solver_settings) =
        stage_solver_settings_for_stage(run_spec, CalibrationStage::Hawkes)
    else {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::MissingStageSolverSettings,
        );
    };

    if stage_input.candidate_inputs.is_empty() {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::MissingHawkesCandidateInputs,
        );
    }

    let expected_seasonality_handoff = run_spec.stage_handoffs.iter().find(|stage_handoff| {
        stage_handoff.from_stage == CalibrationStage::Seasonality
            && stage_handoff.to_stage == CalibrationStage::Hawkes
    });
    let expected_transport = expected_seasonality_handoff
        .map(|stage_handoff| stage_handoff.transport())
        .unwrap_or(match run_spec.execution_mode {
            CalibrationExecutionMode::Memory => CalibrationStageHandoffTransport::InMemoryOutput,
            CalibrationExecutionMode::Disk => {
                CalibrationStageHandoffTransport::PersistedArtifactReference
            }
        });
    let provided_transport = stage_input.seasonality_input.transport();

    if provided_transport != expected_transport {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::SeasonalityInputTransportMismatch {
                expected_transport,
                provided_transport,
            },
        );
    }

    if provided_transport == CalibrationStageHandoffTransport::PersistedArtifactReference {
        let Some(provided_artifact_reference) = stage_input
            .seasonality_input
            .artifact_reference()
            .map(ToOwned::to_owned)
        else {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::MissingArtifactReferenceInSeasonalityInput,
            );
        };

        if provided_artifact_reference.trim().is_empty() {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::InvalidArtifactReferenceInSeasonalityInput {
                    artifact_reference: provided_artifact_reference,
                },
            );
        }

        if let Some(expected_stage_handoff) = expected_seasonality_handoff
            && let Some(expected_artifact_reference) = expected_stage_handoff.artifact_reference()
            && expected_artifact_reference != provided_artifact_reference
        {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::SeasonalityArtifactReferenceMismatch {
                    expected_artifact_reference: expected_artifact_reference.to_string(),
                    provided_artifact_reference,
                },
            );
        }
    }

    let resolved_stage_seed =
        resolve_calibration_stage_seed_metadata(run_spec, CalibrationStage::Hawkes);
    let optimizer_adapter_configuration = match CalibrationOptimizerAdapterConfiguration::try_new(
        "candidate_enumeration.v1",
        resolved_stage_seed.stage_seed,
        stage_solver_settings.clone(),
        hawkes_stage_optimizer_bounds(),
    ) {
        Ok(configuration) => configuration,
        Err(source) => {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::CandidateConstructionFailed {
                    candidate_id: "optimizer_adapter_configuration".to_string(),
                    error_message: source.to_string(),
                },
            );
        }
    };

    let seasonality_payload = stage_input.seasonality_input.payload();
    let max_runtime_hourly_seasonality_factor =
        match maximum_runtime_hourly_seasonality_factor_from_payload(
            calendar_registry,
            seasonality_payload,
        ) {
            Ok(value) => value,
            Err(error_message) => {
                return build_hawkes_stage_failure_report(
                    run_spec,
                    HawkesStageExecutionFailure::CandidateConstructionFailed {
                        candidate_id: "runtime_hourly_seasonality_envelope".to_string(),
                        error_message,
                    },
                );
            }
        };

    let initial_prepared_candidates = match prepare_hawkes_candidates(
        run_spec,
        stage_input.candidate_inputs.as_slice(),
        seasonality_payload,
        max_runtime_hourly_seasonality_factor,
    ) {
        Ok(prepared_candidates) => prepared_candidates,
        Err(report) => return report,
    };
    let (initial_candidate_diagnostics, initial_optimizer_output) =
        match evaluate_prepared_hawkes_candidates(
            run_spec,
            &optimizer_adapter_configuration,
            initial_prepared_candidates.as_slice(),
        ) {
            Ok(result) => result,
            Err(report) => return report,
        };

    let refined_candidate_inputs = generate_refined_hawkes_candidate_inputs(
        initial_prepared_candidates.as_slice(),
        initial_candidate_diagnostics.as_slice(),
    );
    let mut all_prepared_candidates = initial_prepared_candidates.clone();
    if !refined_candidate_inputs.is_empty() {
        let refined_prepared_candidates = match prepare_hawkes_candidates(
            run_spec,
            refined_candidate_inputs.as_slice(),
            seasonality_payload,
            max_runtime_hourly_seasonality_factor,
        ) {
            Ok(prepared_candidates) => prepared_candidates,
            Err(report) => return report,
        };
        all_prepared_candidates.extend(refined_prepared_candidates);
    }

    let (candidate_diagnostics, optimizer_adapter_output) =
        if all_prepared_candidates.len() == initial_prepared_candidates.len() {
            (initial_candidate_diagnostics, initial_optimizer_output)
        } else {
            match evaluate_prepared_hawkes_candidates(
                run_spec,
                &optimizer_adapter_configuration,
                all_prepared_candidates.as_slice(),
            ) {
                Ok(result) => result,
                Err(report) => return report,
            }
        };

    let Some(selected_candidate_id) = optimizer_adapter_output.selected_candidate_id.as_ref()
    else {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::ConstraintViolationsForAllCandidates {
                candidate_diagnostics,
            },
        );
    };

    let Some(selected_candidate_position) = all_prepared_candidates.iter().position(|candidate| {
        candidate.adjusted_candidate_input.candidate_id == *selected_candidate_id
    }) else {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: "optimizer_adapter_selection".to_string(),
                error_message: "selected candidate id was not found in evaluated hawkes candidates"
                    .to_string(),
            },
        );
    };

    let selected_candidate_input =
        &all_prepared_candidates[selected_candidate_position].adjusted_candidate_input;
    let selected_candidate_diagnostic = &candidate_diagnostics[selected_candidate_position];

    let Some(derived_alpha) = selected_candidate_diagnostic
        .validation_report
        .derived_alpha
    else {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: selected_candidate_input.candidate_id.clone(),
                error_message:
                    "selected candidate passed validation but did not provide derived alpha"
                        .to_string(),
            },
        );
    };

    let Some(derived_rho) = selected_candidate_diagnostic.validation_report.derived_rho else {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: selected_candidate_input.candidate_id.clone(),
                error_message:
                    "selected candidate passed validation but did not provide derived rho"
                        .to_string(),
            },
        );
    };

    let kernel_parameters = match HawkesKernelParameters::try_new_with_stability_tolerance(
        selected_candidate_input.lambda_bar,
        selected_candidate_input.beta,
        derived_alpha,
        derived_rho,
        selected_candidate_input.stability_numeric_tolerance,
    ) {
        Ok(kernel_parameters) => kernel_parameters,

        Err(source) => {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::CandidateConstructionFailed {
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

    let fitted_stage_payload = HawkesStageFittedPayload {
        schema_version: CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
        selected_candidate_id: selected_candidate_input.candidate_id.clone(),
        fitted_parameters: HawkesStageFittedParameters {
            kernel_parameters,
            structured_alpha: selected_candidate_input.structured_alpha,
            structured_rho: selected_candidate_input.structured_rho,
            self_dominance_policy: selected_candidate_input.self_dominance_policy,
            stability_numeric_tolerance: selected_candidate_input.stability_numeric_tolerance,
        },
        candidate_diagnostics,
        seasonality_input_transport: stage_input.seasonality_input.transport(),
        seasonality_dataset_id: stage_input
            .seasonality_input
            .dataset_id()
            .map(ToOwned::to_owned),
        seasonality_artifact_reference: stage_input
            .seasonality_input
            .artifact_reference()
            .map(ToOwned::to_owned),
    };

    HawkesStageExecutionReport {
        schema_version: CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::Hawkes,
        status: CalibrationStageExecutionStatus::Success,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_HAWKES_STAGE_HANDOFF_POLICY,
        handoffs: build_hawkes_stage_handoff_outputs(run_spec),
        fitted_stage_payload: Some(fitted_stage_payload),
        metadata: Some(HawkesStageExecutionMetadata {
            stage_output_schema_version: CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
            run_global_seed: resolved_stage_seed.run_global_seed,
            stage_seed: resolved_stage_seed.stage_seed,
            stage_seed_source: resolved_stage_seed.stage_seed_source,
            stage_seed_derivation_policy: CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
            solver_settings: stage_solver_settings,
            optimizer_adapter_metadata: optimizer_adapter_output.metadata.clone(),
            input_candidate_count: all_prepared_candidates.len(),
            valid_candidate_count,
            selected_candidate_raw_objective,
            selected_candidate_penalized_objective,
        }),
        failure: None,
    }
}

fn build_candidate_objective_terms(
    candidate_input: &HawkesStageCandidateInput,
    observed_activity_targets: HawkesObservedActivityTargets,
    lambda_bar_scaling_ratio_applied: f64,
    max_runtime_hourly_seasonality_factor: f64,
    derived_alpha: Option<janiform_model_kernels::hawkes::HawkesKernelLoadings>,
    derived_rho: Option<janiform_model_kernels::hawkes::HawkesKernelLoadings>,
) -> (
    Vec<CalibrationObjectiveTermInput>,
    Vec<HawkesStageObjectiveTermDiagnostic>,
    HawkesStageImpliedMetricsDiagnostic,
    Vec<String>,
) {
    let Some(derived_alpha) = derived_alpha else {
        return invalid_candidate_objective_bundle(
            candidate_input,
            "candidate did not derive alpha",
        );
    };
    let Some(derived_rho) = derived_rho else {
        return invalid_candidate_objective_bundle(candidate_input, "candidate did not derive rho");
    };

    let implied_metrics = derive_candidate_implied_metrics(
        candidate_input,
        observed_activity_targets,
        lambda_bar_scaling_ratio_applied,
        max_runtime_hourly_seasonality_factor,
        derived_alpha,
        derived_rho,
    );
    let mut warnings = Vec::new();

    if implied_metrics.event_volume_ratio_vs_total_observed > 1.5 {
        warnings.push(format!(
            "model implied event count over the observation window is {:.3}x the observed total tick count",
            implied_metrics.event_volume_ratio_vs_total_observed
        ));
    }
    if implied_metrics.model_implied_open_event_rate_per_hour
        > implied_metrics.observed_open_event_rate_per_hour * 1.5
    {
        warnings.push(format!(
            "model implied open-session rate {:.3} exceeds the observed open-session rate {:.3} by more than 50%",
            implied_metrics.model_implied_open_event_rate_per_hour,
            implied_metrics.observed_open_event_rate_per_hour
        ));
    }

    let mut objective_terms = Vec::new();
    let mut term_diagnostics = Vec::new();

    push_objective_term(
        "open_session_event_rate",
        3.0,
        implied_metrics.observed_open_event_rate_per_hour,
        implied_metrics.model_implied_open_event_rate_per_hour,
        implied_metrics.observed_open_event_rate_per_hour.max(1.0),
        &mut objective_terms,
        &mut term_diagnostics,
    );
    push_objective_term(
        "directional_imbalance_magnitude",
        1.5,
        observed_directional_imbalance_magnitude(&candidate_input.source_regime),
        (implied_metrics.model_implied_open_sell_share - 0.5).abs() * 2.0,
        observed_directional_imbalance_magnitude(&candidate_input.source_regime).max(0.05),
        &mut objective_terms,
        &mut term_diagnostics,
    );
    push_objective_term(
        "interarrival_coefficient_of_variation",
        1.25,
        observed_activity_targets.deseasonalized_interarrival_coefficient_of_variation,
        implied_metrics.model_implied_interarrival_coefficient_of_variation,
        observed_activity_targets
            .deseasonalized_interarrival_coefficient_of_variation
            .max(0.25),
        &mut objective_terms,
        &mut term_diagnostics,
    );
    push_objective_term(
        "short_gap_ratio",
        1.0,
        observed_activity_targets.deseasonalized_short_gap_ratio,
        implied_metrics.model_implied_short_gap_ratio,
        observed_activity_targets
            .deseasonalized_short_gap_ratio
            .max(0.05),
        &mut objective_terms,
        &mut term_diagnostics,
    );
    push_objective_term(
        "observation_window_event_count",
        2.0,
        candidate_input.source_regime.open_session_tick_count as f64,
        implied_metrics.model_implied_expected_event_count_over_observation_window,
        (candidate_input.source_regime.open_session_tick_count as f64).max(1.0),
        &mut objective_terms,
        &mut term_diagnostics,
    );

    if candidate_input.source_regime.known_news_open_event_count > 0 {
        push_objective_term(
            "known_news_activity_share",
            0.75,
            candidate_input
                .source_regime
                .open_session_known_news_activity_share,
            implied_metrics.model_implied_known_news_activity_share,
            candidate_input
                .source_regime
                .open_session_known_news_activity_share
                .max(0.02),
            &mut objective_terms,
            &mut term_diagnostics,
        );
    }

    (objective_terms, term_diagnostics, implied_metrics, warnings)
}

fn prepare_hawkes_candidates(
    run_spec: &CalibrationRunSpec,
    candidate_inputs: &[HawkesStageCandidateInput],
    seasonality_payload: &SeasonalityStageNormalizedPayload,
    max_runtime_hourly_seasonality_factor: f64,
) -> Result<Vec<HawkesPreparedCandidate>, HawkesStageExecutionReport> {
    let mut seen_candidate_ids = BTreeSet::new();
    let mut prepared_candidates = Vec::with_capacity(candidate_inputs.len());

    for candidate_input in candidate_inputs {
        if !seen_candidate_ids.insert(candidate_input.candidate_id.clone()) {
            return Err(build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::DuplicateCandidateId {
                    candidate_id: candidate_input.candidate_id.clone(),
                },
            ));
        }

        if let Err(source) = candidate_input.validate() {
            return Err(build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::InvalidCandidateInput {
                    candidate_id: candidate_input.candidate_id.clone(),
                    error_message: source.to_string(),
                },
            ));
        }

        let observed_activity_targets = derive_observed_activity_targets(
            candidate_input.source_regime.clone(),
            seasonality_payload,
        );
        let (adjusted_candidate_input, lambda_bar_scaling_ratio_applied) =
            adjust_candidate_for_deseasonalized_activity(
                candidate_input,
                observed_activity_targets,
            );
        let validation_report = precheck_structured_hawkes_kernel_parameter_domain(
            adjusted_candidate_input.lambda_bar,
            adjusted_candidate_input.beta,
            adjusted_candidate_input.structured_alpha,
            adjusted_candidate_input.structured_rho,
            adjusted_candidate_input.self_dominance_policy,
            adjusted_candidate_input.stability_numeric_tolerance,
        );

        let mut external_constraint_violation_count = validation_report.summary.failed_check_count;
        let (objective_terms, objective_term_diagnostics, implied_metrics, mut warnings) =
            build_candidate_objective_terms(
                &adjusted_candidate_input,
                observed_activity_targets,
                lambda_bar_scaling_ratio_applied,
                max_runtime_hourly_seasonality_factor,
                validation_report.derived_alpha,
                validation_report.derived_rho,
            );

        if let Some(stationarity_check) =
            validation_report.derived_alpha.and_then(|derived_alpha| {
                check_seasonality_scaled_stationarity(
                    max_runtime_hourly_seasonality_factor,
                    derived_alpha,
                    adjusted_candidate_input.beta,
                )
            })
        {
            external_constraint_violation_count =
                external_constraint_violation_count.saturating_add(1);
            warnings.push(format!(
                "seasonality-scaled effective branching ratio {:.6} reaches or exceeds 1.0 with max seasonality factor {:.6}",
                stationarity_check.effective_branching_ratio,
                stationarity_check.max_seasonality_factor,
            ));
        }

        prepared_candidates.push(HawkesPreparedCandidate {
            adjusted_candidate_input,
            validation_report,
            objective_terms,
            objective_term_diagnostics,
            implied_metrics,
            warnings,
            external_constraint_violation_count,
        });
    }

    Ok(prepared_candidates)
}

fn evaluate_prepared_hawkes_candidates(
    run_spec: &CalibrationRunSpec,
    optimizer_adapter_configuration: &CalibrationOptimizerAdapterConfiguration,
    prepared_candidates: &[HawkesPreparedCandidate],
) -> Result<
    (
        Vec<HawkesStageCandidateDiagnostic>,
        CalibrationOptimizerAdapterRunOutput,
    ),
    HawkesStageExecutionReport,
> {
    let adapter_candidate_inputs: Vec<CalibrationOptimizerAdapterCandidateInput> =
        prepared_candidates
            .iter()
            .map(|candidate| CalibrationOptimizerAdapterCandidateInput {
                candidate_id: candidate.adjusted_candidate_input.candidate_id.clone(),
                objective_terms: candidate.objective_terms.clone(),
                parameter_values: vec![
                    CalibrationOptimizerCandidateParameterValue {
                        parameter_name: "lambda_bar".to_string(),
                        value: candidate.adjusted_candidate_input.lambda_bar,
                    },
                    CalibrationOptimizerCandidateParameterValue {
                        parameter_name: "beta".to_string(),
                        value: candidate.adjusted_candidate_input.beta,
                    },
                    CalibrationOptimizerCandidateParameterValue {
                        parameter_name: "stability_numeric_tolerance".to_string(),
                        value: candidate
                            .adjusted_candidate_input
                            .stability_numeric_tolerance,
                    },
                ],
                external_constraint_violation_count: candidate.external_constraint_violation_count,
            })
            .collect();

    let optimizer_adapter_output = match run_constrained_optimizer_adapter(
        optimizer_adapter_configuration,
        adapter_candidate_inputs.as_slice(),
    ) {
        Ok(output) => output,
        Err(CalibrationOptimizerAdapterError::InvalidCandidateInput {
            candidate_id,
            source,
        }) => {
            return Err(build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::InvalidCandidateInput {
                    candidate_id,
                    error_message: source.to_string(),
                },
            ));
        }
        Err(CalibrationOptimizerAdapterError::InvalidObjectiveComposition {
            candidate_id,
            source,
        }) => {
            return Err(build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::InvalidCandidateInput {
                    candidate_id,
                    error_message: source.to_string(),
                },
            ));
        }
        Err(CalibrationOptimizerAdapterError::DuplicateCandidateId { candidate_id }) => {
            return Err(build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::DuplicateCandidateId { candidate_id },
            ));
        }
        Err(source) => {
            return Err(build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::CandidateConstructionFailed {
                    candidate_id: "optimizer_adapter".to_string(),
                    error_message: source.to_string(),
                },
            ));
        }
    };

    let candidate_diagnostics = prepared_candidates
        .iter()
        .zip(optimizer_adapter_output.candidate_diagnostics.iter())
        .map(
            |(candidate, optimizer_diagnostic)| HawkesStageCandidateDiagnostic {
                candidate_id: candidate.adjusted_candidate_input.candidate_id.clone(),
                raw_objective_value: optimizer_diagnostic.raw_objective_value,
                constraint_penalty: optimizer_diagnostic.constraint_penalty,
                penalized_objective_value: optimizer_diagnostic.penalized_objective_value,
                constraint_violation_count: optimizer_diagnostic.total_constraint_violation_count,
                objective_terms: candidate.objective_term_diagnostics.clone(),
                source_regime: candidate.adjusted_candidate_input.source_regime.clone(),
                directional_label_diagnostics: candidate
                    .adjusted_candidate_input
                    .directional_label_diagnostics
                    .clone(),
                // Intentional clone into the persisted stage diagnostics payload.
                implied_metrics: candidate.implied_metrics.clone(),
                warnings: candidate.warnings.clone(),
                // Intentional clone into the persisted stage diagnostics payload.
                validation_report: candidate.validation_report.clone(),
            },
        )
        .collect();

    Ok((candidate_diagnostics, optimizer_adapter_output))
}

fn generate_refined_hawkes_candidate_inputs(
    prepared_candidates: &[HawkesPreparedCandidate],
    candidate_diagnostics: &[HawkesStageCandidateDiagnostic],
) -> Vec<HawkesStageCandidateInput> {
    const MAX_REFINEMENT_SEEDS: usize = 3;
    const LAMBDA_SCALES: [f64; 3] = [0.7, 1.0, 1.4];
    const BETA_SCALES: [f64; 3] = [0.7, 1.0, 1.4];
    const RHO_SCALES: [f64; 3] = [0.6, 1.0, 1.6];

    let ranked_seed_indices =
        ranked_refinement_seed_indices(prepared_candidates, candidate_diagnostics);

    let mut refinement_candidates = Vec::new();
    let mut seen_ids = BTreeSet::new();
    for (seed_rank, candidate_index) in ranked_seed_indices
        .into_iter()
        .take(MAX_REFINEMENT_SEEDS)
        .enumerate()
    {
        let seed_candidate = &prepared_candidates[candidate_index].adjusted_candidate_input;
        let alpha_scales = refinement_alpha_scales(&candidate_diagnostics[candidate_index]);
        for lambda_scale in LAMBDA_SCALES {
            for beta_scale in BETA_SCALES {
                for alpha_scale in &alpha_scales {
                    for rho_scale in RHO_SCALES {
                        if lambda_scale == 1.0
                            && beta_scale == 1.0
                            && *alpha_scale == 1.0
                            && rho_scale == 1.0
                        {
                            continue;
                        }

                        let candidate_id = format!(
                            "{}_refined_{seed_rank}_{:03}",
                            seed_candidate.candidate_id,
                            refinement_candidates.len()
                        );
                        if !seen_ids.insert(candidate_id.clone()) {
                            continue;
                        }

                        refinement_candidates.push(HawkesStageCandidateInput {
                            candidate_id,
                            lambda_bar: round_positive_to_domain(
                                seed_candidate.lambda_bar * lambda_scale,
                                12,
                            ),
                            beta: round_positive_to_domain(seed_candidate.beta * beta_scale, 12),
                            structured_alpha: scale_structured_alpha(
                                seed_candidate.structured_alpha,
                                *alpha_scale,
                            ),
                            structured_rho: scale_structured_rho(
                                seed_candidate.structured_rho,
                                rho_scale,
                            ),
                            self_dominance_policy: seed_candidate.self_dominance_policy,
                            stability_numeric_tolerance: seed_candidate.stability_numeric_tolerance,
                            source_regime: seed_candidate.source_regime.clone(),
                            directional_label_diagnostics: seed_candidate
                                .directional_label_diagnostics
                                .clone(),
                        });
                    }
                }
            }
        }
    }

    refinement_candidates
}

fn ranked_refinement_seed_indices(
    prepared_candidates: &[HawkesPreparedCandidate],
    candidate_diagnostics: &[HawkesStageCandidateDiagnostic],
) -> Vec<usize> {
    let eligible_seed_indices: Vec<usize> = prepared_candidates
        .iter()
        .enumerate()
        .filter_map(|(candidate_index, candidate)| {
            (candidate.validation_report.status == HawkesKernelValidationStatus::Pass)
                .then_some(candidate_index)
        })
        .collect();
    if eligible_seed_indices.is_empty() {
        return Vec::new();
    }

    let has_valid_candidate =
        candidate_diagnostics
            .iter()
            .enumerate()
            .any(|(candidate_index, candidate)| {
                eligible_seed_indices.contains(&candidate_index)
                    && candidate.constraint_violation_count == 0
            });
    let minimum_violation_count = eligible_seed_indices
        .iter()
        .map(|candidate_index| candidate_diagnostics[*candidate_index].constraint_violation_count)
        .min()
        .unwrap_or(0);

    let mut ranked_seed_indices: Vec<(usize, usize, f64, f64, &str)> = candidate_diagnostics
        .iter()
        .enumerate()
        .filter_map(|(candidate_index, candidate)| {
            if !eligible_seed_indices.contains(&candidate_index) {
                return None;
            }
            let is_selected = if has_valid_candidate {
                candidate.constraint_violation_count == 0
            } else {
                candidate.constraint_violation_count == minimum_violation_count
            };
            if !is_selected {
                return None;
            }

            Some((
                candidate_index,
                candidate.constraint_violation_count,
                candidate.penalized_objective_value,
                candidate.raw_objective_value,
                candidate.candidate_id.as_str(),
            ))
        })
        .collect();
    ranked_seed_indices.sort_by(|left, right| {
        left.1
            .cmp(&right.1)
            .then_with(|| left.2.total_cmp(&right.2))
            .then_with(|| left.3.total_cmp(&right.3))
            .then_with(|| left.4.cmp(right.4))
    });

    ranked_seed_indices
        .into_iter()
        .map(|(candidate_index, _, _, _, _)| candidate_index)
        .collect()
}

fn refinement_alpha_scales(candidate_diagnostic: &HawkesStageCandidateDiagnostic) -> Vec<f64> {
    const BASE_ALPHA_SCALES: [f64; 7] = [0.05, 0.1, 0.2, 0.4, 0.8, 1.0, 1.2];
    const TARGET_STABILITY_MARGIN: f64 = 0.95;
    const MINIMUM_ALPHA_SCALE: f64 = 0.01;

    let mut alpha_scales = BASE_ALPHA_SCALES.to_vec();
    let effective_branching_ratio = candidate_diagnostic
        .implied_metrics
        .max_runtime_effective_branching_ratio;
    if effective_branching_ratio.is_finite() && effective_branching_ratio > 0.0 {
        let targeted_scale = round_to(
            (TARGET_STABILITY_MARGIN / effective_branching_ratio).clamp(MINIMUM_ALPHA_SCALE, 1.0),
            12,
        );
        alpha_scales.push(targeted_scale);
        alpha_scales.push(round_to(
            (targeted_scale * 0.5).clamp(MINIMUM_ALPHA_SCALE, 1.0),
            12,
        ));
    }

    alpha_scales.sort_by(|left, right| left.total_cmp(right));
    alpha_scales.dedup_by(|left, right| left.total_cmp(right).is_eq());
    alpha_scales
}

fn scale_structured_alpha(
    structured_alpha: HawkesStructuredAlphaParameterization,
    scale: f64,
) -> HawkesStructuredAlphaParameterization {
    HawkesStructuredAlphaParameterization {
        a_self: round_positive_to_domain(structured_alpha.a_self * scale, 12),
        a_cross: round_to((structured_alpha.a_cross * scale).max(0.0), 12),
        delta_self: round_to((structured_alpha.delta_self * scale).max(0.0), 12),
        delta_cross: round_to((structured_alpha.delta_cross * scale).max(0.0), 12),
    }
}

fn scale_structured_rho(
    structured_rho: HawkesStructuredRhoParameterization,
    scale: f64,
) -> HawkesStructuredRhoParameterization {
    HawkesStructuredRhoParameterization {
        r_self: round_positive_to_domain(structured_rho.r_self * scale, 12),
        r_cross: round_to((structured_rho.r_cross * scale).max(0.0), 12),
        delta_self_r: round_to((structured_rho.delta_self_r * scale).max(0.0), 12),
        delta_cross_r: round_to((structured_rho.delta_cross_r * scale).max(0.0), 12),
    }
}

fn invalid_candidate_objective_bundle(
    candidate_input: &HawkesStageCandidateInput,
    warning_message: &str,
) -> (
    Vec<CalibrationObjectiveTermInput>,
    Vec<HawkesStageObjectiveTermDiagnostic>,
    HawkesStageImpliedMetricsDiagnostic,
    Vec<String>,
) {
    (
        vec![CalibrationObjectiveTermInput {
            term_id: "invalid_parameterization_penalty".to_string(),
            weight: 1.0,
            observed_score: 1_000_000.0,
        }],
        vec![HawkesStageObjectiveTermDiagnostic {
            term_id: "invalid_parameterization_penalty".to_string(),
            weight: 1.0,
            observed_value: 0.0,
            model_implied_value: 1_000_000.0,
            scale: 1.0,
            absolute_error: 1_000_000.0,
            normalized_error: 1_000_000.0,
            weighted_contribution: 1_000_000.0,
        }],
        HawkesStageImpliedMetricsDiagnostic {
            baseline_buy_intensity_per_hour: candidate_input.lambda_bar,
            baseline_sell_intensity_per_hour: candidate_input.lambda_bar,
            observed_open_event_rate_per_hour: observed_open_session_event_rate_per_hour(
                &candidate_input.source_regime,
            ),
            deseasonalized_open_event_rate_per_hour_target: 0.0,
            lambda_bar_scaling_ratio_applied: 1.0,
            max_runtime_hourly_seasonality_factor: 1.0,
            max_runtime_effective_branching_ratio: 0.0,
            runtime_effective_branching_margin: 1.0,
            model_implied_open_event_rate_per_hour: 0.0,
            model_implied_open_buy_share: 0.5,
            model_implied_open_sell_share: 0.5,
            model_implied_interarrival_coefficient_of_variation: 0.0,
            model_implied_short_gap_ratio: 0.0,
            model_implied_known_news_activity_share: 0.0,
            model_implied_expected_event_count_over_observation_window: 0.0,
            endogenous_branching_ratio: 0.0,
            self_excitation_ratio: 0.0,
            cross_excitation_ratio: 0.0,
            exogenous_self_loading_ratio: 0.0,
            exogenous_cross_loading_ratio: 0.0,
            stability_margin: 0.0,
            event_volume_ratio_vs_total_observed: 0.0,
        },
        vec![warning_message.to_string()],
    )
}

fn derive_observed_activity_targets(
    source_regime: HawkesStageObservedSourceRegime,
    seasonality_payload: &SeasonalityStageNormalizedPayload,
) -> HawkesObservedActivityTargets {
    const MAX_DESEASONALIZED_GAP_SECONDS: f64 = 3_600.0;
    const OPEN_MARKET_STATE_CODE: u8 = 1;

    let open_event_points: Vec<(i64, f64)> = seasonality_payload
        .timestamp_utc_nanos
        .iter()
        .copied()
        .zip(
            seasonality_payload
                .normalized_combined_factor_by_event
                .iter()
                .copied(),
        )
        .zip(seasonality_payload.market_state_codes.iter().copied())
        .filter_map(|((timestamp_utc_nanos, factor), market_state_code)| {
            if market_state_code == OPEN_MARKET_STATE_CODE {
                Some((timestamp_utc_nanos, factor.max(1.0e-9)))
            } else {
                None
            }
        })
        .collect();

    let mut transformed_open_gaps_seconds: Vec<f64> =
        Vec::with_capacity(open_event_points.len().saturating_sub(1));
    let mut same_window_gap_seconds_sum = 0.0;
    let mut seasonality_weighted_gap_seconds_sum = 0.0;

    for (
        (previous_timestamp_utc_nanos, previous_factor),
        (current_timestamp_utc_nanos, current_factor),
    ) in open_event_points
        .iter()
        .copied()
        .zip(open_event_points.iter().copied().skip(1))
    {
        let raw_gap_seconds = ((current_timestamp_utc_nanos - previous_timestamp_utc_nanos) as f64
            / 1_000_000_000.0)
            .max(0.0);
        if raw_gap_seconds > MAX_DESEASONALIZED_GAP_SECONDS {
            continue;
        }

        let average_factor = ((previous_factor + current_factor) / 2.0).max(1.0e-9);

        same_window_gap_seconds_sum += raw_gap_seconds;
        seasonality_weighted_gap_seconds_sum += raw_gap_seconds * average_factor;

        transformed_open_gaps_seconds.push(raw_gap_seconds * average_factor);
    }

    let fallback_mean_factor = arithmetic_mean(
        open_event_points
            .iter()
            .map(|(_, factor)| *factor)
            .collect::<Vec<f64>>()
            .as_slice(),
    )
    .unwrap_or(1.0)
    .max(1.0e-9);

    let mean_open_factor = if same_window_gap_seconds_sum > 0.0 {
        (seasonality_weighted_gap_seconds_sum / same_window_gap_seconds_sum).max(1.0e-9)
    } else {
        fallback_mean_factor
    };

    let open_factor_exposure_hours =
        (source_regime.open_session_duration_hours * mean_open_factor).max(1.0e-9);
    let deseasonalized_open_event_rate_per_hour =
        source_regime.open_session_tick_count as f64 / open_factor_exposure_hours;
    let deseasonalized_mean_interarrival_seconds =
        arithmetic_mean(transformed_open_gaps_seconds.as_slice())
            .unwrap_or(source_regime.open_session_mean_interarrival_seconds * mean_open_factor)
            .max(1.0e-6);
    let deseasonalized_interarrival_coefficient_of_variation =
        coefficient_of_variation(transformed_open_gaps_seconds.as_slice());
    let deseasonalized_short_gap_threshold_seconds =
        (deseasonalized_mean_interarrival_seconds * 0.5).clamp(1.0, 300.0);
    let deseasonalized_short_gap_ratio = if transformed_open_gaps_seconds.is_empty() {
        0.0
    } else {
        transformed_open_gaps_seconds
            .iter()
            .filter(|gap_seconds| **gap_seconds <= deseasonalized_short_gap_threshold_seconds)
            .count() as f64
            / transformed_open_gaps_seconds.len() as f64
    };
    let deseasonalized_known_news_rate_per_hour =
        source_regime.known_news_open_event_count as f64 / open_factor_exposure_hours.max(1.0e-9);

    HawkesObservedActivityTargets {
        deseasonalized_open_event_rate_per_hour,
        open_factor_exposure_hours,
        deseasonalized_mean_interarrival_seconds,
        deseasonalized_interarrival_coefficient_of_variation,
        deseasonalized_short_gap_threshold_seconds,
        deseasonalized_short_gap_ratio,
        deseasonalized_known_news_rate_per_hour,
    }
}

fn adjust_candidate_for_deseasonalized_activity(
    candidate_input: &HawkesStageCandidateInput,
    _observed_activity_targets: HawkesObservedActivityTargets,
) -> (HawkesStageCandidateInput, f64) {
    (candidate_input.clone(), 1.0)
}

fn check_seasonality_scaled_stationarity(
    max_seasonality_factor: f64,
    derived_alpha: janiform_model_kernels::hawkes::HawkesKernelLoadings,
    beta: f64,
) -> Option<SeasonalityScaledStationarityCheck> {
    let spectral_radius_alpha_over_beta = spectral_radius_alpha_over_beta(derived_alpha, beta);
    let effective_branching_ratio = max_seasonality_factor * spectral_radius_alpha_over_beta;
    if effective_branching_ratio >= 1.0 {
        return Some(SeasonalityScaledStationarityCheck {
            max_seasonality_factor,
            spectral_radius_alpha_over_beta,
            effective_branching_ratio,
        });
    }

    None
}

fn derive_candidate_implied_metrics(
    candidate_input: &HawkesStageCandidateInput,
    observed_activity_targets: HawkesObservedActivityTargets,
    lambda_bar_scaling_ratio_applied: f64,
    max_runtime_hourly_seasonality_factor: f64,
    derived_alpha: janiform_model_kernels::hawkes::HawkesKernelLoadings,
    derived_rho: janiform_model_kernels::hawkes::HawkesKernelLoadings,
) -> HawkesStageImpliedMetricsDiagnostic {
    let scaled_plus_plus = derived_alpha.plus_plus / candidate_input.beta;
    let scaled_plus_minus = derived_alpha.plus_minus / candidate_input.beta;
    let scaled_minus_plus = derived_alpha.minus_plus / candidate_input.beta;
    let scaled_minus_minus = derived_alpha.minus_minus / candidate_input.beta;

    let determinant = (1.0 - scaled_plus_plus) * (1.0 - scaled_minus_minus)
        - scaled_plus_minus * scaled_minus_plus;

    let stationary_buy_multiplier = ((1.0 - scaled_minus_minus) + scaled_plus_minus) / determinant;
    let stationary_sell_multiplier = (scaled_minus_plus + (1.0 - scaled_plus_plus)) / determinant;

    let stationary_buy_intensity = candidate_input.lambda_bar * stationary_buy_multiplier.max(0.0);
    let stationary_sell_intensity =
        candidate_input.lambda_bar * stationary_sell_multiplier.max(0.0);
    let stationary_total_intensity = stationary_buy_intensity + stationary_sell_intensity;

    let model_implied_open_sell_share = if stationary_total_intensity > 0.0 {
        stationary_sell_intensity / stationary_total_intensity
    } else {
        0.5
    };

    let observed_open_event_rate_per_hour =
        observed_open_session_event_rate_per_hour(&candidate_input.source_regime);
    let endogenous_branching_ratio =
        spectral_radius_alpha_over_beta(derived_alpha, candidate_input.beta);
    let max_runtime_effective_branching_ratio =
        max_runtime_hourly_seasonality_factor * endogenous_branching_ratio;
    let self_excitation_ratio =
        (derived_alpha.plus_plus + derived_alpha.minus_minus) / (2.0 * candidate_input.beta);
    let cross_excitation_ratio =
        (derived_alpha.plus_minus + derived_alpha.minus_plus) / (2.0 * candidate_input.beta);
    let exogenous_self_loading_ratio =
        (derived_rho.plus_plus + derived_rho.minus_minus) / (2.0 * candidate_input.beta);
    let exogenous_cross_loading_ratio =
        (derived_rho.plus_minus + derived_rho.minus_plus) / (2.0 * candidate_input.beta);

    let model_implied_interarrival_coefficient_of_variation = (1.0
        + 2.0 * endogenous_branching_ratio / (1.0 - endogenous_branching_ratio).max(1e-9))
    .sqrt();

    let poisson_short_gap_ratio = 1.0
        - (-(stationary_total_intensity / 3600.0)
            * observed_activity_targets.deseasonalized_short_gap_threshold_seconds)
            .exp();

    let clustering_decay = (-(candidate_input.beta / 60.0)
        * observed_activity_targets.deseasonalized_short_gap_threshold_seconds)
        .exp();

    let model_implied_short_gap_ratio = (poisson_short_gap_ratio
        + (1.0 - poisson_short_gap_ratio) * endogenous_branching_ratio * clustering_decay)
        .clamp(0.0, 0.999_999);

    let news_rate_per_hour = observed_activity_targets.deseasonalized_known_news_rate_per_hour;

    let rho_average_row_sum = (derived_rho.plus_plus
        + derived_rho.plus_minus
        + derived_rho.minus_plus
        + derived_rho.minus_minus)
        / 2.0;

    let exogenous_extra_rate = if news_rate_per_hour > 0.0 {
        news_rate_per_hour * rho_average_row_sum
            / (candidate_input.beta * (1.0 - endogenous_branching_ratio).max(1e-9))
    } else {
        0.0
    };

    let model_implied_known_news_activity_share = if stationary_total_intensity > 0.0 {
        (exogenous_extra_rate / stationary_total_intensity).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let expected_event_count_over_observation_window = stationary_total_intensity
        * candidate_input
            .source_regime
            .open_session_duration_hours
            .max(1.0e-9);

    HawkesStageImpliedMetricsDiagnostic {
        baseline_buy_intensity_per_hour: candidate_input.lambda_bar,
        baseline_sell_intensity_per_hour: candidate_input.lambda_bar,
        observed_open_event_rate_per_hour,
        deseasonalized_open_event_rate_per_hour_target: observed_activity_targets
            .deseasonalized_open_event_rate_per_hour,
        lambda_bar_scaling_ratio_applied,
        max_runtime_hourly_seasonality_factor,
        max_runtime_effective_branching_ratio,
        runtime_effective_branching_margin: (1.0 - max_runtime_effective_branching_ratio).max(0.0),
        model_implied_open_event_rate_per_hour: stationary_total_intensity,
        model_implied_open_buy_share: 1.0 - model_implied_open_sell_share,
        model_implied_open_sell_share,
        model_implied_interarrival_coefficient_of_variation,
        model_implied_short_gap_ratio,
        model_implied_known_news_activity_share,
        model_implied_expected_event_count_over_observation_window:
            expected_event_count_over_observation_window,
        endogenous_branching_ratio,
        self_excitation_ratio,
        cross_excitation_ratio,
        exogenous_self_loading_ratio,
        exogenous_cross_loading_ratio,
        stability_margin: (1.0 - endogenous_branching_ratio).max(0.0),
        event_volume_ratio_vs_total_observed: expected_event_count_over_observation_window
            / (candidate_input.source_regime.open_session_tick_count as f64).max(1.0),
    }
}

fn push_objective_term(
    term_id: &str,
    weight: f64,
    observed_value: f64,
    model_implied_value: f64,
    scale: f64,
    objective_terms: &mut Vec<CalibrationObjectiveTermInput>,
    term_diagnostics: &mut Vec<HawkesStageObjectiveTermDiagnostic>,
) {
    let absolute_error = (model_implied_value - observed_value).abs();
    let normalized_error = absolute_error / scale.max(1e-9);
    let weighted_contribution = weight * normalized_error;

    objective_terms.push(CalibrationObjectiveTermInput {
        term_id: term_id.to_string(),
        weight,
        observed_score: normalized_error,
    });
    term_diagnostics.push(HawkesStageObjectiveTermDiagnostic {
        term_id: term_id.to_string(),
        weight,
        observed_value,
        model_implied_value,
        scale,
        absolute_error,
        normalized_error,
        weighted_contribution,
    });
}

fn observed_open_session_event_rate_per_hour(
    source_regime: &HawkesStageObservedSourceRegime,
) -> f64 {
    source_regime.open_session_tick_count as f64
        / source_regime.open_session_duration_hours.max(1e-9)
}

fn observed_directional_imbalance_magnitude(
    source_regime: &HawkesStageObservedSourceRegime,
) -> f64 {
    ((source_regime.open_session_sell_tick_count as f64
        / source_regime.open_session_tick_count as f64)
        - 0.5)
        .abs()
        * 2.0
}

fn spectral_radius_alpha_over_beta(
    alpha: janiform_model_kernels::hawkes::HawkesKernelLoadings,
    beta: f64,
) -> f64 {
    let scaled_plus_plus = alpha.plus_plus / beta;
    let scaled_plus_minus = alpha.plus_minus / beta;
    let scaled_minus_plus = alpha.minus_plus / beta;
    let scaled_minus_minus = alpha.minus_minus / beta;
    let trace = scaled_plus_plus + scaled_minus_minus;
    let discriminant = ((scaled_plus_plus - scaled_minus_minus).powi(2)
        + 4.0 * scaled_plus_minus * scaled_minus_plus)
        .sqrt();

    0.5 * (trace + discriminant)
}

pub(crate) fn stationary_open_event_rate_per_hour(
    fitted_parameters: &HawkesStageFittedParameters,
) -> Option<f64> {
    let derived_alpha =
        HawkesKernelLoadings::try_from_structured_alpha(fitted_parameters.structured_alpha).ok()?;
    let beta = fitted_parameters.kernel_parameters.beta;
    if !beta.is_finite() || beta <= 0.0 {
        return None;
    }

    let scaled_plus_plus = derived_alpha.plus_plus / beta;
    let scaled_plus_minus = derived_alpha.plus_minus / beta;
    let scaled_minus_plus = derived_alpha.minus_plus / beta;
    let scaled_minus_minus = derived_alpha.minus_minus / beta;

    let determinant = (1.0 - scaled_plus_plus) * (1.0 - scaled_minus_minus)
        - scaled_plus_minus * scaled_minus_plus;
    if !determinant.is_finite() || determinant.abs() <= f64::EPSILON {
        return None;
    }

    let stationary_buy_multiplier = ((1.0 - scaled_minus_minus) + scaled_plus_minus) / determinant;
    let stationary_sell_multiplier = (scaled_minus_plus + (1.0 - scaled_plus_plus)) / determinant;

    Some(
        fitted_parameters.kernel_parameters.lambda_bar
            * (stationary_buy_multiplier.max(0.0) + stationary_sell_multiplier.max(0.0)),
    )
}

fn hawkes_stage_optimizer_bounds() -> Vec<CalibrationOptimizerParameterBound> {
    vec![
        CalibrationOptimizerParameterBound {
            parameter_name: "lambda_bar".to_string(),
            lower_inclusive: 0.0,
            upper_inclusive: f64::MAX,
        },
        CalibrationOptimizerParameterBound {
            parameter_name: "beta".to_string(),
            lower_inclusive: 0.0,
            upper_inclusive: f64::MAX,
        },
        CalibrationOptimizerParameterBound {
            parameter_name: "stability_numeric_tolerance".to_string(),
            lower_inclusive: 0.0,
            upper_inclusive: f64::MAX,
        },
    ]
}

fn build_hawkes_stage_handoff_outputs(
    run_spec: &CalibrationRunSpec,
) -> Vec<HawkesStageHandoffOutput> {
    run_spec
        .stage_handoffs
        .iter()
        .filter(|stage_handoff| stage_handoff.from_stage == CalibrationStage::Hawkes)
        .map(|stage_handoff| match stage_handoff.transport() {
            CalibrationStageHandoffTransport::InMemoryOutput => HawkesStageHandoffOutput {
                from_stage: stage_handoff.from_stage,
                to_stage: stage_handoff.to_stage,
                transport: stage_handoff.transport(),
                in_memory_payload_available: true,
                artifact_reference: None,
            },
            CalibrationStageHandoffTransport::PersistedArtifactReference => {
                HawkesStageHandoffOutput {
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

fn build_hawkes_stage_failure_report(
    run_spec: &CalibrationRunSpec,
    failure: HawkesStageExecutionFailure,
) -> HawkesStageExecutionReport {
    HawkesStageExecutionReport {
        schema_version: CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::Hawkes,
        status: CalibrationStageExecutionStatus::Failure,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_HAWKES_STAGE_HANDOFF_POLICY,
        handoffs: Vec::new(),
        fitted_stage_payload: None,
        metadata: None,
        failure: Some(failure),
    }
}

#[cfg(test)]
mod tests {
    use super::{HawkesPreparedCandidate, ranked_refinement_seed_indices, refinement_alpha_scales};
    use crate::adapters::precheck_structured_hawkes_kernel_parameter_domain;
    use crate::stages::hawkes::{
        HawkesStageCandidateDiagnostic, HawkesStageCandidateInput,
        HawkesStageDirectionalLabelDiagnostics, HawkesStageImpliedMetricsDiagnostic,
        HawkesStageObservedSourceRegime,
    };
    use janiform_model_kernels::hawkes::{
        HawkesStructuredAlphaParameterization, HawkesStructuredRhoParameterization,
        HawkesStructuredRhoSelfDominancePolicy,
    };

    #[test]
    fn ranked_refinement_seed_indices_fall_back_to_least_violating_candidates() {
        let candidate_diagnostics = vec![
            test_candidate_diagnostic("candidate_a", 2, 1.0, 1.0),
            test_candidate_diagnostic("candidate_b", 1, 2.0, 2.0),
            test_candidate_diagnostic("candidate_c", 1, 0.5, 0.5),
        ];
        let prepared_candidates = vec![
            test_prepared_candidate("candidate_a"),
            test_prepared_candidate("candidate_b"),
            test_prepared_candidate("candidate_c"),
        ];

        let ranked_seed_indices =
            ranked_refinement_seed_indices(&prepared_candidates, &candidate_diagnostics);

        assert_eq!(ranked_seed_indices, vec![2, 1]);
    }

    #[test]
    fn refinement_alpha_scales_include_targeted_stability_reduction() {
        let candidate_diagnostic =
            test_candidate_diagnostic("candidate_supercritical", 1, 1.0, 1.0);

        let alpha_scales = refinement_alpha_scales(&candidate_diagnostic);

        assert!(alpha_scales.contains(&0.095));
        assert!(alpha_scales.contains(&0.19));
        assert!(alpha_scales.iter().any(|scale| *scale < 0.2));
    }

    fn test_candidate_diagnostic(
        candidate_id: &str,
        constraint_violation_count: usize,
        raw_objective_value: f64,
        penalized_objective_value: f64,
    ) -> HawkesStageCandidateDiagnostic {
        HawkesStageCandidateDiagnostic {
            candidate_id: candidate_id.to_string(),
            raw_objective_value,
            constraint_penalty: 10.0,
            penalized_objective_value,
            constraint_violation_count,
            objective_terms: Vec::new(),
            source_regime: test_source_regime(),
            directional_label_diagnostics: test_directional_label_diagnostics(),
            implied_metrics: HawkesStageImpliedMetricsDiagnostic {
                baseline_buy_intensity_per_hour: 1.0,
                baseline_sell_intensity_per_hour: 1.0,
                observed_open_event_rate_per_hour: 1.0,
                deseasonalized_open_event_rate_per_hour_target: 1.0,
                lambda_bar_scaling_ratio_applied: 1.0,
                max_runtime_hourly_seasonality_factor: 10.0,
                max_runtime_effective_branching_ratio: 5.0,
                runtime_effective_branching_margin: 0.0,
                model_implied_open_event_rate_per_hour: 1.0,
                model_implied_open_buy_share: 0.5,
                model_implied_open_sell_share: 0.5,
                model_implied_interarrival_coefficient_of_variation: 1.0,
                model_implied_short_gap_ratio: 0.5,
                model_implied_known_news_activity_share: 0.0,
                model_implied_expected_event_count_over_observation_window: 1.0,
                endogenous_branching_ratio: 0.5,
                self_excitation_ratio: 0.5,
                cross_excitation_ratio: 0.0,
                exogenous_self_loading_ratio: 0.0,
                exogenous_cross_loading_ratio: 0.0,
                stability_margin: 0.5,
                event_volume_ratio_vs_total_observed: 1.0,
            },
            warnings: Vec::new(),
            validation_report: precheck_structured_hawkes_kernel_parameter_domain(
                1.0,
                10.0,
                HawkesStructuredAlphaParameterization {
                    a_self: 0.1,
                    a_cross: 0.0,
                    delta_self: 0.0,
                    delta_cross: 0.0,
                },
                HawkesStructuredRhoParameterization {
                    r_self: 1.0e-12,
                    r_cross: 0.0,
                    delta_self_r: 0.0,
                    delta_cross_r: 0.0,
                },
                Some(HawkesStructuredRhoSelfDominancePolicy {
                    minimum_self_to_cross_ratio: 1.0,
                }),
                0.0,
            ),
        }
    }

    fn test_prepared_candidate(candidate_id: &str) -> HawkesPreparedCandidate {
        HawkesPreparedCandidate {
            adjusted_candidate_input: HawkesStageCandidateInput {
                candidate_id: candidate_id.to_string(),
                lambda_bar: 1.0,
                beta: 10.0,
                structured_alpha: HawkesStructuredAlphaParameterization {
                    a_self: 0.1,
                    a_cross: 0.0,
                    delta_self: 0.0,
                    delta_cross: 0.0,
                },
                structured_rho: HawkesStructuredRhoParameterization {
                    r_self: 1.0e-12,
                    r_cross: 0.0,
                    delta_self_r: 0.0,
                    delta_cross_r: 0.0,
                },
                self_dominance_policy: Some(HawkesStructuredRhoSelfDominancePolicy {
                    minimum_self_to_cross_ratio: 1.0,
                }),
                stability_numeric_tolerance: 0.0,
                source_regime: test_source_regime(),
                directional_label_diagnostics: test_directional_label_diagnostics(),
            },
            validation_report: precheck_structured_hawkes_kernel_parameter_domain(
                1.0,
                10.0,
                HawkesStructuredAlphaParameterization {
                    a_self: 0.1,
                    a_cross: 0.0,
                    delta_self: 0.0,
                    delta_cross: 0.0,
                },
                HawkesStructuredRhoParameterization {
                    r_self: 1.0e-12,
                    r_cross: 0.0,
                    delta_self_r: 0.0,
                    delta_cross_r: 0.0,
                },
                Some(HawkesStructuredRhoSelfDominancePolicy {
                    minimum_self_to_cross_ratio: 1.0,
                }),
                0.0,
            ),
            objective_terms: Vec::new(),
            objective_term_diagnostics: Vec::new(),
            implied_metrics: test_candidate_diagnostic(candidate_id, 0, 0.0, 0.0).implied_metrics,
            warnings: Vec::new(),
            external_constraint_violation_count: 0,
        }
    }

    fn test_source_regime() -> HawkesStageObservedSourceRegime {
        HawkesStageObservedSourceRegime {
            observation_duration_hours: 1.0,
            open_session_duration_hours: 1.0,
            total_tick_count: 10,
            open_session_tick_count: 10,
            closed_session_tick_count: 0,
            open_session_buy_tick_count: 5,
            open_session_sell_tick_count: 5,
            open_session_mean_interarrival_seconds: 1.0,
            open_session_interarrival_coefficient_of_variation: 1.0,
            open_session_short_gap_threshold_seconds: 0.5,
            open_session_short_gap_ratio: 0.5,
            known_news_open_event_count: 0,
            open_session_known_news_activity_share: 0.0,
        }
    }

    fn test_directional_label_diagnostics() -> HawkesStageDirectionalLabelDiagnostics {
        HawkesStageDirectionalLabelDiagnostics {
            policy_id: "policy".to_string(),
            first_inferred_trade_sign: "buy".to_string(),
            provided_label_count: 0,
            inferred_label_count: 0,
            flat_move_carry_count: 0,
            price_up_inference_count: 0,
            price_down_inference_count: 0,
        }
    }
}
