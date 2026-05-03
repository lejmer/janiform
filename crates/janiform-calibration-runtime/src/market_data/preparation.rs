use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono::{DateTime, Utc};
use janiform_calibration_artifacts::{
    CalibrationExecutionMode, CalibrationRunDataInputs, CalibrationRunSeedSettings,
    CalibrationRunSpec, CalibrationStage, CalibrationStageSolverSettings,
};
use janiform_engine_pricing::{PricingIntervalSchedulePolicy, VanillaOptionPayoffType};
use janiform_foundations::identifiers::sanitize_ascii_identifier_component;
use janiform_foundations::numeric::coverage_ratio;
use janiform_foundations::time::{NANOS_PER_HOUR_F64, duration_nanos_to_hours};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::solver_settings::{
    DATA_DERIVED_SOLVER_SETTINGS_POLICY_ID, has_missing_stage_solver_settings,
    resolve_solver_settings_by_stage,
};
use crate::stages::mean_reversion::mean_reversion_full_model_required_source_observation_count;
use crate::{
    CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION, CalibrationJointRefinementCarryInput,
    CalibrationJointRefinementCurrentStateEstimationInput,
    CalibrationJointRefinementOptionPricingInput, CalibrationJointRefinementOptionSnapshotNode,
    CalibrationMeanReversionStageExecutionInput, CalibrationMeanReversionStageObservation,
    CalibrationStagedExecutionInput, CalibrationStagedExecutionRuntimeResources,
    MarketDataInputFileReference, MarketDataKnownNewsColumnMapping,
    MarketDataKnownNewsLoadingInput, MarketDataLoadingOutput, MarketDataLoadingRequest,
    MarketDataNormalizationPolicy, MarketDataNormalizedTickOutput,
    MarketDataOptionSurfaceColumnMapping, MarketDataOptionSurfaceLoadingInput,
    MarketDataSessionConfiguration, MarketDataSessionResolutionPayload,
    MarketDataTickDataColumnMapping, MarketDataTranslationPreprocessingSettings,
    normalize_market_data_source, prepare_market_data_loading_from_phases,
    resolve_market_data_session_configuration,
};

const PREPARED_MARKET_DATA_DERIVATION_POLICY_ID: &str =
    "market_data_loading_and_translation.rust.v1";
const KNOWN_NEWS_WINDOW_FILTER_POLICY_ID: &str = "known_news_outside_tick_window_are_filtered.v1";
const KNOWN_NEWS_OVERLAY_POLICY_ID: &str =
    "known_news_overlays_are_point_anchors_with_zero_log_adjustment.v1";
const TRADE_LABEL_DERIVATION_POLICY_ID: &str =
    "unsigned_trade_direction_labels_follow_tick_price_change_with_flat_move_carryover.v1";
const HAWKES_SEARCH_POLICY_ID: &str = "hawkes_search_family_uses_unsigned_open_session_activity_targets_and_model_implied_fit_terms.v1";
const RETURN_COMPONENTS_JUMP_POLICY_ID: &str = "surprise_jump_candidates_use_data_driven_material_open_session_return_thresholds_outside_known_news_windows.v1";
const RETURN_COMPONENTS_BOUNDARY_POLICY_ID: &str =
    "close_open_boundaries_are_resolved_from_calendar_state_transitions.v1";
const RETURN_COMPONENTS_CONTINUOUS_FIT_POLICY_ID: &str =
    "continuous_nig_fit_uses_filtered_observed_open_session_returns_and_stage_search.v1";
const DEFAULT_SEASONALITY_SESSION_FILTER_POLICY_ID: &str = "seasonality.session_filter=open_only";
const DEFAULT_SEASONALITY_SPARSE_THRESHOLD_POLICY_ID: &str =
    "seasonality.sparse_hour_event_threshold=1";
const DEFAULT_SEASONALITY_INTRADAY_MINUTES_POLICY_ID: &str =
    "seasonality.intraday.minutes_per_bin=1";
const DEFAULT_SEASONALITY_INTRADAY_KERNEL_POLICY_ID: &str =
    "seasonality.intraday.smoothing_kernel=gaussian";
const DEFAULT_SEASONALITY_INTRADAY_BANDWIDTH_POLICY_ID: &str =
    "seasonality.intraday.smoothing_bandwidth_bins=3.0";
const DEFAULT_SEASONALITY_DAY_OF_WEEK_POLICY_ID: &str =
    "seasonality.day_of_week.minimum_bucket_events=1";
const DEFAULT_SEASONALITY_DAY_OF_YEAR_POLICY_ID: &str =
    "seasonality.day_of_year.minimum_bucket_events=1";
const DEFAULT_KNOWN_NEWS_OVERLAY_POLICY_ID: &str = "known_news_overlay.log_adjustment=0.0";
const DEFAULT_CLOSE_OPEN_GAP_POLICY_ID: &str = "close_open_gap_detection_policy.material_gap_absolute_return_tolerance=3x_median_abs_intraday_return";
const DEFAULT_INTRADAY_JUMP_POLICY_ID: &str =
    "intraday_jump_detection.material_threshold=4x_median_abs_open_return";
const MEAN_REVERSION_OPEN_SESSION_GRID_KIND: &str =
    "market_data_open_session_log_price_interval_grid.v1";
const MEAN_REVERSION_OPEN_SESSION_ALIGNMENT_POLICY: &str =
    "last_observed_open_session_log_price_per_complete_utc_interval.v1";
const MEAN_REVERSION_BOUNDARY_INTERVAL_DERIVATION_POLICY: &str =
    "median_boundary_open_spacing_hours_from_retained_observations.v1";
const MEAN_REVERSION_SESSION_DURATION_INTERVAL_DERIVATION_POLICY: &str =
    "median_observed_session_duration_hours_from_retained_open_session_ticks.v1";
const MEAN_REVERSION_SOURCE_WINDOW_INTERVAL_DERIVATION_POLICY: &str =
    "source_window_interval_hours_from_identifiable_open_session_coverage.v1";
const MEAN_REVERSION_INTERVAL_OVERRIDE_POLICY: &str =
    "explicit_mean_reversion_grid_interval_hours.v1";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationMarketDataWorkflowScope {
    SeasonalityOnly,
    FullWithoutOptionSurface,
    FullWithOptionSurface,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationMarketDataColumnMapping {
    pub tick_data: MarketDataTickDataColumnMapping,
    #[serde(default)]
    pub option_surface: Option<MarketDataOptionSurfaceColumnMapping>,
    #[serde(default)]
    pub known_news: Option<MarketDataKnownNewsColumnMapping>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationMarketDataOutputPreferences {
    pub execution_mode: CalibrationExecutionMode,
    #[serde(default)]
    pub disk_checkpoint_directory: Option<String>,
    #[serde(rename = "emit_input_provenance_report")]
    pub emit_input_provenance_report: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationMarketDataKnownNewsInput {
    pub calendar_source: MarketDataInputFileReference,
    #[serde(default)]
    pub calendar_timezone: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationMarketDataMeanReversionConfiguration {
    #[serde(default)]
    pub calibration_grid_interval_hours: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationMarketDataCarryInput {
    #[serde(default)]
    pub discount_rate: f64,
    #[serde(default)]
    pub carry_rate: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationMarketDataPreparationRequest {
    #[serde(rename = "schema_version")]
    pub schema_version: String,
    pub run_id: String,
    pub tick_data_source: MarketDataInputFileReference,
    pub session_configuration: MarketDataSessionConfiguration,
    pub column_mapping: CalibrationMarketDataColumnMapping,
    pub workflow_scope: CalibrationMarketDataWorkflowScope,
    pub normalization_policy: MarketDataNormalizationPolicy,
    pub output_preferences: CalibrationMarketDataOutputPreferences,
    #[serde(default)]
    pub optimizer_settings_by_stage: Vec<CalibrationStageSolverSettings>,
    #[serde(default)]
    pub preprocessing_settings: Option<MarketDataTranslationPreprocessingSettings>,
    #[serde(default)]
    pub option_surface_source: Option<MarketDataInputFileReference>,
    #[serde(default)]
    pub option_surface_valuation_time_utc: Option<String>,
    #[serde(default)]
    pub carry: Option<CalibrationMarketDataCarryInput>,
    #[serde(default)]
    pub pricing_interval_schedule_policy: PricingIntervalSchedulePolicy,
    #[serde(default)]
    pub dynamics_search_restart_candidate_budget: Option<usize>,
    #[serde(default)]
    pub joint_refinement_option_refinement_candidate_count_override: Option<usize>,
    #[serde(default)]
    pub joint_refinement_audit_option_pricing_enabled: bool,
    #[serde(default)]
    pub known_news_inputs: Option<CalibrationMarketDataKnownNewsInput>,
    #[serde(default)]
    pub mean_reversion: Option<CalibrationMarketDataMeanReversionConfiguration>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CalibrationMarketDataPreparationDiagnostics {
    pub workflow_scope: CalibrationMarketDataWorkflowScope,
    pub selected_stages: Vec<CalibrationStage>,
    pub executed_preprocessing_steps: Vec<String>,
    pub tick_record_count: usize,
    pub option_surface_record_count: usize,
    pub known_news_event_count: usize,
    pub known_news_events_outside_source_window_count: usize,
    pub session_open_tick_count: usize,
    pub session_closed_tick_count: usize,
    pub known_news_open_event_count: usize,
    pub known_news_closed_event_count: usize,
    pub seasonality_overlay_count: usize,
    pub hawkes_candidate_count: usize,
    pub known_news_return_sample_count: usize,
    pub intraday_jump_candidate_count: usize,
    pub close_open_boundary_observation_count: usize,
    pub material_close_open_gap_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CalibrationMarketDataPreparationProvenance {
    pub tick_data_summary: Value,
    pub option_surface_summary: Option<Value>,
    pub known_news_summary: Option<Value>,
    pub normalization_policy: Value,
    pub workflow_scope: CalibrationMarketDataWorkflowScope,
    pub selected_stages: Vec<CalibrationStage>,
    pub session_definition_path: Option<String>,
    pub derived_trade_events_data_reference: String,
    pub derived_option_surface_data_reference: Option<String>,
    pub derivation_policies: Vec<String>,
    pub deterministic_defaults_applied: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedMarketDataCalibrationPlan {
    pub execution_input: CalibrationStagedExecutionInput,
    pub diagnostics: CalibrationMarketDataPreparationDiagnostics,
    pub provenance: CalibrationMarketDataPreparationProvenance,
    pub session_resolution: MarketDataSessionResolutionPayload,
    pub fit_warning: Option<String>,
    pub reference_price: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationMarketDataPreparationError {
    Message { message: String },
}

impl Display for CalibrationMarketDataPreparationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Message { message } => formatter.write_str(message),
        }
    }
}

impl Error for CalibrationMarketDataPreparationError {}

impl From<String> for CalibrationMarketDataPreparationError {
    fn from(message: String) -> Self {
        Self::Message { message }
    }
}

fn validate_joint_refinement_option_refinement_candidate_count_override(
    request: &CalibrationMarketDataPreparationRequest,
) -> Result<(), CalibrationMarketDataPreparationError> {
    if request.joint_refinement_option_refinement_candidate_count_override == Some(0) {
        return Err(CalibrationMarketDataPreparationError::from(
            "joint_refinement_option_refinement_candidate_count_override must be greater than zero when provided"
                .to_string(),
        ));
    }

    Ok(())
}

fn validate_dynamics_search_restart_candidate_budget(
    request: &CalibrationMarketDataPreparationRequest,
) -> Result<(), CalibrationMarketDataPreparationError> {
    if request.dynamics_search_restart_candidate_budget == Some(0) {
        return Err(CalibrationMarketDataPreparationError::from(
            "dynamics_search_restart_candidate_budget must be greater than zero when provided"
                .to_string(),
        ));
    }

    Ok(())
}

fn validate_joint_refinement_audit_option_pricing(
    request: &CalibrationMarketDataPreparationRequest,
) -> Result<(), CalibrationMarketDataPreparationError> {
    if request.joint_refinement_audit_option_pricing_enabled
        && request.option_surface_source.is_none()
    {
        return Err(CalibrationMarketDataPreparationError::from(
            "joint_refinement_audit_option_pricing_enabled requires option_surface_source"
                .to_string(),
        ));
    }

    Ok(())
}

struct LoadedMarketDataPhases {
    normalized_tick_output: MarketDataNormalizedTickOutput,
    loading_output: MarketDataLoadingOutput,
}

struct MeanReversionGridObservations {
    source_observation_count: usize,
    coverage_ratio: f64,
    observations: Vec<CalibrationMeanReversionStageObservation>,
}

struct ResolvedMeanReversionGridObservations {
    calibration_grid_interval_hours: f64,
    interval_policy: &'static str,
    grid_observations: MeanReversionGridObservations,
}

pub fn prepare_market_data_calibration_plan(
    request: CalibrationMarketDataPreparationRequest,
    cpu_count: Option<usize>,
) -> Result<PreparedMarketDataCalibrationPlan, CalibrationMarketDataPreparationError> {
    validate_dynamics_search_restart_candidate_budget(&request)?;
    validate_joint_refinement_option_refinement_candidate_count_override(&request)?;
    validate_joint_refinement_audit_option_pricing(&request)?;

    let selected_stages = selected_stages_for_scope(request.workflow_scope);
    let (loading_request, trade_events_data_reference, option_surface_data_reference) =
        build_loading_request(&request, &selected_stages)?;
    let loaded_market_data = load_market_data_from_phases(loading_request)?;

    prepared_market_data_plan_from_loading_output(
        request,
        cpu_count,
        selected_stages,
        trade_events_data_reference,
        option_surface_data_reference,
        loaded_market_data.normalized_tick_output,
        loaded_market_data.loading_output,
    )
}

pub fn prepare_market_data_calibration_plan_from_phases(
    request: CalibrationMarketDataPreparationRequest,
    normalized_tick_output: &MarketDataNormalizedTickOutput,
    session_resolution: MarketDataSessionResolutionPayload,
    cpu_count: Option<usize>,
) -> Result<PreparedMarketDataCalibrationPlan, CalibrationMarketDataPreparationError> {
    validate_dynamics_search_restart_candidate_budget(&request)?;
    validate_joint_refinement_option_refinement_candidate_count_override(&request)?;
    validate_joint_refinement_audit_option_pricing(&request)?;

    let selected_stages = selected_stages_for_scope(request.workflow_scope);
    let (loading_request, trade_events_data_reference, option_surface_data_reference) =
        build_loading_request(&request, &selected_stages)?;

    let loading_output = prepare_market_data_loading_from_phases(
        loading_request,
        normalized_tick_output,
        session_resolution,
    )
    .map_err(|source| {
        CalibrationMarketDataPreparationError::from(format!(
            "failed to prepare market-data calibration inputs: {source}"
        ))
    })?;

    prepared_market_data_plan_from_loading_output(
        request,
        cpu_count,
        selected_stages,
        trade_events_data_reference,
        option_surface_data_reference,
        normalized_tick_output.clone(),
        loading_output,
    )
}

fn selected_stages_for_scope(
    workflow_scope: CalibrationMarketDataWorkflowScope,
) -> Vec<CalibrationStage> {
    match workflow_scope {
        CalibrationMarketDataWorkflowScope::SeasonalityOnly => {
            vec![CalibrationStage::Seasonality]
        }
        CalibrationMarketDataWorkflowScope::FullWithoutOptionSurface
        | CalibrationMarketDataWorkflowScope::FullWithOptionSurface => vec![
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            CalibrationStage::ReturnComponents,
            CalibrationStage::MeanReversion,
            CalibrationStage::JointRefinement,
        ],
    }
}

fn build_dataset_reference(run_id: &str, dataset_role: &str, provenance_label: &str) -> String {
    format!(
        "market_data://{}/{}/{}",
        sanitize_ascii_identifier_component(run_id).unwrap_or_else(|| "unknown".to_string()),
        dataset_role,
        sanitize_ascii_identifier_component(provenance_label)
            .unwrap_or_else(|| "unknown".to_string())
    )
}

fn stable_seed_from_run_id(run_id: &str) -> u64 {
    run_id
        .chars()
        .enumerate()
        .map(|(character_index, character)| {
            let position = u64::try_from(character_index + 1).unwrap_or(u64::MAX);
            position.saturating_mul(u64::from(u32::from(character)))
        })
        .sum()
}

fn executed_preprocessing_steps(
    workflow_scope: CalibrationMarketDataWorkflowScope,
    option_surface_present: bool,
    known_news_present: bool,
) -> Vec<String> {
    let mut preprocessing_steps = vec![
        "source_loading".to_string(),
        "column_mapping_validation".to_string(),
        "timestamp_timezone_normalization".to_string(),
        "duplicate_record_resolution".to_string(),
        "timestamp_sorting".to_string(),
        "session_state_resolution".to_string(),
        "workflow_scope_selection".to_string(),
    ];

    if known_news_present {
        preprocessing_steps.push("known_news_window_filtering".to_string());
        preprocessing_steps.push("known_news_overlay_derivation".to_string());
    }

    if !matches!(
        workflow_scope,
        CalibrationMarketDataWorkflowScope::SeasonalityOnly
    ) {
        preprocessing_steps.push("hawkes_candidate_generation".to_string());
        preprocessing_steps.push("return_components_candidate_generation".to_string());
    }

    if option_surface_present {
        preprocessing_steps.push("option_surface_quote_normalization".to_string());
    }

    preprocessing_steps
}

fn deterministic_defaults_applied(
    request: &CalibrationMarketDataPreparationRequest,
) -> Vec<String> {
    let mut applied_defaults = Vec::new();

    if request
        .preprocessing_settings
        .as_ref()
        .and_then(|settings| settings.seasonality.as_ref())
        .is_none()
    {
        applied_defaults.push(DEFAULT_SEASONALITY_SESSION_FILTER_POLICY_ID.to_string());
        applied_defaults.push(DEFAULT_SEASONALITY_SPARSE_THRESHOLD_POLICY_ID.to_string());
        applied_defaults.push(DEFAULT_SEASONALITY_INTRADAY_MINUTES_POLICY_ID.to_string());
        applied_defaults.push(DEFAULT_SEASONALITY_INTRADAY_KERNEL_POLICY_ID.to_string());
        applied_defaults.push(DEFAULT_SEASONALITY_INTRADAY_BANDWIDTH_POLICY_ID.to_string());
        applied_defaults.push(DEFAULT_SEASONALITY_DAY_OF_WEEK_POLICY_ID.to_string());
        applied_defaults.push(DEFAULT_SEASONALITY_DAY_OF_YEAR_POLICY_ID.to_string());
        applied_defaults.push(DEFAULT_KNOWN_NEWS_OVERLAY_POLICY_ID.to_string());
    }

    if request
        .preprocessing_settings
        .as_ref()
        .and_then(|settings| settings.return_components.as_ref())
        .is_none()
    {
        applied_defaults.push(DEFAULT_CLOSE_OPEN_GAP_POLICY_ID.to_string());
        applied_defaults.push(DEFAULT_INTRADAY_JUMP_POLICY_ID.to_string());
    }

    applied_defaults
}

fn build_loading_request(
    request: &CalibrationMarketDataPreparationRequest,
    selected_stages: &[CalibrationStage],
) -> Result<(MarketDataLoadingRequest, String, Option<String>), CalibrationMarketDataPreparationError>
{
    let trade_events_data_reference = build_dataset_reference(
        request.run_id.as_str(),
        "ticks",
        request.tick_data_source.provenance_label.as_str(),
    );

    let option_surface_data_reference = request.option_surface_source.as_ref().map(|source| {
        build_dataset_reference(
            request.run_id.as_str(),
            "options",
            source.provenance_label.as_str(),
        )
    });

    let option_surface_input = match (
        request.option_surface_source.as_ref(),
        request.column_mapping.option_surface.clone(),
    ) {
        (Some(source), Some(column_mapping)) => Some(MarketDataOptionSurfaceLoadingInput {
            source: source.clone(),
            column_mapping,
        }),
        (Some(source), None) => Some(MarketDataOptionSurfaceLoadingInput {
            source: source.clone(),
            column_mapping: MarketDataOptionSurfaceColumnMapping {
                quote_timestamp_column: None,
                expiry_timestamp_column: None,
                strike_column: None,
                option_type_column: None,
                mid_price_column: None,
                bid_column: None,
                ask_column: None,
                implied_volatility_column: None,
            },
        }),
        (None, None) => None,
        (None, Some(_)) => {
            return Err(CalibrationMarketDataPreparationError::from(
                "option_surface_source is required when column_mapping.option_surface is provided"
                    .to_string(),
            ));
        }
    };

    let known_news_input = match (
        request.known_news_inputs.as_ref(),
        request.column_mapping.known_news.clone(),
    ) {
        (Some(input), Some(column_mapping)) => Some(MarketDataKnownNewsLoadingInput {
            calendar_source: input.calendar_source.clone(),
            column_mapping,
            calendar_timezone: input.calendar_timezone.clone(),
        }),
        (None, None) => None,
        (Some(_), None) => {
            return Err(CalibrationMarketDataPreparationError::from(
                "column_mapping.known_news is required when known_news_inputs is provided"
                    .to_string(),
            ));
        }
        (None, Some(_)) => {
            return Err(CalibrationMarketDataPreparationError::from(
                "known_news_inputs is required when column_mapping.known_news is provided"
                    .to_string(),
            ));
        }
    };

    Ok((
        MarketDataLoadingRequest {
            session_configuration: request.session_configuration.clone(),
            trade_events_data_reference: trade_events_data_reference.clone(),
            tick_data_source: request.tick_data_source.clone(),
            tick_data_column_mapping: request.column_mapping.tick_data.clone(),
            normalization_policy: request.normalization_policy.clone(),
            preprocessing_settings: request.preprocessing_settings.clone(),
            option_surface_input,
            known_news_input,
            require_hawkes: selected_stages.contains(&CalibrationStage::Hawkes),
            require_return_components: selected_stages
                .contains(&CalibrationStage::ReturnComponents),
        },
        trade_events_data_reference,
        option_surface_data_reference,
    ))
}

fn runtime_resources_from_output_preferences(
    output_preferences: &CalibrationMarketDataOutputPreferences,
) -> Option<CalibrationStagedExecutionRuntimeResources> {
    if output_preferences.execution_mode != CalibrationExecutionMode::Disk {
        return None;
    }

    Some(CalibrationStagedExecutionRuntimeResources {
        disk_checkpoint_directory: output_preferences.disk_checkpoint_directory.clone(),
    })
}

fn mean_reversion_stage_input_from_request(
    request: &CalibrationMarketDataPreparationRequest,
    selected_stages: &[CalibrationStage],
    loading_output: &MarketDataLoadingOutput,
) -> Result<
    Option<CalibrationMeanReversionStageExecutionInput>,
    CalibrationMarketDataPreparationError,
> {
    if !selected_stages.contains(&CalibrationStage::MeanReversion) {
        return Ok(None);
    }

    let return_components_input = loading_output
        .preprocessing_output
        .return_components_candidate_generation_input
        .as_ref();

    let default_mean_reversion_configuration = CalibrationMarketDataMeanReversionConfiguration {
        calibration_grid_interval_hours: None,
    };
    let mean_reversion_configuration = request
        .mean_reversion
        .as_ref()
        .unwrap_or(&default_mean_reversion_configuration);

    let boundary_observations = return_components_input
        .map(|input| input.observed_close_open_boundary_observations.as_slice())
        .unwrap_or(&[]);
    let boundary_open_observations = mean_reversion_stage_observations_relative_to_reference_price(
        boundary_observations
            .iter()
            .map(
                |boundary_observation| CalibrationMeanReversionStageObservation {
                    observation_timestamp_utc_nanos: boundary_observation.open_timestamp_utc_nanos,
                    observed_log_price: boundary_observation.open_price.ln(),
                },
            )
            .collect::<Vec<_>>()
            .as_slice(),
        loading_output.reference_price,
    );
    let open_session_price_observations =
        mean_reversion_stage_observations_relative_to_reference_price(
            loading_output
                .preprocessing_output
                .mean_reversion_open_session_price_observations
                .as_deref()
                .unwrap_or(&[]),
            loading_output.reference_price,
        );
    let observed_session_duration_hours = loading_output
        .preprocessing_output
        .mean_reversion_observed_session_duration_hours
        .as_deref()
        .unwrap_or(&[]);

    let Some(resolved_grid_observations) = resolve_mean_reversion_grid_observations(
        mean_reversion_configuration.calibration_grid_interval_hours,
        boundary_open_observations.as_slice(),
        open_session_price_observations.as_slice(),
        observed_session_duration_hours,
    ) else {
        return Err(CalibrationMarketDataPreparationError::from(
            "mean-reversion calibration grid interval could not be derived from retained boundary spacing or observed session durations"
                .to_string(),
        ));
    };

    if open_session_price_observations.is_empty() {
        return Ok(Some(CalibrationMeanReversionStageExecutionInput {
            calibration_grid_kind: format!(
                "{}:{}:{}",
                MEAN_REVERSION_OPEN_SESSION_GRID_KIND,
                MEAN_REVERSION_OPEN_SESSION_ALIGNMENT_POLICY,
                resolved_grid_observations.interval_policy
            ),
            calibration_grid_interval_hours: resolved_grid_observations
                .calibration_grid_interval_hours,
            source_observation_count: 0,
            coverage_ratio: 0.0,
            observations: Vec::new(),
        }));
    }

    Ok(Some(CalibrationMeanReversionStageExecutionInput {
        calibration_grid_kind: format!(
            "{}:{}:{}",
            MEAN_REVERSION_OPEN_SESSION_GRID_KIND,
            MEAN_REVERSION_OPEN_SESSION_ALIGNMENT_POLICY,
            resolved_grid_observations.interval_policy
        ),
        calibration_grid_interval_hours: resolved_grid_observations.calibration_grid_interval_hours,
        source_observation_count: resolved_grid_observations
            .grid_observations
            .source_observation_count,
        coverage_ratio: resolved_grid_observations.grid_observations.coverage_ratio,
        observations: resolved_grid_observations.grid_observations.observations,
    }))
}

fn joint_refinement_option_pricing_input_from_request(
    request: &CalibrationMarketDataPreparationRequest,
    loading_output: &MarketDataLoadingOutput,
    normalized_tick_output: &MarketDataNormalizedTickOutput,
) -> Result<
    Option<CalibrationJointRefinementOptionPricingInput>,
    CalibrationMarketDataPreparationError,
> {
    if request.workflow_scope != CalibrationMarketDataWorkflowScope::FullWithOptionSurface {
        return Ok(None);
    }

    let option_snapshot_nodes =
        selected_option_snapshot_nodes_from_loading_output(request, loading_output)?;

    let valuation_time_utc_nanos = option_snapshot_nodes
        .first()
        .map(|node| node.quote_timestamp_utc_nanos)
        .ok_or_else(|| {
            CalibrationMarketDataPreparationError::from(
                "joint option calibration requires a non-empty option snapshot".to_string(),
            )
        })?;

    let reference_price = loading_output
        .reference_price
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| {
            CalibrationMarketDataPreparationError::from(
                "joint option calibration requires a strictly positive reference price from tick normalization"
                    .to_string(),
            )
        })?;

    let current_state_estimation_input = joint_option_current_state_estimation_input(
        valuation_time_utc_nanos,
        reference_price,
        normalized_tick_output,
    )?;

    let carry_input = match request.carry.clone() {
        Some(carry) => CalibrationJointRefinementCarryInput {
            discount_rate: carry.discount_rate,
            carry_rate: carry.carry_rate,
        },
        None => CalibrationJointRefinementCarryInput {
            discount_rate: 0.0,
            carry_rate: 0.0,
        },
    };

    Ok(Some(CalibrationJointRefinementOptionPricingInput {
        valuation_time_utc_nanos,
        pricing_interval_schedule_policy: request.pricing_interval_schedule_policy.clone(),
        carry_input,
        current_state_estimation_input,
        option_snapshot_nodes,
    }))
}

fn selected_option_snapshot_nodes_from_loading_output(
    request: &CalibrationMarketDataPreparationRequest,
    loading_output: &MarketDataLoadingOutput,
) -> Result<Vec<CalibrationJointRefinementOptionSnapshotNode>, CalibrationMarketDataPreparationError>
{
    let option_surface_records =
        loading_output
            .option_surface_records
            .as_ref()
            .ok_or_else(|| {
                CalibrationMarketDataPreparationError::from(
                    "joint option calibration requires normalized option surface records"
                        .to_string(),
                )
            })?;

    let requested_valuation_time_utc = request
        .option_surface_valuation_time_utc
        .as_deref()
        .map(parse_timestamp_utc)
        .transpose()?;

    let requested_valuation_time_utc_nanos =
        requested_valuation_time_utc.and_then(|timestamp| timestamp.timestamp_nanos_opt());

    let selected_quote_timestamp_utc_nanos = if let Some(requested_valuation_time_utc_nanos) =
        requested_valuation_time_utc_nanos
    {
        if option_surface_records
            .iter()
            .any(|record| record.quote_timestamp_utc_nanos == requested_valuation_time_utc_nanos)
        {
            requested_valuation_time_utc_nanos
        } else {
            return Err(CalibrationMarketDataPreparationError::from(format!(
                "joint option calibration could not find option quotes for valuation timestamp {}",
                request
                    .option_surface_valuation_time_utc
                    .clone()
                    .unwrap_or_else(|| requested_valuation_time_utc_nanos.to_string())
            )));
        }
    } else {
        let unique_quote_timestamps = option_surface_records
            .iter()
            .map(|record| record.quote_timestamp_utc_nanos)
            .collect::<BTreeSet<_>>();
        match unique_quote_timestamps.len() {
            1 => *unique_quote_timestamps.first().ok_or_else(|| {
                CalibrationMarketDataPreparationError::from(
                    "joint option calibration requires option quotes".to_string(),
                )
            })?,
            0 => {
                return Err(CalibrationMarketDataPreparationError::from(
                    "joint option calibration requires option quotes but none were retained"
                        .to_string(),
                ));
            }
            _ => {
                return Err(CalibrationMarketDataPreparationError::from(
                    "joint option calibration requires option_surface_valuation_time_utc when the option file contains multiple quote timestamps"
                        .to_string(),
                ));
            }
        }
    };

    let mut selected_nodes = Vec::new();
    for record in option_surface_records {
        if record.quote_timestamp_utc_nanos != selected_quote_timestamp_utc_nanos {
            continue;
        }

        let bid_price = record.bid_price.ok_or_else(|| {
            CalibrationMarketDataPreparationError::from(format!(
                "joint option calibration requires bid prices for option row {}",
                record.source_row_number
            ))
        })?;
        let ask_price = record.ask_price.ok_or_else(|| {
            CalibrationMarketDataPreparationError::from(format!(
                "joint option calibration requires ask prices for option row {}",
                record.source_row_number
            ))
        })?;
        if !bid_price.is_finite() || !ask_price.is_finite() || ask_price <= bid_price {
            return Err(CalibrationMarketDataPreparationError::from(format!(
                "joint option calibration requires strictly positive bid/ask width for option row {}",
                record.source_row_number
            )));
        }

        let observed_mid_price = match record.mid_price {
            Some(mid_price) => mid_price,
            None => 0.5 * (bid_price + ask_price),
        };
        if !observed_mid_price.is_finite() {
            return Err(CalibrationMarketDataPreparationError::from(format!(
                "joint option calibration requires finite observed mid prices for option row {}",
                record.source_row_number
            )));
        }

        let payoff_type = match record.option_type.as_str() {
            "call" => VanillaOptionPayoffType::Call,
            "put" => VanillaOptionPayoffType::Put,
            other => {
                return Err(CalibrationMarketDataPreparationError::from(format!(
                    "joint option calibration encountered unsupported option type `{other}` for option row {}",
                    record.source_row_number
                )));
            }
        };

        selected_nodes.push(CalibrationJointRefinementOptionSnapshotNode {
            quote_timestamp_utc_nanos: record.quote_timestamp_utc_nanos,
            expiry_timestamp_utc_nanos: record.expiry_timestamp_utc_nanos,
            strike: record.strike,
            payoff_type,
            bid_price,
            ask_price,
            observed_mid_price,
        });
    }

    if selected_nodes.is_empty() {
        return Err(CalibrationMarketDataPreparationError::from(
            "joint option calibration requires a non-empty selected option snapshot".to_string(),
        ));
    }

    Ok(selected_nodes)
}

fn joint_option_current_state_estimation_input(
    valuation_time_utc_nanos: i64,
    reference_price: f64,
    normalized_tick_output: &MarketDataNormalizedTickOutput,
) -> Result<
    CalibrationJointRefinementCurrentStateEstimationInput,
    CalibrationMarketDataPreparationError,
> {
    let mut observation_timestamp_utc_nanos = Vec::new();
    let mut observation_trade_prices = Vec::new();
    let mut observation_trade_sides = Vec::new();

    for tick_record in &normalized_tick_output.tick_records {
        if tick_record.timestamp_utc_nanos > valuation_time_utc_nanos {
            break;
        }

        let Some(price) = tick_record.price else {
            continue;
        };
        let Some(trade_sign) = tick_record.trade_sign.as_deref() else {
            continue;
        };

        let trade_side = match trade_sign {
            "buy" => "buy",
            "sell" => "sell",
            other => {
                return Err(CalibrationMarketDataPreparationError::from(format!(
                    "joint option calibration encountered unsupported trade sign `{other}` in retained tick data"
                )));
            }
        };

        observation_timestamp_utc_nanos.push(tick_record.timestamp_utc_nanos);
        observation_trade_prices.push(price);
        observation_trade_sides.push(trade_side.to_string());
    }

    let observed_spot = observation_trade_prices
        .last()
        .copied()
        .ok_or_else(|| {
            CalibrationMarketDataPreparationError::from(
                "joint option calibration requires retained priced tick observations at or before the selected valuation timestamp"
                    .to_string(),
            )
        })?;
    let observed_log_price = (observed_spot / reference_price).ln();

    Ok(CalibrationJointRefinementCurrentStateEstimationInput {
        state_time_utc_nanos: valuation_time_utc_nanos,
        observed_spot,
        observed_log_price,
        observation_timestamp_utc_nanos,
        observation_trade_prices,
        observation_trade_sides,
    })
}

fn mean_reversion_stage_observations_relative_to_reference_price(
    observations: &[CalibrationMeanReversionStageObservation],
    reference_price: Option<f64>,
) -> Vec<CalibrationMeanReversionStageObservation> {
    let Some(reference_price) = reference_price
        .filter(|reference_price| reference_price.is_finite() && *reference_price > 0.0)
    else {
        return observations.to_vec();
    };

    let reference_log_price = reference_price.ln();

    observations
        .iter()
        .map(|observation| CalibrationMeanReversionStageObservation {
            observation_timestamp_utc_nanos: observation.observation_timestamp_utc_nanos,
            observed_log_price: observation.observed_log_price - reference_log_price,
        })
        .collect()
}

fn mean_reversion_grid_observations_from_open_session_prices(
    open_session_price_observations: &[CalibrationMeanReversionStageObservation],
    calibration_grid_interval_hours: f64,
) -> MeanReversionGridObservations {
    let Some(interval_nanos) = interval_hours_to_nanos(calibration_grid_interval_hours) else {
        return MeanReversionGridObservations {
            source_observation_count: 0,
            coverage_ratio: 0.0,
            observations: Vec::new(),
        };
    };

    let Some(first_observation) = open_session_price_observations.first() else {
        return MeanReversionGridObservations {
            source_observation_count: 0,
            coverage_ratio: 0.0,
            observations: Vec::new(),
        };
    };
    let Some(last_observation) = open_session_price_observations.last() else {
        return MeanReversionGridObservations {
            source_observation_count: 0,
            coverage_ratio: 0.0,
            observations: Vec::new(),
        };
    };
    if last_observation.observation_timestamp_utc_nanos
        <= first_observation.observation_timestamp_utc_nanos
    {
        return MeanReversionGridObservations {
            source_observation_count: 0,
            coverage_ratio: 0.0,
            observations: Vec::new(),
        };
    }

    let source_window_start_utc_nanos = first_observation.observation_timestamp_utc_nanos;
    let source_window_end_utc_nanos = last_observation.observation_timestamp_utc_nanos;
    let source_duration_nanos = source_window_end_utc_nanos - source_window_start_utc_nanos;
    if source_duration_nanos < interval_nanos {
        return MeanReversionGridObservations {
            source_observation_count: 0,
            coverage_ratio: 0.0,
            observations: Vec::new(),
        };
    }

    let mut observations = Vec::new();

    // Covered intervals require at least one direct source tick. Carry-forward intervals stay on
    // the grid but do not add observed coverage.
    let mut covered_interval_count = 0usize;

    let mut interval_end_utc_nanos = source_window_start_utc_nanos.saturating_add(interval_nanos);
    let mut pending_observation: Option<CalibrationMeanReversionStageObservation> = None;

    let mut interval_has_direct_observation = false;

    for observation in open_session_price_observations {
        while observation.observation_timestamp_utc_nanos >= interval_end_utc_nanos {
            if let Some(pending_observation) = pending_observation.as_ref() {
                let mut snapped_observation = pending_observation.clone();
                snapped_observation.observation_timestamp_utc_nanos = interval_end_utc_nanos;

                observations.push(snapped_observation);

                if interval_has_direct_observation {
                    covered_interval_count = covered_interval_count.saturating_add(1);
                }
            }

            interval_end_utc_nanos = interval_end_utc_nanos.saturating_add(interval_nanos);
            interval_has_direct_observation = false;
            if interval_end_utc_nanos > source_window_end_utc_nanos {
                break;
            }
        }

        if interval_end_utc_nanos > source_window_end_utc_nanos {
            break;
        }

        pending_observation = Some(observation.clone());
        interval_has_direct_observation = true;
    }

    let source_observation_count = covered_interval_count;
    MeanReversionGridObservations {
        source_observation_count,
        coverage_ratio: coverage_ratio(source_observation_count, observations.len()),
        observations,
    }
}

fn interval_hours_to_nanos(interval_hours: f64) -> Option<i64> {
    let interval_nanos = interval_hours * NANOS_PER_HOUR_F64;
    if !interval_nanos.is_finite() || interval_nanos < 1.0 || interval_nanos > i64::MAX as f64 {
        return None;
    }

    Some(interval_nanos.round() as i64)
}

fn resolve_mean_reversion_grid_observations(
    explicit_calibration_grid_interval_hours: Option<f64>,
    boundary_open_observations: &[CalibrationMeanReversionStageObservation],
    open_session_price_observations: &[CalibrationMeanReversionStageObservation],
    observed_session_duration_hours: &[f64],
) -> Option<ResolvedMeanReversionGridObservations> {
    if let Some(calibration_grid_interval_hours) = explicit_calibration_grid_interval_hours {
        return Some(build_resolved_mean_reversion_grid_observations(
            calibration_grid_interval_hours,
            MEAN_REVERSION_INTERVAL_OVERRIDE_POLICY,
            open_session_price_observations,
        ));
    }

    let boundary_grid = derive_mean_reversion_grid_interval_hours_from_boundary_observations(
        boundary_open_observations,
    )
    .map(|calibration_grid_interval_hours| {
        build_resolved_mean_reversion_grid_observations(
            calibration_grid_interval_hours,
            MEAN_REVERSION_BOUNDARY_INTERVAL_DERIVATION_POLICY,
            open_session_price_observations,
        )
    });

    if boundary_grid
        .as_ref()
        .is_some_and(|grid| mean_reversion_grid_has_identifiable_full_model_coverage(grid))
    {
        return boundary_grid;
    }

    let session_duration_grid = derive_mean_reversion_grid_interval_hours_from_session_durations(
        observed_session_duration_hours,
    )
    .map(|calibration_grid_interval_hours| {
        build_resolved_mean_reversion_grid_observations(
            calibration_grid_interval_hours,
            MEAN_REVERSION_SESSION_DURATION_INTERVAL_DERIVATION_POLICY,
            open_session_price_observations,
        )
    });

    if session_duration_grid
        .as_ref()
        .is_some_and(|grid| mean_reversion_grid_has_identifiable_full_model_coverage(grid))
    {
        return session_duration_grid;
    }

    let source_window_grid = derive_mean_reversion_grid_interval_hours_from_source_window(
        open_session_price_observations,
        mean_reversion_full_model_required_source_observation_count(),
    )
    .map(|calibration_grid_interval_hours| {
        build_resolved_mean_reversion_grid_observations(
            calibration_grid_interval_hours,
            MEAN_REVERSION_SOURCE_WINDOW_INTERVAL_DERIVATION_POLICY,
            open_session_price_observations,
        )
    });

    if source_window_grid
        .as_ref()
        .is_some_and(|grid| mean_reversion_grid_has_identifiable_full_model_coverage(grid))
    {
        return source_window_grid;
    }

    session_duration_grid.or(boundary_grid)
}

fn build_resolved_mean_reversion_grid_observations(
    calibration_grid_interval_hours: f64,
    interval_policy: &'static str,
    open_session_price_observations: &[CalibrationMeanReversionStageObservation],
) -> ResolvedMeanReversionGridObservations {
    ResolvedMeanReversionGridObservations {
        calibration_grid_interval_hours,
        interval_policy,
        grid_observations: mean_reversion_grid_observations_from_open_session_prices(
            open_session_price_observations,
            calibration_grid_interval_hours,
        ),
    }
}

fn mean_reversion_grid_has_identifiable_full_model_coverage(
    resolved_grid_observations: &ResolvedMeanReversionGridObservations,
) -> bool {
    let grid_observations = &resolved_grid_observations.grid_observations;
    let required_source_observation_count =
        mean_reversion_full_model_required_source_observation_count();
    let retained_observation_count = grid_observations.observations.len();
    if retained_observation_count < required_source_observation_count {
        return false;
    }

    let required_coverage_ratio = coverage_ratio(
        required_source_observation_count,
        retained_observation_count,
    );

    grid_observations.source_observation_count >= required_source_observation_count
        && grid_observations.coverage_ratio >= required_coverage_ratio
}

fn derive_mean_reversion_grid_interval_hours_from_boundary_observations(
    boundary_open_observations: &[CalibrationMeanReversionStageObservation],
) -> Option<f64> {
    let mut positive_interval_hours = boundary_open_observations
        .windows(2)
        .filter_map(|window| {
            let current_observation = &window[0];
            let next_observation = &window[1];
            if next_observation.observation_timestamp_utc_nanos
                <= current_observation.observation_timestamp_utc_nanos
            {
                return None;
            }
            Some(duration_nanos_to_hours(
                next_observation.observation_timestamp_utc_nanos
                    - current_observation.observation_timestamp_utc_nanos,
            ))
        })
        .collect::<Vec<_>>();

    median_positive_interval_hours(positive_interval_hours.as_mut_slice())
}

fn derive_mean_reversion_grid_interval_hours_from_session_durations(
    observed_session_duration_hours: &[f64],
) -> Option<f64> {
    let mut positive_interval_hours = observed_session_duration_hours
        .iter()
        .copied()
        .filter(|duration_hours| duration_hours.is_finite() && *duration_hours > 0.0)
        .collect::<Vec<_>>();

    median_positive_interval_hours(positive_interval_hours.as_mut_slice())
}

fn derive_mean_reversion_grid_interval_hours_from_source_window(
    open_session_price_observations: &[CalibrationMeanReversionStageObservation],
    required_source_observation_count: usize,
) -> Option<f64> {
    let first_observation = open_session_price_observations.first()?;
    let last_observation = open_session_price_observations.last()?;

    if required_source_observation_count == 0 {
        return None;
    }

    let source_duration_nanos = last_observation
        .observation_timestamp_utc_nanos
        .checked_sub(first_observation.observation_timestamp_utc_nanos)?;
    if source_duration_nanos < required_source_observation_count as i64 {
        return None;
    }

    let interval_nanos = source_duration_nanos / required_source_observation_count as i64;
    if interval_nanos <= 0 {
        return None;
    }

    Some(duration_nanos_to_hours(interval_nanos))
}

fn median_positive_interval_hours(positive_interval_hours: &mut [f64]) -> Option<f64> {
    if positive_interval_hours.is_empty() {
        return None;
    }

    positive_interval_hours.sort_by(|left, right| left.total_cmp(right));
    let midpoint = positive_interval_hours.len() / 2;

    if positive_interval_hours.len() % 2 == 1 {
        Some(positive_interval_hours[midpoint])
    } else {
        Some((positive_interval_hours[midpoint - 1] + positive_interval_hours[midpoint]) / 2.0)
    }
}

fn load_market_data_from_phases(
    loading_request: MarketDataLoadingRequest,
) -> Result<LoadedMarketDataPhases, CalibrationMarketDataPreparationError> {
    let normalized_tick_output = normalize_market_data_source(
        &loading_request.tick_data_source,
        &loading_request.tick_data_column_mapping,
        &loading_request.normalization_policy,
        loading_request.require_hawkes,
    )
    .map_err(|source| {
        CalibrationMarketDataPreparationError::from(format!(
            "failed to normalize market-data source: {source}"
        ))
    })?;

    let session_resolution = resolve_market_data_session_configuration(
        loading_request.session_configuration.clone(),
        Some(&normalized_tick_output),
    )
    .map_err(|source| {
        CalibrationMarketDataPreparationError::from(format!(
            "failed to resolve market-data session: {source}"
        ))
    })?;

    let loading_output = prepare_market_data_loading_from_phases(
        loading_request,
        &normalized_tick_output,
        session_resolution,
    )
    .map_err(|source| {
        CalibrationMarketDataPreparationError::from(format!(
            "failed to prepare market-data calibration inputs: {source}"
        ))
    })?;

    Ok(LoadedMarketDataPhases {
        normalized_tick_output,
        loading_output,
    })
}

fn prepared_market_data_plan_from_loading_output(
    request: CalibrationMarketDataPreparationRequest,
    cpu_count: Option<usize>,
    selected_stages: Vec<CalibrationStage>,
    trade_events_data_reference: String,
    option_surface_data_reference: Option<String>,
    normalized_tick_output: MarketDataNormalizedTickOutput,
    loading_output: MarketDataLoadingOutput,
) -> Result<PreparedMarketDataCalibrationPlan, CalibrationMarketDataPreparationError> {
    let run_request = CalibrationRunSpec::try_new(
        request.run_id.clone(),
        CalibrationRunDataInputs {
            exchange_code: request.session_configuration.exchange_code.clone(),
            trade_events_data_reference: trade_events_data_reference.clone(),
            option_surface_data_reference: option_surface_data_reference.clone(),
        },
        request.output_preferences.execution_mode,
        selected_stages.clone(),
        CalibrationRunSeedSettings {
            global_seed: stable_seed_from_run_id(request.run_id.as_str()),
            stage_seed_overrides: Vec::new(),
        },
        resolve_solver_settings_by_stage(
            &selected_stages,
            &request.optimizer_settings_by_stage,
            &loading_output,
            cpu_count,
        )
        .map_err(CalibrationMarketDataPreparationError::from)?,
    )
    .map_err(|source| {
        CalibrationMarketDataPreparationError::from(format!(
            "failed to build calibration run spec: {source}"
        ))
    })?;

    let execution_input = CalibrationStagedExecutionInput {
        schema_version: CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION.to_string(),
        request: run_request,
        seasonality_stage_input: Some(
            loading_output
                .preprocessing_output
                .seasonality_stage_input
                .clone(),
        ),
        hawkes_candidate_inputs: loading_output
            .preprocessing_output
            .hawkes_candidate_inputs
            .clone(),
        return_components_candidate_generation_input: loading_output
            .preprocessing_output
            .return_components_candidate_generation_input
            .clone(),
        mean_reversion_stage_input: mean_reversion_stage_input_from_request(
            &request,
            &selected_stages,
            &loading_output,
        )?,
        joint_refinement_restart_candidate_budget_override: request
            .dynamics_search_restart_candidate_budget,
        joint_refinement_option_refinement_candidate_count_override: request
            .joint_refinement_option_refinement_candidate_count_override,
        joint_refinement_audit_option_pricing_enabled: request
            .joint_refinement_audit_option_pricing_enabled,
        joint_refinement_option_pricing_input: joint_refinement_option_pricing_input_from_request(
            &request,
            &loading_output,
            &normalized_tick_output,
        )?,
        runtime_resources: runtime_resources_from_output_preferences(&request.output_preferences),
    };

    let return_components_input = loading_output
        .preprocessing_output
        .return_components_candidate_generation_input
        .as_ref();

    let diagnostics = CalibrationMarketDataPreparationDiagnostics {
        workflow_scope: request.workflow_scope,
        selected_stages: selected_stages.clone(),
        executed_preprocessing_steps: executed_preprocessing_steps(
            request.workflow_scope,
            loading_output.option_surface_summary.is_some(),
            loading_output.known_news_summary.is_some(),
        ),
        tick_record_count: loading_output.tick_data_summary.retained_row_count,
        option_surface_record_count: loading_output
            .option_surface_summary
            .as_ref()
            .map(|summary| summary.retained_row_count)
            .unwrap_or(0),
        known_news_event_count: loading_output.preprocessing_output.known_news_event_count,
        known_news_events_outside_source_window_count: loading_output
            .preprocessing_output
            .known_news_events_outside_source_window_count,
        session_open_tick_count: loading_output.preprocessing_output.session_open_tick_count,
        session_closed_tick_count: loading_output
            .preprocessing_output
            .session_closed_tick_count,
        known_news_open_event_count: loading_output
            .preprocessing_output
            .known_news_open_event_count,
        known_news_closed_event_count: loading_output
            .preprocessing_output
            .known_news_closed_event_count,
        seasonality_overlay_count: loading_output
            .preprocessing_output
            .seasonality_stage_input
            .scheduled_event_overlays
            .len(),
        hawkes_candidate_count: loading_output
            .preprocessing_output
            .hawkes_candidate_inputs
            .as_ref()
            .map_or(0, Vec::len),
        known_news_return_sample_count: return_components_input
            .map(|input| input.realized_known_news_return_samples.len())
            .unwrap_or(0),
        intraday_jump_candidate_count: return_components_input
            .map(|input| input.detected_intraday_jump_candidates.len())
            .unwrap_or(0),
        close_open_boundary_observation_count: return_components_input
            .map(|input| input.observed_close_open_boundary_observations.len())
            .unwrap_or(0),
        material_close_open_gap_count: loading_output
            .preprocessing_output
            .material_close_open_gap_count,
    };

    let mut derivation_policies = vec![
        loading_output.session_resolution.selected_policy_id.clone(),
        PREPARED_MARKET_DATA_DERIVATION_POLICY_ID.to_string(),
        KNOWN_NEWS_WINDOW_FILTER_POLICY_ID.to_string(),
        KNOWN_NEWS_OVERLAY_POLICY_ID.to_string(),
        TRADE_LABEL_DERIVATION_POLICY_ID.to_string(),
        HAWKES_SEARCH_POLICY_ID.to_string(),
        RETURN_COMPONENTS_JUMP_POLICY_ID.to_string(),
        RETURN_COMPONENTS_BOUNDARY_POLICY_ID.to_string(),
        RETURN_COMPONENTS_CONTINUOUS_FIT_POLICY_ID.to_string(),
    ];

    if has_missing_stage_solver_settings(
        selected_stages.as_slice(),
        &request.optimizer_settings_by_stage,
    ) {
        derivation_policies.push(DATA_DERIVED_SOLVER_SETTINGS_POLICY_ID.to_string());
    }

    let provenance = CalibrationMarketDataPreparationProvenance {
        tick_data_summary: serde_json::to_value(&loading_output.tick_data_summary).map_err(
            |source| {
                CalibrationMarketDataPreparationError::from(format!(
                    "failed to serialize tick-data summary: {source}"
                ))
            },
        )?,
        option_surface_summary: loading_output
            .option_surface_summary
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|source| {
                CalibrationMarketDataPreparationError::from(format!(
                    "failed to serialize option-surface summary: {source}"
                ))
            })?,
        known_news_summary: loading_output
            .known_news_summary
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|source| {
                CalibrationMarketDataPreparationError::from(format!(
                    "failed to serialize known-news summary: {source}"
                ))
            })?,
        normalization_policy: serde_json::to_value(&request.normalization_policy).map_err(
            |source| {
                CalibrationMarketDataPreparationError::from(format!(
                    "failed to serialize normalization policy: {source}"
                ))
            },
        )?,
        workflow_scope: request.workflow_scope,
        selected_stages,
        session_definition_path: if loading_output.session_resolution.manual_override_applied {
            request
                .session_configuration
                .session_definition_source
                .as_ref()
                .map(|source| source.path.clone())
        } else {
            None
        },
        derived_trade_events_data_reference: trade_events_data_reference,
        derived_option_surface_data_reference: option_surface_data_reference,
        derivation_policies,
        deterministic_defaults_applied: deterministic_defaults_applied(&request),
    };

    Ok(PreparedMarketDataCalibrationPlan {
        execution_input,
        diagnostics,
        provenance,
        session_resolution: loading_output.session_resolution,
        fit_warning: loading_output.fit_warning,
        reference_price: loading_output.reference_price,
    })
}

fn parse_timestamp_utc(
    raw_timestamp: &str,
) -> Result<DateTime<Utc>, CalibrationMarketDataPreparationError> {
    DateTime::parse_from_rfc3339(raw_timestamp)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|_| {
            CalibrationMarketDataPreparationError::from(format!(
                "joint option calibration received invalid option_surface_valuation_time_utc: {raw_timestamp}"
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::{
        CalibrationMeanReversionStageObservation, MEAN_REVERSION_INTERVAL_OVERRIDE_POLICY,
        MEAN_REVERSION_SESSION_DURATION_INTERVAL_DERIVATION_POLICY,
        MEAN_REVERSION_SOURCE_WINDOW_INTERVAL_DERIVATION_POLICY,
        mean_reversion_full_model_required_source_observation_count,
        mean_reversion_grid_observations_from_open_session_prices,
        resolve_mean_reversion_grid_observations,
    };

    const DAY_NANOS: i64 = 86_400_000_000_000;

    #[test]
    fn mean_reversion_grid_observations_snap_to_interval_boundaries() {
        let grid_observations = mean_reversion_grid_observations_from_open_session_prices(
            &[
                CalibrationMeanReversionStageObservation {
                    observation_timestamp_utc_nanos: 1_000,
                    observed_log_price: 0.0,
                },
                CalibrationMeanReversionStageObservation {
                    observation_timestamp_utc_nanos: 86_400_000_020_000,
                    observed_log_price: 0.1,
                },
                CalibrationMeanReversionStageObservation {
                    observation_timestamp_utc_nanos: 172_800_000_030_000,
                    observed_log_price: 0.2,
                },
            ],
            24.0,
        );

        assert_eq!(grid_observations.source_observation_count, 2);
        assert_eq!(grid_observations.coverage_ratio, 1.0);
        assert_eq!(
            grid_observations
                .observations
                .iter()
                .map(|observation| observation.observation_timestamp_utc_nanos)
                .collect::<Vec<_>>(),
            vec![86_400_000_001_000, 172_800_000_001_000]
        );
        assert_eq!(
            grid_observations
                .observations
                .iter()
                .map(|observation| observation.observed_log_price)
                .collect::<Vec<_>>(),
            vec![0.0, 0.1]
        );
    }

    #[test]
    fn mean_reversion_grid_observations_carry_forward_last_observed_price_across_empty_intervals() {
        let grid_observations = mean_reversion_grid_observations_from_open_session_prices(
            &[
                CalibrationMeanReversionStageObservation {
                    observation_timestamp_utc_nanos: 1_000,
                    observed_log_price: 0.0,
                },
                CalibrationMeanReversionStageObservation {
                    observation_timestamp_utc_nanos: 259_200_000_020_000,
                    observed_log_price: 0.3,
                },
            ],
            24.0,
        );

        assert_eq!(grid_observations.source_observation_count, 1);
        assert_eq!(grid_observations.coverage_ratio, 1.0 / 3.0);
        assert_eq!(
            grid_observations
                .observations
                .iter()
                .map(|observation| observation.observation_timestamp_utc_nanos)
                .collect::<Vec<_>>(),
            vec![86_400_000_001_000, 172_800_000_001_000, 259_200_000_001_000]
        );
        assert_eq!(
            grid_observations
                .observations
                .iter()
                .map(|observation| observation.observed_log_price)
                .collect::<Vec<_>>(),
            vec![0.0, 0.0, 0.0]
        );
    }

    #[test]
    fn mean_reversion_grid_resolution_uses_session_grid_when_boundary_grid_is_too_sparse() {
        let open_session_observations = vec![
            CalibrationMeanReversionStageObservation {
                observation_timestamp_utc_nanos: 1_000,
                observed_log_price: 0.0,
            },
            CalibrationMeanReversionStageObservation {
                observation_timestamp_utc_nanos: DAY_NANOS + 20_000,
                observed_log_price: 0.1,
            },
            CalibrationMeanReversionStageObservation {
                observation_timestamp_utc_nanos: 2 * DAY_NANOS + 30_000,
                observed_log_price: 0.2,
            },
            CalibrationMeanReversionStageObservation {
                observation_timestamp_utc_nanos: 3 * DAY_NANOS + 40_000,
                observed_log_price: 0.3,
            },
        ];
        let boundary_open_observations = vec![
            CalibrationMeanReversionStageObservation {
                observation_timestamp_utc_nanos: 1_000,
                observed_log_price: 0.0,
            },
            CalibrationMeanReversionStageObservation {
                observation_timestamp_utc_nanos: 7 * DAY_NANOS + 1_000,
                observed_log_price: 0.7,
            },
        ];

        let resolved_grid_observations = resolve_mean_reversion_grid_observations(
            None,
            boundary_open_observations.as_slice(),
            open_session_observations.as_slice(),
            &[24.0],
        )
        .unwrap();

        assert_eq!(
            resolved_grid_observations.interval_policy,
            MEAN_REVERSION_SESSION_DURATION_INTERVAL_DERIVATION_POLICY
        );
        assert_eq!(
            resolved_grid_observations.calibration_grid_interval_hours,
            24.0
        );
        assert!(
            resolved_grid_observations
                .grid_observations
                .source_observation_count
                >= mean_reversion_full_model_required_source_observation_count()
        );
    }

    #[test]
    fn mean_reversion_grid_resolution_uses_source_window_grid_when_session_grid_is_too_sparse() {
        let source_window_nanos = 3 * 3_600_000_000_000_i64;
        let open_session_observations = vec![
            CalibrationMeanReversionStageObservation {
                observation_timestamp_utc_nanos: 1_000,
                observed_log_price: 0.0,
            },
            CalibrationMeanReversionStageObservation {
                observation_timestamp_utc_nanos: source_window_nanos / 3 + 20_000,
                observed_log_price: 0.1,
            },
            CalibrationMeanReversionStageObservation {
                observation_timestamp_utc_nanos: 2 * source_window_nanos / 3 + 30_000,
                observed_log_price: 0.2,
            },
            CalibrationMeanReversionStageObservation {
                observation_timestamp_utc_nanos: source_window_nanos + 40_000,
                observed_log_price: 0.3,
            },
        ];

        let resolved_grid_observations = resolve_mean_reversion_grid_observations(
            None,
            &[],
            open_session_observations.as_slice(),
            &[24.0],
        )
        .unwrap();

        assert_eq!(
            resolved_grid_observations.interval_policy,
            MEAN_REVERSION_SOURCE_WINDOW_INTERVAL_DERIVATION_POLICY
        );
        assert_eq!(
            resolved_grid_observations
                .grid_observations
                .source_observation_count,
            mean_reversion_full_model_required_source_observation_count()
        );
        assert_eq!(
            resolved_grid_observations
                .grid_observations
                .observations
                .len(),
            mean_reversion_full_model_required_source_observation_count()
        );
    }

    #[test]
    fn mean_reversion_grid_resolution_honors_explicit_grid_even_when_sparse() {
        let open_session_observations = vec![
            CalibrationMeanReversionStageObservation {
                observation_timestamp_utc_nanos: 1_000,
                observed_log_price: 0.0,
            },
            CalibrationMeanReversionStageObservation {
                observation_timestamp_utc_nanos: DAY_NANOS + 20_000,
                observed_log_price: 0.1,
            },
            CalibrationMeanReversionStageObservation {
                observation_timestamp_utc_nanos: 2 * DAY_NANOS + 30_000,
                observed_log_price: 0.2,
            },
        ];

        let resolved_grid_observations = resolve_mean_reversion_grid_observations(
            Some(168.0),
            &[],
            open_session_observations.as_slice(),
            &[24.0],
        )
        .unwrap();

        assert_eq!(
            resolved_grid_observations.interval_policy,
            MEAN_REVERSION_INTERVAL_OVERRIDE_POLICY
        );
        assert_eq!(
            resolved_grid_observations.calibration_grid_interval_hours,
            168.0
        );
    }
}
