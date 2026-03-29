use super::known_news_samples::{activity_lift, is_within_known_news_exclusion_window};
use super::shared::{
    CalibrationReturnComponentsDetectedIntradayJumpCandidate, median_abs, median_abs_non_zero,
};

pub(crate) fn derive_intraday_jump_candidates(
    same_session_returns: &[(i64, f64)],
    open_price_tick_timestamps: &[i64],
    known_news_timestamps: &[i64],
    threshold_multiplier: f64,
) -> Vec<CalibrationReturnComponentsDetectedIntradayJumpCandidate> {
    let candidate_returns: Vec<(i64, f64)> = same_session_returns
        .iter()
        .copied()
        .filter(|(candidate_timestamp_utc_nanos, _)| {
            !is_within_known_news_exclusion_window(
                *candidate_timestamp_utc_nanos,
                known_news_timestamps,
            )
        })
        .collect();
    if candidate_returns.is_empty() {
        return Vec::new();
    }

    let candidate_jump_returns: Vec<f64> = candidate_returns
        .iter()
        .map(|(_, jump_return)| *jump_return)
        .collect();
    let threshold = median_abs_non_zero(candidate_jump_returns.as_slice())
        .unwrap_or_else(|| median_abs(candidate_jump_returns.as_slice()))
        * threshold_multiplier;
    let material_candidates: Vec<(i64, f64)> = candidate_returns
        .iter()
        .copied()
        .filter(|(_, jump_return)| f64::abs(*jump_return) > threshold)
        .collect();

    material_candidates
        .into_iter()
        .map(|(candidate_timestamp_utc_nanos, jump_return)| {
            CalibrationReturnComponentsDetectedIntradayJumpCandidate {
                jump_timestamp_utc_nanos: candidate_timestamp_utc_nanos,
                jump_return,
                post_shock_activity_lift: activity_lift(
                    candidate_timestamp_utc_nanos,
                    open_price_tick_timestamps,
                ),
            }
        })
        .collect()
}
