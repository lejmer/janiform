//! Calibration runtime orchestration and staged execution.

#![forbid(unsafe_code)]

mod adapters;
mod checkpointing;
mod current_state;
mod diagnostics;
mod execution_bridge;
mod market_data;
mod objective;
mod optimizer_adapter;
mod runtime_seasonality;
mod shock_mark_fitting;
mod staged_runner;
mod stages;
mod surprise_profile;
mod temporal;

pub use adapters::{
    OptionSurfaceCalibrationObservation, OptionSurfaceCalibrationObservationError,
    build_option_surface_calibration_observation_from_pricing_hooks,
    evaluate_calibration_martingale_condition_under_esscher_tilt,
    evolve_hawkes_state_for_calibration_step, load_interval_factor_grid_for_calibration,
    precheck_hawkes_kernel_parameter_domain, precheck_structured_hawkes_kernel_parameter_domain,
    project_hawkes_intensities_for_calibration_interval,
    transform_nig_exponent_for_calibration_under_esscher_tilt,
};
pub use checkpointing::CalibrationMemoryCheckpointStore;
pub use current_state::{
    CurrentStateEstimationBatchInput, CurrentStateEstimationBatchItem,
    CurrentStateEstimationBatchProgressEvent, CurrentStateEstimationError,
    CurrentStateEstimationInput, CurrentStateEstimationOutput,
    estimate_current_state_from_observed_data, estimate_current_states_from_observed_data,
    estimate_current_states_from_observed_data_with_observer,
};
pub use diagnostics::types::{CalibrationDiagnosticsRequest, CalibrationExtendedDiagnosticKind};
pub use execution_bridge::{
    CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION,
    CalibrationHawkesDirectionalLabelDiagnostics, CalibrationHawkesStageCandidateInput,
    CalibrationJointRefinementCarryInput, CalibrationJointRefinementCurrentStateEstimationInput,
    CalibrationJointRefinementOptionPricingInput, CalibrationJointRefinementOptionSnapshotNode,
    CalibrationMeanReversionStageExecutionInput, CalibrationMeanReversionStageGridCandidate,
    CalibrationMeanReversionStageObservation,
    CalibrationReturnComponentsActivityContextCandidateDiagnostic,
    CalibrationReturnComponentsActivityContextImpliedMetrics,
    CalibrationReturnComponentsActivityContextPayload,
    CalibrationReturnComponentsCloseOpenGapDetectionPolicy,
    CalibrationReturnComponentsDetectedIntradayJumpCandidate,
    CalibrationReturnComponentsKnownNewsCalendarInput,
    CalibrationReturnComponentsKnownNewsReturnSample,
    CalibrationReturnComponentsObjectiveMetricWeights,
    CalibrationReturnComponentsObservedCloseOpenBoundaryObservation,
    CalibrationReturnComponentsObservedOpenSessionReturn,
    CalibrationReturnComponentsSignalSourceWindow,
    CalibrationReturnComponentsStageCandidateGenerationInput,
    CalibrationSeasonalityStageExecutionInput, CalibrationSeasonalityStageExecutionSettings,
    CalibrationSharedNewsMarkLawConfiguration, CalibrationStagedExecutionBridgeError,
    CalibrationStagedExecutionInput, CalibrationStagedExecutionPlan,
    CalibrationStagedExecutionRuntimeResources, CalibrationTimestampEventRecord,
    CalibrationTimestampEventSessionFilter, evaluate_return_components_exact_candidate_from_bridge,
    run_staged_calibration_from_execution_input,
    run_staged_calibration_from_execution_input_with_observer,
};
pub use market_data::{
    CalibrationDirectionalLabelDiagnosticsPayload, CalibrationMarketDataCarryInput,
    CalibrationMarketDataColumnMapping, CalibrationMarketDataKnownNewsInput,
    CalibrationMarketDataMeanReversionConfiguration, CalibrationMarketDataOutputPreferences,
    CalibrationMarketDataPreparationDiagnostics, CalibrationMarketDataPreparationError,
    CalibrationMarketDataPreparationProvenance, CalibrationMarketDataPreparationRequest,
    CalibrationMarketDataWorkflowScope, CalibrationTickObjectiveDataUsagePolicy,
    DEFAULT_MARKET_DATA_TIMESTAMP_TIMEZONE,
    DEFAULT_OPTION_FORWARD_PARITY_WARNING_RELATIVE_THRESHOLD, MarketDataCoarseDataColumnMapping,
    MarketDataCoarseDataKind, MarketDataCoarseDataLoadingInput, MarketDataCoarseDataSummary,
    MarketDataCoarseDataTimestampSemantics, MarketDataColumnAliasHint,
    MarketDataDuplicateRecordPolicy, MarketDataFitInputInferenceError,
    MarketDataFitInputInferenceOutput, MarketDataFitInputInferenceRequest,
    MarketDataFitMarketProfileDefaults, MarketDataFitNormalizationOverrides,
    MarketDataInputFileFormat, MarketDataInputFileReference, MarketDataKnownNewsColumnMapping,
    MarketDataKnownNewsLoadingInput, MarketDataLoadingDatasetPosition, MarketDataLoadingError,
    MarketDataLoadingOutput, MarketDataLoadingProgressError, MarketDataLoadingProgressEvent,
    MarketDataLoadingProgressObserver, MarketDataLoadingProgressPhase, MarketDataLoadingRequest,
    MarketDataLoadingSource, MarketDataNormalizationPolicy, MarketDataNormalizedCoarseDataRecord,
    MarketDataNormalizedOptionSurfaceOutput, MarketDataNormalizedOptionSurfaceRecord,
    MarketDataNormalizedTickOutput, MarketDataOptionQuoteInputPolicy,
    MarketDataOptionSurfaceColumnMapping, MarketDataOptionSurfaceLoadingInput,
    MarketDataPreparationProgressError, MarketDataPreparationProgressEvent,
    MarketDataPreparationProgressObserver, MarketDataPreparationProgressPhase,
    MarketDataPresetInferenceError, MarketDataSessionConfiguration,
    MarketDataSessionRecurringClosedWindowPayload, MarketDataSessionRegularWindowPayload,
    MarketDataSessionResolutionPayload, MarketDataSourceProbePayload, MarketDataSourceSummary,
    MarketDataTickColumnCandidatesPayload, MarketDataTickDataColumnMapping,
    MarketDataTickPriceInputPolicy, MarketDataTranslationKnownNewsRecord,
    MarketDataTranslationPreprocessingError, MarketDataTranslationPreprocessingOutput,
    MarketDataTranslationPreprocessingRequest, MarketDataTranslationPreprocessingSettings,
    MarketDataTranslationReturnComponentsPreprocessingSettings,
    MarketDataTranslationSeasonalityPreprocessingSettings, MarketDataTranslationTickRecord,
    OptionForwardParityCheck, OptionForwardParityWarningPolicy, PreparedMarketDataCalibrationPlan,
    PresetWorkflowResolutionError, PriceCoordinatePolicy, PriceCoordinateResolutionInput,
    PriceCoordinateResolutionOutput, ResolvedMarketProfile, ResolvedPriceCoordinate,
    ResolvedPriceCoordinateSet, ResolvedRollAdjustmentEvidence,
    build_market_data_session_definition_json, built_in_preset_registry_index,
    compute_preset_artifact_content_hash, deserialize_preset_artifact_json,
    exchange_code_from_market_profile_reference, exchange_code_from_session_reference,
    infer_bundled_market_profile_from_tick_data_path, infer_market_data_fit_inputs,
    infer_market_data_input_file_format_from_path, infer_market_data_option_quote_input_policy,
    infer_market_data_run_id_from_path, infer_market_data_tick_price_input_policy,
    load_and_preprocess_market_data_translation_request,
    load_and_preprocess_market_data_translation_request_with_progress,
    normalize_market_data_source, normalize_market_data_source_with_progress,
    normalize_option_surface_source, normalize_option_surface_source_with_progress,
    normalize_preset_artifact, prepare_market_data_calibration_plan,
    prepare_market_data_calibration_plan_from_phases,
    prepare_market_data_calibration_plan_with_observer,
    prepare_market_data_calibration_plan_with_progress, prepare_market_data_loading_from_phases,
    prepare_market_data_loading_from_phases_with_progress,
    preprocess_market_data_translation_request,
    preprocessing_settings_with_return_components_tick_size, preset_registry_index,
    probe_market_data_source, resolve_market_data_option_surface_column_mapping_for_source,
    resolve_market_data_session_configuration,
    resolve_market_data_tick_data_column_mapping_for_source, resolve_market_profile_configuration,
    resolve_preset_artifact, resolve_preset_snapshot, resolve_preset_workflow_configuration,
    resolve_price_coordinates, serialize_preset_artifact,
};
pub use objective::{
    CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_BOUNDS_POLICY,
    CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_PENALTY_POLICY,
    CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_SELECTION_POLICY,
    CALIBRATION_OBJECTIVE_COMPOSITION_SCHEMA_VERSION, CalibrationObjectiveCompositionError,
    CalibrationObjectiveCompositionReport, CalibrationObjectiveTermDiagnostic,
    CalibrationObjectiveTermInput, CalibrationObjectiveTermInputError,
    HawkesStageObjectiveTermInput, HawkesStageObjectiveTermInputError,
    compose_calibration_weighted_objective,
};
pub use optimizer_adapter::{
    CalibrationOptimizerAdapterCandidateDiagnostic, CalibrationOptimizerAdapterCandidateInput,
    CalibrationOptimizerAdapterCandidateInputError, CalibrationOptimizerAdapterConfiguration,
    CalibrationOptimizerAdapterConfigurationError, CalibrationOptimizerAdapterError,
    CalibrationOptimizerAdapterRunMetadata, CalibrationOptimizerAdapterRunOutput,
    CalibrationOptimizerBoundViolationDiagnostic, CalibrationOptimizerBoundViolationKind,
    CalibrationOptimizerCandidateParameterValue, CalibrationOptimizerParameterBound,
    CalibrationOptimizerParameterBoundError, run_constrained_optimizer_adapter,
};
pub use runtime_seasonality::{
    RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD,
    RuntimeStructuralSeasonalityMaterializationError,
    RuntimeStructuralSeasonalityMaterializationInput,
    RuntimeStructuralSeasonalityMaterializationOutput, materialize_runtime_structural_seasonality,
};
pub use staged_runner::{
    CalibrationStageProgressPhase, CalibrationStagedRunFailure, CalibrationStagedRunInputs,
    CalibrationStagedRunObserverEvent, CalibrationStagedRunReport,
    CalibrationStagedRunStageProgressEvent, CalibrationStagedRunStageReport,
    run_staged_calibration_with_checkpointing,
    run_staged_calibration_with_checkpointing_and_observer,
};
pub use stages::hawkes::{
    HawkesStageCandidateDiagnostic, HawkesStageCandidateInput, HawkesStageCandidateInputError,
    HawkesStageDirectionalLabelDiagnostics, HawkesStageDirectionalLabelDiagnosticsError,
    HawkesStageExecutionFailure, HawkesStageExecutionInput, HawkesStageExecutionMetadata,
    HawkesStageExecutionReport, HawkesStageFittedPayload, HawkesStageHandoffOutput,
    HawkesStageImpliedMetricsDiagnostic, HawkesStageObjectiveTermDiagnostic,
    HawkesStageObservedSourceRegime, HawkesStageObservedSourceRegimeError,
    HawkesStageProgressPhase, HawkesStageProgressUpdate, HawkesStageSeasonalityInput,
    run_hawkes_calibration_stage,
};
pub use stages::joint_refinement::{
    CloseSessionRequest, CloseSessionResponse, InitializeSessionRequest, InitializeSessionResponse,
    JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION,
    JointRefinementProgressCandidateAssessment, JointRefinementSeedSummary,
    JointRefinementStageCandidateDiagnostic, JointRefinementStageCoarseTickConsistencyMetric,
    JointRefinementStageCurrentStateEstimationInput, JointRefinementStageExecutionFailure,
    JointRefinementStageExecutionInput, JointRefinementStageExecutionMetadata,
    JointRefinementStageExecutionReport, JointRefinementStageFittedPayload,
    JointRefinementStageObjectiveTermDiagnostic, JointRefinementStageOptionPricingInput,
    JointRefinementStageOptionSnapshotNode, JointRefinementStageProgressPhase,
    JointRefinementStageProgressUpdate, JointRefinementStageSeasonalityFactorsInput,
    JointRefinementStageSimulationSummary, JointRefinementSummarySimulationDiagnostics,
    JointRefinementSummarySimulationMemoryEstimate, JointRefinementSummarySimulationSeedSummary,
    JointRefinementSummarySimulationSessionContext, JointRefinementSummarySimulationSessionState,
    JointRefinementSummarySimulationWorkerRequest, JointRefinementSummarySimulationWorkerResponse,
    JointRefinementWorkerRequestedOutput, RunCandidateRequest, RunCandidateResponse,
    current_process_resident_bytes, endogenous_interval_factor_grids_from_interval_factor_grid,
    initialize_joint_refinement_summary_simulation_session, read_joint_refinement_worker_frame,
    run_joint_refinement_calibration_stage, run_joint_refinement_candidate_in_session,
    simulate_joint_refinement_seed_summary_in_process, write_joint_refinement_worker_frame,
};
pub use stages::long_variance::{
    LongVarianceStageExecutionFailure, LongVarianceStageExecutionInput,
    LongVarianceStageExecutionMetadata, LongVarianceStageExecutionReport,
    LongVarianceStageFittedPayload, LongVarianceStageProgressPhase,
    LongVarianceStageProgressUpdate, run_long_variance_calibration_stage,
};
pub use stages::mean_reversion::{
    MeanReversionStageExecutionFailure, MeanReversionStageExecutionInput,
    MeanReversionStageExecutionMetadata, MeanReversionStageExecutionReport,
    MeanReversionStageFittedPayload, MeanReversionStageGridCandidate,
    MeanReversionStageObservation, MeanReversionStageProgressPhase,
    MeanReversionStageProgressUpdate, MeanReversionStageSeasonalityInput,
    run_mean_reversion_calibration_stage,
};
pub use stages::news_shocks::{
    NewsShocksStageExecutionFailure, NewsShocksStageExecutionInput,
    NewsShocksStageExecutionMetadata, NewsShocksStageExecutionReport, NewsShocksStageFittedPayload,
    run_news_shocks_calibration_stage,
};
pub use stages::return_backfit::{
    ReturnBackfitStageExecutionFailure, ReturnBackfitStageExecutionInput,
    ReturnBackfitStageExecutionMetadata, ReturnBackfitStageExecutionReport,
    ReturnBackfitStageFittedPayload, run_return_backfit_calibration_stage,
};
pub use stages::return_components::{
    ReturnComponentsActivityCouplingCandidateFamily, ReturnComponentsCloseOpenGapDetectionPolicy,
    ReturnComponentsDetectedCloseOpenGapCandidate, ReturnComponentsDetectedIntradayJumpCandidate,
    ReturnComponentsKnownNewsArrivalCandidateFamily, ReturnComponentsKnownNewsCalendarInput,
    ReturnComponentsObservedCloseOpenBoundaryObservation,
    ReturnComponentsObservedOpenSessionReturn, ReturnComponentsRealizedKnownNewsReturnSample,
    ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    ReturnComponentsStageActivityCalibrationContextSnapshot,
    ReturnComponentsStageActivityCouplingDiagnostic, ReturnComponentsStageCandidateDiagnostic,
    ReturnComponentsStageCandidateFamilies, ReturnComponentsStageCandidateFamilyCounts,
    ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageCandidateGenerationInputError, ReturnComponentsStageCandidateInput,
    ReturnComponentsStageCandidateInputError, ReturnComponentsStageCandidateProvenance,
    ReturnComponentsStageCandidateSourceWindow, ReturnComponentsStageConstraintViolationDiagnostic,
    ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic,
    ReturnComponentsStageContinuousOptimizerBasinDiagnostic,
    ReturnComponentsStageContinuousReturnSampleDiagnostic,
    ReturnComponentsStageDeterministicSeedMetadata, ReturnComponentsStageDiagnosticPolicy,
    ReturnComponentsStageExactCandidateEvaluation,
    ReturnComponentsStageExactCandidateEvaluationError, ReturnComponentsStageExecutionFailure,
    ReturnComponentsStageExecutionInput, ReturnComponentsStageExecutionMetadata,
    ReturnComponentsStageExecutionReport, ReturnComponentsStageFittedPayload,
    ReturnComponentsStageGapDiagnostic, ReturnComponentsStageHandoffOutput,
    ReturnComponentsStageJumpDiagnostic, ReturnComponentsStageObjectiveMetricBuckets,
    ReturnComponentsStageObjectiveMetricWeights, ReturnComponentsStageObjectiveScoringPolicy,
    ReturnComponentsStageObjectiveTermDiagnostic, ReturnComponentsStageObjectiveTermInput,
    ReturnComponentsStageObservedSignalSummaryStatistics,
    ReturnComponentsStagePriceRangeSanityDiagnostic, ReturnComponentsStagePriceRangeSanityPolicy,
    ReturnComponentsStageProgressPhase, ReturnComponentsStageQuantileBinningPolicy,
    ReturnComponentsStageReturnDistributionDiagnostic, ReturnComponentsStageSignalSourceWindow,
    ReturnComponentsStageSurpriseArrivalDiagnostic,
    ReturnComponentsStageSurpriseEpisodeSelectionPolicy,
    ReturnComponentsSurpriseNewsArrivalCandidateFamily, evaluate_return_components_exact_candidate,
    run_return_components_calibration_stage,
};
pub use stages::seasonality::{
    CalibrationStageExecutionStatus, SeasonalityStageExecutionFailure,
    SeasonalityStageExecutionInput, SeasonalityStageExecutionMetadata,
    SeasonalityStageExecutionReport, SeasonalityStageExecutionSettings,
    SeasonalityStageHandoffOutput, SeasonalityStageNormalizedPayload,
    SeasonalityStageProgressPhase, SeasonalityStageProgressUpdate,
    run_seasonality_calibration_stage,
};
pub use surprise_profile::{
    SurpriseNewsWeeklyProfileError, average_surprise_news_weekly_profile_multiplier_over_interval,
    materialize_surprise_news_weekly_profile_multipliers_for_interval_grid,
    materialize_surprise_news_weekly_profile_multipliers_for_interval_range,
    validate_surprise_news_weekly_profile,
};
