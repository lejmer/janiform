use serde_json::Value;

use crate::{
    MarketDataLoadingOutput, MarketDataNormalizedCoarseDataRecord, MarketDataNormalizedTickOutput,
};

use super::CalibrationTickObjectiveDataUsagePolicy;

pub(super) fn calibration_evidence_routing_payload(
    loading_output: &MarketDataLoadingOutput,
    normalized_tick_output: &MarketDataNormalizedTickOutput,
    tick_objective_data_usage_policy: CalibrationTickObjectiveDataUsagePolicy,
) -> Value {
    serde_json::json!({
        "schema_version": "calibration.market_data.evidence_routing.v1",
        "tick": {
            "role": "required_microstructure_event_and_short_horizon_evidence",
            "retained_record_count": loading_output.tick_data_summary.retained_row_count,
            "objective_data_usage_policy": tick_objective_data_usage_policy,
        },
        "coarse_data": loading_output.coarse_data_summary.as_ref().map(|summary| {
            serde_json::json!({
                "role": "optional_slow_return_variance_mean_reversion_gap_tail_evidence",
                "inferred_kind": summary.inferred_kind,
                "retained_record_count": summary.source_summary.retained_row_count,
                "return_interval_count": summary.retained_return_interval_count,
                "overlap_interval_count": coarse_overlap_interval_count(
                    loading_output.coarse_data_records.as_deref(),
                    normalized_tick_output,
                ),
                "independent_interval_count": coarse_independent_interval_count(
                    loading_output.coarse_data_records.as_deref(),
                    normalized_tick_output,
                ),
            })
        }),
        "options": loading_output.option_surface_summary.as_ref().map(|summary| {
            serde_json::json!({
                "role": "optional_terminal_q_surface_evidence",
                "retained_record_count": summary.retained_row_count,
            })
        }),
    })
}

pub(super) fn coarse_overlap_interval_count(
    coarse_records: Option<&[MarketDataNormalizedCoarseDataRecord]>,
    normalized_tick_output: &MarketDataNormalizedTickOutput,
) -> usize {
    let Some(coarse_records) = coarse_records else {
        return 0;
    };

    let Some((tick_start, tick_end)) = tick_window(normalized_tick_output) else {
        return 0;
    };

    coarse_records
        .iter()
        .filter(|record| {
            record.close_to_close_log_return.is_some()
                && record.interval_end_utc_nanos > tick_start
                && record.interval_start_utc_nanos < tick_end
        })
        .count()
}

pub(super) fn coarse_independent_interval_count(
    coarse_records: Option<&[MarketDataNormalizedCoarseDataRecord]>,
    normalized_tick_output: &MarketDataNormalizedTickOutput,
) -> usize {
    let Some(coarse_records) = coarse_records else {
        return 0;
    };

    let return_interval_count = coarse_records
        .iter()
        .filter(|record| record.close_to_close_log_return.is_some())
        .count();

    return_interval_count.saturating_sub(coarse_overlap_interval_count(
        Some(coarse_records),
        normalized_tick_output,
    ))
}

fn tick_window(normalized_tick_output: &MarketDataNormalizedTickOutput) -> Option<(i64, i64)> {
    let first = normalized_tick_output.tick_records.first()?;
    let last = normalized_tick_output.tick_records.last()?;

    if last.timestamp_utc_nanos <= first.timestamp_utc_nanos {
        return None;
    }

    Some((first.timestamp_utc_nanos, last.timestamp_utc_nanos))
}
