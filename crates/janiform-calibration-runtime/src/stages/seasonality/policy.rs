use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_foundations::numeric::support_information_depth;
use janiform_foundations::time::{DAYS_PER_WEEK, MINUTES_PER_DAY};
use janiform_market_time::business_time_clock::FourierBusinessTimeClockFitSettings;
use janiform_market_time::calendar::TimestampEventPreprocessingOutput;

use super::SeasonalityStageExecutionSettings;

pub(super) fn resolve_clock_fit_settings(
    settings: SeasonalityStageExecutionSettings,
    preprocessed: &TimestampEventPreprocessingOutput,
    stage_solver_settings: CalibrationOptimizerSettings,
) -> FourierBusinessTimeClockFitSettings {
    let solver_harmonic_budget = stage_solver_settings
        .max_iterations
        .saturating_sub(1)
        .checked_div(2)
        .unwrap_or(0);

    let intraday_support_depth = support_information_depth(
        distinct_u16_count(preprocessed.exchange_local_minute_of_day.as_slice()).max(1),
    );

    let weekly_support_depth = support_information_depth(
        distinct_u8_count(preprocessed.exchange_local_day_of_week.as_slice()).max(1),
    );

    let annual_support_depth = support_information_depth(
        distinct_u16_count(preprocessed.exchange_local_day_of_year.as_slice()).max(1),
    );

    let maximum_intraday_harmonic_order =
        settings.maximum_intraday_harmonic_order.unwrap_or_else(|| {
            intraday_support_depth
                .min(solver_harmonic_budget)
                .min(MINUTES_PER_DAY / 2)
        });

    let maximum_weekly_harmonic_order =
        settings.maximum_weekly_harmonic_order.unwrap_or_else(|| {
            weekly_support_depth
                .min(solver_harmonic_budget)
                .min(DAYS_PER_WEEK / 2)
        });

    let maximum_annual_harmonic_order = settings
        .maximum_annual_harmonic_order
        .unwrap_or_else(|| annual_support_depth.min(solver_harmonic_budget));

    FourierBusinessTimeClockFitSettings {
        maximum_intraday_harmonic_order,
        maximum_weekly_harmonic_order,
        maximum_annual_harmonic_order,
        maximum_parameter_count: stage_solver_settings.max_iterations.max(1),
        worker_count: stage_solver_settings.cpu_count.max(1),
    }
}

fn distinct_u8_count(values: &[u8]) -> usize {
    let mut values = values.to_vec();

    values.sort_unstable();
    values.dedup();

    values.len()
}

fn distinct_u16_count(values: &[u16]) -> usize {
    let mut values = values.to_vec();

    values.sort_unstable();
    values.dedup();

    values.len()
}
