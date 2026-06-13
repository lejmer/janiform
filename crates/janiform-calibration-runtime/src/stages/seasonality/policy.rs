use std::collections::BTreeMap;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_foundations::numeric::support_information_depth;
use janiform_market_time::business_time_clock::{
    FOURIER_BUSINESS_TIME_CLOCK_AXES, FourierBusinessTimeClockAxis,
    FourierBusinessTimeClockFitSettings,
};
use janiform_market_time::calendar::TimestampEventPreprocessingOutput;

use super::SeasonalityStageExecutionSettings;

pub(super) fn resolve_clock_fit_settings(
    settings: &SeasonalityStageExecutionSettings,
    preprocessed: &TimestampEventPreprocessingOutput,
    stage_solver_settings: CalibrationOptimizerSettings,
) -> FourierBusinessTimeClockFitSettings {
    let solver_harmonic_budget = stage_solver_settings
        .max_iterations
        .saturating_sub(1)
        .checked_div(2)
        .unwrap_or(0);

    let maximum_harmonic_orders = FOURIER_BUSINESS_TIME_CLOCK_AXES
        .into_iter()
        .map(|axis| {
            (
                axis,
                resolve_harmonic_order(HarmonicOrderResolution {
                    configured_order: settings.maximum_harmonic_orders.get(&axis).copied(),
                    observed_support_count: observed_support_count(axis, preprocessed),
                    solver_harmonic_budget,
                    nyquist_limit: axis.nyquist_harmonic_limit(),
                }),
            )
        })
        .collect::<BTreeMap<_, _>>();

    FourierBusinessTimeClockFitSettings {
        maximum_harmonic_orders,
        maximum_parameter_count: stage_solver_settings.max_iterations.max(1),
        worker_count: stage_solver_settings.cpu_count.max(1),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HarmonicOrderResolution {
    configured_order: Option<usize>,
    observed_support_count: usize,
    solver_harmonic_budget: usize,
    nyquist_limit: Option<usize>,
}

fn resolve_harmonic_order(resolution: HarmonicOrderResolution) -> usize {
    resolution.configured_order.unwrap_or_else(|| {
        let mut order = support_information_depth(resolution.observed_support_count.max(1))
            .min(resolution.solver_harmonic_budget);

        if let Some(nyquist_limit) = resolution.nyquist_limit {
            order = order.min(nyquist_limit);
        }

        order
    })
}

fn observed_support_count(
    axis: FourierBusinessTimeClockAxis,
    preprocessed: &TimestampEventPreprocessingOutput,
) -> usize {
    match axis {
        FourierBusinessTimeClockAxis::Intraday => {
            distinct_count(preprocessed.exchange_local_minute_of_day.as_slice())
        }
        FourierBusinessTimeClockAxis::Weekly => {
            distinct_count(preprocessed.exchange_local_day_of_week.as_slice())
        }
        FourierBusinessTimeClockAxis::Annual => {
            distinct_count(preprocessed.exchange_local_day_of_year.as_slice())
        }
    }
}

fn distinct_count<T: Copy + Ord>(values: &[T]) -> usize {
    let mut values = values.to_vec();

    values.sort_unstable();
    values.dedup();

    values.len()
}
