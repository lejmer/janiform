use janiform_calibration_artifacts::{
    CALIBRATION_OBSERVATION_MODELS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_OBSERVATION_MODELS_STAGE_HANDOFF_POLICY,
    CALIBRATION_OBSERVATION_MODELS_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_STAGE_SEED_DERIVATION_POLICY, CalibrationOptimizerSettings, CalibrationRunSpec,
    CalibrationStage, ObservationModelsStageDiagnostics, resolve_calibration_stage_seed_metadata,
    stage_solver_settings_for_stage,
};
use janiform_model_kernels::no_move_event::NoMoveEventModel;
use janiform_model_kernels::return_clock::SideSourceActivityDecomposition;
use janiform_model_kernels::timestamp_observation::TimestampBatchingObservationModel;

use crate::stages::observation_models::contracts::{
    ObservationModelsOpenSessionTimestampTransition, ObservationModelsStageExecutionFailure,
    ObservationModelsStageExecutionInput, ObservationModelsStageExecutionMetadata,
    ObservationModelsStageExecutionReport, ObservationModelsStageFittedPayload,
};
use crate::stages::observation_models::hawkes_source_fit::fit_open_session_return_source_activities;
use crate::stages::observation_models::no_move_fit::fit_no_move_event_model_from_open_session_returns;
use crate::stages::observation_models::progress::{
    ObservationModelsStageProgressPhase, ObservationModelsStageProgressUpdate,
    emit_observation_models_progress,
};
use crate::stages::observation_models::timestamp_fit::{
    TimestampBatchingObservationFit, timestamp_batching_observation_fit,
    timestamp_batching_observation_model_from_fit,
};
use crate::stages::return_components::open_session_returns_with_source_activities;
use crate::stages::seasonality::CalibrationStageExecutionStatus;

const OBSERVATION_MODELS_STAGE_ESTIMATION_POLICY: &str =
    "hawkes_source_weighted_no_move_and_timestamp_batching_empirical_fit.v1";

pub fn run_observation_models_calibration_stage(
    run_spec: &CalibrationRunSpec,
    execution_input: ObservationModelsStageExecutionInput,
) -> ObservationModelsStageExecutionReport {
    run_observation_models_calibration_stage_internal(run_spec, execution_input, None)
}

pub(crate) fn run_observation_models_calibration_stage_with_progress_observer(
    run_spec: &CalibrationRunSpec,
    execution_input: ObservationModelsStageExecutionInput,
    progress_observer: Option<&mut dyn FnMut(ObservationModelsStageProgressUpdate)>,
) -> ObservationModelsStageExecutionReport {
    run_observation_models_calibration_stage_internal(run_spec, execution_input, progress_observer)
}

fn run_observation_models_calibration_stage_internal(
    run_spec: &CalibrationRunSpec,
    execution_input: ObservationModelsStageExecutionInput,
    mut progress_observer: Option<&mut dyn FnMut(ObservationModelsStageProgressUpdate)>,
) -> ObservationModelsStageExecutionReport {
    let solver_settings = match preflight_observation_models_stage(
        run_spec,
        &execution_input,
        &mut progress_observer,
    ) {
        Ok(solver_settings) => solver_settings,
        Err(failure) => return failure_report(run_spec, failure),
    };

    emit_observation_models_progress(
        &mut progress_observer,
        ObservationModelsStageProgressPhase::NoMoveEventModelFit,
        1,
        4,
        Some("no-move events"),
    );

    let (no_move_event_model, open_session_return_source_activities) =
        match fit_source_weighted_no_move_event_model(&execution_input) {
            Ok(fit) => fit,
            Err(failure) => return failure_report(run_spec, failure),
        };

    emit_observation_models_progress(
        &mut progress_observer,
        ObservationModelsStageProgressPhase::TimestampBatchingModelFit,
        2,
        4,
        Some("timestamp batching"),
    );

    let (timestamp_batching_observation_model, timestamp_fit) =
        match fit_timestamp_batching_observation_model(
            execution_input
                .open_session_timestamp_transitions
                .as_slice(),
        ) {
            Ok(fit) => fit,
            Err(failure) => return failure_report(run_spec, failure),
        };

    emit_observation_models_progress(
        &mut progress_observer,
        ObservationModelsStageProgressPhase::ReportAssembly,
        3,
        4,
        Some("stage report"),
    );

    let report = success_report(
        run_spec,
        solver_settings,
        execution_input.observed_open_session_returns.len(),
        no_move_event_model,
        timestamp_batching_observation_model,
        timestamp_fit,
        open_session_return_source_activities,
    );

    emit_observation_models_progress(
        &mut progress_observer,
        ObservationModelsStageProgressPhase::ReportAssembly,
        4,
        4,
        Some("complete"),
    );

    report
}

fn fit_source_weighted_no_move_event_model(
    execution_input: &ObservationModelsStageExecutionInput,
) -> Result<
    (NoMoveEventModel, Vec<SideSourceActivityDecomposition>),
    ObservationModelsStageExecutionFailure,
> {
    let source_responsibility_input = execution_input
        .source_responsibility_input
        .as_ref()
        .ok_or(ObservationModelsStageExecutionFailure::MissingHawkesSourceResponsibilityInput)?;

    let source_activity_fit = fit_open_session_return_source_activities(
        execution_input.observed_open_session_returns.as_slice(),
        &source_responsibility_input.seasonality_payload,
        source_responsibility_input.event_side_codes.as_slice(),
        &source_responsibility_input.hawkes_fitted_parameters,
    )
    .map_err(|source| {
        ObservationModelsStageExecutionFailure::InvalidHawkesSourceResponsibilityInput {
            error_message: source.to_string(),
        }
    })?;

    let observed_open_session_returns = open_session_returns_with_source_activities(
        execution_input.observed_open_session_returns.as_slice(),
        source_activity_fit.source_activities.as_slice(),
    )
    .map_err(|source| {
        ObservationModelsStageExecutionFailure::InvalidHawkesSourceResponsibilityInput {
            error_message: source.to_string(),
        }
    })?;

    let no_move_event_model =
        fit_no_move_event_model_from_open_session_returns(observed_open_session_returns.as_slice());

    no_move_event_model.validate().map_err(|source| {
        ObservationModelsStageExecutionFailure::InvalidNoMoveEventModel {
            error_message: source.to_string(),
        }
    })?;

    Ok((no_move_event_model, source_activity_fit.source_activities))
}

fn fit_timestamp_batching_observation_model(
    open_session_timestamp_transitions: &[ObservationModelsOpenSessionTimestampTransition],
) -> Result<
    (
        TimestampBatchingObservationModel,
        TimestampBatchingObservationFit,
    ),
    ObservationModelsStageExecutionFailure,
> {
    let timestamp_fit = timestamp_batching_observation_fit(open_session_timestamp_transitions);

    let timestamp_batching_observation_model =
        timestamp_batching_observation_model_from_fit(&timestamp_fit).map_err(|error_message| {
            ObservationModelsStageExecutionFailure::InvalidTimestampBatchingObservationModel {
                error_message,
            }
        })?;

    Ok((timestamp_batching_observation_model, timestamp_fit))
}

fn preflight_observation_models_stage(
    run_spec: &CalibrationRunSpec,
    execution_input: &ObservationModelsStageExecutionInput,
    progress_observer: &mut Option<&mut dyn FnMut(ObservationModelsStageProgressUpdate)>,
) -> Result<CalibrationOptimizerSettings, ObservationModelsStageExecutionFailure> {
    emit_observation_models_progress(
        progress_observer,
        ObservationModelsStageProgressPhase::InputPreflight,
        0,
        4,
        Some("run spec"),
    );

    if let Err(error_message) = run_spec.validate() {
        return Err(ObservationModelsStageExecutionFailure::InvalidRunSpec {
            error_message: error_message.to_string(),
        });
    }

    if !run_spec
        .selected_stages
        .contains(&CalibrationStage::ObservationModels)
    {
        return Err(ObservationModelsStageExecutionFailure::StageIsNotSelectedInRunSpec);
    }

    let Some(solver_settings) =
        stage_solver_settings_for_stage(run_spec, CalibrationStage::ObservationModels)
    else {
        return Err(ObservationModelsStageExecutionFailure::MissingStageSolverSettings);
    };

    if execution_input.observed_open_session_returns.is_empty() {
        return Err(ObservationModelsStageExecutionFailure::MissingObservedOpenSessionReturns);
    }

    Ok(solver_settings)
}

fn success_report(
    run_spec: &CalibrationRunSpec,
    solver_settings: CalibrationOptimizerSettings,
    observed_open_session_return_count: usize,
    no_move_event_model: NoMoveEventModel,
    timestamp_batching_observation_model: TimestampBatchingObservationModel,
    timestamp_fit: TimestampBatchingObservationFit,
    open_session_return_source_activities: Vec<SideSourceActivityDecomposition>,
) -> ObservationModelsStageExecutionReport {
    let seed_metadata =
        resolve_calibration_stage_seed_metadata(run_spec, CalibrationStage::ObservationModels);

    let diagnostics = ObservationModelsStageDiagnostics {
        estimation_policy: OBSERVATION_MODELS_STAGE_ESTIMATION_POLICY.to_string(),
        observed_open_session_return_count,
        same_session_timestamp_transition_count: timestamp_fit
            .same_session_observation_transition_count,
        same_timestamp_continuation_count: timestamp_fit.same_timestamp_continuation_count,
    };

    let fitted_stage_payload = ObservationModelsStageFittedPayload {
        schema_version: CALIBRATION_OBSERVATION_MODELS_STAGE_OUTPUT_SCHEMA_VERSION,
        no_move_event_model,
        timestamp_batching_observation_model,
        open_session_return_source_activities,
        diagnostics,
    };

    ObservationModelsStageExecutionReport {
        schema_version: CALIBRATION_OBSERVATION_MODELS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::ObservationModels,
        status: CalibrationStageExecutionStatus::Success,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_OBSERVATION_MODELS_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: Some(fitted_stage_payload),
        metadata: Some(ObservationModelsStageExecutionMetadata {
            stage_output_schema_version: CALIBRATION_OBSERVATION_MODELS_STAGE_OUTPUT_SCHEMA_VERSION,
            run_global_seed: seed_metadata.run_global_seed,
            stage_seed: seed_metadata.stage_seed,
            stage_seed_source: seed_metadata.stage_seed_source,
            stage_seed_derivation_policy: CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
            solver_settings,
            observed_open_session_return_count,
            same_session_timestamp_transition_count: timestamp_fit
                .same_session_observation_transition_count,
        }),
        failure: None,
    }
}

fn failure_report(
    run_spec: &CalibrationRunSpec,
    failure: ObservationModelsStageExecutionFailure,
) -> ObservationModelsStageExecutionReport {
    ObservationModelsStageExecutionReport {
        schema_version: CALIBRATION_OBSERVATION_MODELS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::ObservationModels,
        status: CalibrationStageExecutionStatus::Failure,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_OBSERVATION_MODELS_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: None,
        metadata: None,
        failure: Some(failure),
    }
}
