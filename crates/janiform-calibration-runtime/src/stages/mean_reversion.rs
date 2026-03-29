use chrono::{DateTime, Utc};
use serde::Serialize;

use janiform_affine_oracle::affine_oracle::ExponentialAffineTransformEquationSpecification;
use janiform_affine_oracle::endogenous_anchor::{
    EndogenousAnchorIntervalFactors, materialize_endogenous_anchor_path,
};
use janiform_calibration_artifacts::{
    CALIBRATION_MEAN_REVERSION_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_MEAN_REVERSION_STAGE_HANDOFF_POLICY,
    CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION, CalibrationExecutionMode,
    CalibrationOptimizerSettings, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStageSeedSource,
    MeanReversionStageCalibrationGrid, MeanReversionStageDiagnostics,
};
use janiform_foundations::time::{ONE_HOUR_NANOS, ceil_to_hour_utc_nanos, floor_to_hour_utc_nanos};
use janiform_market_time::hourly_factors::{
    HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource, HourlySeasonalityFactorSource,
};
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::mean_reversion::{
    MeanReversionAnchorModel, MeanReversionAnchorModelKind, MeanReversionFittedParameters,
};

use crate::adapters::load_hourly_factor_contract_for_calibration;
use crate::calendar::ExchangeCalendarRegistry;
use crate::stages::hawkes::HawkesStageFittedPayload;
use crate::stages::joint_refinement::build_surprise_multipliers_for_interval_grid;
use crate::stages::return_components::ReturnComponentsStageFittedPayload;
use crate::stages::seasonality::{
    CalibrationStageExecutionStatus, SeasonalityStageNormalizedPayload,
    build_runtime_hourly_seasonality_factors_from_components,
};

const MINIMUM_MEAN_REVERSION_OBSERVATION_COUNT: usize = 3;
const MINIMUM_MEAN_REVERSION_COVERAGE_RATIO: f64 = 0.5;
const MINIMUM_PHI: f64 = 1.0e-12;
const ENDOGENOUS_ANCHOR_REASON: &str = "endogenous_anchor_core_expectation_one_interval_first_observation_boundary_anchor_zero_hawkes_state.v1";
const ENDOGENOUS_RUNTIME_PROJECTION_REQUIRED_REASON: &str =
    "endogenous_anchor_runtime_projection_required.v1";
const RUNTIME_SEASONALITY_MATERIALIZATION_METHOD: &str =
    "runtime_compact_seasonality_parameters_mean_renormalized.v1";
const CALIBRATION_GRID_ALIGNMENT_POLICY: &str =
    "calibration_grid_observation_boundary_alignment.v1";

#[derive(Debug, Clone, PartialEq)]
pub enum MeanReversionStageReturnComponentsInput {
    InMemoryOutput { selected_candidate_id: String },
    PersistedArtifactReference { artifact_reference: String },
}

impl MeanReversionStageReturnComponentsInput {
    pub const fn transport(&self) -> CalibrationStageHandoffTransport {
        match self {
            Self::InMemoryOutput { .. } => CalibrationStageHandoffTransport::InMemoryOutput,
            Self::PersistedArtifactReference { .. } => {
                CalibrationStageHandoffTransport::PersistedArtifactReference
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeanReversionStageObservation {
    pub observation_timestamp_utc_nanos: i64,
    pub observed_log_price: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeanReversionStageSeasonalityInput {
    pub exchange_code: String,
    pub intraday_minutes_per_bin: u16,
    pub intraday_factor_by_bin: Vec<f64>,
    pub day_of_week_factor: Vec<f64>,
    pub day_of_year_factor: Vec<f64>,
}

impl MeanReversionStageSeasonalityInput {
    pub fn from_seasonality_stage_payload(payload: &SeasonalityStageNormalizedPayload) -> Self {
        Self {
            exchange_code: payload.exchange_code.clone(),
            intraday_minutes_per_bin: payload.intraday_minutes_per_bin,
            intraday_factor_by_bin: payload.intraday_factor_by_bin.clone(),
            day_of_week_factor: payload.day_of_week_factor.clone(),
            day_of_year_factor: payload.day_of_year_factor.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeanReversionStageExecutionInput {
    pub return_components_input: Option<MeanReversionStageReturnComponentsInput>,
    pub seasonality_stage_input: Option<MeanReversionStageSeasonalityInput>,
    pub hawkes_stage_payload: Option<HawkesStageFittedPayload>,
    pub return_components_stage_payload: Option<ReturnComponentsStageFittedPayload>,
    pub requested_anchor_model_kind: MeanReversionAnchorModelKind,
    pub calibration_grid_kind: String,
    pub calibration_grid_interval_hours: f64,
    pub source_observation_count: usize,
    pub coverage_ratio: f64,
    pub observations: Vec<MeanReversionStageObservation>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MeanReversionStageFittedPayload {
    pub schema_version: &'static str,
    pub fitted_parameters: MeanReversionFittedParameters,
    pub diagnostics: MeanReversionStageDiagnostics,
    pub return_components_input_transport: CalibrationStageHandoffTransport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_components_selected_candidate_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_components_artifact_reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MeanReversionStageExecutionMetadata {
    pub stage_output_schema_version: &'static str,
    pub run_global_seed: u64,
    pub stage_seed: u64,
    pub stage_seed_source: CalibrationStageSeedSource,
    pub stage_seed_derivation_policy: &'static str,
    pub solver_settings: CalibrationOptimizerSettings,
    pub source_observation_count: usize,
    pub retained_observation_count: usize,
    pub coverage_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum MeanReversionStageExecutionFailure {
    InvalidRunSpec { error_message: String },
    StageIsNotSelectedInRunSpec,
    MissingStageSolverSettings,
    MissingReturnComponentsInput,
    MissingSeasonalityStagePayload,
    MissingHawkesStagePayload,
    MissingReturnComponentsStagePayload,
    InvalidObservationTimestampGrid { error_message: String },
    HourlyFactorMaterializationFailed { error_message: String },
    EndogenousAnchorMaterializationFailed { error_message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MeanReversionStageExecutionReport {
    pub schema_version: &'static str,
    pub stage: CalibrationStage,
    pub status: CalibrationStageExecutionStatus,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub handoff_policy: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fitted_stage_payload: Option<MeanReversionStageFittedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<MeanReversionStageExecutionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<MeanReversionStageExecutionFailure>,
}

pub fn run_mean_reversion_calibration_stage(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    execution_input: MeanReversionStageExecutionInput,
) -> MeanReversionStageExecutionReport {
    if let Err(error_message) = run_spec.validate() {
        return failure_report(
            run_spec,
            MeanReversionStageExecutionFailure::InvalidRunSpec {
                error_message: error_message.to_string(),
            },
        );
    }

    if !run_spec
        .selected_stages
        .iter()
        .any(|selected_stage| *selected_stage == CalibrationStage::MeanReversion)
    {
        return failure_report(
            run_spec,
            MeanReversionStageExecutionFailure::StageIsNotSelectedInRunSpec,
        );
    }

    let Some(solver_settings) = janiform_calibration_artifacts::stage_solver_settings_for_stage(
        run_spec,
        CalibrationStage::MeanReversion,
    ) else {
        return failure_report(
            run_spec,
            MeanReversionStageExecutionFailure::MissingStageSolverSettings,
        );
    };

    let seed_metadata = janiform_calibration_artifacts::resolve_calibration_stage_seed_metadata(
        run_spec,
        CalibrationStage::MeanReversion,
    );

    let Some(return_components_input) = execution_input.return_components_input.as_ref() else {
        return failure_report(
            run_spec,
            MeanReversionStageExecutionFailure::MissingReturnComponentsInput,
        );
    };

    let (fitted_parameters, diagnostics) =
        match estimate_mean_reversion_fitted_parameters(calendar_registry, &execution_input) {
            Ok(result) => result,
            Err(failure) => return failure_report(run_spec, failure),
        };

    let (return_components_selected_candidate_id, return_components_artifact_reference) =
        match return_components_input {
            MeanReversionStageReturnComponentsInput::InMemoryOutput {
                selected_candidate_id,
            } => (Some(selected_candidate_id.clone()), None),
            MeanReversionStageReturnComponentsInput::PersistedArtifactReference {
                artifact_reference,
            } => (None, Some(artifact_reference.clone())),
        };

    MeanReversionStageExecutionReport {
        schema_version: CALIBRATION_MEAN_REVERSION_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::MeanReversion,
        status: CalibrationStageExecutionStatus::Success,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_MEAN_REVERSION_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: Some(MeanReversionStageFittedPayload {
            schema_version: CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION,
            fitted_parameters,
            diagnostics: diagnostics.clone(),
            return_components_input_transport: return_components_input.transport(),
            return_components_selected_candidate_id,
            return_components_artifact_reference,
        }),
        metadata: Some(MeanReversionStageExecutionMetadata {
            stage_output_schema_version: CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION,
            run_global_seed: seed_metadata.run_global_seed,
            stage_seed: seed_metadata.stage_seed,
            stage_seed_source: seed_metadata.stage_seed_source,
            stage_seed_derivation_policy:
                janiform_calibration_artifacts::CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
            solver_settings,
            source_observation_count: execution_input.source_observation_count,
            retained_observation_count: execution_input.observations.len(),
            coverage_ratio: execution_input.coverage_ratio,
        }),
        failure: None,
    }
}

fn failure_report(
    run_spec: &CalibrationRunSpec,
    failure: MeanReversionStageExecutionFailure,
) -> MeanReversionStageExecutionReport {
    MeanReversionStageExecutionReport {
        schema_version: CALIBRATION_MEAN_REVERSION_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::MeanReversion,
        status: CalibrationStageExecutionStatus::Failure,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_MEAN_REVERSION_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: None,
        metadata: None,
        failure: Some(failure),
    }
}

fn estimate_mean_reversion_fitted_parameters(
    calendar_registry: &ExchangeCalendarRegistry,
    execution_input: &MeanReversionStageExecutionInput,
) -> Result<
    (MeanReversionFittedParameters, MeanReversionStageDiagnostics),
    MeanReversionStageExecutionFailure,
> {
    let calibration_grid = MeanReversionStageCalibrationGrid {
        calibration_grid_kind: execution_input.calibration_grid_kind.clone(),
        calibration_grid_interval_hours: execution_input.calibration_grid_interval_hours,
        source_observation_count: execution_input.source_observation_count,
        retained_observation_count: execution_input.observations.len(),
        coverage_ratio: execution_input.coverage_ratio,
    };

    if execution_input.observations.len() < MINIMUM_MEAN_REVERSION_OBSERVATION_COUNT
        || !execution_input.coverage_ratio.is_finite()
        || execution_input.coverage_ratio < MINIMUM_MEAN_REVERSION_COVERAGE_RATIO
    {
        return Ok(zero_kappa_endogenous_fit(
            calibration_grid,
            ENDOGENOUS_RUNTIME_PROJECTION_REQUIRED_REASON,
        ));
    }

    let observation_levels: Vec<f64> = execution_input
        .observations
        .iter()
        .map(|observation| observation.observed_log_price)
        .collect();

    estimate_endogenous_mean_reversion_fitted_parameters(
        calendar_registry,
        execution_input,
        calibration_grid,
        observation_levels.as_slice(),
    )
}

fn estimate_endogenous_mean_reversion_fitted_parameters(
    calendar_registry: &ExchangeCalendarRegistry,
    execution_input: &MeanReversionStageExecutionInput,
    calibration_grid: MeanReversionStageCalibrationGrid,
    observation_levels: &[f64],
) -> Result<
    (MeanReversionFittedParameters, MeanReversionStageDiagnostics),
    MeanReversionStageExecutionFailure,
> {
    let Some(seasonality_input) = execution_input.seasonality_stage_input.as_ref() else {
        return Err(MeanReversionStageExecutionFailure::MissingSeasonalityStagePayload);
    };

    let Some(hawkes_payload) = execution_input.hawkes_stage_payload.as_ref() else {
        return Err(MeanReversionStageExecutionFailure::MissingHawkesStagePayload);
    };

    let Some(return_components_payload) = execution_input.return_components_stage_payload.as_ref()
    else {
        return Err(MeanReversionStageExecutionFailure::MissingReturnComponentsStagePayload);
    };

    let interval_duration_nanos =
        derive_observation_interval_duration_nanos(execution_input.observations.as_slice())?;

    let range_start_utc = DateTime::<Utc>::from_timestamp_nanos(
        execution_input
            .observations
            .first()
            .expect("observation count is checked before endogenous estimation")
            .observation_timestamp_utc_nanos,
    );
    let range_end_utc = DateTime::<Utc>::from_timestamp_nanos(
        execution_input
            .observations
            .last()
            .expect("observation count is checked before endogenous estimation")
            .observation_timestamp_utc_nanos,
    );

    let runtime_hourly_seasonality_range_start_utc_nanos = floor_to_hour_utc_nanos(
        execution_input
            .observations
            .first()
            .expect("observation count is checked before endogenous estimation")
            .observation_timestamp_utc_nanos,
    );
    let runtime_hourly_seasonality_range_end_utc_nanos = ceil_to_hour_utc_nanos(
        execution_input
            .observations
            .last()
            .expect("observation count is checked before endogenous estimation")
            .observation_timestamp_utc_nanos,
    );

    let runtime_hourly_seasonality_factors =
        build_runtime_hourly_seasonality_factors_from_components(
            calendar_registry,
            seasonality_input.exchange_code.as_str(),
            seasonality_input.intraday_minutes_per_bin,
            seasonality_input.intraday_factor_by_bin.as_slice(),
            seasonality_input.day_of_week_factor.as_slice(),
            seasonality_input.day_of_year_factor.as_slice(),
            runtime_hourly_seasonality_range_start_utc_nanos,
            runtime_hourly_seasonality_range_end_utc_nanos,
            ONE_HOUR_NANOS,
        )
        .map_err(|error_message| {
            MeanReversionStageExecutionFailure::HourlyFactorMaterializationFailed { error_message }
        })?;

    let interval_count = execution_input.observations.len().saturating_sub(1);

    let surprise_deterministic_time_multiplier_by_interval_index =
        build_surprise_multipliers_for_interval_grid(
            range_start_utc,
            interval_count,
            interval_duration_nanos,
            return_components_payload
                .fitted_parameters
                .surprise_deterministic_intensity_profile
                .weekly_hour_utc_multipliers
                .as_slice(),
        );

    let hourly_factor_contract = load_hourly_factor_contract_for_calibration(
        calendar_registry,
        HourlyFactorProviderRequest {
            exchange_code: seasonality_input.exchange_code.as_str(),
            range_start_utc,
            range_end_utc,
            interval_duration_nanos,
            seasonality_source: Some(HourlySeasonalityFactorSource::Explicit {
                hourly_factors: runtime_hourly_seasonality_factors.as_slice(),
                materialization_method: RUNTIME_SEASONALITY_MATERIALIZATION_METHOD,
            }),
            news_intensity_source: Some(HourlyNewsIntensityFactorSource::Materialized {
                known_scheduled_event_timestamps_utc_nanos: return_components_payload
                    .fitted_parameters
                    .news_process_configuration
                    .arrivals
                    .known
                    .scheduled_event_timestamps_utc_nanos
                    .as_slice(),
                surprise_baseline_intensity: return_components_payload
                    .fitted_parameters
                    .news_process_configuration
                    .arrivals
                    .surprise
                    .baseline_intensity,
                surprise_deterministic_time_multiplier_by_interval_index:
                    surprise_deterministic_time_multiplier_by_interval_index.as_slice(),
            }),
        },
    )
    .map_err(|source| {
        MeanReversionStageExecutionFailure::HourlyFactorMaterializationFailed {
            error_message: source.to_string(),
        }
    })?;

    if hourly_factor_contract.diagnostics.interval_count != interval_count {
        return Err(
            MeanReversionStageExecutionFailure::HourlyFactorMaterializationFailed {
                error_message: format!(
                    "mean-reversion endogenous calibration expected {interval_count} interval factors from the observation grid, found {}",
                    hourly_factor_contract.diagnostics.interval_count
                ),
            },
        );
    }

    let interval_delta = interval_duration_nanos as f64 / ONE_HOUR_NANOS as f64;

    let interval_factors: Result<Vec<EndogenousAnchorIntervalFactors>, _> = hourly_factor_contract
        .s_h
        .iter()
        .zip(hourly_factor_contract.known_component_nu_h.iter())
        .zip(hourly_factor_contract.surprise_component_nu_h.iter())
        .map(
            |((seasonality_factor, known_intensity), surprise_intensity)| {
                EndogenousAnchorIntervalFactors::try_new(
                    *seasonality_factor,
                    *known_intensity,
                    *surprise_intensity,
                    interval_delta,
                )
            },
        )
        .collect();

    let anchor_materialization = materialize_endogenous_anchor_path(
        CALIBRATION_GRID_ALIGNMENT_POLICY,
        observation_levels[0],
        ExponentialAffineTransformEquationSpecification::new(
            hawkes_payload.fitted_parameters.kernel_parameters,
            return_components_payload
                .fitted_parameters
                .news_process_configuration
                .shared_mark_law
                .gamma_severity_marks,
        ),
        return_components_payload
            .fitted_parameters
            .nig_shared_plus_shift_configuration,
        &return_components_payload
            .fitted_parameters
            .news_process_configuration,
        HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: 0.0,
            y_minus_minus: 0.0,
        },
        interval_factors
            .map_err(|source| {
                MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
                    error_message: source.to_string(),
                }
            })?
            .as_slice(),
    )
    .map_err(|source| {
        MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
            error_message: source.to_string(),
        }
    })?;

    let mut boundary_anchor_values = Vec::with_capacity(
        anchor_materialization
            .runtime_interval_anchors
            .end_anchor_values
            .len()
            + 1,
    );

    boundary_anchor_values.push(
        anchor_materialization
            .runtime_interval_anchors
            .start_anchor_value,
    );
    boundary_anchor_values.extend(
        anchor_materialization
            .runtime_interval_anchors
            .end_anchor_values
            .iter()
            .copied(),
    );

    let (phi, kappa) = estimate_phi_and_kappa(
        observation_levels,
        boundary_anchor_values.as_slice(),
        execution_input.calibration_grid_interval_hours,
    );

    let anchor_model = MeanReversionAnchorModel::endogenous_core_expectation_one_interval();

    let fitted_parameters = MeanReversionFittedParameters::try_new(kappa, anchor_model.clone())
        .expect("endogenous mean reversion fit should validate");

    Ok((
        fitted_parameters,
        MeanReversionStageDiagnostics {
            chosen_anchor_family:
                MeanReversionAnchorModelKind::EndogenousCoreExpectationOneInterval,
            calibration_grid,
            anchor_parameter_estimates: anchor_model,
            phi,
            kappa,
            deviation_sample_count: observation_levels.len(),
            regression_pair_count: observation_levels.len().saturating_sub(1),
            stage_decision_reason: ENDOGENOUS_ANCHOR_REASON.to_string(),
        },
    ))
}

fn zero_kappa_endogenous_fit(
    calibration_grid: MeanReversionStageCalibrationGrid,
    stage_decision_reason: &str,
) -> (MeanReversionFittedParameters, MeanReversionStageDiagnostics) {
    let anchor_model = MeanReversionAnchorModel::endogenous_core_expectation_one_interval();
    let fitted_parameters = MeanReversionFittedParameters::try_new(0.0, anchor_model.clone())
        .expect("zero-kappa endogenous mean reversion fit should validate");

    (
        fitted_parameters,
        MeanReversionStageDiagnostics {
            chosen_anchor_family:
                MeanReversionAnchorModelKind::EndogenousCoreExpectationOneInterval,
            calibration_grid,
            anchor_parameter_estimates: anchor_model,
            phi: 1.0,
            kappa: 0.0,
            deviation_sample_count: 0,
            regression_pair_count: 0,
            stage_decision_reason: stage_decision_reason.to_string(),
        },
    )
}

fn derive_observation_interval_duration_nanos(
    observations: &[MeanReversionStageObservation],
) -> Result<i64, MeanReversionStageExecutionFailure> {
    let mut interval_duration_nanos: Option<i64> = None;

    for observation_window in observations.windows(2) {
        let current_timestamp = observation_window[0].observation_timestamp_utc_nanos;
        let next_timestamp = observation_window[1].observation_timestamp_utc_nanos;

        let observed_delta_nanos = next_timestamp.saturating_sub(current_timestamp);
        if observed_delta_nanos <= 0 {
            return Err(
                MeanReversionStageExecutionFailure::InvalidObservationTimestampGrid {
                    error_message: format!(
                        "mean-reversion observations must be strictly increasing, found current_timestamp_utc_nanos={current_timestamp} and next_timestamp_utc_nanos={next_timestamp}"
                    ),
                },
            );
        }

        match interval_duration_nanos {
            Some(expected_delta_nanos) if observed_delta_nanos != expected_delta_nanos => {
                return Err(
                    MeanReversionStageExecutionFailure::InvalidObservationTimestampGrid {
                        error_message: format!(
                            "mean-reversion observations must lie on one regular interval grid, found expected_delta_nanos={expected_delta_nanos} and observed_delta_nanos={observed_delta_nanos}"
                        ),
                    },
                );
            }
            Some(_) => {}
            None => interval_duration_nanos = Some(observed_delta_nanos),
        }
    }

    interval_duration_nanos.ok_or(
        MeanReversionStageExecutionFailure::InvalidObservationTimestampGrid {
            error_message:
                "mean-reversion endogenous calibration requires at least two observations"
                    .to_string(),
        },
    )
}

fn estimate_phi_and_kappa(
    observation_levels: &[f64],
    boundary_anchor_values: &[f64],
    calibration_grid_interval_hours: f64,
) -> (f64, f64) {
    let deviations: Vec<f64> = observation_levels
        .iter()
        .zip(boundary_anchor_values.iter())
        .map(|(observed_log_price, anchor_value)| observed_log_price - anchor_value)
        .collect();

    let denominator = deviations
        .iter()
        .take(deviations.len().saturating_sub(1))
        .map(|deviation| deviation * deviation)
        .sum::<f64>();
    if denominator <= MINIMUM_PHI {
        return (1.0, 0.0);
    }

    let numerator = deviations
        .iter()
        .zip(deviations.iter().skip(1))
        .map(|(current_deviation, next_deviation)| current_deviation * next_deviation)
        .sum::<f64>();

    let phi = (numerator / denominator).clamp(0.0, 1.0).max(MINIMUM_PHI);
    let kappa = if calibration_grid_interval_hours > 0.0 {
        -phi.ln() / calibration_grid_interval_hours
    } else {
        0.0
    };

    (phi, kappa.max(0.0))
}
