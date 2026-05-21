use janiform::pricing::PricingProgressEvent;
use janiform_foundations::parallelism::resolved_cpu_count;
use janiform_terminal_ui::ProgressAdapter;
use janiform_work_scheduler::{
    StreamingWorkContext, StreamingWorkOutput, StreamingWorkPoolInput, StreamingWorkProgress,
    run_streaming_work_pool,
};

use crate::error::WorkflowError;
use crate::progress::{
    ProgressUnitTracker, completed_tracked_progress_units, total_tracked_progress_units,
};

use super::super::validation::pricing_validation_error;
use super::pricing::{
    prepared_snapshot_node_count, price_prepared_snapshot,
    price_prepared_snapshot_with_terminal_progress, resolve_surface_task_interval_counts,
};
use super::progress::{complete_observed_pricing_stage, start_observed_pricing_stage};
use super::surface_progress::{
    SurfacePricingMessage, SurfacePricingResultCollector, SurfaceWorkerCompletedInput,
    SurfaceWorkerPricingProgressInput, SurfaceWorkerProgressInput, SurfaceWorkerState,
    emit_surface_interval_planning, emit_surface_pricing_plan, emit_surface_worker_completed,
    emit_surface_worker_progress, emit_surface_worker_started, surface_task_progress_totals,
};
use super::types::{
    ObservedOptionsPricingRequest, ObservedOptionsSnapshotResult, PRICE_VALUATION_SURFACE_STAGE,
    PreparedObservedSnapshot,
};

pub(super) fn price_prepared_surface_snapshots_parallel(
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
    let jobs = prepared_snapshots
        .into_iter()
        .enumerate()
        .collect::<Vec<_>>();
    let mut collector = SurfacePricingResultCollector::new(
        task_count,
        worker_count,
        task_interval_counts.as_slice(),
        progress_adapter,
    );

    run_streaming_work_pool(StreamingWorkPoolInput::new(
        &mut collector,
        jobs,
        worker_count,
        |(task_position, prepared_snapshot),
         context: StreamingWorkContext<
            SurfacePricingMessage,
            Result<ObservedOptionsSnapshotResult, WorkflowError>,
        >| {
            let worker_index = context.worker_index();
            let node_count = prepared_snapshot_node_count(&prepared_snapshot);
            let valuation_time_utc_nanos = prepared_snapshot.valuation_time_utc_nanos;
            let total_interval_count = task_interval_counts
                .get(task_position)
                .copied()
                .unwrap_or(0);

            if !context.send_progress(SurfacePricingMessage::Started {
                worker_index,
                task_position,
                valuation_time_utc_nanos,
                node_count,
                total_interval_count,
            }) {
                return Err(pricing_validation_error(
                    "pricing_progress",
                    "observed surface pricing progress channel closed",
                ));
            }

            let mut progress_observer = |event: PricingProgressEvent| {
                let _ = context.send_progress(SurfacePricingMessage::Progress {
                    worker_index,
                    event: Box::new(event),
                });
            };

            price_prepared_snapshot(
                request,
                prepared_snapshot,
                Some(&mut progress_observer),
                // Surface valuation tasks own the outer CPU budget.
                // Keeping each inner grid pricer single-threaded avoids
                // nested thread pools competing for the same cores.
                Some(1),
            )
        },
        |collector: &mut SurfacePricingResultCollector<'_, '_>,
         progress: StreamingWorkProgress<SurfacePricingMessage>| {
            collector.record_progress(progress.progress)
        },
        |collector: &mut SurfacePricingResultCollector<'_, '_>,
         output: StreamingWorkOutput<Result<ObservedOptionsSnapshotResult, WorkflowError>>| {
            collector.record_completed(output.worker_index, output.job_position, output.output)
        },
    ))?;

    let results = collector.finish()?;

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
    let task_progress_totals = surface_task_progress_totals(task_interval_counts.as_slice());
    let mut task_progress_units =
        ProgressUnitTracker::planned_for_totals(task_progress_totals.as_slice());
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
                task_progress.record(worker_state.completed_progress_units);
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
