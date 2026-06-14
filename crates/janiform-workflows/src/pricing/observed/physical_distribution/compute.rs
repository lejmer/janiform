use std::sync::Arc;

use chrono::{DateTime, Utc};
use janiform::simulation::CoarseSimulationRunRequest;
use janiform_foundations::cancellation::CancellationToken;
use janiform_foundations::parallelism::resolved_cpu_count;
use janiform_terminal_ui::ProgressAdapter;
use janiform_work_scheduler::{
    StreamingWorkContext, StreamingWorkOutput, StreamingWorkPoolInput, StreamingWorkProgress,
    run_streaming_work_pool,
};

use crate::error::WorkflowError;
use crate::model::{
    FittedModelContext, build_coarse_simulation_model_inputs, build_interval_factor_grid,
    build_sufficient_statistics_interval_factor_grids, build_sufficient_statistics_model_inputs,
    fitted_model_context, materialize_mean_reversion_runtime_inputs, validation_error,
};
use crate::progress::bounded_progress_measure;

use super::super::super::validation::pricing_validation_error;
use super::super::progress::{ObservedWorkItemUpdate, emit_observed_stage_progress};
use super::super::types::{
    DEFAULT_PRICING_INTERVAL_DURATION_NANOS, ObservedOptionsPricingRequest,
    ObservedOptionsSnapshotResult, PreparedObservedSnapshot,
};
use super::collector::PhysicalDistributionCollector;
use super::nodes::build_physical_distribution_nodes;
use super::sampling::{
    PhysicalDistributionPartitionOutput, PhysicalDistributionProgress,
    PreparedPhysicalDistributionSnapshot, build_physical_distribution_jobs,
    execute_physical_distribution_job, terminal_record_index_for_expiry,
};
use super::types::{
    ObservedOptionsPhysicalDistributionDiagnosticsNode,
    ObservedOptionsPhysicalDistributionDiagnosticsOutput,
    ObservedOptionsPhysicalDistributionDiagnosticsReport,
    ObservedOptionsPhysicalDistributionDiagnosticsRequest,
    PHYSICAL_DISTRIBUTION_OUTPUT_SCHEMA_VERSION, PHYSICAL_DISTRIBUTION_REPORT_SCHEMA_VERSION,
};

pub(in crate::pricing::observed) fn compute_physical_distribution_diagnostics_if_requested(
    request: &ObservedOptionsPricingRequest,
    prepared_snapshots: &[PreparedObservedSnapshot],
    snapshot_results: &[ObservedOptionsSnapshotResult],
    cancellation_token: CancellationToken,
    progress_adapter: &mut Option<&mut ProgressAdapter>,
) -> Result<Option<ObservedOptionsPhysicalDistributionDiagnosticsOutput>, WorkflowError> {
    let Some(diagnostics_request) = request.physical_distribution_diagnostics else {
        return Ok(None);
    };

    cancellation_token.check_cancelled()?;

    diagnostics_request.validate()?;

    let prepared = prepare_physical_distribution_snapshots(request, prepared_snapshots)?;
    if prepared.len() != snapshot_results.len() {
        return Err(pricing_validation_error(
            "physical_distribution_diagnostics",
            "prepared valuation states and pricing results must have the same length",
        ));
    }

    let run_summary = run_physical_distribution_jobs(
        prepared.as_slice(),
        diagnostics_request,
        request.cpu_count,
        cancellation_token.shared_handle(),
        progress_adapter,
    )?;

    let nodes = build_physical_distribution_nodes(
        prepared_snapshots,
        snapshot_results,
        run_summary.terminal_returns_by_snapshot.as_slice(),
        diagnostics_request.path_count,
    )?;

    emit_observed_stage_progress(
        progress_adapter,
        "physical distribution diagnostics",
        bounded_progress_measure(
            diagnostics_request.path_count * prepared.len(),
            diagnostics_request.path_count * prepared.len(),
        )?,
        Some(format!(
            "computed {} physical distribution rows",
            nodes.len()
        )),
        ObservedWorkItemUpdate::none(),
    )?;

    Ok(Some(build_physical_distribution_output(
        diagnostics_request,
        prepared.len(),
        nodes,
        run_summary,
    )))
}

struct PhysicalDistributionRunSummary {
    terminal_returns_by_snapshot: Vec<Vec<Vec<f64>>>,
    path_partition_count: usize,
    worker_count: usize,
    maximum_horizon_interval_count: usize,
}

fn run_physical_distribution_jobs(
    prepared: &[Arc<PreparedPhysicalDistributionSnapshot>],
    diagnostics_request: ObservedOptionsPhysicalDistributionDiagnosticsRequest,
    cpu_count: Option<usize>,
    cancellation_token: CancellationToken,
    progress_adapter: &mut Option<&mut ProgressAdapter>,
) -> Result<PhysicalDistributionRunSummary, WorkflowError> {
    cancellation_token.check_cancelled()?;

    let worker_count = resolved_cpu_count(cpu_count).max(1);

    let jobs = build_physical_distribution_jobs(prepared, diagnostics_request, worker_count);

    let path_partition_count = jobs.len();

    let maximum_horizon_interval_count = prepared
        .iter()
        .map(|snapshot| snapshot.horizon_interval_count)
        .max()
        .unwrap_or(0);

    let active_worker_count = worker_count.min(path_partition_count.max(1));

    emit_observed_stage_progress(
        progress_adapter,
        "physical distribution diagnostics",
        bounded_progress_measure(0, diagnostics_request.path_count * prepared.len())?,
        Some(format!(
            "simulating {} P paths for {} valuation states on {} CPUs",
            diagnostics_request.path_count,
            prepared.len(),
            active_worker_count
        )),
        ObservedWorkItemUpdate::none(),
    )?;

    let mut collector = PhysicalDistributionCollector::new(
        prepared.len(),
        path_partition_count,
        diagnostics_request.path_count,
        progress_adapter,
    );

    run_streaming_work_pool(StreamingWorkPoolInput::new(
        &mut collector,
        cancellation_token,
        jobs,
        active_worker_count,
        |job,
         context: StreamingWorkContext<
            PhysicalDistributionProgress,
            Result<PhysicalDistributionPartitionOutput, WorkflowError>,
        >| execute_physical_distribution_job(job, context),
        |collector: &mut PhysicalDistributionCollector<'_, '_>,
         progress: StreamingWorkProgress<PhysicalDistributionProgress>| {
            collector.record_progress(progress.progress)
        },
        |collector: &mut PhysicalDistributionCollector<'_, '_>,
         output: StreamingWorkOutput<
            Result<PhysicalDistributionPartitionOutput, WorkflowError>,
        >| { collector.record_completed(output.job_position, output.output?) },
    ))?;

    Ok(PhysicalDistributionRunSummary {
        terminal_returns_by_snapshot: collector.finish()?,
        path_partition_count,
        worker_count: active_worker_count,
        maximum_horizon_interval_count,
    })
}

fn build_physical_distribution_output(
    diagnostics_request: ObservedOptionsPhysicalDistributionDiagnosticsRequest,
    valuation_count: usize,
    nodes: Vec<ObservedOptionsPhysicalDistributionDiagnosticsNode>,
    run_summary: PhysicalDistributionRunSummary,
) -> ObservedOptionsPhysicalDistributionDiagnosticsOutput {
    ObservedOptionsPhysicalDistributionDiagnosticsOutput {
        schema_version: PHYSICAL_DISTRIBUTION_OUTPUT_SCHEMA_VERSION,
        request: diagnostics_request,
        report: ObservedOptionsPhysicalDistributionDiagnosticsReport {
            schema_version: PHYSICAL_DISTRIBUTION_REPORT_SCHEMA_VERSION,
            valuation_count,
            node_count: nodes.len(),
            path_count_per_valuation: diagnostics_request.path_count,
            path_partition_count: run_summary.path_partition_count,
            worker_count: run_summary.worker_count,
            maximum_horizon_interval_count: run_summary.maximum_horizon_interval_count,
        },
        nodes,
    }
}

fn prepare_physical_distribution_snapshots(
    request: &ObservedOptionsPricingRequest,
    prepared_snapshots: &[PreparedObservedSnapshot],
) -> Result<Vec<Arc<PreparedPhysicalDistributionSnapshot>>, WorkflowError> {
    prepared_snapshots
        .iter()
        .map(|prepared_snapshot| {
            prepare_physical_distribution_snapshot(request, prepared_snapshot).map(Arc::new)
        })
        .collect()
}

fn prepare_physical_distribution_snapshot(
    request: &ObservedOptionsPricingRequest,
    prepared_snapshot: &PreparedObservedSnapshot,
) -> Result<PreparedPhysicalDistributionSnapshot, WorkflowError> {
    let valuation_time_utc =
        DateTime::<Utc>::from_timestamp_nanos(prepared_snapshot.valuation_time_utc_nanos);

    let terminal_record_indices = prepared_snapshot
        .snapshot_groups
        .iter()
        .map(|group| {
            terminal_record_index_for_expiry(
                prepared_snapshot.valuation_time_utc_nanos,
                group.expiry_timestamp_utc_nanos,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    let horizon_interval_count = terminal_record_indices
        .iter()
        .map(|terminal_record_index| terminal_record_index + 1)
        .max()
        .ok_or_else(|| pricing_validation_error("option_surface", "snapshot must not be empty"))?;

    let context = fitted_model_context(
        &request.model_payload,
        valuation_time_utc,
        DEFAULT_PRICING_INTERVAL_DURATION_NANOS,
        horizon_interval_count,
    )?;

    let simulation_request_template = build_physical_distribution_simulation_request(
        &context,
        prepared_snapshot,
        horizon_interval_count,
    )?;

    Ok(PreparedPhysicalDistributionSnapshot {
        simulation_request_template,
        terminal_record_indices,
        initial_observed_log_price: prepared_snapshot
            .estimated_state
            .mean_reversion_boundary_state
            .observed_log_price,
        horizon_interval_count,
    })
}

fn build_physical_distribution_simulation_request(
    context: &FittedModelContext,
    prepared_snapshot: &PreparedObservedSnapshot,
    horizon_interval_count: usize,
) -> Result<CoarseSimulationRunRequest, WorkflowError> {
    let valuation_time_utc =
        DateTime::<Utc>::from_timestamp_nanos(prepared_snapshot.valuation_time_utc_nanos);

    let interval_factor_grid = build_interval_factor_grid(
        context,
        valuation_time_utc,
        context.range_end_utc,
        DEFAULT_PRICING_INTERVAL_DURATION_NANOS,
    )?;

    let (runtime_interval_anchors, _) = materialize_mean_reversion_runtime_inputs(
        context,
        &interval_factor_grid,
        prepared_snapshot.estimated_state.hawkes_excitation_state,
        prepared_snapshot
            .estimated_state
            .mean_reversion_boundary_state
            .anchor_value,
        context.interval_delta,
        horizon_interval_count,
    )?;

    let model_inputs = build_sufficient_statistics_model_inputs(
        context,
        prepared_snapshot.estimated_state.hawkes_excitation_state,
        runtime_interval_anchors,
        // Intentional copy because each diagnostic valuation owns the boundary
        // state used to seed physical path sampling.
        prepared_snapshot
            .estimated_state
            .mean_reversion_boundary_state
            .clone(),
    )?;

    let coarse_model_inputs = build_coarse_simulation_model_inputs(context, model_inputs)?;

    let interval_factor_grids =
        build_sufficient_statistics_interval_factor_grids(context, interval_factor_grid)?;

    CoarseSimulationRunRequest::try_new(
        0,
        prepared_snapshot.valuation_time_utc_nanos,
        horizon_interval_count,
        context.interval_delta,
        coarse_model_inputs,
        interval_factor_grids,
    )
    .map_err(|source| validation_error("coarse_interval_request", source.to_string()))
}
