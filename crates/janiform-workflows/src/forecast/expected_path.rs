use std::ops::Range;

use chrono::{DateTime, Utc};
use janiform_foundations::cancellation::CancellationToken;
use janiform_foundations::parallelism::resolved_cpu_count;
use janiform_work_scheduler::{
    StreamingWorkContext, StreamingWorkOutput, StreamingWorkPoolInput, StreamingWorkProgress,
    balanced_work_ranges, run_streaming_work_pool,
};

use crate::error::WorkflowError;

use super::numeric::usize_to_f64;
use super::output::ForecastExpectedPathRow;
use super::progress::{ForecastProgressEvent, ForecastProgressPhase};
use super::range_progress::{ForecastRangeCollector, ForecastRangeProgress};
use super::request::{ForecastWorkflowRequest, validation_error};
use super::runtime::{
    ForecastRuntime, ResolvedForecastRequest, engine_forecast_measure, timestamp_after_intervals,
};

struct ExpectedPathRangeOutput {
    range: Range<usize>,
    result: Result<Vec<ForecastExpectedPathRow>, WorkflowError>,
}

pub(super) fn build_expected_path(
    request: &ForecastWorkflowRequest,
    resolved: &ResolvedForecastRequest,
    runtime: &ForecastRuntime,
    cancellation_token: CancellationToken,
    emit_progress: &mut dyn FnMut(ForecastProgressEvent),
) -> Result<Vec<ForecastExpectedPathRow>, WorkflowError> {
    let mut rows = Vec::with_capacity(resolved.horizon_interval_count + 1);

    rows.push(expected_path_row(
        0,
        request.start_date_utc,
        0.0,
        runtime.initial_log_price,
        runtime.initial_price,
    )?);

    let worker_count =
        resolved_cpu_count(request.cpu_count).min(resolved.horizon_interval_count.max(1));

    let ranges = balanced_work_ranges(resolved.horizon_interval_count, worker_count);

    let mut collector = ForecastRangeCollector::new(
        ForecastProgressPhase::ProjectingExpectedPath,
        ranges.clone(),
        worker_count,
        "intervals",
        "Intervals",
        emit_progress,
    );

    collector.emit_plan("projecting expected path");

    run_streaming_work_pool(StreamingWorkPoolInput::new(
        &mut collector,
        cancellation_token,
        ranges,
        worker_count,
        |range, context: StreamingWorkContext<ForecastRangeProgress, ExpectedPathRangeOutput>| {
            compute_expected_path_range(request, runtime, range, context)
        },
        |collector: &mut ForecastRangeCollector<'_, ExpectedPathRangeOutput>,
         progress: StreamingWorkProgress<ForecastRangeProgress>|
         -> Result<(), WorkflowError> { collector.record_progress(progress) },
        |collector: &mut ForecastRangeCollector<'_, ExpectedPathRangeOutput>,
         output: StreamingWorkOutput<ExpectedPathRangeOutput>|
         -> Result<(), WorkflowError> {
            collector.record_output(output.job_position, &output.output.range)?;
            collector.push_output(output.output);

            Ok(())
        },
    ))?;

    let mut range_outputs = collector.into_outputs();

    range_outputs.sort_by_key(|output| output.range.start);

    for output in range_outputs {
        rows.extend(output.result?);
    }

    Ok(rows)
}

fn compute_expected_path_range(
    request: &ForecastWorkflowRequest,
    runtime: &ForecastRuntime,
    range: Range<usize>,
    context: StreamingWorkContext<ForecastRangeProgress, ExpectedPathRangeOutput>,
) -> ExpectedPathRangeOutput {
    let result = compute_expected_path_range_result(request, runtime, &range, &context);

    ExpectedPathRangeOutput { range, result }
}

fn compute_expected_path_range_result(
    request: &ForecastWorkflowRequest,
    runtime: &ForecastRuntime,
    range: &Range<usize>,
    context: &StreamingWorkContext<ForecastRangeProgress, ExpectedPathRangeOutput>,
) -> Result<Vec<ForecastExpectedPathRow>, WorkflowError> {
    let worker_cancellation_token = context.cancellation_token();

    worker_cancellation_token.check_cancelled()?;

    if !context.emit_progress(ForecastRangeProgress::Started) {
        worker_cancellation_token.check_cancelled()?;

        return Err(validation_error(
            "forecast_progress",
            "expected-path progress channel closed before range start",
        ));
    }

    let mut rows = Vec::with_capacity(range.len());

    for interval_offset in range.start..range.end {
        worker_cancellation_token.check_cancelled()?;

        let interval_index = interval_offset + 1;

        let maturity_interval =
            runtime.context.interval_delta * usize_to_f64(interval_index, "interval_index")?;

        let projection = janiform_engine_pricing::solve_affine_forecast_expected_log_return(
            janiform_engine_pricing::AffineForecastExpectedLogReturnInput {
                pricing_request: &runtime.pricing_request,
                maturity_count: interval_index,
                maturity_interval,
                interval_deltas: runtime.interval_deltas.as_slice(),
                interval_factor_grids: runtime.interval_factor_grids.as_slice(),
                measure: engine_forecast_measure(request.measure),
            },
        )
        .map_err(|source| validation_error("expected_path", source.to_string()))?;

        let timestamp_utc = timestamp_after_intervals(
            request.start_date_utc,
            request.interval_duration_nanos,
            interval_index,
            "expected_path",
        )?;

        rows.push(expected_path_row(
            interval_index,
            timestamp_utc,
            projection.expected_log_return,
            runtime.initial_log_price,
            runtime.initial_price,
        )?);

        let completed_units = interval_offset + 1 - range.start;
        let _ = context.emit_progress(ForecastRangeProgress::Advanced { completed_units });
    }

    Ok(rows)
}

fn expected_path_row(
    interval_index: usize,
    timestamp_utc: DateTime<Utc>,
    expected_log_return: f64,
    initial_log_price: f64,
    initial_price: f64,
) -> Result<ForecastExpectedPathRow, WorkflowError> {
    let expected_price_ratio = expected_log_return.exp();
    let expected_log_price = initial_log_price + expected_log_return;
    let expected_price = initial_price * expected_price_ratio;

    if !expected_price_ratio.is_finite()
        || !expected_log_price.is_finite()
        || !expected_price.is_finite()
    {
        return Err(validation_error(
            "expected_path",
            "expected path row contains a non-finite value",
        ));
    }

    Ok(ForecastExpectedPathRow {
        interval_index,
        timestamp_utc,
        expected_log_return,
        expected_price_ratio,
        expected_log_price,
        expected_price,
    })
}
