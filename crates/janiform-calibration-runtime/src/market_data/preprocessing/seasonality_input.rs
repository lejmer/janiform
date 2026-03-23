use super::shared::{
    CalibrationDayOfWeekSeasonalityEstimatorSettings,
    CalibrationDayOfYearSeasonalityEstimatorSettings,
    CalibrationIntradaySeasonalityEstimatorSettings, CalibrationIntradaySmoothingKernel,
    CalibrationScheduledEventOverlay, CalibrationSeasonalityStageExecutionInput,
    CalibrationSeasonalityStageExecutionSettings, CalibrationTimestampEventRecord,
    CalibrationTimestampEventSessionFilter, ResolvedKnownNewsRecord, normalized_pseudocount,
};

pub(super) fn build_seasonality_stage_input(
    timestamp_event_records: Vec<CalibrationTimestampEventRecord>,
    retained_known_news: &[ResolvedKnownNewsRecord],
    trade_events_data_reference: String,
) -> CalibrationSeasonalityStageExecutionInput {
    let scheduled_event_overlays = retained_known_news
        .iter()
        .enumerate()
        .map(
            |(position, known_news_record)| CalibrationScheduledEventOverlay {
                overlay_id: known_news_record
                    .event_identifier
                    .clone()
                    .unwrap_or_else(|| format!("known_news_{}", position + 1)),
                start_timestamp_utc_nanos: known_news_record.event_timestamp_utc_nanos,
                end_timestamp_utc_nanos: known_news_record.event_timestamp_utc_nanos,
                log_adjustment: 0.0,
                precedence: 0,
                enabled: true,
                source_label: "known_news_calendar".to_string(),
            },
        )
        .collect();

    let timestamp_event_count = timestamp_event_records.len();

    CalibrationSeasonalityStageExecutionInput {
        timestamp_event_records,
        scheduled_event_overlays,
        settings: Some(CalibrationSeasonalityStageExecutionSettings {
            session_filter: CalibrationTimestampEventSessionFilter::OpenOnly,
            sparse_hour_event_threshold: 1,
            intraday_settings: CalibrationIntradaySeasonalityEstimatorSettings {
                minutes_per_bin: 1,
                smoothing_kernel: CalibrationIntradaySmoothingKernel::Gaussian,
                smoothing_bandwidth_bins: 3.0,
                minimum_bin_events: 1,
                pseudocount: normalized_pseudocount(timestamp_event_count),
            },
            day_of_week_settings: CalibrationDayOfWeekSeasonalityEstimatorSettings {
                minimum_bucket_events: 1,
                pseudocount: normalized_pseudocount(timestamp_event_count),
            },
            day_of_year_settings: CalibrationDayOfYearSeasonalityEstimatorSettings {
                minimum_bucket_events: 1,
                pseudocount: normalized_pseudocount(timestamp_event_count),
            },
        }),
        dataset_id_override: Some(trade_events_data_reference),
        estimator_version_override: None,
    }
}
