//! Sufficient-statistics sampling adapters and public contracts for sufficient-statistics paths.

mod path;
mod request;
mod returns;

pub use janiform_affine_oracle::sufficient_statistics::{
    DEFAULT_INTRADAY_SUFFICIENT_STATISTICS_INTERVAL_DELTA,
    SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY,
    SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY,
    SUFFICIENT_STATISTICS_INTERVAL_FACTOR_POLICY,
    SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_BREAKDOWN_POLICY,
    SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY,
    SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY,
    SUFFICIENT_STATISTICS_STREAMING_CHUNKING_POLICY,
    SUFFICIENT_STATISTICS_TRANSFORM_SLICE_EXPECTATION_ESTIMATION_POLICY,
    SUFFICIENT_STATISTICS_TRANSFORM_SLICE_RANDOMNESS_SUBSTREAM_POLICY,
    SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SAMPLING_POLICY,
    SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SETTINGS_POLICY, SufficientStatisticsBoundaryState,
    SufficientStatisticsBoundaryStateUpdateError, SufficientStatisticsBoundaryStateUpdateSnapshot,
    SufficientStatisticsExpectedIntervalStatistics, SufficientStatisticsIntervalFactors,
    SufficientStatisticsIntervalFactorsError, SufficientStatisticsIntervalStatistics,
    SufficientStatisticsIntervalStatisticsChunk, SufficientStatisticsIntervalStatisticsError,
    SufficientStatisticsIntervalStatisticsPathChunk, SufficientStatisticsModelInputs,
    SufficientStatisticsModelInputsError, SufficientStatisticsReturnComponents,
    SufficientStatisticsReturnComponentsError, SufficientStatisticsSampledEventCounts,
    SufficientStatisticsSampledIntervalTransition, SufficientStatisticsStreamingChunkingSettings,
    SufficientStatisticsStreamingChunkingSettingsError, SufficientStatisticsTransformSliceContext,
    SufficientStatisticsTransformSliceContextError,
    SufficientStatisticsTransformSliceExecutionError,
    SufficientStatisticsTransformSliceSamplingError, SufficientStatisticsTransformSliceSettings,
    SufficientStatisticsTransformSliceSettingsError, SufficientStatisticsTransformSliceSolveError,
    SufficientStatisticsTransformSlices, SufficientStatisticsTransformSlicesError,
    derive_expected_interval_statistics_from_transform_slices,
    derive_sufficient_statistics_batch_path_seed,
    sample_interval_endogenous_transition_from_sampled_counts_with_randomness_factory,
    sample_interval_event_counts_from_expected_statistics_with_randomness_factory,
    sample_interval_statistics_batch_from_transform_slices,
    sample_interval_statistics_batch_from_transform_slices_with_randomness_factory,
    sample_interval_statistics_from_expected_statistics_with_randomness_factory,
    sample_interval_statistics_from_transform_slices,
    sample_interval_statistics_from_transform_slices_with_randomness_factory,
    sample_interval_statistics_multi_path_batch_from_transform_slices,
    sample_interval_statistics_multi_path_batch_from_transform_slices_with_randomness_factory,
    solve_exponential_affine_transform_for_sufficient_statistics_sampling,
    solve_exponential_affine_transform_for_sufficient_statistics_sampling_with_settings,
    solve_sufficient_statistics_transform_slices,
    solve_sufficient_statistics_transform_slices_with_solver_settings,
    stream_interval_statistics_from_transform_slices,
    stream_interval_statistics_from_transform_slices_with_randomness_factory,
    stream_interval_statistics_multi_path_batch_from_transform_slices,
    stream_interval_statistics_multi_path_batch_from_transform_slices_with_randomness_factory,
    sufficient_statistics_batch_seed_partition_policy,
    update_boundary_state_from_interval_statistics, validate_excitation_state,
};
pub use path::contracts::{
    SUFFICIENT_STATISTICS_PATH_OUTPUT_SCHEMA_METADATA, SUFFICIENT_STATISTICS_PATH_PROVENANCE,
    SufficientStatisticsPath, SufficientStatisticsPathDiagnostics, SufficientStatisticsPathError,
    SufficientStatisticsPathOutputSchemaMetadata, SufficientStatisticsPathProvenance,
    SufficientStatisticsPathRecord, SufficientStatisticsPathRecordError,
};
pub use request::constants::{
    SUFFICIENT_STATISTICS_PATH_ARTIFACT_WORKFLOW_POLICY,
    SUFFICIENT_STATISTICS_PATH_DIAGNOSTICS_SCHEMA_VERSION,
    SUFFICIENT_STATISTICS_PATH_ENVIRONMENT_METADATA_POLICY,
    SUFFICIENT_STATISTICS_PATH_OUTPUT_SCHEMA_METADATA_VERSION,
    SUFFICIENT_STATISTICS_PATH_PROVENANCE_SCHEMA_VERSION,
    SUFFICIENT_STATISTICS_PATH_RECORD_SCHEMA_VERSION, SUFFICIENT_STATISTICS_PATH_SCHEMA_VERSION,
    SUFFICIENT_STATISTICS_RUN_REQUEST_SCHEMA_VERSION,
};
pub use request::contracts::SufficientStatisticsRunRequest;
pub use request::errors::SufficientStatisticsRunRequestError;
pub use returns::{
    SufficientStatisticsGeneratedIntervalReturn, SufficientStatisticsReturnGenerationError,
    SufficientStatisticsReturnGenerationOptions, generate_interval_return_from_sampled_statistics,
};

#[cfg(test)]
mod tests;
