use serde::Serialize;

use janiform_calibration_artifacts::{
    CALIBRATION_MEAN_REVERSION_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_MEAN_REVERSION_STAGE_HANDOFF_POLICY,
    CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION, CalibrationExecutionMode,
    CalibrationOptimizerSettings, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStageSeedSource,
    MeanReversionStageCalibrationGrid, MeanReversionStageDiagnostics,
};
use janiform_foundations::numeric::compute_finite_median_or_zero;
use janiform_model_kernels::mean_reversion::{
    MeanReversionAnchorModel, MeanReversionAnchorModelKind, MeanReversionFittedParameters,
};

use crate::calendar::ExchangeCalendarRegistry;
use crate::stages::seasonality::CalibrationStageExecutionStatus;

const MINIMUM_MEAN_REVERSION_OBSERVATION_COUNT: usize = 3;
const MINIMUM_MEAN_REVERSION_COVERAGE_RATIO: f64 = 0.5;
const MINIMUM_PHI: f64 = 1.0e-12;
const DETERMINISTIC_ANCHOR_ALIGNMENT_POLICY: &str = "piecewise_constant_utc_interval_grid.v1";
const DISABLED_REQUEST_REASON: &str = "requested_mean_reversion_disabled.v1";
const INSUFFICIENT_SAMPLE_REASON: &str = "insufficient_mean_reversion_observation_count.v1";
const INSUFFICIENT_COVERAGE_REASON: &str = "insufficient_mean_reversion_grid_coverage.v1";
const CONSTANT_ANCHOR_REASON: &str = "constant_anchor_fitted_from_median_log_price_levels.v1";
const DETERMINISTIC_ANCHOR_REASON: &str =
    "deterministic_interval_anchor_fitted_from_local_median_log_price_levels.v1";

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
pub struct MeanReversionStageExecutionInput {
    pub return_components_input: Option<MeanReversionStageReturnComponentsInput>,
    pub requested_anchor_model_kind: MeanReversionAnchorModelKind,
    pub calibration_grid_kind: String,
    pub calibration_grid_interval_hours: f64,
    pub source_observation_count: usize,
    pub coverage_ratio: f64,
    pub observations: Vec<MeanReversionStageObservation>,
    pub disabled_stage_reason: Option<String>,
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
    _calendar_registry: &ExchangeCalendarRegistry,
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

    let (fitted_parameters, diagnostics) =
        estimate_mean_reversion_fitted_parameters(&execution_input);

    let (return_components_selected_candidate_id, return_components_artifact_reference) =
        match execution_input
            .return_components_input
            .as_ref()
            .expect("mean-reversion stage should receive return-components handoff")
        {
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
            return_components_input_transport: execution_input
                .return_components_input
                .as_ref()
                .expect("mean-reversion stage should receive return-components handoff")
                .transport(),
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
    execution_input: &MeanReversionStageExecutionInput,
) -> (MeanReversionFittedParameters, MeanReversionStageDiagnostics) {
    let calibration_grid = MeanReversionStageCalibrationGrid {
        calibration_grid_kind: execution_input.calibration_grid_kind.clone(),
        calibration_grid_interval_hours: execution_input.calibration_grid_interval_hours,
        source_observation_count: execution_input.source_observation_count,
        retained_observation_count: execution_input.observations.len(),
        coverage_ratio: execution_input.coverage_ratio,
    };

    if execution_input.requested_anchor_model_kind == MeanReversionAnchorModelKind::Disabled {
        return disabled_stage_fit(
            calibration_grid,
            execution_input
                .disabled_stage_reason
                .clone()
                .unwrap_or_else(|| DISABLED_REQUEST_REASON.to_string()),
        );
    }

    if execution_input.observations.len() < MINIMUM_MEAN_REVERSION_OBSERVATION_COUNT {
        return disabled_stage_fit(calibration_grid, INSUFFICIENT_SAMPLE_REASON.to_string());
    }

    if !execution_input.coverage_ratio.is_finite()
        || execution_input.coverage_ratio < MINIMUM_MEAN_REVERSION_COVERAGE_RATIO
    {
        return disabled_stage_fit(calibration_grid, INSUFFICIENT_COVERAGE_REASON.to_string());
    }

    let observation_levels: Vec<f64> = execution_input
        .observations
        .iter()
        .map(|observation| observation.observed_log_price)
        .collect();
    let observation_times: Vec<i64> = execution_input
        .observations
        .iter()
        .map(|observation| observation.observation_timestamp_utc_nanos)
        .collect();

    match execution_input.requested_anchor_model_kind {
        MeanReversionAnchorModelKind::Disabled => {
            disabled_stage_fit(calibration_grid, DISABLED_REQUEST_REASON.to_string())
        }
        MeanReversionAnchorModelKind::Constant => {
            let anchor_value = compute_finite_median_or_zero(observation_levels.as_slice());
            let anchor_model = MeanReversionAnchorModel::constant(anchor_value)
                .expect("constant anchor fit should validate");

            let boundary_anchor_values = vec![anchor_value; observation_levels.len()];

            let (phi, kappa) = estimate_phi_and_kappa(
                observation_levels.as_slice(),
                boundary_anchor_values.as_slice(),
                execution_input.calibration_grid_interval_hours,
            );

            let fitted_parameters =
                MeanReversionFittedParameters::try_new(kappa, anchor_model.clone())
                    .expect("constant mean reversion fit should validate");

            (
                fitted_parameters,
                MeanReversionStageDiagnostics {
                    chosen_anchor_family: MeanReversionAnchorModelKind::Constant,
                    calibration_grid,
                    anchor_parameter_estimates: anchor_model,
                    phi,
                    kappa,
                    deviation_sample_count: observation_levels.len(),
                    regression_pair_count: observation_levels.len().saturating_sub(1),
                    stage_decision_reason: CONSTANT_ANCHOR_REASON.to_string(),
                },
            )
        }
        MeanReversionAnchorModelKind::DeterministicIntervalGrid => {
            let interval_anchor_values =
                smoothed_interval_anchor_values(observation_levels.as_slice());

            let anchor_model = MeanReversionAnchorModel::deterministic_interval_grid(
                observation_times[..observation_times.len().saturating_sub(1)].to_vec(),
                observation_times.iter().skip(1).copied().collect(),
                interval_anchor_values.clone(),
                DETERMINISTIC_ANCHOR_ALIGNMENT_POLICY,
            )
            .expect("deterministic interval-grid anchor fit should validate");

            let boundary_anchor_values =
                boundary_anchor_values_from_interval_anchors(interval_anchor_values.as_slice());

            let (phi, kappa) = estimate_phi_and_kappa(
                observation_levels.as_slice(),
                boundary_anchor_values.as_slice(),
                execution_input.calibration_grid_interval_hours,
            );

            let fitted_parameters =
                MeanReversionFittedParameters::try_new(kappa, anchor_model.clone())
                    .expect("deterministic mean reversion fit should validate");

            (
                fitted_parameters,
                MeanReversionStageDiagnostics {
                    chosen_anchor_family: MeanReversionAnchorModelKind::DeterministicIntervalGrid,
                    calibration_grid,
                    anchor_parameter_estimates: anchor_model,
                    phi,
                    kappa,
                    deviation_sample_count: observation_levels.len(),
                    regression_pair_count: observation_levels.len().saturating_sub(1),
                    stage_decision_reason: DETERMINISTIC_ANCHOR_REASON.to_string(),
                },
            )
        }
    }
}

fn disabled_stage_fit(
    calibration_grid: MeanReversionStageCalibrationGrid,
    stage_decision_reason: String,
) -> (MeanReversionFittedParameters, MeanReversionStageDiagnostics) {
    let fitted_parameters = MeanReversionFittedParameters::disabled();

    (
        fitted_parameters.clone(),
        MeanReversionStageDiagnostics {
            chosen_anchor_family: MeanReversionAnchorModelKind::Disabled,
            calibration_grid,
            anchor_parameter_estimates: fitted_parameters.anchor_model.clone(),
            phi: 1.0,
            kappa: 0.0,
            deviation_sample_count: 0,
            regression_pair_count: 0,
            stage_decision_reason,
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
    if denominator <= 1.0e-12 {
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

fn smoothed_interval_anchor_values(observation_levels: &[f64]) -> Vec<f64> {
    observation_levels
        .windows(2)
        .enumerate()
        .map(|(interval_index, _)| {
            let window_start = interval_index.saturating_sub(1);
            let window_end = (interval_index + 2).min(observation_levels.len());
            compute_finite_median_or_zero(&observation_levels[window_start..window_end])
        })
        .collect()
}

fn boundary_anchor_values_from_interval_anchors(interval_anchor_values: &[f64]) -> Vec<f64> {
    if interval_anchor_values.is_empty() {
        return vec![0.0];
    }

    let mut boundary_anchor_values = interval_anchor_values.to_vec();
    boundary_anchor_values.push(*interval_anchor_values.last().unwrap_or(&0.0));

    boundary_anchor_values
}
