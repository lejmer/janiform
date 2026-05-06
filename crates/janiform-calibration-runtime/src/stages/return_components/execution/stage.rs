use std::collections::BTreeSet;
use std::sync::mpsc;
use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::CalibrationResolvedStageSeed;
use janiform_calibration_artifacts::{
    CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_RETURN_COMPONENTS_STAGE_HANDOFF_POLICY,
    CALIBRATION_RETURN_COMPONENTS_STAGE_OBJECTIVE_WEIGHTING_POLICY,
    CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_STAGE_SEED_DERIVATION_POLICY, CalibrationExecutionMode, CalibrationRunSpec,
    CalibrationStage, CalibrationStageHandoffTransport, ReturnComponentsStageFittedParameters,
    SurpriseNewsDeterministicIntensityProfile, resolve_calibration_stage_seed_metadata,
    stage_solver_settings_for_stage,
};
use janiform_model_kernels::mean_reversion::MeanReversionFittedParameters;
use janiform_model_kernels::news::NewsProcessConfiguration;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

use super::super::constants::{
    RETURN_COMPONENTS_STAGE_CANDIDATE_GENERATION_POLICY,
    RETURN_COMPONENTS_STAGE_OBJECTIVE_METRIC_BUCKET_POLICY,
};
use super::super::contracts::{
    ReturnComponentsStageCandidateDiagnostic, ReturnComponentsStageConstraintViolationDiagnostic,
    ReturnComponentsStageExecutionFailure, ReturnComponentsStageExecutionInput,
    ReturnComponentsStageExecutionMetadata, ReturnComponentsStageExecutionReport,
    ReturnComponentsStageHandoffOutput, ReturnComponentsStageProgressPhase,
    ReturnComponentsStageProgressUpdate,
};
use super::candidate_generation::generate_return_components_stage_candidate_inputs;
use super::close_open_gap::prepare_candidate_generation_input;
use super::diagnostics::collect_return_components_stage_constraint_violations;
use super::signals::resolve_activity_calibration_context;
use crate::optimizer_adapter::{
    CalibrationOptimizerAdapterCandidateInput, CalibrationOptimizerAdapterConfiguration,
    CalibrationOptimizerAdapterError, CalibrationOptimizerAdapterRunMetadata,
    CalibrationOptimizerAdapterRunOutput, CalibrationOptimizerCandidateParameterValue,
    CalibrationOptimizerParameterBound, run_constrained_optimizer_adapter,
};
use crate::stages::return_components::progress::send_return_components_progress;
use crate::stages::seasonality::CalibrationStageExecutionStatus;
use janiform_market_time::calendar::ExchangeCalendarRegistry;

struct ReturnComponentsStagePreflight {
    stage_solver_settings: janiform_calibration_artifacts::CalibrationOptimizerSettings,
    prepared_candidate_generation_input:
        super::signals::ReturnComponentsStagePreparedCandidateGenerationInput,
}

struct ReturnComponentsStageSelectionArtifacts {
    candidate_diagnostics: Vec<ReturnComponentsStageCandidateDiagnostic>,
    selected_candidate_position: usize,
    valid_candidate_count: usize,
    selected_candidate_raw_objective: f64,
    selected_candidate_penalized_objective: f64,
    optimizer_adapter_metadata: CalibrationOptimizerAdapterRunMetadata,
}

/// Execute the return-components stage with NIG and coupled-gap parameter constraints.
pub fn run_return_components_calibration_stage(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: ReturnComponentsStageExecutionInput,
) -> ReturnComponentsStageExecutionReport {
    run_return_components_calibration_stage_with_progress_observer(
        run_spec,
        calendar_registry,
        stage_input,
        None,
    )
}

pub(crate) fn run_return_components_calibration_stage_with_progress_observer(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: ReturnComponentsStageExecutionInput,
    progress_observer: Option<&mut dyn FnMut(ReturnComponentsStageProgressUpdate)>,
) -> ReturnComponentsStageExecutionReport {
    let Some(progress_observer) = progress_observer else {
        return run_return_components_calibration_stage_internal(
            run_spec,
            calendar_registry,
            stage_input,
            None,
        );
    };

    let (progress_sender, progress_receiver) =
        mpsc::channel::<ReturnComponentsStageProgressUpdate>();

    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            run_return_components_calibration_stage_internal(
                run_spec,
                calendar_registry,
                stage_input,
                Some(progress_sender),
            )
        });

        for progress_update in progress_receiver {
            progress_observer(progress_update);
        }

        match worker.join() {
            Ok(report) => report,
            Err(_) => build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::CandidateGenerationFailed {
                    error_message: "return components progress worker terminated unexpectedly"
                        .to_string(),
                },
            ),
        }
    })
}

fn run_return_components_calibration_stage_internal(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: ReturnComponentsStageExecutionInput,
    progress_sender: Option<Sender<ReturnComponentsStageProgressUpdate>>,
) -> ReturnComponentsStageExecutionReport {
    let preflight = match prepare_return_components_stage_execution(
        run_spec,
        calendar_registry,
        &stage_input,
        progress_sender.as_ref(),
    ) {
        Ok(preflight) => preflight,
        Err(failure) => return build_return_components_stage_failure_report(run_spec, failure),
    };

    let resolved_stage_seed =
        resolve_calibration_stage_seed_metadata(run_spec, CalibrationStage::ReturnComponents);

    let generated_candidates = match generate_stage_candidates(
        &stage_input,
        &preflight,
        resolved_stage_seed.stage_seed,
        progress_sender.as_ref(),
    ) {
        Ok(generated_candidates) => generated_candidates,
        Err(failure) => return build_return_components_stage_failure_report(run_spec, failure),
    };

    let selection = match select_generated_candidate(
        &preflight.stage_solver_settings,
        &generated_candidates,
        resolved_stage_seed.stage_seed,
        progress_sender.as_ref(),
    ) {
        Ok(selection) => selection,
        Err(failure) => return build_return_components_stage_failure_report(run_spec, failure),
    };

    match build_return_components_stage_success_report(
        run_spec,
        stage_input,
        preflight.stage_solver_settings,
        generated_candidates,
        selection,
        resolved_stage_seed,
        progress_sender.as_ref(),
    ) {
        Ok(report) => report,
        Err(failure) => build_return_components_stage_failure_report(run_spec, failure),
    }
}

fn prepare_return_components_stage_execution(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: &ReturnComponentsStageExecutionInput,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> Result<ReturnComponentsStagePreflight, ReturnComponentsStageExecutionFailure> {
    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::InputPreflight,
        0,
        3,
        Some("validate run spec"),
    );

    validate_stage_run_spec(run_spec)?;

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::InputPreflight,
        1,
        3,
        Some("resolve solver settings"),
    );

    let stage_solver_settings =
        stage_solver_settings_for_stage(run_spec, CalibrationStage::ReturnComponents)
            .ok_or(ReturnComponentsStageExecutionFailure::MissingStageSolverSettings)?;

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::InputPreflight,
        2,
        3,
        Some("prepare candidate input"),
    );

    stage_input
        .candidate_generation_input
        .validate()
        .map_err(|source| {
            ReturnComponentsStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: source.to_string(),
            }
        })?;

    let prepared_candidate_generation_input = prepare_candidate_generation_input(
        &run_spec.data_inputs.exchange_code,
        calendar_registry,
        stage_input.candidate_generation_input.clone(),
    )
    .map_err(|source| {
        ReturnComponentsStageExecutionFailure::InvalidCandidateGenerationInput {
            error_message: source.to_string(),
        }
    })?;

    validate_activity_input_transport(run_spec, stage_input)?;

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::InputPreflight,
        3,
        3,
        Some("ready"),
    );

    Ok(ReturnComponentsStagePreflight {
        stage_solver_settings,
        prepared_candidate_generation_input,
    })
}

fn validate_stage_run_spec(
    run_spec: &CalibrationRunSpec,
) -> Result<(), ReturnComponentsStageExecutionFailure> {
    run_spec.validate().map_err(|source| {
        ReturnComponentsStageExecutionFailure::InvalidRunSpec {
            error_message: format!("{source} (classification={:?})", source.classification()),
        }
    })?;

    if !run_spec
        .selected_stages
        .iter()
        .copied()
        .any(|selected_stage| selected_stage == CalibrationStage::ReturnComponents)
    {
        return Err(ReturnComponentsStageExecutionFailure::StageIsNotSelectedInRunSpec);
    }

    Ok(())
}

fn validate_activity_input_transport(
    run_spec: &CalibrationRunSpec,
    stage_input: &ReturnComponentsStageExecutionInput,
) -> Result<(), ReturnComponentsStageExecutionFailure> {
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
        return Err(
            ReturnComponentsStageExecutionFailure::ActivityInputTransportMismatch {
                expected_transport,
                provided_transport,
            },
        );
    }

    if provided_transport != CalibrationStageHandoffTransport::PersistedArtifactReference {
        return Ok(());
    }

    let Some(provided_artifact_reference) = stage_input
        .activity_input
        .artifact_reference()
        .map(ToOwned::to_owned)
    else {
        return Err(ReturnComponentsStageExecutionFailure::MissingArtifactReferenceInActivityInput);
    };

    if provided_artifact_reference.trim().is_empty() {
        return Err(
            ReturnComponentsStageExecutionFailure::InvalidArtifactReferenceInActivityInput {
                artifact_reference: provided_artifact_reference,
            },
        );
    }

    if let Some(expected_stage_handoff) = expected_activity_handoff
        && let Some(expected_artifact_reference) = expected_stage_handoff.artifact_reference()
        && expected_artifact_reference != provided_artifact_reference
    {
        return Err(
            ReturnComponentsStageExecutionFailure::ActivityArtifactReferenceMismatch {
                expected_artifact_reference: expected_artifact_reference.to_string(),
                provided_artifact_reference,
            },
        );
    }

    Ok(())
}

fn generate_stage_candidates(
    stage_input: &ReturnComponentsStageExecutionInput,
    preflight: &ReturnComponentsStagePreflight,
    stage_seed: u64,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> Result<
    super::candidate_generation::ReturnComponentsStageCandidateGenerationOutput,
    ReturnComponentsStageExecutionFailure,
> {
    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::CloseOpenGapPreparation,
        1,
        1,
        Some(
            preflight
                .prepared_candidate_generation_input
                .close_open_gap_detection_filter
                .as_str(),
        ),
    );

    let generated_candidates = generate_return_components_stage_candidate_inputs(
        &preflight
            .prepared_candidate_generation_input
            .candidate_generation_input,
        preflight
            .prepared_candidate_generation_input
            .derived_close_open_gap_candidates
            .as_slice(),
        preflight
            .prepared_candidate_generation_input
            .close_open_gap_detection_filter
            .as_str(),
        resolve_activity_calibration_context(&stage_input.activity_input),
        stage_seed,
        &preflight.stage_solver_settings,
        progress_sender,
    )
    .map_err(
        |source| ReturnComponentsStageExecutionFailure::CandidateGenerationFailed {
            error_message: source.to_string(),
        },
    )?;

    if generated_candidates.candidate_inputs.is_empty() {
        return Err(
            ReturnComponentsStageExecutionFailure::CandidateGenerationFailed {
                error_message: "candidate generation completed but produced zero candidate_inputs"
                    .to_string(),
            },
        );
    }

    Ok(generated_candidates)
}

fn select_generated_candidate(
    stage_solver_settings: &janiform_calibration_artifacts::CalibrationOptimizerSettings,
    generated_candidates: &super::candidate_generation::ReturnComponentsStageCandidateGenerationOutput,
    stage_seed: u64,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> Result<ReturnComponentsStageSelectionArtifacts, ReturnComponentsStageExecutionFailure> {
    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::CandidateSelection,
        0,
        3,
        Some("build optimizer configuration"),
    );

    let optimizer_adapter_configuration = CalibrationOptimizerAdapterConfiguration::try_new(
        "candidate_enumeration.v1",
        stage_seed,
        stage_solver_settings.clone(),
        return_components_stage_optimizer_bounds(),
    )
    .map_err(
        |source| ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
            candidate_id: "optimizer_adapter_configuration".to_string(),
            error_message: source.to_string(),
        },
    )?;

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::CandidateConstraintEvaluation,
        0,
        generated_candidates.candidate_inputs.len(),
        Some("evaluate generated candidate constraints"),
    );

    let (adapter_candidate_inputs, external_constraint_diagnostics) =
        build_optimizer_candidate_inputs(generated_candidates, progress_sender)?;

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::CandidateSelection,
        1,
        3,
        Some("select optimizer candidate"),
    );

    let optimizer_adapter_output = run_constrained_optimizer_adapter(
        &optimizer_adapter_configuration,
        &adapter_candidate_inputs,
    )
    .map_err(map_optimizer_adapter_error)?;

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::CandidateSelection,
        2,
        3,
        Some("build selection artifacts"),
    );

    let selection_artifacts = build_selection_artifacts(
        generated_candidates,
        external_constraint_diagnostics,
        optimizer_adapter_output,
    )?;

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::CandidateSelection,
        3,
        3,
        Some("selected generated candidate"),
    );

    Ok(selection_artifacts)
}

fn build_optimizer_candidate_inputs(
    generated_candidates: &super::candidate_generation::ReturnComponentsStageCandidateGenerationOutput,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> Result<
    (
        Vec<CalibrationOptimizerAdapterCandidateInput>,
        Vec<Vec<ReturnComponentsStageConstraintViolationDiagnostic>>,
    ),
    ReturnComponentsStageExecutionFailure,
> {
    let mut seen_candidate_ids = BTreeSet::new();

    let mut external_constraint_diagnostics =
        Vec::<Vec<ReturnComponentsStageConstraintViolationDiagnostic>>::with_capacity(
            generated_candidates.candidate_inputs.len(),
        );

    let mut adapter_candidate_inputs =
        Vec::with_capacity(generated_candidates.candidate_inputs.len());

    let total_candidate_count = generated_candidates.candidate_inputs.len();
    for (candidate_position, candidate_input) in
        generated_candidates.candidate_inputs.iter().enumerate()
    {
        if !seen_candidate_ids.insert(candidate_input.candidate_id.clone()) {
            return Err(
                ReturnComponentsStageExecutionFailure::DuplicateCandidateId {
                    candidate_id: candidate_input.candidate_id.clone(),
                },
            );
        }

        if let Err(source) = candidate_input.validate() {
            return Err(
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

        send_return_components_progress(
            progress_sender,
            ReturnComponentsStageProgressPhase::CandidateConstraintEvaluation,
            candidate_position + 1,
            total_candidate_count,
            Some(candidate_input.candidate_id.as_str()),
        );
    }

    Ok((adapter_candidate_inputs, external_constraint_diagnostics))
}

fn map_optimizer_adapter_error(
    source: CalibrationOptimizerAdapterError,
) -> ReturnComponentsStageExecutionFailure {
    match source {
        CalibrationOptimizerAdapterError::InvalidCandidateInput {
            candidate_id,
            source,
        } => ReturnComponentsStageExecutionFailure::InvalidCandidateInput {
            candidate_id,
            error_message: source.to_string(),
        },
        CalibrationOptimizerAdapterError::InvalidObjectiveComposition {
            candidate_id,
            source,
        } => ReturnComponentsStageExecutionFailure::InvalidCandidateInput {
            candidate_id,
            error_message: source.to_string(),
        },
        CalibrationOptimizerAdapterError::DuplicateCandidateId { candidate_id } => {
            ReturnComponentsStageExecutionFailure::DuplicateCandidateId { candidate_id }
        }
        source => ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
            candidate_id: "optimizer_adapter".to_string(),
            error_message: source.to_string(),
        },
    }
}

fn build_selection_artifacts(
    generated_candidates: &super::candidate_generation::ReturnComponentsStageCandidateGenerationOutput,
    external_constraint_diagnostics: Vec<Vec<ReturnComponentsStageConstraintViolationDiagnostic>>,
    optimizer_adapter_output: CalibrationOptimizerAdapterRunOutput,
) -> Result<ReturnComponentsStageSelectionArtifacts, ReturnComponentsStageExecutionFailure> {
    let candidate_diagnostics = build_candidate_diagnostics(
        generated_candidates,
        external_constraint_diagnostics,
        &optimizer_adapter_output,
    );

    let Some(selected_candidate_id) = optimizer_adapter_output.selected_candidate_id.as_ref()
    else {
        return Err(
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
        return Err(
            ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: "optimizer_adapter_selection".to_string(),
                error_message: "selected candidate id was not found in generated candidate_inputs"
                    .to_string(),
            },
        );
    };

    Ok(ReturnComponentsStageSelectionArtifacts {
        selected_candidate_raw_objective: candidate_diagnostics[selected_candidate_position]
            .raw_objective_value,
        selected_candidate_penalized_objective: candidate_diagnostics[selected_candidate_position]
            .penalized_objective_value,
        valid_candidate_count: optimizer_adapter_output
            .candidate_diagnostics
            .iter()
            .filter(|candidate_diagnostic| {
                candidate_diagnostic.total_constraint_violation_count == 0
            })
            .count(),
        selected_candidate_position,
        candidate_diagnostics,
        optimizer_adapter_metadata: optimizer_adapter_output.metadata,
    })
}

fn build_candidate_diagnostics(
    generated_candidates: &super::candidate_generation::ReturnComponentsStageCandidateGenerationOutput,
    external_constraint_diagnostics: Vec<Vec<ReturnComponentsStageConstraintViolationDiagnostic>>,
    optimizer_adapter_output: &CalibrationOptimizerAdapterRunOutput,
) -> Vec<ReturnComponentsStageCandidateDiagnostic> {
    let mut candidate_diagnostics = Vec::with_capacity(generated_candidates.candidate_inputs.len());

    for ((candidate_input, constraint_violations), optimizer_diagnostic) in generated_candidates
        .candidate_inputs
        .iter()
        .zip(external_constraint_diagnostics)
        .zip(optimizer_adapter_output.candidate_diagnostics.iter())
    {
        let mut combined_constraint_violations: Vec<
            ReturnComponentsStageConstraintViolationDiagnostic,
        > = constraint_violations;

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
            continuous_distribution_fit_breakdown: candidate_input
                .continuous_distribution_fit_breakdown
                .clone(),
            surprise_arrival_diagnostic: candidate_input.surprise_arrival_diagnostic.clone(),
            jump_diagnostic: candidate_input.jump_diagnostic.clone(),
            gap_diagnostic: candidate_input.gap_diagnostic.clone(),
            activity_coupling_diagnostic: candidate_input.activity_coupling_diagnostic.clone(),
            price_range_sanity_diagnostic: candidate_input.price_range_sanity_diagnostic.clone(),
            dominant_scale_driver: candidate_input.dominant_scale_driver.clone(),
        });
    }

    candidate_diagnostics
}

fn build_return_components_stage_success_report(
    run_spec: &CalibrationRunSpec,
    stage_input: ReturnComponentsStageExecutionInput,
    stage_solver_settings: janiform_calibration_artifacts::CalibrationOptimizerSettings,
    generated_candidates: super::candidate_generation::ReturnComponentsStageCandidateGenerationOutput,
    selection: ReturnComponentsStageSelectionArtifacts,
    resolved_stage_seed: CalibrationResolvedStageSeed,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> Result<ReturnComponentsStageExecutionReport, ReturnComponentsStageExecutionFailure> {
    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::ReportAssembly,
        0,
        2,
        Some("validate selected candidate"),
    );

    let stage_seed = resolved_stage_seed.stage_seed;
    let candidate_generation_seed = stage_seed;

    let selected_candidate_input =
        &generated_candidates.candidate_inputs[selection.selected_candidate_position];

    let nig_configuration = validated_selected_nig_configuration(selected_candidate_input)?;
    let news_process_configuration =
        validated_selected_news_process_configuration(selected_candidate_input)?;

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::ReportAssembly,
        1,
        2,
        Some("build fitted payload"),
    );

    let fitted_stage_payload = super::super::contracts::ReturnComponentsStageFittedPayload {
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
            mean_reversion_fitted_parameters:
                MeanReversionFittedParameters::zero_kappa_flat_anchor_mean_reversion(),
        },
        candidate_diagnostics: selection.candidate_diagnostics,
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

    let report = ReturnComponentsStageExecutionReport {
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
            optimizer_adapter_metadata: selection.optimizer_adapter_metadata,
            input_candidate_count: generated_candidates.candidate_inputs.len(),
            valid_candidate_count: selection.valid_candidate_count,
            selected_candidate_raw_objective: selection.selected_candidate_raw_objective,
            selected_candidate_penalized_objective: selection
                .selected_candidate_penalized_objective,
            objective_weighting_policy:
                CALIBRATION_RETURN_COMPONENTS_STAGE_OBJECTIVE_WEIGHTING_POLICY,
            objective_metric_bucket_policy: RETURN_COMPONENTS_STAGE_OBJECTIVE_METRIC_BUCKET_POLICY,
            candidate_generation_policy: RETURN_COMPONENTS_STAGE_CANDIDATE_GENERATION_POLICY,
            candidate_family_counts: generated_candidates.candidate_family_counts,
        }),
        failure: None,
    };

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::ReportAssembly,
        2,
        2,
        Some("report ready"),
    );

    Ok(report)
}

fn validated_selected_nig_configuration(
    selected_candidate_input: &super::super::contracts::ReturnComponentsStageCandidateInput,
) -> Result<NigSharedPlusShiftConfiguration, ReturnComponentsStageExecutionFailure> {
    let nig_configuration = NigSharedPlusShiftConfiguration::try_new(
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
    )
    .map_err(
        |source| ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
            candidate_id: selected_candidate_input.candidate_id.clone(),
            error_message: source.to_string(),
        },
    )?;

    if !nig_configuration.delta_beta.is_finite() || nig_configuration.delta_beta >= 0.0 {
        return Err(
            ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: selected_candidate_input.candidate_id.clone(),
                error_message:
                    "nig_shared_plus_shift_configuration.delta_beta must be finite and strictly negative for return skew asymmetry".to_string(),
            },
        );
    }

    Ok(nig_configuration)
}

fn validated_selected_news_process_configuration(
    selected_candidate_input: &super::super::contracts::ReturnComponentsStageCandidateInput,
) -> Result<NewsProcessConfiguration, ReturnComponentsStageExecutionFailure> {
    NewsProcessConfiguration::try_new(
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
    )
    .map_err(
        |source| ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
            candidate_id: selected_candidate_input.candidate_id.clone(),
            error_message: source.to_string(),
        },
    )
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
