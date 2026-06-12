use janiform::pricing::PricingProgressEvent;
use janiform_foundations::cancellation::CancellationToken;
use janiform_foundations::parallelism::resolved_cpu_count;
use janiform_terminal_ui::ProgressAdapter;
use janiform_work_scheduler::{
    StreamingWorkContext, StreamingWorkOutput, StreamingWorkPoolInput, StreamingWorkProgress,
    run_streaming_work_pool,
};

use crate::error::WorkflowError;

use super::super::validation::pricing_validation_error;
use super::pricing::{
    prepared_snapshot_node_count, price_prepared_snapshot,
    price_prepared_snapshot_with_terminal_progress, resolve_surface_task_progress_plans,
};
use super::progress::{
    complete_observed_pricing_stage, observed_pricing_stage_count, start_observed_pricing_stage,
};
use super::surface_progress::{
    SurfacePricingMessage, SurfacePricingResultCollector, SurfaceTaskProgressPlan,
    SurfaceWorkerCompletedInput, SurfaceWorkerPricingProgressInput, SurfaceWorkerProgressInput,
    SurfaceWorkerState, emit_surface_interval_planning, emit_surface_pricing_plan,
    emit_surface_worker_completed, emit_surface_worker_progress, emit_surface_worker_started,
};
use super::types::{
    ObservedOptionsPricingRequest, ObservedOptionsSnapshotResult, PRICE_VALUATION_SURFACE_STAGE,
    PreparedObservedSnapshot,
};

pub(super) fn price_prepared_surface_snapshots_parallel(
    request: &ObservedOptionsPricingRequest,
    prepared_snapshots: Vec<PreparedObservedSnapshot>,
    cancellation_token: CancellationToken,
    progress_adapter: &mut Option<&mut ProgressAdapter>,
) -> Result<Vec<ObservedOptionsSnapshotResult>, WorkflowError> {
    cancellation_token.check_cancelled()?;

    let task_count = prepared_snapshots.len();
    if task_count == 0 {
        return Ok(Vec::new());
    }

    let worker_count = resolved_cpu_count(request.cpu_count).min(task_count.max(1));

    let stage_count =
        observed_pricing_stage_count(request.physical_distribution_diagnostics.is_some());

    let pricing_stage_started_at = start_observed_pricing_stage(
        progress_adapter,
        3,
        stage_count,
        PRICE_VALUATION_SURFACE_STAGE,
    )?;

    if task_count == 1 {
        let prepared_snapshot = prepared_snapshots.into_iter().next().ok_or_else(|| {
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

        return Ok(vec![result]);
    }

    if worker_count <= 1 {
        let task_progress_plans =
            resolve_surface_task_progress_plans(request, prepared_snapshots.as_slice())?;

        let results = price_prepared_surface_snapshots_serial(
            request,
            prepared_snapshots,
            cancellation_token.shared_handle(),
            task_progress_plans,
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

    let task_progress_plans =
        resolve_surface_task_progress_plans(request, prepared_snapshots.as_slice())?;

    emit_surface_pricing_plan(progress_adapter, task_count, worker_count)?;

    let jobs = prepared_snapshots
        .into_iter()
        .enumerate()
        .collect::<Vec<_>>();

    let mut collector =
        SurfacePricingResultCollector::new(task_count, worker_count, progress_adapter);

    run_streaming_work_pool(StreamingWorkPoolInput::new(
        &mut collector,
        cancellation_token,
        jobs,
        worker_count,
        |(task_position, prepared_snapshot),
         context: StreamingWorkContext<
            SurfacePricingMessage,
            Result<ObservedOptionsSnapshotResult, WorkflowError>,
        >| {
            let worker_cancellation_token = context.cancellation_token();

            worker_cancellation_token.check_cancelled()?;

            let worker_index = context.worker_index();
            let node_count = prepared_snapshot_node_count(&prepared_snapshot);
            let valuation_time_utc_nanos = prepared_snapshot.valuation_time_utc_nanos;

            let progress_plan = task_progress_plans
                .get(task_position)
                .copied()
                .ok_or_else(|| {
                    pricing_validation_error(
                        "pricing_progress.task_position",
                        "referenced an unknown valuation task",
                    )
                })?;

            if !context.emit_progress(SurfacePricingMessage::Started {
                worker_index,
                task_position,
                valuation_time_utc_nanos,
                node_count,
                progress_plan,
            }) {
                worker_cancellation_token.check_cancelled()?;

                return Err(pricing_validation_error(
                    "pricing_progress",
                    "observed surface pricing progress channel closed",
                ));
            }

            let mut progress_observer = |event: PricingProgressEvent| {
                if worker_cancellation_token.is_cancelled() {
                    return;
                }

                let _ = context.emit_progress(SurfacePricingMessage::Progress {
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
            )?;

            worker_cancellation_token.check_cancelled()?;

            Ok(result)
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
    cancellation_token: CancellationToken,
    task_progress_plans: Vec<SurfaceTaskProgressPlan>,
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    worker_count: usize,
) -> Result<Vec<ObservedOptionsSnapshotResult>, WorkflowError> {
    let task_count = prepared_snapshots.len();

    let mut results = Vec::with_capacity(task_count);

    emit_surface_pricing_plan(progress_adapter, task_count, worker_count.max(1))?;

    for (task_position, prepared_snapshot) in prepared_snapshots.into_iter().enumerate() {
        cancellation_token.check_cancelled()?;

        let node_count = prepared_snapshot_node_count(&prepared_snapshot);
        let valuation_time_utc_nanos = prepared_snapshot.valuation_time_utc_nanos;

        let progress_plan = task_progress_plans
            .get(task_position)
            .copied()
            .ok_or_else(|| {
                pricing_validation_error(
                    "pricing_progress.task_position",
                    "referenced an unknown valuation task",
                )
            })?;

        let mut worker_state = SurfaceWorkerState::started(
            task_position,
            valuation_time_utc_nanos,
            node_count,
            progress_plan,
        );

        emit_surface_worker_started(
            progress_adapter,
            SurfaceWorkerProgressInput {
                worker_index: 0,
                task_position,
                task_count,
                completed_task_count: results.len(),
                worker_count: worker_count.max(1),
                valuation_time_utc_nanos,
                node_count,
                progress_plan,
            },
        )?;

        let mut progress_error = None::<WorkflowError>;

        let mut progress_observer = |event: PricingProgressEvent| {
            if progress_error.is_some() {
                return;
            }

            if cancellation_token.is_cancelled() {
                return;
            }

            worker_state.apply_pricing_event(&event);

            if let Err(source) = emit_surface_worker_progress(
                progress_adapter,
                SurfaceWorkerPricingProgressInput {
                    worker_index: 0,
                    task_count,
                    completed_task_count: results.len(),
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

        cancellation_token.check_cancelled()?;

        if let Some(progress_error) = progress_error {
            return Err(progress_error);
        }

        results.push(result);
        worker_state.complete();

        emit_surface_worker_completed(
            progress_adapter,
            SurfaceWorkerCompletedInput {
                worker_index: 0,
                task_count,
                completed_task_count: results.len(),
                worker_count: worker_count.max(1),
                worker_state,
            },
        )?;
    }

    Ok(results)
}
