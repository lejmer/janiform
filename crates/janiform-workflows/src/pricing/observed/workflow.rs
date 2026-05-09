use std::collections::BTreeSet;
use std::time::Instant;

use chrono::{DateTime, Utc};
use janiform::calibration::market_data::{
    MarketDataInputFileReference, MarketDataNormalizationPolicy,
    MarketDataNormalizedOptionSurfaceRecord, MarketDataOptionSurfaceLoadingInput,
    MarketDataTickDataColumnMapping, MarketDataTranslationTickRecord,
    infer_market_data_tick_price_input_policy, normalize_market_data_source,
    normalize_option_surface_source,
};
use janiform_terminal_ui::{ProgressAdapter, ProgressMeasure};

use crate::error::WorkflowError;
use crate::terminal::{TerminalProgressMode, stderr_progress_adapter, terminal_summary_policy};

use super::super::progress::{
    complete_pricing_progress, emit_pricing_completion_summary, emit_pricing_start_summary,
};
use super::super::validation::pricing_validation_error;
use super::super::workflow::PricingWorkflowObserver;
use super::artifacts::{
    observed_options_completion_summary, observed_options_start_summary, observed_pricing_run_id,
    write_observed_artifacts_if_requested,
};
use super::data::{
    prepare_selected_expiry_snapshot_specs_by_valuation_times, prepare_snapshot_groups,
    resolve_effective_valuation_time_utc_nanos, resolve_single_expiry_surface_valuation_times,
    retained_snapshot_valuation_count, selected_expiry_history,
};
use super::pricing::price_prepared_snapshot_with_terminal_progress;
use super::progress::{
    ObservedWorkItemUpdate, complete_observed_pricing_stage, emit_observed_stage_progress,
    start_observed_pricing_progress, start_observed_pricing_stage,
};
use super::state::prepare_observed_snapshots;
use super::surface::price_prepared_surface_snapshots_parallel;
use super::types::{
    LOAD_OBSERVED_DATA_STAGE, ObservedOptionsPricingOutput, ObservedOptionsPricingRequest,
    ObservedOptionsPricingScope, ObservedOptionsSnapshotResult, PREPARE_VALUATION_STATES_STAGE,
    PRICE_VALUATION_SURFACE_STAGE, PreparedSnapshotGroup, WRITE_ARTIFACTS_STAGE,
};

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
