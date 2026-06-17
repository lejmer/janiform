use serde::Serialize;

use chrono::{DateTime, Utc};
use janiform_calibration_artifacts::{
    CALIBRATION_RETURN_BACKFIT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_RETURN_BACKFIT_STAGE_HANDOFF_POLICY,
    CALIBRATION_RETURN_BACKFIT_STAGE_OUTPUT_SCHEMA_VERSION, CalibrationExecutionMode,
    CalibrationOptimizerSettings, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStageSeedSource,
    CalibrationStageSelectedCandidateSource, ReturnBackfitStageDiagnostics,
    ReturnBackfitStageVarianceBudgetDiagnostic, SurpriseNewsDeterministicIntensityProfile,
    stage_solver_settings_for_stage,
};
use janiform_foundations::numeric::{strictly_positive_machine_value, validate_positive_number};
use janiform_foundations::time::duration_nanos_to_hours;
use janiform_model_kernels::hawkes::target_source_rates_from_shared_activity_loading;
use janiform_model_kernels::long_variance::{LongVarianceParameters, LongVarianceState};
use janiform_model_kernels::news::{
    NewsArrivalIntensityEvaluationInput, NewsProcessConfiguration,
    expected_news_activity_loading_rate,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use janiform_model_kernels::no_move_event::NoMoveEventModel;
use janiform_model_kernels::return_clock::ReturnClockExposureParameters;

use crate::stages::hawkes::HawkesStageFittedPayload;
use crate::stages::long_variance::LongVarianceStageFittedPayload;
use crate::stages::news_shocks::NewsShocksStageFittedPayload;
use crate::stages::return_components::{
    ReturnComponentsObservedOpenSessionReturn, ReturnComponentsStageCandidateDiagnostic,
    ReturnComponentsStageCandidateGenerationInput, ReturnComponentsStageFittedPayload,
    moving_return_clock_proxy,
};
use crate::stages::seasonality::CalibrationStageExecutionStatus;
use crate::surprise_profile::average_surprise_news_weekly_profile_multiplier_over_interval;

const RETURN_BACKFIT_ESTIMATION_POLICY: &str =
    "deterministic_long_variance_clock_residual_nig_scale_backfit.v1";
const RETURN_BACKFIT_TRANSFORMATION_ID: &str = "long_variance_clock_residual_nig_scale";

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReturnBackfitStageExecutionInput {
    pub return_components_input: Option<CalibrationStageSelectedCandidateSource>,
    pub hawkes_stage_payload: Option<HawkesStageFittedPayload>,
    pub candidate_generation_input: Option<ReturnComponentsStageCandidateGenerationInput>,
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

pub fn run_return_backfit_calibration_stage(
    run_spec: &CalibrationRunSpec,
    execution_input: ReturnBackfitStageExecutionInput,
) -> ReturnBackfitStageExecutionReport {
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

    let fitted_stage_payload = match build_return_backfit_fitted_stage_payload(
        &hawkes_stage_payload,
        &candidate_generation_input,
        return_components_input,
        return_components_stage_payload,
        long_variance_stage_payload,
        news_shocks_stage_payload,
    ) {
        Ok(payload) => payload,
        Err(error_message) => {
            return failure_report(
                run_spec,
                ReturnBackfitStageExecutionFailure::InvalidBackfitInput { error_message },
            );
        }
    };

    let seed_metadata = janiform_calibration_artifacts::resolve_calibration_stage_seed_metadata(
        run_spec,
        CalibrationStage::ReturnBackfit,
    );

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
    hawkes_stage_payload: &HawkesStageFittedPayload,
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    return_components_input: CalibrationStageSelectedCandidateSource,
    mut effective_return_components_payload: ReturnComponentsStageFittedPayload,
    long_variance_stage_payload: LongVarianceStageFittedPayload,
    news_shocks_stage_payload: NewsShocksStageFittedPayload,
) -> Result<ReturnBackfitStageFittedPayload, String> {
    let selected_candidate =
        selected_return_components_candidate(&effective_return_components_payload)?;

    let backfit_budget = return_backfit_variance_budget(ReturnBackfitVarianceBudgetInput {
        hawkes_stage_payload,
        candidate_generation_input,
        selected_candidate,
        nig_configuration: effective_return_components_payload
            .fitted_parameters
            .nig_shared_plus_shift_configuration,
        return_clock_exposure_parameters: effective_return_components_payload
            .fitted_parameters
            .return_clock_exposure_parameters,
        no_move_event_model: effective_return_components_payload
            .fitted_parameters
            .no_move_event_model,
        long_variance_parameters: long_variance_stage_payload.fitted_parameters,
        news_process_configuration: &news_shocks_stage_payload.news_process_configuration,
        surprise_deterministic_intensity_profile: &news_shocks_stage_payload
            .surprise_deterministic_intensity_profile,
    })?;

    let original_nig_configuration = effective_return_components_payload
        .fitted_parameters
        .nig_shared_plus_shift_configuration;

    let nig_delta_scale =
        backfit_budget.backfitted_nig_contribution / backfit_budget.initial_nig_contribution;

    let backfitted_nig_configuration = NigSharedPlusShiftConfiguration::try_new(
        original_nig_configuration.alpha,
        original_nig_configuration.beta_0,
        original_nig_configuration.delta * nig_delta_scale,
        original_nig_configuration.mu,
        original_nig_configuration.delta_beta,
    )
    .map_err(|source| format!("backfitted NIG configuration is invalid: {source}"))?;

    effective_return_components_payload
        .fitted_parameters
        .nig_shared_plus_shift_configuration = backfitted_nig_configuration;

    effective_return_components_payload
        .fitted_parameters
        .long_variance_parameters = long_variance_stage_payload.fitted_parameters;

    effective_return_components_payload
        .fitted_parameters
        .news_process_configuration = news_shocks_stage_payload.news_process_configuration;

    effective_return_components_payload
        .fitted_parameters
        .surprise_deterministic_intensity_profile =
        news_shocks_stage_payload.surprise_deterministic_intensity_profile;

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

    Ok(ReturnBackfitStageFittedPayload {
        schema_version: CALIBRATION_RETURN_BACKFIT_STAGE_OUTPUT_SCHEMA_VERSION,
        selected_candidate_id: RETURN_BACKFIT_TRANSFORMATION_ID.to_string(),
        effective_return_components_payload,
        diagnostics: ReturnBackfitStageDiagnostics {
            estimation_policy: RETURN_BACKFIT_ESTIMATION_POLICY.to_string(),
            selected_candidate_id: RETURN_BACKFIT_TRANSFORMATION_ID.to_string(),
            variance_budget: ReturnBackfitStageVarianceBudgetDiagnostic {
                initial_nig_contribution: Some(backfit_budget.initial_nig_contribution),
                fixed_long_variance_news_contribution: Some(
                    backfit_budget.fixed_long_variance_news_contribution,
                ),
                backfitted_nig_contribution: Some(backfit_budget.backfitted_nig_contribution),
                final_total_modeled_return_scale: Some(
                    backfit_budget.final_total_modeled_return_scale,
                ),
                observed_target_scale: Some(backfit_budget.observed_target_scale),
            },
            objective_blocks: Vec::new(),
            warnings: Vec::new(),
        },
        return_components_input_transport,
        return_components_selected_candidate_id,
        return_components_artifact_reference,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ReturnBackfitVarianceBudget {
    long_variance_clock_scale_multiplier: f64,
    initial_nig_contribution: f64,
    fixed_long_variance_news_contribution: f64,
    backfitted_nig_contribution: f64,
    final_total_modeled_return_scale: f64,
    observed_target_scale: f64,
}

struct ReturnBackfitVarianceBudgetInput<'a> {
    hawkes_stage_payload: &'a HawkesStageFittedPayload,
    candidate_generation_input: &'a ReturnComponentsStageCandidateGenerationInput,
    selected_candidate: &'a ReturnComponentsStageCandidateDiagnostic,
    nig_configuration: NigSharedPlusShiftConfiguration,
    return_clock_exposure_parameters: ReturnClockExposureParameters,
    no_move_event_model: NoMoveEventModel,
    long_variance_parameters: LongVarianceParameters,
    news_process_configuration: &'a NewsProcessConfiguration,
    surprise_deterministic_intensity_profile: &'a SurpriseNewsDeterministicIntensityProfile,
}

struct LongVarianceClockScaleMultiplierInput<'a> {
    hawkes_stage_payload: &'a HawkesStageFittedPayload,
    candidate_generation_input: &'a ReturnComponentsStageCandidateGenerationInput,
    news_process_configuration: &'a NewsProcessConfiguration,
    surprise_deterministic_intensity_profile: &'a SurpriseNewsDeterministicIntensityProfile,
    return_clock_exposure_parameters: ReturnClockExposureParameters,
    no_move_event_model: NoMoveEventModel,
    long_variance_parameters: LongVarianceParameters,
    aligned_returns: &'a [ReturnComponentsObservedOpenSessionReturn],
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

fn return_backfit_variance_budget(
    input: ReturnBackfitVarianceBudgetInput<'_>,
) -> Result<ReturnBackfitVarianceBudget, String> {
    let ReturnBackfitVarianceBudgetInput {
        hawkes_stage_payload,
        candidate_generation_input,
        selected_candidate,
        nig_configuration,
        return_clock_exposure_parameters,
        no_move_event_model,
        long_variance_parameters,
        news_process_configuration,
        surprise_deterministic_intensity_profile,
    } = input;

    let observed_open_session_returns =
        sorted_observed_open_session_returns_for_backfit(candidate_generation_input)?;

    let long_variance_clock_scale_multiplier =
        long_variance_clock_scale_multiplier(LongVarianceClockScaleMultiplierInput {
            hawkes_stage_payload,
            candidate_generation_input,
            news_process_configuration,
            surprise_deterministic_intensity_profile,
            return_clock_exposure_parameters,
            no_move_event_model,
            long_variance_parameters,
            aligned_returns: observed_open_session_returns.as_slice(),
        })?;

    let observed_target_scale = selected_candidate
        .return_distribution_diagnostic
        .observed_return_volatility
        .powi(2);

    let initial_nig_contribution = selected_candidate
        .return_distribution_diagnostic
        .fitted_return_volatility
        .powi(2);

    validate_nig_configuration_for_backfit(nig_configuration)?;

    return_backfit_variance_budget_from_scale_inputs(
        long_variance_clock_scale_multiplier,
        initial_nig_contribution,
        observed_target_scale,
    )
}

fn return_backfit_variance_budget_from_scale_inputs(
    long_variance_clock_scale_multiplier: f64,
    initial_nig_contribution: f64,
    observed_target_scale: f64,
) -> Result<ReturnBackfitVarianceBudget, String> {
    validate_positive_number(
        long_variance_clock_scale_multiplier,
        "long_variance_clock_scale_multiplier",
    )
    .map_err(|source| {
        format!("return-backfit long-variance clock scale multiplier is invalid: {source}")
    })?;

    validate_positive_number(initial_nig_contribution, "initial_nig_contribution").map_err(
        |source| format!("return-backfit initial NIG contribution is invalid: {source}"),
    )?;

    validate_positive_number(observed_target_scale, "observed_target_scale")
        .map_err(|source| format!("return-backfit observed target scale is invalid: {source}"))?;

    let backfitted_nig_contribution = observed_target_scale / long_variance_clock_scale_multiplier;

    let final_total_modeled_return_scale =
        backfitted_nig_contribution * long_variance_clock_scale_multiplier;

    Ok(ReturnBackfitVarianceBudget {
        long_variance_clock_scale_multiplier,
        initial_nig_contribution,
        fixed_long_variance_news_contribution: observed_target_scale - backfitted_nig_contribution,
        backfitted_nig_contribution,
        final_total_modeled_return_scale,
        observed_target_scale,
    })
}

fn sorted_observed_open_session_returns_for_backfit(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
) -> Result<Vec<ReturnComponentsObservedOpenSessionReturn>, String> {
    let mut sorted_observations = candidate_generation_input
        .observed_open_session_returns
        .clone();

    sorted_observations.sort_by_key(|observation| observation.return_timestamp_utc_nanos);

    if sorted_observations.is_empty() {
        return Err("return-backfit requires observed open-session returns".to_string());
    }

    Ok(sorted_observations)
}

fn long_variance_clock_scale_multiplier(
    input: LongVarianceClockScaleMultiplierInput<'_>,
) -> Result<f64, String> {
    let LongVarianceClockScaleMultiplierInput {
        hawkes_stage_payload,
        candidate_generation_input,
        news_process_configuration,
        surprise_deterministic_intensity_profile,
        return_clock_exposure_parameters,
        no_move_event_model,
        long_variance_parameters,
        aligned_returns,
    } = input;

    let stationary_mean_reversion_level = stationary_mean_reversion_level_for_backfit(
        hawkes_stage_payload,
        candidate_generation_input,
        news_process_configuration,
        surprise_deterministic_intensity_profile,
        long_variance_parameters,
    )?;

    let mut state = LongVarianceState::unit();
    let mut original_return_count_scale = 0.0;
    let mut long_variance_return_count_scale = 0.0;

    for observation in aligned_returns {
        let clock_scale = state
            .baseline_clock_scale(
                long_variance_parameters,
                stationary_mean_reversion_level,
                observation.elapsed_same_session_hours,
            )
            .map_err(|source| {
                format!("return-backfit long-variance clock replay failed: {source}")
            })?;

        original_return_count_scale += moving_return_clock_proxy(
            *observation,
            no_move_event_model,
            return_clock_exposure_parameters,
            1.0,
        );

        long_variance_return_count_scale += moving_return_clock_proxy(
            *observation,
            no_move_event_model,
            return_clock_exposure_parameters,
            clock_scale,
        );

        state = state
            .apply_mean_reversion_drift(
                long_variance_parameters,
                stationary_mean_reversion_level,
                observation.elapsed_same_session_hours,
            )
            .map_err(|source| {
                format!("return-backfit long-variance state replay failed: {source}")
            })?;

        let sell_share = observation.sell_share_proxy.clamp(0.0, 1.0);
        let event_loading = long_variance_parameters.buy_event_loading * (1.0 - sell_share)
            + long_variance_parameters.sell_event_loading * sell_share;

        state = LongVarianceState::try_new(state.level + event_loading).map_err(|source| {
            format!("return-backfit long-variance event jump replay failed: {source}")
        })?;
    }

    if original_return_count_scale <= 0.0 {
        return Err("return-backfit requires positive total return count scale".to_string());
    }

    let scale_multiplier: f64 = long_variance_return_count_scale / original_return_count_scale;

    if !scale_multiplier.is_finite() || scale_multiplier <= 0.0 {
        return Err(format!(
            "return-backfit produced invalid long-variance scale multiplier {scale_multiplier}"
        ));
    }

    Ok(scale_multiplier.max(strictly_positive_machine_value()))
}

fn validate_nig_configuration_for_backfit(
    nig_configuration: NigSharedPlusShiftConfiguration,
) -> Result<(), String> {
    nig_configuration
        .derive_driver_parameters()
        .map(|_| ())
        .map_err(|source| format!("selected NIG configuration is invalid: {source}"))
}

fn stationary_mean_reversion_level_for_backfit(
    hawkes_stage_payload: &HawkesStageFittedPayload,
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    news_process_configuration: &NewsProcessConfiguration,
    surprise_deterministic_intensity_profile: &SurpriseNewsDeterministicIntensityProfile,
    long_variance_parameters: LongVarianceParameters,
) -> Result<f64, String> {
    let source_window = candidate_generation_input.known_news_calendar.source_window;
    let source_window_duration_hours = duration_nanos_to_hours(
        source_window
            .window_end_utc_nanos
            .saturating_sub(source_window.window_start_utc_nanos),
    );

    if !source_window_duration_hours.is_finite() || source_window_duration_hours <= 0.0 {
        return Err("return-backfit requires a positive source-window duration".to_string());
    }

    let surprise_time_multiplier = average_surprise_news_weekly_profile_multiplier_over_interval(
        DateTime::<Utc>::from_timestamp_nanos(source_window.window_start_utc_nanos),
        DateTime::<Utc>::from_timestamp_nanos(source_window.window_end_utc_nanos),
        surprise_deterministic_intensity_profile,
    )
    .map_err(|source| format!("return-backfit surprise intensity profile invalid: {source}"))?;

    let news_intensities = news_process_configuration
        .arrivals
        .evaluate_with_deterministic_time_multiplier(NewsArrivalIntensityEvaluationInput {
            interval_start_utc_nanos: source_window.window_start_utc_nanos,
            interval_end_utc_nanos: source_window.window_end_utc_nanos,
            surprise_deterministic_time_multiplier: surprise_time_multiplier,
            surprise_open_window_share: 1.0,
            surprise_closed_window_share: 0.0,
        })
        .map_err(|source| format!("return-backfit news intensity evaluation failed: {source}"))?;

    let news_activity_loading_rate = expected_news_activity_loading_rate(
        news_intensities.known_component_intensity,
        news_intensities.surprise_component_intensity,
        news_process_configuration.known_component_mark_scales,
        news_process_configuration.surprise_component_mark_scales,
        news_process_configuration
            .shared_mark_law
            .gamma_severity_marks,
    )
    .map_err(|source| {
        format!("return-backfit news activity-loading rate evaluation failed: {source}")
    })?;

    let exogenous_source_rates = target_source_rates_from_shared_activity_loading(
        hawkes_stage_payload.fitted_parameters.kernel_parameters.rho,
        news_activity_loading_rate,
    )
    .ok_or_else(|| "return-backfit news target source-rate evaluation failed".to_string())?;

    long_variance_parameters
        .stationary_mean_reversion_level_for_hawkes_kernel(
            hawkes_stage_payload.fitted_parameters.kernel_parameters,
            exogenous_source_rates,
        )
        .map_err(|source| {
            format!("return-backfit long-variance stationary level evaluation failed: {source}")
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
    use super::return_backfit_variance_budget_from_scale_inputs;

    #[test]
    fn return_backfit_targets_observed_variance_after_long_variance_clock_scaling() {
        let budget = return_backfit_variance_budget_from_scale_inputs(4.0, 0.09, 0.16)
            .expect("positive scale inputs should produce a budget");

        assert_eq!(budget.initial_nig_contribution, 0.09);
        assert_eq!(budget.backfitted_nig_contribution, 0.04);
        assert_eq!(budget.fixed_long_variance_news_contribution, 0.12);
        assert_eq!(budget.final_total_modeled_return_scale, 0.16);
        assert_eq!(budget.observed_target_scale, 0.16);
    }

    #[test]
    fn return_backfit_rejects_non_positive_observed_variance_target() {
        let error = return_backfit_variance_budget_from_scale_inputs(1.0, 0.09, 0.0)
            .expect_err("zero observed variance cannot define a positive NIG scale");

        assert!(error.contains("observed_target_scale"));
    }
}
