use super::shared::{
    CalibrationSeasonalityStageExecutionInput, CalibrationSeasonalityStageExecutionSettings,
    CalibrationTimestampEventRecord, CalibrationTimestampEventSessionFilter,
    MarketDataTranslationSeasonalityPreprocessingSettings, ResolvedKnownNewsRecord,
};

pub(super) fn build_seasonality_stage_input(
    timestamp_event_records: Vec<CalibrationTimestampEventRecord>,
    _retained_known_news: &[ResolvedKnownNewsRecord],
    trade_events_data_reference: String,
    preprocessing_settings: Option<&MarketDataTranslationSeasonalityPreprocessingSettings>,
) -> CalibrationSeasonalityStageExecutionInput {
    let seasonality_settings = preprocessing_settings.cloned().unwrap_or(
        MarketDataTranslationSeasonalityPreprocessingSettings {
            session_filter: CalibrationTimestampEventSessionFilter::OpenOnly,
            sparse_hour_event_threshold: 1,
            maximum_intraday_harmonic_order: None,
            maximum_weekly_harmonic_order: None,
            maximum_annual_harmonic_order: None,
        },
    );

    CalibrationSeasonalityStageExecutionInput {
        timestamp_event_records,
        settings: Some(CalibrationSeasonalityStageExecutionSettings {
            session_filter: seasonality_settings.session_filter,
            sparse_hour_event_threshold: seasonality_settings.sparse_hour_event_threshold,
            maximum_intraday_harmonic_order: seasonality_settings.maximum_intraday_harmonic_order,
            maximum_weekly_harmonic_order: seasonality_settings.maximum_weekly_harmonic_order,
            maximum_annual_harmonic_order: seasonality_settings.maximum_annual_harmonic_order,
        }),
        dataset_id_override: Some(trade_events_data_reference),
        estimator_version_override: None,
    }
}
