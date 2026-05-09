use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use janiform::calibration::current_state::CurrentStateEstimationBatchProgressEvent;
use janiform::calibration::market_data::{
    MarketDataInputFileReference, MarketDataNormalizationPolicy,
    MarketDataNormalizedOptionSurfaceRecord, MarketDataOptionSurfaceColumnMapping,
    MarketDataOptionSurfaceLoadingInput, MarketDataTickDataColumnMapping,
    MarketDataTranslationTickRecord, infer_market_data_tick_price_input_policy,
    normalize_market_data_source, normalize_option_surface_source,
};
use janiform::model::factors::hourly::NANOS_PER_HOUR;
use janiform::model::hawkes::HawkesEventSide;
use janiform::pricing::{
    AnchoredVanillaOptionPricingBoundaryInput, HighLevelPricingMaturityResolution,
    HighLevelPricingMaturityResolutionRequest, PricingIntervalSchedulePolicy, PricingProgressEvent,
    PricingProgressPhase, VanillaOptionGridBatchPricingExecutionInput,
    VanillaOptionGridBatchPricingRequest, VanillaOptionGridBatchPricingRunResult,
    VanillaOptionGridRequest, VanillaOptionPayoffType, VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection, materialize_schedule_interval_factors,
    resolve_high_level_pricing_maturity_inputs, run_vanilla_option_grid_batch_pricing,
};
use janiform_calibration_runtime::{
    CurrentStateEstimationBatchInput, CurrentStateEstimationOutput,
    estimate_current_states_from_observed_data_with_observer,
};
use janiform_engine_pricing::{VanillaOptionPricingFailureStage, VanillaOptionPricingStatus};
use janiform_foundations::parallelism::resolved_cpu_count;
use janiform_terminal_ui::{
    CompletionSummary, CompletionSummaryStatus, KeyValueRow, ProgressAdapter, ProgressEvent,
    ProgressMeasure, ProgressWorkItemColumn, ProgressWorkItemMetric, StartSummary,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::artifact_io::{ArtifactWriteKind, write_pretty_json_file};
use crate::error::WorkflowError;
use crate::model::{
    build_hourly_factor_contract, fitted_model_context, model_reference_price,
    pricing_nig_shared_plus_shift_configuration, validation_error,
};
use crate::progress::{
    ProgressUnitTracker, completed_tracked_progress_units, format_cpu_count, progress_percent,
    total_tracked_progress_units,
};
use crate::terminal::{TerminalProgressMode, stderr_progress_adapter, terminal_summary_policy};

use super::artifacts::{PricingArtifactPaths, pricing_artifact_paths};
use super::maturity::{
    finalize_maturity_resolution, runtime_schedule_hourly_factor_contract_coverage,
};
use super::progress::{
    complete_pricing_progress, emit_pricing_completion_summary, emit_pricing_start_summary,
    pricing_progress_phase_label,
};
use super::validation::pricing_validation_error;
use super::workflow::PricingWorkflowObserver;

const DEFAULT_PRICING_INTERVAL_DURATION_NANOS: i64 = NANOS_PER_HOUR;
const OBSERVED_PRICING_STAGE_COUNT: usize = 4;
const LOAD_OBSERVED_DATA_STAGE: &str = "load observed data";
const PREPARE_VALUATION_STATES_STAGE: &str = "prepare valuation states";
const PRICE_VALUATION_SURFACE_STAGE: &str = "price valuation surface";
const WRITE_ARTIFACTS_STAGE: &str = "write artifacts";

/// Observed-options pricing scope.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ObservedOptionsPricingScope {
    /// Price the latest observed snapshot at one valuation time.
    Snapshot,
    /// Price a surface panel for one expiry, or fall back to a one-time snapshot for multiple expiries.
    Surface,
}

/// Observed option type filter.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ObservedOptionTypeFilter {
    Call,
    Put,
}

impl ObservedOptionTypeFilter {
    fn payoff_type(self) -> VanillaOptionPayoffType {
        match self {
            Self::Call => VanillaOptionPayoffType::Call,
            Self::Put => VanillaOptionPayoffType::Put,
        }
    }
}

/// Rust-owned observed-options pricing request.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedOptionsPricingRequest {
    /// Persisted `JaniformModel` payload.
    pub model_payload: Value,
    /// Optional stable pricing run id.
    pub run_id: Option<String>,
    /// Observed tick source.
    pub tick_data_source: MarketDataInputFileReference,
    /// Tick source column mapping.
    pub tick_data_column_mapping: MarketDataTickDataColumnMapping,
    /// Observed option surface source.
    pub option_surface_source: MarketDataInputFileReference,
    /// Option source column mapping.
    pub option_surface_column_mapping: MarketDataOptionSurfaceColumnMapping,
    /// Shared market-data normalization policy.
    pub normalization_policy: MarketDataNormalizationPolicy,
    /// Optional valuation timestamp. Required when snapshot inputs contain more than one quote time.
    #[serde(default, deserialize_with = "deserialize_optional_utc_datetime")]
    pub valuation_time_utc: Option<DateTime<Utc>>,
    /// Optional option type filter.
    pub option_type: Option<ObservedOptionTypeFilter>,
    /// Pricing workflow scope.
    pub scope: ObservedOptionsPricingScope,
    /// Continuously compounded risk-free rate.
    pub risk_free_rate: f64,
    /// Continuously compounded dividend yield.
    pub dividend_yield: f64,
    /// Surface valuation count selection.
    pub valuation_count: Option<usize>,
    /// Include every retained valuation time for single-expiry surface pricing.
    #[serde(default)]
    pub use_all_valuation_times: bool,
    /// Optional pricer CPU count.
    pub cpu_count: Option<usize>,
    /// Optional pricing output selections.
    #[serde(default)]
    pub optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    /// Pricing interval schedule policy.
    #[serde(default)]
    pub pricing_interval_schedule_policy: PricingIntervalSchedulePolicy,
    /// Primary output path.
    pub output_path: Option<PathBuf>,
}

/// Completed observed-options pricing workflow.
#[derive(Debug, Serialize)]
pub struct ObservedOptionsPricingOutput {
    pub schema_version: &'static str,
    pub run_id: String,
    pub scope: ObservedOptionsPricingScope,
    #[serde(serialize_with = "serialize_utc_datetime")]
    pub valuation_time_utc: DateTime<Utc>,
    pub nodes: Vec<ObservedOptionsPricingNode>,
    pub snapshot_results: Vec<ObservedOptionsSnapshotResult>,
    pub artifact_paths: Option<PricingArtifactPaths>,
    pub execution_duration_seconds: f64,
}

impl ObservedOptionsPricingOutput {
    /// Serialize the completion payload for machine output.
    pub fn completion_payload(&self, model_path: &std::path::Path) -> Result<Value, WorkflowError> {
        let response = serde_json::to_value(self)
            .map_err(|source| WorkflowError::OutputSerialize { source })?;

        Ok(json!({
            "schema_version": "janiform.price_observed_options_run.v1",
            "command_id": "price observed-options",
            "status": "completed",
            "run_id": self.run_id,
            "model_path": model_path,
            "primary_output_path": self.artifact_paths.as_ref().map(|paths| &paths.primary_output_path),
            "report_path": self.artifact_paths.as_ref().map(|paths| &paths.report_path),
            "manifest_path": self.artifact_paths.as_ref().map(|paths| &paths.manifest_path),
            "run_directory": self.artifact_paths.as_ref().map(|paths| &paths.run_directory),
            "priced_node_count": self.nodes.iter().filter(|node| node.model_price.is_some()).count(),
            "failed_node_count": self.nodes.iter().filter(|node| node.model_price.is_none()).count(),
            "execution_duration_seconds": self.execution_duration_seconds,
            "response": response,
        }))
    }
}

/// Observed snapshot result.
#[derive(Debug, Serialize)]
pub struct ObservedOptionsSnapshotResult {
    #[serde(serialize_with = "serialize_utc_datetime")]
    pub valuation_time_utc: DateTime<Utc>,
    pub spot: f64,
    pub nodes: Vec<ObservedOptionsPricingNode>,
}

/// Priced observed option node.
#[derive(Clone, Debug, Serialize)]
pub struct ObservedOptionsPricingNode {
    #[serde(serialize_with = "serialize_utc_datetime")]
    pub valuation_time_utc: DateTime<Utc>,
    #[serde(serialize_with = "serialize_utc_datetime")]
    pub quote_timestamp_utc: DateTime<Utc>,
    #[serde(serialize_with = "serialize_utc_datetime")]
    pub expiry_timestamp_utc: DateTime<Utc>,
    pub strike: f64,
    pub option_type: VanillaOptionPayoffType,
    pub risk_free_rate: f64,
    pub dividend_yield: f64,
    pub maturity_interval: f64,
    pub observed_bid_price: Option<f64>,
    pub observed_ask_price: Option<f64>,
    pub observed_mid_price: Option<f64>,
    pub model_price: Option<f64>,
    pub model_implied_volatility: Option<f64>,
    pub implied_volatility_inversion_status:
        Option<janiform::pricing::VanillaOptionImpliedVolatilityInversionStatus>,
    pub pricing_status: VanillaOptionPricingStatus,
    pub pricing_failure_stage: Option<VanillaOptionPricingFailureStage>,
    pub pricing_failure_reason: Option<String>,
    pub forward_price: Option<f64>,
    pub discount_factor: Option<f64>,
    pub boundary_state_norm: Option<f64>,
}

#[derive(Clone)]
struct PreparedSnapshotGroup {
    payoff_type: VanillaOptionPayoffType,
    expiry_timestamp_utc_nanos: i64,
    nodes: Vec<PreparedSnapshotNode>,
}

#[derive(Clone)]
struct PreparedSnapshotNode {
    quote_timestamp_utc_nanos: i64,
    expiry_timestamp_utc_nanos: i64,
    strike: f64,
    option_type: VanillaOptionPayoffType,
    observed_bid_price: Option<f64>,
    observed_ask_price: Option<f64>,
    observed_mid_price: Option<f64>,
}

struct PreparedObservedSnapshot {
    valuation_time_utc_nanos: i64,
    spot: f64,
    estimated_state: CurrentStateEstimationOutput,
    snapshot_groups: Vec<PreparedSnapshotGroup>,
}

struct ObservedTickStateColumns {
    timestamp_utc_nanos: Vec<i64>,
    trade_prices: Vec<f64>,
    trade_sides: Vec<HawkesEventSide>,
}

enum SurfacePricingMessage {
    Started {
        worker_index: usize,
        task_position: usize,
        valuation_time_utc_nanos: i64,
        node_count: usize,
        total_interval_count: usize,
    },
    Progress {
        worker_index: usize,
        event: Box<PricingProgressEvent>,
    },
    Completed {
        worker_index: usize,
        task_position: usize,
        result: Result<ObservedOptionsSnapshotResult, WorkflowError>,
    },
}

/// Run observed-options pricing without terminal progress.
pub fn run_observed_options_pricing(
    request: ObservedOptionsPricingRequest,
) -> Result<ObservedOptionsPricingOutput, WorkflowError> {
    run_observed_options_pricing_with_observer(request, PricingWorkflowObserver::default())
}

/// Run observed-options pricing with shared terminal progress.
pub fn run_observed_options_pricing_with_terminal_progress(
    request: ObservedOptionsPricingRequest,
    progress_mode: TerminalProgressMode,
) -> Result<ObservedOptionsPricingOutput, WorkflowError> {
    let mut progress_adapter = stderr_progress_adapter(progress_mode);

    run_observed_options_pricing_with_observer(
        request,
        PricingWorkflowObserver {
            progress_adapter: progress_adapter.as_mut(),
            terminal_output: terminal_summary_policy(progress_mode),
        },
    )
}

/// Run observed-options pricing with workflow observers.
pub fn run_observed_options_pricing_with_observer(
    request: ObservedOptionsPricingRequest,
    mut observer: PricingWorkflowObserver<'_>,
) -> Result<ObservedOptionsPricingOutput, WorkflowError> {
    validate_request(&request)?;

    let started_at = Instant::now();

    let run_id = request
        .run_id
        // Intentional copy because the output owns the final run id while the
        // borrowed request remains available for workflow preparation.
        .clone()
        .unwrap_or_else(|| observed_pricing_run_id("observed_options"));

    emit_pricing_start_summary(
        &observer.terminal_output,
        observed_options_start_summary(&run_id, &request),
    )?;

    start_observed_pricing_progress(
        &mut observer.progress_adapter,
        &run_id,
        request.output_path.as_deref(),
    )?;

    let load_stage_started_at =
        start_observed_pricing_stage(&mut observer.progress_adapter, 1, LOAD_OBSERVED_DATA_STAGE)?;
    emit_observed_stage_progress(
        &mut observer.progress_adapter,
        "load observed data",
        ProgressMeasure::Unbounded,
        Some("loading observed ticks and option quotes".to_string()),
        ObservedWorkItemUpdate::none(),
    )?;
    let tick_normalization_policy = observed_tick_normalization_policy(
        &request.tick_data_source,
        &request.tick_data_column_mapping,
        &request.normalization_policy,
    )?;
    let tick_output = normalize_market_data_source(
        &request.tick_data_source,
        &request.tick_data_column_mapping,
        &tick_normalization_policy,
        true,
    )
    .map_err(|source| pricing_validation_error("tick_data_source", source.to_string()))?;

    let option_output = normalize_option_surface_source(
        &MarketDataOptionSurfaceLoadingInput {
            // Intentional copies because the calibration-runtime loader owns
            // its source and mapping contract for normalization.
            source: request.option_surface_source.clone(),
            column_mapping: request.option_surface_column_mapping.clone(),
        },
        &request.normalization_policy,
    )
    .map_err(|source| pricing_validation_error("option_surface_source", source.to_string()))?;

    emit_observed_stage_progress(
        &mut observer.progress_adapter,
        "load observed data",
        ProgressMeasure::bounded(1, 1)
            .map_err(|source| WorkflowError::OutputProgress { source })?,
        Some(format!(
            "loaded {} ticks and {} option quotes",
            tick_output.tick_records.len(),
            option_output.option_surface_records.len()
        )),
        ObservedWorkItemUpdate::none(),
    )?;
    complete_observed_pricing_stage(
        &mut observer.progress_adapter,
        LOAD_OBSERVED_DATA_STAGE,
        load_stage_started_at,
    )?;

    let valuation_time_utc_nanos = resolve_effective_valuation_time_utc_nanos(
        request.valuation_time_utc,
        option_output.option_surface_records.as_slice(),
    )?;

    let snapshot_groups = prepare_snapshot_groups(
        option_output.option_surface_records.as_slice(),
        valuation_time_utc_nanos,
        request.option_type,
    )?;

    let snapshot_results = match request.scope {
        ObservedOptionsPricingScope::Snapshot => vec![price_snapshot_groups(
            &request,
            valuation_time_utc_nanos,
            snapshot_groups,
            tick_output.tick_records.as_slice(),
            &mut observer.progress_adapter,
        )?],
        ObservedOptionsPricingScope::Surface => price_surface_groups(
            &request,
            valuation_time_utc_nanos,
            snapshot_groups,
            option_output.option_surface_records.as_slice(),
            tick_output.tick_records.as_slice(),
            &mut observer.progress_adapter,
        )?,
    };

    let execution_duration = started_at.elapsed();

    let mut nodes = Vec::new();

    for snapshot_result in &snapshot_results {
        nodes.extend(snapshot_result.nodes.iter().cloned());
    }

    let write_stage_started_at =
        start_observed_pricing_stage(&mut observer.progress_adapter, 4, WRITE_ARTIFACTS_STAGE)?;
    emit_observed_stage_progress(
        &mut observer.progress_adapter,
        "write artifacts",
        ProgressMeasure::Unbounded,
        Some("writing pricing artifacts".to_string()),
        ObservedWorkItemUpdate::none(),
    )?;

    let resolved_valuation_time_utc =
        DateTime::<Utc>::from_timestamp_nanos(valuation_time_utc_nanos);

    let artifact_paths = write_observed_artifacts_if_requested(
        request.output_path.as_ref(),
        &run_id,
        request.scope,
        resolved_valuation_time_utc,
        nodes.as_slice(),
        &snapshot_results,
        execution_duration,
    )?;

    emit_observed_stage_progress(
        &mut observer.progress_adapter,
        "write artifacts",
        ProgressMeasure::bounded(1, 1)
            .map_err(|source| WorkflowError::OutputProgress { source })?,
        Some(match artifact_paths.as_ref() {
            Some(_) => "wrote pricing artifacts".to_string(),
            None => "no pricing artifact output requested".to_string(),
        }),
        ObservedWorkItemUpdate::none(),
    )?;
    complete_observed_pricing_stage(
        &mut observer.progress_adapter,
        WRITE_ARTIFACTS_STAGE,
        write_stage_started_at,
    )?;

    let output = ObservedOptionsPricingOutput {
        schema_version: "janiform.observed_options_pricing_result.v1",
        run_id,
        scope: request.scope,
        valuation_time_utc: resolved_valuation_time_utc,
        nodes,
        snapshot_results,
        artifact_paths,
        execution_duration_seconds: execution_duration.as_secs_f64(),
    };

    emit_pricing_completion_summary(
        &observer.terminal_output,
        observed_options_completion_summary(&output, execution_duration),
    )?;

    complete_pricing_progress(observer.progress_adapter.as_deref_mut())?;

    Ok(output)
}

fn validate_request(request: &ObservedOptionsPricingRequest) -> Result<(), WorkflowError> {
    if !request.risk_free_rate.is_finite() {
        return Err(pricing_validation_error("risk_free_rate", "must be finite"));
    }

    if !request.dividend_yield.is_finite() {
        return Err(pricing_validation_error("dividend_yield", "must be finite"));
    }

    if let Some(valuation_count) = request.valuation_count
        && valuation_count == 0
    {
        return Err(pricing_validation_error(
            "valuation_count",
            "must be strictly positive when provided",
        ));
    }

    if request.use_all_valuation_times && request.valuation_count.is_some() {
        return Err(pricing_validation_error(
            "valuation_count",
            "cannot be combined with use_all_valuation_times",
        ));
    }

    if let Some(cpu_count) = request.cpu_count
        && cpu_count == 0
    {
        return Err(pricing_validation_error(
            "cpu_count",
            "must be strictly positive when provided",
        ));
    }

    Ok(())
}

fn observed_tick_normalization_policy(
    tick_data_source: &MarketDataInputFileReference,
    tick_data_column_mapping: &MarketDataTickDataColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
) -> Result<MarketDataNormalizationPolicy, WorkflowError> {
    if normalization_policy.tick_price_input_policy.is_some() {
        // Intentional copy because the observed-pricing workflow resolves an
        // effective boundary policy without mutating the request contract.
        return Ok(normalization_policy.clone());
    }

    let inferred_policy =
        infer_market_data_tick_price_input_policy(tick_data_source, tick_data_column_mapping)
            .map_err(|source| pricing_validation_error("tick_data_source", source.to_string()))?
            .ok_or_else(|| {
                pricing_validation_error(
                    "tick_data.price",
                    "could not infer a tick price input policy from the supplied source",
                )
            })?;

    // Intentional copy because the observed-pricing workflow injects a resolved
    // tick price policy while preserving the original request payload.
    let mut resolved_policy = normalization_policy.clone();
    resolved_policy.tick_price_input_policy = Some(inferred_policy);

    Ok(resolved_policy)
}

fn resolve_effective_valuation_time_utc_nanos(
    valuation_time_utc: Option<DateTime<Utc>>,
    option_records: &[MarketDataNormalizedOptionSurfaceRecord],
) -> Result<i64, WorkflowError> {
    if let Some(valuation_time_utc) = valuation_time_utc {
        return valuation_time_utc.timestamp_nanos_opt().ok_or_else(|| {
            pricing_validation_error(
                "valuation_time_utc",
                "timestamp is outside the supported nanosecond range",
            )
        });
    }

    let unique_quote_timestamps = option_records
        .iter()
        .map(|record| record.quote_timestamp_utc_nanos)
        .collect::<BTreeSet<_>>();

    if unique_quote_timestamps.len() == 1 {
        return unique_quote_timestamps
            .first()
            .copied()
            .ok_or_else(|| pricing_validation_error("option_surface", "must contain quotes"));
    }

    Err(pricing_validation_error(
        "valuation_time_utc",
        "is required when option_surface contains more than one quote timestamp",
    ))
}

fn prepare_snapshot_groups(
    option_records: &[MarketDataNormalizedOptionSurfaceRecord],
    valuation_time_utc_nanos: i64,
    option_type: Option<ObservedOptionTypeFilter>,
) -> Result<Vec<PreparedSnapshotGroup>, WorkflowError> {
    let mut latest_by_node = BTreeMap::new();

    for record in option_records {
        if record.quote_timestamp_utc_nanos > valuation_time_utc_nanos {
            continue;
        }

        let Some(node) = prepared_snapshot_node_from_record(record, option_type)? else {
            continue;
        };

        latest_by_node.insert(
            option_record_key(
                node.option_type,
                node.expiry_timestamp_utc_nanos,
                node.strike,
            ),
            node,
        );
    }

    if latest_by_node.is_empty() {
        return Err(pricing_validation_error(
            "valuation_time_utc",
            "could not find option quotes at or before valuation_time_utc",
        ));
    }

    let mut grouped = BTreeMap::<(OptionTypeSortKey, i64), Vec<PreparedSnapshotNode>>::new();

    for node in latest_by_node.into_values() {
        grouped
            .entry((
                option_type_sort_key(node.option_type),
                node.expiry_timestamp_utc_nanos,
            ))
            .or_default()
            .push(node);
    }

    Ok(grouped
        .into_iter()
        .map(|((_option_type, expiry_timestamp_utc_nanos), mut nodes)| {
            nodes.sort_by(|left, right| left.strike.total_cmp(&right.strike));
            PreparedSnapshotGroup {
                payoff_type: nodes[0].option_type,
                expiry_timestamp_utc_nanos,
                nodes,
            }
        })
        .collect())
}

fn price_surface_groups(
    request: &ObservedOptionsPricingRequest,
    valuation_time_utc_nanos: i64,
    snapshot_groups: Vec<PreparedSnapshotGroup>,
    option_records: &[MarketDataNormalizedOptionSurfaceRecord],
    tick_records: &[MarketDataTranslationTickRecord],
    progress_adapter: &mut Option<&mut ProgressAdapter>,
) -> Result<Vec<ObservedOptionsSnapshotResult>, WorkflowError> {
    let expiry_count = snapshot_groups
        .iter()
        .map(|group| group.expiry_timestamp_utc_nanos)
        .collect::<BTreeSet<_>>()
        .len();

    if expiry_count > 1 {
        return Ok(vec![price_snapshot_groups(
            request,
            valuation_time_utc_nanos,
            snapshot_groups,
            tick_records,
            progress_adapter,
        )?]);
    }

    let selected_expiry_timestamp_utc_nanos = snapshot_groups
        .first()
        .ok_or_else(|| pricing_validation_error("option_surface", "snapshot must not be empty"))?
        .expiry_timestamp_utc_nanos;

    let history = selected_expiry_history(
        option_records,
        valuation_time_utc_nanos,
        selected_expiry_timestamp_utc_nanos,
        request.option_type,
    )?;

    let valuation_times = resolve_single_expiry_surface_valuation_times(
        history.as_slice(),
        valuation_time_utc_nanos,
        retained_snapshot_valuation_count(snapshot_groups.as_slice()),
        request.valuation_count,
        request.use_all_valuation_times,
    );

    let prepare_stage_started_at =
        start_observed_pricing_stage(progress_adapter, 2, PREPARE_VALUATION_STATES_STAGE)?;

    let snapshot_specs = prepare_selected_expiry_snapshot_specs_by_valuation_times(
        history.as_slice(),
        valuation_times.as_slice(),
    )?;

    let prepared_snapshots =
        prepare_observed_snapshots(request, snapshot_specs, tick_records, progress_adapter)?;

    complete_observed_pricing_stage(
        progress_adapter,
        PREPARE_VALUATION_STATES_STAGE,
        prepare_stage_started_at,
    )?;

    price_prepared_surface_snapshots_parallel(request, prepared_snapshots, progress_adapter)
}

fn price_snapshot_groups(
    request: &ObservedOptionsPricingRequest,
    valuation_time_utc_nanos: i64,
    snapshot_groups: Vec<PreparedSnapshotGroup>,
    tick_records: &[MarketDataTranslationTickRecord],
    progress_adapter: &mut Option<&mut ProgressAdapter>,
) -> Result<ObservedOptionsSnapshotResult, WorkflowError> {
    let prepare_stage_started_at =
        start_observed_pricing_stage(progress_adapter, 2, PREPARE_VALUATION_STATES_STAGE)?;

    let mut prepared_snapshots = prepare_observed_snapshots(
        request,
        vec![(valuation_time_utc_nanos, snapshot_groups)],
        tick_records,
        progress_adapter,
    )?;

    complete_observed_pricing_stage(
        progress_adapter,
        PREPARE_VALUATION_STATES_STAGE,
        prepare_stage_started_at,
    )?;

    let pricing_stage_started_at =
        start_observed_pricing_stage(progress_adapter, 3, PRICE_VALUATION_SURFACE_STAGE)?;

    let prepared_snapshot = prepared_snapshots.pop().ok_or_else(|| {
        pricing_validation_error("valuation_time_utc", "snapshot was not prepared")
    })?;
    let result = price_prepared_snapshot_with_terminal_progress(
        request,
        prepared_snapshot,
        progress_adapter,
        request.cpu_count,
    )?;

    complete_observed_pricing_stage(
        progress_adapter,
        PRICE_VALUATION_SURFACE_STAGE,
        pricing_stage_started_at,
    )?;

    Ok(result)
}

fn prepare_observed_snapshots(
    request: &ObservedOptionsPricingRequest,
    snapshot_specs: Vec<(i64, Vec<PreparedSnapshotGroup>)>,
    tick_records: &[MarketDataTranslationTickRecord],
    progress_adapter: &mut Option<&mut ProgressAdapter>,
) -> Result<Vec<PreparedObservedSnapshot>, WorkflowError> {
    if snapshot_specs.is_empty() {
        return Err(pricing_validation_error(
            "valuation_time_utc",
            "must include at least one valuation time",
        ));
    }

    let valuation_times = snapshot_specs
        .iter()
        .map(|(valuation_time_utc_nanos, _)| *valuation_time_utc_nanos)
        .collect::<Vec<_>>();
    let final_valuation_time_utc_nanos = valuation_times[valuation_times.len() - 1];
    let final_valuation_time_utc =
        DateTime::<Utc>::from_timestamp_nanos(final_valuation_time_utc_nanos);

    let context = fitted_model_context(
        &request.model_payload,
        final_valuation_time_utc,
        DEFAULT_PRICING_INTERVAL_DURATION_NANOS,
        1,
    )?;
    let reference_price = model_reference_price(&context.model)?;
    let tick_state_columns =
        observed_tick_state_columns(tick_records, final_valuation_time_utc_nanos)?;

    let mut progress_error = None::<WorkflowError>;
    let mut progress_observer = |event: CurrentStateEstimationBatchProgressEvent| {
        if progress_error.is_some() {
            return;
        }

        if let Err(source) = emit_valuation_state_progress(progress_adapter, event) {
            progress_error = Some(source);
        }
    };

    let state_items = estimate_current_states_from_observed_data_with_observer(
        CurrentStateEstimationBatchInput {
            kernel_parameters: context.hawkes_fitted_parameters.kernel_parameters,
            // Intentional copy because the batch estimator owns validated
            // fitted parameters while the workflow context remains reusable.
            mean_reversion_fitted_parameters: context
                .return_components_fitted_parameters
                .mean_reversion_fitted_parameters
                .clone(),
            reference_price,
            state_times_utc_nanos: valuation_times,
            observation_timestamp_utc_nanos: tick_state_columns.timestamp_utc_nanos,
            observation_trade_prices: tick_state_columns.trade_prices,
            observation_trade_sides: tick_state_columns.trade_sides,
        },
        Some(&mut progress_observer),
    )
    .map_err(|source| pricing_validation_error("observation_data", source.to_string()))?;

    if let Some(progress_error) = progress_error {
        return Err(progress_error);
    }

    snapshot_specs
        .into_iter()
        .zip(state_items)
        .map(
            |((valuation_time_utc_nanos, snapshot_groups), state_item)| {
                if valuation_time_utc_nanos != state_item.state_time_utc_nanos {
                    return Err(pricing_validation_error(
                        "valuation_time_utc",
                        "prepared state order did not match valuation order",
                    ));
                }

                Ok(PreparedObservedSnapshot {
                    valuation_time_utc_nanos,
                    spot: state_item.observed_price,
                    estimated_state: state_item.state,
                    snapshot_groups,
                })
            },
        )
        .collect()
}

fn observed_tick_state_columns(
    tick_records: &[MarketDataTranslationTickRecord],
    final_valuation_time_utc_nanos: i64,
) -> Result<ObservedTickStateColumns, WorkflowError> {
    let retained_tick_count = tick_records
        .iter()
        .take_while(|record| record.timestamp_utc_nanos <= final_valuation_time_utc_nanos)
        .count();

    if retained_tick_count == 0 {
        return Err(pricing_validation_error(
            "tick_data",
            "does not contain any observations at or before valuation_time_utc",
        ));
    }

    let mut timestamp_utc_nanos = Vec::with_capacity(retained_tick_count);
    let mut trade_prices = Vec::with_capacity(retained_tick_count);
    let mut trade_sides = Vec::with_capacity(retained_tick_count);

    for record in tick_records.iter().take(retained_tick_count) {
        let price = record.price.ok_or_else(|| {
            pricing_validation_error("tick_data.price", "retained ticks must have a price")
        })?;

        if !price.is_finite() || price <= 0.0 {
            return Err(pricing_validation_error(
                "tick_data.price",
                "retained ticks must have finite, strictly positive prices",
            ));
        }

        let trade_side = match record.trade_sign.as_deref() {
            Some(raw_trade_sign) => {
                HawkesEventSide::try_from(raw_trade_sign).map_err(|source| {
                    pricing_validation_error("tick_data.trade_sign", source.to_string())
                })?
            }
            None => HawkesEventSide::Buy,
        };

        timestamp_utc_nanos.push(record.timestamp_utc_nanos);
        trade_prices.push(price);
        trade_sides.push(trade_side);
    }

    Ok(ObservedTickStateColumns {
        timestamp_utc_nanos,
        trade_prices,
        trade_sides,
    })
}

fn price_prepared_snapshot_with_terminal_progress(
    request: &ObservedOptionsPricingRequest,
    prepared_snapshot: PreparedObservedSnapshot,
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    cpu_count: Option<usize>,
) -> Result<ObservedOptionsSnapshotResult, WorkflowError> {
    let total_nodes = prepared_snapshot_node_count(&prepared_snapshot);

    emit_observed_stage_progress(
        progress_adapter,
        "price valuation surface",
        ProgressMeasure::bounded(0, 1)
            .map_err(|source| WorkflowError::OutputProgress { source })?,
        Some(format!(
            "pricing 1 valuation snapshot with {total_nodes} option nodes"
        )),
        ObservedWorkItemUpdate::none(),
    )?;

    let mut progress_error = None::<WorkflowError>;
    let mut observer = |event: PricingProgressEvent| {
        if progress_error.is_some() {
            return;
        }

        if let Err(source) = emit_single_snapshot_pricing_progress(progress_adapter, &event) {
            progress_error = Some(source);
        }
    };

    let result =
        price_prepared_snapshot(request, prepared_snapshot, Some(&mut observer), cpu_count)?;

    if let Some(progress_error) = progress_error {
        return Err(progress_error);
    }

    emit_observed_stage_progress(
        progress_adapter,
        "price valuation surface",
        ProgressMeasure::bounded(1, 1)
            .map_err(|source| WorkflowError::OutputProgress { source })?,
        Some("priced 1 valuation snapshot".to_string()),
        ObservedWorkItemUpdate::none(),
    )?;

    Ok(result)
}

fn price_prepared_snapshot(
    request: &ObservedOptionsPricingRequest,
    prepared_snapshot: PreparedObservedSnapshot,
    progress_observer: Option<&mut dyn FnMut(PricingProgressEvent)>,
    cpu_count: Option<usize>,
) -> Result<ObservedOptionsSnapshotResult, WorkflowError> {
    let valuation_time_utc =
        DateTime::<Utc>::from_timestamp_nanos(prepared_snapshot.valuation_time_utc_nanos);

    let execution_input = build_grid_batch_execution_input(
        request,
        valuation_time_utc,
        prepared_snapshot.spot,
        &prepared_snapshot.snapshot_groups,
        &prepared_snapshot.estimated_state,
        cpu_count,
    )?;

    let run_result = if let Some(progress_observer) = progress_observer {
        janiform::pricing::run_vanilla_option_grid_batch_pricing_with_observer(
            execution_input,
            Some(progress_observer),
        )
    } else {
        run_vanilla_option_grid_batch_pricing(execution_input)
    }
    .map_err(|source| WorkflowError::PricingExecutionFailed {
        message: source.to_string(),
    })?;

    let nodes = build_priced_nodes(
        valuation_time_utc,
        &prepared_snapshot.snapshot_groups,
        &run_result,
    )?;

    Ok(ObservedOptionsSnapshotResult {
        valuation_time_utc,
        spot: prepared_snapshot.spot,
        nodes,
    })
}

fn price_prepared_surface_snapshots_parallel(
    request: &ObservedOptionsPricingRequest,
    prepared_snapshots: Vec<PreparedObservedSnapshot>,
    progress_adapter: &mut Option<&mut ProgressAdapter>,
) -> Result<Vec<ObservedOptionsSnapshotResult>, WorkflowError> {
    let task_count = prepared_snapshots.len();
    if task_count == 0 {
        return Ok(Vec::new());
    }

    let worker_count = resolved_cpu_count(request.cpu_count).min(task_count.max(1));

    let pricing_stage_started_at =
        start_observed_pricing_stage(progress_adapter, 3, PRICE_VALUATION_SURFACE_STAGE)?;

    if task_count == 1 {
        let prepared_snapshot = prepared_snapshots.into_iter().next().ok_or_else(|| {
            pricing_validation_error("valuation_time_utc", "snapshot was not prepared")
        })?;
        let result = price_prepared_snapshot_with_terminal_progress(
            request,
            prepared_snapshot,
            progress_adapter,
            request.cpu_count,
        )?;

        complete_observed_pricing_stage(
            progress_adapter,
            PRICE_VALUATION_SURFACE_STAGE,
            pricing_stage_started_at,
        )?;

        return Ok(vec![result]);
    }

    if worker_count <= 1 {
        let task_interval_counts =
            resolve_surface_task_interval_counts(request, prepared_snapshots.as_slice())?;
        let results = price_prepared_surface_snapshots_serial(
            request,
            prepared_snapshots,
            task_interval_counts,
            progress_adapter,
            worker_count,
        )?;

        complete_observed_pricing_stage(
            progress_adapter,
            PRICE_VALUATION_SURFACE_STAGE,
            pricing_stage_started_at,
        )?;

        return Ok(results);
    }

    emit_surface_interval_planning(progress_adapter, task_count)?;
    let task_interval_counts =
        resolve_surface_task_interval_counts(request, prepared_snapshots.as_slice())?;

    emit_surface_pricing_plan(
        progress_adapter,
        task_count,
        worker_count,
        &task_interval_counts,
    )?;
    let task_interval_counts = Arc::new(task_interval_counts);

    let task_queue = Arc::new(Mutex::new(
        prepared_snapshots
            .into_iter()
            .enumerate()
            .collect::<VecDeque<_>>(),
    ));
    let (sender, receiver) = mpsc::channel::<SurfacePricingMessage>();

    let results = std::thread::scope(|scope| {
        for worker_index in 0..worker_count {
            let sender = sender.clone();
            let task_queue = Arc::clone(&task_queue);
            // Intentional copy of the shared handle so each worker can read
            // planned interval totals without sharing mutable state.
            let task_interval_counts = Arc::clone(&task_interval_counts);

            scope.spawn(move || {
                loop {
                    let task = {
                        let Ok(mut guard) = task_queue.lock() else {
                            return;
                        };

                        guard.pop_front()
                    };

                    let Some((task_position, prepared_snapshot)) = task else {
                        return;
                    };

                    let node_count = prepared_snapshot_node_count(&prepared_snapshot);
                    let valuation_time_utc_nanos = prepared_snapshot.valuation_time_utc_nanos;
                    let total_interval_count = task_interval_counts
                        .get(task_position)
                        .copied()
                        .unwrap_or(0);

                    if sender
                        .send(SurfacePricingMessage::Started {
                            worker_index,
                            task_position,
                            valuation_time_utc_nanos,
                            node_count,
                            total_interval_count,
                        })
                        .is_err()
                    {
                        return;
                    }

                    let progress_sender = sender.clone();
                    let mut progress_observer = move |event: PricingProgressEvent| {
                        let _ = progress_sender.send(SurfacePricingMessage::Progress {
                            worker_index,
                            event: Box::new(event),
                        });
                    };

                    let result = price_prepared_snapshot(
                        request,
                        prepared_snapshot,
                        Some(&mut progress_observer),
                        // Surface valuation tasks own the outer CPU budget.
                        // Keeping each inner grid pricer single-threaded avoids
                        // nested thread pools competing for the same cores.
                        Some(1),
                    );

                    if sender
                        .send(SurfacePricingMessage::Completed {
                            worker_index,
                            task_position,
                            result,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            });
        }

        drop(sender);

        collect_parallel_surface_pricing_results(
            receiver,
            task_count,
            worker_count,
            task_interval_counts.as_slice(),
            progress_adapter,
        )
    })?;

    complete_observed_pricing_stage(
        progress_adapter,
        PRICE_VALUATION_SURFACE_STAGE,
        pricing_stage_started_at,
    )?;

    Ok(results)
}

fn price_prepared_surface_snapshots_serial(
    request: &ObservedOptionsPricingRequest,
    prepared_snapshots: Vec<PreparedObservedSnapshot>,
    task_interval_counts: Vec<usize>,
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    worker_count: usize,
) -> Result<Vec<ObservedOptionsSnapshotResult>, WorkflowError> {
    let task_count = prepared_snapshots.len();
    let mut results = Vec::with_capacity(task_count);
    let mut task_progress_units =
        ProgressUnitTracker::planned_for_totals(task_interval_counts.as_slice());
    let total_progress_units = total_tracked_progress_units(task_progress_units.as_slice());

    emit_surface_pricing_plan(
        progress_adapter,
        task_count,
        worker_count.max(1),
        task_interval_counts.as_slice(),
    )?;

    for (task_position, prepared_snapshot) in prepared_snapshots.into_iter().enumerate() {
        let node_count = prepared_snapshot_node_count(&prepared_snapshot);
        let valuation_time_utc_nanos = prepared_snapshot.valuation_time_utc_nanos;
        let total_interval_count = task_interval_counts
            .get(task_position)
            .copied()
            .unwrap_or(0);
        let mut worker_state = SurfaceWorkerState::started(
            task_position,
            valuation_time_utc_nanos,
            node_count,
            total_interval_count,
        );

        emit_surface_worker_started(
            progress_adapter,
            SurfaceWorkerProgressInput {
                worker_index: 0,
                task_position,
                task_count,
                completed_progress_units: completed_tracked_progress_units(
                    task_progress_units.as_slice(),
                ),
                total_progress_units,
                worker_count: worker_count.max(1),
                valuation_time_utc_nanos,
                node_count,
                total_interval_count,
            },
        )?;

        let mut progress_error = None::<WorkflowError>;
        let mut progress_observer = |event: PricingProgressEvent| {
            if progress_error.is_some() {
                return;
            }

            worker_state.apply_pricing_event(&event);

            if let Some(task_progress) = task_progress_units.get_mut(task_position) {
                task_progress.record(worker_state.completed_intervals);
            }

            if let Err(source) = emit_surface_worker_progress(
                progress_adapter,
                SurfaceWorkerPricingProgressInput {
                    worker_index: 0,
                    task_count,
                    completed_progress_units: completed_tracked_progress_units(
                        task_progress_units.as_slice(),
                    ),
                    total_progress_units,
                    worker_count: worker_count.max(1),
                    // Intentional copy because progress events carry a point-in-time
                    // worker snapshot while the worker keeps advancing.
                    worker_state: worker_state.clone(),
                },
            ) {
                progress_error = Some(source);
            }
        };

        let result = price_prepared_snapshot(
            request,
            prepared_snapshot,
            Some(&mut progress_observer),
            // Serial fallback represents one worker lane, so each valuation is
            // priced by that lane without nested pricing parallelism.
            Some(1),
        )?;

        if let Some(progress_error) = progress_error {
            return Err(progress_error);
        }

        results.push(result);
        worker_state.complete();

        if let Some(task_progress) = task_progress_units.get_mut(task_position) {
            task_progress.complete();
        }

        emit_surface_worker_completed(
            progress_adapter,
            SurfaceWorkerCompletedInput {
                worker_index: 0,
                task_count,
                completed_task_count: results.len(),
                completed_progress_units: completed_tracked_progress_units(
                    task_progress_units.as_slice(),
                ),
                total_progress_units,
                worker_count: worker_count.max(1),
                worker_state,
            },
        )?;
    }

    Ok(results)
}

#[derive(Clone)]
struct SurfaceWorkerState {
    task_position: usize,
    valuation_time_utc_nanos: i64,
    node_count: usize,
    completed_intervals: usize,
    total_intervals: usize,
    progress_percent: usize,
    interval_units: String,
    row_units: String,
    phase: String,
}

impl SurfaceWorkerState {
    fn started(
        task_position: usize,
        valuation_time_utc_nanos: i64,
        node_count: usize,
        total_intervals: usize,
    ) -> Self {
        Self {
            task_position,
            valuation_time_utc_nanos,
            node_count,
            completed_intervals: 0,
            total_intervals,
            progress_percent: 0,
            interval_units: "-".to_string(),
            row_units: "-".to_string(),
            phase: "starting".to_string(),
        }
    }

    fn apply_pricing_event(&mut self, event: &PricingProgressEvent) {
        if let Some(total_intervals) = event.total_intervals {
            self.total_intervals = total_intervals;
        }

        self.completed_intervals = self
            .completed_intervals
            .max(surface_worker_event_completed_intervals(event));
        self.progress_percent = progress_percent(self.completed_intervals, self.total_intervals);
        self.interval_units = surface_worker_interval_units(event);
        self.row_units = surface_worker_row_units(event);
        self.phase = pricing_progress_phase_label(event.phase).to_string();
    }

    fn complete(&mut self) {
        self.completed_intervals = self.total_intervals;
        self.progress_percent = 100;
        self.interval_units = "-".to_string();
        self.row_units = "-".to_string();
        self.phase = "done".to_string();
    }
}

struct SurfaceWorkerProgressInput {
    worker_index: usize,
    task_position: usize,
    task_count: usize,
    completed_progress_units: usize,
    total_progress_units: usize,
    worker_count: usize,
    valuation_time_utc_nanos: i64,
    node_count: usize,
    total_interval_count: usize,
}

struct SurfaceWorkerPricingProgressInput {
    worker_index: usize,
    task_count: usize,
    completed_progress_units: usize,
    total_progress_units: usize,
    worker_count: usize,
    worker_state: SurfaceWorkerState,
}

struct SurfaceWorkerCompletedInput {
    worker_index: usize,
    task_count: usize,
    completed_task_count: usize,
    completed_progress_units: usize,
    total_progress_units: usize,
    worker_count: usize,
    worker_state: SurfaceWorkerState,
}

fn collect_parallel_surface_pricing_results(
    receiver: mpsc::Receiver<SurfacePricingMessage>,
    task_count: usize,
    worker_count: usize,
    task_interval_counts: &[usize],
    progress_adapter: &mut Option<&mut ProgressAdapter>,
) -> Result<Vec<ObservedOptionsSnapshotResult>, WorkflowError> {
    let mut worker_states = (0..worker_count).map(|_| None).collect::<Vec<_>>();
    let mut task_progress_units = ProgressUnitTracker::planned_for_totals(task_interval_counts);
    let mut completed_task_count = 0usize;
    let mut results = (0..task_count).map(|_| None).collect::<Vec<_>>();
    let total_progress_units = total_tracked_progress_units(task_progress_units.as_slice());

    for message in receiver {
        match message {
            SurfacePricingMessage::Started {
                worker_index,
                task_position,
                valuation_time_utc_nanos,
                node_count,
                total_interval_count,
            } => {
                let Some(worker_state) = worker_states.get_mut(worker_index) else {
                    return Err(pricing_validation_error(
                        "pricing_progress.worker_index",
                        "referenced an unknown worker",
                    ));
                };

                *worker_state = Some(SurfaceWorkerState::started(
                    task_position,
                    valuation_time_utc_nanos,
                    node_count,
                    total_interval_count,
                ));

                emit_surface_worker_started(
                    progress_adapter,
                    SurfaceWorkerProgressInput {
                        worker_index,
                        task_position,
                        task_count,
                        completed_progress_units: completed_tracked_progress_units(
                            task_progress_units.as_slice(),
                        ),
                        total_progress_units,
                        worker_count,
                        valuation_time_utc_nanos,
                        node_count,
                        total_interval_count,
                    },
                )?;
            }
            SurfacePricingMessage::Progress {
                worker_index,
                event,
            } => {
                let worker_state = worker_states
                    .get_mut(worker_index)
                    .and_then(Option::as_mut)
                    .ok_or_else(|| {
                        pricing_validation_error(
                            "pricing_progress.worker_index",
                            "received worker progress before worker start",
                        )
                    })?;
                worker_state.apply_pricing_event(&event);

                let Some(task_progress) = task_progress_units.get_mut(worker_state.task_position)
                else {
                    return Err(pricing_validation_error(
                        "pricing_progress.task_position",
                        "referenced an unknown valuation task",
                    ));
                };

                task_progress.record(worker_state.completed_intervals);

                emit_surface_worker_progress(
                    progress_adapter,
                    SurfaceWorkerPricingProgressInput {
                        worker_index,
                        task_count,
                        completed_progress_units: completed_tracked_progress_units(
                            task_progress_units.as_slice(),
                        ),
                        total_progress_units,
                        worker_count,
                        // Intentional copy because progress events carry a point-in-time
                        // worker snapshot while the worker keeps advancing.
                        worker_state: worker_state.clone(),
                    },
                )?;
            }
            SurfacePricingMessage::Completed {
                worker_index,
                task_position,
                result,
            } => {
                let worker_state = worker_states
                    .get_mut(worker_index)
                    .and_then(Option::as_mut)
                    .ok_or_else(|| {
                        pricing_validation_error(
                            "pricing_progress.worker_index",
                            "received worker completion before worker start",
                        )
                    })?;
                worker_state.complete();

                let Some(task_progress) = task_progress_units.get_mut(task_position) else {
                    return Err(pricing_validation_error(
                        "pricing_progress.task_position",
                        "referenced an unknown valuation task",
                    ));
                };

                task_progress.complete();

                // Intentional copy because the completion event needs a stable worker
                // snapshot after the mutable worker-state borrow ends.
                let worker_state = worker_state.clone();

                let result = result?;
                completed_task_count += 1;

                let Some(output_slot) = results.get_mut(task_position) else {
                    return Err(pricing_validation_error(
                        "pricing_progress.task_position",
                        "referenced an unknown valuation task",
                    ));
                };

                *output_slot = Some(result);

                emit_surface_worker_completed(
                    progress_adapter,
                    SurfaceWorkerCompletedInput {
                        worker_index,
                        task_count,
                        completed_task_count,
                        completed_progress_units: completed_tracked_progress_units(
                            task_progress_units.as_slice(),
                        ),
                        total_progress_units,
                        worker_count,
                        worker_state,
                    },
                )?;
            }
        }
    }

    if completed_task_count != task_count {
        return Err(WorkflowError::PricingExecutionFailed {
            message: format!(
                "observed surface pricing completed {completed_task_count}/{task_count} valuation tasks"
            ),
        });
    }

    results
        .into_iter()
        .map(|result| {
            result.ok_or_else(|| WorkflowError::PricingExecutionFailed {
                message: "observed surface pricing did not return every valuation task".to_string(),
            })
        })
        .collect()
}

fn build_grid_batch_execution_input(
    request: &ObservedOptionsPricingRequest,
    valuation_time_utc: DateTime<Utc>,
    spot: f64,
    snapshot_groups: &[PreparedSnapshotGroup],
    estimated_state: &CurrentStateEstimationOutput,
    cpu_count: Option<usize>,
) -> Result<VanillaOptionGridBatchPricingExecutionInput, WorkflowError> {
    let horizon_interval_count = 1;

    let context = fitted_model_context(
        &request.model_payload,
        valuation_time_utc,
        DEFAULT_PRICING_INTERVAL_DURATION_NANOS,
        horizon_interval_count,
    )?;

    let maturity_resolution = resolve_snapshot_maturity_resolution(
        valuation_time_utc,
        snapshot_groups,
        &request.pricing_interval_schedule_policy,
    )?;

    let (pricing_coverage_start_utc, pricing_coverage_end_utc) =
        runtime_schedule_hourly_factor_contract_coverage(
            valuation_time_utc,
            &maturity_resolution.pricing_interval_schedule,
        )?;

    let pricing_hourly_factor_contract = build_hourly_factor_contract(
        &context,
        pricing_coverage_start_utc,
        pricing_coverage_end_utc,
        DEFAULT_PRICING_INTERVAL_DURATION_NANOS,
    )?;

    let maturity_resolution = finalize_maturity_resolution(
        &request.pricing_interval_schedule_policy,
        maturity_resolution,
        &pricing_hourly_factor_contract,
    )?;

    let reference_price = model_reference_price(&context.model)?;
    let observed_log_price = (spot / reference_price).ln();

    let interval_delta = maturity_resolution
        .pricing_interval_schedule
        .interval_delta_hours(0)
        .ok_or_else(|| {
            pricing_validation_error("runtime_interval_schedule", "must not be empty")
        })?;

    let boundary_input = AnchoredVanillaOptionPricingBoundaryInput::try_new(
        0,
        estimated_state.hawkes_excitation_state,
        context
            .return_components_fitted_parameters
            .mean_reversion_fitted_parameters
            // Intentional copy because the pricing boundary owns a validated
            // mean-reversion contract independent of the shared model context.
            .clone(),
        interval_delta,
        // Intentional copy because the pricing boundary owns the estimated
        // state while prepared snapshots remain available for result assembly.
        estimated_state.mean_reversion_boundary_state.clone(),
    )
    .map_err(|source| pricing_validation_error("boundary_input", source.to_string()))?;

    if !observed_log_price.is_finite() {
        return Err(pricing_validation_error(
            "tick_data.last_price",
            "must produce a finite observed log price",
        ));
    }

    let model_parameters = VanillaOptionPricingModelParameters::try_new(
        context.hawkes_fitted_parameters.kernel_parameters,
        context
            .return_components_fitted_parameters
            .news_process_configuration
            // Intentional copy because pricing model parameters own their
            // native contract after workflow preparation completes.
            .clone(),
        pricing_nig_shared_plus_shift_configuration(&context)?,
    )
    .map_err(|source| pricing_validation_error("model_parameters", source.to_string()))?;

    let interval_factors = materialize_schedule_interval_factors(
        &maturity_resolution.pricing_interval_schedule,
        &pricing_hourly_factor_contract,
    )
    .map_err(|source| pricing_validation_error("runtime_interval_schedule", source.to_string()))?;

    Ok(VanillaOptionGridBatchPricingExecutionInput {
        request: VanillaOptionGridBatchPricingRequest {
            spot,
            grids: snapshot_groups
                .iter()
                .enumerate()
                .map(|(group_index, group)| VanillaOptionGridRequest {
                    payoff_type: group.payoff_type,
                    risk_free_rate: request.risk_free_rate,
                    dividend_yield: request.dividend_yield,
                    strikes: group.nodes.iter().map(|node| node.strike).collect(),
                    maturity_intervals: vec![maturity_resolution.maturity_intervals[group_index]],
                })
                .collect(),
        },
        boundary_input,
        model_parameters,
        runtime_interval_schedule: maturity_resolution.pricing_interval_schedule,
        interval_delta,
        interval_factors,
        optional_output_selection: request.optional_output_selection,
        spectral_collocation_node_count_override: None,
        cpu_count,
    })
}

fn resolve_surface_task_interval_counts(
    request: &ObservedOptionsPricingRequest,
    prepared_snapshots: &[PreparedObservedSnapshot],
) -> Result<Vec<usize>, WorkflowError> {
    prepared_snapshots
        .iter()
        .map(|prepared_snapshot| {
            resolve_prepared_snapshot_interval_count(request, prepared_snapshot)
        })
        .collect()
}

fn resolve_prepared_snapshot_interval_count(
    request: &ObservedOptionsPricingRequest,
    prepared_snapshot: &PreparedObservedSnapshot,
) -> Result<usize, WorkflowError> {
    let valuation_time_utc =
        DateTime::<Utc>::from_timestamp_nanos(prepared_snapshot.valuation_time_utc_nanos);
    let maturity_resolution = resolve_snapshot_maturity_resolution(
        valuation_time_utc,
        &prepared_snapshot.snapshot_groups,
        &request.pricing_interval_schedule_policy,
    )?;

    if !matches!(
        request.pricing_interval_schedule_policy,
        PricingIntervalSchedulePolicy::AutoActivityCompressed
    ) {
        return Ok(maturity_resolution
            .pricing_interval_schedule
            .interval_count());
    }

    let context = fitted_model_context(
        &request.model_payload,
        valuation_time_utc,
        DEFAULT_PRICING_INTERVAL_DURATION_NANOS,
        1,
    )?;

    let (pricing_coverage_start_utc, pricing_coverage_end_utc) =
        runtime_schedule_hourly_factor_contract_coverage(
            valuation_time_utc,
            &maturity_resolution.pricing_interval_schedule,
        )?;

    let pricing_hourly_factor_contract = build_hourly_factor_contract(
        &context,
        pricing_coverage_start_utc,
        pricing_coverage_end_utc,
        DEFAULT_PRICING_INTERVAL_DURATION_NANOS,
    )?;

    let maturity_resolution = finalize_maturity_resolution(
        &request.pricing_interval_schedule_policy,
        maturity_resolution,
        &pricing_hourly_factor_contract,
    )?;

    Ok(maturity_resolution
        .pricing_interval_schedule
        .interval_count())
}

fn resolve_snapshot_maturity_resolution(
    valuation_time_utc: DateTime<Utc>,
    snapshot_groups: &[PreparedSnapshotGroup],
    pricing_interval_schedule_policy: &PricingIntervalSchedulePolicy,
) -> Result<HighLevelPricingMaturityResolution, WorkflowError> {
    let policy = match pricing_interval_schedule_policy {
        PricingIntervalSchedulePolicy::AutoActivityCompressed => {
            PricingIntervalSchedulePolicy::AutoExactSparse
        }
        // Intentional copy because maturity resolution owns the policy value.
        other => other.clone(),
    };

    resolve_high_level_pricing_maturity_inputs(HighLevelPricingMaturityResolutionRequest {
        valuation_time_utc: valuation_time_utc.to_rfc3339(),
        expiry_timestamps_utc: Some(
            snapshot_groups
                .iter()
                .map(|group| {
                    Some(
                        DateTime::<Utc>::from_timestamp_nanos(group.expiry_timestamp_utc_nanos)
                            .to_rfc3339(),
                    )
                })
                .collect(),
        ),
        expiry_dates: None,
        tenors: None,
        pricing_interval_schedule_policy: policy,
    })
    .map_err(|source| pricing_validation_error("maturity_input", source.to_string()))
}

fn build_priced_nodes(
    valuation_time_utc: DateTime<Utc>,
    snapshot_groups: &[PreparedSnapshotGroup],
    run_result: &VanillaOptionGridBatchPricingRunResult,
) -> Result<Vec<ObservedOptionsPricingNode>, WorkflowError> {
    let mut nodes = Vec::new();

    for (group, priced_group) in snapshot_groups.iter().zip(run_result.grids.iter()) {
        let maturity_interval = priced_group
            .maturity_intervals
            .first()
            .copied()
            .ok_or_else(|| {
                validation_error(
                    "pricing_result.maturity_intervals",
                    "must contain one maturity",
                )
            })?;

        for (node_index, prepared_node) in group.nodes.iter().enumerate() {
            nodes.push(ObservedOptionsPricingNode {
                valuation_time_utc,
                quote_timestamp_utc: DateTime::<Utc>::from_timestamp_nanos(
                    prepared_node.quote_timestamp_utc_nanos,
                ),
                expiry_timestamp_utc: DateTime::<Utc>::from_timestamp_nanos(
                    prepared_node.expiry_timestamp_utc_nanos,
                ),
                strike: prepared_node.strike,
                option_type: prepared_node.option_type,
                risk_free_rate: priced_group.risk_free_rate,
                dividend_yield: priced_group.dividend_yield,
                maturity_interval,
                observed_bid_price: prepared_node.observed_bid_price,
                observed_ask_price: prepared_node.observed_ask_price,
                observed_mid_price: prepared_node.observed_mid_price,
                model_price: priced_group.price_grid[0][node_index],
                model_implied_volatility: priced_group.implied_volatility_grid[0][node_index],
                implied_volatility_inversion_status: priced_group
                    .implied_volatility_inversion_status_grid[0][node_index],
                pricing_status: priced_group.pricing_status_grid[0][node_index],
                pricing_failure_stage: priced_group.pricing_failure_stage_grid[0][node_index],
                pricing_failure_reason: priced_group.pricing_failure_reason_grid[0][node_index]
                    // Intentional copy because the observed node owns the
                    // failure reason while the raw grid result is retained.
                    .clone(),
                forward_price: priced_group.forward_price_grid[0][node_index],
                discount_factor: priced_group.discount_factor_grid[0][node_index],
                boundary_state_norm: priced_group.boundary_state_norm_grid[0][node_index],
            });
        }
    }

    Ok(nodes)
}

struct ObservedWorkItemUpdate {
    work_item_label: Option<String>,
    work_item_progress_percent: Option<usize>,
    planned_work_item_labels: Vec<String>,
    work_item_columns: Vec<ProgressWorkItemColumn>,
    work_item_metrics: Vec<ProgressWorkItemMetric>,
}

impl ObservedWorkItemUpdate {
    fn none() -> Self {
        Self {
            work_item_label: None,
            work_item_progress_percent: None,
            planned_work_item_labels: Vec::new(),
            work_item_columns: Vec::new(),
            work_item_metrics: Vec::new(),
        }
    }

    fn planned(
        planned_work_item_labels: Vec<String>,
        work_item_columns: Vec<ProgressWorkItemColumn>,
    ) -> Self {
        Self {
            work_item_label: None,
            work_item_progress_percent: None,
            planned_work_item_labels,
            work_item_columns,
            work_item_metrics: Vec::new(),
        }
    }

    fn active(
        work_item_label: String,
        work_item_progress_percent: usize,
        work_item_columns: Vec<ProgressWorkItemColumn>,
        work_item_metrics: Vec<ProgressWorkItemMetric>,
    ) -> Self {
        Self {
            work_item_label: Some(work_item_label),
            work_item_progress_percent: Some(work_item_progress_percent),
            planned_work_item_labels: Vec::new(),
            work_item_columns,
            work_item_metrics,
        }
    }
}

fn start_observed_pricing_progress(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    run_id: &str,
    output_path: Option<&std::path::Path>,
) -> Result<(), WorkflowError> {
    let Some(progress_adapter) = progress_adapter.as_deref_mut() else {
        return Ok(());
    };

    progress_adapter.observe(ProgressEvent::RunStarted {
        run_id: run_id.to_string(),
        scope: "Observed options".to_string(),
        input_count: 1,
        output_path: output_path.map(janiform_terminal_ui::format_path),
        stage_sequence: vec![
            LOAD_OBSERVED_DATA_STAGE.to_string(),
            PREPARE_VALUATION_STATES_STAGE.to_string(),
            PRICE_VALUATION_SURFACE_STAGE.to_string(),
            WRITE_ARTIFACTS_STAGE.to_string(),
        ],
        completed_stage_count: 0,
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })
}

fn start_observed_pricing_stage(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    stage_number: usize,
    stage: &str,
) -> Result<Instant, WorkflowError> {
    let started_at = Instant::now();

    let Some(progress_adapter) = progress_adapter.as_deref_mut() else {
        return Ok(started_at);
    };

    progress_adapter.observe(ProgressEvent::StageStarted {
        stage: stage.to_string(),
        stage_number,
        stage_count: OBSERVED_PRICING_STAGE_COUNT,
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })?;

    Ok(started_at)
}

fn complete_observed_pricing_stage(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    stage: &str,
    started_at: Instant,
) -> Result<(), WorkflowError> {
    let Some(progress_adapter) = progress_adapter.as_deref_mut() else {
        return Ok(());
    };

    progress_adapter.observe(ProgressEvent::StageCompleted {
        stage: stage.to_string(),
        runtime_seconds: started_at.elapsed().as_secs_f64(),
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })
}

fn emit_observed_stage_progress(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    phase: &str,
    measure: ProgressMeasure,
    latest_signal: Option<String>,
    work_item_update: ObservedWorkItemUpdate,
) -> Result<(), WorkflowError> {
    let Some(progress_adapter) = progress_adapter.as_deref_mut() else {
        return Ok(());
    };

    progress_adapter.observe(ProgressEvent::StageProgress {
        phase: phase.to_string(),
        measure,
        work_item_label: work_item_update.work_item_label,
        work_item_progress_percent: work_item_update.work_item_progress_percent,
        planned_work_item_labels: work_item_update.planned_work_item_labels,
        work_item_columns: work_item_update.work_item_columns,
        work_item_metrics: work_item_update.work_item_metrics,
        latest_signal,
    });

    progress_adapter
        .draw_latest()
        .map_err(|source| WorkflowError::OutputProgress { source })
}

fn emit_valuation_state_progress(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    event: CurrentStateEstimationBatchProgressEvent,
) -> Result<(), WorkflowError> {
    let completed_units = event.consumed_observation_count + event.completed_state_count;
    let total_units = event.total_observation_count + event.total_state_count;

    emit_observed_stage_progress(
        progress_adapter,
        "prepare valuation states",
        ProgressMeasure::bounded(completed_units, total_units)
            .map_err(|source| WorkflowError::OutputProgress { source })?,
        Some(format!(
            "replayed {}/{} ticks, prepared {}/{} valuation states",
            event.consumed_observation_count,
            event.total_observation_count,
            event.completed_state_count,
            event.total_state_count
        )),
        ObservedWorkItemUpdate::none(),
    )
}

fn emit_single_snapshot_pricing_progress(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    event: &PricingProgressEvent,
) -> Result<(), WorkflowError> {
    emit_observed_stage_progress(
        progress_adapter,
        pricing_progress_phase_label(event.phase),
        ProgressMeasure::bounded(event.completed_units, event.total_units)
            .map_err(|source| WorkflowError::OutputProgress { source })?,
        Some(format!(
            "{} {}",
            pricing_progress_phase_label(event.phase),
            pricing_event_units(event)
        )),
        ObservedWorkItemUpdate::none(),
    )
}

fn emit_surface_pricing_plan(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    task_count: usize,
    worker_count: usize,
    task_interval_counts: &[usize],
) -> Result<(), WorkflowError> {
    let task_progress_units = ProgressUnitTracker::planned_for_totals(task_interval_counts);
    let total_progress_units = total_tracked_progress_units(task_progress_units.as_slice());

    emit_observed_stage_progress(
        progress_adapter,
        "price valuation surface",
        ProgressMeasure::bounded(0, total_progress_units)
            .map_err(|source| WorkflowError::OutputProgress { source })?,
        Some(format!(
            "pricing {total_progress_units} intervals across {task_count} valuation times on {}",
            format_cpu_count(worker_count)
        )),
        ObservedWorkItemUpdate::planned(
            surface_worker_labels(worker_count),
            surface_worker_columns(),
        ),
    )
}

fn emit_surface_interval_planning(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    task_count: usize,
) -> Result<(), WorkflowError> {
    emit_observed_stage_progress(
        progress_adapter,
        "price valuation surface",
        ProgressMeasure::Unbounded,
        Some(format!(
            "planning pricing intervals for {task_count} valuation times"
        )),
        ObservedWorkItemUpdate::none(),
    )
}

fn emit_surface_worker_started(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    input: SurfaceWorkerProgressInput,
) -> Result<(), WorkflowError> {
    let worker_state = SurfaceWorkerState::started(
        input.task_position,
        input.valuation_time_utc_nanos,
        input.node_count,
        input.total_interval_count,
    );

    emit_observed_stage_progress(
        progress_adapter,
        "price valuation surface",
        ProgressMeasure::bounded(input.completed_progress_units, input.total_progress_units)
            .map_err(|source| WorkflowError::OutputProgress { source })?,
        Some(format!(
            "pricing task {}/{} with {} intervals on {}",
            input.task_position + 1,
            input.task_count,
            input.total_interval_count,
            format_cpu_count(input.worker_count)
        )),
        ObservedWorkItemUpdate::active(
            worker_label(input.worker_index),
            0,
            surface_worker_columns(),
            surface_worker_metrics(input.task_count, &worker_state),
        ),
    )
}

fn emit_surface_worker_progress(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    input: SurfaceWorkerPricingProgressInput,
) -> Result<(), WorkflowError> {
    emit_observed_stage_progress(
        progress_adapter,
        "price valuation surface",
        ProgressMeasure::bounded(input.completed_progress_units, input.total_progress_units)
            .map_err(|source| WorkflowError::OutputProgress { source })?,
        Some(format!(
            "pricing intervals on {}",
            format_cpu_count(input.worker_count)
        )),
        ObservedWorkItemUpdate::active(
            worker_label(input.worker_index),
            input.worker_state.progress_percent,
            surface_worker_columns(),
            surface_worker_metrics(input.task_count, &input.worker_state),
        ),
    )
}

fn emit_surface_worker_completed(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    input: SurfaceWorkerCompletedInput,
) -> Result<(), WorkflowError> {
    emit_observed_stage_progress(
        progress_adapter,
        "price valuation surface",
        ProgressMeasure::bounded(input.completed_progress_units, input.total_progress_units)
            .map_err(|source| WorkflowError::OutputProgress { source })?,
        Some(format!(
            "priced {}/{} valuation times, {}/{} intervals on {}",
            input.completed_task_count,
            input.task_count,
            input.completed_progress_units,
            input.total_progress_units,
            format_cpu_count(input.worker_count)
        )),
        ObservedWorkItemUpdate::active(
            worker_label(input.worker_index),
            100,
            surface_worker_columns(),
            surface_worker_metrics(input.task_count, &input.worker_state),
        ),
    )
}

fn surface_worker_columns() -> Vec<ProgressWorkItemColumn> {
    vec![
        ProgressWorkItemColumn::new("task", "Task"),
        ProgressWorkItemColumn::new("time", "Time"),
        ProgressWorkItemColumn::new("nodes", "Nodes"),
        ProgressWorkItemColumn::new("interval", "Interval"),
        ProgressWorkItemColumn::new("rows", "Rows"),
        ProgressWorkItemColumn::new("phase", "Phase"),
    ]
}

fn surface_worker_metrics(
    task_count: usize,
    worker_state: &SurfaceWorkerState,
) -> Vec<ProgressWorkItemMetric> {
    vec![
        ProgressWorkItemMetric::new(
            "task",
            format!("{}/{}", worker_state.task_position + 1, task_count),
        ),
        ProgressWorkItemMetric::new(
            "time",
            DateTime::<Utc>::from_timestamp_nanos(worker_state.valuation_time_utc_nanos)
                .to_rfc3339(),
        ),
        ProgressWorkItemMetric::new("nodes", worker_state.node_count.to_string()),
        ProgressWorkItemMetric::new("interval", worker_state.interval_units.clone()),
        ProgressWorkItemMetric::new("rows", worker_state.row_units.clone()),
        ProgressWorkItemMetric::new("phase", worker_state.phase.clone()),
    ]
}

fn surface_worker_labels(worker_count: usize) -> Vec<String> {
    (0..worker_count).map(worker_label).collect()
}

fn worker_label(worker_index: usize) -> String {
    format!("worker {:02}", worker_index + 1)
}

fn pricing_event_units(event: &PricingProgressEvent) -> String {
    format!("{}/{}", event.completed_units, event.total_units)
}

fn surface_worker_event_completed_intervals(event: &PricingProgressEvent) -> usize {
    match event.phase {
        PricingProgressPhase::IntervalProgress => event.completed_units,
        PricingProgressPhase::TransitionRowProgress => event
            .current_interval_index
            .zip(event.total_intervals)
            .map(|(interval_index, total_intervals)| interval_index.min(total_intervals))
            .unwrap_or_default(),
        PricingProgressPhase::StrikePricingProgress
        | PricingProgressPhase::GroupProgress
        | PricingProgressPhase::MaturityProgress => 0,
    }
}

fn surface_worker_interval_units(event: &PricingProgressEvent) -> String {
    match (event.current_interval_index, event.total_intervals) {
        (Some(interval_index), Some(total_intervals)) => {
            format!("{}/{}", interval_index + 1, total_intervals)
        }
        _ => "-".to_string(),
    }
}

fn surface_worker_row_units(event: &PricingProgressEvent) -> String {
    match event.phase {
        PricingProgressPhase::TransitionRowProgress => pricing_event_units(event),
        _ => "-".to_string(),
    }
}

fn prepared_snapshot_node_count(prepared_snapshot: &PreparedObservedSnapshot) -> usize {
    prepared_snapshot
        .snapshot_groups
        .iter()
        .map(|group| group.nodes.len())
        .sum()
}

fn selected_expiry_history(
    option_records: &[MarketDataNormalizedOptionSurfaceRecord],
    valuation_time_utc_nanos: i64,
    selected_expiry_timestamp_utc_nanos: i64,
    option_type: Option<ObservedOptionTypeFilter>,
) -> Result<Vec<PreparedSnapshotNode>, WorkflowError> {
    let mut history = Vec::new();

    for record in option_records {
        if record.quote_timestamp_utc_nanos > valuation_time_utc_nanos {
            continue;
        }

        if record.expiry_timestamp_utc_nanos != selected_expiry_timestamp_utc_nanos {
            continue;
        }

        if let Some(node) = prepared_snapshot_node_from_record(record, option_type)? {
            history.push(node);
        }
    }

    if history.is_empty() {
        return Err(pricing_validation_error(
            "valuation_time_utc",
            "could not find option quotes for the selected expiry at or before valuation_time_utc",
        ));
    }

    history.sort_by(|left, right| {
        left.quote_timestamp_utc_nanos
            .cmp(&right.quote_timestamp_utc_nanos)
            .then_with(|| left.strike.total_cmp(&right.strike))
    });

    Ok(history)
}

fn resolve_single_expiry_surface_valuation_times(
    selected_expiry_history: &[PreparedSnapshotNode],
    valuation_time_utc_nanos: i64,
    retained_snapshot_valuation_count: usize,
    valuation_count: Option<usize>,
    use_all_valuation_times: bool,
) -> Vec<i64> {
    let full_history_valuation_times = selected_expiry_history
        .iter()
        .map(|node| node.quote_timestamp_utc_nanos)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    if use_all_valuation_times {
        return full_history_valuation_times;
    }

    let sample_count = valuation_count
        .unwrap_or(retained_snapshot_valuation_count)
        .min(full_history_valuation_times.len());

    let mut valuation_times = if sample_count == full_history_valuation_times.len() {
        full_history_valuation_times
    } else if sample_count == 1 {
        vec![valuation_time_utc_nanos]
    } else {
        let selected_positions = (0..sample_count)
            .map(|selection_index| {
                rounded_selection_position(
                    selection_index,
                    full_history_valuation_times.len(),
                    sample_count,
                )
            })
            .collect::<BTreeSet<_>>();

        selected_positions
            .into_iter()
            .map(|position| full_history_valuation_times[position])
            .collect()
    };

    if valuation_times.last().copied() != Some(valuation_time_utc_nanos)
        && let Some(last) = valuation_times.last_mut()
    {
        *last = valuation_time_utc_nanos;
    }

    valuation_times
}

fn rounded_selection_position(
    selection_index: usize,
    full_history_count: usize,
    sample_count: usize,
) -> usize {
    let numerator = selection_index * (full_history_count - 1);
    let denominator = sample_count - 1;

    (numerator + denominator / 2) / denominator
}

fn retained_snapshot_valuation_count(snapshot_groups: &[PreparedSnapshotGroup]) -> usize {
    snapshot_groups
        .iter()
        .flat_map(|group| {
            group
                .nodes
                .iter()
                .map(|node| node.quote_timestamp_utc_nanos)
        })
        .collect::<BTreeSet<_>>()
        .len()
}

fn prepare_selected_expiry_snapshot_specs_by_valuation_times(
    history: &[PreparedSnapshotNode],
    valuation_times_utc_nanos: &[i64],
) -> Result<Vec<(i64, Vec<PreparedSnapshotGroup>)>, WorkflowError> {
    let mut snapshot_specs = Vec::with_capacity(valuation_times_utc_nanos.len());
    let mut latest_by_strike = BTreeMap::new();
    let mut history_index = 0usize;

    for &valuation_time_utc_nanos in valuation_times_utc_nanos {
        while history_index < history.len()
            && history[history_index].quote_timestamp_utc_nanos <= valuation_time_utc_nanos
        {
            let node = &history[history_index];

            // Intentional copy because each valuation slice owns the latest
            // node per strike while the selected-expiry history remains reusable.
            latest_by_strike.insert(node.strike.to_bits(), node.clone());
            history_index += 1;
        }

        if latest_by_strike.is_empty() {
            return Err(pricing_validation_error(
                "valuation_time_utc",
                "could not find option quotes for the selected expiry at or before valuation_time_utc",
            ));
        }

        let mut nodes = latest_by_strike.values().cloned().collect::<Vec<_>>();

        nodes.sort_by(|left, right| left.strike.total_cmp(&right.strike));

        snapshot_specs.push((
            valuation_time_utc_nanos,
            vec![PreparedSnapshotGroup {
                payoff_type: nodes[0].option_type,
                expiry_timestamp_utc_nanos: nodes[0].expiry_timestamp_utc_nanos,
                nodes,
            }],
        ));
    }

    Ok(snapshot_specs)
}

fn prepared_snapshot_node_from_record(
    record: &MarketDataNormalizedOptionSurfaceRecord,
    option_type: Option<ObservedOptionTypeFilter>,
) -> Result<Option<PreparedSnapshotNode>, WorkflowError> {
    let payoff_type =
        normalized_option_payoff_type(&record.option_type, "option_surface.option_type")?;

    if let Some(option_type) = option_type
        && payoff_type != option_type.payoff_type()
    {
        return Ok(None);
    }

    validate_strike(record.strike)?;

    Ok(Some(PreparedSnapshotNode {
        quote_timestamp_utc_nanos: record.quote_timestamp_utc_nanos,
        expiry_timestamp_utc_nanos: record.expiry_timestamp_utc_nanos,
        strike: record.strike,
        option_type: payoff_type,
        observed_bid_price: record.bid_price,
        observed_ask_price: record.ask_price,
        observed_mid_price: record.observed_mid_price(),
    }))
}

fn normalized_option_payoff_type(
    raw_value: &str,
    field_path: &'static str,
) -> Result<VanillaOptionPayoffType, WorkflowError> {
    match raw_value {
        "call" => Ok(VanillaOptionPayoffType::Call),
        "put" => Ok(VanillaOptionPayoffType::Put),
        _ => Err(pricing_validation_error(
            field_path,
            "must be normalized to one of: call, put",
        )),
    }
}

fn validate_strike(strike: f64) -> Result<(), WorkflowError> {
    if !strike.is_finite() || strike <= 0.0 {
        return Err(pricing_validation_error(
            "option_surface.strike",
            "must be finite and strictly positive",
        ));
    }

    Ok(())
}

type OptionRecordKey = (OptionTypeSortKey, i64, u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum OptionTypeSortKey {
    Call,
    Put,
}

fn option_type_sort_key(payoff_type: VanillaOptionPayoffType) -> OptionTypeSortKey {
    match payoff_type {
        VanillaOptionPayoffType::Call => OptionTypeSortKey::Call,
        VanillaOptionPayoffType::Put => OptionTypeSortKey::Put,
    }
}

fn option_record_key(
    payoff_type: VanillaOptionPayoffType,
    expiry_timestamp_utc_nanos: i64,
    strike: f64,
) -> OptionRecordKey {
    (
        option_type_sort_key(payoff_type),
        expiry_timestamp_utc_nanos,
        strike.to_bits(),
    )
}

fn write_observed_artifacts_if_requested(
    output_path: Option<&PathBuf>,
    run_id: &str,
    scope: ObservedOptionsPricingScope,
    valuation_time_utc: DateTime<Utc>,
    nodes: &[ObservedOptionsPricingNode],
    snapshot_results: &[ObservedOptionsSnapshotResult],
    execution_duration: Duration,
) -> Result<Option<PricingArtifactPaths>, WorkflowError> {
    let Some(output_path) = output_path else {
        return Ok(None);
    };

    let paths = pricing_artifact_paths(output_path);

    let primary_payload = json!({
        "schema_version": "janiform.observed_options_pricing_result.v1",
        "run_id": run_id,
        "scope": scope,
        "valuation_time_utc": valuation_time_utc.to_rfc3339(),
        "nodes": nodes,
        "snapshot_results": snapshot_results,
        "artifact_paths": null,
        "execution_duration_seconds": execution_duration.as_secs_f64(),
    });

    write_pretty_json_file(
        &paths.primary_output_path,
        &primary_payload,
        ArtifactWriteKind::Pricing,
    )?;

    write_pretty_json_file(
        &paths.report_path,
        &json!({
            "schema_version": "janiform.workflow_report.v1",
            "workflow": "price observed-options",
            "primary_output_path": paths.primary_output_path,
            "snapshot_count": snapshot_results.len(),
            "node_count": snapshot_results.iter().map(|result| result.nodes.len()).sum::<usize>(),
            "execution_duration_seconds": execution_duration.as_secs_f64(),
        }),
        ArtifactWriteKind::Pricing,
    )?;

    write_pretty_json_file(
        &paths.manifest_path,
        &json!({
            "schema_version": "janiform.run_manifest.v1",
            "workflow": "price observed-options",
            "run_id": run_id,
            "run_directory": paths.run_directory,
            "primary_output_path": paths.primary_output_path,
            "report_path": paths.report_path,
        }),
        ArtifactWriteKind::Pricing,
    )?;

    Ok(Some(paths))
}

fn observed_options_start_summary(
    run_id: &str,
    request: &ObservedOptionsPricingRequest,
) -> StartSummary {
    StartSummary {
        title: "Janiform price".to_string(),
        rows: vec![
            KeyValueRow::new("Run", run_id),
            KeyValueRow::new("Scope", "observed options"),
            KeyValueRow::new("Ticks", request.tick_data_source.path.as_str()),
            KeyValueRow::new("Options", request.option_surface_source.path.as_str()),
        ],
    }
}

fn observed_options_completion_summary(
    output: &ObservedOptionsPricingOutput,
    elapsed: Duration,
) -> CompletionSummary {
    let priced_node_count = output
        .nodes
        .iter()
        .filter(|node| node.model_price.is_some())
        .count();

    CompletionSummary {
        status: CompletionSummaryStatus::Succeeded,
        title: format!(
            "Observed option pricing completed in {}",
            janiform_terminal_ui::format_elapsed_label(elapsed)
        ),
        rows: vec![
            KeyValueRow::new("Snapshots", output.snapshot_results.len().to_string()),
            KeyValueRow::new("Priced nodes", priced_node_count.to_string()),
            KeyValueRow::new(
                "Failed nodes",
                output
                    .nodes
                    .len()
                    .saturating_sub(priced_node_count)
                    .to_string(),
            ),
        ],
    }
}

fn observed_pricing_run_id(scope: &str) -> String {
    format!(
        "{scope}_price_{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    )
}

fn deserialize_optional_utc_datetime<'de, D>(
    deserializer: D,
) -> Result<Option<DateTime<Utc>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;

    value
        .map(|raw_value| {
            janiform_foundations::time::parse_utc_datetime(&raw_value)
                .map_err(serde::de::Error::custom)
        })
        .transpose()
}

fn serialize_utc_datetime<S>(value: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&value.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use chrono::{DateTime, Utc};
    use janiform::pricing::{PricingProgressEvent, VanillaOptionPayoffType};
    use janiform_engine_pricing::VanillaOptionPricingStatus;
    use serde_json::json;

    use crate::progress::{
        ProgressUnitTracker, completed_tracked_progress_units, total_tracked_progress_units,
    };

    use super::{
        HawkesEventSide, MarketDataTranslationTickRecord, ObservedOptionsPricingNode,
        ObservedOptionsPricingScope, ObservedOptionsSnapshotResult, PreparedSnapshotNode,
        SurfaceWorkerState, observed_tick_state_columns,
        prepare_selected_expiry_snapshot_specs_by_valuation_times, surface_worker_labels,
        write_observed_artifacts_if_requested,
    };

    #[test]
    fn selected_expiry_snapshot_specs_reuse_incremental_latest_quotes() {
        let history = vec![
            snapshot_node(1, 100, 10.0, 1.0),
            snapshot_node(1, 100, 12.0, 2.0),
            snapshot_node(2, 100, 10.0, 3.0),
            snapshot_node(3, 100, 12.0, 4.0),
        ];

        let snapshot_specs =
            prepare_selected_expiry_snapshot_specs_by_valuation_times(&history, &[1, 2, 3])
                .expect("snapshot specs should be prepared");

        assert_eq!(snapshot_specs.len(), 3);
        assert_eq!(
            snapshot_specs[0].1[0].nodes[0].observed_mid_price,
            Some(1.0)
        );
        assert_eq!(
            snapshot_specs[0].1[0].nodes[1].observed_mid_price,
            Some(2.0)
        );
        assert_eq!(
            snapshot_specs[1].1[0].nodes[0].observed_mid_price,
            Some(3.0)
        );
        assert_eq!(
            snapshot_specs[1].1[0].nodes[1].observed_mid_price,
            Some(2.0)
        );
        assert_eq!(
            snapshot_specs[2].1[0].nodes[0].observed_mid_price,
            Some(3.0)
        );
        assert_eq!(
            snapshot_specs[2].1[0].nodes[1].observed_mid_price,
            Some(4.0)
        );
    }

    #[test]
    fn observed_tick_state_columns_apply_cutoff_and_default_missing_trade_side() {
        let tick_records = vec![
            tick_record(1, 100.0, Some("buy")),
            tick_record(2, 100.5, None),
            tick_record(3, 101.0, Some("sell")),
        ];

        let columns = observed_tick_state_columns(&tick_records, 2)
            .expect("tick state columns should be prepared");

        assert_eq!(columns.timestamp_utc_nanos, vec![1, 2]);
        assert_eq!(columns.trade_prices, vec![100.0, 100.5]);
        assert_eq!(
            columns.trade_sides,
            vec![HawkesEventSide::Buy, HawkesEventSide::Buy]
        );
    }

    #[test]
    fn surface_worker_labels_are_stable_worker_lanes() {
        assert_eq!(
            surface_worker_labels(3),
            vec![
                "worker 01".to_string(),
                "worker 02".to_string(),
                "worker 03".to_string(),
            ]
        );
    }

    #[test]
    fn surface_global_progress_sums_real_interval_progress() {
        let mut task_progress = ProgressUnitTracker::planned_for_totals(&[100, 200, 300]);
        task_progress[0].record(25);
        task_progress[1].record(50);
        task_progress[2].complete();

        assert_eq!(total_tracked_progress_units(&task_progress), 600);
        assert_eq!(completed_tracked_progress_units(&task_progress), 375);
    }

    #[test]
    fn surface_worker_transition_row_progress_does_not_reset_interval_progress() {
        let mut worker_state = SurfaceWorkerState::started(0, 1, 10, 10);
        worker_state.apply_pricing_event(&PricingProgressEvent::interval_progress(
            5,
            10,
            Some(5),
            10,
        ));

        worker_state.apply_pricing_event(&PricingProgressEvent::transition_row_progress(
            12,
            65,
            Some(5),
            Some(10),
        ));

        assert_eq!(worker_state.progress_percent, 50);
        assert_eq!(worker_state.interval_units, "6/10");
        assert_eq!(worker_state.row_units, "12/65");
    }

    #[test]
    fn observed_primary_artifact_matches_native_result_shape() {
        let valuation_time_utc = DateTime::<Utc>::from_timestamp_nanos(1_000_000_000);
        let output_directory = std::env::temp_dir().join(format!(
            "janiform-observed-pricing-artifact-test-{}",
            std::process::id()
        ));
        let output_path = output_directory.join("observed_pricing.json");
        let node = observed_pricing_node(valuation_time_utc);
        let snapshot_result = ObservedOptionsSnapshotResult {
            valuation_time_utc,
            spot: 100.0,
            nodes: vec![node.clone()],
        };

        let paths = write_observed_artifacts_if_requested(
            Some(&output_path),
            "observed-options-test",
            ObservedOptionsPricingScope::Surface,
            valuation_time_utc,
            &[node],
            &[snapshot_result],
            Duration::from_secs(2),
        )
        .expect("observed artifact should be writable")
        .expect("artifact paths should be returned");
        let primary_payload: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&paths.primary_output_path)
                .expect("primary observed artifact should be readable"),
        )
        .expect("primary observed artifact should be valid JSON");

        fs::remove_dir_all(&output_directory).expect("test artifact directory should be removable");

        assert_eq!(
            primary_payload["schema_version"],
            json!("janiform.observed_options_pricing_result.v1")
        );
        assert_eq!(primary_payload["run_id"], json!("observed-options-test"));
        assert_eq!(primary_payload["scope"], json!("surface"));
        assert_eq!(
            primary_payload["valuation_time_utc"],
            json!(valuation_time_utc.to_rfc3339())
        );
        assert_eq!(primary_payload["nodes"].as_array().map(Vec::len), Some(1));
        assert_eq!(
            primary_payload["snapshot_results"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(primary_payload["artifact_paths"], json!(null));
        assert_eq!(primary_payload["execution_duration_seconds"], json!(2.0));
    }

    fn snapshot_node(
        quote_timestamp_utc_nanos: i64,
        expiry_timestamp_utc_nanos: i64,
        strike: f64,
        observed_mid_price: f64,
    ) -> PreparedSnapshotNode {
        PreparedSnapshotNode {
            quote_timestamp_utc_nanos,
            expiry_timestamp_utc_nanos,
            strike,
            option_type: VanillaOptionPayoffType::Call,
            observed_bid_price: None,
            observed_ask_price: None,
            observed_mid_price: Some(observed_mid_price),
        }
    }

    fn observed_pricing_node(valuation_time_utc: DateTime<Utc>) -> ObservedOptionsPricingNode {
        ObservedOptionsPricingNode {
            valuation_time_utc,
            quote_timestamp_utc: valuation_time_utc,
            expiry_timestamp_utc: DateTime::<Utc>::from_timestamp_nanos(3_600_000_000_000),
            strike: 100.0,
            option_type: VanillaOptionPayoffType::Call,
            risk_free_rate: 0.0,
            dividend_yield: 0.0,
            maturity_interval: 1.0,
            observed_bid_price: Some(1.0),
            observed_ask_price: Some(1.2),
            observed_mid_price: Some(1.1),
            model_price: Some(1.05),
            model_implied_volatility: Some(0.2),
            implied_volatility_inversion_status: None,
            pricing_status: VanillaOptionPricingStatus::Succeeded,
            pricing_failure_stage: None,
            pricing_failure_reason: None,
            forward_price: Some(100.0),
            discount_factor: Some(1.0),
            boundary_state_norm: Some(0.0),
        }
    }

    fn tick_record(
        timestamp_utc_nanos: i64,
        price: f64,
        trade_sign: Option<&str>,
    ) -> MarketDataTranslationTickRecord {
        MarketDataTranslationTickRecord {
            timestamp_utc_nanos,
            price: Some(price),
            trade_sign: trade_sign.map(str::to_string),
            source_row_number: 0,
        }
    }
}
