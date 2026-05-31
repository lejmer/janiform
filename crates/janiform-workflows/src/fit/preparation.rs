use std::fs;
use std::path::Path;

use janiform_calibration_artifacts::{CalibrationEffortOptions, CalibrationStageSolverSettings};
use janiform_calibration_runtime::{
    CalibrationDiagnosticsRequest, CalibrationMarketDataCarryInput,
    CalibrationMarketDataColumnMapping, CalibrationMarketDataKnownNewsInput,
    CalibrationMarketDataMeanReversionConfiguration, CalibrationMarketDataOutputPreferences,
    CalibrationMarketDataPreparationRequest, CalibrationMarketDataWorkflowScope,
    CalibrationStagedExecutionInput, CalibrationTickObjectiveDataUsagePolicy,
    DEFAULT_MARKET_DATA_TIMESTAMP_TIMEZONE, MarketDataCoarseDataColumnMapping,
    MarketDataFitInputInferenceRequest, MarketDataFitMarketProfileDefaults,
    MarketDataFitNormalizationOverrides, MarketDataInputFileFormat, MarketDataInputFileReference,
    MarketDataKnownNewsColumnMapping, MarketDataLoadingProgressError,
    MarketDataLoadingProgressEvent, MarketDataNormalizationPolicy,
    MarketDataOptionSurfaceColumnMapping, MarketDataSessionConfiguration,
    MarketDataSessionRecurringClosedWindowPayload, MarketDataSessionRegularWindowPayload,
    MarketDataTickDataColumnMapping, MarketDataTranslationPreprocessingSettings,
    OptionForwardParityWarningPolicy, PriceCoordinatePolicy,
    build_market_data_session_definition_json, infer_bundled_market_profile_from_tick_data_path,
    infer_market_data_fit_inputs, normalize_market_data_source,
    prepare_market_data_calibration_plan, prepare_market_data_calibration_plan_with_progress,
    resolve_market_data_session_configuration, resolve_preset_workflow_configuration,
};
use janiform_engine_pricing::PricingIntervalSchedulePolicy;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

use crate::error::WorkflowError;

use super::input_paths::preparation_request_input_paths;

const MARKET_DATA_REQUEST_SCHEMA_VERSION: &str = "calibration.market_data.request.v1";
const MARKET_DATA_PREPARATION_SCHEMA_VERSION: &str = "calibration.market_data.preparation.v1";
const DEFAULT_KNOWN_NEWS_TIMESTAMP_COLUMN: &str = "event_timestamp_utc";
const DEFAULT_KNOWN_NEWS_IDENTIFIER_COLUMN: &str = "event_id";

/// Request for Rust-owned fit workflow preparation.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FitPreparationRequest {
    /// Explicit run identifier override.
    #[serde(default)]
    pub run_id: Option<String>,
    /// Tick-data source path.
    pub tick_data_path: String,
    /// Explicit tick-data file format.
    #[serde(default)]
    pub tick_data_format: Option<MarketDataInputFileFormat>,
    /// Tick-data provenance label.
    pub tick_data_provenance_label: String,
    /// Optional coarse data source path.
    #[serde(default)]
    pub coarse_data_path: Option<String>,
    /// Coarse data file format.
    #[serde(default = "default_coarse_data_file_format")]
    pub coarse_data_format: MarketDataInputFileFormat,
    /// Coarse data provenance label.
    #[serde(default = "default_coarse_data_provenance_label")]
    pub coarse_data_provenance_label: String,
    /// Optional option-surface source path.
    #[serde(default)]
    pub option_surface_path: Option<String>,
    /// Option-surface file format.
    pub option_surface_format: MarketDataInputFileFormat,
    /// Option-surface provenance label.
    pub option_surface_provenance_label: String,
    /// Option-surface valuation timestamp.
    #[serde(default)]
    pub option_surface_valuation_time_utc: Option<String>,
    /// Flat carry input.
    #[serde(default)]
    pub carry: Option<CalibrationMarketDataCarryInput>,
    /// Preparation configuration.
    pub configuration: FitPreparationConfiguration,
}

/// Configuration for Rust-owned fit workflow preparation.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FitPreparationConfiguration {
    /// Optional workflow-scope override.
    #[serde(default)]
    pub workflow_scope_override: Option<CalibrationMarketDataWorkflowScope>,
    /// Optional tick column mapping overrides.
    #[serde(default)]
    pub tick_data_column_mapping: Option<MarketDataTickDataColumnMapping>,
    /// Optional coarse data column mapping.
    #[serde(default)]
    pub coarse_data_column_mapping: Option<MarketDataCoarseDataColumnMapping>,
    /// Optional normalization policy overrides.
    #[serde(default)]
    pub normalization_overrides: MarketDataFitNormalizationOverrides,
    /// Optional option surface column mapping.
    #[serde(default)]
    pub option_surface_column_mapping: Option<MarketDataOptionSurfaceColumnMapping>,
    /// Price-coordinate policy for option workflows.
    #[serde(default)]
    pub price_coordinate_policy: PriceCoordinatePolicy,
    /// Put-call parity warning policy for option valuation coordinates.
    #[serde(default)]
    pub option_forward_parity_warning_policy: OptionForwardParityWarningPolicy,
    /// Output preferences.
    pub output_preferences: CalibrationMarketDataOutputPreferences,
    /// Optimizer settings by stage.
    #[serde(default)]
    pub optimizer_settings_by_stage: Vec<CalibrationStageSolverSettings>,
    /// Optional preprocessing settings.
    #[serde(default)]
    pub preprocessing_settings: Option<MarketDataTranslationPreprocessingSettings>,
    /// Optional mean-reversion settings.
    #[serde(default)]
    pub mean_reversion: Option<CalibrationMarketDataMeanReversionConfiguration>,
    /// Pricing interval schedule policy.
    #[serde(default)]
    pub pricing_interval_schedule_policy: PricingIntervalSchedulePolicy,
    /// Policy for how much tick evidence slower objectives may consume.
    #[serde(default)]
    pub tick_objective_data_usage_policy: CalibrationTickObjectiveDataUsagePolicy,
    /// Optional dynamics restart budget override.
    #[serde(default)]
    pub dynamics_search_restart_candidate_budget: Option<usize>,
    /// Calibration search effort policy.
    #[serde(default)]
    pub calibration_effort: Option<CalibrationEffortOptions>,
    /// Treat admissibility warnings as hard fit errors.
    #[serde(default)]
    pub treat_calibration_warnings_as_errors: bool,
    /// Optional joint option-refinement candidate count override.
    #[serde(default)]
    pub joint_refinement_option_refinement_candidate_count_override: Option<usize>,
    /// Optional joint option-refinement wall-clock budget in milliseconds.
    #[serde(default)]
    pub joint_refinement_option_refinement_wall_clock_budget_millis: Option<u64>,
    /// Whether audit option pricing is enabled.
    #[serde(default)]
    pub joint_refinement_audit_option_pricing_enabled: bool,
    /// Opt-in extended diagnostics.
    #[serde(default)]
    pub diagnostics: CalibrationDiagnosticsRequest,
    /// Optional stylized-fact Huber cutoff override.
    #[serde(default)]
    pub stylized_fact_huber_cutoff: Option<f64>,
    /// Optional stylized-fact admissibility z-threshold override.
    #[serde(default)]
    pub stylized_fact_admissibility_z_threshold: Option<f64>,
    /// Default mean-reversion configuration.
    pub default_endogenous_mean_reversion_configuration:
        CalibrationMarketDataMeanReversionConfiguration,
    /// Optional market-profile reference.
    #[serde(default)]
    pub market_profile_reference: Option<Value>,
    /// Optional known-news reference.
    #[serde(default)]
    pub known_news_reference: Option<Value>,
    /// Optional explicit session reference.
    #[serde(default)]
    pub session_reference: Option<Value>,
    /// Whether session inference is required.
    pub requires_session_inference: bool,
    /// Exchange code used for automatic session inference.
    #[serde(default)]
    pub exchange_code_for_inference: Option<String>,
    /// Session timezone override.
    #[serde(default)]
    pub session_timezone: Option<String>,
    /// Native preset registry payload.
    pub registry: Value,
    /// Worker count.
    #[serde(default)]
    pub cpu_count: Option<usize>,
}

/// Prepared fit workflow owned by Rust.
#[derive(Debug)]
pub struct PreparedFitWorkflow {
    /// Prepared staged execution input.
    pub execution_input: CalibrationStagedExecutionInput,
    /// Human-readable input paths.
    pub input_paths: Vec<String>,
    /// Optional preparation payload for the fit report.
    pub preparation_payload: Value,
    /// Resolved session definition JSON.
    pub session_definition_json: String,
}

/// Optional callbacks for fit workflow preparation.
#[derive(Default)]
pub(crate) struct FitPreparationObserver<'a> {
    pub(crate) preparation_request_ready: Option<FitPreparationRequestReadySink<'a>>,
    pub(crate) preparation_progress: Option<FitPreparationProgressSink<'a>>,
    pub(crate) market_data_loading_progress: Option<FitPreparationLoadingProgressSink<'a>>,
}

type FitPreparationRequestReadySink<'a> =
    &'a mut dyn FnMut(&CalibrationMarketDataPreparationRequest) -> Result<(), WorkflowError>;

type FitPreparationProgressSink<'a> =
    &'a mut dyn FnMut(FitPreparationProgress) -> Result<(), WorkflowError>;

type FitPreparationLoadingProgressSink<'a> =
    &'a mut dyn FnMut(&str, MarketDataLoadingProgressEvent) -> Result<(), WorkflowError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FitPreparationProgress {
    pub(crate) phase: &'static str,
    pub(crate) latest_signal: &'static str,
}

struct EffectiveMarketProfileReference {
    reference: Option<Value>,
    session_reference: Option<Value>,
    requires_session_inference: bool,
}

struct PreparationRequestBuildInput<'a> {
    request: &'a FitPreparationRequest,
    workflow_scope: CalibrationMarketDataWorkflowScope,
    run_id: &'a str,
    tick_data_source: MarketDataInputFileReference,
    coarse_data_source: Option<MarketDataInputFileReference>,
    option_surface_source: Option<MarketDataInputFileReference>,
    tick_data_column_mapping: MarketDataTickDataColumnMapping,
    normalization_policy: MarketDataNormalizationPolicy,
    resolved_configuration: &'a Value,
    materialization_directory: &'a Path,
}

/// Prepare a fit workflow entirely in Rust.
pub fn prepare_fit_workflow(
    request: FitPreparationRequest,
    working_directory: &Path,
) -> Result<PreparedFitWorkflow, WorkflowError> {
    prepare_fit_workflow_with_observer(
        request,
        working_directory,
        FitPreparationObserver::default(),
    )
}

pub(crate) fn prepare_fit_workflow_with_observer(
    request: FitPreparationRequest,
    working_directory: &Path,
    mut observer: FitPreparationObserver<'_>,
) -> Result<PreparedFitWorkflow, WorkflowError> {
    emit_preparation_progress(
        &mut observer,
        "Resolving workflow inputs",
        "inferring workflow scope and market profile",
    )?;

    let workflow_scope = effective_workflow_scope(
        request.configuration.workflow_scope_override,
        request.option_surface_path.as_deref(),
    );

    let market_profile_reference = resolve_effective_market_profile_reference(
        &request.configuration,
        Path::new(&request.tick_data_path),
    )?;

    let market_profile_defaults = market_profile_defaults(
        &request.configuration.registry,
        market_profile_reference.reference.as_ref(),
    )?;

    emit_preparation_progress(
        &mut observer,
        "Inferring input columns",
        "probing tick data and resolving normalization policy",
    )?;

    let input_inference = infer_market_data_fit_inputs(MarketDataFitInputInferenceRequest {
        tick_data_path: request.tick_data_path.clone(),
        tick_data_format: request.tick_data_format,
        tick_data_provenance_label: request.tick_data_provenance_label.clone(),
        workflow_scope,
        run_id: request.run_id.clone(),
        column_overrides: request
            .configuration
            .tick_data_column_mapping
            .clone()
            .unwrap_or_else(empty_tick_data_column_mapping),
        normalization_overrides: request.configuration.normalization_overrides.clone(),
        market_profile_defaults,
    })
    .map_err(|source| WorkflowError::FitExecutionFailed {
        message: format!("failed to infer fit inputs: {source}"),
    })?;

    let coarse_data_source = coarse_data_source(&request)?;
    let option_surface_source = option_surface_source(&request)?;

    emit_preparation_progress(
        &mut observer,
        "Resolving presets",
        "selecting session and known-news inputs",
    )?;

    let inferred_session_resolution = infer_session_resolution(
        &request,
        workflow_scope,
        &input_inference.tick_data_source,
        &input_inference.tick_data_column_mapping,
        &input_inference.normalization_policy,
        market_profile_reference.requires_session_inference,
    )?;

    let resolved_configuration = resolve_preset_workflow_configuration(json!({
        "registry": request.configuration.registry.clone(),
        "market_profile_reference": market_profile_reference.reference,
        "session_reference": market_profile_reference.session_reference,
        "known_news_reference": request.configuration.known_news_reference.clone(),
        "inferred_session_resolution": inferred_session_resolution,
    }))
    .map_err(|source| WorkflowError::FitExecutionFailed {
        message: format!("failed to resolve preset workflow configuration: {source}"),
    })?;

    let materialization_directory = working_directory.join("resolved-inputs");

    emit_preparation_progress(
        &mut observer,
        "Building market-data request",
        "assembling prepared calibration inputs",
    )?;

    let preparation_request = build_preparation_request(PreparationRequestBuildInput {
        request: &request,
        workflow_scope,
        run_id: &input_inference.run_id,
        tick_data_source: input_inference.tick_data_source.clone(),
        coarse_data_source,
        option_surface_source,
        tick_data_column_mapping: input_inference.tick_data_column_mapping.clone(),
        normalization_policy: input_inference.normalization_policy.clone(),
        resolved_configuration: &resolved_configuration,
        materialization_directory: materialization_directory.as_path(),
    })?;

    if let Some(preparation_request_ready) = observer.preparation_request_ready.as_mut() {
        preparation_request_ready(&preparation_request)?;
    }

    emit_preparation_progress(
        &mut observer,
        "Loading market data",
        "normalizing source records and materializing runtime inputs",
    )?;

    let prepared_plan = if observer.market_data_loading_progress.is_some() {
        let mut loading_progress = |event| {
            emit_market_data_loading_progress(
                &mut observer,
                preparation_request.run_id.as_str(),
                event,
            )
            .map_err(|source| MarketDataLoadingProgressError::observer(source.to_string()))
        };

        prepare_market_data_calibration_plan_with_progress(
            preparation_request.clone(),
            request.configuration.cpu_count,
            &mut loading_progress,
        )
    } else {
        prepare_market_data_calibration_plan(
            preparation_request.clone(),
            request.configuration.cpu_count,
        )
    }
    .map_err(|source| WorkflowError::FitExecutionFailed {
        message: format!("failed to prepare market data: {source}"),
    })?;

    Ok(PreparedFitWorkflow {
        execution_input: prepared_plan.execution_input,
        input_paths: preparation_request_input_paths(&preparation_request),
        preparation_payload: json!({
            "schema_version": MARKET_DATA_PREPARATION_SCHEMA_VERSION,
            "request": preparation_request,
            "diagnostics": prepared_plan.diagnostics,
            "provenance": prepared_plan.provenance,
            "session_resolution": prepared_plan.session_resolution,
            "price_coordinate_resolution": prepared_plan.price_coordinate_resolution,
            "resolved_configuration": resolved_configuration,
            "input_inference": input_inference,
            "reference_price": prepared_plan.reference_price,
            "warnings": prepared_plan.warnings,
            "fit_warning": prepared_plan.fit_warning,
        }),
        session_definition_json: prepared_plan
            .session_resolution
            .resolved_session_definition_json,
    })
}

fn emit_market_data_loading_progress(
    observer: &mut FitPreparationObserver<'_>,
    run_id: &str,
    event: MarketDataLoadingProgressEvent,
) -> Result<(), WorkflowError> {
    let Some(market_data_loading_progress) = observer.market_data_loading_progress.as_mut() else {
        return Ok(());
    };

    market_data_loading_progress(run_id, event)
}

fn effective_workflow_scope(
    workflow_scope_override: Option<CalibrationMarketDataWorkflowScope>,
    option_surface_path: Option<&str>,
) -> CalibrationMarketDataWorkflowScope {
    if let Some(workflow_scope_override) = workflow_scope_override {
        return workflow_scope_override;
    }

    if option_surface_path.is_some() {
        return CalibrationMarketDataWorkflowScope::FullWithOptionSurface;
    }

    CalibrationMarketDataWorkflowScope::FullWithoutOptionSurface
}

fn emit_preparation_progress(
    observer: &mut FitPreparationObserver<'_>,
    phase: &'static str,
    latest_signal: &'static str,
) -> Result<(), WorkflowError> {
    let Some(preparation_progress) = observer.preparation_progress.as_mut() else {
        return Ok(());
    };

    preparation_progress(FitPreparationProgress {
        phase,
        latest_signal,
    })
}

fn resolve_effective_market_profile_reference(
    configuration: &FitPreparationConfiguration,
    tick_data_path: &Path,
) -> Result<EffectiveMarketProfileReference, WorkflowError> {
    if configuration.market_profile_reference.is_some()
        || session_reference_blocks_market_profile_inference(
            configuration.session_reference.as_ref(),
        )
        || configuration.exchange_code_for_inference.is_some()
    {
        return Ok(EffectiveMarketProfileReference {
            reference: configuration.market_profile_reference.clone(),
            session_reference: configuration.session_reference.clone(),
            requires_session_inference: configuration.requires_session_inference,
        });
    }

    let Some(profile) =
        infer_bundled_market_profile_from_tick_data_path(tick_data_path).map_err(|source| {
            WorkflowError::FitExecutionFailed {
                message: format!("failed to infer bundled market profile: {source}"),
            }
        })?
    else {
        if configuration.requires_session_inference {
            return Err(missing_automatic_session_exchange_context_error());
        }

        return Ok(EffectiveMarketProfileReference {
            reference: None,
            session_reference: configuration.session_reference.clone(),
            requires_session_inference: configuration.requires_session_inference,
        });
    };

    Ok(EffectiveMarketProfileReference {
        reference: Some(json!({
            "usage_mode": "explicit",
            "configuration": {
                "profile_name": profile.profile_name,
                "session_reference": {
                    "usage_mode": "named",
                    "registry_scope": "built_in",
                    "preset_name": profile.session_preset_name,
                },
                "known_news_reference": profile.known_news_preset_name.map(|preset_name| json!({
                    "usage_mode": "named",
                    "registry_scope": "built_in",
                    "preset_name": preset_name,
                })),
                "known_news_policy": profile.known_news_policy.unwrap_or_else(|| "suggest_only".to_string()),
                "timestamp_timezone_name": profile.timestamp_timezone_name,
                "column_alias_hints": profile.column_alias_hints,
                "default_calibration_hint_ids": profile.default_calibration_hint_ids,
            },
        })),
        session_reference: None,
        requires_session_inference: false,
    })
}

fn session_reference_blocks_market_profile_inference(session_reference: Option<&Value>) -> bool {
    session_reference
        .and_then(Value::as_object)
        .and_then(|reference| reference.get("usage_mode"))
        .and_then(Value::as_str)
        .is_some_and(|usage_mode| usage_mode != "automatic")
}

fn market_profile_defaults(
    registry: &Value,
    market_profile_reference: Option<&Value>,
) -> Result<Option<MarketDataFitMarketProfileDefaults>, WorkflowError> {
    let Some(market_profile_reference) = market_profile_reference else {
        return Ok(None);
    };

    let resolved_configuration =
        janiform_calibration_runtime::resolve_market_profile_configuration(json!({
            "registry": registry,
            "market_profile_reference": market_profile_reference,
        }))
        .map_err(|source| WorkflowError::FitExecutionFailed {
            message: format!("failed to resolve market-profile defaults: {source}"),
        })?;

    let Some(resolved_configuration) = resolved_configuration else {
        return Ok(None);
    };

    let defaults: MarketProfileDefaultsPayload = serde_json::from_value(resolved_configuration)
        .map_err(|source| WorkflowError::OutputPayloadInvalid {
            message: format!("resolved market-profile defaults were invalid: {source}"),
        })?;

    Ok(Some(MarketDataFitMarketProfileDefaults {
        timestamp_timezone_name: defaults.timestamp_timezone_name,
        column_alias_hints: defaults.column_alias_hints,
        default_calibration_hint_ids: defaults.default_calibration_hint_ids,
    }))
}

#[derive(Debug, Deserialize)]
struct MarketProfileDefaultsPayload {
    #[serde(default)]
    timestamp_timezone_name: Option<String>,
    #[serde(default)]
    column_alias_hints: Vec<janiform_calibration_runtime::MarketDataColumnAliasHint>,
    #[serde(default)]
    default_calibration_hint_ids: Vec<String>,
}

fn option_surface_source(
    request: &FitPreparationRequest,
) -> Result<Option<MarketDataInputFileReference>, WorkflowError> {
    let Some(option_surface_path) = &request.option_surface_path else {
        return Ok(None);
    };

    if option_surface_path.trim().is_empty() {
        return Err(WorkflowError::FitExecutionFailed {
            message: "option_surface_path must be non-empty when provided".to_string(),
        });
    }

    Ok(Some(MarketDataInputFileReference {
        path: option_surface_path.clone(),
        file_format: request.option_surface_format,
        provenance_label: request.option_surface_provenance_label.clone(),
    }))
}

fn default_coarse_data_file_format() -> MarketDataInputFileFormat {
    MarketDataInputFileFormat::Parquet
}

fn default_coarse_data_provenance_label() -> String {
    "coarse_data".to_string()
}

fn coarse_data_source(
    request: &FitPreparationRequest,
) -> Result<Option<MarketDataInputFileReference>, WorkflowError> {
    let Some(coarse_data_path) = &request.coarse_data_path else {
        return Ok(None);
    };

    if coarse_data_path.trim().is_empty() {
        return Err(WorkflowError::FitExecutionFailed {
            message: "coarse_data_path must be non-empty when provided".to_string(),
        });
    }

    Ok(Some(MarketDataInputFileReference {
        path: coarse_data_path.clone(),
        file_format: request.coarse_data_format,
        provenance_label: request.coarse_data_provenance_label.clone(),
    }))
}

fn empty_tick_data_column_mapping() -> MarketDataTickDataColumnMapping {
    MarketDataTickDataColumnMapping {
        timestamp_column: None,
        price_column: None,
        trade_sign_column: None,
        bid_column: None,
        ask_column: None,
    }
}

fn infer_session_resolution(
    request: &FitPreparationRequest,
    workflow_scope: CalibrationMarketDataWorkflowScope,
    tick_data_source: &MarketDataInputFileReference,
    tick_data_column_mapping: &MarketDataTickDataColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
    requires_session_inference: bool,
) -> Result<Option<Value>, WorkflowError> {
    if !requires_session_inference {
        return Ok(None);
    }

    let exchange_code = request
        .configuration
        .exchange_code_for_inference
        .as_deref()
        .ok_or_else(missing_automatic_session_exchange_context_error)?;

    let normalized_market_data = normalize_market_data_source(
        tick_data_source,
        tick_data_column_mapping,
        normalization_policy,
        workflow_scope != CalibrationMarketDataWorkflowScope::SeasonalityOnly,
    )
    .map_err(|source| WorkflowError::FitExecutionFailed {
        message: format!("market-data normalization failed: {source}"),
    })?;

    let session_resolution = resolve_market_data_session_configuration(
        MarketDataSessionConfiguration {
            exchange_code: exchange_code.to_string(),
            session_definition_source: None,
            session_timezone: request.configuration.session_timezone.clone(),
        },
        Some(&normalized_market_data),
    )
    .map_err(|source| WorkflowError::FitValidationFailed {
        field: "session_configuration",
        message: format!("market-data session inference failed: {source}"),
    })?;

    serde_json::to_value(session_resolution)
        .map(Some)
        .map_err(|source| WorkflowError::OutputSerialize { source })
}

fn build_preparation_request(
    input: PreparationRequestBuildInput<'_>,
) -> Result<CalibrationMarketDataPreparationRequest, WorkflowError> {
    let session_configuration = build_session_configuration(
        input.resolved_configuration,
        input.materialization_directory,
    )?;

    let known_news_inputs = build_known_news_inputs(
        input.resolved_configuration,
        input.materialization_directory,
    )?;

    let column_mapping = CalibrationMarketDataColumnMapping {
        tick_data: input.tick_data_column_mapping,
        coarse_data: input
            .request
            .configuration
            .coarse_data_column_mapping
            .clone(),
        option_surface: input
            .request
            .configuration
            .option_surface_column_mapping
            .clone(),
        known_news: known_news_inputs
            .as_ref()
            .map(|_| default_known_news_columns()),
    };

    Ok(CalibrationMarketDataPreparationRequest {
        schema_version: MARKET_DATA_REQUEST_SCHEMA_VERSION.to_string(),
        run_id: input.run_id.to_string(),
        tick_data_source: input.tick_data_source,
        coarse_data_source: input.coarse_data_source,
        session_configuration,
        column_mapping,
        workflow_scope: input.workflow_scope,
        normalization_policy: input.normalization_policy,
        output_preferences: input.request.configuration.output_preferences.clone(),
        optimizer_settings_by_stage: input
            .request
            .configuration
            .optimizer_settings_by_stage
            .clone(),
        preprocessing_settings: input.request.configuration.preprocessing_settings.clone(),
        option_surface_source: input.option_surface_source,
        option_surface_valuation_time_utc: input.request.option_surface_valuation_time_utc.clone(),
        carry: input.request.carry.clone(),
        price_coordinate_policy: input.request.configuration.price_coordinate_policy,
        option_forward_parity_warning_policy: input
            .request
            .configuration
            .option_forward_parity_warning_policy
            .clone(),
        pricing_interval_schedule_policy: input
            .request
            .configuration
            .pricing_interval_schedule_policy
            .clone(),
        tick_objective_data_usage_policy: input
            .request
            .configuration
            .tick_objective_data_usage_policy,
        dynamics_search_restart_candidate_budget: input
            .request
            .configuration
            .dynamics_search_restart_candidate_budget,
        calibration_effort: input.request.configuration.calibration_effort,
        treat_calibration_warnings_as_errors: input
            .request
            .configuration
            .treat_calibration_warnings_as_errors,
        joint_refinement_option_refinement_candidate_count_override: input
            .request
            .configuration
            .joint_refinement_option_refinement_candidate_count_override,
        joint_refinement_option_refinement_wall_clock_budget_millis: input
            .request
            .configuration
            .joint_refinement_option_refinement_wall_clock_budget_millis,
        joint_refinement_audit_option_pricing_enabled: input
            .request
            .configuration
            .joint_refinement_audit_option_pricing_enabled,
        diagnostics: input.request.configuration.diagnostics.clone(),
        stylized_fact_huber_cutoff: input.request.configuration.stylized_fact_huber_cutoff,
        stylized_fact_admissibility_z_threshold: input
            .request
            .configuration
            .stylized_fact_admissibility_z_threshold,
        known_news_inputs,
        mean_reversion: mean_reversion_configuration(input.request, input.workflow_scope),
    })
}

fn build_session_configuration(
    resolved_configuration: &Value,
    materialization_directory: &Path,
) -> Result<MarketDataSessionConfiguration, WorkflowError> {
    let session = required_object(resolved_configuration, "session")?;
    let configuration = required_object_field(session, "configuration")?;
    let exchange_code = required_string(configuration, "exchange_code")?;
    let timezone_name = required_string(configuration, "timezone_name")?;
    let applied_override_fields = string_array(session.get("applied_override_fields"));
    let source_kind = required_string(session, "source_kind")?;

    let inferred_from_data = session
        .get("inferred_from_data")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let can_use_builtin_resolution =
        applied_override_fields.is_empty() && (inferred_from_data || source_kind == "built_in");
    if can_use_builtin_resolution {
        return Ok(MarketDataSessionConfiguration {
            exchange_code: exchange_code.to_string(),
            session_definition_source: None,
            session_timezone: Some(timezone_name.to_string()),
        });
    }

    fs::create_dir_all(materialization_directory).map_err(|source| {
        WorkflowError::FitArtifactWrite {
            path: materialization_directory.to_path_buf(),
            source,
        }
    })?;

    let regular_session_windows = session_windows(configuration, "regular_session_windows")?;

    let recurring_closed_windows =
        recurring_closed_windows(configuration, "recurring_closed_windows")?;

    let exchange_name = configuration
        .get("exchange_name")
        .and_then(Value::as_str)
        .unwrap_or(exchange_code);

    let session_definition_json = build_market_data_session_definition_json(
        exchange_code,
        exchange_name,
        timezone_name,
        regular_session_windows.as_slice(),
        recurring_closed_windows.as_slice(),
    )
    .map_err(|source| WorkflowError::FitExecutionFailed {
        message: format!("failed to materialize session definition: {source}"),
    })?;

    let session_definition_path =
        materialization_directory.join("resolved-session-definition.json");

    fs::write(&session_definition_path, session_definition_json).map_err(|source| {
        WorkflowError::FitArtifactWrite {
            path: session_definition_path.clone(),
            source,
        }
    })?;

    Ok(MarketDataSessionConfiguration {
        exchange_code: exchange_code.to_string(),
        session_definition_source: Some(MarketDataInputFileReference {
            path: session_definition_path.display().to_string(),
            file_format: MarketDataInputFileFormat::Json,
            provenance_label: "resolved-session-configuration".to_string(),
        }),
        session_timezone: Some(timezone_name.to_string()),
    })
}

fn build_known_news_inputs(
    resolved_configuration: &Value,
    materialization_directory: &Path,
) -> Result<Option<CalibrationMarketDataKnownNewsInput>, WorkflowError> {
    let known_news = required_object(resolved_configuration, "known_news")?;
    let configuration = required_object_field(known_news, "configuration")?;

    let Some(events) = configuration.get("events").and_then(Value::as_array) else {
        return Ok(None);
    };
    if events.is_empty() {
        return Ok(None);
    }

    fs::create_dir_all(materialization_directory).map_err(|source| {
        WorkflowError::FitArtifactWrite {
            path: materialization_directory.to_path_buf(),
            source,
        }
    })?;

    let known_news_path = materialization_directory.join("resolved-known-news.csv");

    let mut rows = vec!["event_timestamp_utc,event_id".to_string()];

    for (event_index, event) in events.iter().enumerate() {
        let event = event
            .as_object()
            .ok_or_else(|| WorkflowError::OutputPayloadInvalid {
                message: format!(
                    "known_news.configuration.events[{event_index}] must be an object"
                ),
            })?;

        let timestamp = required_string(event, "scheduled_at")?;
        let event_id = required_string(event, "event_id")?;

        rows.push(format!("{timestamp},{event_id}"));
    }

    fs::write(&known_news_path, rows.join("\n")).map_err(|source| {
        WorkflowError::FitArtifactWrite {
            path: known_news_path.clone(),
            source,
        }
    })?;

    Ok(Some(CalibrationMarketDataKnownNewsInput {
        calendar_source: MarketDataInputFileReference {
            path: known_news_path.display().to_string(),
            file_format: MarketDataInputFileFormat::Csv,
            provenance_label: "resolved-known-news-configuration".to_string(),
        },
        calendar_timezone: configuration
            .get("calendar_timezone_name")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_MARKET_DATA_TIMESTAMP_TIMEZONE)
            .to_string()
            .into(),
    }))
}

fn default_known_news_columns() -> MarketDataKnownNewsColumnMapping {
    MarketDataKnownNewsColumnMapping {
        event_timestamp_column: DEFAULT_KNOWN_NEWS_TIMESTAMP_COLUMN.to_string(),
        event_identifier_column: Some(DEFAULT_KNOWN_NEWS_IDENTIFIER_COLUMN.to_string()),
    }
}

fn mean_reversion_configuration(
    request: &FitPreparationRequest,
    workflow_scope: CalibrationMarketDataWorkflowScope,
) -> Option<CalibrationMarketDataMeanReversionConfiguration> {
    if workflow_scope == CalibrationMarketDataWorkflowScope::SeasonalityOnly {
        return None;
    }

    Some(
        request
            .configuration
            .mean_reversion
            .clone()
            .unwrap_or_else(|| {
                request
                    .configuration
                    .default_endogenous_mean_reversion_configuration
                    .clone()
            }),
    )
}

fn session_windows(
    configuration: &serde_json::Map<String, Value>,
    field_name: &str,
) -> Result<Vec<MarketDataSessionRegularWindowPayload>, WorkflowError> {
    serde_json::from_value(
        configuration
            .get(field_name)
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    )
    .map_err(|source| WorkflowError::OutputPayloadInvalid {
        message: format!("session.configuration.{field_name} was invalid: {source}"),
    })
}

fn recurring_closed_windows(
    configuration: &serde_json::Map<String, Value>,
    field_name: &str,
) -> Result<Vec<MarketDataSessionRecurringClosedWindowPayload>, WorkflowError> {
    serde_json::from_value(
        configuration
            .get(field_name)
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    )
    .map_err(|source| WorkflowError::OutputPayloadInvalid {
        message: format!("session.configuration.{field_name} was invalid: {source}"),
    })
}

fn required_object<'a>(
    value: &'a Value,
    field_name: &str,
) -> Result<&'a serde_json::Map<String, Value>, WorkflowError> {
    value
        .get(field_name)
        .and_then(Value::as_object)
        .ok_or_else(|| WorkflowError::OutputPayloadInvalid {
            message: format!("{field_name} must be an object"),
        })
}

fn required_object_field<'a>(
    value: &'a serde_json::Map<String, Value>,
    field_name: &str,
) -> Result<&'a serde_json::Map<String, Value>, WorkflowError> {
    value
        .get(field_name)
        .and_then(Value::as_object)
        .ok_or_else(|| WorkflowError::OutputPayloadInvalid {
            message: format!("{field_name} must be an object"),
        })
}

fn required_string<'a>(
    value: &'a serde_json::Map<String, Value>,
    field_name: &str,
) -> Result<&'a str, WorkflowError> {
    value
        .get(field_name)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| WorkflowError::OutputPayloadInvalid {
            message: format!("{field_name} must be a non-empty string"),
        })
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn missing_automatic_session_exchange_context_error() -> WorkflowError {
    WorkflowError::FitValidationFailed {
        field: "exchange_code",
        message: "is required for automatic session inference when no session preset, market profile, or native tick-data profile inference provides the exchange baseline".to_string(),
    }
}
