use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::calendar::{
    ExchangeCalendarRegistry, TimestampEventInputRecord, TimestampEventSessionFilter,
};
use crate::seasonality::{
    DayOfWeekSeasonalityEstimatorSettings, DayOfYearSeasonalityEstimatorSettings,
    IntradaySeasonalityEstimatorSettings, IntradaySmoothingKernel, ScheduledEventOverlayDefinition,
};
use janiform_calibration_artifacts::{
    CalibrationExecutionMode, CalibrationRunContract, CalibrationRunContractError,
    CalibrationRunSpec, CalibrationStage, CalibrationStageDependency, CalibrationStageGraph,
    ReturnComponentsStageFittedParameters,
};
use janiform_model_kernels::hawkes::{
    HawkesStructuredAlphaParameterization, HawkesStructuredRhoParameterization,
    HawkesStructuredRhoSelfDominancePolicy,
};
use janiform_model_kernels::news::{
    CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
    GammaSeverityMarkConfiguration, SharedNewsMarkLawConfiguration,
};

use super::checkpointing::CalibrationMemoryCheckpointStore;
use super::staged_runner::{
    CalibrationStagedRunInputs, CalibrationStagedRunObserverEvent, CalibrationStagedRunReport,
    run_staged_calibration_with_checkpointing,
    run_staged_calibration_with_checkpointing_and_observer,
};
use super::stages::hawkes::HawkesStageCandidateInput;
use super::stages::mean_reversion::{
    MeanReversionStageExecutionInput, MeanReversionStageObservation,
};
use super::stages::return_components::{
    ReturnComponentsCloseOpenGapDetectionPolicy, ReturnComponentsDetectedIntradayJumpCandidate,
    ReturnComponentsKnownNewsCalendarInput, ReturnComponentsObservedCloseOpenBoundaryObservation,
    ReturnComponentsObservedOpenSessionReturn, ReturnComponentsRealizedKnownNewsReturnSample,
    ReturnComponentsStageActivityCalibrationContextSnapshot,
    ReturnComponentsStageCandidateGenerationInput, ReturnComponentsStageExactCandidateEvaluation,
    ReturnComponentsStageExactCandidateEvaluationError,
    ReturnComponentsStageObjectiveMetricWeights,
    ReturnComponentsStageObservedSignalSummaryStatistics, ReturnComponentsStageSignalSourceWindow,
    evaluate_return_components_exact_candidate,
};
use super::stages::seasonality::{
    SeasonalityStageExecutionInput, SeasonalityStageExecutionSettings,
};

pub const CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION: &str =
    "calibration.orchestration.staged_execution_input.v1";

#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationStagedExecutionPlan {
    pub run_contract: CalibrationRunContract,
    pub staged_run_inputs: CalibrationStagedRunInputs,
    pub disk_checkpoint_directory: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationStagedExecutionInput {
    pub schema_version: String,
    pub request: CalibrationRunSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seasonality_stage_input: Option<CalibrationSeasonalityStageExecutionInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hawkes_candidate_inputs: Option<Vec<CalibrationHawkesStageCandidateInput>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_components_candidate_generation_input:
        Option<CalibrationReturnComponentsStageCandidateGenerationInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mean_reversion_stage_input: Option<CalibrationMeanReversionStageExecutionInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_resources: Option<CalibrationStagedExecutionRuntimeResources>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CalibrationStagedExecutionRuntimeResources {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_checkpoint_directory: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationTimestampEventSessionFilter {
    All,
    OpenOnly,
    ClosedOnly,
}

impl From<CalibrationTimestampEventSessionFilter> for TimestampEventSessionFilter {
    fn from(value: CalibrationTimestampEventSessionFilter) -> Self {
        match value {
            CalibrationTimestampEventSessionFilter::All => Self::All,
            CalibrationTimestampEventSessionFilter::OpenOnly => Self::OpenOnly,
            CalibrationTimestampEventSessionFilter::ClosedOnly => Self::ClosedOnly,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationIntradaySmoothingKernel {
    Gaussian,
    Uniform,
}

impl From<CalibrationIntradaySmoothingKernel> for IntradaySmoothingKernel {
    fn from(value: CalibrationIntradaySmoothingKernel) -> Self {
        match value {
            CalibrationIntradaySmoothingKernel::Gaussian => Self::Gaussian,
            CalibrationIntradaySmoothingKernel::Uniform => Self::Uniform,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationTimestampEventRecord {
    pub source_record_position: u64,
    pub timestamp_utc_nanos: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationScheduledEventOverlay {
    pub overlay_id: String,
    pub start_timestamp_utc_nanos: i64,
    pub end_timestamp_utc_nanos: i64,
    pub log_adjustment: f64,
    pub precedence: u16,
    pub enabled: bool,
    pub source_label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationIntradaySeasonalityEstimatorSettings {
    pub minutes_per_bin: u16,
    pub smoothing_kernel: CalibrationIntradaySmoothingKernel,
    pub smoothing_bandwidth_bins: f64,
    pub minimum_bin_events: usize,
    pub pseudocount: f64,
}

impl From<CalibrationIntradaySeasonalityEstimatorSettings>
    for IntradaySeasonalityEstimatorSettings
{
    fn from(value: CalibrationIntradaySeasonalityEstimatorSettings) -> Self {
        Self {
            minutes_per_bin: value.minutes_per_bin,
            smoothing_kernel: value.smoothing_kernel.into(),
            smoothing_bandwidth_bins: value.smoothing_bandwidth_bins,
            minimum_bin_events: value.minimum_bin_events,
            pseudocount: value.pseudocount,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationDayOfWeekSeasonalityEstimatorSettings {
    pub minimum_bucket_events: usize,
    pub pseudocount: f64,
}

impl From<CalibrationDayOfWeekSeasonalityEstimatorSettings>
    for DayOfWeekSeasonalityEstimatorSettings
{
    fn from(value: CalibrationDayOfWeekSeasonalityEstimatorSettings) -> Self {
        Self {
            minimum_bucket_events: value.minimum_bucket_events,
            pseudocount: value.pseudocount,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationDayOfYearSeasonalityEstimatorSettings {
    pub minimum_bucket_events: usize,
    pub pseudocount: f64,
}

impl From<CalibrationDayOfYearSeasonalityEstimatorSettings>
    for DayOfYearSeasonalityEstimatorSettings
{
    fn from(value: CalibrationDayOfYearSeasonalityEstimatorSettings) -> Self {
        Self {
            minimum_bucket_events: value.minimum_bucket_events,
            pseudocount: value.pseudocount,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationSeasonalityStageExecutionSettings {
    pub session_filter: CalibrationTimestampEventSessionFilter,
    pub sparse_hour_event_threshold: usize,
    pub intraday_settings: CalibrationIntradaySeasonalityEstimatorSettings,
    pub day_of_week_settings: CalibrationDayOfWeekSeasonalityEstimatorSettings,
    pub day_of_year_settings: CalibrationDayOfYearSeasonalityEstimatorSettings,
}

impl From<CalibrationSeasonalityStageExecutionSettings> for SeasonalityStageExecutionSettings {
    fn from(value: CalibrationSeasonalityStageExecutionSettings) -> Self {
        Self {
            session_filter: value.session_filter.into(),
            sparse_hour_event_threshold: value.sparse_hour_event_threshold,
            intraday_settings: value.intraday_settings.into(),
            day_of_week_settings: value.day_of_week_settings.into(),
            day_of_year_settings: value.day_of_year_settings.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationSeasonalityStageExecutionInput {
    pub timestamp_event_records: Vec<CalibrationTimestampEventRecord>,
    #[serde(default)]
    pub scheduled_event_overlays: Vec<CalibrationScheduledEventOverlay>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<CalibrationSeasonalityStageExecutionSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset_id_override: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimator_version_override: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationHawkesObservedSourceRegime {
    pub observation_duration_hours: f64,
    pub open_session_duration_hours: f64,
    pub total_tick_count: usize,
    pub open_session_tick_count: usize,
    pub closed_session_tick_count: usize,
    pub open_session_buy_tick_count: usize,
    pub open_session_sell_tick_count: usize,
    pub open_session_mean_interarrival_seconds: f64,
    pub open_session_interarrival_coefficient_of_variation: f64,
    pub open_session_short_gap_threshold_seconds: f64,
    pub open_session_short_gap_ratio: f64,
    pub known_news_open_event_count: usize,
    pub open_session_known_news_activity_share: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationHawkesDirectionalLabelDiagnostics {
    pub policy_id: String,
    pub first_inferred_trade_sign: String,
    pub provided_label_count: usize,
    pub inferred_label_count: usize,
    pub flat_move_carry_count: usize,
    pub price_up_inference_count: usize,
    pub price_down_inference_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationHawkesStageCandidateInput {
    pub candidate_id: String,
    pub lambda_bar: f64,
    pub beta: f64,
    pub structured_alpha: HawkesStructuredAlphaParameterization,
    pub structured_rho: HawkesStructuredRhoParameterization,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_dominance_policy: Option<HawkesStructuredRhoSelfDominancePolicy>,
    pub stability_numeric_tolerance: f64,
    pub source_regime: CalibrationHawkesObservedSourceRegime,
    pub directional_label_diagnostics: CalibrationHawkesDirectionalLabelDiagnostics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationReturnComponentsSignalSourceWindow {
    pub window_start_utc_nanos: i64,
    pub window_end_utc_nanos: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationReturnComponentsKnownNewsCalendarInput {
    pub source_window: CalibrationReturnComponentsSignalSourceWindow,
    pub scheduled_event_timestamps_utc_nanos: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationReturnComponentsKnownNewsReturnSample {
    pub event_timestamp_utc_nanos: i64,
    pub open_window_jump_return: f64,
    pub closed_window_gap_return: f64,
    pub post_shock_activity_lift: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationReturnComponentsDetectedIntradayJumpCandidate {
    pub jump_timestamp_utc_nanos: i64,
    pub jump_return: f64,
    pub post_shock_activity_lift: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationReturnComponentsObservedOpenSessionReturn {
    pub return_timestamp_utc_nanos: i64,
    pub log_return: f64,
    pub post_shock_activity_lift: f64,
    pub sell_share_proxy: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationReturnComponentsObservedCloseOpenBoundaryObservation {
    pub close_timestamp_utc_nanos: i64,
    pub close_price: f64,
    pub open_timestamp_utc_nanos: i64,
    pub open_price: f64,
    pub post_shock_activity_lift: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationReturnComponentsCloseOpenGapDetectionPolicy {
    pub material_gap_absolute_return_tolerance: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationReturnComponentsObjectiveMetricWeights {
    pub continuous_distribution_fit_weight: f64,
    pub surprise_arrival_fit_weight: f64,
    pub jump_fit_weight: f64,
    pub gap_fit_weight: f64,
    pub activity_coupling_fit_weight: f64,
    pub price_range_sanity_fit_weight: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationSharedNewsMarkLawConfiguration {
    pub gamma_severity_marks: GammaSeverityMarkConfiguration,
    pub double_exponential_base_shock: DoubleExponentialBaseShockConfiguration,
    pub coupled_gap_marks: CoupledGapMarkConfiguration,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationReturnComponentsStageCandidateGenerationInput {
    pub known_news_calendar: CalibrationReturnComponentsKnownNewsCalendarInput,
    pub realized_known_news_return_samples: Vec<CalibrationReturnComponentsKnownNewsReturnSample>,
    pub observed_open_session_returns: Vec<CalibrationReturnComponentsObservedOpenSessionReturn>,
    pub detected_intraday_jump_candidates:
        Vec<CalibrationReturnComponentsDetectedIntradayJumpCandidate>,
    pub observed_close_open_boundary_observations:
        Vec<CalibrationReturnComponentsObservedCloseOpenBoundaryObservation>,
    pub close_open_gap_detection_policy: CalibrationReturnComponentsCloseOpenGapDetectionPolicy,
    pub open_session_sell_share_proxy: f64,
    pub shared_mark_law_template: CalibrationSharedNewsMarkLawConfiguration,
    pub objective_metric_weights: CalibrationReturnComponentsObjectiveMetricWeights,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationReturnComponentsActivityContextImpliedMetrics {
    pub model_implied_open_event_rate_per_hour: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationReturnComponentsActivityContextCandidateDiagnostic {
    pub candidate_id: String,
    pub source_regime: CalibrationHawkesObservedSourceRegime,
    pub implied_metrics: CalibrationReturnComponentsActivityContextImpliedMetrics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationReturnComponentsActivityContextPayload {
    pub selected_candidate_id: String,
    pub candidate_diagnostics: Vec<CalibrationReturnComponentsActivityContextCandidateDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationMeanReversionStageObservation {
    pub observation_timestamp_utc_nanos: i64,
    pub observed_log_price: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationMeanReversionStageExecutionInput {
    pub calibration_grid_kind: String,
    pub calibration_grid_interval_hours: f64,
    pub source_observation_count: usize,
    pub coverage_ratio: f64,
    #[serde(default)]
    pub observations: Vec<CalibrationMeanReversionStageObservation>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationStagedExecutionBridgeError {
    UnsupportedSchemaVersion {
        expected: &'static str,
        found: String,
    },
    InvalidRunContract {
        source: CalibrationRunContractError,
    },
    MissingStageExecutionInput {
        stage: CalibrationStage,
    },
    UnexpectedStageExecutionInput {
        stage: CalibrationStage,
    },
    MissingDiskCheckpointDirectory,
    EmptyDiskCheckpointDirectory,
    UnexpectedDiskCheckpointDirectoryForMemoryMode,
}

impl Display for CalibrationStagedExecutionBridgeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion { expected, found } => write!(
                formatter,
                "staged calibration execution input schema_version must be `{expected}`, found `{found}`"
            ),
            Self::InvalidRunContract { source } => {
                write!(
                    formatter,
                    "staged calibration request is not runnable: {source}"
                )
            }
            Self::MissingStageExecutionInput { stage } => write!(
                formatter,
                "staged calibration execution input requires explicit stage input for `{stage:?}`"
            ),
            Self::UnexpectedStageExecutionInput { stage } => write!(
                formatter,
                "staged calibration execution input provided stage input for unselected stage `{stage:?}`"
            ),
            Self::MissingDiskCheckpointDirectory => write!(
                formatter,
                "runtime_resources.disk_checkpoint_directory is required when execution_mode=`disk`"
            ),
            Self::EmptyDiskCheckpointDirectory => write!(
                formatter,
                "runtime_resources.disk_checkpoint_directory must be non-empty when provided"
            ),
            Self::UnexpectedDiskCheckpointDirectoryForMemoryMode => write!(
                formatter,
                "runtime_resources.disk_checkpoint_directory is only supported when execution_mode=`disk`"
            ),
        }
    }
}

impl Error for CalibrationStagedExecutionBridgeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRunContract { source } => Some(source),
            _ => None,
        }
    }
}

impl CalibrationStagedExecutionInput {
    pub fn build_execution_plan(
        self,
    ) -> Result<CalibrationStagedExecutionPlan, CalibrationStagedExecutionBridgeError> {
        if self.schema_version != CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION {
            return Err(
                CalibrationStagedExecutionBridgeError::UnsupportedSchemaVersion {
                    expected: CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION,
                    found: self.schema_version,
                },
            );
        }

        let request = self.request;
        let selected_stages = request.selected_stages.clone();

        let seasonality_selected = selected_stages.contains(&CalibrationStage::Seasonality);
        let hawkes_selected = selected_stages.contains(&CalibrationStage::Hawkes);
        let return_components_selected =
            selected_stages.contains(&CalibrationStage::ReturnComponents);
        let mean_reversion_selected = selected_stages.contains(&CalibrationStage::MeanReversion);

        if seasonality_selected && self.seasonality_stage_input.is_none() {
            return Err(
                CalibrationStagedExecutionBridgeError::MissingStageExecutionInput {
                    stage: CalibrationStage::Seasonality,
                },
            );
        }
        if !seasonality_selected && self.seasonality_stage_input.is_some() {
            return Err(
                CalibrationStagedExecutionBridgeError::UnexpectedStageExecutionInput {
                    stage: CalibrationStage::Seasonality,
                },
            );
        }

        if hawkes_selected && self.hawkes_candidate_inputs.is_none() {
            return Err(
                CalibrationStagedExecutionBridgeError::MissingStageExecutionInput {
                    stage: CalibrationStage::Hawkes,
                },
            );
        }
        if !hawkes_selected && self.hawkes_candidate_inputs.is_some() {
            return Err(
                CalibrationStagedExecutionBridgeError::UnexpectedStageExecutionInput {
                    stage: CalibrationStage::Hawkes,
                },
            );
        }

        if return_components_selected && self.return_components_candidate_generation_input.is_none()
        {
            return Err(
                CalibrationStagedExecutionBridgeError::MissingStageExecutionInput {
                    stage: CalibrationStage::ReturnComponents,
                },
            );
        }
        if !return_components_selected
            && self.return_components_candidate_generation_input.is_some()
        {
            return Err(
                CalibrationStagedExecutionBridgeError::UnexpectedStageExecutionInput {
                    stage: CalibrationStage::ReturnComponents,
                },
            );
        }

        if mean_reversion_selected && self.mean_reversion_stage_input.is_none() {
            return Err(
                CalibrationStagedExecutionBridgeError::MissingStageExecutionInput {
                    stage: CalibrationStage::MeanReversion,
                },
            );
        }
        if !mean_reversion_selected && self.mean_reversion_stage_input.is_some() {
            return Err(
                CalibrationStagedExecutionBridgeError::UnexpectedStageExecutionInput {
                    stage: CalibrationStage::MeanReversion,
                },
            );
        }

        let disk_checkpoint_directory =
            resolve_disk_checkpoint_directory(request.execution_mode, self.runtime_resources)?;

        let stage_graph =
            canonical_stage_graph_for_selected_stages(request.selected_stages.as_slice());
        let run_contract =
            CalibrationRunContract::try_new(request, stage_graph).map_err(|source| {
                CalibrationStagedExecutionBridgeError::InvalidRunContract { source }
            })?;

        let staged_run_inputs = CalibrationStagedRunInputs {
            seasonality_stage_input: self
                .seasonality_stage_input
                .map(CalibrationSeasonalityStageExecutionInput::try_into_core)
                .transpose()?,
            hawkes_candidate_inputs: self.hawkes_candidate_inputs.map(|candidate_inputs| {
                candidate_inputs
                    .into_iter()
                    .map(CalibrationHawkesStageCandidateInput::into_core)
                    .collect()
            }),
            return_components_candidate_generation_input: self
                .return_components_candidate_generation_input
                .map(CalibrationReturnComponentsStageCandidateGenerationInput::into_core),
            mean_reversion_stage_input: self
                .mean_reversion_stage_input
                .map(CalibrationMeanReversionStageExecutionInput::into_core),
        };

        Ok(CalibrationStagedExecutionPlan {
            run_contract,
            staged_run_inputs,
            disk_checkpoint_directory,
        })
    }
}

pub fn run_staged_calibration_from_execution_input(
    execution_input: CalibrationStagedExecutionInput,
    calendar_registry: &ExchangeCalendarRegistry,
    memory_checkpoint_store: Option<&mut CalibrationMemoryCheckpointStore>,
) -> Result<CalibrationStagedRunReport, CalibrationStagedExecutionBridgeError> {
    let execution_plan = execution_input.build_execution_plan()?;

    Ok(run_staged_calibration_with_checkpointing(
        &execution_plan.run_contract,
        calendar_registry,
        execution_plan.staged_run_inputs,
        memory_checkpoint_store,
        execution_plan.disk_checkpoint_directory.as_deref(),
    ))
}

pub fn run_staged_calibration_from_execution_input_with_observer(
    execution_input: CalibrationStagedExecutionInput,
    calendar_registry: &ExchangeCalendarRegistry,
    memory_checkpoint_store: Option<&mut CalibrationMemoryCheckpointStore>,
    observer: &mut dyn FnMut(CalibrationStagedRunObserverEvent),
) -> Result<CalibrationStagedRunReport, CalibrationStagedExecutionBridgeError> {
    let execution_plan = execution_input.build_execution_plan()?;

    Ok(run_staged_calibration_with_checkpointing_and_observer(
        &execution_plan.run_contract,
        calendar_registry,
        execution_plan.staged_run_inputs,
        memory_checkpoint_store,
        execution_plan.disk_checkpoint_directory.as_deref(),
        Some(observer),
    ))
}

pub fn evaluate_return_components_exact_candidate_from_bridge(
    candidate_generation_input: CalibrationReturnComponentsStageCandidateGenerationInput,
    activity_context_payload: Option<CalibrationReturnComponentsActivityContextPayload>,
    observed_summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    fitted_parameters: ReturnComponentsStageFittedParameters,
    stage_seed: u64,
) -> Result<
    ReturnComponentsStageExactCandidateEvaluation,
    ReturnComponentsStageExactCandidateEvaluationError,
> {
    evaluate_return_components_exact_candidate(
        candidate_generation_input.into_core(),
        activity_context_payload
            .and_then(CalibrationReturnComponentsActivityContextPayload::into_core_snapshot),
        observed_summary_statistics,
        fitted_parameters,
        stage_seed,
    )
}

fn canonical_stage_graph_for_selected_stages(
    selected_stages: &[CalibrationStage],
) -> CalibrationStageGraph {
    let selected_stage_set: std::collections::BTreeSet<CalibrationStage> =
        selected_stages.iter().copied().collect();
    let dependencies = vec![
        CalibrationStageDependency {
            stage: CalibrationStage::Hawkes,
            depends_on: CalibrationStage::Seasonality,
        },
        CalibrationStageDependency {
            stage: CalibrationStage::ReturnComponents,
            depends_on: CalibrationStage::Hawkes,
        },
        CalibrationStageDependency {
            stage: CalibrationStage::JointRefinement,
            depends_on: CalibrationStage::Hawkes,
        },
        CalibrationStageDependency {
            stage: CalibrationStage::JointRefinement,
            depends_on: CalibrationStage::ReturnComponents,
        },
        CalibrationStageDependency {
            stage: CalibrationStage::MeanReversion,
            depends_on: CalibrationStage::ReturnComponents,
        },
        CalibrationStageDependency {
            stage: CalibrationStage::JointRefinement,
            depends_on: CalibrationStage::MeanReversion,
        },
    ];

    CalibrationStageGraph::new(
        dependencies
            .into_iter()
            .filter(|dependency| selected_stage_set.contains(&dependency.stage))
            .collect(),
    )
}

fn resolve_disk_checkpoint_directory(
    execution_mode: CalibrationExecutionMode,
    runtime_resources: Option<CalibrationStagedExecutionRuntimeResources>,
) -> Result<Option<PathBuf>, CalibrationStagedExecutionBridgeError> {
    let disk_checkpoint_directory =
        runtime_resources.and_then(|resources| resources.disk_checkpoint_directory);

    match execution_mode {
        CalibrationExecutionMode::Memory => {
            if disk_checkpoint_directory.is_some() {
                return Err(
                    CalibrationStagedExecutionBridgeError::UnexpectedDiskCheckpointDirectoryForMemoryMode,
                );
            }
            Ok(None)
        }
        CalibrationExecutionMode::Disk => {
            let Some(disk_checkpoint_directory) = disk_checkpoint_directory else {
                return Err(CalibrationStagedExecutionBridgeError::MissingDiskCheckpointDirectory);
            };
            if disk_checkpoint_directory.trim().is_empty() {
                return Err(CalibrationStagedExecutionBridgeError::EmptyDiskCheckpointDirectory);
            }
            Ok(Some(PathBuf::from(disk_checkpoint_directory)))
        }
    }
}

impl CalibrationSeasonalityStageExecutionInput {
    fn try_into_core(
        self,
    ) -> Result<SeasonalityStageExecutionInput, CalibrationStagedExecutionBridgeError> {
        let timestamp_event_records = self
            .timestamp_event_records
            .into_iter()
            .enumerate()
            .map(|(record_position, record)| record.try_into_core(record_position))
            .collect::<Result<Vec<_>, _>>()?;

        let scheduled_event_overlays = self
            .scheduled_event_overlays
            .into_iter()
            .map(CalibrationScheduledEventOverlay::into_core)
            .collect();

        Ok(SeasonalityStageExecutionInput {
            timestamp_event_records,
            scheduled_event_overlays,
            settings: self
                .settings
                .map(SeasonalityStageExecutionSettings::from)
                .unwrap_or_default(),
            dataset_id_override: self.dataset_id_override,
            estimator_version_override: self.estimator_version_override,
        })
    }
}

impl CalibrationTimestampEventRecord {
    fn try_into_core(
        self,
        _record_position: usize,
    ) -> Result<TimestampEventInputRecord, CalibrationStagedExecutionBridgeError> {
        Ok(TimestampEventInputRecord {
            source_record_position: self.source_record_position,
            timestamp_utc: DateTime::<Utc>::from_timestamp_nanos(self.timestamp_utc_nanos),
        })
    }
}

impl CalibrationScheduledEventOverlay {
    fn into_core(self) -> ScheduledEventOverlayDefinition {
        ScheduledEventOverlayDefinition {
            overlay_id: self.overlay_id,
            start_timestamp_utc_nanos: self.start_timestamp_utc_nanos,
            end_timestamp_utc_nanos: self.end_timestamp_utc_nanos,
            log_adjustment: self.log_adjustment,
            precedence: self.precedence,
            enabled: self.enabled,
            source_label: self.source_label,
        }
    }
}

impl CalibrationHawkesStageCandidateInput {
    fn into_core(self) -> HawkesStageCandidateInput {
        HawkesStageCandidateInput {
            candidate_id: self.candidate_id,
            lambda_bar: self.lambda_bar,
            beta: self.beta,
            structured_alpha: self.structured_alpha,
            structured_rho: self.structured_rho,
            self_dominance_policy: self.self_dominance_policy,
            stability_numeric_tolerance: self.stability_numeric_tolerance,
            source_regime: self.source_regime.into_core(),
            directional_label_diagnostics: self.directional_label_diagnostics.into_core(),
        }
    }
}

impl CalibrationHawkesObservedSourceRegime {
    fn into_core(self) -> super::stages::hawkes::HawkesStageObservedSourceRegime {
        super::stages::hawkes::HawkesStageObservedSourceRegime {
            observation_duration_hours: self.observation_duration_hours,
            open_session_duration_hours: self.open_session_duration_hours,
            total_tick_count: self.total_tick_count,
            open_session_tick_count: self.open_session_tick_count,
            closed_session_tick_count: self.closed_session_tick_count,
            open_session_buy_tick_count: self.open_session_buy_tick_count,
            open_session_sell_tick_count: self.open_session_sell_tick_count,
            open_session_mean_interarrival_seconds: self.open_session_mean_interarrival_seconds,
            open_session_interarrival_coefficient_of_variation: self
                .open_session_interarrival_coefficient_of_variation,
            open_session_short_gap_threshold_seconds: self.open_session_short_gap_threshold_seconds,
            open_session_short_gap_ratio: self.open_session_short_gap_ratio,
            known_news_open_event_count: self.known_news_open_event_count,
            open_session_known_news_activity_share: self.open_session_known_news_activity_share,
        }
    }
}

impl CalibrationHawkesDirectionalLabelDiagnostics {
    fn into_core(self) -> super::stages::hawkes::HawkesStageDirectionalLabelDiagnostics {
        super::stages::hawkes::HawkesStageDirectionalLabelDiagnostics {
            policy_id: self.policy_id,
            first_inferred_trade_sign: self.first_inferred_trade_sign,
            provided_label_count: self.provided_label_count,
            inferred_label_count: self.inferred_label_count,
            flat_move_carry_count: self.flat_move_carry_count,
            price_up_inference_count: self.price_up_inference_count,
            price_down_inference_count: self.price_down_inference_count,
        }
    }
}

impl CalibrationReturnComponentsStageCandidateGenerationInput {
    fn into_core(self) -> ReturnComponentsStageCandidateGenerationInput {
        ReturnComponentsStageCandidateGenerationInput {
            known_news_calendar: self.known_news_calendar.into_core(),
            realized_known_news_return_samples: self
                .realized_known_news_return_samples
                .into_iter()
                .map(CalibrationReturnComponentsKnownNewsReturnSample::into_core)
                .collect(),
            observed_open_session_returns: self
                .observed_open_session_returns
                .into_iter()
                .map(CalibrationReturnComponentsObservedOpenSessionReturn::into_core)
                .collect(),
            detected_intraday_jump_candidates: self
                .detected_intraday_jump_candidates
                .into_iter()
                .map(CalibrationReturnComponentsDetectedIntradayJumpCandidate::into_core)
                .collect(),
            observed_close_open_boundary_observations: self
                .observed_close_open_boundary_observations
                .into_iter()
                .map(CalibrationReturnComponentsObservedCloseOpenBoundaryObservation::into_core)
                .collect(),
            close_open_gap_detection_policy: self.close_open_gap_detection_policy.into_core(),
            open_session_sell_share_proxy: self.open_session_sell_share_proxy,
            shared_mark_law_template: self.shared_mark_law_template.into_core(),
            objective_metric_weights: self.objective_metric_weights.into_core(),
        }
    }
}

impl CalibrationReturnComponentsKnownNewsCalendarInput {
    fn into_core(self) -> ReturnComponentsKnownNewsCalendarInput {
        ReturnComponentsKnownNewsCalendarInput {
            source_window: self.source_window.into_core(),
            scheduled_event_timestamps_utc_nanos: self.scheduled_event_timestamps_utc_nanos,
        }
    }
}

impl CalibrationReturnComponentsSignalSourceWindow {
    fn into_core(self) -> ReturnComponentsStageSignalSourceWindow {
        ReturnComponentsStageSignalSourceWindow {
            window_start_utc_nanos: self.window_start_utc_nanos,
            window_end_utc_nanos: self.window_end_utc_nanos,
        }
    }
}

impl CalibrationReturnComponentsKnownNewsReturnSample {
    fn into_core(self) -> ReturnComponentsRealizedKnownNewsReturnSample {
        ReturnComponentsRealizedKnownNewsReturnSample {
            event_timestamp_utc_nanos: self.event_timestamp_utc_nanos,
            open_window_jump_return: self.open_window_jump_return,
            closed_window_gap_return: self.closed_window_gap_return,
            post_shock_activity_lift: self.post_shock_activity_lift,
        }
    }
}

impl CalibrationReturnComponentsDetectedIntradayJumpCandidate {
    fn into_core(self) -> ReturnComponentsDetectedIntradayJumpCandidate {
        ReturnComponentsDetectedIntradayJumpCandidate {
            jump_timestamp_utc_nanos: self.jump_timestamp_utc_nanos,
            jump_return: self.jump_return,
            post_shock_activity_lift: self.post_shock_activity_lift,
        }
    }
}

impl CalibrationReturnComponentsObservedOpenSessionReturn {
    fn into_core(self) -> ReturnComponentsObservedOpenSessionReturn {
        ReturnComponentsObservedOpenSessionReturn {
            return_timestamp_utc_nanos: self.return_timestamp_utc_nanos,
            log_return: self.log_return,
            post_shock_activity_lift: self.post_shock_activity_lift,
            sell_share_proxy: self.sell_share_proxy,
        }
    }
}

impl CalibrationReturnComponentsObservedCloseOpenBoundaryObservation {
    fn into_core(self) -> ReturnComponentsObservedCloseOpenBoundaryObservation {
        ReturnComponentsObservedCloseOpenBoundaryObservation {
            close_timestamp_utc_nanos: self.close_timestamp_utc_nanos,
            close_price: self.close_price,
            open_timestamp_utc_nanos: self.open_timestamp_utc_nanos,
            open_price: self.open_price,
            post_shock_activity_lift: self.post_shock_activity_lift,
        }
    }
}

impl CalibrationReturnComponentsCloseOpenGapDetectionPolicy {
    fn into_core(self) -> ReturnComponentsCloseOpenGapDetectionPolicy {
        ReturnComponentsCloseOpenGapDetectionPolicy::new(
            self.material_gap_absolute_return_tolerance,
        )
    }
}

impl CalibrationSharedNewsMarkLawConfiguration {
    fn into_core(self) -> SharedNewsMarkLawConfiguration {
        SharedNewsMarkLawConfiguration {
            gamma_severity_marks: self.gamma_severity_marks,
            double_exponential_base_shock: self.double_exponential_base_shock,
            coupled_gap_marks: self.coupled_gap_marks,
        }
    }
}

impl CalibrationReturnComponentsObjectiveMetricWeights {
    fn into_core(self) -> ReturnComponentsStageObjectiveMetricWeights {
        ReturnComponentsStageObjectiveMetricWeights::new(
            self.continuous_distribution_fit_weight,
            self.surprise_arrival_fit_weight,
            self.jump_fit_weight,
            self.gap_fit_weight,
            self.activity_coupling_fit_weight,
            self.price_range_sanity_fit_weight,
        )
    }
}

impl CalibrationReturnComponentsActivityContextPayload {
    fn into_core_snapshot(self) -> Option<ReturnComponentsStageActivityCalibrationContextSnapshot> {
        let selected_candidate = self
            .candidate_diagnostics
            .into_iter()
            .find(|candidate| candidate.candidate_id == self.selected_candidate_id)?;
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

        Some(ReturnComponentsStageActivityCalibrationContextSnapshot {
            modeled_open_event_rate_per_hour,
            representative_interarrival_hours,
        })
    }
}

impl CalibrationMeanReversionStageExecutionInput {
    fn into_core(self) -> MeanReversionStageExecutionInput {
        MeanReversionStageExecutionInput {
            return_components_input: None,
            seasonality_stage_input: None,
            hawkes_stage_payload: None,
            return_components_stage_payload: None,
            calibration_grid_kind: self.calibration_grid_kind,
            calibration_grid_interval_hours: self.calibration_grid_interval_hours,
            source_observation_count: self.source_observation_count,
            coverage_ratio: self.coverage_ratio,
            observations: self
                .observations
                .into_iter()
                .map(CalibrationMeanReversionStageObservation::into_core)
                .collect(),
        }
    }
}

impl CalibrationMeanReversionStageObservation {
    fn into_core(self) -> MeanReversionStageObservation {
        MeanReversionStageObservation {
            observation_timestamp_utc_nanos: self.observation_timestamp_utc_nanos,
            observed_log_price: self.observed_log_price,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION,
        CalibrationHawkesDirectionalLabelDiagnostics, CalibrationHawkesObservedSourceRegime,
        CalibrationHawkesStageCandidateInput, CalibrationMeanReversionStageExecutionInput,
        CalibrationMeanReversionStageObservation,
        CalibrationReturnComponentsCloseOpenGapDetectionPolicy,
        CalibrationReturnComponentsDetectedIntradayJumpCandidate,
        CalibrationReturnComponentsKnownNewsCalendarInput,
        CalibrationReturnComponentsKnownNewsReturnSample,
        CalibrationReturnComponentsObjectiveMetricWeights,
        CalibrationReturnComponentsObservedCloseOpenBoundaryObservation,
        CalibrationReturnComponentsObservedOpenSessionReturn,
        CalibrationReturnComponentsSignalSourceWindow,
        CalibrationReturnComponentsStageCandidateGenerationInput,
        CalibrationSeasonalityStageExecutionInput, CalibrationSharedNewsMarkLawConfiguration,
        CalibrationStagedExecutionBridgeError, CalibrationStagedExecutionInput,
        CalibrationStagedExecutionRuntimeResources, CalibrationTimestampEventRecord,
    };
    use std::path::PathBuf;

    use janiform_calibration_artifacts::{
        CalibrationExecutionMode, CalibrationOptimizerSettings, CalibrationRunDataInputs,
        CalibrationRunSeedSettings, CalibrationRunSpec, CalibrationStage,
        CalibrationStageSeedOverride, CalibrationStageSolverSettings,
    };
    use janiform_model_kernels::hawkes::{
        HawkesStructuredAlphaParameterization, HawkesStructuredRhoParameterization,
        HawkesStructuredRhoSelfDominancePolicy,
    };
    use janiform_model_kernels::news::{
        CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
        GammaSeverityMarkConfiguration,
    };

    fn valid_request(execution_mode: CalibrationExecutionMode) -> CalibrationRunSpec {
        CalibrationRunSpec::try_new(
            "run-bridge",
            CalibrationRunDataInputs::try_new(
                "XNYS",
                "dataset://synthetic/trades/v1",
                Some("dataset://synthetic/options/v1".to_string()),
            )
            .expect("data inputs should validate"),
            execution_mode,
            vec![
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents,
                CalibrationStage::MeanReversion,
            ],
            CalibrationRunSeedSettings::try_new(
                17,
                vec![CalibrationStageSeedOverride {
                    stage: CalibrationStage::Seasonality,
                    seed: 101,
                }],
            )
            .expect("seed settings should validate"),
            vec![
                stage_solver_settings(CalibrationStage::Seasonality),
                stage_solver_settings(CalibrationStage::Hawkes),
                stage_solver_settings(CalibrationStage::ReturnComponents),
                stage_solver_settings(CalibrationStage::MeanReversion),
            ],
        )
        .expect("run spec should validate")
    }

    fn stage_solver_settings(stage: CalibrationStage) -> CalibrationStageSolverSettings {
        CalibrationStageSolverSettings {
            stage,
            optimizer_settings: CalibrationOptimizerSettings::try_new(
                "lbfgs", 250, 1, 1e-8, 1e-6, 10.0,
            )
            .expect("optimizer settings should validate"),
        }
    }

    fn valid_execution_input(
        execution_mode: CalibrationExecutionMode,
    ) -> CalibrationStagedExecutionInput {
        CalibrationStagedExecutionInput {
            schema_version: CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION.to_string(),
            request: valid_request(execution_mode),
            seasonality_stage_input: Some(CalibrationSeasonalityStageExecutionInput {
                timestamp_event_records: vec![
                    CalibrationTimestampEventRecord {
                        source_record_position: 0,
                        timestamp_utc_nanos: 1_770_042_600_000_000_000,
                    },
                    CalibrationTimestampEventRecord {
                        source_record_position: 1,
                        timestamp_utc_nanos: 1_770_046_200_000_000_000,
                    },
                ],
                scheduled_event_overlays: vec![],
                settings: None,
                dataset_id_override: None,
                estimator_version_override: None,
            }),
            hawkes_candidate_inputs: Some(vec![CalibrationHawkesStageCandidateInput {
                candidate_id: "candidate-001".to_string(),
                lambda_bar: 0.80,
                beta: 1.60,
                structured_alpha: HawkesStructuredAlphaParameterization::new(
                    0.35, 0.06, 0.07, 0.04,
                ),
                structured_rho: HawkesStructuredRhoParameterization::new(0.30, 0.08, 0.04, 0.05),
                self_dominance_policy: Some(HawkesStructuredRhoSelfDominancePolicy::new(1.0)),
                stability_numeric_tolerance: 0.0,
                source_regime: CalibrationHawkesObservedSourceRegime {
                    observation_duration_hours: 12.0,
                    open_session_duration_hours: 8.0,
                    total_tick_count: 95,
                    open_session_tick_count: 84,
                    closed_session_tick_count: 11,
                    open_session_buy_tick_count: 39,
                    open_session_sell_tick_count: 45,
                    open_session_mean_interarrival_seconds: 34.0,
                    open_session_interarrival_coefficient_of_variation: 1.18,
                    open_session_short_gap_threshold_seconds: 17.0,
                    open_session_short_gap_ratio: 0.43,
                    known_news_open_event_count: 2,
                    open_session_known_news_activity_share: 0.06,
                },
                directional_label_diagnostics: CalibrationHawkesDirectionalLabelDiagnostics {
                    policy_id: "tick_price_change_with_flat_move_carryover.v1".to_string(),
                    first_inferred_trade_sign: "buy".to_string(),
                    provided_label_count: 10,
                    inferred_label_count: 74,
                    flat_move_carry_count: 9,
                    price_up_inference_count: 35,
                    price_down_inference_count: 30,
                },
            }]),
            return_components_candidate_generation_input: Some(
                CalibrationReturnComponentsStageCandidateGenerationInput {
                    known_news_calendar: CalibrationReturnComponentsKnownNewsCalendarInput {
                        source_window: CalibrationReturnComponentsSignalSourceWindow {
                            window_start_utc_nanos: 1_770_037_200_000_000_000,
                            window_end_utc_nanos: 1_770_260_400_000_000_000,
                        },
                        scheduled_event_timestamps_utc_nanos: vec![
                            1_770_042_600_000_000_000,
                            1_770_131_600_000_000_000,
                        ],
                    },
                    realized_known_news_return_samples: vec![
                        CalibrationReturnComponentsKnownNewsReturnSample {
                            event_timestamp_utc_nanos: 1_770_042_600_000_000_000,
                            open_window_jump_return: 0.016,
                            closed_window_gap_return: -0.009,
                            post_shock_activity_lift: 1.20,
                        },
                    ],
                    observed_open_session_returns: vec![
                        CalibrationReturnComponentsObservedOpenSessionReturn {
                            return_timestamp_utc_nanos: 1_770_046_200_000_000_000,
                            log_return: 0.004,
                            post_shock_activity_lift: 1.00,
                            sell_share_proxy: 0.50,
                        },
                        CalibrationReturnComponentsObservedOpenSessionReturn {
                            return_timestamp_utc_nanos: 1_770_048_300_000_000_000,
                            log_return: 0.018,
                            post_shock_activity_lift: 1.35,
                            sell_share_proxy: 0.50,
                        },
                    ],
                    detected_intraday_jump_candidates: vec![
                        CalibrationReturnComponentsDetectedIntradayJumpCandidate {
                            jump_timestamp_utc_nanos: 1_770_048_300_000_000_000,
                            jump_return: 0.018,
                            post_shock_activity_lift: 1.35,
                        },
                    ],
                    observed_close_open_boundary_observations: vec![
                        CalibrationReturnComponentsObservedCloseOpenBoundaryObservation {
                            close_timestamp_utc_nanos: 1_770_066_000_000_000_000,
                            close_price: 100.0,
                            open_timestamp_utc_nanos: 1_770_215_400_000_000_000,
                            open_price: 98.70858118310978,
                            post_shock_activity_lift: 1.40,
                        },
                    ],
                    close_open_gap_detection_policy:
                        CalibrationReturnComponentsCloseOpenGapDetectionPolicy {
                            material_gap_absolute_return_tolerance: 0.005,
                        },
                    open_session_sell_share_proxy: 0.52,
                    shared_mark_law_template: CalibrationSharedNewsMarkLawConfiguration {
                        gamma_severity_marks: GammaSeverityMarkConfiguration::try_new(1.8, 0.7)
                            .expect("gamma configuration should validate"),
                        double_exponential_base_shock:
                            DoubleExponentialBaseShockConfiguration::try_new(0.45, 8.0, 10.0)
                                .expect("base shock configuration should validate"),
                        coupled_gap_marks: CoupledGapMarkConfiguration::new(0.22),
                    },
                    objective_metric_weights: CalibrationReturnComponentsObjectiveMetricWeights {
                        continuous_distribution_fit_weight: 1.0,
                        surprise_arrival_fit_weight: 1.0,
                        jump_fit_weight: 1.0,
                        gap_fit_weight: 1.0,
                        activity_coupling_fit_weight: 1.0,
                        price_range_sanity_fit_weight: 0.1,
                    },
                },
            ),
            mean_reversion_stage_input: Some(CalibrationMeanReversionStageExecutionInput {
                calibration_grid_kind: "synthetic_daily_close_grid.v1".to_string(),
                calibration_grid_interval_hours: 24.0,
                source_observation_count: 4,
                coverage_ratio: 1.0,
                observations: vec![
                    CalibrationMeanReversionStageObservation {
                        observation_timestamp_utc_nanos: 1_770_042_600_000_000_000,
                        observed_log_price: 4.60,
                    },
                    CalibrationMeanReversionStageObservation {
                        observation_timestamp_utc_nanos: 1_770_129_000_000_000_000,
                        observed_log_price: 4.58,
                    },
                    CalibrationMeanReversionStageObservation {
                        observation_timestamp_utc_nanos: 1_770_215_400_000_000_000,
                        observed_log_price: 4.57,
                    },
                    CalibrationMeanReversionStageObservation {
                        observation_timestamp_utc_nanos: 1_770_301_800_000_000_000,
                        observed_log_price: 4.59,
                    },
                ],
            }),
            runtime_resources: match execution_mode {
                CalibrationExecutionMode::Memory => None,
                CalibrationExecutionMode::Disk => {
                    Some(CalibrationStagedExecutionRuntimeResources {
                        disk_checkpoint_directory: Some("/tmp/janiform-bridge".to_string()),
                    })
                }
            },
        }
    }

    #[test]
    fn execution_input_plan_requires_explicit_stage_inputs() {
        let mut execution_input = valid_execution_input(CalibrationExecutionMode::Memory);
        execution_input.hawkes_candidate_inputs = None;

        let error = execution_input
            .build_execution_plan()
            .expect_err("missing hawkes stage input should fail");

        assert_eq!(
            error,
            CalibrationStagedExecutionBridgeError::MissingStageExecutionInput {
                stage: CalibrationStage::Hawkes,
            }
        );
    }

    #[test]
    fn execution_input_plan_requires_disk_directory_for_disk_mode() {
        let mut execution_input = valid_execution_input(CalibrationExecutionMode::Disk);
        execution_input.runtime_resources = None;

        let error = execution_input
            .build_execution_plan()
            .expect_err("missing disk runtime resources should fail");

        assert_eq!(
            error,
            CalibrationStagedExecutionBridgeError::MissingDiskCheckpointDirectory
        );
    }

    #[test]
    fn execution_input_plan_builds_run_contract_and_runtime_resources() {
        let execution_input = valid_execution_input(CalibrationExecutionMode::Disk);

        let plan = execution_input
            .build_execution_plan()
            .expect("execution input should build");

        assert_eq!(
            plan.run_contract.run_spec.execution_mode,
            CalibrationExecutionMode::Disk
        );
        assert_eq!(
            plan.run_contract.execution_stage_order,
            vec![
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents,
                CalibrationStage::MeanReversion,
            ]
        );
        assert_eq!(
            plan.disk_checkpoint_directory,
            Some(PathBuf::from("/tmp/janiform-bridge"))
        );
    }
}
