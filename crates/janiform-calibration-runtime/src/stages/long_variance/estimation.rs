use janiform_foundations::numeric::{
    compute_finite_median_or_zero, pearson_correlation, sample_variance,
};
use janiform_foundations::time::{NANOS_PER_HOUR, duration_nanos_to_hours};
use janiform_model_kernels::long_variance::LongVarianceParameters;

use crate::stages::joint_metrics::JointRefinementAggregatedIntervalObservation;
use crate::stages::long_variance::feedback::{
    event_loading_fit_coverage_ratio, return_energy_loading_estimate, side_event_loading_estimate,
};
use crate::stages::long_variance::metrics::{
    finite_long_variance_squared_returns, mean_long_variance_squared_return,
};
use crate::stages::long_variance::policy::{
    LONG_VARIANCE_STANDARD_CIR_VARIANCE_PROXY_BOUNDARY, LongVarianceFeedbackLoadings,
    LongVarianceStationaryEventRates, long_variance_mean_reversion_rate_calibration_upper_bound,
    project_long_variance_feedback_loadings_to_stationary_budget,
};

const UTC_HOUR_BUCKET_COUNT: usize = 24;
const LONG_VARIANCE_PERSISTENCE_LAG_HOURS: [i64; 5] = [1, 2, 4, 8, 24];

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LongVarianceParameterEstimation {
    pub(crate) parameters: LongVarianceParameters,
    pub(crate) diagnostics: LongVarianceParameterEstimationDiagnostics,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LongVarianceParameterEstimationDiagnostics {
    pub(crate) persistence_lag_diagnostics: Vec<LongVariancePersistenceLagDiagnostic>,
    pub(crate) selected_mean_reversion_rate: f64,
    pub(crate) event_loading_fit_candidate_pair_count: usize,
    pub(crate) event_loading_fit_pair_count: usize,
    pub(crate) event_loading_fit_coverage_ratio: f64,
    pub(crate) stationary_return_energy_rate_per_hour: f64,
    pub(crate) return_energy_fit_candidate_pair_count: usize,
    pub(crate) return_energy_fit_pair_count: usize,
    pub(crate) return_energy_fit_coverage_ratio: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LongVariancePersistenceLagDiagnostic {
    pub(crate) lag_hours: f64,
    pub(crate) pair_count: usize,
    pub(crate) correlation: Option<f64>,
    pub(crate) implied_mean_reversion_rate: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct DeseasonalizedSquaredReturnObservation {
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
    deseasonalized_squared_return: f64,
}

pub(crate) fn estimate_long_variance_parameters(
    baseline_parameters: LongVarianceParameters,
    stationary_event_rates: LongVarianceStationaryEventRates,
    return_intervals: &[JointRefinementAggregatedIntervalObservation],
    activity_intervals: &[JointRefinementAggregatedIntervalObservation],
) -> LongVarianceParameterEstimation {
    let persistence_lag_diagnostics =
        hourly_squared_return_persistence_lag_diagnostics(return_intervals);

    let persistence_decay_rate = selected_hourly_squared_return_persistence_decay_rate(
        persistence_lag_diagnostics.as_slice(),
    )
    .unwrap_or(baseline_parameters.mean_reversion_rate);

    let mean_reversion_rate =
        persistence_decay_rate.min(long_variance_mean_reversion_rate_calibration_upper_bound());

    let event_loading_estimation =
        side_event_loading_estimate(activity_intervals, mean_reversion_rate);
    let return_energy_loading_estimation =
        return_energy_loading_estimate(return_intervals, mean_reversion_rate);

    let feedback_loadings = project_long_variance_feedback_loadings_to_stationary_budget(
        LongVarianceFeedbackLoadings {
            stationary_return_energy_rate_per_hour: return_energy_loading_estimation
                .stationary_return_energy_rate_per_hour,
            return_energy_loading: return_energy_loading_estimation.return_energy_loading,
            buy_event_loading: event_loading_estimation.event_loadings.buy_event_loading,
            sell_event_loading: event_loading_estimation.event_loadings.sell_event_loading,
        },
        stationary_event_rates,
        mean_reversion_rate,
    );

    let vol_of_vol = long_variance_vol_of_vol_estimate(mean_reversion_rate, return_intervals)
        .unwrap_or(baseline_parameters.vol_of_vol);

    let parameters = LongVarianceParameters::try_new(
        mean_reversion_rate,
        vol_of_vol,
        feedback_loadings.stationary_return_energy_rate_per_hour,
        feedback_loadings.return_energy_loading,
        feedback_loadings.buy_event_loading,
        feedback_loadings.sell_event_loading,
    )
    .unwrap_or(baseline_parameters);

    LongVarianceParameterEstimation {
        parameters,
        diagnostics: LongVarianceParameterEstimationDiagnostics {
            persistence_lag_diagnostics,
            selected_mean_reversion_rate: parameters.mean_reversion_rate,
            event_loading_fit_candidate_pair_count: event_loading_estimation.candidate_pair_count,
            event_loading_fit_pair_count: event_loading_estimation.fit_pair_count,
            event_loading_fit_coverage_ratio: event_loading_fit_coverage_ratio(
                event_loading_estimation.fit_pair_count,
                event_loading_estimation.candidate_pair_count,
            ),
            stationary_return_energy_rate_per_hour: parameters
                .stationary_return_energy_rate_per_hour,
            return_energy_fit_candidate_pair_count: return_energy_loading_estimation
                .candidate_pair_count,
            return_energy_fit_pair_count: return_energy_loading_estimation.fit_pair_count,
            return_energy_fit_coverage_ratio: event_loading_fit_coverage_ratio(
                return_energy_loading_estimation.fit_pair_count,
                return_energy_loading_estimation.candidate_pair_count,
            ),
        },
    }
}

fn selected_hourly_squared_return_persistence_decay_rate(
    lag_diagnostics: &[LongVariancePersistenceLagDiagnostic],
) -> Option<f64> {
    let implied_rates = lag_diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.implied_mean_reversion_rate)
        .collect::<Vec<_>>();

    if implied_rates.is_empty() {
        return None;
    }

    let selected_rate = compute_finite_median_or_zero(implied_rates.as_slice());

    (selected_rate.is_finite() && selected_rate > 0.0).then_some(selected_rate)
}

fn hourly_squared_return_persistence_lag_diagnostics(
    hourly_intervals: &[JointRefinementAggregatedIntervalObservation],
) -> Vec<LongVariancePersistenceLagDiagnostic> {
    let observations = deseasonalized_hourly_squared_return_observations(hourly_intervals);

    LONG_VARIANCE_PERSISTENCE_LAG_HOURS
        .into_iter()
        .map(|lag_hours| persistence_lag_diagnostic(observations.as_slice(), lag_hours))
        .collect()
}

fn persistence_lag_diagnostic(
    observations: &[DeseasonalizedSquaredReturnObservation],
    lag_hours: i64,
) -> LongVariancePersistenceLagDiagnostic {
    let lag_nanos = lag_hours.saturating_mul(NANOS_PER_HOUR);

    let (lagged_squared_returns, current_squared_returns) =
        lagged_squared_return_pairs(observations, lag_nanos);

    let pair_count = lagged_squared_returns.len();

    let persistence_correlation = pearson_correlation(
        lagged_squared_returns.as_slice(),
        current_squared_returns.as_slice(),
    );

    let correlation = persistence_correlation
        .is_finite()
        .then_some(persistence_correlation);

    let lag_hours = duration_nanos_to_hours(lag_nanos);

    let implied_mean_reversion_rate = correlation
        .filter(|correlation| *correlation > 0.0 && *correlation < 1.0)
        .and_then(|correlation| {
            let decay_rate = -correlation.ln() / lag_hours;

            (decay_rate.is_finite() && decay_rate > 0.0).then_some(decay_rate)
        });

    LongVariancePersistenceLagDiagnostic {
        lag_hours,
        pair_count,
        correlation,
        implied_mean_reversion_rate,
    }
}

fn lagged_squared_return_pairs(
    observations: &[DeseasonalizedSquaredReturnObservation],
    lag_nanos: i64,
) -> (Vec<f64>, Vec<f64>) {
    if observations.len() < 2 || lag_nanos <= 0 {
        return (Vec::new(), Vec::new());
    }

    let mut lagged_squared_returns = Vec::new();
    let mut current_squared_returns = Vec::new();
    let mut lagged_observation_position = 0usize;

    for current_observation in observations {
        let Some(target_start_utc_nanos) = current_observation
            .interval_start_utc_nanos
            .checked_sub(lag_nanos)
        else {
            continue;
        };

        while observations
            .get(lagged_observation_position)
            .is_some_and(|lagged_observation| {
                lagged_observation.interval_start_utc_nanos < target_start_utc_nanos
            })
        {
            lagged_observation_position = lagged_observation_position.saturating_add(1);
        }

        let Some(lagged_observation) = observations.get(lagged_observation_position) else {
            continue;
        };

        if lagged_observation.interval_start_utc_nanos != target_start_utc_nanos {
            continue;
        }

        lagged_squared_returns.push(lagged_observation.deseasonalized_squared_return);
        current_squared_returns.push(current_observation.deseasonalized_squared_return);
    }

    (lagged_squared_returns, current_squared_returns)
}

fn deseasonalized_hourly_squared_return_observations(
    hourly_intervals: &[JointRefinementAggregatedIntervalObservation],
) -> Vec<DeseasonalizedSquaredReturnObservation> {
    let squared_return_observations = hourly_intervals
        .iter()
        .filter(|interval| interval.log_return.is_finite())
        .filter_map(|interval| {
            let squared_return = interval.log_return * interval.log_return;
            if !squared_return.is_finite() {
                return None;
            }

            let hour_bucket = utc_hour_bucket(interval.interval_start_utc_nanos);

            Some((*interval, hour_bucket, squared_return))
        })
        .collect::<Vec<_>>();

    let mut squared_return_sum_by_hour = [0.0; UTC_HOUR_BUCKET_COUNT];
    let mut squared_return_count_by_hour = [0usize; UTC_HOUR_BUCKET_COUNT];

    for (_, hour_bucket, squared_return) in squared_return_observations.iter().copied() {
        squared_return_sum_by_hour[hour_bucket] += squared_return;
        squared_return_count_by_hour[hour_bucket] =
            squared_return_count_by_hour[hour_bucket].saturating_add(1);
    }

    let mut observations = squared_return_observations
        .into_iter()
        .filter_map(|(interval, hour_bucket, squared_return)| {
            let bucket_count = squared_return_count_by_hour[hour_bucket];
            if bucket_count == 0 {
                return None;
            }

            let bucket_mean = squared_return_sum_by_hour[hour_bucket] / bucket_count as f64;
            if !bucket_mean.is_finite() || bucket_mean <= 0.0 {
                return None;
            }

            let deseasonalized_squared_return = squared_return / bucket_mean;

            (deseasonalized_squared_return.is_finite() && deseasonalized_squared_return >= 0.0)
                .then_some(DeseasonalizedSquaredReturnObservation {
                    interval_start_utc_nanos: interval.interval_start_utc_nanos,
                    interval_end_utc_nanos: interval.interval_end_utc_nanos,
                    deseasonalized_squared_return,
                })
        })
        .collect::<Vec<_>>();

    observations.sort_by(|left, right| {
        left.interval_start_utc_nanos
            .cmp(&right.interval_start_utc_nanos)
            .then(
                left.interval_end_utc_nanos
                    .cmp(&right.interval_end_utc_nanos),
            )
    });

    observations
}

fn utc_hour_bucket(timestamp_utc_nanos: i64) -> usize {
    timestamp_utc_nanos
        .div_euclid(NANOS_PER_HOUR)
        .rem_euclid(UTC_HOUR_BUCKET_COUNT as i64) as usize
}

fn long_variance_vol_of_vol_estimate(
    mean_reversion_rate: f64,
    hourly_intervals: &[JointRefinementAggregatedIntervalObservation],
) -> Option<f64> {
    if !mean_reversion_rate.is_finite() || mean_reversion_rate <= 0.0 {
        return None;
    }

    let mean_squared_return = mean_long_variance_squared_return(hourly_intervals)?;

    let normalized_squared_returns = finite_long_variance_squared_returns(hourly_intervals)
        .into_iter()
        .map(|squared_return| squared_return / mean_squared_return)
        .collect::<Vec<_>>();

    let variance_proxy = sample_variance(normalized_squared_returns.as_slice())?;
    if !variance_proxy.is_finite() || variance_proxy < 0.0 {
        return None;
    }

    let projected_variance_proxy =
        variance_proxy.min(LONG_VARIANCE_STANDARD_CIR_VARIANCE_PROXY_BOUNDARY);

    Some((2.0 * mean_reversion_rate * projected_variance_proxy).sqrt())
}

#[cfg(test)]
mod tests {
    use janiform_foundations::time::{NANOS_PER_FIVE_MINUTES, NANOS_PER_HOUR};
    use janiform_model_kernels::long_variance::LongVarianceParameters;

    use super::{
        LongVariancePersistenceLagDiagnostic, estimate_long_variance_parameters,
        hourly_squared_return_persistence_lag_diagnostics,
        selected_hourly_squared_return_persistence_decay_rate,
    };
    use crate::stages::joint_metrics::JointRefinementAggregatedIntervalObservation;
    use crate::stages::long_variance::policy::{
        LongVarianceStationaryEventRates, long_variance_feedback_drift_budget,
        long_variance_vol_of_vol_upper_bound_for_mean_reversion_rate,
    };

    fn default_stationary_event_rates() -> LongVarianceStationaryEventRates {
        LongVarianceStationaryEventRates {
            buy_event_rate_per_hour: 1.0,
            sell_event_rate_per_hour: 1.0,
        }
    }

    fn interval(
        interval_start_utc_nanos: i64,
        interval_duration_nanos: i64,
        log_return: f64,
        tick_count: usize,
        sell_share_proxy: f64,
    ) -> JointRefinementAggregatedIntervalObservation {
        JointRefinementAggregatedIntervalObservation {
            interval_start_utc_nanos,
            interval_end_utc_nanos: interval_start_utc_nanos + interval_duration_nanos,
            log_return,
            tick_count,
            sell_share_proxy,
        }
    }

    fn deterministic_persistent_hourly_intervals(
        interval_count: i64,
        decay_rate: f64,
    ) -> Vec<JointRefinementAggregatedIntervalObservation> {
        let persistence = (-decay_rate).exp();

        let mut state = 0.0;

        (0..interval_count)
            .map(|hour_position| {
                let innovation = ((hour_position * 17).rem_euclid(11) as f64 - 5.0) / 5.0;
                state = persistence * state + innovation;

                let normalized_squared_return = 1.0 + 0.05 * state;
                let log_return = normalized_squared_return.sqrt();

                interval(
                    hour_position * NANOS_PER_HOUR,
                    NANOS_PER_HOUR,
                    log_return,
                    100,
                    0.50,
                )
            })
            .collect()
    }

    #[test]
    fn hourly_persistence_reports_fixed_lag_diagnostics() {
        let hourly_intervals = deterministic_persistent_hourly_intervals(96, 0.10);

        let diagnostics =
            hourly_squared_return_persistence_lag_diagnostics(hourly_intervals.as_slice());

        assert_eq!(diagnostics.len(), 5);
        assert_eq!(diagnostics[0].lag_hours, 1.0);
        assert_eq!(diagnostics[1].lag_hours, 2.0);
        assert_eq!(diagnostics[2].lag_hours, 4.0);
        assert_eq!(diagnostics[3].lag_hours, 8.0);
        assert_eq!(diagnostics[4].lag_hours, 24.0);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.pair_count > 0)
        );
    }

    #[test]
    fn hourly_persistence_recovers_known_data_derived_half_life() {
        let expected_decay_rate = std::f64::consts::LN_2 / 6.0;
        let diagnostics = [1.0, 2.0, 4.0, 8.0, 24.0]
            .into_iter()
            .map(|lag_hours| LongVariancePersistenceLagDiagnostic {
                lag_hours,
                pair_count: 100,
                correlation: Some((-expected_decay_rate * lag_hours).exp()),
                implied_mean_reversion_rate: Some(expected_decay_rate),
            })
            .collect::<Vec<_>>();

        assert_eq!(
            selected_hourly_squared_return_persistence_decay_rate(diagnostics.as_slice()),
            Some(expected_decay_rate)
        );
    }

    #[test]
    fn hourly_persistence_median_selection_limits_adjacent_lag_noise() {
        let diagnostics = [
            LongVariancePersistenceLagDiagnostic {
                lag_hours: 1.0,
                pair_count: 100,
                correlation: Some((-5.0_f64).exp()),
                implied_mean_reversion_rate: Some(5.0),
            },
            LongVariancePersistenceLagDiagnostic {
                lag_hours: 2.0,
                pair_count: 100,
                correlation: Some((-0.25_f64 * 2.0).exp()),
                implied_mean_reversion_rate: Some(0.25),
            },
            LongVariancePersistenceLagDiagnostic {
                lag_hours: 4.0,
                pair_count: 100,
                correlation: Some((-0.30_f64 * 4.0).exp()),
                implied_mean_reversion_rate: Some(0.30),
            },
            LongVariancePersistenceLagDiagnostic {
                lag_hours: 8.0,
                pair_count: 100,
                correlation: Some((-0.35_f64 * 8.0).exp()),
                implied_mean_reversion_rate: Some(0.35),
            },
            LongVariancePersistenceLagDiagnostic {
                lag_hours: 24.0,
                pair_count: 100,
                correlation: Some((-0.40_f64 * 24.0).exp()),
                implied_mean_reversion_rate: Some(0.40),
            },
        ];

        assert_eq!(
            selected_hourly_squared_return_persistence_decay_rate(&diagnostics),
            Some(0.35)
        );
    }

    #[test]
    fn estimator_keeps_neutral_event_loadings_for_flat_activity_relation() {
        let activity_intervals = [
            interval(0, NANOS_PER_FIVE_MINUTES, 0.01, 10, 0.50),
            interval(
                NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                -0.01,
                10,
                0.50,
            ),
        ];

        let hourly_intervals = [
            interval(0, NANOS_PER_HOUR, 0.02, 100, 0.50),
            interval(NANOS_PER_HOUR, NANOS_PER_HOUR, -0.02, 100, 0.50),
        ];

        let estimation = estimate_long_variance_parameters(
            LongVarianceParameters::neutral(),
            default_stationary_event_rates(),
            hourly_intervals.as_slice(),
            activity_intervals.as_slice(),
        );

        assert_eq!(estimation.parameters.buy_event_loading, 0.0);
        assert_eq!(estimation.parameters.sell_event_loading, 0.0);
        assert_eq!(
            estimation
                .diagnostics
                .event_loading_fit_candidate_pair_count,
            1
        );
        assert_eq!(estimation.diagnostics.event_loading_fit_pair_count, 1);
    }

    #[test]
    fn estimator_moves_buy_loading_when_buy_activity_tracks_future_squared_returns() {
        let activity_intervals = [
            interval(0, NANOS_PER_FIVE_MINUTES, 0.01, 30, 0.0),
            interval(
                NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.05,
                10,
                0.0,
            ),
            interval(
                2 * NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.01,
                30,
                0.0,
            ),
            interval(
                3 * NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.05,
                10,
                0.0,
            ),
            interval(
                4 * NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.01,
                30,
                0.0,
            ),
            interval(
                5 * NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.05,
                10,
                0.0,
            ),
        ];

        let hourly_intervals = [
            interval(0, NANOS_PER_HOUR, 0.01, 100, 0.50),
            interval(NANOS_PER_HOUR, NANOS_PER_HOUR, 0.02, 100, 0.50),
            interval(2 * NANOS_PER_HOUR, NANOS_PER_HOUR, 0.03, 100, 0.50),
        ];

        let estimation = estimate_long_variance_parameters(
            LongVarianceParameters::neutral(),
            default_stationary_event_rates(),
            hourly_intervals.as_slice(),
            activity_intervals.as_slice(),
        );

        assert!(estimation.parameters.buy_event_loading > 0.0);
        assert_eq!(estimation.parameters.sell_event_loading, 0.0);
        assert_eq!(estimation.diagnostics.event_loading_fit_pair_count, 5);
    }

    #[test]
    fn estimator_moves_sell_loading_when_sell_activity_tracks_future_squared_returns() {
        let activity_intervals = [
            interval(0, NANOS_PER_FIVE_MINUTES, 0.01, 30, 1.0),
            interval(
                NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.05,
                10,
                1.0,
            ),
            interval(
                2 * NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.01,
                30,
                1.0,
            ),
            interval(
                3 * NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.05,
                10,
                1.0,
            ),
            interval(
                4 * NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.01,
                30,
                1.0,
            ),
            interval(
                5 * NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.05,
                10,
                1.0,
            ),
        ];

        let hourly_intervals = [
            interval(0, NANOS_PER_HOUR, 0.01, 100, 0.50),
            interval(NANOS_PER_HOUR, NANOS_PER_HOUR, 0.02, 100, 0.50),
            interval(2 * NANOS_PER_HOUR, NANOS_PER_HOUR, 0.03, 100, 0.50),
        ];

        let estimation = estimate_long_variance_parameters(
            LongVarianceParameters::neutral(),
            default_stationary_event_rates(),
            hourly_intervals.as_slice(),
            activity_intervals.as_slice(),
        );

        assert_eq!(estimation.parameters.buy_event_loading, 0.0);
        assert!(estimation.parameters.sell_event_loading > 0.0);
        assert_eq!(estimation.diagnostics.event_loading_fit_pair_count, 5);
    }

    #[test]
    fn estimator_projects_event_loadings_into_stationary_budget() {
        let activity_intervals = [
            interval(0, NANOS_PER_FIVE_MINUTES, 0.01, 200, 0.0),
            interval(
                NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.10,
                10,
                0.0,
            ),
            interval(
                2 * NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.01,
                200,
                0.0,
            ),
            interval(
                3 * NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.10,
                10,
                0.0,
            ),
            interval(
                4 * NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.01,
                200,
                0.0,
            ),
            interval(
                5 * NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                0.10,
                10,
                0.0,
            ),
        ];

        let hourly_intervals = [
            interval(0, NANOS_PER_HOUR, 0.02, 100, 0.50),
            interval(NANOS_PER_HOUR, NANOS_PER_HOUR, -0.02, 100, 0.50),
        ];

        let raw_event_loadings =
            crate::stages::long_variance::feedback::side_event_loading_estimate(
                activity_intervals.as_slice(),
                LongVarianceParameters::neutral().mean_reversion_rate,
            )
            .event_loadings;

        let event_feedback_budget = long_variance_feedback_drift_budget(
            LongVarianceParameters::neutral().mean_reversion_rate,
        )
        .expect("neutral long variance mean reversion should have a feedback budget");

        let stationary_event_rates = LongVarianceStationaryEventRates {
            buy_event_rate_per_hour: 2.0 * event_feedback_budget
                / raw_event_loadings.buy_event_loading,
            sell_event_rate_per_hour: 1.0,
        };

        let estimation = estimate_long_variance_parameters(
            LongVarianceParameters::neutral(),
            stationary_event_rates,
            hourly_intervals.as_slice(),
            activity_intervals.as_slice(),
        );

        assert!(estimation.parameters.buy_event_loading < raw_event_loadings.buy_event_loading);
        assert_eq!(
            estimation.parameters.sell_event_loading,
            raw_event_loadings.sell_event_loading
        );
        assert_eq!(
            estimation.parameters.mean_reversion_rate,
            LongVarianceParameters::neutral().mean_reversion_rate
        );
    }

    #[test]
    fn estimator_projects_vol_of_vol_into_restart_domain() {
        let activity_intervals = [
            interval(0, NANOS_PER_FIVE_MINUTES, 0.01, 10, 0.50),
            interval(
                NANOS_PER_FIVE_MINUTES,
                NANOS_PER_FIVE_MINUTES,
                -0.01,
                10,
                0.50,
            ),
        ];

        let hourly_intervals = [
            interval(0, NANOS_PER_HOUR, 0.001, 100, 0.50),
            interval(NANOS_PER_HOUR, NANOS_PER_HOUR, 0.001, 100, 0.50),
            interval(2 * NANOS_PER_HOUR, NANOS_PER_HOUR, 1.0, 100, 0.50),
        ];

        let estimation = estimate_long_variance_parameters(
            LongVarianceParameters::neutral(),
            default_stationary_event_rates(),
            hourly_intervals.as_slice(),
            activity_intervals.as_slice(),
        );

        let upper_bound = long_variance_vol_of_vol_upper_bound_for_mean_reversion_rate(
            estimation.parameters.mean_reversion_rate,
        )
        .expect("positive mean reversion should have a vol-of-vol bound");

        assert!(estimation.parameters.vol_of_vol <= upper_bound);
    }

    #[test]
    fn coarse_only_return_input_estimates_return_parameters_and_zero_event_loadings() {
        let return_intervals = deterministic_persistent_hourly_intervals(24 * 120, 0.08);

        let estimation = estimate_long_variance_parameters(
            LongVarianceParameters::neutral(),
            default_stationary_event_rates(),
            return_intervals.as_slice(),
            &[],
        );

        assert!(estimation.parameters.mean_reversion_rate > 0.0);
        assert!(estimation.parameters.vol_of_vol > 0.0);
        assert_eq!(estimation.parameters.buy_event_loading, 0.0);
        assert_eq!(estimation.parameters.sell_event_loading, 0.0);
        assert_eq!(
            estimation
                .diagnostics
                .event_loading_fit_candidate_pair_count,
            0
        );
        assert_eq!(estimation.diagnostics.event_loading_fit_pair_count, 0);
    }
}
