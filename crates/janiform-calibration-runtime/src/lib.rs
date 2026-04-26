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
mod temporal;

pub use adapters::{
    OptionSurfaceCalibrationObservation, OptionSurfaceCalibrationObservationError,
    build_option_surface_calibration_observation_from_pricing_hooks,
    evaluate_calibration_martingale_condition_under_esscher_tilt,
    evolve_hawkes_state_for_calibration_step, load_hourly_factor_contract_for_calibration,
    precheck_hawkes_kernel_parameter_domain, precheck_structured_hawkes_kernel_parameter_domain,
    project_hawkes_intensities_for_calibration_interval,
    transform_nig_exponent_for_calibration_under_esscher_tilt,
};
pub use checkpointing::CalibrationMemoryCheckpointStore;
pub use current_state::{
    CurrentStateEstimationError, CurrentStateEstimationInput, CurrentStateEstimationOutput,
    estimate_current_state_from_observed_data,
};
pub use execution_bridge::{
    CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION,
    CalibrationDayOfWeekSeasonalityEstimatorSettings,
    CalibrationDayOfYearSeasonalityEstimatorSettings, CalibrationHawkesDirectionalLabelDiagnostics,
    CalibrationHawkesObservedSourceRegime, CalibrationHawkesStageCandidateInput,
    CalibrationIntradaySeasonalityEstimatorSettings, CalibrationIntradaySmoothingKernel,
    CalibrationJointRefinementCarryInput, CalibrationJointRefinementCurrentStateEstimationInput,
    CalibrationJointRefinementOptionPricingInput, CalibrationJointRefinementOptionSnapshotNode,
    CalibrationMeanReversionStageExecutionInput, CalibrationMeanReversionStageObservation,
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
    CalibrationReturnComponentsStageCandidateGenerationInput, CalibrationScheduledEventOverlay,
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
    CalibrationMarketDataWorkflowScope, DEFAULT_MARKET_DATA_TIMESTAMP_TIMEZONE,
    MarketDataColumnAliasHint, MarketDataDuplicateRecordPolicy, MarketDataFitInputInferenceError,
    MarketDataFitInputInferenceOutput, MarketDataFitInputInferenceRequest,
    MarketDataFitMarketProfileDefaults, MarketDataFitNormalizationOverrides,
    MarketDataInputFileFormat, MarketDataInputFileReference, MarketDataKnownNewsColumnMapping,
    MarketDataKnownNewsLoadingInput, MarketDataLoadingError, MarketDataLoadingOutput,
    MarketDataLoadingRequest, MarketDataNormalizationPolicy, MarketDataNormalizedTickOutput,
    MarketDataOptionQuoteInputPolicy, MarketDataOptionSurfaceColumnMapping,
    MarketDataOptionSurfaceLoadingInput, MarketDataPresetInferenceError,
    MarketDataSessionConfiguration, MarketDataSessionRecurringClosedWindowPayload,
    MarketDataSessionRegularWindowPayload, MarketDataSessionResolutionPayload,
    MarketDataSourceProbePayload, MarketDataSourceSummary, MarketDataTickColumnCandidatesPayload,
    MarketDataTickDataColumnMapping, MarketDataTickPriceInputPolicy,
    MarketDataTranslationKnownNewsRecord, MarketDataTranslationPreprocessingError,
    MarketDataTranslationPreprocessingOutput, MarketDataTranslationPreprocessingRequest,
    MarketDataTranslationPreprocessingSettings,
    MarketDataTranslationReturnComponentsPreprocessingSettings,
    MarketDataTranslationSeasonalityPreprocessingSettings, MarketDataTranslationTickRecord,
    PreparedMarketDataCalibrationPlan, ResolvedMarketProfile,
    infer_bundled_market_profile_from_tick_data_path, infer_market_data_fit_inputs,
    infer_market_data_input_file_format_from_path, infer_market_data_option_quote_input_policy,
    infer_market_data_run_id_from_path, infer_market_data_tick_price_input_policy,
    load_and_preprocess_market_data_translation_request, normalize_market_data_source,
    prepare_market_data_calibration_plan, prepare_market_data_calibration_plan_from_phases,
    prepare_market_data_loading_from_phases, preprocess_market_data_translation_request,
    probe_market_data_source, resolve_market_data_option_surface_column_mapping_for_source,
    resolve_market_data_session_configuration,
    resolve_market_data_tick_data_column_mapping_for_source,
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
    RuntimeStructuralSeasonalityMaterializationError,
    RuntimeStructuralSeasonalityMaterializationInput,
    RuntimeStructuralSeasonalityMaterializationOutput, materialize_runtime_structural_seasonality,
};
pub use staged_runner::{
    CalibrationStageProgressPhase, CalibrationStagedRunFailure, CalibrationStagedRunInputs,
    CalibrationStagedRunObserverEvent, CalibrationStagedRunReport, CalibrationStagedRunStageReport,
    calibration_checkpoint_reference_for_stage, calibration_last_success_checkpoint_reference,
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
    HawkesStageSeasonalityInput, run_hawkes_calibration_stage,
};
pub use stages::joint_refinement::{
    CloseSessionRequest, CloseSessionResponse, InitializeSessionRequest, InitializeSessionResponse,
    JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION, JointRefinementSeedSummary,
    JointRefinementStageCandidateDiagnostic, JointRefinementStageCoarseTickConsistencyMetric,
    JointRefinementStageCurrentStateEstimationInput, JointRefinementStageExecutionFailure,
    JointRefinementStageExecutionInput, JointRefinementStageExecutionMetadata,
    JointRefinementStageExecutionReport, JointRefinementStageFittedPayload,
    JointRefinementStageHawkesInput, JointRefinementStageMeanReversionInput,
    JointRefinementStageObjectiveTermDiagnostic, JointRefinementStageOptionPricingInput,
    JointRefinementStageOptionSnapshotNode, JointRefinementStageProgressPhase,
    JointRefinementStageProgressUpdate, JointRefinementStageReturnComponentsInput,
    JointRefinementStageSeasonalityFactorsInput, JointRefinementStageSimulationSummary,
    JointRefinementSummarySimulationDiagnostics, JointRefinementSummarySimulationMemoryEstimate,
    JointRefinementSummarySimulationSeedSummary, JointRefinementSummarySimulationSessionContext,
    JointRefinementSummarySimulationSessionState, JointRefinementSummarySimulationWorkerRequest,
    JointRefinementSummarySimulationWorkerResponse, JointRefinementWorkerRequestedOutput,
    RunCandidateRequest, RunCandidateResponse, current_process_resident_bytes,
    initialize_joint_refinement_summary_simulation_session, read_joint_refinement_worker_frame,
    run_joint_refinement_calibration_stage, run_joint_refinement_candidate_in_session,
    simulate_joint_refinement_seed_summary_in_process, write_joint_refinement_worker_frame,
};
pub use stages::mean_reversion::{
    MeanReversionStageExecutionFailure, MeanReversionStageExecutionInput,
    MeanReversionStageExecutionMetadata, MeanReversionStageExecutionReport,
    MeanReversionStageFittedPayload, MeanReversionStageObservation,
    MeanReversionStageReturnComponentsInput, MeanReversionStageSeasonalityInput,
    run_mean_reversion_calibration_stage,
};
pub use stages::return_components::{
    ReturnComponentsActivityCouplingCandidateFamily, ReturnComponentsCloseOpenGapDetectionPolicy,
    ReturnComponentsDetectedCloseOpenGapCandidate, ReturnComponentsDetectedIntradayJumpCandidate,
    ReturnComponentsKnownNewsArrivalCandidateFamily, ReturnComponentsKnownNewsCalendarInput,
    ReturnComponentsObservedCloseOpenBoundaryObservation,
    ReturnComponentsObservedOpenSessionReturn, ReturnComponentsRealizedKnownNewsReturnSample,
    ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    ReturnComponentsStageActivityCalibrationContextSnapshot,
    ReturnComponentsStageActivityCouplingDiagnostic, ReturnComponentsStageActivityInput,
    ReturnComponentsStageCandidateDiagnostic, ReturnComponentsStageCandidateFamilies,
    ReturnComponentsStageCandidateFamilyCounts, ReturnComponentsStageCandidateGenerationInput,
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
    SeasonalityStageOverlayMetadata, run_seasonality_calibration_stage,
};
