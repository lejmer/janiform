use std::ops::Range;

use janiform_foundations::cancellation::CancellationToken;
use janiform_foundations::parallelism::resolved_cpu_count;
use janiform_work_scheduler::{
    StreamingWorkContext, StreamingWorkOutput, StreamingWorkPoolInput, StreamingWorkProgress,
    balanced_work_ranges, run_streaming_work_pool,
};

use crate::error::WorkflowError;

use super::cdf_projection::isotonic_non_decreasing_projection;
use super::cos_distribution::CosDistribution;
use super::distribution_grid::{
    TERMINAL_DISTRIBUTION_GRID_POLICY, projected_cdf_quantile_band_grid,
};
use super::numeric::usize_to_f64;
use super::output::{
    ForecastDistributionSupport, ForecastQuantile, ForecastTerminalDistributionRow, ForecastWarning,
};
use super::progress::{ForecastProgressEvent, ForecastProgressPhase};
use super::range_progress::{ForecastRangeCollector, ForecastRangeProgress};
use super::request::{ForecastWorkflowRequest, validation_error};
use super::runtime::{ForecastRuntime, ResolvedForecastRequest, engine_forecast_measure};
use super::support::{
    AUTOMATIC_SUPPORT_PILOT_LOG_RETURN_LOWER, AUTOMATIC_SUPPORT_PILOT_LOG_RETURN_UPPER,
    SupportResolution, automatic_support_resolution, explicit_support_resolution,
};

const DEFAULT_QUANTILE_PROBABILITIES: [f64; 7] = [0.01, 0.05, 0.25, 0.50, 0.75, 0.95, 0.99];

pub(super) struct DistributionProjection {
    pub(super) rows: Vec<ForecastTerminalDistributionRow>,
    pub(super) quantiles: Vec<ForecastQuantile>,
    pub(super) support: ForecastDistributionSupport,
    pub(super) warnings: Vec<ForecastWarning>,
}

pub(super) struct TerminalLawProjection {
    pub(super) terminal_law: janiform_engine_pricing::AffineForecastTerminalLaw,
    support_resolution: SupportResolution,
}

impl TerminalLawProjection {
    pub(super) fn has_warnings(&self) -> bool {
        !self.support_resolution.warnings.is_empty()
    }
}

struct DistributionRangeOutput {
    range: Range<usize>,
    result: Result<Vec<ForecastTerminalDistributionRow>, WorkflowError>,
}

pub(super) fn solve_terminal_law(
    request: &ForecastWorkflowRequest,
    resolved: &ResolvedForecastRequest,
    runtime: &ForecastRuntime,
    emit_progress: &mut dyn FnMut(ForecastProgressEvent),
) -> Result<TerminalLawProjection, WorkflowError> {
    emit_progress(
        ForecastProgressEvent::new(ForecastProgressPhase::SolvingTerminalLaw, 0, 3)
            .with_latest_signal("building CDF reference law"),
    );

    let cdf_law = solve_terminal_law_on_support(
        request,
        resolved,
        runtime,
        AUTOMATIC_SUPPORT_PILOT_LOG_RETURN_LOWER,
        AUTOMATIC_SUPPORT_PILOT_LOG_RETURN_UPPER,
        "distribution_support",
    )?;

    emit_progress(
        ForecastProgressEvent::new(ForecastProgressPhase::SolvingTerminalLaw, 1, 3)
            .with_latest_signal("resolving distribution support"),
    );

    let support_resolution = match (
        request.distribution_log_return_lower,
        request.distribution_log_return_upper,
    ) {
        (Some(lower), Some(upper)) => explicit_support_resolution(lower, upper)?,
        (None, None) => automatic_support_resolution(&cdf_law)?,
        _ => {
            return Err(validation_error(
                "distribution_log_return_bounds",
                "explicit support override requires both lower and upper bounds",
            ));
        }
    };

    emit_progress(
        ForecastProgressEvent::new(ForecastProgressPhase::SolvingTerminalLaw, 2, 3)
            .with_latest_signal("solving display law"),
    );

    let terminal_law = solve_terminal_law_on_support(
        request,
        resolved,
        runtime,
        support_resolution.lower,
        support_resolution.upper,
        "terminal_law",
    )?;

    emit_progress(
        ForecastProgressEvent::new(ForecastProgressPhase::SolvingTerminalLaw, 3, 3)
            .with_latest_signal("terminal law solved"),
    );

    Ok(TerminalLawProjection {
        terminal_law,
        support_resolution,
    })
}

pub(super) fn build_distribution_projection(
    request: &ForecastWorkflowRequest,
    runtime: &ForecastRuntime,
    law_projection: TerminalLawProjection,
    cancellation_token: CancellationToken,
    emit_progress: &mut dyn FnMut(ForecastProgressEvent),
) -> Result<DistributionProjection, WorkflowError> {
    let terminal_law = law_projection.terminal_law;
    let density_distribution = CosDistribution::from_law(&terminal_law)?;
    let cdf_distribution = CosDistribution::from_law(&terminal_law)?;

    let quantile_log_returns =
        cdf_distribution.quantiles(DEFAULT_QUANTILE_PROBABILITIES.as_slice())?;

    let node_log_returns = projected_cdf_quantile_band_grid(
        &cdf_distribution,
        terminal_law.node_count,
        DEFAULT_QUANTILE_PROBABILITIES.as_slice(),
        quantile_log_returns.as_slice(),
    )?;

    let worker_count = resolved_cpu_count(request.cpu_count).min(node_log_returns.len().max(1));
    let ranges = balanced_work_ranges(node_log_returns.len(), worker_count);

    let mut collector = ForecastRangeCollector::new(
        ForecastProgressPhase::BuildingDistribution,
        ranges.clone(),
        worker_count,
        "nodes",
        "Nodes",
        emit_progress,
    );

    collector.emit_plan("building terminal distribution");

    run_streaming_work_pool(StreamingWorkPoolInput::new(
        &mut collector,
        cancellation_token,
        ranges,
        worker_count,
        |range, context: StreamingWorkContext<ForecastRangeProgress, DistributionRangeOutput>| {
            compute_distribution_range(
                runtime,
                &density_distribution,
                node_log_returns.as_slice(),
                range,
                context,
            )
        },
        |collector: &mut ForecastRangeCollector<'_, DistributionRangeOutput>,
         progress: StreamingWorkProgress<ForecastRangeProgress>|
         -> Result<(), WorkflowError> { collector.record_progress(progress) },
        |collector: &mut ForecastRangeCollector<'_, DistributionRangeOutput>,
         output: StreamingWorkOutput<DistributionRangeOutput>|
         -> Result<(), WorkflowError> {
            collector.record_output(output.job_position, &output.output.range)?;
            collector.push_output(output.output);

            Ok(())
        },
    ))?;

    let mut range_outputs = collector.into_outputs();

    range_outputs.sort_by_key(|output| output.range.start);

    let mut rows = Vec::with_capacity(node_log_returns.len());

    for output in range_outputs {
        rows.extend(output.result?);
    }

    materialize_distribution_probabilities(rows.as_mut_slice(), &terminal_law, &cdf_distribution)?;

    let quantiles = DEFAULT_QUANTILE_PROBABILITIES
        .iter()
        .copied()
        .zip(quantile_log_returns)
        .map(|(probability, terminal_log_return)| {
            let terminal_price_ratio = terminal_log_return.exp();
            let terminal_price = runtime.pricing_request.market_inputs.spot * terminal_price_ratio;

            ForecastQuantile {
                probability,
                terminal_log_return,
                terminal_price_ratio,
                terminal_price,
            }
        })
        .collect::<Vec<_>>();

    let support = ForecastDistributionSupport {
        policy: law_projection.support_resolution.policy.to_string(),
        lower_probability: law_projection.support_resolution.lower_probability,
        upper_probability: law_projection.support_resolution.upper_probability,
        log_return_lower: density_distribution.support_lower,
        log_return_upper: density_distribution.support_upper,
        node_count: node_log_returns.len(),
        grid_policy: TERMINAL_DISTRIBUTION_GRID_POLICY.to_string(),
    };

    Ok(DistributionProjection {
        rows,
        quantiles,
        support,
        warnings: law_projection.support_resolution.warnings,
    })
}

fn solve_terminal_law_on_support(
    request: &ForecastWorkflowRequest,
    resolved: &ResolvedForecastRequest,
    runtime: &ForecastRuntime,
    lower: f64,
    upper: f64,
    field_name: &'static str,
) -> Result<janiform_engine_pricing::AffineForecastTerminalLaw, WorkflowError> {
    let maturity_interval = runtime.context.interval_delta
        * usize_to_f64(resolved.horizon_interval_count, "horizon_interval_count")?;

    janiform_engine_pricing::solve_affine_forecast_terminal_law(
        janiform_engine_pricing::AffineForecastTerminalLawInput {
            pricing_request: &runtime.pricing_request,
            maturity_count: resolved.horizon_interval_count,
            maturity_interval,
            interval_deltas: runtime.interval_deltas.as_slice(),
            interval_factor_grids: runtime.interval_factor_grids.as_slice(),
            measure: engine_forecast_measure(request.measure),
            terminal_log_return_lower_bound: lower,
            terminal_log_return_upper_bound: upper,
            node_count: request.distribution_node_count,
            cpu_count: request.cpu_count,
        },
    )
    .map_err(|source| validation_error(field_name, source.to_string()))
}

fn compute_distribution_range(
    runtime: &ForecastRuntime,
    density_distribution: &CosDistribution,
    node_log_returns: &[f64],
    range: Range<usize>,
    context: StreamingWorkContext<ForecastRangeProgress, DistributionRangeOutput>,
) -> DistributionRangeOutput {
    let result = compute_distribution_range_result(
        runtime,
        density_distribution,
        node_log_returns,
        &range,
        &context,
    );

    DistributionRangeOutput { range, result }
}

fn compute_distribution_range_result(
    runtime: &ForecastRuntime,
    density_distribution: &CosDistribution,
    node_log_returns: &[f64],
    range: &Range<usize>,
    context: &StreamingWorkContext<ForecastRangeProgress, DistributionRangeOutput>,
) -> Result<Vec<ForecastTerminalDistributionRow>, WorkflowError> {
    let worker_cancellation_token = context.cancellation_token();

    worker_cancellation_token.check_cancelled()?;

    if !context.emit_progress(ForecastRangeProgress::Started) {
        worker_cancellation_token.check_cancelled()?;

        return Err(validation_error(
            "forecast_progress",
            "distribution progress channel closed before range start",
        ));
    }

    let mut rows = Vec::with_capacity(range.len());

    for node_index in range.start..range.end {
        worker_cancellation_token.check_cancelled()?;

        rows.push(distribution_row(
            runtime,
            density_distribution,
            node_log_returns,
            node_index,
        )?);

        let completed_units = node_index + 1 - range.start;
        let _ = context.emit_progress(ForecastRangeProgress::Advanced { completed_units });
    }

    Ok(rows)
}

fn distribution_row(
    runtime: &ForecastRuntime,
    density_distribution: &CosDistribution,
    node_log_returns: &[f64],
    node_index: usize,
) -> Result<ForecastTerminalDistributionRow, WorkflowError> {
    let terminal_log_return = *node_log_returns.get(node_index).ok_or_else(|| {
        validation_error(
            "terminal_distribution",
            "distribution node index exceeded adaptive grid length",
        )
    })?;

    let terminal_price_ratio = terminal_log_return.exp();
    let terminal_price = runtime.pricing_request.market_inputs.spot * terminal_price_ratio;

    let log_return_density =
        projected_nonnegative_density(density_distribution.density(terminal_log_return)?)?;

    let price_density = log_return_density / terminal_price;

    Ok(ForecastTerminalDistributionRow {
        node_index,
        terminal_log_return,
        terminal_price_ratio,
        terminal_price,
        log_return_density,
        price_density,
        probability_mass: 0.0,
        cumulative_probability: 0.0,
    })
}

/// Project finite COS density samples onto the probability-density contract.
///
/// Spectral truncation can ring below zero at point samples even when the CDF
/// projection remains monotone.
fn projected_nonnegative_density(density: f64) -> Result<f64, WorkflowError> {
    if !density.is_finite() {
        return Err(validation_error(
            "terminal_distribution",
            "distribution density sample must be finite",
        ));
    }

    Ok(density.max(0.0))
}

fn materialize_distribution_probabilities(
    rows: &mut [ForecastTerminalDistributionRow],
    terminal_law: &janiform_engine_pricing::AffineForecastTerminalLaw,
    cdf_distribution: &CosDistribution,
) -> Result<(), WorkflowError> {
    let mut cdf_points = Vec::with_capacity(rows.len().saturating_mul(2).saturating_add(1));

    let mut previous_log_return = None;

    for row in rows.iter() {
        let lower_edge = match previous_log_return {
            Some(previous) => midpoint_log_return(previous, row.terminal_log_return)?,
            None => terminal_law.terminal_log_return_lower_bound,
        };

        cdf_points.push(cdf_distribution.projected_cdf(lower_edge)?);
        cdf_points.push(cdf_distribution.projected_cdf(row.terminal_log_return)?);

        previous_log_return = Some(row.terminal_log_return);
    }

    cdf_points.push(cdf_distribution.projected_cdf(terminal_law.terminal_log_return_upper_bound)?);

    let projected_cdf_points = isotonic_non_decreasing_projection(cdf_points.as_slice())?;

    for (row_position, row) in rows.iter_mut().enumerate() {
        let lower_edge_position = row_position * 2;
        let center_position = lower_edge_position + 1;
        let upper_edge_position = lower_edge_position + 2;

        let lower_edge_cdf =
            projected_cdf_point(projected_cdf_points.as_slice(), lower_edge_position)?;

        let center_cdf = projected_cdf_point(projected_cdf_points.as_slice(), center_position)?;

        let upper_edge_cdf =
            projected_cdf_point(projected_cdf_points.as_slice(), upper_edge_position)?;

        row.probability_mass = upper_edge_cdf - lower_edge_cdf;
        row.cumulative_probability = center_cdf;
    }

    Ok(())
}

fn projected_cdf_point(points: &[f64], position: usize) -> Result<f64, WorkflowError> {
    points.get(position).copied().ok_or_else(|| {
        validation_error(
            "terminal_distribution",
            "projected distribution CDF grid was shorter than the terminal distribution rows",
        )
    })
}

fn midpoint_log_return(lower: f64, upper: f64) -> Result<f64, WorkflowError> {
    if !lower.is_finite() || !upper.is_finite() || upper < lower {
        return Err(validation_error(
            "terminal_distribution",
            "distribution grid cell edges require finite ordered log-return nodes",
        ));
    }

    let midpoint = lower + (upper - lower) / 2.0;
    if !midpoint.is_finite() {
        return Err(validation_error(
            "terminal_distribution",
            "distribution grid cell midpoint must be finite",
        ));
    }

    Ok(midpoint)
}

#[cfg(test)]
mod tests {
    use super::DEFAULT_QUANTILE_PROBABILITIES;

    #[test]
    fn forecast_default_quantiles_match_contract() {
        assert_eq!(
            DEFAULT_QUANTILE_PROBABILITIES,
            [0.01, 0.05, 0.25, 0.50, 0.75, 0.95, 0.99]
        );
    }

    #[test]
    fn projected_nonnegative_density_enforces_density_contract() {
        assert_eq!(super::projected_nonnegative_density(-1.0).unwrap(), 0.0);
        assert_eq!(super::projected_nonnegative_density(2.0).unwrap(), 2.0);
        assert!(super::projected_nonnegative_density(f64::NAN).is_err());
    }
}
