//! Risk-sampling adapters and public contracts for sufficient-statistics paths.

mod calendar {
    #[allow(unused_imports)]
    pub use janiform_market_time::calendar::{
        CalendarDate, CalendarRuleMetadata, CalendarRuleMetadataArtifact,
        DAY_OF_WEEK_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
        DAY_OF_YEAR_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
        DETERMINISTIC_SEASONALITY_COMPOSITION_SCHEMA_VERSION, DayOfWeekSeasonalityDiagnostics,
        DayOfWeekSeasonalityEstimateOutput, DayOfWeekSeasonalityEstimationError,
        DayOfWeekSeasonalityEstimatorSettings, DayOfYearSeasonalityDiagnostics,
        DayOfYearSeasonalityEstimateOutput, DayOfYearSeasonalityEstimationError,
        DayOfYearSeasonalityEstimatorSettings, DeterministicSeasonalityCompositionDiagnostics,
        DeterministicSeasonalityCompositionError, DeterministicSeasonalityCompositionOutput,
        EarlyCloseSessionArtifact, ExchangeCalendarArtifact, ExchangeCalendarDefinition,
        ExchangeCalendarRegistry, ExchangeCalendarRegistryArtifact,
        ExchangeCalendarRegistryLoadError, ExchangeCalendarRegistryLoader,
        ExchangeCalendarRegistryLookupError, ExchangeCalendarResolveError,
        ExchangeCalendarValidationError, ExchangeMetadata,
        INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION, IntradaySeasonalityDiagnostics,
        IntradaySeasonalityEstimateOutput, IntradaySeasonalityEstimationError,
        IntradaySeasonalityEstimatorSettings, IntradaySmoothingKernel,
        MARKET_STATE_RESOLUTION_POLICY, MarketState, PREPROCESSED_TIMESTAMP_EVENT_SCHEMA_VERSION,
        RegularSessionArtifact, RegularSessionDefinition,
        SEASONALITY_DIAGNOSTICS_BATCH_REPORT_SCHEMA_VERSION,
        SEASONALITY_DIAGNOSTICS_REPORT_SCHEMA_VERSION, SESSION_WINDOW_BOUNDARY_POLICY,
        ScheduledEventOverlayDefinition, ScheduledEventOverlayProvenanceMetadata,
        SeasonalityDiagnosticsBatchReportOutput, SeasonalityDiagnosticsReportError,
        SeasonalityDiagnosticsReportInput, SeasonalityDiagnosticsReportOutput, SessionTime,
        SessionWindowArtifact, SessionWindowDefinition, TIMESTAMP_EVENT_INPUT_SCHEMA_VERSION,
        TimestampEventInputRecord, TimestampEventPreprocessingDiagnostics,
        TimestampEventPreprocessingError, TimestampEventPreprocessingOutput,
        TimestampEventSessionFilter, TradingWeekday, compose_deterministic_seasonality_signal,
        estimate_day_of_week_seasonality, estimate_day_of_year_seasonality,
        estimate_intraday_seasonality, generate_seasonality_diagnostics_batch_report,
        generate_seasonality_diagnostics_report,
    };
}

mod path;
mod request;
mod returns;
mod transform_sampling;

pub use path::{
    SUFFICIENT_STATISTICS_RISK_PATH_OUTPUT_SCHEMA_METADATA,
    SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE, SufficientStatisticsRiskPath,
    SufficientStatisticsRiskPathDiagnostics, SufficientStatisticsRiskPathError,
    SufficientStatisticsRiskPathOutputSchemaMetadata, SufficientStatisticsRiskPathProvenance,
    SufficientStatisticsRiskPathRecord, SufficientStatisticsRiskPathRecordError,
};
pub use request::{
    DEFAULT_INTRADAY_RISK_INTERVAL_DELTA, SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY,
    SUFFICIENT_STATISTICS_RISK_PATH_ARTIFACT_WORKFLOW_POLICY,
    SUFFICIENT_STATISTICS_RISK_PATH_DIAGNOSTICS_SCHEMA_VERSION,
    SUFFICIENT_STATISTICS_RISK_PATH_ENVIRONMENT_METADATA_POLICY,
    SUFFICIENT_STATISTICS_RISK_PATH_OUTPUT_SCHEMA_METADATA_VERSION,
    SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE_SCHEMA_VERSION,
    SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION,
    SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION,
    SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION, SufficientStatisticsRiskIntervalFactors,
    SufficientStatisticsRiskIntervalFactorsError, SufficientStatisticsRiskModelInputs,
    SufficientStatisticsRiskModelInputsError, SufficientStatisticsRiskRunRequest,
    SufficientStatisticsRiskRunRequestError,
};
pub use returns::{
    SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_BREAKDOWN_POLICY,
    SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY,
    SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY,
    SufficientStatisticsGeneratedIntervalReturn, SufficientStatisticsIntervalStatistics,
    SufficientStatisticsIntervalStatisticsError, SufficientStatisticsRiskReturnComponents,
    SufficientStatisticsRiskReturnComponentsError, SufficientStatisticsRiskReturnGenerationError,
    SufficientStatisticsRiskReturnGenerationOptions,
    generate_interval_return_from_sampled_statistics,
};
pub use transform_sampling::{
    SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY,
    SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY,
    SUFFICIENT_STATISTICS_STREAMING_CHUNKING_POLICY,
    SUFFICIENT_STATISTICS_TRANSFORM_SLICE_EXPECTATION_ESTIMATION_POLICY,
    SUFFICIENT_STATISTICS_TRANSFORM_SLICE_RANDOMNESS_SUBSTREAM_POLICY,
    SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SAMPLING_POLICY,
    SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SETTINGS_POLICY,
    SufficientStatisticsBoundaryStateUpdateError, SufficientStatisticsBoundaryStateUpdateSnapshot,
    SufficientStatisticsExpectedIntervalStatistics, SufficientStatisticsIntervalStatisticsChunk,
    SufficientStatisticsIntervalStatisticsPathChunk, SufficientStatisticsStreamingChunkingSettings,
    SufficientStatisticsStreamingChunkingSettingsError, SufficientStatisticsTransformSliceContext,
    SufficientStatisticsTransformSliceContextError,
    SufficientStatisticsTransformSliceExecutionError,
    SufficientStatisticsTransformSliceSamplingError, SufficientStatisticsTransformSliceSettings,
    SufficientStatisticsTransformSliceSettingsError, SufficientStatisticsTransformSliceSolveError,
    SufficientStatisticsTransformSlices, SufficientStatisticsTransformSlicesError,
    derive_expected_interval_statistics_from_transform_slices,
    derive_sufficient_statistics_batch_path_seed,
    sample_interval_statistics_batch_from_transform_slices,
    sample_interval_statistics_batch_from_transform_slices_with_randomness_factory,
    sample_interval_statistics_from_transform_slices,
    sample_interval_statistics_from_transform_slices_with_randomness_factory,
    sample_interval_statistics_multi_path_batch_from_transform_slices,
    sample_interval_statistics_multi_path_batch_from_transform_slices_with_randomness_factory,
    solve_exponential_affine_transform_for_risk_sampling,
    solve_exponential_affine_transform_for_risk_sampling_with_settings,
    solve_sufficient_statistics_transform_slices,
    solve_sufficient_statistics_transform_slices_with_solver_settings,
    stream_interval_statistics_from_transform_slices,
    stream_interval_statistics_from_transform_slices_with_randomness_factory,
    stream_interval_statistics_multi_path_batch_from_transform_slices,
    stream_interval_statistics_multi_path_batch_from_transform_slices_with_randomness_factory,
    sufficient_statistics_batch_seed_partition_policy,
    update_boundary_state_from_interval_statistics, validate_excitation_state,
};

#[cfg(test)]
mod tests;
