use crate::market_data::loading::MarketDataNormalizedCoarseDataRecord;
use crate::stages::joint_metrics::{
    JOINT_REFINEMENT_NEUTRAL_SELL_SHARE_PROXY, JointRefinementAggregatedIntervalObservation,
};

pub(crate) fn coarse_data_interval_observations(
    records: &[MarketDataNormalizedCoarseDataRecord],
) -> Vec<JointRefinementAggregatedIntervalObservation> {
    records
        .iter()
        .filter_map(|record| {
            record
                .close_to_close_log_return
                .filter(|log_return| log_return.is_finite())
                .map(|log_return| JointRefinementAggregatedIntervalObservation {
                    interval_start_utc_nanos: record.interval_start_utc_nanos,
                    interval_end_utc_nanos: record.interval_end_utc_nanos,
                    log_return,
                    tick_count: 0,
                    sell_share_proxy: JOINT_REFINEMENT_NEUTRAL_SELL_SHARE_PROXY,
                })
        })
        .collect()
}
