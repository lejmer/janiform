use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use janiform::calibration::{
    CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION, CalibrationExecutionMode,
    CalibrationMeanReversionStageExecutionInput, CalibrationMeanReversionStageObservation,
    CalibrationOptimizerSettings, CalibrationReturnComponentsActivityContextPayload,
    CalibrationReturnComponentsStageCandidateGenerationInput, CalibrationRunDataInputs,
    CalibrationRunSeedSettings, CalibrationRunSpec, CalibrationStage,
    CalibrationStageSolverSettings, CalibrationStagedExecutionInput,
    CalibrationStagedExecutionRuntimeResources, CalibrationStagedRunObserverEvent,
    CalibrationStagedRunReport, CalibrationStagedRunStageReport, CurrentStateEstimationInput,
    MarketDataInputFileReference, MarketDataKnownNewsColumnMapping,
    MarketDataKnownNewsLoadingInput, MarketDataLoadingOutput, MarketDataLoadingRequest,
    MarketDataNormalizationPolicy, MarketDataOptionSurfaceColumnMapping,
    MarketDataOptionSurfaceLoadingInput, MarketDataSessionConfiguration,
    MarketDataSessionResolutionPayload, MarketDataSourceProbePayload,
    MarketDataTickDataColumnMapping, MarketDataTranslationPreprocessingOutput,
    MarketDataTranslationPreprocessingRequest, MarketDataTranslationPreprocessingSettings,
    ReturnComponentsStageFittedParameters, ReturnComponentsStageObservedSignalSummaryStatistics,
    RuntimeStructuralSeasonalityMaterializationInput,
    estimate_current_state_from_observed_data as estimate_current_state_from_observed_data_core,
    evaluate_return_components_exact_candidate_from_bridge,
    load_and_preprocess_market_data_translation_request as load_and_preprocess_market_data_translation_request_native,
    materialize_runtime_structural_seasonality as materialize_runtime_structural_seasonality_core,
    normalize_market_data_source as normalize_market_data_source_native,
    prepare_market_data_loading_from_phases as prepare_market_data_loading_from_phases_native,
    preprocess_market_data_translation_request as preprocess_market_data_translation_request_native,
    probe_market_data_source as probe_market_data_source_native,
    resolve_market_data_session_configuration as resolve_market_data_session_configuration_native,
    run_staged_calibration_from_execution_input,
    run_staged_calibration_from_execution_input_with_observer,
};
use janiform::hawkes::{HawkesEventSide, HawkesKernelParameters};
use janiform::hourly_factors::{ONE_HOUR_NANOS_F64, duration_nanos_to_hours};
use janiform::simulation::MeanReversionFittedParameters;
use pyo3::prelude::{Bound, Py, PyResult, Python};
use pyo3::types::{PyAny, PyAnyMethods, PyDict, PyDictMethods, PyModule, PyModuleMethods};
use pyo3::{pyfunction, wrap_pyfunction};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::calendar::parse_timestamp_utc;
use crate::CalibrationExecutionError;
use crate::python_value::{json_value_to_py_object, py_any_to_json_value};
use crate::types::{
    PyCalendarRegistry, PyCalibrationMemoryCheckpointStore, PyNormalizedMarketDataHandle,
    PyPreparedCalibrationExecutionHandle,
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
const MEAN_REVERSION_INTERVAL_OVERRIDE_POLICY: &str =
    "explicit_mean_reversion_grid_interval_hours.v1";
const DEFAULT_SEASONALITY_MAX_ITERATIONS: usize = 150;
const DEFAULT_OTHER_STAGE_MAX_ITERATIONS: usize = 250;
const DEFAULT_SOLVER_ABSOLUTE_TOLERANCE: f64 = 1.0e-8;
const DEFAULT_SOLVER_RELATIVE_TOLERANCE: f64 = 1.0e-6;
const DEFAULT_SOLVER_CONSTRAINT_PENALTY_WEIGHT: f64 = 10.0;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum PythonCalibrationMarketDataWorkflowScope {
    SeasonalityOnly,
    FullWithoutOptionSurface,
    FullWithOptionSurface,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PythonMarketDataColumnMapping {
    tick_data: MarketDataTickDataColumnMapping,
    #[serde(default)]
    option_surface: Option<MarketDataOptionSurfaceColumnMapping>,
    #[serde(default)]
    known_news: Option<MarketDataKnownNewsColumnMapping>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PythonMarketDataOutputPreferences {
    execution_mode: CalibrationExecutionMode,
    #[serde(default)]
    disk_checkpoint_directory: Option<String>,
    #[serde(rename = "emit_derived_staged_execution_input")]
    _emit_derived_staged_execution_input: bool,
    #[serde(rename = "emit_input_provenance_report")]
    _emit_input_provenance_report: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PythonMarketDataKnownNewsInput {
    calendar_source: MarketDataInputFileReference,
    #[serde(default)]
    calendar_timezone: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PythonMeanReversionConfiguration {
    #[serde(default)]
    calibration_grid_interval_hours: Option<f64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PythonMarketDataCalibrationRequest {
    #[serde(rename = "schema_version")]
    _schema_version: String,
    run_id: String,
    tick_data_source: MarketDataInputFileReference,
    session_configuration: MarketDataSessionConfiguration,
    column_mapping: PythonMarketDataColumnMapping,
    workflow_scope: PythonCalibrationMarketDataWorkflowScope,
    normalization_policy: MarketDataNormalizationPolicy,
    output_preferences: PythonMarketDataOutputPreferences,
    #[serde(default)]
    optimizer_settings_by_stage: Vec<CalibrationStageSolverSettings>,
    #[serde(default)]
    preprocessing_settings: Option<MarketDataTranslationPreprocessingSettings>,
    #[serde(default)]
    option_surface_source: Option<MarketDataInputFileReference>,
    #[serde(default)]
    known_news_inputs: Option<PythonMarketDataKnownNewsInput>,
    #[serde(default)]
    mean_reversion: Option<PythonMeanReversionConfiguration>,
}

#[derive(Clone, Debug, Serialize)]
struct PreparedMarketDataDiagnostics {
    workflow_scope: PythonCalibrationMarketDataWorkflowScope,
    selected_stages: Vec<CalibrationStage>,
    executed_preprocessing_steps: Vec<String>,
    tick_record_count: usize,
    option_surface_record_count: usize,
    known_news_event_count: usize,
    known_news_events_outside_source_window_count: usize,
    session_open_tick_count: usize,
    session_closed_tick_count: usize,
    known_news_open_event_count: usize,
    known_news_closed_event_count: usize,
    seasonality_overlay_count: usize,
    hawkes_candidate_count: usize,
    known_news_return_sample_count: usize,
    intraday_jump_candidate_count: usize,
    close_open_boundary_observation_count: usize,
    material_close_open_gap_count: usize,
}

#[derive(Clone, Debug, Serialize)]
struct PreparedMarketDataProvenance {
    tick_data_summary: Value,
    option_surface_summary: Option<Value>,
    known_news_summary: Option<Value>,
    normalization_policy: Value,
    workflow_scope: PythonCalibrationMarketDataWorkflowScope,
    selected_stages: Vec<CalibrationStage>,
    session_definition_path: Option<String>,
    derived_trade_events_data_reference: String,
    derived_option_surface_data_reference: Option<String>,
    derivation_policies: Vec<String>,
    deterministic_defaults_applied: Vec<String>,
}

struct PreparedMarketDataPlan {
    execution_input: CalibrationStagedExecutionInput,
    diagnostics: PreparedMarketDataDiagnostics,
    provenance: PreparedMarketDataProvenance,
    session_resolution: MarketDataSessionResolutionPayload,
    fit_warning: Option<String>,
    reference_price: Option<f64>,
}

fn _selected_stages_for_scope(
    workflow_scope: PythonCalibrationMarketDataWorkflowScope,
) -> Vec<CalibrationStage> {
    match workflow_scope {
        PythonCalibrationMarketDataWorkflowScope::SeasonalityOnly => {
            vec![CalibrationStage::Seasonality]
        }
        PythonCalibrationMarketDataWorkflowScope::FullWithoutOptionSurface
        | PythonCalibrationMarketDataWorkflowScope::FullWithOptionSurface => vec![
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            CalibrationStage::ReturnComponents,
            CalibrationStage::MeanReversion,
            CalibrationStage::JointRefinement,
        ],
    }
}

fn _build_dataset_reference(run_id: &str, dataset_role: &str, provenance_label: &str) -> String {
    format!(
        "market_data://{}/{}/{}",
        _sanitize_reference_component(run_id),
        dataset_role,
        _sanitize_reference_component(provenance_label)
    )
}

fn _sanitize_reference_component(value: &str) -> String {
    let sanitized = value
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();

    let trimmed = sanitized.trim_matches('_');
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

fn _stable_seed_from_run_id(run_id: &str) -> u64 {
    run_id
        .chars()
        .enumerate()
        .map(|(character_index, character)| {
            let position = u64::try_from(character_index + 1).unwrap_or(u64::MAX);
            position.saturating_mul(u64::from(u32::from(character)))
        })
        .sum()
}

fn _default_solver_settings(
    stage: CalibrationStage,
    cpu_count: Option<usize>,
) -> Result<CalibrationStageSolverSettings, String> {
    let max_iterations = match stage {
        CalibrationStage::Seasonality => DEFAULT_SEASONALITY_MAX_ITERATIONS,
        _ => DEFAULT_OTHER_STAGE_MAX_ITERATIONS,
    };

    let optimizer_settings = CalibrationOptimizerSettings::try_new(
        "lbfgs",
        max_iterations,
        cpu_count.unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from)),
        DEFAULT_SOLVER_ABSOLUTE_TOLERANCE,
        DEFAULT_SOLVER_RELATIVE_TOLERANCE,
        DEFAULT_SOLVER_CONSTRAINT_PENALTY_WEIGHT,
    )
    .map_err(|source| format!("failed to build default optimizer settings: {source}"))?;

    Ok(CalibrationStageSolverSettings {
        stage,
        optimizer_settings,
    })
}

const fn _calibration_stage_name(stage: CalibrationStage) -> &'static str {
    match stage {
        CalibrationStage::Seasonality => "seasonality",
        CalibrationStage::Hawkes => "hawkes",
        CalibrationStage::ReturnComponents => "return_components",
        CalibrationStage::MeanReversion => "mean_reversion",
        CalibrationStage::JointRefinement => "joint_refinement",
    }
}

fn _resolve_solver_settings_by_stage(
    selected_stages: &[CalibrationStage],
    configured_solver_settings: &[CalibrationStageSolverSettings],
    cpu_count: Option<usize>,
) -> Result<Vec<CalibrationStageSolverSettings>, String> {
    for configured_stage_settings in configured_solver_settings {
        if !selected_stages.contains(&configured_stage_settings.stage) {
            return Err(format!(
                "optimizer_settings_by_stage contains stage '{}' outside the selected workflow scope",
                _calibration_stage_name(configured_stage_settings.stage)
            ));
        }
    }

    selected_stages
        .iter()
        .copied()
        .map(|stage| {
            configured_solver_settings
                .iter()
                .find(|configured_stage_settings| configured_stage_settings.stage == stage)
                .cloned()
                .map_or_else(|| _default_solver_settings(stage, cpu_count), Ok)
        })
        .collect()
}

fn _executed_preprocessing_steps(
    workflow_scope: PythonCalibrationMarketDataWorkflowScope,
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
        PythonCalibrationMarketDataWorkflowScope::SeasonalityOnly
    ) {
        preprocessing_steps.push("hawkes_candidate_generation".to_string());
        preprocessing_steps.push("return_components_candidate_generation".to_string());
    }

    if option_surface_present {
        preprocessing_steps.push("option_surface_quote_normalization".to_string());
    }

    preprocessing_steps
}

fn _deterministic_defaults_applied(request: &PythonMarketDataCalibrationRequest) -> Vec<String> {
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

fn _build_loading_request(
    request: &PythonMarketDataCalibrationRequest,
    selected_stages: &[CalibrationStage],
) -> Result<(MarketDataLoadingRequest, String, Option<String>), String> {
    let trade_events_data_reference = _build_dataset_reference(
        request.run_id.as_str(),
        "ticks",
        request.tick_data_source.provenance_label.as_str(),
    );

    let option_surface_data_reference = request.option_surface_source.as_ref().map(|source| {
        _build_dataset_reference(
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
        (None, None) => None,
        (Some(_), None) => {
            return Err(
                "column_mapping.option_surface is required when option_surface_source is provided"
                    .to_string(),
            );
        }
        (None, Some(_)) => {
            return Err(
                "option_surface_source is required when column_mapping.option_surface is provided"
                    .to_string(),
            );
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
            return Err(
                "column_mapping.known_news is required when known_news_inputs is provided"
                    .to_string(),
            );
        }
        (None, Some(_)) => {
            return Err(
                "known_news_inputs is required when column_mapping.known_news is provided"
                    .to_string(),
            );
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

fn _runtime_resources_from_output_preferences(
    output_preferences: &PythonMarketDataOutputPreferences,
) -> Option<CalibrationStagedExecutionRuntimeResources> {
    if output_preferences.execution_mode != CalibrationExecutionMode::Disk {
        return None;
    }

    Some(CalibrationStagedExecutionRuntimeResources {
        disk_checkpoint_directory: output_preferences.disk_checkpoint_directory.clone(),
    })
}

fn _mean_reversion_stage_input_from_request(
    request: &PythonMarketDataCalibrationRequest,
    selected_stages: &[CalibrationStage],
    loading_output: &MarketDataLoadingOutput,
) -> Result<Option<CalibrationMeanReversionStageExecutionInput>, String> {
    if !selected_stages.contains(&CalibrationStage::MeanReversion) {
        return Ok(None);
    }

    let return_components_input = loading_output
        .preprocessing_output
        .return_components_candidate_generation_input
        .as_ref();

    let default_mean_reversion_configuration = PythonMeanReversionConfiguration {
        calibration_grid_interval_hours: None,
    };
    let mean_reversion_configuration = request
        .mean_reversion
        .as_ref()
        .unwrap_or(&default_mean_reversion_configuration);

    let boundary_observations = return_components_input
        .map(|input| input.observed_close_open_boundary_observations.as_slice())
        .unwrap_or(&[]);
    let boundary_open_observations = _mean_reversion_stage_observations_relative_to_reference_price(
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
        _mean_reversion_stage_observations_relative_to_reference_price(
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

    let (calibration_grid_interval_hours, interval_policy) =
        if let Some(calibration_grid_interval_hours) =
            mean_reversion_configuration.calibration_grid_interval_hours
        {
            (
                Some(calibration_grid_interval_hours),
                MEAN_REVERSION_INTERVAL_OVERRIDE_POLICY,
            )
        } else if let Some(calibration_grid_interval_hours) =
            _derive_mean_reversion_grid_interval_hours_from_boundary_observations(
                boundary_open_observations.as_slice(),
            )
        {
            (
                Some(calibration_grid_interval_hours),
                MEAN_REVERSION_BOUNDARY_INTERVAL_DERIVATION_POLICY,
            )
        } else {
            (
                _derive_mean_reversion_grid_interval_hours_from_session_durations(
                    observed_session_duration_hours,
                ),
                MEAN_REVERSION_SESSION_DURATION_INTERVAL_DERIVATION_POLICY,
            )
        };

    let Some(calibration_grid_interval_hours) = calibration_grid_interval_hours else {
        return Err(
            "mean-reversion calibration grid interval could not be derived from retained boundary spacing or observed session durations"
                .to_string(),
        );
    };

    if open_session_price_observations.is_empty() {
        return Ok(Some(CalibrationMeanReversionStageExecutionInput {
            calibration_grid_kind: format!(
                "{}:{}:{}",
                MEAN_REVERSION_OPEN_SESSION_GRID_KIND,
                MEAN_REVERSION_OPEN_SESSION_ALIGNMENT_POLICY,
                interval_policy
            ),
            calibration_grid_interval_hours,
            source_observation_count: 0,
            coverage_ratio: 0.0,
            observations: Vec::new(),
        }));
    }

    let grid_observations = _mean_reversion_grid_observations_from_open_session_prices(
        open_session_price_observations.as_slice(),
        calibration_grid_interval_hours,
    );

    Ok(Some(CalibrationMeanReversionStageExecutionInput {
        calibration_grid_kind: format!(
            "{}:{}:{}",
            MEAN_REVERSION_OPEN_SESSION_GRID_KIND,
            MEAN_REVERSION_OPEN_SESSION_ALIGNMENT_POLICY,
            interval_policy
        ),
        calibration_grid_interval_hours,
        source_observation_count: grid_observations.source_observation_count,
        coverage_ratio: grid_observations.coverage_ratio,
        observations: grid_observations.observations,
    }))
}

fn _mean_reversion_stage_observations_relative_to_reference_price(
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

struct MeanReversionGridObservations {
    source_observation_count: usize,
    coverage_ratio: f64,
    observations: Vec<CalibrationMeanReversionStageObservation>,
}

fn _mean_reversion_grid_observations_from_open_session_prices(
    open_session_price_observations: &[CalibrationMeanReversionStageObservation],
    calibration_grid_interval_hours: f64,
) -> MeanReversionGridObservations {
    let interval_nanos = _interval_hours_to_nanos(calibration_grid_interval_hours);
    let Some(interval_nanos) = interval_nanos else {
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

    let mut observations: Vec<CalibrationMeanReversionStageObservation> = Vec::new();
    let mut interval_end_utc_nanos = source_window_start_utc_nanos.saturating_add(interval_nanos);
    let mut pending_observation: Option<CalibrationMeanReversionStageObservation> = None;

    for observation in open_session_price_observations {
        while observation.observation_timestamp_utc_nanos >= interval_end_utc_nanos {
            if let Some(pending_observation) = pending_observation.as_ref() {
                let mut snapped_observation = pending_observation.clone();
                snapped_observation.observation_timestamp_utc_nanos = interval_end_utc_nanos;
                observations.push(snapped_observation);
            }

            interval_end_utc_nanos = interval_end_utc_nanos.saturating_add(interval_nanos);
            if interval_end_utc_nanos > source_window_end_utc_nanos {
                break;
            }
        }

        if interval_end_utc_nanos > source_window_end_utc_nanos {
            break;
        }

        pending_observation = Some(observation.clone());
    }

    let source_observation_count = observations.len();
    MeanReversionGridObservations {
        source_observation_count,
        coverage_ratio: if source_observation_count == 0 {
            0.0
        } else {
            1.0
        },
        observations,
    }
}

fn _interval_hours_to_nanos(interval_hours: f64) -> Option<i64> {
    let interval_nanos = interval_hours * ONE_HOUR_NANOS_F64;
    if !interval_nanos.is_finite() || interval_nanos < 1.0 || interval_nanos > i64::MAX as f64 {
        return None;
    }

    Some(interval_nanos.round() as i64)
}

fn _derive_mean_reversion_grid_interval_hours_from_boundary_observations(
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
    _median_positive_interval_hours(positive_interval_hours.as_mut_slice())
}

fn _derive_mean_reversion_grid_interval_hours_from_session_durations(
    observed_session_duration_hours: &[f64],
) -> Option<f64> {
    let mut positive_interval_hours = observed_session_duration_hours
        .iter()
        .copied()
        .filter(|duration_hours| duration_hours.is_finite() && *duration_hours > 0.0)
        .collect::<Vec<_>>();
    _median_positive_interval_hours(positive_interval_hours.as_mut_slice())
}

fn _median_positive_interval_hours(positive_interval_hours: &mut [f64]) -> Option<f64> {
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

#[cfg(test)]
mod tests {
    use super::{
        _mean_reversion_grid_observations_from_open_session_prices,
        CalibrationMeanReversionStageObservation,
    };

    #[test]
    fn mean_reversion_grid_observations_snap_to_interval_boundaries() {
        let grid_observations = _mean_reversion_grid_observations_from_open_session_prices(
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
        let grid_observations = _mean_reversion_grid_observations_from_open_session_prices(
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

        assert_eq!(grid_observations.source_observation_count, 3);
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
}

fn _load_market_data_from_phases(
    loading_request: MarketDataLoadingRequest,
) -> Result<MarketDataLoadingOutput, String> {
    let normalized_tick_output = normalize_market_data_source_native(
        &loading_request.tick_data_source,
        &loading_request.tick_data_column_mapping,
        &loading_request.normalization_policy,
        loading_request.require_hawkes,
    )
    .map_err(|source| format!("failed to normalize market-data source: {source}"))?;

    let session_resolution = resolve_market_data_session_configuration_native(
        loading_request.session_configuration.clone(),
        Some(&normalized_tick_output),
    )
    .map_err(|source| format!("failed to resolve market-data session: {source}"))?;

    prepare_market_data_loading_from_phases_native(
        loading_request,
        normalized_tick_output,
        session_resolution,
    )
    .map_err(|source| format!("failed to prepare market-data calibration inputs: {source}"))
}

fn _prepare_market_data_calibration_plan(
    request: PythonMarketDataCalibrationRequest,
    cpu_count: Option<usize>,
) -> Result<PreparedMarketDataPlan, String> {
    let selected_stages = _selected_stages_for_scope(request.workflow_scope);

    let (loading_request, trade_events_data_reference, option_surface_data_reference) =
        _build_loading_request(&request, &selected_stages)?;

    let loading_output = _load_market_data_from_phases(loading_request)?;

    _prepared_market_data_plan_from_loading_output(
        request,
        cpu_count,
        selected_stages,
        trade_events_data_reference,
        option_surface_data_reference,
        loading_output,
    )
}

fn _prepared_market_data_plan_from_loading_output(
    request: PythonMarketDataCalibrationRequest,
    cpu_count: Option<usize>,
    selected_stages: Vec<CalibrationStage>,
    trade_events_data_reference: String,
    option_surface_data_reference: Option<String>,
    loading_output: MarketDataLoadingOutput,
) -> Result<PreparedMarketDataPlan, String> {
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
            global_seed: _stable_seed_from_run_id(request.run_id.as_str()),
            stage_seed_overrides: Vec::new(),
        },
        _resolve_solver_settings_by_stage(
            &selected_stages,
            &request.optimizer_settings_by_stage,
            cpu_count,
        )?,
    )
    .map_err(|source| format!("failed to build calibration run spec: {source}"))?;

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
        mean_reversion_stage_input: _mean_reversion_stage_input_from_request(
            &request,
            &selected_stages,
            &loading_output,
        )?,
        runtime_resources: _runtime_resources_from_output_preferences(&request.output_preferences),
    };

    let return_components_input = loading_output
        .preprocessing_output
        .return_components_candidate_generation_input
        .as_ref();

    let diagnostics = PreparedMarketDataDiagnostics {
        workflow_scope: request.workflow_scope,
        selected_stages: selected_stages.clone(),
        executed_preprocessing_steps: _executed_preprocessing_steps(
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

    let provenance = PreparedMarketDataProvenance {
        tick_data_summary: serde_json::to_value(&loading_output.tick_data_summary)
            .map_err(|source| format!("failed to serialize tick-data summary: {source}"))?,
        option_surface_summary: loading_output
            .option_surface_summary
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|source| format!("failed to serialize option-surface summary: {source}"))?,
        known_news_summary: loading_output
            .known_news_summary
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|source| format!("failed to serialize known-news summary: {source}"))?,
        normalization_policy: serde_json::to_value(&request.normalization_policy)
            .map_err(|source| format!("failed to serialize normalization policy: {source}"))?,
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
        derivation_policies: vec![
            loading_output.session_resolution.selected_policy_id.clone(),
            PREPARED_MARKET_DATA_DERIVATION_POLICY_ID.to_string(),
            KNOWN_NEWS_WINDOW_FILTER_POLICY_ID.to_string(),
            KNOWN_NEWS_OVERLAY_POLICY_ID.to_string(),
            TRADE_LABEL_DERIVATION_POLICY_ID.to_string(),
            HAWKES_SEARCH_POLICY_ID.to_string(),
            RETURN_COMPONENTS_JUMP_POLICY_ID.to_string(),
            RETURN_COMPONENTS_BOUNDARY_POLICY_ID.to_string(),
            RETURN_COMPONENTS_CONTINUOUS_FIT_POLICY_ID.to_string(),
        ],
        deterministic_defaults_applied: _deterministic_defaults_applied(&request),
    };

    Ok(PreparedMarketDataPlan {
        execution_input,
        diagnostics,
        provenance,
        session_resolution: loading_output.session_resolution,
        fit_warning: loading_output.fit_warning,
        reference_price: loading_output.reference_price,
    })
}

#[pyfunction]
#[pyo3(signature = (preprocessing_request, calendar_registry))]
fn preprocess_market_data_translation_request(
    py: Python<'_>,
    preprocessing_request: &Bound<'_, PyAny>,
    calendar_registry: &PyCalendarRegistry,
) -> PyResult<Py<PyAny>> {
    let preprocessing_request: MarketDataTranslationPreprocessingRequest =
        serde_json::from_value(py_any_to_json_value(preprocessing_request)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize market-data preprocessing request: {source}"
            ))
        })?;

    let preprocessing_output = py
        .allow_threads(|| {
            preprocess_market_data_translation_request_native(
                preprocessing_request,
                &calendar_registry.registry,
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    market_data_translation_preprocessing_output_to_py_object(py, &preprocessing_output).map_err(
        |source| {
            CalibrationExecutionError::new_err(format!(
                "failed to materialize market-data preprocessing output: {source}"
            ))
        },
    )
}

#[pyfunction]
#[pyo3(signature = (loading_request))]
fn load_and_preprocess_market_data_translation_request(
    py: Python<'_>,
    loading_request: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let loading_request: MarketDataLoadingRequest =
        serde_json::from_value(py_any_to_json_value(loading_request)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize market-data loading request: {source}"
            ))
        })?;

    let loading_output = py
        .allow_threads(|| {
            load_and_preprocess_market_data_translation_request_native(loading_request)
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    market_data_loading_output_to_py_object(py, &loading_output).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to materialize market-data loading output: {source}"
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (tick_data_source))]
fn probe_market_data_source(
    py: Python<'_>,
    tick_data_source: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let tick_data_source: MarketDataInputFileReference =
        serde_json::from_value(py_any_to_json_value(tick_data_source)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize tick-data source reference: {source}"
            ))
        })?;

    let source_probe = py
        .allow_threads(|| probe_market_data_source_native(&tick_data_source))
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    market_data_source_probe_to_py_object(py, &source_probe).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to materialize market-data source probe: {source}"
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (tick_data_source, tick_data_column_mapping, normalization_policy, require_hawkes = false))]
fn normalize_market_data_source(
    py: Python<'_>,
    tick_data_source: &Bound<'_, PyAny>,
    tick_data_column_mapping: &Bound<'_, PyAny>,
    normalization_policy: &Bound<'_, PyAny>,
    require_hawkes: bool,
) -> PyResult<Py<PyAny>> {
    let tick_data_source: MarketDataInputFileReference =
        serde_json::from_value(py_any_to_json_value(tick_data_source)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize tick-data source reference: {source}"
            ))
        })?;

    let tick_data_column_mapping: MarketDataTickDataColumnMapping = serde_json::from_value(
        py_any_to_json_value(tick_data_column_mapping)?,
    )
    .map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to deserialize tick-data column mapping: {source}"
        ))
    })?;

    let normalization_policy: MarketDataNormalizationPolicy =
        serde_json::from_value(py_any_to_json_value(normalization_policy)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize market-data normalization policy: {source}"
            ))
        })?;

    let normalized_output = py
        .allow_threads(|| {
            normalize_market_data_source_native(
                &tick_data_source,
                &tick_data_column_mapping,
                &normalization_policy,
                require_hawkes,
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    let result = PyDict::new(py);
    let normalized_handle = Py::new(
        py,
        PyNormalizedMarketDataHandle {
            normalized_output: normalized_output.clone(),
        },
    )?;

    result.set_item("normalized_handle", normalized_handle)?;
    result.set_item(
        "tick_data_summary",
        json_value_to_py_object(
            py,
            &serde_json::to_value(&normalized_output.tick_data_summary).map_err(|source| {
                CalibrationExecutionError::new_err(format!(
                    "failed to serialize normalized tick-data summary: {source}"
                ))
            })?,
        )?,
    )?;
    result.set_item("reference_price", normalized_output.reference_price)?;
    result.set_item("fit_warning", normalized_output.fit_warning)?;

    Ok(result.into())
}

#[pyfunction]
#[pyo3(signature = (session_configuration, normalized_market_data_handle = None))]
fn resolve_market_data_session(
    py: Python<'_>,
    session_configuration: &Bound<'_, PyAny>,
    normalized_market_data_handle: Option<&PyNormalizedMarketDataHandle>,
) -> PyResult<Py<PyAny>> {
    let session_configuration: MarketDataSessionConfiguration =
        serde_json::from_value(py_any_to_json_value(session_configuration)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize market-data session configuration: {source}"
            ))
        })?;

    let session_resolution = py
        .allow_threads(|| {
            resolve_market_data_session_configuration_native(
                session_configuration,
                normalized_market_data_handle.map(|handle| &handle.normalized_output),
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    market_data_session_resolution_to_py_object(py, &session_resolution).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to materialize market-data session resolution: {source}"
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (request, normalized_market_data_handle, session_resolution, cpu_count = None))]
fn prepare_market_data_calibration_plan_from_phases(
    py: Python<'_>,
    request: &Bound<'_, PyAny>,
    normalized_market_data_handle: &PyNormalizedMarketDataHandle,
    session_resolution: &Bound<'_, PyAny>,
    cpu_count: Option<usize>,
) -> PyResult<Py<PyAny>> {
    let request: PythonMarketDataCalibrationRequest =
        serde_json::from_value(py_any_to_json_value(request)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize market-data calibration request: {source}"
            ))
        })?;
    let session_resolution: MarketDataSessionResolutionPayload =
        serde_json::from_value(py_any_to_json_value(session_resolution)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize market-data session resolution payload: {source}"
            ))
        })?;

    let selected_stages = _selected_stages_for_scope(request.workflow_scope);
    let (loading_request, trade_events_data_reference, option_surface_data_reference) =
        _build_loading_request(&request, &selected_stages)
            .map_err(CalibrationExecutionError::new_err)?;

    let loading_output = py
        .allow_threads(|| {
            prepare_market_data_loading_from_phases_native(
                loading_request,
                normalized_market_data_handle.normalized_output.clone(),
                session_resolution,
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    let prepared_plan = _prepared_market_data_plan_from_loading_output(
        request,
        cpu_count,
        selected_stages,
        trade_events_data_reference,
        option_surface_data_reference,
        loading_output,
    )
    .map_err(CalibrationExecutionError::new_err)?;

    prepared_market_data_plan_to_py_object(py, prepared_plan)
}

fn market_data_loading_output_to_py_object(
    py: Python<'_>,
    loading_output: &MarketDataLoadingOutput,
) -> PyResult<Py<PyAny>> {
    let raw_value = serde_json::to_value(loading_output).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize market-data loading output: {source}"
        ))
    })?;

    json_value_to_py_object(py, &raw_value)
}

fn market_data_source_probe_to_py_object(
    py: Python<'_>,
    source_probe: &MarketDataSourceProbePayload,
) -> PyResult<Py<PyAny>> {
    let raw_value = serde_json::to_value(source_probe).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize market-data source probe: {source}"
        ))
    })?;

    json_value_to_py_object(py, &raw_value)
}

fn market_data_session_resolution_to_py_object(
    py: Python<'_>,
    session_resolution: &MarketDataSessionResolutionPayload,
) -> PyResult<Py<PyAny>> {
    let raw_value = serde_json::to_value(session_resolution).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize market-data session resolution: {source}"
        ))
    })?;

    json_value_to_py_object(py, &raw_value)
}

fn market_data_translation_preprocessing_output_to_py_object(
    py: Python<'_>,
    preprocessing_output: &MarketDataTranslationPreprocessingOutput,
) -> PyResult<Py<PyAny>> {
    let raw_value = serde_json::to_value(preprocessing_output).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize market-data preprocessing output: {source}"
        ))
    })?;

    json_value_to_py_object(py, &raw_value)
}

fn prepared_market_data_plan_to_py_object(
    py: Python<'_>,
    prepared_plan: PreparedMarketDataPlan,
) -> PyResult<Py<PyAny>> {
    let result = PyDict::new(py);

    let prepared_handle = Py::new(
        py,
        PyPreparedCalibrationExecutionHandle {
            execution_input: prepared_plan.execution_input.clone(),
        },
    )?;

    result.set_item("prepared_handle", prepared_handle)?;
    result.set_item(
        "staged_request",
        json_value_to_py_object(
            py,
            &serde_json::to_value(&prepared_plan.execution_input.request).map_err(|source| {
                CalibrationExecutionError::new_err(format!(
                    "failed to serialize staged request: {source}"
                ))
            })?,
        )?,
    )?;
    result.set_item(
        "diagnostics",
        json_value_to_py_object(
            py,
            &serde_json::to_value(&prepared_plan.diagnostics).map_err(|source| {
                CalibrationExecutionError::new_err(format!(
                    "failed to serialize preparation diagnostics: {source}"
                ))
            })?,
        )?,
    )?;
    result.set_item(
        "provenance",
        json_value_to_py_object(
            py,
            &serde_json::to_value(&prepared_plan.provenance).map_err(|source| {
                CalibrationExecutionError::new_err(format!(
                    "failed to serialize preparation provenance: {source}"
                ))
            })?,
        )?,
    )?;
    result.set_item(
        "session_resolution",
        market_data_session_resolution_to_py_object(py, &prepared_plan.session_resolution)?,
    )?;
    result.set_item("fit_warning", prepared_plan.fit_warning)?;
    result.set_item("reference_price", prepared_plan.reference_price)?;

    Ok(result.into_any().unbind())
}

#[pyfunction]
#[pyo3(signature = (request, cpu_count = None))]
fn prepare_market_data_calibration_plan(
    py: Python<'_>,
    request: &Bound<'_, PyAny>,
    cpu_count: Option<usize>,
) -> PyResult<Py<PyAny>> {
    let request: PythonMarketDataCalibrationRequest =
        serde_json::from_value(py_any_to_json_value(request)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize market-data calibration request: {source}"
            ))
        })?;

    let prepared_plan = py
        .allow_threads(|| _prepare_market_data_calibration_plan(request, cpu_count))
        .map_err(CalibrationExecutionError::new_err)?;

    prepared_market_data_plan_to_py_object(py, prepared_plan)
}

#[pyfunction]
#[pyo3(signature = (prepared_handle, calendar_registry, memory_checkpoint_store = None))]
fn run_prepared_staged_calibration(
    py: Python<'_>,
    prepared_handle: &PyPreparedCalibrationExecutionHandle,
    calendar_registry: &PyCalendarRegistry,
    memory_checkpoint_store: Option<&mut PyCalibrationMemoryCheckpointStore>,
) -> PyResult<Py<PyAny>> {
    let report = py
        .allow_threads(|| {
            run_staged_calibration_from_execution_input(
                prepared_handle.execution_input.clone(),
                &calendar_registry.registry,
                memory_checkpoint_store.map(|store| &mut store.store),
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    python_staged_calibration_report_to_py_object(py, &report)
}

#[pyfunction]
#[pyo3(signature = (prepared_handle, calendar_registry, event_callback, memory_checkpoint_store = None))]
fn run_prepared_staged_calibration_with_events(
    py: Python<'_>,
    prepared_handle: &PyPreparedCalibrationExecutionHandle,
    calendar_registry: &PyCalendarRegistry,
    event_callback: &Bound<'_, PyAny>,
    memory_checkpoint_store: Option<&mut PyCalibrationMemoryCheckpointStore>,
) -> PyResult<Py<PyAny>> {
    let event_callback = event_callback.clone().unbind();

    let callback_error_message = Arc::new(Mutex::new(None::<String>));
    let callback_error_message_for_observer = Arc::clone(&callback_error_message);

    let report = py
        .allow_threads(move || {
            let mut observer = |event: CalibrationStagedRunObserverEvent| {
                if callback_error_message_for_observer
                    .lock()
                    .ok()
                    .and_then(|guard| guard.clone())
                    .is_some()
                {
                    return;
                }

                Python::with_gil(|py| {
                    let callback_result = python_staged_calibration_event_to_py_object(py, &event)
                        .and_then(|payload| event_callback.call1(py, (payload,)).map(|_| ()));
                    if let Err(source) = callback_result {
                        if let Ok(mut guard) = callback_error_message_for_observer.lock() {
                            if guard.is_none() {
                                *guard = Some(source.to_string());
                            }
                        }
                    }
                });
            };

            run_staged_calibration_from_execution_input_with_observer(
                prepared_handle.execution_input.clone(),
                &calendar_registry.registry,
                memory_checkpoint_store.map(|store| &mut store.store),
                &mut observer,
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    if let Ok(guard) = callback_error_message.lock() {
        if let Some(error_message) = guard.as_ref() {
            return Err(CalibrationExecutionError::new_err(format!(
                "staged calibration event callback failed: {error_message}"
            )));
        }
    }

    python_staged_calibration_report_to_py_object(py, &report)
}

#[pyfunction]
#[pyo3(signature = (execution_input, calendar_registry, memory_checkpoint_store = None))]
fn run_staged_calibration(
    py: Python<'_>,
    execution_input: &Bound<'_, PyAny>,
    calendar_registry: &PyCalendarRegistry,
    memory_checkpoint_store: Option<&mut PyCalibrationMemoryCheckpointStore>,
) -> PyResult<Py<PyAny>> {
    let execution_input: CalibrationStagedExecutionInput =
        serde_json::from_value(py_any_to_json_value(execution_input)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize staged calibration execution input: {source}"
            ))
        })?;

    let report = py
        .allow_threads(move || {
            run_staged_calibration_from_execution_input(
                execution_input,
                &calendar_registry.registry,
                memory_checkpoint_store.map(|store| &mut store.store),
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    python_staged_calibration_report_to_py_object(py, &report)
}

#[pyfunction]
#[pyo3(signature = (execution_input, calendar_registry, event_callback, memory_checkpoint_store = None))]
fn run_staged_calibration_with_events(
    py: Python<'_>,
    execution_input: &Bound<'_, PyAny>,
    calendar_registry: &PyCalendarRegistry,
    event_callback: &Bound<'_, PyAny>,
    memory_checkpoint_store: Option<&mut PyCalibrationMemoryCheckpointStore>,
) -> PyResult<Py<PyAny>> {
    let execution_input: CalibrationStagedExecutionInput =
        serde_json::from_value(py_any_to_json_value(execution_input)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize staged calibration execution input: {source}"
            ))
        })?;

    let event_callback = event_callback.clone().unbind();

    let callback_error_message = Arc::new(Mutex::new(None::<String>));
    let callback_error_message_for_observer = Arc::clone(&callback_error_message);

    let report = py
        .allow_threads(move || {
            let mut observer = |event: CalibrationStagedRunObserverEvent| {
                if callback_error_message_for_observer
                    .lock()
                    .ok()
                    .and_then(|guard| guard.clone())
                    .is_some()
                {
                    return;
                }

                Python::with_gil(|py| {
                    let callback_result = python_staged_calibration_event_to_py_object(py, &event)
                        .and_then(|payload| event_callback.call1(py, (payload,)).map(|_| ()));
                    if let Err(source) = callback_result {
                        if let Ok(mut guard) = callback_error_message_for_observer.lock() {
                            if guard.is_none() {
                                *guard = Some(source.to_string());
                            }
                        }
                    }
                });
            };

            run_staged_calibration_from_execution_input_with_observer(
                execution_input,
                &calendar_registry.registry,
                memory_checkpoint_store.map(|store| &mut store.store),
                &mut observer,
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    if let Ok(guard) = callback_error_message.lock() {
        if let Some(error_message) = guard.as_ref() {
            return Err(CalibrationExecutionError::new_err(format!(
                "staged calibration event callback failed: {error_message}"
            )));
        }
    }

    python_staged_calibration_report_to_py_object(py, &report)
}

#[pyfunction]
#[pyo3(signature = (candidate_generation_input, observed_summary_statistics, fitted_parameters, stage_seed, hawkes_payload = None))]
fn evaluate_return_components_candidate(
    py: Python<'_>,
    candidate_generation_input: &Bound<'_, PyAny>,
    observed_summary_statistics: &Bound<'_, PyAny>,
    fitted_parameters: &Bound<'_, PyAny>,
    stage_seed: u64,
    hawkes_payload: Option<&Bound<'_, PyAny>>,
) -> PyResult<Py<PyAny>> {
    let candidate_generation_input: CalibrationReturnComponentsStageCandidateGenerationInput =
        serde_json::from_value(py_any_to_json_value(candidate_generation_input)?).map_err(
            |source| {
                CalibrationExecutionError::new_err(format!(
                    "failed to deserialize return-components candidate generation input: {source}"
                ))
            },
        )?;

    let observed_summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics =
        serde_json::from_value(py_any_to_json_value(observed_summary_statistics)?).map_err(
            |source| {
                CalibrationExecutionError::new_err(format!(
                    "failed to deserialize return-components observed summary statistics: {source}"
                ))
            },
        )?;

    let fitted_parameters: ReturnComponentsStageFittedParameters =
        serde_json::from_value(py_any_to_json_value(fitted_parameters)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize return-components fitted parameters: {source}"
            ))
        })?;

    let activity_context_payload: Option<CalibrationReturnComponentsActivityContextPayload> =
        hawkes_payload
            .map(py_any_to_json_value)
            .transpose()?
            .map(serde_json::from_value)
            .transpose()
            .map_err(|source| {
                CalibrationExecutionError::new_err(format!(
                    "failed to deserialize Hawkes activity context payload: {source}"
                ))
            })?;

    let evaluation = py
        .allow_threads(|| {
            evaluate_return_components_exact_candidate_from_bridge(
                candidate_generation_input,
                activity_context_payload,
                observed_summary_statistics,
                fitted_parameters,
                stage_seed,
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    let raw_value = serde_json::to_value(evaluation).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize return-components exact candidate evaluation: {source}"
        ))
    })?;

    json_value_to_py_object(py, &raw_value)
}

#[pyfunction]
fn estimate_current_state_from_observed_data(
    py: Python<'_>,
    kernel_parameters: &Bound<'_, PyAny>,
    mean_reversion_fitted_parameters: &Bound<'_, PyAny>,
    state_time_utc_nanos: i64,
    observed_log_price: f64,
    observation_timestamps_utc: &Bound<'_, PyAny>,
    observation_trade_prices: &Bound<'_, PyAny>,
    observation_trade_sides: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let parsed_kernel_parameters: HawkesKernelParameters =
        serde_json::from_value(py_any_to_json_value(kernel_parameters)?).map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to deserialize current-state kernel parameters: {source}"
            ))
        })?;
    let parsed_mean_reversion_fitted_parameters: MeanReversionFittedParameters =
        serde_json::from_value(py_any_to_json_value(mean_reversion_fitted_parameters)?).map_err(
            |source| {
                CalibrationExecutionError::new_err(format!(
                    "failed to deserialize current-state mean-reversion fitted parameters: {source}"
                ))
            },
        )?;
    let parsed_observation_timestamps_utc: Vec<String> =
        observation_timestamps_utc.extract().map_err(|_| {
            CalibrationExecutionError::new_err(
                "current-state observation_timestamps_utc must be an iterable of RFC3339 UTC timestamp strings",
            )
        })?;
    let parsed_observation_trade_prices: Vec<f64> =
        observation_trade_prices.extract().map_err(|_| {
            CalibrationExecutionError::new_err(
                "current-state observation_trade_prices must be an iterable of float values",
            )
        })?;
    let parsed_observation_trade_sides: Vec<String> =
        observation_trade_sides.extract().map_err(|_| {
            CalibrationExecutionError::new_err(
                "current-state observation_trade_sides must be an iterable of trade-side strings",
            )
        })?;

    let parsed_observation_timestamp_utc_nanos = parsed_observation_timestamps_utc
        .iter()
        .enumerate()
        .map(|(observation_index, raw_timestamp_utc)| {
            DateTime::parse_from_rfc3339(raw_timestamp_utc)
                .map(|timestamp| timestamp.with_timezone(&Utc))
                .map_err(|_| {
                    CalibrationExecutionError::new_err(format!(
                        "current-state observation_timestamps_utc[{observation_index}] must be a RFC3339 timestamp with UTC offset, received `{raw_timestamp_utc}`"
                    ))
                })?
                .timestamp_nanos_opt()
                .ok_or_else(|| {
                    CalibrationExecutionError::new_err(format!(
                        "current-state observation_timestamps_utc[{observation_index}] is outside the supported nanosecond timestamp range"
                    ))
                })
        })
        .collect::<PyResult<Vec<_>>>()?;

    let parsed_observation_trade_sides = parsed_observation_trade_sides
        .iter()
        .map(|raw_trade_side: &String| {
            HawkesEventSide::try_from(raw_trade_side.as_str()).map_err(|source| {
                CalibrationExecutionError::new_err(format!(
                    "failed to parse current-state trade_side: {source}"
                ))
            })
        })
        .collect::<PyResult<Vec<_>>>()?;

    let estimation_output =
        estimate_current_state_from_observed_data_core(CurrentStateEstimationInput {
            kernel_parameters: parsed_kernel_parameters,
            mean_reversion_fitted_parameters: parsed_mean_reversion_fitted_parameters,
            state_time_utc_nanos,
            observed_log_price,
            observation_timestamp_utc_nanos: parsed_observation_timestamp_utc_nanos,
            observation_trade_prices: parsed_observation_trade_prices,
            observation_trade_sides: parsed_observation_trade_sides,
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    let serialized_output = serde_json::to_value(&estimation_output).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize current-state estimation output: {source}"
        ))
    })?;

    json_value_to_py_object(py, &serialized_output)
}

#[pyfunction]
#[pyo3(signature = (calendar_registry, exchange_code, range_start_utc, range_end_utc, interval_duration_nanos, intraday_minutes_per_bin, intraday_factor_by_bin, day_of_week_factor, day_of_year_factor))]
fn materialize_runtime_structural_seasonality(
    py: Python<'_>,
    calendar_registry: &PyCalendarRegistry,
    exchange_code: &str,
    range_start_utc: &str,
    range_end_utc: &str,
    interval_duration_nanos: i64,
    intraday_minutes_per_bin: u16,
    intraday_factor_by_bin: &Bound<'_, PyAny>,
    day_of_week_factor: &Bound<'_, PyAny>,
    day_of_year_factor: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let parsed_range_start_utc = parse_timestamp_utc(range_start_utc)?;
    let parsed_range_end_utc = parse_timestamp_utc(range_end_utc)?;
    let parsed_intraday_factor_by_bin: Vec<f64> =
        intraday_factor_by_bin.extract().map_err(|_| {
            CalibrationExecutionError::new_err(
                "runtime seasonality intraday_factor_by_bin must be an iterable of float values",
            )
        })?;
    let parsed_day_of_week_factor: Vec<f64> = day_of_week_factor.extract().map_err(|_| {
        CalibrationExecutionError::new_err(
            "runtime seasonality day_of_week_factor must be an iterable of float values",
        )
    })?;
    let parsed_day_of_year_factor: Vec<f64> = day_of_year_factor.extract().map_err(|_| {
        CalibrationExecutionError::new_err(
            "runtime seasonality day_of_year_factor must be an iterable of float values",
        )
    })?;

    let parsed_range_start_utc_nanos =
        parsed_range_start_utc.timestamp_nanos_opt().ok_or_else(|| {
            CalibrationExecutionError::new_err(
                "runtime seasonality range_start_utc is outside the supported nanosecond timestamp range",
            )
        })?;
    let parsed_range_end_utc_nanos = parsed_range_end_utc.timestamp_nanos_opt().ok_or_else(|| {
        CalibrationExecutionError::new_err(
            "runtime seasonality range_end_utc is outside the supported nanosecond timestamp range",
        )
    })?;

    let materialization = py
        .allow_threads(|| {
            materialize_runtime_structural_seasonality_core(
                &calendar_registry.registry,
                RuntimeStructuralSeasonalityMaterializationInput {
                    exchange_code: exchange_code.to_string(),
                    intraday_minutes_per_bin,
                    intraday_factor_by_bin: parsed_intraday_factor_by_bin,
                    day_of_week_factor: parsed_day_of_week_factor,
                    day_of_year_factor: parsed_day_of_year_factor,
                    range_start_utc_nanos: parsed_range_start_utc_nanos,
                    range_end_utc_nanos: parsed_range_end_utc_nanos,
                    interval_duration_nanos,
                },
            )
        })
        .map_err(|source| CalibrationExecutionError::new_err(source.to_string()))?;

    let serialized_output = serde_json::to_value(&materialization).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize runtime structural seasonality materialization: {source}"
        ))
    })?;

    json_value_to_py_object(py, &serialized_output)
}

fn python_staged_calibration_report_to_py_object(
    py: Python<'_>,
    report: &CalibrationStagedRunReport,
) -> PyResult<Py<PyAny>> {
    let mut serialized_report = match serde_json::to_value(report).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize staged calibration report: {source}"
        ))
    })? {
        Value::Object(object) => object,
        _ => {
            return Err(CalibrationExecutionError::new_err(
                "failed to serialize staged calibration report: expected a JSON object",
            ));
        }
    };

    let flattened_stage_reports = report
        .stage_reports
        .iter()
        .map(flatten_stage_report_for_python)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to flatten staged calibration report for Python: {source}"
            ))
        })?;

    serialized_report.insert(
        "stage_reports".to_string(),
        Value::Array(flattened_stage_reports),
    );

    json_value_to_py_object(py, &Value::Object(serialized_report))
}

fn python_staged_calibration_event_to_py_object(
    py: Python<'_>,
    event: &CalibrationStagedRunObserverEvent,
) -> PyResult<Py<PyAny>> {
    let mut serialized_event = match serde_json::to_value(event).map_err(|source| {
        CalibrationExecutionError::new_err(format!(
            "failed to serialize staged calibration observer event: {source}"
        ))
    })? {
        Value::Object(object) => object,
        _ => {
            return Err(CalibrationExecutionError::new_err(
                "failed to serialize staged calibration observer event: expected a JSON object",
            ));
        }
    };

    if let Some(stage_report) = serialized_event.get("stage_report") {
        let flattened_stage_report = match event {
            CalibrationStagedRunObserverEvent::StageCompleted { stage_report, .. } => {
                flatten_stage_report_for_python(stage_report)
            }
            _ => unreachable!("stage_report field is only emitted for stage_completed events"),
        }
        .map_err(|source| {
            CalibrationExecutionError::new_err(format!(
                "failed to flatten staged calibration observer event stage report: {source}"
            ))
        })?;

        let _ = stage_report;

        serialized_event.insert("stage_report".to_string(), flattened_stage_report);
    }

    json_value_to_py_object(py, &Value::Object(serialized_event))
}

fn flatten_stage_report_for_python(
    report: &CalibrationStagedRunStageReport,
) -> Result<Value, serde_json::Error> {
    let (stage_report_kind, raw_report) = match report {
        CalibrationStagedRunStageReport::Seasonality(stage_report) => {
            ("seasonality", serde_json::to_value(stage_report)?)
        }
        CalibrationStagedRunStageReport::Hawkes(stage_report) => {
            ("hawkes", serde_json::to_value(stage_report)?)
        }
        CalibrationStagedRunStageReport::ReturnComponents(stage_report) => {
            ("return_components", serde_json::to_value(stage_report)?)
        }
        CalibrationStagedRunStageReport::MeanReversion(stage_report) => {
            ("mean_reversion", serde_json::to_value(stage_report)?)
        }
        CalibrationStagedRunStageReport::JointRefinement(stage_report) => {
            ("joint_refinement", serde_json::to_value(stage_report)?)
        }
    };

    let mut flattened_report = match raw_report {
        Value::Object(object) => object,
        _ => {
            return Err(serde_json::Error::io(std::io::Error::other(
                "stage report should serialize to a JSON object",
            )));
        }
    };

    flattened_report.insert(
        "stage_report_kind".to_string(),
        Value::String(stage_report_kind.to_string()),
    );

    Ok(Value::Object(flattened_report))
}

pub(super) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(
        preprocess_market_data_translation_request,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        load_and_preprocess_market_data_translation_request,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(probe_market_data_source, module)?)?;
    module.add_function(wrap_pyfunction!(normalize_market_data_source, module)?)?;
    module.add_function(wrap_pyfunction!(resolve_market_data_session, module)?)?;
    module.add_function(wrap_pyfunction!(
        prepare_market_data_calibration_plan,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        prepare_market_data_calibration_plan_from_phases,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(run_prepared_staged_calibration, module)?)?;
    module.add_function(wrap_pyfunction!(
        run_prepared_staged_calibration_with_events,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(run_staged_calibration, module)?)?;
    module.add_function(wrap_pyfunction!(
        run_staged_calibration_with_events,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        evaluate_return_components_candidate,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        estimate_current_state_from_observed_data,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        materialize_runtime_structural_seasonality,
        module
    )?)?;

    Ok(())
}
