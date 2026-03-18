use super::contracts::ReturnComponentsStageObservedSignalSummaryStatistics;

pub(crate) fn test_summary_statistics() -> ReturnComponentsStageObservedSignalSummaryStatistics {
    ReturnComponentsStageObservedSignalSummaryStatistics {
        known_calendar_event_count: 0,
        known_realized_sample_count: 0,
        source_window_duration_hours: 1.0,
        observed_open_session_return_count: 0,
        continuous_fit_return_count: 0,
        intraday_jump_candidate_count: 0,
        intraday_jump_episode_count: 0,
        selected_surprise_jump_episode_count: 0,
        close_open_gap_candidate_count: 0,
        close_open_gap_episode_count: 0,
        open_session_sell_share_proxy: 0.5,
        mean_continuous_business_time_scale: 1.0,
        continuous_return_mean: 0.0,
        continuous_return_volatility: 0.0,
        continuous_return_skewness: 0.0,
        continuous_return_excess_kurtosis: 0.0,
        observed_log_price_span: 0.0,
        mean_abs_known_open_jump_return: 0.0,
        mean_abs_known_closed_gap_return: 0.0,
        mean_abs_surprise_open_jump_return: 0.0,
        mean_abs_surprise_closed_gap_return: 0.0,
        mean_known_post_shock_activity_lift: 0.0,
        mean_surprise_post_shock_activity_lift: 0.0,
        known_activity_coupling_target: 0.0,
        surprise_activity_coupling_target: 0.0,
        known_news_coverage_ratio: 0.0,
        surprise_arrival_intensity_per_hour: 0.0,
    }
}
