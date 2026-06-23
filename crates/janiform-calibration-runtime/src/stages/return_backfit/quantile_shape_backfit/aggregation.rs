use std::collections::BTreeMap;

use janiform_foundations::numeric::validate_positive_number;
use janiform_foundations::time::{
    MINUTES_PER_HOUR, NANOS_PER_HOUR, NANOS_PER_MINUTE, floor_to_interval_utc_nanos,
    format_duration_spec_nanos,
};

use super::DynamicContinuousReturnObservation;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct AggregatedQuantileCalibrationSample {
    pub(super) horizon: ReturnBackfitAggregationHorizon,
    pub(super) observed_returns: Vec<f64>,
    pub(super) business_time_scales: Vec<f64>,
    pub(super) sell_share_proxies: Vec<f64>,
}

impl AggregatedQuantileCalibrationSample {
    pub(super) fn sample_count(&self) -> usize {
        self.observed_returns
            .len()
            .min(self.business_time_scales.len())
            .min(self.sell_share_proxies.len())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ReturnBackfitAggregationHorizon {
    pub(super) interval_nanos: Option<i64>,
}

impl ReturnBackfitAggregationHorizon {
    fn fixed_interval(interval_nanos: i64) -> Self {
        Self {
            interval_nanos: Some(interval_nanos),
        }
    }

    fn tick() -> Self {
        Self {
            interval_nanos: None,
        }
    }

    pub(super) fn label(self) -> String {
        match self.interval_nanos {
            Some(interval_nanos) => format_duration_spec_nanos(interval_nanos)
                .unwrap_or_else(|| format!("{interval_nanos}ns")),
            None => "tick".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct AggregatedQuantileReturnAccumulator {
    observed_log_return: f64,
    business_time_scale: f64,
    sell_business_time_scale: f64,
    sell_share_sum: f64,
    member_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FinishedAggregatedQuantileReturn {
    observed_return: f64,
    business_time_scale: f64,
    sell_share_proxy: f64,
}

pub(super) fn select_quantile_shape_aggregation_sample(
    observations: &[DynamicContinuousReturnObservation],
    minimum_sample_count: usize,
    warnings: &mut Vec<String>,
) -> Result<AggregatedQuantileCalibrationSample, String> {
    if observations.is_empty() {
        return Err(
            "return-backfit requires at least one continuous return observation".to_string(),
        );
    }

    for horizon in fixed_intraday_aggregation_horizons() {
        let sample = aggregate_quantile_calibration_sample(observations, horizon)?;
        if sample.sample_count() >= minimum_sample_count {
            return Ok(sample);
        }
    }

    let tick_sample = aggregate_quantile_calibration_sample(
        observations,
        ReturnBackfitAggregationHorizon::tick(),
    )?;

    if tick_sample.sample_count() < minimum_sample_count {
        warnings.push(format!(
            "return-backfit quantile-shape sample count {} is below the policy minimum {} at tick resolution",
            tick_sample.sample_count(),
            minimum_sample_count
        ));
    }

    Ok(tick_sample)
}

fn aggregate_quantile_calibration_sample(
    observations: &[DynamicContinuousReturnObservation],
    horizon: ReturnBackfitAggregationHorizon,
) -> Result<AggregatedQuantileCalibrationSample, String> {
    match horizon.interval_nanos {
        Some(interval_nanos) => {
            aggregate_fixed_interval_quantile_calibration_sample(observations, interval_nanos)
        }
        None => aggregate_tick_quantile_calibration_sample(observations),
    }
}

fn aggregate_tick_quantile_calibration_sample(
    observations: &[DynamicContinuousReturnObservation],
) -> Result<AggregatedQuantileCalibrationSample, String> {
    let mut observed_returns = Vec::with_capacity(observations.len());
    let mut business_time_scales = Vec::with_capacity(observations.len());
    let mut sell_share_proxies = Vec::with_capacity(observations.len());

    for observation in observations {
        validate_positive_number(
            observation.dynamic_business_time_scale,
            "dynamic_business_time_scale",
        )
        .map_err(|source| {
            format!("return-backfit tick aggregation has invalid dynamic scale: {source}")
        })?;

        observed_returns.push(observation.observation.log_return);
        business_time_scales.push(observation.dynamic_business_time_scale);
        sell_share_proxies.push(observation.observation.sell_share_proxy.clamp(0.0, 1.0));
    }

    Ok(AggregatedQuantileCalibrationSample {
        horizon: ReturnBackfitAggregationHorizon::tick(),
        observed_returns,
        business_time_scales,
        sell_share_proxies,
    })
}

fn aggregate_fixed_interval_quantile_calibration_sample(
    observations: &[DynamicContinuousReturnObservation],
    interval_nanos: i64,
) -> Result<AggregatedQuantileCalibrationSample, String> {
    if interval_nanos <= 0 || interval_nanos > NANOS_PER_HOUR {
        return Err(format!(
            "return-backfit aggregation interval must be in (0, 1h], found {interval_nanos}"
        ));
    }

    let mut buckets = BTreeMap::<i64, AggregatedQuantileReturnAccumulator>::new();

    for observation in observations {
        validate_positive_number(
            observation.dynamic_business_time_scale,
            "dynamic_business_time_scale",
        )
        .map_err(|source| {
            format!("return-backfit fixed-interval aggregation has invalid dynamic scale: {source}")
        })?;

        let bucket_start = floor_to_interval_utc_nanos(
            observation.observation.return_timestamp_utc_nanos,
            interval_nanos,
        );

        buckets
            .entry(bucket_start)
            .or_default()
            .accumulate(*observation);
    }

    let mut observed_returns = Vec::with_capacity(buckets.len());
    let mut business_time_scales = Vec::with_capacity(buckets.len());
    let mut sell_share_proxies = Vec::with_capacity(buckets.len());

    for accumulator in buckets.into_values() {
        let Some(finished_observation) = accumulator.finish() else {
            continue;
        };

        observed_returns.push(finished_observation.observed_return);
        business_time_scales.push(finished_observation.business_time_scale);
        sell_share_proxies.push(finished_observation.sell_share_proxy);
    }

    Ok(AggregatedQuantileCalibrationSample {
        horizon: ReturnBackfitAggregationHorizon::fixed_interval(interval_nanos),
        observed_returns,
        business_time_scales,
        sell_share_proxies,
    })
}

impl AggregatedQuantileReturnAccumulator {
    fn accumulate(&mut self, observation: DynamicContinuousReturnObservation) {
        let sell_share_proxy = observation.observation.sell_share_proxy.clamp(0.0, 1.0);

        self.observed_log_return += observation.observation.log_return;
        self.business_time_scale += observation.dynamic_business_time_scale;
        self.sell_business_time_scale += observation.dynamic_business_time_scale * sell_share_proxy;
        self.sell_share_sum += sell_share_proxy;
        self.member_count = self.member_count.saturating_add(1);
    }

    fn finish(self) -> Option<FinishedAggregatedQuantileReturn> {
        if self.member_count == 0
            || !self.business_time_scale.is_finite()
            || self.business_time_scale <= 0.0
        {
            return None;
        }

        let mean_sell_share = self.sell_share_sum / self.member_count as f64;
        let sell_share_proxy = if self.sell_business_time_scale.is_finite() {
            self.sell_business_time_scale / self.business_time_scale
        } else {
            mean_sell_share
        };

        Some(FinishedAggregatedQuantileReturn {
            observed_return: self.observed_log_return,
            business_time_scale: self.business_time_scale,
            sell_share_proxy: sell_share_proxy.clamp(0.0, 1.0),
        })
    }
}

fn fixed_intraday_aggregation_horizons() -> Vec<ReturnBackfitAggregationHorizon> {
    (1..=MINUTES_PER_HOUR)
        .rev()
        .filter(|minute_count| MINUTES_PER_HOUR.is_multiple_of(*minute_count))
        .map(|minute_count| {
            ReturnBackfitAggregationHorizon::fixed_interval(minute_count as i64 * NANOS_PER_MINUTE)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use janiform_foundations::time::{NANOS_PER_FIFTEEN_MINUTES, NANOS_PER_HOUR, NANOS_PER_MINUTE};
    use janiform_model_kernels::return_clock::SideSourceActivityDecomposition;

    use super::{
        ReturnBackfitAggregationHorizon, aggregate_quantile_calibration_sample,
        select_quantile_shape_aggregation_sample,
    };
    use crate::stages::return_backfit::quantile_shape_backfit::DynamicContinuousReturnObservation;
    use crate::stages::return_components::ReturnComponentsObservedOpenSessionReturn;

    #[test]
    fn selects_largest_intraday_horizon_with_required_quantile_sample_count() {
        let observations = (0..400)
            .map(|observation_number| {
                dynamic_observation(
                    observation_number as i64 * NANOS_PER_FIFTEEN_MINUTES,
                    0.001,
                    0.25,
                )
            })
            .collect::<Vec<_>>();

        let mut warnings = Vec::new();
        let sample =
            select_quantile_shape_aggregation_sample(observations.as_slice(), 360, &mut warnings)
                .expect("aggregation horizon should be selected");

        assert_eq!(
            sample.horizon,
            ReturnBackfitAggregationHorizon::fixed_interval(NANOS_PER_FIFTEEN_MINUTES)
        );
        assert_eq!(sample.sample_count(), 400);
        assert!(warnings.is_empty());
    }

    #[test]
    fn selects_hourly_horizon_when_it_satisfies_quantile_sample_count() {
        let observations = (0..400)
            .map(|observation_number| {
                dynamic_observation(observation_number as i64 * NANOS_PER_HOUR, 0.001, 0.25)
            })
            .collect::<Vec<_>>();

        let mut warnings = Vec::new();
        let sample =
            select_quantile_shape_aggregation_sample(observations.as_slice(), 360, &mut warnings)
                .expect("aggregation horizon should be selected");

        assert_eq!(
            sample.horizon,
            ReturnBackfitAggregationHorizon::fixed_interval(NANOS_PER_HOUR)
        );
        assert_eq!(sample.sample_count(), 400);
        assert!(warnings.is_empty());
    }

    #[test]
    fn falls_back_to_tick_horizon_when_minute_horizons_are_too_sparse() {
        let observations = (0..400)
            .map(|observation_number| {
                dynamic_observation(
                    observation_number as i64 * NANOS_PER_MINUTE / 2,
                    0.001,
                    0.25,
                )
            })
            .collect::<Vec<_>>();

        let mut warnings = Vec::new();
        let sample =
            select_quantile_shape_aggregation_sample(observations.as_slice(), 360, &mut warnings)
                .expect("aggregation horizon should be selected");

        assert_eq!(sample.horizon, ReturnBackfitAggregationHorizon::tick());
        assert_eq!(sample.sample_count(), 400);
        assert!(warnings.is_empty());
    }

    #[test]
    fn fixed_interval_aggregation_accumulates_business_time_and_sell_share() {
        let observations = vec![
            dynamic_observation(0, 0.001, 0.0),
            dynamic_observation(NANOS_PER_MINUTE, -0.002, 1.0),
        ];

        let sample = aggregate_quantile_calibration_sample(
            observations.as_slice(),
            ReturnBackfitAggregationHorizon::fixed_interval(NANOS_PER_HOUR),
        )
        .expect("aggregation should succeed");

        assert_eq!(sample.observed_returns, vec![-0.001]);
        assert_eq!(sample.business_time_scales, vec![2.0]);
        assert_eq!(sample.sell_share_proxies, vec![0.5]);
    }

    fn dynamic_observation(
        return_timestamp_utc_nanos: i64,
        log_return: f64,
        sell_share_proxy: f64,
    ) -> DynamicContinuousReturnObservation {
        DynamicContinuousReturnObservation {
            observation: ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos,
                elapsed_same_session_hours: 1.0,
                log_return,
                post_shock_activity_lift: 0.0,
                sell_share_proxy,
                source_activity: SideSourceActivityDecomposition::zero(),
            },
            original_business_time_scale: 1.0,
            dynamic_business_time_scale: 1.0,
        }
    }
}
