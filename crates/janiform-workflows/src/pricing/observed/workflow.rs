use std::collections::BTreeSet;
use std::time::Instant;

use chrono::{DateTime, Utc};
use janiform::calibration::market_data::{
    MarketDataNormalizedOptionSurfaceRecord, MarketDataTranslationTickRecord,
};
use janiform_foundations::cancellation::CancellationToken;
use janiform_terminal_ui::{ProgressAdapter, ProgressMeasure};

use crate::error::WorkflowError;
use crate::progress::bounded_progress_measure;
use crate::terminal::{TerminalProgressMode, stderr_progress_adapter, terminal_summary_policy};

use super::super::progress::{
    cancel_pricing_progress, complete_pricing_progress, emit_pricing_completion_summary,
    emit_pricing_start_summary,
};
use super::super::validation::pricing_validation_error;
use super::super::workflow::PricingWorkflowObserver;
use super::artifacts::{
    ObservedArtifactWriteInput, observed_options_completion_summary,
    observed_options_start_summary, observed_pricing_run_id, write_observed_artifacts_if_requested,
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
    cancellation_token: CancellationToken,
) -> Result<ObservedOptionsPricingOutput, WorkflowError> {
    run_observed_options_pricing_with_observer(
        request,
        PricingWorkflowObserver {
            cancellation_token,
            ..PricingWorkflowObserver::default()
        },
    )
}

/// Run observed-options pricing with shared terminal progress.
pub fn run_observed_options_pricing_with_terminal_progress(
    request: ObservedOptionsPricingRequest,
    cancellation_token: CancellationToken,
    progress_mode: TerminalProgressMode,
) -> Result<ObservedOptionsPricingOutput, WorkflowError> {
    let mut progress_adapter = stderr_progress_adapter(progress_mode);

    run_observed_options_pricing_with_observer(
        request,
        PricingWorkflowObserver {
            cancellation_token,
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
    let cancellation_token = observer.cancellation_token.shared_handle();

    validate_request(&request)?;
    cancellation_token.check_cancelled()?;

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

    if let Err(source) = cancellation_token.check_cancelled() {
        cancel_pricing_progress(observer.progress_adapter.as_deref_mut())?;
        return Err(WorkflowError::Cancelled { source });
    }

    let loaded_inputs =
        load_observed_pricing_inputs(&request, stage_count, &mut observer.progress_adapter)?;

    if let Err(source) = cancellation_token.check_cancelled() {
        cancel_pricing_progress(observer.progress_adapter.as_deref_mut())?;
        return Err(WorkflowError::Cancelled { source });
    }

    let LoadedObservedOptionsPricingInputs {
        valuation_time_utc_nanos,
        snapshot_groups,
        option_records,
        tick_records,
        price_coordinate_resolution,
        warnings,
        option_valuation_spot,
        option_valuation_tick_records,
    } = loaded_inputs;

    let option_valuation_tick_records = option_valuation_tick_records.as_deref();

    let pricing_execution_result = match request.scope {
        ObservedOptionsPricingScope::Snapshot => price_snapshot_groups(
            ObservedOptionsPricingGroupInput {
                request: &request,
                valuation_time_utc_nanos,
                snapshot_groups,
                option_records: option_records.as_slice(),
                tick_records: tick_records.as_slice(),
                option_valuation_spot,
                option_valuation_tick_records,
            },
            cancellation_token.shared_handle(),
            &mut observer.progress_adapter,
        ),
        ObservedOptionsPricingScope::Surface => price_surface_groups(
            ObservedOptionsPricingGroupInput {
                request: &request,
                valuation_time_utc_nanos,
                snapshot_groups,
                option_records: option_records.as_slice(),
                tick_records: tick_records.as_slice(),
                option_valuation_spot,
                option_valuation_tick_records,
            },
            cancellation_token.shared_handle(),
            &mut observer.progress_adapter,
        ),
    };

    let pricing_execution = match pricing_execution_result {
        Ok(pricing_execution) => pricing_execution,
        Err(WorkflowError::Cancelled { source }) => {
            cancel_pricing_progress(observer.progress_adapter.as_deref_mut())?;
            return Err(WorkflowError::Cancelled { source });
        }
        Err(error) => return Err(error),
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
            cancellation_token.shared_handle(),
            &mut observer.progress_adapter,
        );

        let physical_distribution = match physical_distribution {
            Ok(physical_distribution) => physical_distribution,
            Err(WorkflowError::Cancelled { source }) => {
                cancel_pricing_progress(observer.progress_adapter.as_deref_mut())?;
                return Err(WorkflowError::Cancelled { source });
            }
            Err(error) => return Err(error),
        };

        complete_observed_pricing_stage(
            &mut observer.progress_adapter,
            PHYSICAL_DISTRIBUTION_DIAGNOSTICS_STAGE,
            physical_stage_started_at,
        )?;

        physical_distribution
    } else {
        None
    };

    if let Err(source) = cancellation_token.check_cancelled() {
        cancel_pricing_progress(observer.progress_adapter.as_deref_mut())?;
        return Err(WorkflowError::Cancelled { source });
    }

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

    if let Err(source) = cancellation_token.check_cancelled() {
        cancel_pricing_progress(observer.progress_adapter.as_deref_mut())?;
        return Err(WorkflowError::Cancelled { source });
    }

    emit_observed_stage_progress(
        &mut observer.progress_adapter,
        WRITE_ARTIFACTS_STAGE,
        ProgressMeasure::Unbounded,
        Some("writing pricing artifacts".to_string()),
        ObservedWorkItemUpdate::none(),
    )?;

    let resolved_valuation_time_utc =
        DateTime::<Utc>::from_timestamp_nanos(valuation_time_utc_nanos);

    let artifact_paths = write_observed_artifacts_if_requested(ObservedArtifactWriteInput {
        output_path: request.output_path.as_ref(),
        run_id: &run_id,
        scope: request.scope,
        valuation_time_utc: resolved_valuation_time_utc,
        nodes: nodes.as_slice(),
        snapshot_results: &pricing_execution.snapshot_results,
        price_coordinate_resolution: price_coordinate_resolution.as_ref(),
        warnings: warnings.as_slice(),
        physical_distribution: physical_distribution.as_ref(),
        execution_duration,
    })?;

    if let Err(source) = cancellation_token.check_cancelled() {
        cancel_pricing_progress(observer.progress_adapter.as_deref_mut())?;
        return Err(WorkflowError::Cancelled { source });
    }

    emit_observed_stage_progress(
        &mut observer.progress_adapter,
        WRITE_ARTIFACTS_STAGE,
        bounded_progress_measure(1, 1)?,
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
        price_coordinate_resolution,
        warnings,
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

    request
        .option_forward_parity_warning_policy
        .validate()
        .map_err(|source| {
            pricing_validation_error("option_forward_parity_warning_policy", source.to_string())
        })?;

    if let Some(physical_distribution_diagnostics) =
        request.physical_distribution_diagnostics.as_ref()
    {
        physical_distribution_diagnostics.validate()?;
    }

    Ok(())
}

struct ObservedOptionsPricingGroupInput<'a> {
    request: &'a ObservedOptionsPricingRequest,
    valuation_time_utc_nanos: i64,
    snapshot_groups: Vec<PreparedSnapshotGroup>,
    option_records: &'a [MarketDataNormalizedOptionSurfaceRecord],
    tick_records: &'a [MarketDataTranslationTickRecord],
    option_valuation_spot: Option<f64>,
    option_valuation_tick_records: Option<&'a [MarketDataTranslationTickRecord]>,
}

fn price_surface_groups(
    input: ObservedOptionsPricingGroupInput<'_>,
    cancellation_token: CancellationToken,
    progress_adapter: &mut Option<&mut ProgressAdapter>,
) -> Result<ObservedOptionsPricingExecutionResult, WorkflowError> {
    cancellation_token.check_cancelled()?;

    let request = input.request;
    let valuation_time_utc_nanos = input.valuation_time_utc_nanos;

    let expiry_count = input
        .snapshot_groups
        .iter()
        .map(|group| group.expiry_timestamp_utc_nanos)
        .collect::<BTreeSet<_>>()
        .len();

    if expiry_count > 1 {
        return price_snapshot_groups(input, cancellation_token, progress_adapter);
    }

    let selected_expiry_timestamp_utc_nanos = input
        .snapshot_groups
        .first()
        .ok_or_else(|| pricing_validation_error("option_surface", "snapshot must not be empty"))?
        .expiry_timestamp_utc_nanos;

    let history = selected_expiry_history(
        input.option_records,
        valuation_time_utc_nanos,
        selected_expiry_timestamp_utc_nanos,
        request.option_type,
    )?;

    let valuation_times = resolve_single_expiry_surface_valuation_times(
        history.as_slice(),
        valuation_time_utc_nanos,
        retained_snapshot_valuation_count(input.snapshot_groups.as_slice()),
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

    let prepared_snapshots = prepare_observed_snapshots(
        request,
        snapshot_specs,
        input.option_records,
        input.tick_records,
        input.option_valuation_spot,
        input.option_valuation_tick_records,
        progress_adapter,
    )?;

    cancellation_token.check_cancelled()?;

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

    let snapshot_results = price_prepared_surface_snapshots_parallel(
        request,
        prepared_snapshots,
        cancellation_token,
        progress_adapter,
    )?;

    Ok(ObservedOptionsPricingExecutionResult {
        prepared_snapshots: retained_prepared_snapshots,
        snapshot_results,
    })
}

fn price_snapshot_groups(
    input: ObservedOptionsPricingGroupInput<'_>,
    cancellation_token: CancellationToken,
    progress_adapter: &mut Option<&mut ProgressAdapter>,
) -> Result<ObservedOptionsPricingExecutionResult, WorkflowError> {
    cancellation_token.check_cancelled()?;

    let request = input.request;

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
        vec![(input.valuation_time_utc_nanos, input.snapshot_groups)],
        input.option_records,
        input.tick_records,
        input.option_valuation_spot,
        input.option_valuation_tick_records,
        progress_adapter,
    )?;

    cancellation_token.check_cancelled()?;

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

    cancellation_token.check_cancelled()?;

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
