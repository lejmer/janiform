mod progress;
mod variance_handoff;

use std::sync::Arc;
use std::sync::mpsc::{self, Sender};

use janiform_model_kernels::market_pressure::MarketPressureParameters;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use serde::Serialize;

use janiform_calibration_artifacts::{
    CALIBRATION_RETURN_BACKFIT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_RETURN_BACKFIT_STAGE_HANDOFF_POLICY,
    CALIBRATION_RETURN_BACKFIT_STAGE_OUTPUT_SCHEMA_VERSION, CalibrationExecutionMode,
    CalibrationOptimizerSettings, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStageSeedSource,
    CalibrationStageSelectedCandidateSource, ReturnBackfitStageDiagnostics,
    ReturnBackfitStageScaleHandoffDiagnostic, stage_solver_settings_for_stage,
};

use crate::market_data::loading::MarketDataNormalizedCoarseDataRecord;
use crate::market_pressure::{
    MarketPressureFitProgress, MarketPressureNigFitInput, MarketPressureRuntimeState,
    fit_market_pressure_from_nig_configuration, market_pressure_parameters_from_runtime_state,
};
use crate::stages::hawkes::HawkesStageFittedPayload;
use crate::stages::long_variance::LongVarianceStageFittedPayload;
use crate::stages::long_variance::generated_energy::project_generated_stationary_feedback_to_budget;
use crate::stages::news_shocks::NewsShocksStageFittedPayload;
use crate::stages::return_components::{
    ReturnComponentsStageCandidateDiagnostic, ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageFittedPayload,
};
use crate::stages::seasonality::CalibrationStageExecutionStatus;
use progress::{
    RETURN_BACKFIT_STAGE_PROGRESS_UNITS, emit_return_backfit_progress,
    emit_return_backfit_subphase_progress,
};
pub use progress::{ReturnBackfitStageProgressPhase, ReturnBackfitStageProgressUpdate};
use variance_handoff::{ReturnBackfitVarianceHandoffInput, return_backfit_delta_variance_handoff};

const RETURN_BACKFIT_ESTIMATION_POLICY: &str =
    "nig_delta_variance_handoff_under_long_variance_news_pressure_clock.v1";
const RETURN_BACKFIT_TRANSFORMATION_ID: &str = "nig_delta_variance_handoff_under_dynamic_clock";

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReturnBackfitStageExecutionInput {
    pub return_components_input: Option<CalibrationStageSelectedCandidateSource>,
    pub hawkes_stage_payload: Option<HawkesStageFittedPayload>,
    pub candidate_generation_input: Option<ReturnComponentsStageCandidateGenerationInput>,
    pub coarse_data_records: Option<Vec<MarketDataNormalizedCoarseDataRecord>>,
    pub return_components_stage_payload: Option<ReturnComponentsStageFittedPayload>,
    pub long_variance_stage_payload: Option<LongVarianceStageFittedPayload>,
    pub news_shocks_stage_payload: Option<NewsShocksStageFittedPayload>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnBackfitStageFittedPayload {
    pub schema_version: &'static str,
    pub selected_candidate_id: String,
    pub effective_return_components_payload: ReturnComponentsStageFittedPayload,
    pub diagnostics: ReturnBackfitStageDiagnostics,
    pub return_components_input_transport: CalibrationStageHandoffTransport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_components_selected_candidate_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_components_artifact_reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnBackfitStageExecutionMetadata {
    pub stage_output_schema_version: &'static str,
    pub run_global_seed: u64,
    pub stage_seed: u64,
    pub stage_seed_source: CalibrationStageSeedSource,
    pub stage_seed_derivation_policy: &'static str,
    pub solver_settings: CalibrationOptimizerSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum ReturnBackfitStageExecutionFailure {
    InvalidRunSpec { error_message: String },
    StageIsNotSelectedInRunSpec,
    MissingStageSolverSettings,
    MissingReturnComponentsInput,
    MissingHawkesStagePayload,
    MissingCandidateGenerationInput,
    MissingReturnComponentsStagePayload,
    MissingLongVarianceStagePayload,
    MissingNewsShocksStagePayload,
    InvalidBackfitInput { error_message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnBackfitStageExecutionReport {
    pub schema_version: &'static str,
    pub stage: CalibrationStage,
    pub status: CalibrationStageExecutionStatus,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub handoff_policy: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fitted_stage_payload: Option<ReturnBackfitStageFittedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ReturnBackfitStageExecutionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<ReturnBackfitStageExecutionFailure>,
}

struct ReturnBackfitFittedStagePayloadInput<'a> {
    hawkes_stage_payload: &'a HawkesStageFittedPayload,
    candidate_generation_input: &'a ReturnComponentsStageCandidateGenerationInput,
    coarse_data_records: Option<&'a [MarketDataNormalizedCoarseDataRecord]>,
    return_components_input: CalibrationStageSelectedCandidateSource,
    effective_return_components_payload: ReturnComponentsStageFittedPayload,
    long_variance_stage_payload: LongVarianceStageFittedPayload,
    news_shocks_stage_payload: NewsShocksStageFittedPayload,
    solver_settings: &'a CalibrationOptimizerSettings,
    progress_sender: Option<Sender<ReturnBackfitStageProgressUpdate>>,
}

struct ReturnBackfitMarketPressureRefitInput<'a> {
    candidate_generation_input: &'a ReturnComponentsStageCandidateGenerationInput,
    coarse_data_records: Option<&'a [MarketDataNormalizedCoarseDataRecord]>,
    nig_configuration: NigSharedPlusShiftConfiguration,
    source_window_open_exposure_hours: f64,
    solver_settings: &'a CalibrationOptimizerSettings,
    runtime_state: Option<&'a Arc<MarketPressureRuntimeState>>,
    progress_sender: Option<Sender<ReturnBackfitStageProgressUpdate>>,
}

struct ReturnBackfitMarketPressureRefitOutput {
    parameters: MarketPressureParameters,
    runtime_state: Option<Arc<MarketPressureRuntimeState>>,
}

pub fn run_return_backfit_calibration_stage(
    run_spec: &CalibrationRunSpec,
    execution_input: ReturnBackfitStageExecutionInput,
) -> ReturnBackfitStageExecutionReport {
    run_return_backfit_calibration_stage_internal(run_spec, execution_input, None)
}

pub(crate) fn run_return_backfit_calibration_stage_with_progress_observer(
    run_spec: &CalibrationRunSpec,
    execution_input: ReturnBackfitStageExecutionInput,
    progress_observer: Option<&mut dyn FnMut(ReturnBackfitStageProgressUpdate)>,
) -> ReturnBackfitStageExecutionReport {
    let Some(progress_observer) = progress_observer else {
        return run_return_backfit_calibration_stage_internal(run_spec, execution_input, None);
    };

    let (progress_sender, progress_receiver) = mpsc::channel::<ReturnBackfitStageProgressUpdate>();

    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            run_return_backfit_calibration_stage_internal(
                run_spec,
                execution_input,
                Some(progress_sender),
            )
        });

        for progress_update in progress_receiver {
            progress_observer(progress_update);
        }

        match worker.join() {
            Ok(report) => report,
            Err(_) => failure_report(
                run_spec,
                ReturnBackfitStageExecutionFailure::InvalidBackfitInput {
                    error_message: "return backfit progress worker terminated unexpectedly"
                        .to_string(),
                },
            ),
        }
    })
}

fn run_return_backfit_calibration_stage_internal(
    run_spec: &CalibrationRunSpec,
    execution_input: ReturnBackfitStageExecutionInput,
    progress_sender: Option<Sender<ReturnBackfitStageProgressUpdate>>,
) -> ReturnBackfitStageExecutionReport {
    emit_return_backfit_progress(
        progress_sender.as_ref(),
        ReturnBackfitStageProgressPhase::InputPreflight,
        0,
        RETURN_BACKFIT_STAGE_PROGRESS_UNITS,
        Some("validate run spec"),
    );

    if let Err(error_message) = run_spec.validate() {
        return failure_report(
            run_spec,
            ReturnBackfitStageExecutionFailure::InvalidRunSpec {
                error_message: error_message.to_string(),
            },
        );
    }

    if !run_spec
        .selected_stages
        .contains(&CalibrationStage::ReturnBackfit)
    {
        return failure_report(
            run_spec,
            ReturnBackfitStageExecutionFailure::StageIsNotSelectedInRunSpec,
        );
    }

    let Some(solver_settings) =
        stage_solver_settings_for_stage(run_spec, CalibrationStage::ReturnBackfit)
    else {
        return failure_report(
            run_spec,
            ReturnBackfitStageExecutionFailure::MissingStageSolverSettings,
        );
    };

    let Some(return_components_input) = execution_input.return_components_input else {
        return failure_report(
            run_spec,
            ReturnBackfitStageExecutionFailure::MissingReturnComponentsInput,
        );
    };

    let Some(hawkes_stage_payload) = execution_input.hawkes_stage_payload else {
        return failure_report(
            run_spec,
            ReturnBackfitStageExecutionFailure::MissingHawkesStagePayload,
        );
    };

    let Some(candidate_generation_input) = execution_input.candidate_generation_input else {
        return failure_report(
            run_spec,
            ReturnBackfitStageExecutionFailure::MissingCandidateGenerationInput,
        );
    };

    let Some(return_components_stage_payload) = execution_input.return_components_stage_payload
    else {
        return failure_report(
            run_spec,
            ReturnBackfitStageExecutionFailure::MissingReturnComponentsStagePayload,
        );
    };

    let Some(long_variance_stage_payload) = execution_input.long_variance_stage_payload else {
        return failure_report(
            run_spec,
            ReturnBackfitStageExecutionFailure::MissingLongVarianceStagePayload,
        );
    };

    let Some(news_shocks_stage_payload) = execution_input.news_shocks_stage_payload else {
        return failure_report(
            run_spec,
            ReturnBackfitStageExecutionFailure::MissingNewsShocksStagePayload,
        );
    };

    let seed_metadata = janiform_calibration_artifacts::resolve_calibration_stage_seed_metadata(
        run_spec,
        CalibrationStage::ReturnBackfit,
    );

    let fitted_stage_payload =
        match build_return_backfit_fitted_stage_payload(ReturnBackfitFittedStagePayloadInput {
            hawkes_stage_payload: &hawkes_stage_payload,
            candidate_generation_input: &candidate_generation_input,
            coarse_data_records: execution_input.coarse_data_records.as_deref(),
            return_components_input,
            effective_return_components_payload: return_components_stage_payload,
            long_variance_stage_payload,
            news_shocks_stage_payload,
            solver_settings: &solver_settings,
            progress_sender,
        }) {
            Ok(payload) => payload,
            Err(error_message) => {
                return failure_report(
                    run_spec,
                    ReturnBackfitStageExecutionFailure::InvalidBackfitInput { error_message },
                );
            }
        };

    ReturnBackfitStageExecutionReport {
        schema_version: CALIBRATION_RETURN_BACKFIT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::ReturnBackfit,
        status: CalibrationStageExecutionStatus::Success,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_RETURN_BACKFIT_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: Some(fitted_stage_payload),
        metadata: Some(ReturnBackfitStageExecutionMetadata {
            stage_output_schema_version: CALIBRATION_RETURN_BACKFIT_STAGE_OUTPUT_SCHEMA_VERSION,
            run_global_seed: seed_metadata.run_global_seed,
            stage_seed: seed_metadata.stage_seed,
            stage_seed_source: seed_metadata.stage_seed_source,
            stage_seed_derivation_policy:
                janiform_calibration_artifacts::CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
            solver_settings,
        }),
        failure: None,
    }
}

fn build_return_backfit_fitted_stage_payload(
    input: ReturnBackfitFittedStagePayloadInput<'_>,
) -> Result<ReturnBackfitStageFittedPayload, String> {
    let ReturnBackfitFittedStagePayloadInput {
        hawkes_stage_payload,
        candidate_generation_input,
        coarse_data_records,
        return_components_input,
        effective_return_components_payload,
        long_variance_stage_payload,
        news_shocks_stage_payload,
        solver_settings,
        progress_sender,
    } = input;

    let mut effective_return_components_payload = effective_return_components_payload;

    let selected_candidate =
        selected_return_components_candidate(&effective_return_components_payload)?;

    let variance_handoff_progress_sender = progress_sender.clone();

    let variance_handoff_fit =
        return_backfit_delta_variance_handoff(ReturnBackfitVarianceHandoffInput {
            hawkes_stage_payload,
            candidate_generation_input,
            selected_candidate,
            nig_configuration: effective_return_components_payload
                .fitted_parameters
                .nig_shared_plus_shift_configuration,
            return_clock_exposure_parameters: effective_return_components_payload
                .fitted_parameters
                .return_clock_exposure_parameters,
            long_variance_parameters: long_variance_stage_payload.fitted_parameters,
            news_process_configuration: &news_shocks_stage_payload.news_process_configuration,
            surprise_deterministic_intensity_profile: &news_shocks_stage_payload
                .surprise_deterministic_intensity_profile,
            progress_sender: variance_handoff_progress_sender,
        })?;

    let nig_configuration = variance_handoff_fit.post_handoff_nig_configuration;

    effective_return_components_payload
        .fitted_parameters
        .nig_shared_plus_shift_configuration = nig_configuration;

    let long_variance_parameters = project_generated_stationary_feedback_to_budget(
        long_variance_stage_payload.fitted_parameters,
        hawkes_stage_payload.fitted_parameters.kernel_parameters,
        nig_configuration,
        effective_return_components_payload
            .fitted_parameters
            .return_clock_exposure_parameters,
    )?;

    effective_return_components_payload
        .fitted_parameters
        .long_variance_parameters = long_variance_parameters;

    effective_return_components_payload
        .fitted_parameters
        .news_process_configuration = news_shocks_stage_payload.news_process_configuration;

    effective_return_components_payload
        .fitted_parameters
        .surprise_deterministic_intensity_profile =
        news_shocks_stage_payload.surprise_deterministic_intensity_profile;

    let market_pressure_refit =
        refit_return_backfit_market_pressure(ReturnBackfitMarketPressureRefitInput {
            candidate_generation_input,
            coarse_data_records,
            nig_configuration,
            source_window_open_exposure_hours: effective_return_components_payload
                .source_window_open_exposure_hours,
            solver_settings,
            runtime_state: effective_return_components_payload
                .market_pressure_runtime_state
                .as_ref(),
            progress_sender: progress_sender.clone(),
        })?;

    effective_return_components_payload
        .fitted_parameters
        .market_pressure_parameters = market_pressure_refit.parameters;

    effective_return_components_payload.market_pressure_runtime_state =
        market_pressure_refit.runtime_state;

    emit_return_backfit_progress(
        progress_sender.as_ref(),
        ReturnBackfitStageProgressPhase::ReportAssembly,
        5,
        RETURN_BACKFIT_STAGE_PROGRESS_UNITS,
        Some("assemble return backfit report"),
    );

    let return_components_input_transport = return_components_input.transport();

    let (return_components_selected_candidate_id, return_components_artifact_reference) =
        match return_components_input {
            CalibrationStageSelectedCandidateSource::InMemoryOutput {
                selected_candidate_id,
            } => (Some(selected_candidate_id), None),
            CalibrationStageSelectedCandidateSource::PersistedArtifactReference {
                artifact_reference,
            } => (None, Some(artifact_reference)),
        };

    let fitted_payload = ReturnBackfitStageFittedPayload {
        schema_version: CALIBRATION_RETURN_BACKFIT_STAGE_OUTPUT_SCHEMA_VERSION,
        selected_candidate_id: RETURN_BACKFIT_TRANSFORMATION_ID.to_string(),
        effective_return_components_payload,
        diagnostics: ReturnBackfitStageDiagnostics {
            estimation_policy: RETURN_BACKFIT_ESTIMATION_POLICY.to_string(),
            selected_candidate_id: RETURN_BACKFIT_TRANSFORMATION_ID.to_string(),
            scale_handoff: ReturnBackfitStageScaleHandoffDiagnostic {
                selected_nig_delta: Some(variance_handoff_fit.selected_nig_delta),
                post_handoff_nig_delta: Some(variance_handoff_fit.post_handoff_nig_delta),
                selected_continuous_model_variance_scale: Some(
                    variance_handoff_fit.selected_continuous_model_variance_scale,
                ),
                long_variance_clock_scale_multiplier: Some(
                    variance_handoff_fit.long_variance_clock_scale_multiplier,
                ),
                delta_handoff_multiplier: Some(variance_handoff_fit.delta_handoff_multiplier),
                post_handoff_base_variance_scale: Some(
                    variance_handoff_fit.post_handoff_base_variance_scale,
                ),
                post_handoff_dynamic_variance_scale: Some(
                    variance_handoff_fit.post_handoff_dynamic_variance_scale,
                ),
                observed_reference_variance_scale: Some(
                    variance_handoff_fit.observed_reference_variance_scale,
                ),
            },
            objective_blocks: Vec::new(),
            warnings: Vec::new(),
        },
        return_components_input_transport,
        return_components_selected_candidate_id,
        return_components_artifact_reference,
    };

    Ok(fitted_payload)
}

fn refit_return_backfit_market_pressure(
    input: ReturnBackfitMarketPressureRefitInput<'_>,
) -> Result<ReturnBackfitMarketPressureRefitOutput, String> {
    if let Some(runtime_state) = input
        .runtime_state
        .filter(|_| market_pressure_runtime_state_covers_backfit_input(&input))
    {
        emit_return_backfit_subphase_progress(
            input.progress_sender.as_ref(),
            ReturnBackfitStageProgressPhase::MarketPressureRefit,
            4,
            1,
            1,
            Some("apply fitted pressure state"),
        );

        let parameters = market_pressure_parameters_from_runtime_state(runtime_state)
            .map_err(|source| format!("market pressure backfit failed: {source}"))?;

        return Ok(ReturnBackfitMarketPressureRefitOutput {
            parameters,
            runtime_state: Some(Arc::clone(runtime_state)),
        });
    }

    emit_return_backfit_progress(
        input.progress_sender.as_ref(),
        ReturnBackfitStageProgressPhase::MarketPressureRefit,
        4,
        RETURN_BACKFIT_STAGE_PROGRESS_UNITS,
        Some("fit market pressure parameters"),
    );

    let market_pressure_progress_sender = input.progress_sender.clone();
    let mut market_pressure_progress_observer = move |progress: MarketPressureFitProgress| {
        emit_return_backfit_subphase_progress(
            market_pressure_progress_sender.as_ref(),
            ReturnBackfitStageProgressPhase::MarketPressureRefit,
            4,
            progress.completed_units,
            progress.total_units,
            Some(progress.current_item_label),
        );
    };

    let output = fit_market_pressure_from_nig_configuration(MarketPressureNigFitInput {
        observed_open_session_returns: input
            .candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
        coarse_data_records: input.coarse_data_records,
        nig_configuration: input.nig_configuration,
        source_window_open_exposure_hours: input.source_window_open_exposure_hours,
        solver_settings: input.solver_settings,
        progress_observer: Some(&mut market_pressure_progress_observer),
    })
    .map_err(|source| format!("market pressure backfit failed: {source}"))?;

    Ok(ReturnBackfitMarketPressureRefitOutput {
        parameters: output.parameters,
        runtime_state: Some(output.runtime_state),
    })
}

fn market_pressure_runtime_state_covers_backfit_input(
    input: &ReturnBackfitMarketPressureRefitInput<'_>,
) -> bool {
    let Some(runtime_state) = input.runtime_state else {
        return false;
    };

    runtime_state.covers_coarse_return_record_count(
        input.coarse_data_records.map_or(0, |records| records.len()),
    )
}

fn selected_return_components_candidate(
    payload: &ReturnComponentsStageFittedPayload,
) -> Result<&ReturnComponentsStageCandidateDiagnostic, String> {
    payload
        .candidate_diagnostics
        .iter()
        .find(|candidate| candidate.candidate_id == payload.selected_candidate_id)
        .ok_or_else(|| {
            format!(
                "return-backfit requires selected return-components candidate diagnostics for {}",
                payload.selected_candidate_id
            )
        })
}

fn failure_report(
    run_spec: &CalibrationRunSpec,
    failure: ReturnBackfitStageExecutionFailure,
) -> ReturnBackfitStageExecutionReport {
    ReturnBackfitStageExecutionReport {
        schema_version: CALIBRATION_RETURN_BACKFIT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::ReturnBackfit,
        status: CalibrationStageExecutionStatus::Failure,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_RETURN_BACKFIT_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: None,
        metadata: None,
        failure: Some(failure),
    }
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::CalibrationOptimizerSettings;
    use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
    use janiform_model_kernels::return_clock::{
        IntegratedSideActivityDecomposition, SideSourceActivityDecomposition,
    };

    use crate::market_data::loading::MarketDataNormalizedCoarseDataRecord;
    use crate::market_pressure::{
        MarketPressureFitInput, MarketPressureNigFitInput, fit_market_pressure,
        fit_market_pressure_from_nig_configuration,
    };
    use crate::stages::return_components::ReturnComponentsObservedOpenSessionReturn;

    #[test]
    fn side_only_pressure_state_does_not_cover_available_coarse_records() {
        let observed_returns = vec![observed_return(1, 0.0)];
        let coarse_records = vec![coarse_record(0, 1, 1.0e-4)];

        let output = fit_market_pressure(MarketPressureFitInput {
            observed_open_session_returns: observed_returns.as_slice(),
            coarse_data_records: Some(coarse_records.as_slice()),
            coarse_return_law: None,
            source_window_open_exposure_hours: 1.0,
            solver_settings: &test_solver_settings(),
            progress_observer: None,
        })
        .expect("side-only pressure fit should succeed");

        assert!(
            !output
                .runtime_state
                .covers_coarse_return_record_count(coarse_records.len())
        );
    }

    #[test]
    fn coarse_aware_pressure_state_covers_available_coarse_records() {
        let observed_returns = vec![observed_return(1, 0.0)];
        let coarse_records = vec![coarse_record(0, 1, 1.0e-4)];

        let output = fit_market_pressure_from_nig_configuration(MarketPressureNigFitInput {
            observed_open_session_returns: observed_returns.as_slice(),
            coarse_data_records: Some(coarse_records.as_slice()),
            nig_configuration: test_nig_configuration(),
            source_window_open_exposure_hours: 1.0,
            solver_settings: &test_solver_settings(),
            progress_observer: None,
        })
        .expect("coarse-aware pressure fit should succeed");

        assert!(
            output
                .runtime_state
                .covers_coarse_return_record_count(coarse_records.len())
        );
    }

    fn observed_return(
        return_timestamp_utc_nanos: i64,
        log_return: f64,
    ) -> ReturnComponentsObservedOpenSessionReturn {
        ReturnComponentsObservedOpenSessionReturn {
            return_timestamp_utc_nanos,
            elapsed_same_session_hours: 0.001,
            log_return,
            post_shock_activity_lift: 1.0,
            sell_share_proxy: 0.5,
            source_activity: SideSourceActivityDecomposition::new(
                IntegratedSideActivityDecomposition::new(1.0, 0.0),
                IntegratedSideActivityDecomposition::new(1.0, 0.0),
            ),
        }
    }

    fn coarse_record(
        interval_start_utc_nanos: i64,
        interval_end_utc_nanos: i64,
        log_return: f64,
    ) -> MarketDataNormalizedCoarseDataRecord {
        MarketDataNormalizedCoarseDataRecord {
            interval_start_utc_nanos,
            interval_end_utc_nanos,
            observation_timestamp_utc_nanos: interval_end_utc_nanos,
            close_price: 1.0,
            open_price: None,
            high_price: None,
            low_price: None,
            volume: None,
            close_to_close_log_return: Some(log_return),
            open_to_close_log_return: None,
            close_to_open_gap_log_return: None,
            high_low_log_range: None,
            source_row_number: 1,
        }
    }

    fn test_solver_settings() -> CalibrationOptimizerSettings {
        CalibrationOptimizerSettings::try_new("return-backfit-test", 8, 1, 1.0e-10, 1.0e-10, 0.0)
            .expect("test solver settings are valid")
    }

    fn test_nig_configuration() -> NigSharedPlusShiftConfiguration {
        NigSharedPlusShiftConfiguration::try_new(80.0, -12.0, 2.0e-4, 0.0, -8.0)
            .expect("test NIG configuration is valid")
    }
}
