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
    MeanReversionStageCalibrationGrid, MeanReversionStageCandidateDiagnostics,
    MeanReversionStageDiagnostics,
};
use janiform_foundations::matrix2::solve_two_by_two_linear_system;
use janiform_foundations::numeric::coverage_ratio;
use janiform_foundations::time::{ONE_HOUR_NANOS, ceil_to_hour_utc_nanos, floor_to_hour_utc_nanos};
use janiform_market_time::hourly_factors::{
    HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource, HourlySeasonalityFactorSource,
};
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::mean_reversion::{
    MeanReversionAnchorModel, MeanReversionFittedParameters,
    materialize_mean_reversion_runtime_interval_anchors_from_core_expected_increments,
};

use crate::adapters::load_hourly_factor_contract_for_calibration;
use crate::stages::hawkes::HawkesStageFittedPayload;
use crate::stages::joint_refinement::build_surprise_multipliers_for_interval_grid;
use crate::stages::return_components::ReturnComponentsStageFittedPayload;
use crate::stages::seasonality::{
    CalibrationStageExecutionStatus, SeasonalityStageNormalizedPayload,
    build_runtime_hourly_seasonality_factors_from_components,
};
use janiform_market_time::calendar::ExchangeCalendarRegistry;

const FULL_PRESSURE_TO_LEVEL_REGRESSION_COLUMN_COUNT: usize = 2;
const PRESSURE_GAIN_REGRESSION_COLUMN_COUNT: usize = 1;
const FLAT_ANCHOR_MEAN_REVERSION_IDENTIFIABLE_PARAMETER_COUNT: usize = 2;
const FULL_PRESSURE_TO_LEVEL_CANDIDATE_FAMILY: &str =
    "endogenous_core_pressure_level_mean_reversion";
const PRESSURE_TO_LEVEL_NO_MEAN_REVERSION_CANDIDATE_FAMILY: &str =
    "endogenous_core_pressure_level_no_mean_reversion";
const FLAT_ANCHOR_MEAN_REVERSION_CANDIDATE_FAMILY: &str = "flat_anchor_mean_reversion";
const RANDOM_WALK_NULL_CANDIDATE_FAMILY: &str = "random_walk_null";
const FULL_PRESSURE_TO_LEVEL_REASON: &str =
    "endogenous_core_pressure_level_profiled_over_pressure_persistence_and_phi_reference.v1";
const PRESSURE_TO_LEVEL_NO_MEAN_REVERSION_REASON: &str =
    "endogenous_core_pressure_level_profiled_over_pressure_persistence_phi_reference_fixed_one.v1";
const FLAT_ANCHOR_MEAN_REVERSION_REASON: &str =
    "flat_anchor_mean_reversion_profiled_over_phi_reference_zero_pressure_gain.v1";
const RANDOM_WALK_NULL_REASON: &str = "random_walk_null_baseline.v1";
const ENDOGENOUS_RUNTIME_PROJECTION_REQUIRED_REASON: &str =
    "endogenous_anchor_runtime_projection_required.v1";
const QUASI_LIKELIHOOD_MODEL_SELECTION_SAMPLE_SUPPORT_REQUIRED_REASON: &str =
    "gaussian_quasi_likelihood_bic_model_selection_sample_support_required.v1";
const ENDOGENOUS_MODEL_SELECTION_POLICY: &str =
    "gaussian_quasi_likelihood_bic_with_endogenous_core_pressure_level_candidate_family_profile.v1";
const RUNTIME_SEASONALITY_MATERIALIZATION_METHOD: &str =
    "runtime_compact_seasonality_parameters_mean_renormalized.v1";
const CALIBRATION_GRID_ALIGNMENT_POLICY: &str =
    "calibration_grid_observation_boundary_alignment.v1";

pub(crate) fn mean_reversion_full_model_required_source_observation_count() -> usize {
    // The richest endogenous candidate fits a two-column regression, so the stage needs three
    // directly covered boundary observations to identify that structure over the prepared horizon.
    FULL_PRESSURE_TO_LEVEL_REGRESSION_COLUMN_COUNT.saturating_add(1)
}

fn mean_reversion_pressure_gain_required_source_observation_count() -> usize {
    PRESSURE_GAIN_REGRESSION_COLUMN_COUNT.saturating_add(1)
}

fn mean_reversion_flat_anchor_required_source_observation_count() -> usize {
    // The flat-anchor mean-reversion candidate estimates both an anchor level and a decay. A
    // single transition can fit that pair exactly, so two transitions are the first identifiable
    // case for this candidate family.
    FLAT_ANCHOR_MEAN_REVERSION_IDENTIFIABLE_PARAMETER_COUNT.saturating_add(1)
}

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

    let (fitted_parameters, diagnostics) = match estimate_mean_reversion_fitted_parameters(
        calendar_registry,
        &execution_input,
        &solver_settings,
    ) {
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
    solver_settings: &CalibrationOptimizerSettings,
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

    let candidate_family_support = mean_reversion_candidate_family_support(execution_input);
    if !candidate_family_support.random_walk_null {
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
        solver_settings,
        calibration_grid,
        observation_levels.as_slice(),
        candidate_family_support,
    )
}

#[derive(Debug, Clone, Copy)]
struct MeanReversionCandidateFamilySupport {
    full_pressure_to_level: bool,
    pressure_to_level_without_mean_reversion: bool,
    flat_anchor_mean_reversion: bool,
    random_walk_null: bool,
}

impl MeanReversionCandidateFamilySupport {
    fn requires_endogenous_context(self) -> bool {
        self.full_pressure_to_level || self.pressure_to_level_without_mean_reversion
    }
}

fn mean_reversion_candidate_family_support(
    execution_input: &MeanReversionStageExecutionInput,
) -> MeanReversionCandidateFamilySupport {
    MeanReversionCandidateFamilySupport {
        full_pressure_to_level: mean_reversion_sample_supports_required_source_observations(
            execution_input,
            mean_reversion_full_model_required_source_observation_count(),
        ),
        pressure_to_level_without_mean_reversion:
            mean_reversion_sample_supports_required_source_observations(
                execution_input,
                mean_reversion_pressure_gain_required_source_observation_count(),
            ),
        flat_anchor_mean_reversion: mean_reversion_sample_supports_required_source_observations(
            execution_input,
            mean_reversion_flat_anchor_required_source_observation_count(),
        ),
        random_walk_null: mean_reversion_sample_supports_required_source_observations(
            execution_input,
            mean_reversion_pressure_gain_required_source_observation_count(),
        ),
    }
}

fn mean_reversion_sample_supports_required_source_observations(
    execution_input: &MeanReversionStageExecutionInput,
    required_source_observation_count: usize,
) -> bool {
    if !execution_input.coverage_ratio.is_finite() {
        return false;
    }

    let retained_observation_count = execution_input.observations.len();
    let required_minimum_coverage_ratio = coverage_ratio(
        required_source_observation_count,
        retained_observation_count,
    );

    retained_observation_count >= required_source_observation_count
        && execution_input.source_observation_count >= required_source_observation_count
        && execution_input.coverage_ratio >= required_minimum_coverage_ratio
}

#[derive(Debug, Clone)]
struct EndogenousMeanReversionCalibrationContext {
    core_expected_increments: Vec<f64>,
    interval_deltas: Vec<f64>,
}

#[derive(Debug, Clone)]
struct MeanReversionLeastSquaresSolution {
    first_coefficient: f64,
    second_coefficient: f64,
    rank: usize,
}

#[derive(Debug, Clone)]
struct MeanReversionStageCandidateEvaluation {
    candidate_family: &'static str,
    anchor_model: MeanReversionAnchorModel,
    anchor_origin_level: Option<f64>,
    phi_reference: f64,
    kappa: f64,
    residual_sum_of_squares: f64,
    quasi_log_likelihood: f64,
    information_criterion_bic: f64,
    parameter_count: usize,
    least_squares_rank: usize,
    pressure_state_path: Vec<f64>,
    boundary_anchor_values: Vec<f64>,
    deviations: Vec<f64>,
    residuals: Vec<f64>,
    stage_decision_reason: String,
}

impl MeanReversionStageCandidateEvaluation {
    fn into_diagnostics(
        self,
        core_expected_increments: &[f64],
    ) -> MeanReversionStageCandidateDiagnostics {
        MeanReversionStageCandidateDiagnostics {
            candidate_family: self.candidate_family.to_string(),
            anchor_model: self.anchor_model,
            anchor_origin_level: self.anchor_origin_level,
            phi_reference: self.phi_reference,
            kappa: self.kappa,
            residual_sum_of_squares: self.residual_sum_of_squares,
            quasi_log_likelihood: finite_metric_or_none(self.quasi_log_likelihood),
            information_criterion_bic: finite_metric_or_none(self.information_criterion_bic),
            parameter_count: self.parameter_count,
            least_squares_rank: self.least_squares_rank,
            deviation_sample_count: self.deviations.len(),
            regression_pair_count: self.residuals.len(),
            core_expected_increments: core_expected_increments.to_vec(),
            pressure_state_path: self.pressure_state_path,
            boundary_anchor_values: self.boundary_anchor_values,
            deviations: self.deviations,
            residuals: self.residuals,
            stage_decision_reason: self.stage_decision_reason,
        }
    }
}

fn estimate_endogenous_mean_reversion_fitted_parameters(
    calendar_registry: &ExchangeCalendarRegistry,
    execution_input: &MeanReversionStageExecutionInput,
    solver_settings: &CalibrationOptimizerSettings,
    calibration_grid: MeanReversionStageCalibrationGrid,
    observation_levels: &[f64],
    candidate_family_support: MeanReversionCandidateFamilySupport,
) -> Result<
    (MeanReversionFittedParameters, MeanReversionStageDiagnostics),
    MeanReversionStageExecutionFailure,
> {
    let calibration_context = if candidate_family_support.requires_endogenous_context() {
        Some(build_endogenous_mean_reversion_calibration_context(
            calendar_registry,
            execution_input,
            observation_levels,
        )?)
    } else {
        None
    };

    let mut evaluated_candidates = Vec::new();

    if let Some(calibration_context) = calibration_context.as_ref() {
        if candidate_family_support.full_pressure_to_level {
            evaluated_candidates.push(profile_full_pressure_to_level_candidate(
                observation_levels,
                calibration_grid.calibration_grid_interval_hours,
                calibration_context.core_expected_increments.as_slice(),
                calibration_context.interval_deltas.as_slice(),
                solver_settings,
            )?);
        }

        if candidate_family_support.pressure_to_level_without_mean_reversion {
            evaluated_candidates.push(profile_pressure_to_level_without_mean_reversion_candidate(
                observation_levels,
                calibration_grid.calibration_grid_interval_hours,
                calibration_context.core_expected_increments.as_slice(),
                calibration_context.interval_deltas.as_slice(),
                solver_settings,
            )?);
        }
    }

    if candidate_family_support.flat_anchor_mean_reversion {
        evaluated_candidates.push(profile_flat_anchor_mean_reversion_candidate(
            observation_levels,
            calibration_grid.calibration_grid_interval_hours,
            solver_settings,
        )?);
    }

    evaluated_candidates.push(evaluate_random_walk_null_candidate(
        observation_levels,
        calibration_grid.calibration_grid_interval_hours,
    ));

    let Some(selected_candidate) =
        select_best_admissible_candidate_by_information_criterion(evaluated_candidates.as_slice())
    else {
        return Ok(zero_kappa_endogenous_fit_with_evaluated_candidates(
            calibration_grid,
            QUASI_LIKELIHOOD_MODEL_SELECTION_SAMPLE_SUPPORT_REQUIRED_REASON,
            evaluated_candidates,
            calibration_context
                .as_ref()
                .map(|context| context.core_expected_increments.as_slice())
                .unwrap_or(&[]),
        ));
    };

    let fitted_parameters = MeanReversionFittedParameters::try_new(
        selected_candidate.kappa,
        selected_candidate.anchor_model.clone(),
    )
    .map_err(
        |source| MeanReversionStageExecutionFailure::InvalidRunSpec {
            error_message: format!("selected mean-reversion candidate is invalid: {source}"),
        },
    )?;

    Ok((
        fitted_parameters,
        MeanReversionStageDiagnostics {
            selected_candidate_family: selected_candidate.candidate_family.to_string(),
            calibration_grid,
            anchor_parameter_estimates: selected_candidate.anchor_model.clone(),
            anchor_origin_level: selected_candidate.anchor_origin_level,
            phi_reference: selected_candidate.phi_reference,
            kappa: selected_candidate.kappa,
            residual_sum_of_squares: selected_candidate.residual_sum_of_squares,
            quasi_log_likelihood: finite_metric_or_none(selected_candidate.quasi_log_likelihood),
            information_criterion_bic: finite_metric_or_none(
                selected_candidate.information_criterion_bic,
            ),
            model_selection_policy: ENDOGENOUS_MODEL_SELECTION_POLICY.to_string(),
            deviation_sample_count: selected_candidate.deviations.len(),
            regression_pair_count: selected_candidate.residuals.len(),
            least_squares_rank: selected_candidate.least_squares_rank,
            evaluated_candidates: evaluated_candidates
                .into_iter()
                .map(|candidate| {
                    let core_expected_increments = calibration_context
                        .as_ref()
                        .map(|context| context.core_expected_increments.as_slice())
                        .unwrap_or(&[]);

                    candidate.into_diagnostics(core_expected_increments)
                })
                .collect(),
            stage_decision_reason: format!(
                "{}|{}",
                selected_candidate.stage_decision_reason, ENDOGENOUS_MODEL_SELECTION_POLICY
            ),
        },
    ))
}

fn build_endogenous_mean_reversion_calibration_context(
    calendar_registry: &ExchangeCalendarRegistry,
    execution_input: &MeanReversionStageExecutionInput,
    observation_levels: &[f64],
) -> Result<EndogenousMeanReversionCalibrationContext, MeanReversionStageExecutionFailure> {
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

    let Some(first_observation) = execution_input.observations.first() else {
        return Err(
            MeanReversionStageExecutionFailure::InvalidObservationTimestampGrid {
                error_message:
                    "mean-reversion endogenous calibration requires at least two observations"
                        .to_string(),
            },
        );
    };

    let Some(last_observation) = execution_input.observations.last() else {
        return Err(
            MeanReversionStageExecutionFailure::InvalidObservationTimestampGrid {
                error_message:
                    "mean-reversion endogenous calibration requires at least two observations"
                        .to_string(),
            },
        );
    };

    let range_start_utc =
        DateTime::<Utc>::from_timestamp_nanos(first_observation.observation_timestamp_utc_nanos);
    let range_end_utc =
        DateTime::<Utc>::from_timestamp_nanos(last_observation.observation_timestamp_utc_nanos);

    let runtime_hourly_seasonality_range_start_utc_nanos =
        floor_to_hour_utc_nanos(first_observation.observation_timestamp_utc_nanos);
    let runtime_hourly_seasonality_range_end_utc_nanos =
        ceil_to_hour_utc_nanos(last_observation.observation_timestamp_utc_nanos);

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

    Ok(EndogenousMeanReversionCalibrationContext {
        core_expected_increments: anchor_materialization
            .interval_expectations
            .iter()
            .map(|expectation| expectation.core_expected_increment)
            .collect(),
        interval_deltas: anchor_materialization
            .runtime_interval_anchors
            .interval_deltas,
    })
}

fn profile_full_pressure_to_level_candidate(
    observation_levels: &[f64],
    calibration_grid_interval_hours: f64,
    core_expected_increments: &[f64],
    interval_deltas: &[f64],
    solver_settings: &CalibrationOptimizerSettings,
) -> Result<MeanReversionStageCandidateEvaluation, MeanReversionStageExecutionFailure> {
    profile_candidate_over_pressure_persistence(solver_settings, |pressure_persistence| {
        profile_full_pressure_to_level_candidate_at_pressure_persistence(
            observation_levels,
            calibration_grid_interval_hours,
            core_expected_increments,
            interval_deltas,
            solver_settings,
            pressure_persistence,
        )
    })
}

fn profile_full_pressure_to_level_candidate_at_pressure_persistence(
    observation_levels: &[f64],
    calibration_grid_interval_hours: f64,
    core_expected_increments: &[f64],
    interval_deltas: &[f64],
    solver_settings: &CalibrationOptimizerSettings,
    pressure_persistence_per_reference_interval: f64,
) -> Result<MeanReversionStageCandidateEvaluation, MeanReversionStageExecutionFailure> {
    let phi_boundary_candidate = evaluate_full_pressure_to_level_candidate(
        observation_levels,
        calibration_grid_interval_hours,
        core_expected_increments,
        interval_deltas,
        pressure_persistence_per_reference_interval,
        1.0,
    )?;

    profile_candidate_over_unit_interval(solver_settings, phi_boundary_candidate, |phi_reference| {
        evaluate_full_pressure_to_level_candidate(
            observation_levels,
            calibration_grid_interval_hours,
            core_expected_increments,
            interval_deltas,
            pressure_persistence_per_reference_interval,
            phi_reference,
        )
    })
}

fn evaluate_full_pressure_to_level_candidate(
    observation_levels: &[f64],
    calibration_grid_interval_hours: f64,
    core_expected_increments: &[f64],
    interval_deltas: &[f64],
    pressure_persistence_per_reference_interval: f64,
    phi_reference: f64,
) -> Result<MeanReversionStageCandidateEvaluation, MeanReversionStageExecutionFailure> {
    let pressure_state_path = materialize_pressure_state_path(
        calibration_grid_interval_hours,
        pressure_persistence_per_reference_interval,
        core_expected_increments,
        interval_deltas,
    )?;

    let transition_terms = build_transition_terms(
        observation_levels,
        pressure_state_path.as_slice(),
        interval_deltas,
        calibration_grid_interval_hours,
        phi_reference,
    );

    let least_squares_solution = solve_two_column_least_squares(
        transition_terms.intercept_column.as_slice(),
        transition_terms.pressure_column.as_slice(),
        transition_terms.response.as_slice(),
    );

    let anchor_origin_level = if phi_reference == 1.0 {
        None
    } else {
        Some(least_squares_solution.first_coefficient)
    };

    assemble_candidate_evaluation(
        observation_levels,
        core_expected_increments,
        interval_deltas,
        calibration_grid_interval_hours,
        FULL_PRESSURE_TO_LEVEL_CANDIDATE_FAMILY,
        4,
        MeanReversionAnchorModel::endogenous_core_pressure_level(
            calibration_grid_interval_hours,
            pressure_persistence_per_reference_interval,
            least_squares_solution.second_coefficient,
        ),
        anchor_origin_level,
        phi_reference,
        least_squares_solution.rank,
        transition_terms.response.as_slice(),
        transition_terms.intercept_column.as_slice(),
        Some(transition_terms.pressure_column.as_slice()),
        pressure_state_path,
        FULL_PRESSURE_TO_LEVEL_REASON,
    )
}

fn profile_pressure_to_level_without_mean_reversion_candidate(
    observation_levels: &[f64],
    calibration_grid_interval_hours: f64,
    core_expected_increments: &[f64],
    interval_deltas: &[f64],
    solver_settings: &CalibrationOptimizerSettings,
) -> Result<MeanReversionStageCandidateEvaluation, MeanReversionStageExecutionFailure> {
    profile_candidate_over_pressure_persistence(solver_settings, |pressure_persistence| {
        evaluate_pressure_to_level_without_mean_reversion_candidate(
            observation_levels,
            calibration_grid_interval_hours,
            core_expected_increments,
            interval_deltas,
            pressure_persistence,
        )
    })
}

fn evaluate_pressure_to_level_without_mean_reversion_candidate(
    observation_levels: &[f64],
    calibration_grid_interval_hours: f64,
    core_expected_increments: &[f64],
    interval_deltas: &[f64],
    pressure_persistence_per_reference_interval: f64,
) -> Result<MeanReversionStageCandidateEvaluation, MeanReversionStageExecutionFailure> {
    let pressure_state_path = materialize_pressure_state_path(
        calibration_grid_interval_hours,
        pressure_persistence_per_reference_interval,
        core_expected_increments,
        interval_deltas,
    )?;

    let response = observation_levels
        .iter()
        .zip(observation_levels.iter().skip(1))
        .map(|(current_observation, next_observation)| next_observation - current_observation)
        .collect::<Vec<_>>();

    let pressure_column = pressure_state_path
        .iter()
        .zip(pressure_state_path.iter().skip(1))
        .map(|(current_pressure_state, next_pressure_state)| {
            next_pressure_state - current_pressure_state
        })
        .collect::<Vec<_>>();

    let least_squares_solution =
        solve_one_column_least_squares(pressure_column.as_slice(), response.as_slice());

    assemble_candidate_evaluation(
        observation_levels,
        core_expected_increments,
        interval_deltas,
        calibration_grid_interval_hours,
        PRESSURE_TO_LEVEL_NO_MEAN_REVERSION_CANDIDATE_FAMILY,
        2,
        MeanReversionAnchorModel::endogenous_core_pressure_level(
            calibration_grid_interval_hours,
            pressure_persistence_per_reference_interval,
            least_squares_solution.first_coefficient,
        ),
        None,
        1.0,
        least_squares_solution.rank,
        response.as_slice(),
        &[],
        Some(pressure_column.as_slice()),
        pressure_state_path,
        PRESSURE_TO_LEVEL_NO_MEAN_REVERSION_REASON,
    )
}

fn profile_flat_anchor_mean_reversion_candidate(
    observation_levels: &[f64],
    calibration_grid_interval_hours: f64,
    solver_settings: &CalibrationOptimizerSettings,
) -> Result<MeanReversionStageCandidateEvaluation, MeanReversionStageExecutionFailure> {
    let phi_boundary_candidate = evaluate_flat_anchor_mean_reversion_candidate(
        observation_levels,
        calibration_grid_interval_hours,
        1.0,
    );

    profile_candidate_over_unit_interval(solver_settings, phi_boundary_candidate, |phi_reference| {
        Ok(evaluate_flat_anchor_mean_reversion_candidate(
            observation_levels,
            calibration_grid_interval_hours,
            phi_reference,
        ))
    })
}

fn evaluate_flat_anchor_mean_reversion_candidate(
    observation_levels: &[f64],
    calibration_grid_interval_hours: f64,
    phi_reference: f64,
) -> MeanReversionStageCandidateEvaluation {
    let response = observation_levels
        .iter()
        .zip(observation_levels.iter().skip(1))
        .map(|(current_observation, next_observation)| {
            next_observation - phi_reference * current_observation
        })
        .collect::<Vec<_>>();

    let intercept_column = vec![1.0 - phi_reference; observation_levels.len().saturating_sub(1)];

    let least_squares_solution =
        solve_one_column_least_squares(intercept_column.as_slice(), response.as_slice());

    let anchor_origin_level = if phi_reference == 1.0 {
        None
    } else {
        Some(least_squares_solution.first_coefficient)
    };

    let anchor_value = anchor_origin_level.unwrap_or(0.0);
    let boundary_anchor_values = vec![anchor_value; observation_levels.len()];

    let deviations = observation_levels
        .iter()
        .map(|observation| observation - anchor_value)
        .collect::<Vec<_>>();

    let residuals = response
        .iter()
        .zip(intercept_column.iter())
        .map(|(current_response, intercept_term)| {
            current_response - least_squares_solution.first_coefficient * intercept_term
        })
        .collect::<Vec<_>>();

    let residual_sum_of_squares = residuals
        .iter()
        .map(|residual| residual * residual)
        .sum::<f64>();

    let quasi_log_likelihood =
        gaussian_quasi_log_likelihood(residuals.len(), residual_sum_of_squares);

    MeanReversionStageCandidateEvaluation {
        candidate_family: FLAT_ANCHOR_MEAN_REVERSION_CANDIDATE_FAMILY,
        anchor_model: MeanReversionAnchorModel::endogenous_core_pressure_level(
            calibration_grid_interval_hours,
            0.0,
            0.0,
        ),
        anchor_origin_level,
        phi_reference,
        kappa: kappa_from_phi_reference(phi_reference, calibration_grid_interval_hours),
        residual_sum_of_squares,
        quasi_log_likelihood,
        information_criterion_bic: bic(residuals.len(), 2, quasi_log_likelihood),
        parameter_count: 2,
        least_squares_rank: least_squares_solution.rank,
        pressure_state_path: vec![0.0; observation_levels.len()],
        boundary_anchor_values,
        deviations,
        residuals,
        stage_decision_reason: stage_decision_reason_with_boundary_flags(
            FLAT_ANCHOR_MEAN_REVERSION_REASON,
            None,
            Some(phi_reference),
        ),
    }
}

fn evaluate_random_walk_null_candidate(
    observation_levels: &[f64],
    calibration_grid_interval_hours: f64,
) -> MeanReversionStageCandidateEvaluation {
    let residuals = observation_levels
        .iter()
        .zip(observation_levels.iter().skip(1))
        .map(|(current_observation, next_observation)| next_observation - current_observation)
        .collect::<Vec<_>>();

    let residual_sum_of_squares = residuals
        .iter()
        .map(|residual| residual * residual)
        .sum::<f64>();

    let quasi_log_likelihood =
        gaussian_quasi_log_likelihood(residuals.len(), residual_sum_of_squares);

    MeanReversionStageCandidateEvaluation {
        candidate_family: RANDOM_WALK_NULL_CANDIDATE_FAMILY,
        anchor_model: MeanReversionAnchorModel::endogenous_core_pressure_level(
            calibration_grid_interval_hours,
            0.0,
            0.0,
        ),
        anchor_origin_level: None,
        phi_reference: 1.0,
        kappa: 0.0,
        residual_sum_of_squares,
        quasi_log_likelihood,
        information_criterion_bic: bic(residuals.len(), 0, quasi_log_likelihood),
        parameter_count: 0,
        least_squares_rank: 0,
        pressure_state_path: vec![0.0; observation_levels.len()],
        boundary_anchor_values: vec![0.0; observation_levels.len()],
        deviations: observation_levels.to_vec(),
        residuals,
        stage_decision_reason: RANDOM_WALK_NULL_REASON.to_string(),
    }
}

fn select_best_admissible_candidate_by_information_criterion(
    evaluated_candidates: &[MeanReversionStageCandidateEvaluation],
) -> Option<MeanReversionStageCandidateEvaluation> {
    let mut admissible_candidates = evaluated_candidates
        .iter()
        .filter(|candidate| candidate_supports_quasi_likelihood_model_selection(candidate));

    let first_candidate = admissible_candidates.next()?.clone();

    Some(admissible_candidates.fold(first_candidate, |best_candidate, candidate| {
        if candidate.information_criterion_bic < best_candidate.information_criterion_bic {
            candidate.clone()
        } else {
            best_candidate
        }
    }))
}

fn candidate_supports_quasi_likelihood_model_selection(
    candidate: &MeanReversionStageCandidateEvaluation,
) -> bool {
    // The Gaussian quasi likelihood profiles the innovation variance from the same transition
    // sample as the structural coefficients, so the candidate needs one residual degree of
    // freedom after estimating both the structural parameters and that variance term.
    candidate.residuals.len() > candidate.parameter_count.saturating_add(1)
}

fn profile_candidate_over_pressure_persistence(
    solver_settings: &CalibrationOptimizerSettings,
    mut evaluate_candidate_at_pressure_persistence: impl FnMut(
        f64,
    ) -> Result<
        MeanReversionStageCandidateEvaluation,
        MeanReversionStageExecutionFailure,
    >,
) -> Result<MeanReversionStageCandidateEvaluation, MeanReversionStageExecutionFailure> {
    let psi_zero_candidate = evaluate_candidate_at_pressure_persistence(0.0)?;
    let psi_one_candidate = evaluate_candidate_at_pressure_persistence(1.0)?;

    let best_boundary_candidate =
        best_candidate_by_quasi_log_likelihood(psi_zero_candidate, psi_one_candidate);

    profile_candidate_over_unit_interval(
        solver_settings,
        best_boundary_candidate,
        |pressure_persistence| evaluate_candidate_at_pressure_persistence(pressure_persistence),
    )
}

fn profile_candidate_over_unit_interval<Error>(
    solver_settings: &CalibrationOptimizerSettings,
    mut best_candidate: MeanReversionStageCandidateEvaluation,
    mut evaluate_candidate_at_position: impl FnMut(
        f64,
    ) -> Result<
        MeanReversionStageCandidateEvaluation,
        Error,
    >,
) -> Result<MeanReversionStageCandidateEvaluation, Error> {
    let mut lower_position = 0.0_f64;
    let mut upper_position = 1.0_f64;

    let golden_ratio = (1.0 + 5.0_f64.sqrt()) / 2.0;

    let mut left_position = upper_position - (upper_position - lower_position) / golden_ratio;
    let mut right_position = lower_position + (upper_position - lower_position) / golden_ratio;

    let mut left_candidate = evaluate_candidate_at_position(left_position)?;
    let mut right_candidate = evaluate_candidate_at_position(right_position)?;

    for _ in 0..solver_settings.max_iterations {
        let width = upper_position - lower_position;
        let tolerance = solver_settings
            .absolute_tolerance
            .max(solver_settings.relative_tolerance * upper_position);

        if width <= tolerance {
            break;
        }

        if left_candidate.quasi_log_likelihood >= right_candidate.quasi_log_likelihood {
            upper_position = right_position;
            right_position = left_position;

            right_candidate = left_candidate;

            left_position = upper_position - (upper_position - lower_position) / golden_ratio;
            left_candidate = evaluate_candidate_at_position(left_position)?;
        } else {
            lower_position = left_position;
            left_position = right_position;

            left_candidate = right_candidate;

            right_position = lower_position + (upper_position - lower_position) / golden_ratio;
            right_candidate = evaluate_candidate_at_position(right_position)?;
        }
    }

    best_candidate = best_candidate_by_quasi_log_likelihood(best_candidate, left_candidate);
    best_candidate = best_candidate_by_quasi_log_likelihood(best_candidate, right_candidate);

    Ok(best_candidate)
}

fn best_candidate_by_quasi_log_likelihood(
    left_candidate: MeanReversionStageCandidateEvaluation,
    right_candidate: MeanReversionStageCandidateEvaluation,
) -> MeanReversionStageCandidateEvaluation {
    if right_candidate.quasi_log_likelihood > left_candidate.quasi_log_likelihood {
        right_candidate
    } else {
        left_candidate
    }
}

fn materialize_pressure_state_path(
    calibration_grid_interval_hours: f64,
    pressure_persistence_per_reference_interval: f64,
    core_expected_increments: &[f64],
    interval_deltas: &[f64],
) -> Result<Vec<f64>, MeanReversionStageExecutionFailure> {
    let anchor_model = MeanReversionAnchorModel::endogenous_core_pressure_level(
        calibration_grid_interval_hours,
        pressure_persistence_per_reference_interval,
        0.0,
    );

    let runtime_interval_anchors =
        materialize_mean_reversion_runtime_interval_anchors_from_core_expected_increments(
            CALIBRATION_GRID_ALIGNMENT_POLICY,
            0.0,
            0.0,
            &anchor_model,
            core_expected_increments,
            interval_deltas,
        )
        .map_err(|source| {
            MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
                error_message: source.to_string(),
            }
        })?;

    let mut pressure_state_path =
        Vec::with_capacity(runtime_interval_anchors.end_pressure_states.len() + 1);

    pressure_state_path.push(runtime_interval_anchors.start_pressure_state);
    pressure_state_path.extend(runtime_interval_anchors.end_pressure_states);

    Ok(pressure_state_path)
}

struct TransitionTerms {
    response: Vec<f64>,
    intercept_column: Vec<f64>,
    pressure_column: Vec<f64>,
}

fn build_transition_terms(
    observation_levels: &[f64],
    pressure_state_path: &[f64],
    interval_deltas: &[f64],
    calibration_grid_interval_hours: f64,
    phi_reference: f64,
) -> TransitionTerms {
    let mut response = Vec::with_capacity(interval_deltas.len());
    let mut intercept_column = Vec::with_capacity(interval_deltas.len());
    let mut pressure_column = Vec::with_capacity(interval_deltas.len());

    for (
        ((current_observation, next_observation), (current_pressure_state, next_pressure_state)),
        interval_delta,
    ) in observation_levels
        .iter()
        .zip(observation_levels.iter().skip(1))
        .zip(
            pressure_state_path
                .iter()
                .zip(pressure_state_path.iter().skip(1)),
        )
        .zip(interval_deltas.iter())
    {
        let phi = phi_reference.powf(interval_delta / calibration_grid_interval_hours);

        response.push(next_observation - phi * current_observation);
        intercept_column.push(1.0 - phi);
        pressure_column.push(next_pressure_state - phi * current_pressure_state);
    }

    TransitionTerms {
        response,
        intercept_column,
        pressure_column,
    }
}

#[allow(clippy::too_many_arguments)]
fn assemble_candidate_evaluation(
    observation_levels: &[f64],
    core_expected_increments: &[f64],
    interval_deltas: &[f64],
    calibration_grid_interval_hours: f64,
    candidate_family: &'static str,
    parameter_count: usize,
    anchor_model: MeanReversionAnchorModel,
    anchor_origin_level: Option<f64>,
    phi_reference: f64,
    least_squares_rank: usize,
    response: &[f64],
    intercept_column: &[f64],
    pressure_column: Option<&[f64]>,
    pressure_state_path: Vec<f64>,
    base_stage_decision_reason: &str,
) -> Result<MeanReversionStageCandidateEvaluation, MeanReversionStageExecutionFailure> {
    let anchor_origin_level_value = anchor_origin_level.unwrap_or(0.0);

    let pressure_persistence_per_reference_interval =
        anchor_model.pressure_persistence_per_reference_interval;

    let residuals = match (pressure_column, intercept_column.is_empty()) {
        (Some(pressure_column), true) => response
            .iter()
            .zip(pressure_column.iter())
            .map(|(current_response, current_pressure_term)| {
                current_response - anchor_model.pressure_level_gain * current_pressure_term
            })
            .collect::<Vec<_>>(),
        (Some(pressure_column), false) => response
            .iter()
            .zip(intercept_column.iter())
            .zip(pressure_column.iter())
            .map(
                |((current_response, intercept_term), current_pressure_term)| {
                    current_response
                        - anchor_origin_level_value * intercept_term
                        - anchor_model.pressure_level_gain * current_pressure_term
                },
            )
            .collect::<Vec<_>>(),
        (None, _) => response
            .iter()
            .zip(intercept_column.iter())
            .map(|(current_response, intercept_term)| {
                current_response - anchor_origin_level_value * intercept_term
            })
            .collect::<Vec<_>>(),
    };

    let residual_sum_of_squares = residuals
        .iter()
        .map(|residual| residual * residual)
        .sum::<f64>();

    let quasi_log_likelihood =
        gaussian_quasi_log_likelihood(residuals.len(), residual_sum_of_squares);

    let runtime_interval_anchors =
        materialize_mean_reversion_runtime_interval_anchors_from_core_expected_increments(
            CALIBRATION_GRID_ALIGNMENT_POLICY,
            anchor_origin_level_value,
            0.0,
            &anchor_model,
            core_expected_increments,
            interval_deltas,
        )
        .map_err(|source| {
            MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
                error_message: source.to_string(),
            }
        })?;

    let mut boundary_anchor_values =
        Vec::with_capacity(runtime_interval_anchors.end_anchor_values.len() + 1);

    boundary_anchor_values.push(runtime_interval_anchors.start_anchor_value);
    boundary_anchor_values.extend(runtime_interval_anchors.end_anchor_values);

    let deviations = observation_levels
        .iter()
        .zip(boundary_anchor_values.iter())
        .map(|(observed_log_price, anchor_value)| observed_log_price - anchor_value)
        .collect::<Vec<_>>();

    Ok(MeanReversionStageCandidateEvaluation {
        candidate_family,
        anchor_model,
        anchor_origin_level,
        phi_reference,
        kappa: kappa_from_phi_reference(phi_reference, calibration_grid_interval_hours),
        residual_sum_of_squares,
        quasi_log_likelihood,
        information_criterion_bic: bic(residuals.len(), parameter_count, quasi_log_likelihood),
        parameter_count,
        least_squares_rank,
        pressure_state_path,
        boundary_anchor_values,
        deviations,
        residuals,
        stage_decision_reason: stage_decision_reason_with_boundary_flags(
            base_stage_decision_reason,
            Some(pressure_persistence_per_reference_interval),
            Some(phi_reference),
        ),
    })
}

fn zero_kappa_endogenous_fit(
    calibration_grid: MeanReversionStageCalibrationGrid,
    stage_decision_reason: &str,
) -> (MeanReversionFittedParameters, MeanReversionStageDiagnostics) {
    let anchor_model = MeanReversionAnchorModel::endogenous_core_pressure_level(
        calibration_grid.calibration_grid_interval_hours,
        0.0,
        0.0,
    );

    let fitted_parameters = MeanReversionFittedParameters {
        kappa: 0.0,
        anchor_model: anchor_model.clone(),
    };

    let fallback_candidate = MeanReversionStageCandidateEvaluation {
        candidate_family: RANDOM_WALK_NULL_CANDIDATE_FAMILY,
        anchor_model: anchor_model.clone(),
        anchor_origin_level: None,
        phi_reference: 1.0,
        kappa: 0.0,
        residual_sum_of_squares: 0.0,
        quasi_log_likelihood: f64::NEG_INFINITY,
        information_criterion_bic: f64::INFINITY,
        parameter_count: 0,
        least_squares_rank: 0,
        pressure_state_path: Vec::new(),
        boundary_anchor_values: Vec::new(),
        deviations: Vec::new(),
        residuals: Vec::new(),
        stage_decision_reason: stage_decision_reason.to_string(),
    };

    (
        fitted_parameters,
        MeanReversionStageDiagnostics {
            selected_candidate_family: RANDOM_WALK_NULL_CANDIDATE_FAMILY.to_string(),
            calibration_grid,
            anchor_parameter_estimates: anchor_model,
            anchor_origin_level: None,
            phi_reference: 1.0,
            kappa: 0.0,
            residual_sum_of_squares: 0.0,
            quasi_log_likelihood: None,
            information_criterion_bic: None,
            model_selection_policy: ENDOGENOUS_MODEL_SELECTION_POLICY.to_string(),
            deviation_sample_count: 0,
            regression_pair_count: 0,
            least_squares_rank: 0,
            evaluated_candidates: vec![fallback_candidate.into_diagnostics(&[])],
            stage_decision_reason: stage_decision_reason.to_string(),
        },
    )
}

fn zero_kappa_endogenous_fit_with_evaluated_candidates(
    calibration_grid: MeanReversionStageCalibrationGrid,
    stage_decision_reason: &str,
    evaluated_candidates: Vec<MeanReversionStageCandidateEvaluation>,
    core_expected_increments: &[f64],
) -> (MeanReversionFittedParameters, MeanReversionStageDiagnostics) {
    let anchor_model = MeanReversionAnchorModel::endogenous_core_pressure_level(
        calibration_grid.calibration_grid_interval_hours,
        0.0,
        0.0,
    );

    let fitted_parameters = MeanReversionFittedParameters {
        kappa: 0.0,
        anchor_model: anchor_model.clone(),
    };

    (
        fitted_parameters,
        MeanReversionStageDiagnostics {
            selected_candidate_family: RANDOM_WALK_NULL_CANDIDATE_FAMILY.to_string(),
            calibration_grid,
            anchor_parameter_estimates: anchor_model,
            anchor_origin_level: None,
            phi_reference: 1.0,
            kappa: 0.0,
            residual_sum_of_squares: 0.0,
            quasi_log_likelihood: None,
            information_criterion_bic: None,
            model_selection_policy: ENDOGENOUS_MODEL_SELECTION_POLICY.to_string(),
            deviation_sample_count: 0,
            regression_pair_count: 0,
            least_squares_rank: 0,
            evaluated_candidates: evaluated_candidates
                .into_iter()
                .map(|candidate| candidate.into_diagnostics(core_expected_increments))
                .collect(),
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

fn solve_two_column_least_squares(
    first_column: &[f64],
    second_column: &[f64],
    response: &[f64],
) -> MeanReversionLeastSquaresSolution {
    let first_self_inner_product = first_column.iter().map(|value| value * value).sum::<f64>();
    let second_self_inner_product = second_column.iter().map(|value| value * value).sum::<f64>();

    let cross_inner_product = first_column
        .iter()
        .zip(second_column.iter())
        .map(|(first_value, second_value)| first_value * second_value)
        .sum::<f64>();

    let first_response_inner_product = first_column
        .iter()
        .zip(response.iter())
        .map(|(first_value, response_value)| first_value * response_value)
        .sum::<f64>();

    let second_response_inner_product = second_column
        .iter()
        .zip(response.iter())
        .map(|(second_value, response_value)| second_value * response_value)
        .sum::<f64>();

    let first_column_is_zero = first_self_inner_product == 0.0;
    let second_column_is_zero = second_self_inner_product == 0.0;

    if first_column_is_zero && second_column_is_zero {
        return MeanReversionLeastSquaresSolution {
            first_coefficient: 0.0,
            second_coefficient: 0.0,
            rank: 0,
        };
    }

    if first_column_is_zero {
        return MeanReversionLeastSquaresSolution {
            first_coefficient: 0.0,
            second_coefficient: second_response_inner_product / second_self_inner_product,
            rank: 1,
        };
    }

    if second_column_is_zero {
        return MeanReversionLeastSquaresSolution {
            first_coefficient: first_response_inner_product / first_self_inner_product,
            second_coefficient: 0.0,
            rank: 1,
        };
    }

    if let Some([first_coefficient, second_coefficient]) = solve_two_by_two_linear_system(
        first_self_inner_product,
        cross_inner_product,
        cross_inner_product,
        second_self_inner_product,
        [first_response_inner_product, second_response_inner_product],
    ) {
        return MeanReversionLeastSquaresSolution {
            first_coefficient,
            second_coefficient,
            rank: 2,
        };
    }

    let dependence_ratio = cross_inner_product / first_self_inner_product;
    let effective_coefficient = first_response_inner_product / first_self_inner_product;
    let normalization = 1.0 + dependence_ratio * dependence_ratio;

    MeanReversionLeastSquaresSolution {
        first_coefficient: effective_coefficient / normalization,
        second_coefficient: effective_coefficient * dependence_ratio / normalization,
        rank: 1,
    }
}

fn solve_one_column_least_squares(
    column: &[f64],
    response: &[f64],
) -> MeanReversionLeastSquaresSolution {
    let self_inner_product = column.iter().map(|value| value * value).sum::<f64>();
    if self_inner_product == 0.0 {
        return MeanReversionLeastSquaresSolution {
            first_coefficient: 0.0,
            second_coefficient: 0.0,
            rank: 0,
        };
    }

    let response_inner_product = column
        .iter()
        .zip(response.iter())
        .map(|(column_value, response_value)| column_value * response_value)
        .sum::<f64>();

    MeanReversionLeastSquaresSolution {
        first_coefficient: response_inner_product / self_inner_product,
        second_coefficient: 0.0,
        rank: 1,
    }
}

fn kappa_from_phi_reference(phi_reference: f64, calibration_grid_interval_hours: f64) -> f64 {
    if phi_reference == 1.0 {
        0.0
    } else {
        -phi_reference.ln() / calibration_grid_interval_hours
    }
}

fn gaussian_quasi_log_likelihood(
    regression_pair_count: usize,
    residual_sum_of_squares: f64,
) -> f64 {
    if regression_pair_count == 0 {
        return f64::NEG_INFINITY;
    }
    if residual_sum_of_squares == 0.0 {
        return f64::INFINITY;
    }

    let pair_count = regression_pair_count as f64;
    let sigma_squared_hat = residual_sum_of_squares / pair_count;

    -0.5 * pair_count * (1.0 + (2.0 * std::f64::consts::PI * sigma_squared_hat).ln())
}

fn bic(regression_pair_count: usize, parameter_count: usize, quasi_log_likelihood: f64) -> f64 {
    if quasi_log_likelihood.is_infinite() && quasi_log_likelihood.is_sign_positive() {
        return f64::NEG_INFINITY;
    }
    if regression_pair_count == 0 {
        return f64::INFINITY;
    }

    (parameter_count as f64) * (regression_pair_count as f64).ln() - 2.0 * quasi_log_likelihood
}

fn finite_metric_or_none(value: f64) -> Option<f64> {
    if value.is_finite() { Some(value) } else { None }
}

fn stage_decision_reason_with_boundary_flags(
    base_reason: &str,
    pressure_persistence_per_reference_interval: Option<f64>,
    phi_reference: Option<f64>,
) -> String {
    let mut reason = base_reason.to_string();

    match pressure_persistence_per_reference_interval {
        Some(0.0) => {
            reason.push_str("|pressure_persistence_per_reference_interval_boundary_zero.v1");
        }
        Some(1.0) => {
            reason.push_str("|pressure_persistence_per_reference_interval_boundary_one.v1");
        }
        _ => {}
    }

    if phi_reference == Some(1.0) {
        reason.push_str("|phi_reference_boundary_one.v1");
    }

    reason
}
