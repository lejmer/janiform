use super::shared::{
    CalibrationDayOfWeekSeasonalityEstimatorSettings,
    CalibrationDayOfYearSeasonalityEstimatorSettings,
    CalibrationIntradaySeasonalityEstimatorSettings, CalibrationIntradaySmoothingKernel,
    CalibrationScheduledEventOverlay, CalibrationSeasonalityStageExecutionInput,
    CalibrationSeasonalityStageExecutionSettings, CalibrationTimestampEventRecord,
    CalibrationTimestampEventSessionFilter, MarketDataTranslationSeasonalityPreprocessingSettings,
    ResolvedKnownNewsRecord, normalized_pseudocount,
};

pub(super) fn build_seasonality_stage_input(
    timestamp_event_records: Vec<CalibrationTimestampEventRecord>,
    retained_known_news: &[ResolvedKnownNewsRecord],
    trade_events_data_reference: String,
    preprocessing_settings: Option<&MarketDataTranslationSeasonalityPreprocessingSettings>,
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
                log_adjustment: preprocessing_settings
                    .map(|settings| settings.known_news_overlay_log_adjustment)
                    .unwrap_or(0.0),
                precedence: 0,
                enabled: true,
                source_label: "known_news_calendar".to_string(),
            },
        )
        .collect();

    let timestamp_event_count = timestamp_event_records.len();
    let seasonality_settings = preprocessing_settings.cloned().unwrap_or(
        MarketDataTranslationSeasonalityPreprocessingSettings {
            session_filter: CalibrationTimestampEventSessionFilter::OpenOnly,
            sparse_hour_event_threshold: 1,
            known_news_overlay_log_adjustment: 0.0,
            intraday_minutes_per_bin: 1,
            intraday_smoothing_kernel: CalibrationIntradaySmoothingKernel::Gaussian,
            intraday_smoothing_bandwidth_bins: 3.0,
            intraday_minimum_bin_events: 1,
            day_of_week_minimum_bucket_events: 1,
            day_of_year_minimum_bucket_events: 1,
        },
    );

    CalibrationSeasonalityStageExecutionInput {
        timestamp_event_records,
        scheduled_event_overlays,
        settings: Some(CalibrationSeasonalityStageExecutionSettings {
            session_filter: seasonality_settings.session_filter,
            sparse_hour_event_threshold: seasonality_settings.sparse_hour_event_threshold,
            intraday_settings: CalibrationIntradaySeasonalityEstimatorSettings {
                minutes_per_bin: seasonality_settings.intraday_minutes_per_bin,
                smoothing_kernel: seasonality_settings.intraday_smoothing_kernel,
                smoothing_bandwidth_bins: seasonality_settings.intraday_smoothing_bandwidth_bins,
                minimum_bin_events: seasonality_settings.intraday_minimum_bin_events,
                pseudocount: normalized_pseudocount(timestamp_event_count),
            },
            day_of_week_settings: CalibrationDayOfWeekSeasonalityEstimatorSettings {
                minimum_bucket_events: seasonality_settings.day_of_week_minimum_bucket_events,
                pseudocount: normalized_pseudocount(timestamp_event_count),
            },
            day_of_year_settings: CalibrationDayOfYearSeasonalityEstimatorSettings {
                minimum_bucket_events: seasonality_settings.day_of_year_minimum_bucket_events,
                pseudocount: normalized_pseudocount(timestamp_event_count),
            },
        }),
        dataset_id_override: Some(trade_events_data_reference),
        estimator_version_override: None,
    }
}
