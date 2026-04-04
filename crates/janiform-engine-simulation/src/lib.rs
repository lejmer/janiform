//! Simulation-side hourly factor loading through the shared provider interface.

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

mod hourly_factors {
    #[allow(unused_imports)]
    pub use janiform_market_time::hourly_factors::{
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD, DEFAULT_KNOWN_NEWS_CALENDAR_ANCHOR_POLICY,
        DEFAULT_NEWS_INTENSITY_COMPONENT_AGGREGATION_POLICY,
        DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD, DEFAULT_SEASONALITY_AGGREGATION_METHOD,
        DEFAULT_SEASONALITY_INTERPOLATION_METHOD, DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY,
        DEFAULT_SPARSE_HOUR_MINIMUM_OBSERVATION_COUNT,
        DEFAULT_SURPRISE_NEWS_INTENSITY_CONTROL_POLICY, HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
        HOURLY_FACTOR_FALL_BACK_POLICY, HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
        HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
        HOURLY_FACTOR_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD,
        HOURLY_FACTOR_PROVIDER_SEASONALITY_MATERIALIZATION_METHOD,
        HOURLY_FACTOR_SPRING_FORWARD_POLICY, HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
        HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
        HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION, HourlyFactorContractDiagnostics,
        HourlyFactorContractError, HourlyFactorContractOutput, HourlyFactorMaterializationMetadata,
        HourlyFactorProviderError, HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource,
        HourlyNewsIntensityMaterializationDiagnostics, HourlyNewsIntensityMaterializationMetadata,
        HourlyNewsIntensityMaterializationOutput, HourlySeasonalityFactorSource,
        HourlySeasonalityMaterializationDiagnostics, HourlySeasonalityMaterializationMetadata,
        HourlySeasonalityMaterializationOutput, build_hourly_factor_contract,
        materialize_hourly_news_intensity_factors, materialize_hourly_seasonality_factors,
        provide_hourly_factor_contract,
    };
}

mod risk {
    #[allow(unused_imports)]
    pub(crate) use janiform_affine_oracle::sufficient_statistics::{
        DEFAULT_INTRADAY_RISK_INTERVAL_DELTA, SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY,
        SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY,
        SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_BREAKDOWN_POLICY,
        SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY,
        SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY,
        SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY,
        SUFFICIENT_STATISTICS_STREAMING_CHUNKING_POLICY,
        SUFFICIENT_STATISTICS_TRANSFORM_SLICE_EXPECTATION_ESTIMATION_POLICY,
        SUFFICIENT_STATISTICS_TRANSFORM_SLICE_RANDOMNESS_SUBSTREAM_POLICY,
        SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SAMPLING_POLICY,
        SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SETTINGS_POLICY,
        SufficientStatisticsBoundaryStateUpdateError,
        SufficientStatisticsBoundaryStateUpdateSnapshot,
        SufficientStatisticsExpectedIntervalStatistics, SufficientStatisticsIntervalStatistics,
        SufficientStatisticsIntervalStatisticsChunk, SufficientStatisticsIntervalStatisticsError,
        SufficientStatisticsIntervalStatisticsPathChunk, SufficientStatisticsRiskIntervalFactors,
        SufficientStatisticsRiskIntervalFactorsError, SufficientStatisticsRiskModelInputs,
        SufficientStatisticsRiskModelInputsError, SufficientStatisticsRiskReturnComponents,
        SufficientStatisticsRiskReturnComponentsError,
        SufficientStatisticsStreamingChunkingSettings,
        SufficientStatisticsStreamingChunkingSettingsError,
        SufficientStatisticsTransformSliceContext, SufficientStatisticsTransformSliceContextError,
        SufficientStatisticsTransformSliceExecutionError,
        SufficientStatisticsTransformSliceSamplingError,
        SufficientStatisticsTransformSliceSettings,
        SufficientStatisticsTransformSliceSettingsError,
        SufficientStatisticsTransformSliceSolveError, SufficientStatisticsTransformSlices,
        SufficientStatisticsTransformSlicesError,
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
}

mod api;
mod artifacts;
mod coarse;
mod coarse_runtime;
mod config;
mod constants;
mod error;
mod progress;
mod runtime;

pub use api::{
    evolve_hawkes_state, load_hourly_factor_contract,
    project_hawkes_intensities_simulation_interval, sample_nig_trading_time_increment,
    sample_nig_trading_time_increment_with_edge_mode,
};
pub use artifacts::{
    SeededFinePathMetadata, SeededFinePathOutput, SeededFinePathRecord,
    SeededFinePathTransitionKind, SeededTickEventDebugTraceChunk,
    SeededTickEventDebugTraceMetadata, SeededTickEventDebugTraceOutput,
    SeededTickEventDebugTraceRecord, SeededTickEventDebugTraceTransitionKind,
    SeededTickEventRecord, SeededTickEventSide, SeededTickEventStateSnapshot,
    SeededTickEventTapeMetadata, SeededTickEventTapeOutput, SeededTickSimulationNativeDiagnostics,
    SeededTickSimulationNewsEventMarketState, SeededTickSimulationNewsRoutingTotals,
    SeededTickSimulationRunResult, SeededTickSimulationStreamCompletion,
    SeededTickSimulationTransitionConsumer, SeededTickSimulationTransitionEmission,
};
pub use coarse::{
    COARSE_SIMULATION_ARTIFACT_WORKFLOW_POLICY, COARSE_SIMULATION_COMPATIBLE_INTERVAL_GRID_POLICY,
    COARSE_SIMULATION_ENVIRONMENT_METADATA_POLICY,
    COARSE_SIMULATION_INTERVAL_FACTOR_ALIGNMENT_POLICY,
    COARSE_SIMULATION_INTERVAL_IDENTIFIER_POLICY, COARSE_SIMULATION_INTERVAL_TIMING_POLICY,
    COARSE_SIMULATION_PATH_DIAGNOSTICS_SCHEMA_VERSION,
    COARSE_SIMULATION_PATH_OUTPUT_SCHEMA_METADATA_VERSION,
    COARSE_SIMULATION_PATH_PROVENANCE_SCHEMA_VERSION, COARSE_SIMULATION_PATH_RECORD_SCHEMA_VERSION,
    COARSE_SIMULATION_PATH_SCHEMA_VERSION, COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION,
    CoarseSimulationModelInputs, CoarseSimulationModelInputsError, CoarseSimulationPath,
    CoarseSimulationPathDiagnostics, CoarseSimulationPathError,
    CoarseSimulationPathOutputSchemaMetadata, CoarseSimulationPathProvenance,
    CoarseSimulationPathRecord, CoarseSimulationPathRecordError, CoarseSimulationRunRequest,
    CoarseSimulationRunRequestError,
};
pub use coarse_runtime::{
    CoarseSimulationRunError, run_coarse_simulation, run_coarse_simulation_with_observer,
    run_coarse_simulation_with_randomness_factory,
    run_coarse_simulation_with_randomness_factory_and_observer,
};
pub use config::{
    SeededTickEventStateSnapshotCapture, SeededTickSimulationDebugTraceOptions,
    SeededTickSimulationModelInputs, SeededTickSimulationRunConfig, SeededTickSimulationRunOptions,
};
pub use constants::{
    SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNK_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNKING_POLICY,
    SEEDED_TICK_EVENT_DEBUG_TRACE_METADATA_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_DEBUG_TRACE_RECORD_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_DEBUG_TRACE_SAMPLING_POLICY, SEEDED_TICK_EVENT_DEBUG_TRACE_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_DETERMINISM_POLICY, SEEDED_TICK_EVENT_FACTOR_CONTROL_POLICY,
    SEEDED_TICK_EVENT_INTERVAL_BOUNDARY_POLICY, SEEDED_TICK_EVENT_INTRA_INTERVAL_SCHEDULING_POLICY,
    SEEDED_TICK_EVENT_NEWS_MARKET_STATE_ROUTING_POLICY, SEEDED_TICK_EVENT_ORDERING_POLICY,
    SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION, SEEDED_TICK_EVENT_RANDOMNESS_STREAM_POLICY,
    SEEDED_TICK_EVENT_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY,
    SEEDED_TICK_EVENT_RECORD_SCHEMA_VERSION, SEEDED_TICK_EVENT_RUN_CONFIG_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_SIDE_ENCODING_POLICY, SEEDED_TICK_EVENT_STATE_CARRYOVER_POLICY,
    SEEDED_TICK_EVENT_STATE_SNAPSHOT_CAPTURE_POINT_POLICY,
    SEEDED_TICK_EVENT_STATE_SNAPSHOT_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_TAPE_METADATA_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_TIMESTAMP_REPRESENTATION_POLICY,
};
pub use error::SeededTickSimulationRunError;
pub use janiform_model_kernels::mean_reversion::{
    MeanReversionAnchorModel, MeanReversionAnchorPathMaterializationError,
    MeanReversionBoundaryState, MeanReversionFittedParameters, MeanReversionRuntimeIntervalAnchors,
    evolve_mean_reversion_pressure_state,
    materialize_mean_reversion_runtime_interval_anchors_from_core_expected_increments,
};
pub use progress::{SimulationProgressEvent, SimulationProgressPhase};
pub use runtime::{
    estimate_seeded_result_capacities, run_seeded_tick_event_simulation,
    run_seeded_tick_event_simulation_with_observer,
    run_seeded_tick_event_simulation_with_randomness_factory,
    run_seeded_tick_event_simulation_with_randomness_factory_and_observer,
    stream_seeded_tick_event_simulation, stream_seeded_tick_event_simulation_with_observer,
    stream_seeded_tick_event_simulation_with_randomness_factory,
    stream_seeded_tick_event_simulation_with_randomness_factory_and_observer,
};

#[cfg(test)]
mod tests;
