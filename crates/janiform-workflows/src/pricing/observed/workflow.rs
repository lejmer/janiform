use std::collections::BTreeSet;
use std::time::Instant;

use chrono::{DateTime, Utc};
use janiform::calibration::market_data::{
    MarketDataNormalizedOptionSurfaceRecord, MarketDataTranslationTickRecord,
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
    prepare_selected_expiry_snapshot_specs_by_valuation_times,
    resolve_single_expiry_surface_valuation_times, retained_snapshot_valuation_count,
    selected_expiry_history,
};
use super::loading::{LoadedObservedOptionsPricingInputs, load_observed_pricing_inputs};
use super::physical_distribution::compute_physical_distribution_diagnostics_if_requested;
use super::pricing::price_prepared_snapshot_with_terminal_progress;
use super::progress::{
    ObservedWorkItemUpdate, complete_observed_pricing_stage, emit_observed_stage_progress,
    observed_pricing_stage_count, start_observed_pricing_progress, start_observed_pricing_stage,
};
use super::state::prepare_observed_snapshots;
use super::surface::price_prepared_surface_snapshots_parallel;
use super::types::{
    ObservedOptionsPricingOutput, ObservedOptionsPricingRequest, ObservedOptionsPricingScope,
    ObservedOptionsSnapshotResult, PHYSICAL_DISTRIBUTION_DIAGNOSTICS_STAGE,
    PREPARE_VALUATION_STATES_STAGE, PRICE_VALUATION_SURFACE_STAGE, PreparedObservedSnapshot,
    PreparedSnapshotGroup, WRITE_ARTIFACTS_STAGE,
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

    let include_physical_distribution_diagnostics =
        request.physical_distribution_diagnostics.is_some();
    let stage_count = observed_pricing_stage_count(include_physical_distribution_diagnostics);

    start_observed_pricing_progress(
        &mut observer.progress_adapter,
        &run_id,
        request.output_path.as_deref(),
        include_physical_distribution_diagnostics,
    )?;

    let loaded_inputs =
        load_observed_pricing_inputs(&request, stage_count, &mut observer.progress_adapter)?;

    let LoadedObservedOptionsPricingInputs {
        valuation_time_utc_nanos,
        snapshot_groups,
        option_records,
        tick_records,
    } = loaded_inputs;

    let pricing_execution = match request.scope {
        ObservedOptionsPricingScope::Snapshot => price_snapshot_groups(
            &request,
            valuation_time_utc_nanos,
            snapshot_groups,
            tick_records.as_slice(),
            &mut observer.progress_adapter,
        )?,
        ObservedOptionsPricingScope::Surface => price_surface_groups(
            &request,
            valuation_time_utc_nanos,
            snapshot_groups,
            option_records.as_slice(),
            tick_records.as_slice(),
            &mut observer.progress_adapter,
        )?,
    };

    let physical_distribution = if include_physical_distribution_diagnostics {
        let physical_stage_started_at = start_observed_pricing_stage(
            &mut observer.progress_adapter,
            4,
            stage_count,
            PHYSICAL_DISTRIBUTION_DIAGNOSTICS_STAGE,
        )?;
        let physical_distribution = compute_physical_distribution_diagnostics_if_requested(
            &request,
            pricing_execution.prepared_snapshots.as_slice(),
            pricing_execution.snapshot_results.as_slice(),
            &mut observer.progress_adapter,
        )?;

        complete_observed_pricing_stage(
            &mut observer.progress_adapter,
            PHYSICAL_DISTRIBUTION_DIAGNOSTICS_STAGE,
            physical_stage_started_at,
        )?;

        physical_distribution
    } else {
        None
    };

    let execution_duration = started_at.elapsed();

    let mut nodes = Vec::new();

    for snapshot_result in &pricing_execution.snapshot_results {
        nodes.extend(snapshot_result.nodes.iter().cloned());
    }

    let write_stage_number = if include_physical_distribution_diagnostics {
        5
    } else {
        4
    };
    let write_stage_started_at = start_observed_pricing_stage(
        &mut observer.progress_adapter,
        write_stage_number,
        stage_count,
        WRITE_ARTIFACTS_STAGE,
    )?;
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
        &pricing_execution.snapshot_results,
        physical_distribution.as_ref(),
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
        snapshot_results: pricing_execution.snapshot_results,
        physical_distribution,
        artifact_paths,
        execution_duration_seconds: execution_duration.as_secs_f64(),
    };

    complete_pricing_progress(observer.progress_adapter.as_deref_mut())?;

    emit_pricing_completion_summary(
        &observer.terminal_output,
        observed_options_completion_summary(&output, execution_duration),
    )?;

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

    if let Some(physical_distribution_diagnostics) =
        request.physical_distribution_diagnostics.as_ref()
    {
        physical_distribution_diagnostics.validate()?;
    }

    Ok(())
}

fn price_surface_groups(
    request: &ObservedOptionsPricingRequest,
    valuation_time_utc_nanos: i64,
    snapshot_groups: Vec<PreparedSnapshotGroup>,
    option_records: &[MarketDataNormalizedOptionSurfaceRecord],
    tick_records: &[MarketDataTranslationTickRecord],
    progress_adapter: &mut Option<&mut ProgressAdapter>,
) -> Result<ObservedOptionsPricingExecutionResult, WorkflowError> {
    let expiry_count = snapshot_groups
        .iter()
        .map(|group| group.expiry_timestamp_utc_nanos)
        .collect::<BTreeSet<_>>()
        .len();

    if expiry_count > 1 {
        return price_snapshot_groups(
            request,
            valuation_time_utc_nanos,
            snapshot_groups,
            tick_records,
            progress_adapter,
        );
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

    let stage_count =
        observed_pricing_stage_count(request.physical_distribution_diagnostics.is_some());
    let prepare_stage_started_at = start_observed_pricing_stage(
        progress_adapter,
        2,
        stage_count,
        PREPARE_VALUATION_STATES_STAGE,
    )?;

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

    let retained_prepared_snapshots = if request.physical_distribution_diagnostics.is_some() {
        // Intentional copy because Q pricing consumes prepared snapshots while
        // the optional P diagnostic reuses the same valuation states afterward.
        prepared_snapshots.clone()
    } else {
        Vec::new()
    };
    let snapshot_results =
        price_prepared_surface_snapshots_parallel(request, prepared_snapshots, progress_adapter)?;

    Ok(ObservedOptionsPricingExecutionResult {
        prepared_snapshots: retained_prepared_snapshots,
        snapshot_results,
    })
}

fn price_snapshot_groups(
    request: &ObservedOptionsPricingRequest,
    valuation_time_utc_nanos: i64,
    snapshot_groups: Vec<PreparedSnapshotGroup>,
    tick_records: &[MarketDataTranslationTickRecord],
    progress_adapter: &mut Option<&mut ProgressAdapter>,
) -> Result<ObservedOptionsPricingExecutionResult, WorkflowError> {
    let stage_count =
        observed_pricing_stage_count(request.physical_distribution_diagnostics.is_some());
    let prepare_stage_started_at = start_observed_pricing_stage(
        progress_adapter,
        2,
        stage_count,
        PREPARE_VALUATION_STATES_STAGE,
    )?;

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

    let retained_prepared_snapshots = if request.physical_distribution_diagnostics.is_some() {
        // Intentional copy because Q pricing consumes prepared snapshots while
        // the optional P diagnostic reuses the same valuation state afterward.
        prepared_snapshots.clone()
    } else {
        Vec::new()
    };

    let pricing_stage_started_at = start_observed_pricing_stage(
        progress_adapter,
        3,
        stage_count,
        PRICE_VALUATION_SURFACE_STAGE,
    )?;

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

    Ok(ObservedOptionsPricingExecutionResult {
        prepared_snapshots: retained_prepared_snapshots,
        snapshot_results: vec![result],
    })
}

struct ObservedOptionsPricingExecutionResult {
    prepared_snapshots: Vec<PreparedObservedSnapshot>,
    snapshot_results: Vec<ObservedOptionsSnapshotResult>,
}
