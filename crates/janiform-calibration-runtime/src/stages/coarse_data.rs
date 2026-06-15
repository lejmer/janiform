use std::collections::BTreeMap;

use janiform_foundations::time::{NANOS_PER_HOUR, floor_to_interval_utc_nanos};

use crate::market_data::loading::MarketDataNormalizedCoarseDataRecord;
use crate::stages::joint_metrics::{
    JOINT_REFINEMENT_NEUTRAL_SELL_SHARE_PROXY, JointRefinementAggregatedIntervalObservation,
};

#[derive(Debug, Clone, Copy, PartialEq)]
struct CoarseDataHourlyReturnAccumulator {
    log_return: f64,
}

impl CoarseDataHourlyReturnAccumulator {
    fn accumulate(&mut self, log_return: f64) {
        self.log_return += log_return;
    }

    fn finish(self, interval_start_utc_nanos: i64) -> JointRefinementAggregatedIntervalObservation {
        JointRefinementAggregatedIntervalObservation {
            interval_start_utc_nanos,
            interval_end_utc_nanos: interval_start_utc_nanos.saturating_add(NANOS_PER_HOUR),
            log_return: self.log_return,
            tick_count: 0,
            sell_share_proxy: JOINT_REFINEMENT_NEUTRAL_SELL_SHARE_PROXY,
        }
    }
}

pub(crate) fn coarse_data_hourly_return_interval_observations(
    records: &[MarketDataNormalizedCoarseDataRecord],
) -> Vec<JointRefinementAggregatedIntervalObservation> {
    let mut buckets = BTreeMap::<i64, CoarseDataHourlyReturnAccumulator>::new();

    for record in records {
        let Some(log_return) = record
            .close_to_close_log_return
            .filter(|log_return| log_return.is_finite())
        else {
            continue;
        };

        let Some(interval_duration_nanos) = record
            .interval_end_utc_nanos
            .checked_sub(record.interval_start_utc_nanos)
        else {
            continue;
        };

        if interval_duration_nanos <= 0 || interval_duration_nanos > NANOS_PER_HOUR {
            continue;
        }

        let interval_start_utc_nanos =
            floor_to_interval_utc_nanos(record.interval_start_utc_nanos, NANOS_PER_HOUR);

        buckets
            .entry(interval_start_utc_nanos)
            .and_modify(|accumulator| accumulator.accumulate(log_return))
            .or_insert(CoarseDataHourlyReturnAccumulator { log_return });
    }

    buckets
        .into_iter()
        .map(|(interval_start_utc_nanos, accumulator)| accumulator.finish(interval_start_utc_nanos))
        .collect()
}
