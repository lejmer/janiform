#![allow(dead_code, unused_imports)]

pub(crate) use std::collections::BTreeSet;
pub(crate) use std::fs;
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::process::Command;
pub(crate) use std::sync::LazyLock;
pub(crate) use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) use chrono::{TimeZone, Utc};
pub(crate) use janiform_calibration_artifacts::{
    CALIBRATION_ARTIFACT_ATOMIC_COMMIT_POLICY, CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_FAMILY,
    CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_GENERATION, CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION,
    CALIBRATION_ARTIFACT_PARTIAL_WRITE_RECOVERY_POLICY,
    CALIBRATION_ARTIFACT_ROOT_CONFIGURATION_POLICY, CALIBRATION_ARTIFACT_STORAGE_LAYOUT_POLICY,
    CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION,
    CALIBRATION_DATASET_REFERENCE_CHECKSUM_SCOPE,
    CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_FAMILY,
    CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_GENERATION,
    CALIBRATION_DATASET_SNAPSHOT_ARTIFACT_SCHEMA_VERSION,
    CALIBRATION_DATASET_SNAPSHOT_CHECKSUM_ALGORITHM, CALIBRATION_DATASET_SNAPSHOT_CHECKSUM_SCOPE,
    CALIBRATION_DATASET_SNAPSHOT_FORWARD_EXTENSION_POLICY,
    CALIBRATION_DATASET_SNAPSHOT_OUTPUT_SCHEMA_METADATA_VERSION,
    CALIBRATION_DATASET_SNAPSHOT_PERSISTENCE_SCOPE_POLICY,
    CALIBRATION_DATASET_SNAPSHOT_STAGE_INPUT_NAMING_POLICY,
    CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY, CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY,
    CALIBRATION_EXPERIMENT_INDEX_ORDERING_POLICY, CALIBRATION_EXPERIMENT_INDEX_REBUILD_POLICY,
    CALIBRATION_EXPERIMENT_INDEX_SCHEMA_FAMILY, CALIBRATION_EXPERIMENT_INDEX_SCHEMA_GENERATION,
    CALIBRATION_EXPERIMENT_INDEX_SCHEMA_VERSION,
    CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_HAWKES_STAGE_HANDOFF_POLICY, CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_JOINT_REFINEMENT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_JOINT_REFINEMENT_STAGE_HANDOFF_POLICY,
    CALIBRATION_JOINT_REFINEMENT_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_MEAN_REVERSION_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_MEAN_REVERSION_STAGE_HANDOFF_POLICY,
    CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_RETURN_COMPONENTS_STAGE_HANDOFF_POLICY,
    CALIBRATION_RETURN_COMPONENTS_STAGE_OBJECTIVE_WEIGHTING_POLICY,
    CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION, CALIBRATION_RUN_DATA_INPUTS_SCHEMA_VERSION,
    CALIBRATION_RUN_SPEC_SCHEMA_VERSION, CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION,
    CALIBRATION_SEASONALITY_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_SEASONALITY_STAGE_HANDOFF_POLICY,
    CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_SOLVER_SETTINGS_CAPTURE_POLICY, CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY,
    CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION, CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION,
    CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_FAMILY,
    CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_GENERATION,
    CALIBRATION_STAGE_PARAMETER_ARTIFACT_SCHEMA_VERSION,
    CALIBRATION_STAGE_PARAMETER_OPTIMIZER_STATE_SCHEMA_VERSION,
    CALIBRATION_STAGE_PARAMETER_OUTPUT_SCHEMA_METADATA_VERSION,
    CALIBRATION_STAGE_SEED_DERIVATION_POLICY, CALIBRATION_STAGED_RUN_FAILURE_TRIAGE_POLICY,
    CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
    CALIBRATION_VALIDATION_METRIC_ARTIFACT_OUTPUT_SCHEMA_METADATA_VERSION,
    CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_FAMILY,
    CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_GENERATION,
    CALIBRATION_VALIDATION_METRIC_ARTIFACT_SCHEMA_VERSION, CalibrationArtifactIoError,
    CalibrationArtifactKind, CalibrationArtifactManifest, CalibrationArtifactManifestBuildError,
    CalibrationArtifactManifestError, CalibrationArtifactManifestIndexMetadata,
    CalibrationArtifactManifestMetricStatus, CalibrationArtifactManifestMetricStatusCount,
    CalibrationArtifactManifestModelVersion, CalibrationArtifactManifestValidationMetricRole,
    CalibrationArtifactManifestValidationMetricSummary, CalibrationArtifactPersistenceKind,
    CalibrationArtifactReference, CalibrationArtifactReloadComponent,
    CalibrationArtifactReloadError, CalibrationArtifactReloadRequest,
    CalibrationArtifactReloadRequestError, CalibrationArtifactReloadSelection,
    CalibrationArtifactRunPreparationMode, CalibrationArtifactRunWriter,
    CalibrationArtifactRunWriterError, CalibrationCallerSurface,
    CalibrationCheckpointReferenceParseError, CalibrationDatasetInputName,
    CalibrationDatasetInputSnapshot, CalibrationDatasetSnapshotArtifact,
    CalibrationDatasetSnapshotArtifactValidationError,
    CalibrationDatasetSnapshotOutputSchemaMetadata, CalibrationDatasetSnapshotProvenance,
    CalibrationDatasetSnapshotSchemaMetadata, CalibrationExecutionMode,
    CalibrationExperimentIndexDocument, CalibrationExperimentIndexError,
    CalibrationExperimentIndexLookupRequest, CalibrationExperimentIndexLookupResponse,
    CalibrationExperimentIndexRecord, CalibrationOptimizerSettings,
    CalibrationOptimizerSettingsError, CalibrationReloadedArtifactRun,
    CalibrationResolvedStageSeed, CalibrationResumeSettings, CalibrationResumeSettingsError,
    CalibrationResumeSource, CalibrationRunContract, CalibrationRunContractError,
    CalibrationRunDataInputs, CalibrationRunDataInputsError, CalibrationRunSeedSettings,
    CalibrationRunSeedSettingsError, CalibrationRunSpec, CalibrationRunSpecBuilder,
    CalibrationRunSpecError, CalibrationRunSpecErrorClassification, CalibrationStage,
    CalibrationStageCheckpointCompatibilityError, CalibrationStageCheckpointMetadata,
    CalibrationStageDependency, CalibrationStageGraph, CalibrationStageGraphError,
    CalibrationStageHandoff, CalibrationStageHandoffError, CalibrationStageHandoffRoute,
    CalibrationStageHandoffTransport, CalibrationStageParameterArtifact,
    CalibrationStageParameterArtifactBuildError, CalibrationStageParameterArtifactInput,
    CalibrationStageParameterArtifactValidationError, CalibrationStageParameterBuildMetadata,
    CalibrationStageParameterOptimizerAdapterMetadata, CalibrationStageParameterOptimizerState,
    CalibrationStageParameterOptimizerStateInput, CalibrationStageParameterOutputSchemaMetadata,
    CalibrationStageParameterPayload, CalibrationStageParameterSchemaMetadata,
    CalibrationStageParameterSeedMetadata, CalibrationStagePayloadSource,
    CalibrationStageSeedOverride, CalibrationStageSeedSource,
    CalibrationStageSelectedCandidateSource, CalibrationStageSolverSettings,
    CalibrationValidationMetricArtifact,
    CalibrationValidationMetricArtifactReferenceIntegrityError,
    CalibrationValidationMetricArtifactRole, CalibrationValidationMetricArtifactValidationError,
    CalibrationValidationMetricCollection, CalibrationValidationMetricConfidenceInterval,
    CalibrationValidationMetricContext, CalibrationValidationMetricContextKind,
    CalibrationValidationMetricManifestLink, CalibrationValidationMetricOutputSchemaMetadata,
    CalibrationValidationMetricRecord, CalibrationValidationMetricSchemaMetadata,
    CalibrationValidationMetricSourceKind, CalibrationValidationMetricStageOutputLink,
    CalibrationValidationMetricStatus, CalibrationValidationMetricThresholdMetadata,
    CalibrationValidationObservedValue, HawkesStageFittedParameters,
    HawkesStageParameterArtifactInput, HawkesStageParameterPayload,
    JointRefinementStageCandidateDiagnostics, JointRefinementStageCarryInput,
    JointRefinementStageFittedParameters, JointRefinementStageLabeledCount,
    JointRefinementStageObjectiveBreakdown, JointRefinementStageOptimizerDiagnostics,
    JointRefinementStageOptionAggregateDiagnostic,
    JointRefinementStageOptionCalibrationDiagnostics, JointRefinementStageOptionNodeDiagnostic,
    JointRefinementStageOptionSnapshotMetadata, JointRefinementStageParameterArtifactInput,
    JointRefinementStageParameterPayload, JointRefinementStagePricingBlockFittedParameters,
    JointRefinementStageRuntimeProfile, LongVarianceStageParameterArtifactInput,
    MeanReversionStageCalibrationGrid, MeanReversionStageCandidateDiagnostics,
    MeanReversionStageCandidateSeriesDiagnostics, MeanReversionStageDiagnostics,
    MeanReversionStageParameterArtifactInput, MeanReversionStageParameterPayload,
    MeanReversionStageSeriesSummary, NewsShocksStageParameterArtifactInput,
    ReturnBackfitStageParameterArtifactInput, ReturnComponentsStageFittedParameters,
    ReturnComponentsStageParameterArtifactInput, ReturnComponentsStageParameterPayload,
    SeasonalityStageParameterArtifactInput, SeasonalityStageParameterPayload,
    SurpriseNewsDeterministicIntensityProfile, build_calibration_artifact_manifest,
    build_calibration_dataset_snapshot_artifact, calibration_artifact_manifest_path,
    calibration_artifact_manifest_reference, calibration_checkpoint_run_directory,
    calibration_committed_artifact_run_directory,
    calibration_committed_artifact_runs_root_directory, calibration_dataset_snapshot_artifact_path,
    calibration_dataset_snapshot_artifact_reference, calibration_experiment_index_path,
    calibration_experiment_index_path_for_root, calibration_experiment_index_root_directory,
    calibration_last_success_checkpoint_reference, calibration_stage_checkpoint_reference,
    calibration_stage_parameter_artifact_path,
    calibration_stage_parameter_artifact_path_in_run_directory, calibration_stage_token,
    calibration_validation_metric_artifact_path, calibration_validation_metric_artifact_reference,
    default_calibration_execution_mode_for_surface, derive_calibration_stage_seed_from_global_seed,
    load_calibration_artifact_manifest_from_disk,
    load_calibration_dataset_snapshot_artifact_from_disk,
    load_calibration_experiment_index_from_disk,
    load_calibration_stage_parameter_artifact_from_disk,
    load_calibration_validation_metric_artifact_from_disk,
    lookup_calibration_experiment_index_records, persist_calibration_artifact_manifest_to_disk,
    persist_calibration_artifact_manifest_to_run_directory,
    persist_calibration_dataset_snapshot_artifact_to_disk,
    persist_calibration_dataset_snapshot_artifact_to_run_directory,
    persist_calibration_dataset_snapshot_from_run_contract_to_disk,
    persist_calibration_experiment_index_to_disk,
    persist_calibration_stage_parameter_artifact_to_disk,
    persist_calibration_stage_parameter_artifact_to_run_directory,
    persist_calibration_validation_metric_artifact_to_disk,
    rebuild_calibration_experiment_index_from_disk, reload_calibration_artifact_run_from_disk,
    resolve_calibration_execution_mode_for_surface, resolve_calibration_stage_execution_order,
    resolve_calibration_stage_seed, resolve_calibration_stage_seed_metadata,
    stage_output_artifact_references_for_stage, stage_solver_settings_for_stage,
    update_calibration_experiment_index_for_run,
};
pub(crate) use janiform_calibration_runtime::{
    CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_BOUNDS_POLICY,
    CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_PENALTY_POLICY,
    CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_SELECTION_POLICY,
    CALIBRATION_OBJECTIVE_COMPOSITION_SCHEMA_VERSION,
    CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION,
    CalibrationDirectionalLabelDiagnosticsPayload, CalibrationHawkesDirectionalLabelDiagnostics,
    CalibrationHawkesStageCandidateInput, CalibrationJointRefinementCarryInput,
    CalibrationJointRefinementCurrentStateEstimationInput,
    CalibrationJointRefinementOptionPricingInput, CalibrationJointRefinementOptionSnapshotNode,
    CalibrationMeanReversionStageExecutionInput, CalibrationMeanReversionStageObservation,
    CalibrationMemoryCheckpointStore, CalibrationObjectiveCompositionError,
    CalibrationObjectiveCompositionReport, CalibrationObjectiveTermDiagnostic,
    CalibrationObjectiveTermInput, CalibrationObjectiveTermInputError,
    CalibrationOptimizerAdapterCandidateDiagnostic, CalibrationOptimizerAdapterCandidateInput,
    CalibrationOptimizerAdapterCandidateInputError, CalibrationOptimizerAdapterConfiguration,
    CalibrationOptimizerAdapterConfigurationError, CalibrationOptimizerAdapterError,
    CalibrationOptimizerAdapterRunMetadata, CalibrationOptimizerAdapterRunOutput,
    CalibrationOptimizerBoundViolationDiagnostic, CalibrationOptimizerBoundViolationKind,
    CalibrationOptimizerCandidateParameterValue, CalibrationOptimizerParameterBound,
    CalibrationOptimizerParameterBoundError,
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
    CalibrationSharedNewsMarkLawConfiguration, CalibrationStageExecutionStatus,
    CalibrationStagedExecutionBridgeError, CalibrationStagedExecutionInput,
    CalibrationStagedExecutionPlan, CalibrationStagedExecutionRuntimeResources,
    CalibrationStagedRunFailure, CalibrationStagedRunInputs, CalibrationStagedRunObserverEvent,
    CalibrationStagedRunReport, CalibrationStagedRunStageReport,
    CalibrationTickObjectiveDataUsagePolicy, CalibrationTimestampEventRecord,
    CalibrationTimestampEventSessionFilter, CloseSessionRequest, CloseSessionResponse,
    CurrentStateEstimationError, CurrentStateEstimationInput, CurrentStateEstimationOutput,
    HAWKES_EVENT_SIDE_CODE_BUY, HAWKES_EVENT_SIDE_CODE_SELL, HawkesStageCandidateDiagnostic,
    HawkesStageCandidateInput, HawkesStageCandidateInputError,
    HawkesStageDirectionalLabelDiagnostics, HawkesStageDirectionalLabelDiagnosticsError,
    HawkesStageExecutionFailure, HawkesStageExecutionInput, HawkesStageExecutionMetadata,
    HawkesStageExecutionReport, HawkesStageFittedPayload, HawkesStageHandoffOutput,
    HawkesStageImpliedMetricsDiagnostic, HawkesStageObjectiveTermDiagnostic,
    HawkesStageObjectiveTermInput, HawkesStageObjectiveTermInputError,
    HawkesStageObservedSourceRegime, HawkesStageObservedSourceRegimeError,
    HawkesStageSeasonalityInput, InitializeSessionRequest, InitializeSessionResponse,
    JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION, JointRefinementSeedSummary,
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
    JointRefinementWorkerRequestedOutput, MarketDataDuplicateRecordPolicy,
    MarketDataInputFileFormat, MarketDataInputFileReference, MarketDataKnownNewsColumnMapping,
    MarketDataKnownNewsLoadingInput, MarketDataLoadingError, MarketDataLoadingOutput,
    MarketDataLoadingRequest, MarketDataNormalizationPolicy, MarketDataNormalizedTickOutput,
    MarketDataOptionQuoteInputPolicy, MarketDataOptionSurfaceColumnMapping,
    MarketDataOptionSurfaceLoadingInput, MarketDataSessionConfiguration,
    MarketDataSessionRecurringClosedWindowPayload, MarketDataSessionRegularWindowPayload,
    MarketDataSessionResolutionPayload, MarketDataSourceProbePayload, MarketDataSourceSummary,
    MarketDataTickDataColumnMapping, MarketDataTickPriceInputPolicy,
    MarketDataTranslationCalibrationError, MarketDataTranslationCalibrationOutput,
    MarketDataTranslationCalibrationRequest, MarketDataTranslationCalibrationSettings,
    MarketDataTranslationKnownNewsRecord, MarketDataTranslationReturnComponentsCalibrationSettings,
    MarketDataTranslationSeasonalityCalibrationSettings, MarketDataTranslationTickRecord,
    MeanReversionStageExecutionFailure, MeanReversionStageExecutionInput,
    MeanReversionStageExecutionMetadata, MeanReversionStageExecutionReport,
    MeanReversionStageFittedPayload, MeanReversionStageGridCandidate,
    MeanReversionStageObservation, MeanReversionStageSeasonalityInput,
    OptionSurfaceCalibrationObservation, OptionSurfaceCalibrationObservationError,
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
    ReturnComponentsStageQuantileBinningPolicy, ReturnComponentsStageReturnDistributionDiagnostic,
    ReturnComponentsStageSignalSourceWindow, ReturnComponentsStageSurpriseArrivalDiagnostic,
    ReturnComponentsStageSurpriseEpisodeSelectionPolicy,
    ReturnComponentsSurpriseNewsArrivalCandidateFamily, RunCandidateRequest, RunCandidateResponse,
    SeasonalityStageExecutionFailure, SeasonalityStageExecutionInput,
    SeasonalityStageExecutionMetadata, SeasonalityStageExecutionReport,
    SeasonalityStageExecutionSettings, SeasonalityStageHandoffOutput,
    SeasonalityStageNormalizedPayload,
    build_option_surface_calibration_observation_from_pricing_hooks,
    calibrate_market_data_translation_request, compose_calibration_weighted_objective,
    current_process_resident_bytes, estimate_current_state_from_observed_data,
    evaluate_calibration_martingale_condition_under_esscher_tilt,
    evaluate_return_components_exact_candidate,
    evaluate_return_components_exact_candidate_from_bridge,
    evolve_hawkes_state_for_calibration_step,
    initialize_joint_refinement_summary_simulation_session,
    load_and_calibrate_market_data_translation_request, load_interval_factor_grid_for_calibration,
    materialize_runtime_structural_seasonality, normalize_market_data_source,
    precheck_hawkes_kernel_parameter_domain, precheck_structured_hawkes_kernel_parameter_domain,
    prepare_market_data_loading_from_phases, probe_market_data_source,
    project_hawkes_intensities_for_calibration_interval, read_joint_refinement_worker_frame,
    resolve_market_data_session_configuration, run_constrained_optimizer_adapter,
    run_hawkes_calibration_stage, run_joint_refinement_calibration_stage,
    run_joint_refinement_candidate_in_session, run_mean_reversion_calibration_stage,
    run_return_components_calibration_stage, run_seasonality_calibration_stage,
    run_staged_calibration_from_execution_input,
    run_staged_calibration_from_execution_input_with_observer,
    run_staged_calibration_with_checkpointing,
    run_staged_calibration_with_checkpointing_and_observer,
    simulate_joint_refinement_seed_summary_in_process,
    transform_nig_exponent_for_calibration_under_esscher_tilt, write_joint_refinement_worker_frame,
};
pub(crate) use janiform_engine_pricing::{
    PricingIntervalSchedulePolicy, VanillaOptionPayoffType, VanillaOptionPricingCalibrationHooks,
    VanillaOptionPricingMarketInputs,
    intraday_black_scholes_log_return_characteristic_function as black_scholes_log_return_characteristic_function,
};
pub(crate) use janiform_foundations::cancellation::CancellationToken;
pub(crate) use janiform_foundations::complex_math::ComplexScalar;
pub(crate) use janiform_foundations::time::{NANOS_PER_HOUR, utc_nanos};
pub(crate) use janiform_market_time::calendar::{
    ExchangeCalendarRegistry, ExchangeCalendarRegistryLoader, TimestampEventInputRecord,
};
pub(crate) use janiform_model_kernels::hawkes::{
    HAWKES_KERNEL_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION, HawkesEventSide,
    HawkesKernelLoadings, HawkesKernelParameters, HawkesKernelValidationStatus,
    HawkesStructuredAlphaParameterization, HawkesStructuredRhoParameterization,
    HawkesStructuredRhoSelfDominancePolicy,
};
pub(crate) use janiform_model_kernels::news::{
    GammaSeverityMarkConfiguration, NewsArrivalConfiguration, NewsComponentMarkScaleConfiguration,
    NewsProcessConfiguration, SharedNewsMarkLawConfiguration, SurpriseNewsArrivalConfiguration,
};
pub(crate) use janiform_model_kernels::nig::{NigParameters, NigSharedPlusShiftConfiguration};
pub(crate) use janiform_model_kernels::return_clock::SideSourceActivityDecomposition;
pub(crate) use janiform_model_kernels::risk_neutral::{
    EsscherMartingaleConditionInput, NigEsscherTransformationInput,
};
pub(crate) use serde_json::json;

const JOINT_REFINEMENT_SUMMARY_SIMULATION_BACKEND_ENV: &str =
    "JANIFORM_JOINT_REFINEMENT_SUMMARY_SIMULATION_BACKEND";
const JOINT_REFINEMENT_SUMMARY_SIMULATION_BACKEND_IN_PROCESS: &str = "in_process";
const JOINT_REFINEMENT_CANDIDATE_POLICY_ENV: &str = "JANIFORM_JOINT_REFINEMENT_CANDIDATE_POLICY";
const JOINT_REFINEMENT_CANDIDATE_POLICY_CPU_COUNT_LIMITED: &str = "cpu_count_limited";

pub(crate) fn normalize_joint_refinement_runtime_profile_for_reproducibility(
    runtime_profile: &mut JointRefinementStageRuntimeProfile,
) {
    runtime_profile.population_refinement_wall_clock_millis = 0;
    runtime_profile.coarse_consistency_wall_clock_millis = 0;
    runtime_profile.final_seed_evaluation_wall_clock_millis = 0;
    runtime_profile.final_assembly_wall_clock_millis = 0;
    runtime_profile.objective_cache_hit_count = 0;
    runtime_profile.objective_cache_miss_count = 0;
    runtime_profile.deterministic_cache_hit_count = 0;
    runtime_profile.deterministic_cache_miss_count = 0;
    runtime_profile.seed_summary_cache_hit_count = 0;
    runtime_profile.seed_summary_cache_miss_count = 0;
    runtime_profile.aggregated_likelihood_context_cache_hit_count = 0;
    runtime_profile.aggregated_likelihood_context_cache_miss_count = 0;
    runtime_profile.option_pricing_cache_hit_count = 0;
    runtime_profile.option_pricing_cache_miss_count = 0;
    runtime_profile.option_pricing_run_count = 0;
    runtime_profile.cumulative_deterministic_evaluation_millis = 0;
    runtime_profile.cumulative_modeled_rate_millis = 0;
    runtime_profile.cumulative_likelihood_context_resolution_millis = 0;
    runtime_profile.cumulative_likelihood_context_1h_sample_millis = 0;
    runtime_profile.cumulative_likelihood_context_1d_sample_millis = 0;
    runtime_profile.cumulative_likelihood_context_tick_sample_millis = 0;
    runtime_profile.cumulative_likelihood_context_tick_optimizer_millis = 0;
    runtime_profile.cumulative_likelihood_context_1h_optimizer_millis = 0;
    runtime_profile.cumulative_likelihood_context_1d_optimizer_millis = 0;
    runtime_profile.cumulative_tick_likelihood_millis = 0;
    runtime_profile.cumulative_1h_likelihood_millis = 0;
    runtime_profile.cumulative_1d_likelihood_millis = 0;
    runtime_profile.cumulative_expected_activity_counts_millis = 0;
    runtime_profile.cumulative_deterministic_activity_shape_millis = 0;
    runtime_profile.cumulative_deterministic_activity_level_millis = 0;
    runtime_profile.cumulative_seed_summary_simulation_millis = 0;
    runtime_profile.cumulative_raw_seed_metrics_millis = 0;
    runtime_profile.cumulative_loss_build_millis = 0;
    runtime_profile.cumulative_option_pricing_millis = 0;
}

pub(crate) fn assert_joint_refinement_objective_breakdown_matches(
    actual_breakdown: &JointRefinementStageObjectiveBreakdown,
    expected_breakdown: &JointRefinementStageObjectiveBreakdown,
    context: &str,
) {
    assert_f64_within_one_ulp(
        actual_breakdown.global_loss,
        expected_breakdown.global_loss,
        &format!("{context} global_loss"),
    );

    assert_eq!(
        actual_breakdown.family_losses.len(),
        expected_breakdown.family_losses.len(),
        "{context} family loss count"
    );

    for (family_index, (actual_family, expected_family)) in actual_breakdown
        .family_losses
        .iter()
        .zip(expected_breakdown.family_losses.iter())
        .enumerate()
    {
        assert_eq!(
            actual_family.family_id, expected_family.family_id,
            "{context} family id at {family_index}"
        );
        assert_eq!(
            actual_family.coordinate_count, expected_family.coordinate_count,
            "{context} family coordinate count at {family_index}"
        );
        assert_eq!(
            actual_family.rank, expected_family.rank,
            "{context} family rank at {family_index}"
        );
        assert_f64_within_one_ulp(
            actual_family.loss,
            expected_family.loss,
            &format!("{context} family loss at {family_index}"),
        );
        assert_f64_within_one_ulp(
            actual_family.max_standardized_residual,
            expected_family.max_standardized_residual,
            &format!("{context} family max z at {family_index}"),
        );
        assert_eq!(
            actual_family.worst_term_id, expected_family.worst_term_id,
            "{context} family worst term at {family_index}"
        );
        assert_eq!(
            actual_family.unidentified, expected_family.unidentified,
            "{context} family unidentified flag at {family_index}"
        );
    }

    assert_eq!(
        actual_breakdown.term_diagnostics, expected_breakdown.term_diagnostics,
        "{context} term diagnostics"
    );
    assert_eq!(
        actual_breakdown.domain_margin_diagnostics, expected_breakdown.domain_margin_diagnostics,
        "{context} domain margins"
    );
    assert_eq!(
        actual_breakdown.admissibility, expected_breakdown.admissibility,
        "{context} admissibility"
    );
}

pub(crate) fn assert_joint_refinement_optimizer_diagnostics_matches(
    actual_diagnostics: &JointRefinementStageOptimizerDiagnostics,
    expected_diagnostics: &JointRefinementStageOptimizerDiagnostics,
    context: &str,
) {
    assert_f64_within_one_ulp(
        actual_diagnostics.local_objective,
        expected_diagnostics.local_objective,
        &format!("{context} local_objective"),
    );
    assert_f64_within_one_ulp(
        actual_diagnostics.final_objective,
        expected_diagnostics.final_objective,
        &format!("{context} final_objective"),
    );
    assert_eq!(
        actual_diagnostics.local_evaluation_count, expected_diagnostics.local_evaluation_count,
        "{context} local_evaluation_count drifted"
    );
    assert_eq!(
        actual_diagnostics.final_simulation_seeds, expected_diagnostics.final_simulation_seeds,
        "{context} final_simulation_seeds drifted"
    );
    assert_eq!(
        actual_diagnostics.stop_reason, expected_diagnostics.stop_reason,
        "{context} stop_reason drifted"
    );
    assert_joint_refinement_candidate_diagnostics_match(
        &actual_diagnostics.candidate_diagnostics,
        &expected_diagnostics.candidate_diagnostics,
        &format!("{context} candidate_diagnostics"),
    );
    assert_joint_refinement_runtime_profile_matches(
        &actual_diagnostics.runtime_profile,
        &expected_diagnostics.runtime_profile,
        &format!("{context} runtime_profile"),
    );
}

fn assert_joint_refinement_candidate_diagnostics_match(
    actual_diagnostics: &JointRefinementStageCandidateDiagnostics,
    expected_diagnostics: &JointRefinementStageCandidateDiagnostics,
    context: &str,
) {
    assert_eq!(
        actual_diagnostics.candidate_count, expected_diagnostics.candidate_count,
        "{context} candidate_count drifted"
    );
    assert_eq!(
        actual_diagnostics.valid_candidate_count, expected_diagnostics.valid_candidate_count,
        "{context} valid_candidate_count drifted"
    );
    assert_eq!(
        actual_diagnostics.invalid_candidate_count, expected_diagnostics.invalid_candidate_count,
        "{context} invalid_candidate_count drifted"
    );
    assert_eq!(
        actual_diagnostics.unique_terminal_candidate_count,
        expected_diagnostics.unique_terminal_candidate_count,
        "{context} unique_terminal_candidate_count drifted"
    );
    assert_eq!(
        actual_diagnostics.terminal_collision_count, expected_diagnostics.terminal_collision_count,
        "{context} terminal_collision_count drifted"
    );
    assert_f64_within_one_ulp(
        actual_diagnostics.terminal_collision_share,
        expected_diagnostics.terminal_collision_share,
        &format!("{context} terminal_collision_share"),
    );
    assert_eq!(
        actual_diagnostics.stop_reason_frequencies, expected_diagnostics.stop_reason_frequencies,
        "{context} stop_reason_frequencies drifted"
    );
    assert_eq!(
        actual_diagnostics.invalid_reason_frequencies,
        expected_diagnostics.invalid_reason_frequencies,
        "{context} invalid_reason_frequencies drifted"
    );
}

fn assert_joint_refinement_runtime_profile_matches(
    actual_profile: &JointRefinementStageRuntimeProfile,
    expected_profile: &JointRefinementStageRuntimeProfile,
    context: &str,
) {
    assert_eq!(
        actual_profile.population_refinement_wall_clock_millis,
        expected_profile.population_refinement_wall_clock_millis,
        "{context} population_refinement_wall_clock_millis drifted"
    );
    assert_eq!(
        actual_profile.coarse_consistency_wall_clock_millis,
        expected_profile.coarse_consistency_wall_clock_millis,
        "{context} coarse_consistency_wall_clock_millis drifted"
    );
    assert_eq!(
        actual_profile.final_seed_evaluation_wall_clock_millis,
        expected_profile.final_seed_evaluation_wall_clock_millis,
        "{context} final_seed_evaluation_wall_clock_millis drifted"
    );
    assert_eq!(
        actual_profile.final_assembly_wall_clock_millis,
        expected_profile.final_assembly_wall_clock_millis,
        "{context} final_assembly_wall_clock_millis drifted"
    );
    assert_eq!(
        actual_profile.objective_cache_hit_count, expected_profile.objective_cache_hit_count,
        "{context} objective_cache_hit_count drifted"
    );
    assert_eq!(
        actual_profile.objective_cache_miss_count, expected_profile.objective_cache_miss_count,
        "{context} objective_cache_miss_count drifted"
    );
    assert_eq!(
        actual_profile.deterministic_cache_hit_count,
        expected_profile.deterministic_cache_hit_count,
        "{context} deterministic_cache_hit_count drifted"
    );
    assert_eq!(
        actual_profile.deterministic_cache_miss_count,
        expected_profile.deterministic_cache_miss_count,
        "{context} deterministic_cache_miss_count drifted"
    );
    assert_eq!(
        actual_profile.seed_summary_cache_hit_count, expected_profile.seed_summary_cache_hit_count,
        "{context} seed_summary_cache_hit_count drifted"
    );
    assert_eq!(
        actual_profile.seed_summary_cache_miss_count,
        expected_profile.seed_summary_cache_miss_count,
        "{context} seed_summary_cache_miss_count drifted"
    );
    assert_eq!(
        actual_profile.aggregated_likelihood_context_cache_hit_count,
        expected_profile.aggregated_likelihood_context_cache_hit_count,
        "{context} aggregated_likelihood_context_cache_hit_count drifted"
    );
    assert_eq!(
        actual_profile.aggregated_likelihood_context_cache_miss_count,
        expected_profile.aggregated_likelihood_context_cache_miss_count,
        "{context} aggregated_likelihood_context_cache_miss_count drifted"
    );
    assert_eq!(
        actual_profile.option_pricing_cache_hit_count,
        expected_profile.option_pricing_cache_hit_count,
        "{context} option_pricing_cache_hit_count drifted"
    );
    assert_eq!(
        actual_profile.option_pricing_cache_miss_count,
        expected_profile.option_pricing_cache_miss_count,
        "{context} option_pricing_cache_miss_count drifted"
    );
    assert_eq!(
        actual_profile.option_pricing_run_count, expected_profile.option_pricing_run_count,
        "{context} option_pricing_run_count drifted"
    );
    assert_eq!(
        actual_profile.invalid_candidate_evaluation_count,
        expected_profile.invalid_candidate_evaluation_count,
        "{context} invalid_candidate_evaluation_count drifted"
    );
    assert_eq!(
        actual_profile.cumulative_deterministic_evaluation_millis,
        expected_profile.cumulative_deterministic_evaluation_millis,
        "{context} cumulative_deterministic_evaluation_millis drifted"
    );
    assert_eq!(
        actual_profile.cumulative_modeled_rate_millis,
        expected_profile.cumulative_modeled_rate_millis,
        "{context} cumulative_modeled_rate_millis drifted"
    );
    assert_eq!(
        actual_profile.cumulative_likelihood_context_resolution_millis,
        expected_profile.cumulative_likelihood_context_resolution_millis,
        "{context} cumulative_likelihood_context_resolution_millis drifted"
    );
    assert_eq!(
        actual_profile.cumulative_likelihood_context_1h_sample_millis,
        expected_profile.cumulative_likelihood_context_1h_sample_millis,
        "{context} cumulative_likelihood_context_1h_sample_millis drifted"
    );
    assert_eq!(
        actual_profile.cumulative_likelihood_context_1d_sample_millis,
        expected_profile.cumulative_likelihood_context_1d_sample_millis,
        "{context} cumulative_likelihood_context_1d_sample_millis drifted"
    );
    assert_eq!(
        actual_profile.cumulative_likelihood_context_tick_sample_millis,
        expected_profile.cumulative_likelihood_context_tick_sample_millis,
        "{context} cumulative_likelihood_context_tick_sample_millis drifted"
    );
    assert_eq!(
        actual_profile.cumulative_likelihood_context_tick_optimizer_millis,
        expected_profile.cumulative_likelihood_context_tick_optimizer_millis,
        "{context} cumulative_likelihood_context_tick_optimizer_millis drifted"
    );
    assert_eq!(
        actual_profile.cumulative_likelihood_context_1h_optimizer_millis,
        expected_profile.cumulative_likelihood_context_1h_optimizer_millis,
        "{context} cumulative_likelihood_context_1h_optimizer_millis drifted"
    );
    assert_eq!(
        actual_profile.cumulative_likelihood_context_1d_optimizer_millis,
        expected_profile.cumulative_likelihood_context_1d_optimizer_millis,
        "{context} cumulative_likelihood_context_1d_optimizer_millis drifted"
    );
    assert_eq!(
        actual_profile.cumulative_tick_likelihood_millis,
        expected_profile.cumulative_tick_likelihood_millis,
        "{context} cumulative_tick_likelihood_millis drifted"
    );
    assert_eq!(
        actual_profile.cumulative_1h_likelihood_millis,
        expected_profile.cumulative_1h_likelihood_millis,
        "{context} cumulative_1h_likelihood_millis drifted"
    );
    assert_eq!(
        actual_profile.cumulative_1d_likelihood_millis,
        expected_profile.cumulative_1d_likelihood_millis,
        "{context} cumulative_1d_likelihood_millis drifted"
    );
    assert_eq!(
        actual_profile.cumulative_expected_activity_counts_millis,
        expected_profile.cumulative_expected_activity_counts_millis,
        "{context} cumulative_expected_activity_counts_millis drifted"
    );
    assert_eq!(
        actual_profile.cumulative_deterministic_activity_shape_millis,
        expected_profile.cumulative_deterministic_activity_shape_millis,
        "{context} cumulative_deterministic_activity_shape_millis drifted"
    );
    assert_eq!(
        actual_profile.cumulative_deterministic_activity_level_millis,
        expected_profile.cumulative_deterministic_activity_level_millis,
        "{context} cumulative_deterministic_activity_level_millis drifted"
    );
    assert_eq!(
        actual_profile.cumulative_seed_summary_simulation_millis,
        expected_profile.cumulative_seed_summary_simulation_millis,
        "{context} cumulative_seed_summary_simulation_millis drifted"
    );
    assert_eq!(
        actual_profile.cumulative_raw_seed_metrics_millis,
        expected_profile.cumulative_raw_seed_metrics_millis,
        "{context} cumulative_raw_seed_metrics_millis drifted"
    );
    assert_eq!(
        actual_profile.cumulative_loss_build_millis, expected_profile.cumulative_loss_build_millis,
        "{context} cumulative_loss_build_millis drifted"
    );
    assert_eq!(
        actual_profile.cumulative_option_pricing_millis,
        expected_profile.cumulative_option_pricing_millis,
        "{context} cumulative_option_pricing_millis drifted"
    );
}

pub(crate) fn assert_hawkes_fitted_parameters_match(
    actual_parameters: &HawkesStageFittedParameters,
    expected_parameters: &HawkesStageFittedParameters,
    context: &str,
) {
    assert_hawkes_kernel_parameters_match(
        &actual_parameters.kernel_parameters,
        &expected_parameters.kernel_parameters,
        &format!("{context} kernel_parameters"),
    );
    assert_hawkes_structured_alpha_match(
        &actual_parameters.structured_alpha,
        &expected_parameters.structured_alpha,
        &format!("{context} structured_alpha"),
    );
    assert_hawkes_structured_rho_match(
        &actual_parameters.structured_rho,
        &expected_parameters.structured_rho,
        &format!("{context} structured_rho"),
    );
    assert_eq!(
        actual_parameters.self_dominance_policy, expected_parameters.self_dominance_policy,
        "{context} self_dominance_policy drifted"
    );
    assert_f64_within_one_ulp(
        actual_parameters.stability_numeric_tolerance,
        expected_parameters.stability_numeric_tolerance,
        &format!("{context} stability_numeric_tolerance"),
    );
}

pub(crate) fn assert_joint_refinement_fitted_parameters_match(
    actual_parameters: &JointRefinementStageFittedParameters,
    expected_parameters: &JointRefinementStageFittedParameters,
    context: &str,
) {
    assert_hawkes_fitted_parameters_match(
        &actual_parameters.hawkes_fitted_parameters,
        &expected_parameters.hawkes_fitted_parameters,
        &format!("{context} hawkes_fitted_parameters"),
    );
    assert_return_components_fitted_parameters_match(
        &actual_parameters.return_components_fitted_parameters,
        &expected_parameters.return_components_fitted_parameters,
        &format!("{context} return_components_fitted_parameters"),
    );
}

pub(crate) fn assert_optimizer_state_matches(
    actual_optimizer_state: &Option<CalibrationStageParameterOptimizerState>,
    expected_optimizer_state: &Option<CalibrationStageParameterOptimizerState>,
    context: &str,
) {
    match (actual_optimizer_state, expected_optimizer_state) {
        (None, None) => {}
        (Some(actual_optimizer_state), Some(expected_optimizer_state)) => {
            assert_eq!(
                actual_optimizer_state.schema_version, expected_optimizer_state.schema_version,
                "{context} schema_version drifted"
            );
            assert_eq!(
                actual_optimizer_state.optimizer_adapter_metadata,
                expected_optimizer_state.optimizer_adapter_metadata,
                "{context} optimizer_adapter_metadata drifted"
            );
            assert_eq!(
                actual_optimizer_state.input_candidate_count,
                expected_optimizer_state.input_candidate_count,
                "{context} input_candidate_count drifted"
            );
            assert_eq!(
                actual_optimizer_state.valid_candidate_count,
                expected_optimizer_state.valid_candidate_count,
                "{context} valid_candidate_count drifted"
            );
            assert_f64_within_one_ulp(
                actual_optimizer_state.selected_candidate_raw_objective,
                expected_optimizer_state.selected_candidate_raw_objective,
                &format!("{context} selected_candidate_raw_objective"),
            );
            assert_f64_within_one_ulp(
                actual_optimizer_state.selected_candidate_penalized_objective,
                expected_optimizer_state.selected_candidate_penalized_objective,
                &format!("{context} selected_candidate_penalized_objective"),
            );
        }
        _ => panic!(
            "{context} optimizer_state presence drifted: actual={actual_optimizer_state:?} expected={expected_optimizer_state:?}"
        ),
    }
}

fn assert_f64_within_one_ulp(actual_value: f64, expected_value: f64, context: &str) {
    if actual_value == expected_value {
        return;
    }

    let actual_ordered_bits = ordered_f64_bits(actual_value);
    let expected_ordered_bits = ordered_f64_bits(expected_value);
    let bit_distance = actual_ordered_bits.abs_diff(expected_ordered_bits);

    assert!(
        bit_distance <= 1,
        "{context} drifted: actual={actual_value:?} expected={expected_value:?} ulp_distance={bit_distance}"
    );
}

fn assert_hawkes_kernel_parameters_match(
    actual_parameters: &HawkesKernelParameters,
    expected_parameters: &HawkesKernelParameters,
    context: &str,
) {
    assert_f64_within_one_ulp(
        actual_parameters.lambda_bar,
        expected_parameters.lambda_bar,
        &format!("{context} lambda_bar"),
    );
    assert_f64_within_one_ulp(
        actual_parameters.beta,
        expected_parameters.beta,
        &format!("{context} beta"),
    );
    assert_hawkes_kernel_loadings_match(
        &actual_parameters.alpha,
        &expected_parameters.alpha,
        &format!("{context} alpha"),
    );
    assert_hawkes_kernel_loadings_match(
        &actual_parameters.rho,
        &expected_parameters.rho,
        &format!("{context} rho"),
    );
}

fn assert_return_components_fitted_parameters_match(
    actual_parameters: &ReturnComponentsStageFittedParameters,
    expected_parameters: &ReturnComponentsStageFittedParameters,
    context: &str,
) {
    assert_nig_shared_plus_shift_configuration_match(
        actual_parameters.nig_shared_plus_shift_configuration,
        expected_parameters.nig_shared_plus_shift_configuration,
        &format!("{context} nig_shared_plus_shift_configuration"),
    );
    assert_news_process_configuration_match(
        &actual_parameters.news_process_configuration,
        &expected_parameters.news_process_configuration,
        &format!("{context} news_process_configuration"),
    );
    assert_eq!(
        actual_parameters
            .surprise_deterministic_intensity_profile
            .profile_resolution_hours,
        expected_parameters
            .surprise_deterministic_intensity_profile
            .profile_resolution_hours,
        "{context} surprise_deterministic_intensity_profile.profile_resolution_hours drifted"
    );
    assert_eq!(
        actual_parameters
            .surprise_deterministic_intensity_profile
            .fit_policy,
        expected_parameters
            .surprise_deterministic_intensity_profile
            .fit_policy,
        "{context} surprise_deterministic_intensity_profile.fit_policy drifted"
    );
    assert_eq!(
        actual_parameters
            .surprise_deterministic_intensity_profile
            .normalization_policy,
        expected_parameters
            .surprise_deterministic_intensity_profile
            .normalization_policy,
        "{context} surprise_deterministic_intensity_profile.normalization_policy drifted"
    );
    assert_eq!(
        actual_parameters
            .surprise_deterministic_intensity_profile
            .weekly_utc_profile_multipliers
            .len(),
        expected_parameters
            .surprise_deterministic_intensity_profile
            .weekly_utc_profile_multipliers
            .len(),
        "{context} surprise_deterministic_intensity_profile.weekly_utc_profile_multipliers length drifted"
    );

    for (position, (actual_multiplier, expected_multiplier)) in actual_parameters
        .surprise_deterministic_intensity_profile
        .weekly_utc_profile_multipliers
        .iter()
        .zip(
            expected_parameters
                .surprise_deterministic_intensity_profile
                .weekly_utc_profile_multipliers
                .iter(),
        )
        .enumerate()
    {
        assert_f64_within_one_ulp(
            *actual_multiplier,
            *expected_multiplier,
            &format!(
                "{context} surprise_deterministic_intensity_profile.weekly_utc_profile_multipliers[{position}]"
            ),
        );
    }

    assert_eq!(
        actual_parameters.mean_reversion_fitted_parameters,
        expected_parameters.mean_reversion_fitted_parameters,
        "{context} mean_reversion_fitted_parameters drifted"
    );
}

fn assert_nig_shared_plus_shift_configuration_match(
    actual_configuration: NigSharedPlusShiftConfiguration,
    expected_configuration: NigSharedPlusShiftConfiguration,
    context: &str,
) {
    assert_f64_within_one_ulp(
        actual_configuration.alpha,
        expected_configuration.alpha,
        &format!("{context} alpha"),
    );
    assert_f64_within_one_ulp(
        actual_configuration.beta_0,
        expected_configuration.beta_0,
        &format!("{context} beta_0"),
    );
    assert_f64_within_one_ulp(
        actual_configuration.delta,
        expected_configuration.delta,
        &format!("{context} delta"),
    );
    assert_f64_within_one_ulp(
        actual_configuration.mu,
        expected_configuration.mu,
        &format!("{context} mu"),
    );
    assert_f64_within_one_ulp(
        actual_configuration.delta_beta,
        expected_configuration.delta_beta,
        &format!("{context} delta_beta"),
    );
}

fn assert_news_process_configuration_match(
    actual_configuration: &NewsProcessConfiguration,
    expected_configuration: &NewsProcessConfiguration,
    context: &str,
) {
    assert_eq!(
        actual_configuration.schema_version, expected_configuration.schema_version,
        "{context} schema_version drifted"
    );
    assert_news_arrival_configuration_match(
        &actual_configuration.arrivals,
        &expected_configuration.arrivals,
        &format!("{context} arrivals"),
    );
    assert_shared_news_mark_law_configuration_match(
        actual_configuration.shared_mark_law,
        expected_configuration.shared_mark_law,
        &format!("{context} shared_mark_law"),
    );
    assert_news_component_mark_scale_configuration_match(
        actual_configuration.known_component_mark_scales,
        expected_configuration.known_component_mark_scales,
        &format!("{context} known_component_mark_scales"),
    );
    assert_news_component_mark_scale_configuration_match(
        actual_configuration.surprise_component_mark_scales,
        expected_configuration.surprise_component_mark_scales,
        &format!("{context} surprise_component_mark_scales"),
    );
}

fn assert_news_arrival_configuration_match(
    actual_configuration: &NewsArrivalConfiguration,
    expected_configuration: &NewsArrivalConfiguration,
    context: &str,
) {
    assert_eq!(
        actual_configuration
            .known
            .scheduled_event_timestamps_utc_nanos,
        expected_configuration
            .known
            .scheduled_event_timestamps_utc_nanos,
        "{context} known.scheduled_event_timestamps_utc_nanos drifted"
    );
    assert_surprise_news_arrival_configuration_match(
        actual_configuration.surprise,
        expected_configuration.surprise,
        &format!("{context} surprise"),
    );
}

fn assert_surprise_news_arrival_configuration_match(
    actual_configuration: SurpriseNewsArrivalConfiguration,
    expected_configuration: SurpriseNewsArrivalConfiguration,
    context: &str,
) {
    assert_f64_within_one_ulp(
        actual_configuration.open_baseline_intensity,
        expected_configuration.open_baseline_intensity,
        &format!("{context} open_baseline_intensity"),
    );
    assert_f64_within_one_ulp(
        actual_configuration.closed_baseline_intensity,
        expected_configuration.closed_baseline_intensity,
        &format!("{context} closed_baseline_intensity"),
    );
}

fn assert_shared_news_mark_law_configuration_match(
    actual_configuration: SharedNewsMarkLawConfiguration,
    expected_configuration: SharedNewsMarkLawConfiguration,
    context: &str,
) {
    assert_f64_within_one_ulp(
        actual_configuration.gamma_severity_marks.shape_k,
        expected_configuration.gamma_severity_marks.shape_k,
        &format!("{context} gamma_severity_marks.shape_k"),
    );
}

fn assert_news_component_mark_scale_configuration_match(
    actual_configuration: NewsComponentMarkScaleConfiguration,
    expected_configuration: NewsComponentMarkScaleConfiguration,
    context: &str,
) {
    assert_f64_within_one_ulp(
        actual_configuration.activity_coupling_scale,
        expected_configuration.activity_coupling_scale,
        &format!("{context} activity_coupling_scale"),
    );
    assert_f64_within_one_ulp(
        actual_configuration.return_jump_drift,
        expected_configuration.return_jump_drift,
        &format!("{context} return_jump_drift"),
    );
    assert_f64_within_one_ulp(
        actual_configuration.return_jump_volatility,
        expected_configuration.return_jump_volatility,
        &format!("{context} return_jump_volatility"),
    );
}

fn assert_hawkes_kernel_loadings_match(
    actual_loadings: &HawkesKernelLoadings,
    expected_loadings: &HawkesKernelLoadings,
    context: &str,
) {
    assert_f64_within_one_ulp(
        actual_loadings.plus_plus,
        expected_loadings.plus_plus,
        &format!("{context} plus_plus"),
    );
    assert_f64_within_one_ulp(
        actual_loadings.plus_minus,
        expected_loadings.plus_minus,
        &format!("{context} plus_minus"),
    );
    assert_f64_within_one_ulp(
        actual_loadings.minus_plus,
        expected_loadings.minus_plus,
        &format!("{context} minus_plus"),
    );
    assert_f64_within_one_ulp(
        actual_loadings.minus_minus,
        expected_loadings.minus_minus,
        &format!("{context} minus_minus"),
    );
}

fn assert_hawkes_structured_alpha_match(
    actual_parameters: &HawkesStructuredAlphaParameterization,
    expected_parameters: &HawkesStructuredAlphaParameterization,
    context: &str,
) {
    assert_f64_within_one_ulp(
        actual_parameters.a_self,
        expected_parameters.a_self,
        &format!("{context} a_self"),
    );
    assert_f64_within_one_ulp(
        actual_parameters.a_cross,
        expected_parameters.a_cross,
        &format!("{context} a_cross"),
    );
    assert_f64_within_one_ulp(
        actual_parameters.delta_self,
        expected_parameters.delta_self,
        &format!("{context} delta_self"),
    );
    assert_f64_within_one_ulp(
        actual_parameters.delta_cross,
        expected_parameters.delta_cross,
        &format!("{context} delta_cross"),
    );
}

fn assert_hawkes_structured_rho_match(
    actual_parameters: &HawkesStructuredRhoParameterization,
    expected_parameters: &HawkesStructuredRhoParameterization,
    context: &str,
) {
    assert_f64_within_one_ulp(
        actual_parameters.r_self,
        expected_parameters.r_self,
        &format!("{context} r_self"),
    );
    assert_f64_within_one_ulp(
        actual_parameters.r_cross,
        expected_parameters.r_cross,
        &format!("{context} r_cross"),
    );
    assert_f64_within_one_ulp(
        actual_parameters.delta_self_r,
        expected_parameters.delta_self_r,
        &format!("{context} delta_self_r"),
    );
    assert_f64_within_one_ulp(
        actual_parameters.delta_cross_r,
        expected_parameters.delta_cross_r,
        &format!("{context} delta_cross_r"),
    );
}

fn ordered_f64_bits(value: f64) -> u64 {
    let raw_bits = value.to_bits();
    let sign_bit = 1_u64 << 63;

    if raw_bits & sign_bit == 0 {
        raw_bits | sign_bit
    } else {
        !raw_bits
    }
}

pub(crate) fn build_data_inputs() -> CalibrationRunDataInputs {
    CalibrationRunDataInputs::try_new(
        "XNYS",
        "dataset://synthetic/trades/v1",
        None,
        Some("dataset://synthetic/options/v1".to_string()),
    )
    .expect("data inputs should validate")
}

pub(crate) fn build_seed_settings() -> CalibrationRunSeedSettings {
    CalibrationRunSeedSettings::try_new(
        17,
        vec![
            CalibrationStageSeedOverride {
                stage: CalibrationStage::Seasonality,
                seed: 101,
            },
            CalibrationStageSeedOverride {
                stage: CalibrationStage::Hawkes,
                seed: 202,
            },
        ],
    )
    .expect("seed settings should validate")
}

pub(crate) fn build_solver_settings() -> CalibrationOptimizerSettings {
    CalibrationOptimizerSettings::try_new("lbfgs", 64, 1, 1e-8, 1e-6, 10.0)
        .expect("solver settings should validate")
}

pub(crate) fn build_joint_refinement_solver_settings() -> CalibrationOptimizerSettings {
    CalibrationOptimizerSettings::try_new("lbfgs", 32, 1, 1e-8, 1e-6, 10.0)
        .expect("joint-refinement solver settings should validate")
}

pub(crate) fn build_solver_settings_for_seasonality_and_hawkes()
-> Vec<CalibrationStageSolverSettings> {
    vec![
        CalibrationStageSolverSettings {
            stage: CalibrationStage::Seasonality,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::Hawkes,
            optimizer_settings: build_solver_settings(),
        },
    ]
}

pub(crate) fn build_solver_settings_for_all_stages() -> Vec<CalibrationStageSolverSettings> {
    vec![
        CalibrationStageSolverSettings {
            stage: CalibrationStage::Seasonality,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::Hawkes,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::ReturnComponents,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::LongVariance,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::NewsShocks,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::ReturnBackfit,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::MeanReversion,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::JointRefinement,
            optimizer_settings: build_joint_refinement_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::Finalization,
            optimizer_settings: build_joint_refinement_solver_settings(),
        },
    ]
}

pub(crate) fn build_solver_settings_for_pre_joint_pipeline() -> Vec<CalibrationStageSolverSettings>
{
    vec![
        CalibrationStageSolverSettings {
            stage: CalibrationStage::Seasonality,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::Hawkes,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::ReturnComponents,
            optimizer_settings: build_solver_settings(),
        },
    ]
}

pub(crate) fn build_valid_run_spec() -> CalibrationRunSpec {
    build_valid_run_spec_with_mode(CalibrationExecutionMode::Memory)
}

pub(crate) fn build_valid_run_spec_with_mode(
    execution_mode: CalibrationExecutionMode,
) -> CalibrationRunSpec {
    CalibrationRunSpec::try_new(
        "run-001",
        build_data_inputs(),
        execution_mode,
        vec![
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            CalibrationStage::ReturnComponents,
            CalibrationStage::LongVariance,
            CalibrationStage::MeanReversion,
            CalibrationStage::JointRefinement,
        ],
        build_seed_settings(),
        build_solver_settings_for_all_stages(),
    )
    .expect("run spec should validate")
}

pub(crate) fn build_valid_artifact_manifest() -> CalibrationArtifactManifest {
    let run_contract =
        build_run_contract_with_mode_and_resume("run-001", CalibrationExecutionMode::Disk, None);
    let dataset_snapshot =
        build_calibration_dataset_snapshot_artifact(&run_contract).expect("dataset snapshot");

    CalibrationArtifactManifest::try_new(
        "run-001",
        CalibrationArtifactManifestIndexMetadata {
            committed_at_utc: "2026-03-05T12:30:00.000000000Z".to_string(),
            committed_at_unix_nanos: utc_nanos(2026, 3, 5, 12, 30, 0),
            exchange_code: dataset_snapshot.exchange_code.clone(),
            dataset_snapshot_checksum: dataset_snapshot.snapshot_checksum.clone(),
            trade_events_data_reference: run_contract
                .run_spec
                .data_inputs
                .trade_events_data_reference
                .clone(),
            trade_events_reference_checksum: dataset_snapshot
                .input_datasets
                .iter()
                .find(|input_dataset| {
                    input_dataset.input_name
                        == CalibrationDatasetInputName::TradeEventsDataReference
                })
                .expect("trade events dataset should exist")
                .reference_checksum
                .clone(),
            option_surface_data_reference: run_contract
                .run_spec
                .data_inputs
                .option_surface_data_reference
                .clone(),
            option_surface_reference_checksum: dataset_snapshot
                .input_datasets
                .iter()
                .find(|input_dataset| {
                    input_dataset.input_name
                        == CalibrationDatasetInputName::OptionSurfaceDataReference
                })
                .map(|input_dataset| input_dataset.reference_checksum.clone()),
            model_version: CalibrationArtifactManifestModelVersion {
                run_contract_schema_version: CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION.to_string(),
                run_spec_schema_version: CALIBRATION_RUN_SPEC_SCHEMA_VERSION.to_string(),
                seasonality_stage_output_schema_version: Some(
                    CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION.to_string(),
                ),
                hawkes_stage_output_schema_version: Some(
                    CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION.to_string(),
                ),
                return_components_stage_output_schema_version: Some(
                    CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION.to_string(),
                ),
                mean_reversion_stage_output_schema_version: Some(
                    CALIBRATION_MEAN_REVERSION_STAGE_OUTPUT_SCHEMA_VERSION.to_string(),
                ),
                joint_refinement_stage_output_schema_version: Some(
                    CALIBRATION_JOINT_REFINEMENT_STAGE_OUTPUT_SCHEMA_VERSION.to_string(),
                ),
                option_refinement_stage_output_schema_version: None,
                finalization_stage_output_schema_version: None,
                seasonality_estimator_version: Some(
                    CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION.to_string(),
                ),
            },
            validation_metric_summaries: vec![CalibrationArtifactManifestValidationMetricSummary {
                artifact_role: CalibrationArtifactManifestValidationMetricRole::Diagnostics,
                artifact_reference: "artifact://diagnostics/run-001".to_string(),
                overall_status: CalibrationArtifactManifestMetricStatus::Fail,
                collection_count: 1,
                metric_count: 2,
                collection_status_counts: vec![CalibrationArtifactManifestMetricStatusCount {
                    status: CalibrationArtifactManifestMetricStatus::Fail,
                    count: 1,
                }],
                metric_status_counts: vec![
                    CalibrationArtifactManifestMetricStatusCount {
                        status: CalibrationArtifactManifestMetricStatus::Pass,
                        count: 1,
                    },
                    CalibrationArtifactManifestMetricStatusCount {
                        status: CalibrationArtifactManifestMetricStatus::Fail,
                        count: 1,
                    },
                ],
                failing_metric_ids: vec!["tail_skew.hourly".to_string()],
            }],
        },
        vec![
            CalibrationArtifactReference {
                artifact_id: "run_contract".to_string(),
                artifact_kind: CalibrationArtifactKind::RunContract,
                artifact_reference: "artifact://run_contract/run-001".to_string(),
                stage: None,
            },
            CalibrationArtifactReference {
                artifact_id: "seasonality_checkpoint".to_string(),
                artifact_kind: CalibrationArtifactKind::StageCheckpoint,
                artifact_reference: "artifact://stage_checkpoint/seasonality/run-001".to_string(),
                stage: Some(CalibrationStage::Seasonality),
            },
            CalibrationArtifactReference {
                artifact_id: "stylized_fact_report".to_string(),
                artifact_kind: CalibrationArtifactKind::Diagnostics,
                artifact_reference: "artifact://diagnostics/stylized_facts/run-001".to_string(),
                stage: None,
            },
        ],
    )
    .expect("artifact manifest should validate")
}

pub(crate) fn build_validation_metric_artifact_fixture() -> CalibrationValidationMetricArtifact {
    CalibrationValidationMetricArtifact::try_new(
        "run-001",
        CalibrationValidationMetricArtifactRole::Diagnostics,
        vec![
            CalibrationValidationMetricStageOutputLink {
                stage: CalibrationStage::Seasonality,
                artifact_reference: "artifact://seasonality/run-001".to_string(),
                stage_output_schema_version:
                    CALIBRATION_SEASONALITY_STAGE_EXECUTION_REPORT_SCHEMA_VERSION.to_string(),
            },
            CalibrationValidationMetricStageOutputLink {
                stage: CalibrationStage::Hawkes,
                artifact_reference: "artifact://hawkes/run-001".to_string(),
                stage_output_schema_version:
                    CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION.to_string(),
            },
            CalibrationValidationMetricStageOutputLink {
                stage: CalibrationStage::ReturnComponents,
                artifact_reference: "artifact://return_components/run-001".to_string(),
                stage_output_schema_version:
                    CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION.to_string(),
            },
            CalibrationValidationMetricStageOutputLink {
                stage: CalibrationStage::MeanReversion,
                artifact_reference: "artifact://mean_reversion/run-001".to_string(),
                stage_output_schema_version:
                    CALIBRATION_MEAN_REVERSION_STAGE_EXECUTION_REPORT_SCHEMA_VERSION.to_string(),
            },
            CalibrationValidationMetricStageOutputLink {
                stage: CalibrationStage::JointRefinement,
                artifact_reference: "artifact://joint_refinement/run-001".to_string(),
                stage_output_schema_version:
                    CALIBRATION_JOINT_REFINEMENT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION.to_string(),
            },
        ],
        vec![CalibrationValidationMetricCollection {
            collection_id: "activity-clustering-1h".to_string(),
            source_kind: CalibrationValidationMetricSourceKind::ActivityClustering,
            source_schema_version: "validation.activity_clustering.report.v1".to_string(),
            status: CalibrationValidationMetricStatus::Pass,
            context: CalibrationValidationMetricContext {
                context_kind: CalibrationValidationMetricContextKind::Horizon,
                context_id: "1h".to_string(),
                context_label: "1 Hour".to_string(),
            },
            primary_role_label: "empirical".to_string(),
            secondary_role_label: Some("simulated".to_string()),
            threshold_policy_id: "activity_clustering.default.v1".to_string(),
            metrics: vec![CalibrationValidationMetricRecord {
                metric_id: "returns_abs_quantile_shape_ratio_q99_q95".to_string(),
                metric_name: "Returns Abs Quantile Shape Ratio Q99 Q95".to_string(),
                metric_variant: None,
                status: CalibrationValidationMetricStatus::Pass,
                primary_value: CalibrationValidationObservedValue {
                    value: Some(1.18),
                    confidence_interval: Some(CalibrationValidationMetricConfidenceInterval {
                        lower_bound: 1.11,
                        upper_bound: 1.24,
                        confidence_level: 0.95,
                    }),
                },
                secondary_value: Some(CalibrationValidationObservedValue {
                    value: Some(1.16),
                    confidence_interval: Some(CalibrationValidationMetricConfidenceInterval {
                        lower_bound: 1.10,
                        upper_bound: 1.22,
                        confidence_level: 0.95,
                    }),
                }),
                delta_value: Some(-0.02),
                absolute_delta: Some(0.02),
                standardized_delta: Some(0.31),
                threshold_metadata: CalibrationValidationMetricThresholdMetadata {
                    threshold_policy_id: "activity_clustering.default.v1".to_string(),
                    threshold_policy_version: Some("1.0.0".to_string()),
                    threshold_id: Some("abs_delta_q99_q95".to_string()),
                    threshold_rationale: Some(
                        "Keep tail-shape gap below approved threshold".to_string(),
                    ),
                    max_absolute_delta: Some(0.05),
                    max_standardized_delta: Some(1.0),
                    absolute_delta_exceeds_threshold: Some(false),
                    standardized_delta_exceeds_threshold: Some(false),
                },
            }],
        }],
    )
    .expect("validation metric artifact fixture should validate")
}

pub(crate) fn build_stage_output_links(
    stage_parameter_artifacts: &[CalibrationStageParameterArtifact],
) -> Vec<CalibrationValidationMetricStageOutputLink> {
    let mut stage_output_links = stage_parameter_artifacts
        .iter()
        .map(|artifact| CalibrationValidationMetricStageOutputLink {
            stage: artifact.stage,
            artifact_reference: artifact.artifact_reference.clone(),
            stage_output_schema_version: artifact.stage_output_schema_version.clone(),
        })
        .collect::<Vec<_>>();
    stage_output_links.sort_by_key(|link| link.stage);

    stage_output_links
}

pub(crate) fn build_validation_metric_manifest_fixture() -> CalibrationArtifactManifest {
    let index_metadata = build_valid_artifact_manifest().index_metadata;

    CalibrationArtifactManifest::try_new(
        "run-001",
        index_metadata,
        vec![
            CalibrationArtifactReference {
                artifact_id: "dataset_snapshot".to_string(),
                artifact_kind: CalibrationArtifactKind::DatasetSnapshot,
                artifact_reference: "artifact://dataset_snapshot/run-001".to_string(),
                stage: None,
            },
            CalibrationArtifactReference {
                artifact_id: "seasonality_stage_output".to_string(),
                artifact_kind: CalibrationArtifactKind::StageOutput,
                artifact_reference: "artifact://seasonality/run-001".to_string(),
                stage: Some(CalibrationStage::Seasonality),
            },
            CalibrationArtifactReference {
                artifact_id: "hawkes_stage_output".to_string(),
                artifact_kind: CalibrationArtifactKind::StageOutput,
                artifact_reference: "artifact://hawkes/run-001".to_string(),
                stage: Some(CalibrationStage::Hawkes),
            },
            CalibrationArtifactReference {
                artifact_id: "return_components_stage_output".to_string(),
                artifact_kind: CalibrationArtifactKind::StageOutput,
                artifact_reference: "artifact://return_components/run-001".to_string(),
                stage: Some(CalibrationStage::ReturnComponents),
            },
            CalibrationArtifactReference {
                artifact_id: "mean_reversion_stage_output".to_string(),
                artifact_kind: CalibrationArtifactKind::StageOutput,
                artifact_reference: "artifact://mean_reversion/run-001".to_string(),
                stage: Some(CalibrationStage::MeanReversion),
            },
            CalibrationArtifactReference {
                artifact_id: "validation_metric_diagnostics".to_string(),
                artifact_kind: CalibrationArtifactKind::Diagnostics,
                artifact_reference: calibration_validation_metric_artifact_reference(
                    "run-001",
                    CalibrationValidationMetricArtifactRole::Diagnostics,
                ),
                stage: None,
            },
        ],
    )
    .expect("validation metric manifest fixture should validate")
}

pub(crate) fn build_calibration_stage_parameter_artifact(
    run_contract: &CalibrationRunContract,
    stage_report: &CalibrationStagedRunStageReport,
) -> Result<CalibrationStageParameterArtifact, CalibrationStageParameterArtifactBuildError> {
    let stage_input = stage_parameter_artifact_input(stage_report)?;

    janiform_calibration_artifacts::build_calibration_stage_parameter_artifact(
        run_contract,
        stage_input,
    )
}

fn stage_parameter_artifact_input(
    stage_report: &CalibrationStagedRunStageReport,
) -> Result<
    Box<dyn CalibrationStageParameterArtifactInput>,
    CalibrationStageParameterArtifactBuildError,
> {
    match stage_report {
        CalibrationStagedRunStageReport::Seasonality(report) => {
            let payload = report.normalized_stage_payload.as_ref().ok_or(
                CalibrationStageParameterArtifactBuildError::ValidationFailed {
                    stage: CalibrationStage::Seasonality,
                    error_message: "successful seasonality stage is missing its payload"
                        .to_string(),
                },
            )?;

            let metadata = report.metadata.as_ref().ok_or(
                CalibrationStageParameterArtifactBuildError::ValidationFailed {
                    stage: CalibrationStage::Seasonality,
                    error_message: "successful seasonality stage is missing metadata".to_string(),
                },
            )?;

            Ok(Box::new(SeasonalityStageParameterArtifactInput {
                metadata: CalibrationStageParameterBuildMetadata {
                    stage_output_schema_version: metadata.stage_output_schema_version.to_string(),
                    run_global_seed: metadata.run_global_seed,
                    stage_seed: metadata.stage_seed,
                    stage_seed_source: metadata.stage_seed_source,
                    stage_seed_derivation_policy: metadata.stage_seed_derivation_policy.to_string(),
                    solver_settings: metadata.solver_settings.clone(),
                },
                dataset_id: payload.dataset_id.clone(),
                exchange_code: payload.exchange_code.clone(),
                trade_events_data_reference: payload.trade_events_data_reference.clone(),
                estimator_version: metadata.estimator_version.clone(),
                timestamp_event_input_schema_version: payload
                    .timestamp_event_input_schema_version
                    .clone(),
                timestamp_event_preprocessed_schema_version: payload
                    .timestamp_event_preprocessed_schema_version
                    .clone(),
                clock_model_schema_version: payload.clock_model_schema_version.clone(),
                clock_model: payload.clock_model.clone(),
            }))
        }
        CalibrationStagedRunStageReport::Hawkes(report) => {
            let payload = report.fitted_stage_payload.as_ref().ok_or(
                CalibrationStageParameterArtifactBuildError::ValidationFailed {
                    stage: CalibrationStage::Hawkes,
                    error_message: "successful hawkes stage is missing its payload".to_string(),
                },
            )?;

            let metadata = report.metadata.as_ref().ok_or(
                CalibrationStageParameterArtifactBuildError::ValidationFailed {
                    stage: CalibrationStage::Hawkes,
                    error_message: "successful hawkes stage is missing metadata".to_string(),
                },
            )?;

            Ok(Box::new(HawkesStageParameterArtifactInput {
                metadata: CalibrationStageParameterBuildMetadata {
                    stage_output_schema_version: metadata.stage_output_schema_version.to_string(),
                    run_global_seed: metadata.run_global_seed,
                    stage_seed: metadata.stage_seed,
                    stage_seed_source: metadata.stage_seed_source,
                    stage_seed_derivation_policy: metadata.stage_seed_derivation_policy.to_string(),
                    solver_settings: metadata.solver_settings.clone(),
                },
                optimizer_state: CalibrationStageParameterOptimizerStateInput {
                    backend_id: metadata.optimizer_adapter_metadata.backend_id.clone(),
                    random_seed: metadata.optimizer_adapter_metadata.random_seed,
                    selection_policy: metadata
                        .optimizer_adapter_metadata
                        .selection_policy
                        .to_string(),
                    penalty_policy: metadata
                        .optimizer_adapter_metadata
                        .penalty_policy
                        .to_string(),
                    bounds_policy: metadata
                        .optimizer_adapter_metadata
                        .bounds_policy
                        .to_string(),
                    solver_settings: metadata.optimizer_adapter_metadata.solver_settings.clone(),
                    input_candidate_count: metadata.input_candidate_count,
                    valid_candidate_count: metadata.valid_candidate_count,
                    selected_candidate_raw_objective: metadata.selected_candidate_raw_objective,
                    selected_candidate_penalized_objective: metadata
                        .selected_candidate_penalized_objective,
                },
                selected_candidate_id: payload.selected_candidate_id.clone(),
                fitted_parameters: payload.fitted_parameters.clone(),
                seasonality_input_transport: payload.seasonality_input_transport,
                seasonality_dataset_id: payload.seasonality_dataset_id.clone(),
                seasonality_artifact_reference: payload.seasonality_artifact_reference.clone(),
            }))
        }
        CalibrationStagedRunStageReport::ReturnComponents(report) => {
            let payload = report.fitted_stage_payload.as_ref().ok_or(
                CalibrationStageParameterArtifactBuildError::ValidationFailed {
                    stage: CalibrationStage::ReturnComponents,
                    error_message: "successful return-components stage is missing its payload"
                        .to_string(),
                },
            )?;

            let metadata = report.metadata.as_ref().ok_or(
                CalibrationStageParameterArtifactBuildError::ValidationFailed {
                    stage: CalibrationStage::ReturnComponents,
                    error_message: "successful return-components stage is missing metadata"
                        .to_string(),
                },
            )?;

            Ok(Box::new(ReturnComponentsStageParameterArtifactInput {
                metadata: CalibrationStageParameterBuildMetadata {
                    stage_output_schema_version: metadata.stage_output_schema_version.to_string(),
                    run_global_seed: metadata.run_global_seed,
                    stage_seed: metadata.stage_seed,
                    stage_seed_source: metadata.stage_seed_source,
                    stage_seed_derivation_policy: metadata.stage_seed_derivation_policy.to_string(),
                    solver_settings: metadata.solver_settings.clone(),
                },
                optimizer_state: CalibrationStageParameterOptimizerStateInput {
                    backend_id: metadata.optimizer_adapter_metadata.backend_id.clone(),
                    random_seed: metadata.optimizer_adapter_metadata.random_seed,
                    selection_policy: metadata
                        .optimizer_adapter_metadata
                        .selection_policy
                        .to_string(),
                    penalty_policy: metadata
                        .optimizer_adapter_metadata
                        .penalty_policy
                        .to_string(),
                    bounds_policy: metadata
                        .optimizer_adapter_metadata
                        .bounds_policy
                        .to_string(),
                    solver_settings: metadata.optimizer_adapter_metadata.solver_settings.clone(),
                    input_candidate_count: metadata.input_candidate_count,
                    valid_candidate_count: metadata.valid_candidate_count,
                    selected_candidate_raw_objective: metadata.selected_candidate_raw_objective,
                    selected_candidate_penalized_objective: metadata
                        .selected_candidate_penalized_objective,
                },
                selected_candidate_id: payload.selected_candidate_id.clone(),
                source_window_open_exposure_hours: payload.source_window_open_exposure_hours,
                fitted_parameters: payload.fitted_parameters.clone(),
                candidate_generation_policy: payload.candidate_generation_policy.to_string(),
                objective_weighting_policy: payload.objective_weighting_policy.to_string(),
                objective_metric_bucket_policy: payload.objective_metric_bucket_policy.to_string(),
                activity_input_transport: payload.activity_input_transport,
                activity_selected_hawkes_candidate_id: payload
                    .activity_selected_hawkes_candidate_id
                    .clone(),
                activity_artifact_reference: payload.activity_artifact_reference.clone(),
            }))
        }
        CalibrationStagedRunStageReport::LongVariance(report) => {
            let payload = report.fitted_stage_payload.as_ref().ok_or(
                CalibrationStageParameterArtifactBuildError::ValidationFailed {
                    stage: CalibrationStage::LongVariance,
                    error_message: "successful long-variance stage is missing its payload"
                        .to_string(),
                },
            )?;

            let metadata = report.metadata.as_ref().ok_or(
                CalibrationStageParameterArtifactBuildError::ValidationFailed {
                    stage: CalibrationStage::LongVariance,
                    error_message: "successful long-variance stage is missing metadata".to_string(),
                },
            )?;

            Ok(Box::new(LongVarianceStageParameterArtifactInput {
                metadata: CalibrationStageParameterBuildMetadata {
                    stage_output_schema_version: metadata.stage_output_schema_version.to_string(),
                    run_global_seed: metadata.run_global_seed,
                    stage_seed: metadata.stage_seed,
                    stage_seed_source: metadata.stage_seed_source,
                    stage_seed_derivation_policy: metadata.stage_seed_derivation_policy.to_string(),
                    solver_settings: metadata.solver_settings.clone(),
                },
                fitted_parameters: payload.fitted_parameters,
                diagnostics: payload.diagnostics.clone(),
                return_components_input_transport: payload.return_components_input_transport,
                return_components_selected_candidate_id: payload
                    .return_components_selected_candidate_id
                    .clone(),
                return_components_artifact_reference: payload
                    .return_components_artifact_reference
                    .clone(),
            }))
        }
        CalibrationStagedRunStageReport::NewsShocks(report) => {
            let payload = report.fitted_stage_payload.as_ref().ok_or(
                CalibrationStageParameterArtifactBuildError::ValidationFailed {
                    stage: CalibrationStage::NewsShocks,
                    error_message: "successful news-shocks stage is missing its payload"
                        .to_string(),
                },
            )?;

            let metadata = report.metadata.as_ref().ok_or(
                CalibrationStageParameterArtifactBuildError::ValidationFailed {
                    stage: CalibrationStage::NewsShocks,
                    error_message: "successful news-shocks stage is missing metadata".to_string(),
                },
            )?;

            Ok(Box::new(NewsShocksStageParameterArtifactInput {
                metadata: CalibrationStageParameterBuildMetadata {
                    stage_output_schema_version: metadata.stage_output_schema_version.to_string(),
                    run_global_seed: metadata.run_global_seed,
                    stage_seed: metadata.stage_seed,
                    stage_seed_source: metadata.stage_seed_source,
                    stage_seed_derivation_policy: metadata.stage_seed_derivation_policy.to_string(),
                    solver_settings: metadata.solver_settings.clone(),
                },
                news_process_configuration: payload.news_process_configuration.clone(),
                surprise_deterministic_intensity_profile: payload
                    .surprise_deterministic_intensity_profile
                    .clone(),
                diagnostics: payload.diagnostics.clone(),
                return_components_input_transport: payload.return_components_input_transport,
                return_components_selected_candidate_id: payload
                    .return_components_selected_candidate_id
                    .clone(),
                return_components_artifact_reference: payload
                    .return_components_artifact_reference
                    .clone(),
            }))
        }
        CalibrationStagedRunStageReport::ReturnBackfit(report) => {
            let payload = report.fitted_stage_payload.as_ref().ok_or(
                CalibrationStageParameterArtifactBuildError::ValidationFailed {
                    stage: CalibrationStage::ReturnBackfit,
                    error_message: "successful return-backfit stage is missing its payload"
                        .to_string(),
                },
            )?;

            let metadata = report.metadata.as_ref().ok_or(
                CalibrationStageParameterArtifactBuildError::ValidationFailed {
                    stage: CalibrationStage::ReturnBackfit,
                    error_message: "successful return-backfit stage is missing metadata"
                        .to_string(),
                },
            )?;

            Ok(Box::new(ReturnBackfitStageParameterArtifactInput {
                metadata: CalibrationStageParameterBuildMetadata {
                    stage_output_schema_version: metadata.stage_output_schema_version.to_string(),
                    run_global_seed: metadata.run_global_seed,
                    stage_seed: metadata.stage_seed,
                    stage_seed_source: metadata.stage_seed_source,
                    stage_seed_derivation_policy: metadata.stage_seed_derivation_policy.to_string(),
                    solver_settings: metadata.solver_settings.clone(),
                },
                selected_candidate_id: payload.selected_candidate_id.clone(),
                effective_return_components_selected_candidate_id: payload
                    .effective_return_components_payload
                    .selected_candidate_id
                    .clone(),
                source_window_open_exposure_hours: payload
                    .effective_return_components_payload
                    .source_window_open_exposure_hours,
                effective_fitted_parameters: payload
                    .effective_return_components_payload
                    .fitted_parameters
                    .clone(),
                candidate_generation_policy: payload
                    .effective_return_components_payload
                    .candidate_generation_policy
                    .to_string(),
                objective_weighting_policy: payload
                    .effective_return_components_payload
                    .objective_weighting_policy
                    .to_string(),
                objective_metric_bucket_policy: payload
                    .effective_return_components_payload
                    .objective_metric_bucket_policy
                    .to_string(),
                diagnostics: payload.diagnostics.clone(),
                return_components_input_transport: payload.return_components_input_transport,
                return_components_selected_candidate_id: payload
                    .return_components_selected_candidate_id
                    .clone(),
                return_components_artifact_reference: payload
                    .return_components_artifact_reference
                    .clone(),
            }))
        }
        CalibrationStagedRunStageReport::JointRefinement(report) => Ok(Box::new(
            joint_refinement_like_stage_parameter_artifact_input(
                CalibrationStage::JointRefinement,
                report,
            )?,
        )),
        CalibrationStagedRunStageReport::OptionRefinement(report) => Ok(Box::new(
            joint_refinement_like_stage_parameter_artifact_input(
                CalibrationStage::OptionRefinement,
                report,
            )?,
        )),
        CalibrationStagedRunStageReport::Finalization(report) => Ok(Box::new(
            joint_refinement_like_stage_parameter_artifact_input(
                CalibrationStage::Finalization,
                report,
            )?,
        )),
        CalibrationStagedRunStageReport::MeanReversion(report) => {
            let payload = report.fitted_stage_payload.as_ref().ok_or(
                CalibrationStageParameterArtifactBuildError::ValidationFailed {
                    stage: CalibrationStage::MeanReversion,
                    error_message: "successful mean-reversion stage is missing its payload"
                        .to_string(),
                },
            )?;

            let metadata = report.metadata.as_ref().ok_or(
                CalibrationStageParameterArtifactBuildError::ValidationFailed {
                    stage: CalibrationStage::MeanReversion,
                    error_message: "successful mean-reversion stage is missing metadata"
                        .to_string(),
                },
            )?;

            Ok(Box::new(MeanReversionStageParameterArtifactInput {
                metadata: CalibrationStageParameterBuildMetadata {
                    stage_output_schema_version: metadata.stage_output_schema_version.to_string(),
                    run_global_seed: metadata.run_global_seed,
                    stage_seed: metadata.stage_seed,
                    stage_seed_source: metadata.stage_seed_source,
                    stage_seed_derivation_policy: metadata.stage_seed_derivation_policy.to_string(),
                    solver_settings: metadata.solver_settings.clone(),
                },
                fitted_parameters: payload.fitted_parameters,
                diagnostics: payload.diagnostics.clone(),
                return_components_input_transport: payload.return_components_input_transport,
                return_components_selected_candidate_id: payload
                    .return_components_selected_candidate_id
                    .clone(),
                return_components_artifact_reference: payload
                    .return_components_artifact_reference
                    .clone(),
            }))
        }
    }
}

fn joint_refinement_like_stage_parameter_artifact_input(
    stage: CalibrationStage,
    report: &JointRefinementStageExecutionReport,
) -> Result<JointRefinementStageParameterArtifactInput, CalibrationStageParameterArtifactBuildError>
{
    let payload = report.fitted_stage_payload.as_ref().ok_or(
        CalibrationStageParameterArtifactBuildError::ValidationFailed {
            stage,
            error_message: format!(
                "successful {} stage is missing its payload",
                calibration_stage_token(stage)
            ),
        },
    )?;

    let metadata = report.metadata.as_ref().ok_or(
        CalibrationStageParameterArtifactBuildError::ValidationFailed {
            stage,
            error_message: format!(
                "successful {} stage is missing metadata",
                calibration_stage_token(stage)
            ),
        },
    )?;

    Ok(JointRefinementStageParameterArtifactInput {
        stage,
        metadata: CalibrationStageParameterBuildMetadata {
            stage_output_schema_version: metadata.stage_output_schema_version.to_string(),
            run_global_seed: metadata.run_global_seed,
            stage_seed: metadata.stage_seed,
            stage_seed_source: metadata.stage_seed_source,
            stage_seed_derivation_policy: metadata.stage_seed_derivation_policy.to_string(),
            solver_settings: metadata.solver_settings.clone(),
        },
        optimizer_state: CalibrationStageParameterOptimizerStateInput {
            backend_id: metadata.optimizer_adapter_metadata.backend_id.clone(),
            random_seed: metadata.optimizer_adapter_metadata.random_seed,
            selection_policy: metadata
                .optimizer_adapter_metadata
                .selection_policy
                .to_string(),
            penalty_policy: metadata
                .optimizer_adapter_metadata
                .penalty_policy
                .to_string(),
            bounds_policy: metadata
                .optimizer_adapter_metadata
                .bounds_policy
                .to_string(),
            solver_settings: metadata.optimizer_adapter_metadata.solver_settings.clone(),
            input_candidate_count: metadata.input_candidate_count,
            valid_candidate_count: metadata.valid_candidate_count,
            selected_candidate_raw_objective: metadata.selected_candidate_raw_objective,
            selected_candidate_penalized_objective: metadata.selected_candidate_penalized_objective,
        },
        selected_candidate_id: payload.selected_candidate_id.clone(),
        fitted_parameters: payload.fitted_parameters.clone(),
        baseline_hawkes_fitted_parameters: payload.baseline_hawkes_fitted_parameters.clone(),
        baseline_return_components_fitted_parameters: payload
            .baseline_return_components_fitted_parameters
            .clone(),
        objective_breakdown: payload.objective_breakdown.clone(),
        optimizer_diagnostics: payload.optimizer_diagnostics.clone(),
    })
}

pub(crate) fn build_run_contract_with_mode_and_resume(
    run_id: &str,
    execution_mode: CalibrationExecutionMode,
    resume_settings: Option<CalibrationResumeSettings>,
) -> CalibrationRunContract {
    build_run_contract_with_stages_and_resume(
        run_id,
        execution_mode,
        vec![
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            CalibrationStage::ReturnComponents,
            CalibrationStage::LongVariance,
            CalibrationStage::NewsShocks,
            CalibrationStage::ReturnBackfit,
            CalibrationStage::MeanReversion,
            CalibrationStage::JointRefinement,
            CalibrationStage::Finalization,
        ],
        build_solver_settings_for_all_stages(),
        resume_settings,
    )
}

pub(crate) fn build_pre_joint_run_contract_with_mode_and_resume(
    run_id: &str,
    execution_mode: CalibrationExecutionMode,
    resume_settings: Option<CalibrationResumeSettings>,
) -> CalibrationRunContract {
    build_run_contract_with_stages_and_resume(
        run_id,
        execution_mode,
        vec![
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            CalibrationStage::ReturnComponents,
        ],
        build_solver_settings_for_pre_joint_pipeline(),
        resume_settings,
    )
}

pub(crate) fn build_through_mean_reversion_run_contract_with_mode_and_resume(
    run_id: &str,
    execution_mode: CalibrationExecutionMode,
    resume_settings: Option<CalibrationResumeSettings>,
) -> CalibrationRunContract {
    build_run_contract_with_stages_and_resume(
        run_id,
        execution_mode,
        vec![
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            CalibrationStage::ReturnComponents,
            CalibrationStage::LongVariance,
            CalibrationStage::NewsShocks,
            CalibrationStage::ReturnBackfit,
            CalibrationStage::MeanReversion,
        ],
        build_solver_settings_for_through_mean_reversion_pipeline(),
        resume_settings,
    )
}

fn build_run_contract_with_stages_and_resume(
    run_id: &str,
    execution_mode: CalibrationExecutionMode,
    selected_stages: Vec<CalibrationStage>,
    solver_settings_by_stage: Vec<CalibrationStageSolverSettings>,
    resume_settings: Option<CalibrationResumeSettings>,
) -> CalibrationRunContract {
    configure_joint_refinement_test_backend();

    let run_spec = {
        let builder = CalibrationRunSpec::builder(
            run_id,
            build_data_inputs(),
            execution_mode,
            selected_stages,
            build_seed_settings(),
            solver_settings_by_stage,
        );

        let builder = if let Some(resume_settings) = resume_settings {
            builder.with_resume_settings(resume_settings)
        } else {
            builder
        };

        builder.build().expect("run spec should validate")
    };

    CalibrationRunContract::try_new(
        run_spec.clone(),
        stage_graph_for_selected_stages(run_spec.selected_stages.as_slice()),
    )
    .expect("run contract should validate")
}

pub(crate) fn build_run_contract_for_caller_surface(
    run_id: &str,
    caller_surface: CalibrationCallerSurface,
    execution_mode_override: Option<CalibrationExecutionMode>,
) -> CalibrationRunContract {
    build_run_contract_for_caller_surface_with_stages(
        run_id,
        caller_surface,
        execution_mode_override,
        vec![
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            CalibrationStage::ReturnComponents,
            CalibrationStage::LongVariance,
            CalibrationStage::NewsShocks,
            CalibrationStage::ReturnBackfit,
            CalibrationStage::MeanReversion,
            CalibrationStage::JointRefinement,
            CalibrationStage::Finalization,
        ],
        build_solver_settings_for_all_stages(),
    )
}

pub(crate) fn build_pre_joint_run_contract_for_caller_surface(
    run_id: &str,
    caller_surface: CalibrationCallerSurface,
    execution_mode_override: Option<CalibrationExecutionMode>,
) -> CalibrationRunContract {
    build_run_contract_for_caller_surface_with_stages(
        run_id,
        caller_surface,
        execution_mode_override,
        vec![
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            CalibrationStage::ReturnComponents,
        ],
        build_solver_settings_for_pre_joint_pipeline(),
    )
}

pub(crate) fn build_through_mean_reversion_run_contract_for_caller_surface(
    run_id: &str,
    caller_surface: CalibrationCallerSurface,
    execution_mode_override: Option<CalibrationExecutionMode>,
) -> CalibrationRunContract {
    build_run_contract_for_caller_surface_with_stages(
        run_id,
        caller_surface,
        execution_mode_override,
        vec![
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            CalibrationStage::ReturnComponents,
            CalibrationStage::LongVariance,
            CalibrationStage::MeanReversion,
        ],
        build_solver_settings_for_through_mean_reversion_pipeline(),
    )
}

fn build_run_contract_for_caller_surface_with_stages(
    run_id: &str,
    caller_surface: CalibrationCallerSurface,
    execution_mode_override: Option<CalibrationExecutionMode>,
    selected_stages: Vec<CalibrationStage>,
    solver_settings_by_stage: Vec<CalibrationStageSolverSettings>,
) -> CalibrationRunContract {
    configure_joint_refinement_test_backend();

    let execution_mode =
        resolve_calibration_execution_mode_for_surface(caller_surface, execution_mode_override);

    let run_spec = CalibrationRunSpec::try_new(
        run_id,
        build_data_inputs(),
        execution_mode,
        selected_stages,
        build_seed_settings(),
        solver_settings_by_stage,
    )
    .expect("caller-surface run spec should validate");

    CalibrationRunContract::try_new(
        run_spec.clone(),
        stage_graph_for_selected_stages(run_spec.selected_stages.as_slice()),
    )
    .expect("caller-surface run contract should validate")
}

fn configure_joint_refinement_test_backend() {
    // SAFETY: Test helpers in this module force a deterministic backend policy for the current
    // process before invoking the staged runner. Tests that exercise the process-worker backend
    // scope that override around the backend call itself.
    unsafe {
        std::env::set_var(
            JOINT_REFINEMENT_SUMMARY_SIMULATION_BACKEND_ENV,
            JOINT_REFINEMENT_SUMMARY_SIMULATION_BACKEND_IN_PROCESS,
        );
    }

    if std::env::var_os(JOINT_REFINEMENT_CANDIDATE_POLICY_ENV).is_none() {
        // SAFETY: Test helpers in this module set a single deterministic candidate policy for the
        // current process before invoking the staged runner. All callers use the same value.
        unsafe {
            std::env::set_var(
                JOINT_REFINEMENT_CANDIDATE_POLICY_ENV,
                JOINT_REFINEMENT_CANDIDATE_POLICY_CPU_COUNT_LIMITED,
            );
        }
    }
}

fn configure_joint_refinement_worker_environment() {
    configure_joint_refinement_test_backend();

    if std::env::var_os("JANIFORM_JOINT_REFINEMENT_WORKER_EXECUTABLE").is_some() {
        return;
    }

    // SAFETY: Test helpers in this module set a single deterministic worker path for the
    // current process before invoking the staged runner. All callers use the same value.
    unsafe {
        std::env::set_var(
            "JANIFORM_JOINT_REFINEMENT_WORKER_EXECUTABLE",
            ensure_joint_refinement_worker_executable(),
        );
    }
}

pub(crate) fn default_stage_graph() -> CalibrationStageGraph {
    stage_graph_for_selected_stages(&[
        CalibrationStage::Seasonality,
        CalibrationStage::Hawkes,
        CalibrationStage::ReturnComponents,
        CalibrationStage::LongVariance,
        CalibrationStage::NewsShocks,
        CalibrationStage::ReturnBackfit,
        CalibrationStage::MeanReversion,
        CalibrationStage::JointRefinement,
        CalibrationStage::Finalization,
    ])
}

fn build_solver_settings_for_through_mean_reversion_pipeline() -> Vec<CalibrationStageSolverSettings>
{
    vec![
        CalibrationStageSolverSettings {
            stage: CalibrationStage::Seasonality,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::Hawkes,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::ReturnComponents,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::LongVariance,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::NewsShocks,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::ReturnBackfit,
            optimizer_settings: build_solver_settings(),
        },
        CalibrationStageSolverSettings {
            stage: CalibrationStage::MeanReversion,
            optimizer_settings: build_solver_settings(),
        },
    ]
}

pub(crate) fn stage_graph_for_selected_stages(
    selected_stages: &[CalibrationStage],
) -> CalibrationStageGraph {
    let selected_stage_set: BTreeSet<CalibrationStage> = selected_stages.iter().copied().collect();

    CalibrationStageGraph::new(
        vec![
            CalibrationStageDependency {
                stage: CalibrationStage::Hawkes,
                depends_on: CalibrationStage::Seasonality,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::ReturnComponents,
                depends_on: CalibrationStage::Hawkes,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::LongVariance,
                depends_on: CalibrationStage::ReturnComponents,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::NewsShocks,
                depends_on: CalibrationStage::ReturnComponents,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::ReturnBackfit,
                depends_on: CalibrationStage::LongVariance,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::ReturnBackfit,
                depends_on: CalibrationStage::NewsShocks,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::MeanReversion,
                depends_on: CalibrationStage::ReturnBackfit,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::JointRefinement,
                depends_on: CalibrationStage::Hawkes,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::JointRefinement,
                depends_on: CalibrationStage::ReturnComponents,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::JointRefinement,
                depends_on: CalibrationStage::LongVariance,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::JointRefinement,
                depends_on: CalibrationStage::NewsShocks,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::JointRefinement,
                depends_on: CalibrationStage::ReturnBackfit,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::JointRefinement,
                depends_on: CalibrationStage::MeanReversion,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::OptionRefinement,
                depends_on: CalibrationStage::JointRefinement,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::Finalization,
                depends_on: CalibrationStage::JointRefinement,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::Finalization,
                depends_on: CalibrationStage::OptionRefinement,
            },
        ]
        .into_iter()
        .filter(|dependency| {
            selected_stage_set.contains(&dependency.stage)
                && selected_stage_set.contains(&dependency.depends_on)
        })
        .collect(),
    )
}

pub(crate) fn build_mode_aware_stage_handoff(
    from_stage: CalibrationStage,
    to_stage: CalibrationStage,
    execution_mode: CalibrationExecutionMode,
    persisted_artifact_reference: &str,
) -> CalibrationStageHandoff {
    match execution_mode {
        CalibrationExecutionMode::Memory => {
            CalibrationStageHandoff::in_memory_output(from_stage, to_stage)
        }
        CalibrationExecutionMode::Disk => CalibrationStageHandoff::persisted_artifact_reference(
            from_stage,
            to_stage,
            persisted_artifact_reference,
        )
        .expect("disk-mode persisted handoff fixture should be valid"),
    }
}

pub(crate) fn staged_run_inputs(
    include_seasonality_input: bool,
    include_hawkes_candidates: bool,
    include_return_candidate_generation_input: bool,
) -> CalibrationStagedRunInputs {
    CalibrationStagedRunInputs {
        seasonality_stage_input: if include_seasonality_input {
            Some(
                SeasonalityStageExecutionInput::from_timestamp_event_records(
                    calibration_stage_timestamp_fixture(),
                ),
            )
        } else {
            None
        },
        hawkes_candidate_inputs: if include_hawkes_candidates {
            Some(hawkes_stage_candidate_fixture())
        } else {
            None
        },
        hawkes_event_side_codes: if include_hawkes_candidates
            || include_return_candidate_generation_input
        {
            Some(hawkes_event_side_codes_fixture())
        } else {
            None
        },
        return_components_candidate_generation_input: if include_return_candidate_generation_input {
            Some(return_components_stage_candidate_generation_fixture())
        } else {
            None
        },
        coarse_data_records: None,
        tick_objective_data_usage_policy:
            CalibrationTickObjectiveDataUsagePolicy::ShortHorizonWindow,
        mean_reversion_stage_input: if include_return_candidate_generation_input {
            Some(mean_reversion_stage_execution_fixture())
        } else {
            None
        },
        joint_refinement_calibration_effort: None,
        joint_refinement_candidate_budget_override: Some(1),
        treat_invalid_candidate_fallback_warning_as_error: false,
        option_refinement_candidate_count_override: Some(1),
        option_refinement_wall_clock_budget_millis: None,
        audit_option_pricing_enabled: false,
        diagnostics: Default::default(),
        option_refinement_pricing_input: None,
    }
}

pub(crate) fn joint_refinement_stage_option_pricing_input_fixture()
-> JointRefinementStageOptionPricingInput {
    let valuation_time_utc_nanos = 0;
    let expiry_timestamp_utc_nanos = NANOS_PER_HOUR;

    JointRefinementStageOptionPricingInput {
        valuation_time_utc_nanos,
        pricing_interval_schedule_policy: PricingIntervalSchedulePolicy::AutoExactSparse,
        carry_input: janiform_calibration_artifacts::JointRefinementStageCarryInput {
            discount_rate: 0.0,
            carry_rate: 0.0,
        },
        option_valuation_spot: 100.0,
        current_state_estimation_input: JointRefinementStageCurrentStateEstimationInput {
            state_time_utc_nanos: valuation_time_utc_nanos,
            reference_price: 1.0,
            observed_spot: 100.0,
            observed_log_return: 100.0_f64.ln(),
            observation_timestamp_utc_nanos: vec![valuation_time_utc_nanos],
            observation_trade_prices: vec![100.0],
            observation_trade_sides: vec![HawkesEventSide::Buy],
        },
        option_snapshot_nodes: vec![JointRefinementStageOptionSnapshotNode {
            quote_timestamp_utc_nanos: valuation_time_utc_nanos,
            expiry_timestamp_utc_nanos,
            strike: 100.0,
            payoff_type: VanillaOptionPayoffType::Call,
            bid_price: 1.0,
            ask_price: 1.2,
            observed_mid_price: 1.1,
        }],
    }
}

pub(crate) fn mean_reversion_stage_execution_fixture() -> MeanReversionStageExecutionInput {
    MeanReversionStageExecutionInput {
        return_components_input: None,
        seasonality_stage_input: None,
        hawkes_stage_payload: None,
        return_components_stage_payload: None,
        grid_candidates: vec![MeanReversionStageGridCandidate {
            calibration_grid_kind: "synthetic_daily_close_grid.v1".to_string(),
            calibration_grid_interval_hours: 24.0,
            source_observation_count: 4,
            coverage_ratio: 1.0,
            observations: vec![
                MeanReversionStageObservation {
                    observation_timestamp_utc_nanos: 1_770_042_600_000_000_000,
                    observed_log_price: 4.60,
                },
                MeanReversionStageObservation {
                    observation_timestamp_utc_nanos: 1_770_129_000_000_000_000,
                    observed_log_price: 4.58,
                },
                MeanReversionStageObservation {
                    observation_timestamp_utc_nanos: 1_770_215_400_000_000_000,
                    observed_log_price: 4.57,
                },
                MeanReversionStageObservation {
                    observation_timestamp_utc_nanos: 1_770_301_800_000_000_000,
                    observed_log_price: 4.59,
                },
            ],
        }],
    }
}

pub(crate) fn temporary_checkpoint_directory(test_name: &str) -> PathBuf {
    loop {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after UNIX epoch")
            .as_nanos();
        let checkpoint_directory =
            std::env::temp_dir().join(format!("janiform-checkpoints-{test_name}-{unique_suffix}"));

        match fs::create_dir(checkpoint_directory.as_path()) {
            Ok(()) => return checkpoint_directory,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                std::thread::yield_now();
            }
            Err(error) => panic!("temporary checkpoint directory should be created: {error}"),
        }
    }
}

pub(crate) fn ensure_joint_refinement_worker_executable() -> PathBuf {
    static WORKER_EXECUTABLE: LazyLock<PathBuf> = LazyLock::new(|| {
        let manifest_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = manifest_directory
            .parent()
            .and_then(|path| path.parent())
            .expect("workspace root should be resolvable from the crate manifest directory");

        let current_executable =
            std::env::current_exe().expect("current test executable path should resolve");

        let profile_directory = current_executable
            .parent()
            .and_then(|path| path.parent())
            .expect("cargo profile directory should resolve from the current test executable");

        let worker_executable = profile_directory.join(if cfg!(windows) {
            "janiform-joint-refinement-worker.exe"
        } else {
            "janiform-joint-refinement-worker"
        });

        if joint_refinement_worker_executable_requires_rebuild(
            worker_executable.as_path(),
            workspace_root,
        ) {
            let cargo_path = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
            let build_status = Command::new(cargo_path)
                .current_dir(workspace_root)
                .args(["build", "-p", "janiform-joint-refinement-worker"])
                .status()
                .expect("worker crate build should launch successfully");

            assert!(
                build_status.success(),
                "worker crate build failed with status {build_status}",
            );
        }

        assert!(
            worker_executable.is_file(),
            "worker executable should exist at {}",
            worker_executable.display(),
        );

        worker_executable
    });

    WORKER_EXECUTABLE.clone()
}

fn joint_refinement_worker_executable_requires_rebuild(
    worker_executable: &Path,
    workspace_root: &Path,
) -> bool {
    if !worker_executable.is_file() {
        return true;
    }

    let executable_modified =
        match fs::metadata(worker_executable).and_then(|metadata| metadata.modified()) {
            Ok(modified) => modified,
            Err(_) => return true,
        };

    latest_worker_source_modified_at(workspace_root)
        .is_none_or(|source_modified| source_modified > executable_modified)
}

fn latest_worker_source_modified_at(workspace_root: &Path) -> Option<SystemTime> {
    [
        workspace_root.join("Cargo.toml"),
        workspace_root.join("Cargo.lock"),
        workspace_root.join("crates/janiform-joint-refinement-worker/Cargo.toml"),
        workspace_root.join("crates/janiform-joint-refinement-worker/src"),
        workspace_root.join("crates/janiform-calibration-artifacts/Cargo.toml"),
        workspace_root.join("crates/janiform-calibration-artifacts/src"),
        workspace_root.join("crates/janiform-calibration-runtime/Cargo.toml"),
        workspace_root.join("crates/janiform-calibration-runtime/src/stages/joint_refinement"),
        workspace_root.join("crates/janiform-engine-simulation/Cargo.toml"),
        workspace_root.join("crates/janiform-engine-simulation/src"),
        workspace_root.join("crates/janiform-model-kernels/Cargo.toml"),
        workspace_root.join("crates/janiform-model-kernels/src"),
    ]
    .into_iter()
    .filter_map(|path| latest_path_modified_at(path.as_path()))
    .max()
}

fn latest_path_modified_at(path: &Path) -> Option<SystemTime> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.is_file() {
        return metadata.modified().ok();
    }
    if !metadata.is_dir() {
        return None;
    }

    let mut latest_modified = metadata.modified().ok();
    let entries = fs::read_dir(path).ok()?;
    for entry in entries.flatten() {
        if let Some(entry_modified) = latest_path_modified_at(entry.path().as_path()) {
            latest_modified = Some(latest_modified.map_or(entry_modified, |current_latest| {
                current_latest.max(entry_modified)
            }));
        }
    }

    latest_modified
}

pub(crate) fn build_seasonality_stage_run_spec(
    execution_mode: CalibrationExecutionMode,
) -> CalibrationRunSpec {
    let stage_handoff = build_mode_aware_stage_handoff(
        CalibrationStage::Seasonality,
        CalibrationStage::Hawkes,
        execution_mode,
        "artifact://seasonality/run-001",
    );

    CalibrationRunSpec::builder(
        "run-001",
        build_data_inputs(),
        execution_mode,
        vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
        build_seed_settings(),
        build_solver_settings_for_seasonality_and_hawkes(),
    )
    .with_stage_handoffs(vec![stage_handoff])
    .build()
    .expect("seasonality stage run spec should validate")
}

pub(crate) fn build_hawkes_stage_run_spec(
    execution_mode: CalibrationExecutionMode,
) -> CalibrationRunSpec {
    let seasonality_handoff = build_mode_aware_stage_handoff(
        CalibrationStage::Seasonality,
        CalibrationStage::Hawkes,
        execution_mode,
        "artifact://seasonality/run-001",
    );

    let hawkes_handoff = build_mode_aware_stage_handoff(
        CalibrationStage::Hawkes,
        CalibrationStage::ReturnComponents,
        execution_mode,
        "artifact://hawkes/run-001",
    );

    CalibrationRunSpec::builder(
        "run-001",
        build_data_inputs(),
        execution_mode,
        vec![
            CalibrationStage::Seasonality,
            CalibrationStage::Hawkes,
            CalibrationStage::ReturnComponents,
        ],
        build_seed_settings(),
        build_solver_settings_for_pre_joint_pipeline(),
    )
    .with_stage_handoffs(vec![seasonality_handoff, hawkes_handoff])
    .build()
    .expect("hawkes stage run spec should validate")
}

pub(crate) fn build_return_components_stage_pipeline_run_spec(
    execution_mode: CalibrationExecutionMode,
) -> CalibrationRunSpec {
    build_hawkes_stage_run_spec(execution_mode)
}

pub(crate) fn build_return_components_stage_standalone_run_spec(
    execution_mode: CalibrationExecutionMode,
) -> CalibrationRunSpec {
    let seed_settings = CalibrationRunSeedSettings::try_new(
        23,
        vec![CalibrationStageSeedOverride {
            stage: CalibrationStage::ReturnComponents,
            seed: 303,
        }],
    )
    .expect("standalone return-components seed settings should validate");

    CalibrationRunSpec::try_new(
        "run-return-components-standalone",
        build_data_inputs(),
        execution_mode,
        vec![CalibrationStage::ReturnComponents],
        seed_settings,
        vec![CalibrationStageSolverSettings {
            stage: CalibrationStage::ReturnComponents,
            optimizer_settings: build_solver_settings(),
        }],
    )
    .expect("standalone return-components run spec should validate")
}

pub(crate) fn build_seasonality_stage_success_payload() -> SeasonalityStageNormalizedPayload {
    static STAGE_SUCCESS_PAYLOAD: LazyLock<SeasonalityStageNormalizedPayload> =
        LazyLock::new(|| {
            let calendar_registry = calibration_stage_test_registry();
            let report = run_seasonality_calibration_stage(
                &build_seasonality_stage_run_spec(CalibrationExecutionMode::Memory),
                &calendar_registry,
                SeasonalityStageExecutionInput::from_timestamp_event_records(
                    calibration_stage_timestamp_fixture(),
                ),
            );

            report
                .normalized_stage_payload
                .expect("seasonality stage fixture should produce normalized payload")
        });

    STAGE_SUCCESS_PAYLOAD.clone()
}

pub(crate) fn build_hawkes_stage_success_payload() -> HawkesStageFittedPayload {
    static STAGE_SUCCESS_PAYLOAD: LazyLock<HawkesStageFittedPayload> = LazyLock::new(|| {
        let seasonality_payload = build_seasonality_stage_success_payload();
        let calendar_registry = calibration_stage_test_registry();
        let report = run_hawkes_calibration_stage(
            &build_hawkes_stage_run_spec(CalibrationExecutionMode::Memory),
            &calendar_registry,
            HawkesStageExecutionInput::from_in_memory_seasonality_output(
                seasonality_payload,
                hawkes_stage_candidate_fixture(),
            ),
        );

        report
            .fitted_stage_payload
            .expect("hawkes stage fixture should produce fitted payload")
    });

    STAGE_SUCCESS_PAYLOAD.clone()
}

pub(crate) fn build_return_components_stage_success_payload() -> ReturnComponentsStageFittedPayload
{
    static STAGE_SUCCESS_PAYLOAD: LazyLock<ReturnComponentsStageFittedPayload> =
        LazyLock::new(|| {
            let calendar_registry = calibration_stage_test_registry();
            let report = run_return_components_calibration_stage(
                &build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Memory),
                &calendar_registry,
                ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
                    build_hawkes_stage_success_payload(),
                    None,
                    return_components_stage_candidate_generation_fixture(),
                ),
            );

            report
                .fitted_stage_payload
                .expect("return-components stage fixture should produce fitted payload")
        });

    STAGE_SUCCESS_PAYLOAD.clone()
}

pub(crate) fn build_mean_reversion_stage_success_payload() -> MeanReversionStageFittedPayload {
    build_mean_reversion_stage_success_report()
        .fitted_stage_payload
        .expect("mean-reversion stage fixture should produce fitted payload")
}

pub(crate) fn build_mean_reversion_stage_success_report() -> MeanReversionStageExecutionReport {
    static STAGE_SUCCESS_REPORT: LazyLock<MeanReversionStageExecutionReport> =
        LazyLock::new(|| {
            let calendar_registry = calibration_stage_test_registry();

            let return_components_payload = build_return_components_stage_success_payload();

            let mut execution_input = mean_reversion_stage_execution_fixture();

            execution_input.seasonality_stage_input = Some(
                MeanReversionStageSeasonalityInput::from_seasonality_stage_payload(
                    &build_seasonality_stage_success_payload(),
                ),
            );
            execution_input.hawkes_stage_payload = Some(build_hawkes_stage_success_payload());
            execution_input.return_components_stage_payload =
                Some(return_components_payload.clone());
            execution_input.return_components_input =
                Some(CalibrationStageSelectedCandidateSource::InMemoryOutput {
                    selected_candidate_id: return_components_payload.selected_candidate_id.clone(),
                });

            run_mean_reversion_calibration_stage(
                &build_through_mean_reversion_run_contract_with_mode_and_resume(
                    "run-mean-reversion-stage-success",
                    CalibrationExecutionMode::Memory,
                    None,
                )
                .run_spec,
                &calendar_registry,
                execution_input,
            )
        });

    STAGE_SUCCESS_REPORT.clone()
}

pub(crate) fn build_joint_refinement_stage_success_payload() -> JointRefinementStageFittedPayload {
    build_joint_refinement_stage_success_report()
        .fitted_stage_payload
        .expect("joint-refinement stage fixture should produce fitted payload")
}

pub(crate) fn build_joint_refinement_stage_success_report() -> JointRefinementStageExecutionReport {
    static STAGE_SUCCESS_REPORT: LazyLock<JointRefinementStageExecutionReport> =
        LazyLock::new(|| {
            configure_joint_refinement_test_backend();

            let calendar_registry = calibration_stage_test_registry();
            let seasonality_payload = build_seasonality_stage_success_payload();
            run_joint_refinement_calibration_stage(
                &build_run_contract_with_mode_and_resume(
                    "run-joint-stage-success",
                    CalibrationExecutionMode::Memory,
                    None,
                )
                .run_spec,
                &calendar_registry,
                JointRefinementStageExecutionInput {
                    seasonality_factors_input: JointRefinementStageSeasonalityFactorsInput {
                        exchange_code: seasonality_payload.exchange_code,
                        clock_model: seasonality_payload.clock_model,
                    },
                    hawkes_input: CalibrationStagePayloadSource::InMemoryOutput {
                        payload: build_hawkes_stage_success_payload(),
                    },
                    return_components_input: CalibrationStagePayloadSource::InMemoryOutput {
                        payload: build_return_components_stage_success_payload(),
                    },
                    mean_reversion_input: CalibrationStagePayloadSource::InMemoryOutput {
                        payload: build_mean_reversion_stage_success_payload(),
                    },
                    candidate_generation_input:
                        return_components_stage_candidate_generation_fixture(),
                    coarse_data_records: None,
                    tick_objective_data_usage_policy:
                        CalibrationTickObjectiveDataUsagePolicy::ShortHorizonWindow,
                    calibration_effort: None,
                    candidate_budget_override: Some(1),
                    treat_invalid_candidate_fallback_warning_as_error: false,
                    option_refinement_candidate_count_override: Some(1),
                    option_refinement_wall_clock_budget_millis: None,
                    audit_option_pricing_enabled: false,
                    diagnostics: Default::default(),
                    option_pricing_input: None,
                },
                CancellationToken::uncancelled(),
            )
        });

    STAGE_SUCCESS_REPORT.clone()
}

pub(crate) fn hawkes_stage_candidate_fixture() -> Vec<HawkesStageCandidateInput> {
    let source_regime = hawkes_source_regime_fixture();
    let directional_label_diagnostics = hawkes_directional_label_diagnostics_fixture();

    vec![
        HawkesStageCandidateInput {
            candidate_id: "candidate_high_loss".to_string(),
            lambda_bar: 7.20,
            beta: 0.95,
            structured_alpha: HawkesStructuredAlphaParameterization::new(0.28, 0.10, 0.08, 0.06),
            structured_rho: HawkesStructuredRhoParameterization::new(0.14, 0.04, 0.03, 0.02),
            self_dominance_policy: Some(HawkesStructuredRhoSelfDominancePolicy::new(1.0)),
            stability_numeric_tolerance: 0.0,
            source_regime: source_regime.clone(),
            directional_label_diagnostics: directional_label_diagnostics.clone(),
        },
        HawkesStageCandidateInput {
            candidate_id: "candidate_stable_low_loss".to_string(),
            lambda_bar: 9.40,
            beta: 20.0,
            structured_alpha: HawkesStructuredAlphaParameterization::new(
                0.01, 0.002, 0.001, 0.0005,
            ),
            structured_rho: HawkesStructuredRhoParameterization::new(0.08, 0.02, 0.01, 0.01),
            self_dominance_policy: Some(HawkesStructuredRhoSelfDominancePolicy::new(1.0)),
            stability_numeric_tolerance: 0.0,
            source_regime,
            directional_label_diagnostics,
        },
    ]
}

pub(crate) fn hawkes_event_side_codes_fixture() -> Vec<u8> {
    vec![
        HAWKES_EVENT_SIDE_CODE_BUY,
        HAWKES_EVENT_SIDE_CODE_SELL,
        HAWKES_EVENT_SIDE_CODE_BUY,
        HAWKES_EVENT_SIDE_CODE_SELL,
        HAWKES_EVENT_SIDE_CODE_BUY,
        HAWKES_EVENT_SIDE_CODE_SELL,
    ]
}

pub(crate) fn hawkes_stage_constraint_violating_candidate_fixture() -> HawkesStageCandidateInput {
    HawkesStageCandidateInput {
        candidate_id: "candidate_invalid_constraints".to_string(),
        lambda_bar: -0.2,
        beta: 0.0,
        structured_alpha: HawkesStructuredAlphaParameterization::new(-0.1, -0.1, -0.1, -0.1),
        structured_rho: HawkesStructuredRhoParameterization::new(-0.2, -0.1, -0.1, -0.1),
        self_dominance_policy: Some(HawkesStructuredRhoSelfDominancePolicy::new(0.9)),
        stability_numeric_tolerance: -0.5,
        source_regime: hawkes_source_regime_fixture(),
        directional_label_diagnostics: hawkes_directional_label_diagnostics_fixture(),
    }
}

pub(crate) fn hawkes_source_regime_fixture() -> HawkesStageObservedSourceRegime {
    HawkesStageObservedSourceRegime {
        observation_duration_hours: 12.0,
        open_session_duration_hours: 8.0,
        total_tick_count: 95,
        open_session_tick_count: 84,
        closed_session_tick_count: 11,
        open_session_buy_tick_count: 39,
        open_session_sell_tick_count: 45,
        open_session_mean_interarrival_seconds: 34.0,
        open_session_interarrival_coefficient_of_variation: 1.18,
        open_session_short_gap_threshold_seconds: 17.0,
        open_session_short_gap_ratio: 0.43,
        known_news_open_event_count: 2,
        open_session_known_news_activity_share: 0.06,
    }
}

pub(crate) fn hawkes_directional_label_diagnostics_fixture()
-> HawkesStageDirectionalLabelDiagnostics {
    HawkesStageDirectionalLabelDiagnostics {
        policy_id: "tick_price_change_with_flat_move_carryover.v1".to_string(),
        first_inferred_trade_sign: "buy".to_string(),
        provided_label_count: 10,
        inferred_label_count: 74,
        flat_move_carry_count: 9,
        price_up_inference_count: 35,
        price_down_inference_count: 30,
    }
}

pub(crate) fn return_components_stage_candidate_generation_fixture()
-> ReturnComponentsStageCandidateGenerationInput {
    let source_window = ReturnComponentsStageSignalSourceWindow {
        window_start_utc_nanos: utc_nanos(2026, 2, 2, 13, 0, 0),
        window_end_utc_nanos: utc_nanos(2026, 2, 5, 0, 0, 0),
    };

    ReturnComponentsStageCandidateGenerationInput {
        known_news_calendar: ReturnComponentsKnownNewsCalendarInput {
            source_window,
            scheduled_event_timestamps_utc_nanos: vec![
                utc_nanos(2026, 2, 2, 14, 30, 0),
                utc_nanos(2026, 2, 3, 15, 0, 0),
                utc_nanos(2026, 2, 4, 16, 0, 0),
            ],
        },
        realized_known_news_return_samples: vec![
            ReturnComponentsRealizedKnownNewsReturnSample {
                event_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 14, 30, 0),
                attributed_open_return_timestamp_utc_nanos: Some(utc_nanos(2026, 2, 2, 15, 0, 0)),
                open_window_jump_return: 0.016,
                closed_window_gap_return: -0.009,
                post_shock_activity_lift: 1.20,
            },
            ReturnComponentsRealizedKnownNewsReturnSample {
                event_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 15, 0, 0),
                attributed_open_return_timestamp_utc_nanos: Some(utc_nanos(2026, 2, 3, 18, 20, 0)),
                open_window_jump_return: -0.012,
                closed_window_gap_return: 0.004,
                post_shock_activity_lift: 1.05,
            },
            ReturnComponentsRealizedKnownNewsReturnSample {
                event_timestamp_utc_nanos: utc_nanos(2026, 2, 4, 16, 0, 0),
                attributed_open_return_timestamp_utc_nanos: Some(utc_nanos(2026, 2, 4, 19, 15, 0)),
                open_window_jump_return: 0.011,
                closed_window_gap_return: -0.006,
                post_shock_activity_lift: 0.98,
            },
        ],
        observed_open_session_returns: vec![
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 15, 0, 0),
                elapsed_same_session_hours: 1.0,
                log_return: 0.004,
                post_shock_activity_lift: 1.00,
                sell_share_proxy: 0.48,
                source_activity: SideSourceActivityDecomposition::from_sell_share_proxy(0.48),
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 16, 5, 0),
                elapsed_same_session_hours: 1.0,
                log_return: 0.018,
                post_shock_activity_lift: 1.35,
                sell_share_proxy: 0.62,
                source_activity: SideSourceActivityDecomposition::from_sell_share_proxy(0.62),
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 18, 20, 0),
                elapsed_same_session_hours: 1.0,
                log_return: -0.014,
                post_shock_activity_lift: 1.10,
                sell_share_proxy: 0.58,
                source_activity: SideSourceActivityDecomposition::from_sell_share_proxy(0.58),
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 4, 19, 15, 0),
                elapsed_same_session_hours: 1.0,
                log_return: 0.010,
                post_shock_activity_lift: 0.95,
                sell_share_proxy: 0.41,
                source_activity: SideSourceActivityDecomposition::from_sell_share_proxy(0.41),
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: utc_nanos(2026, 2, 4, 20, 0, 0),
                elapsed_same_session_hours: 1.0,
                log_return: -0.003,
                post_shock_activity_lift: 0.92,
                sell_share_proxy: 0.55,
                source_activity: SideSourceActivityDecomposition::from_sell_share_proxy(0.55),
            },
        ],
        detected_intraday_jump_candidates: vec![
            ReturnComponentsDetectedIntradayJumpCandidate {
                jump_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 16, 5, 0),
                jump_return: 0.018,
                post_shock_activity_lift: 1.35,
            },
            ReturnComponentsDetectedIntradayJumpCandidate {
                jump_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 18, 20, 0),
                jump_return: -0.014,
                post_shock_activity_lift: 1.10,
            },
            ReturnComponentsDetectedIntradayJumpCandidate {
                jump_timestamp_utc_nanos: utc_nanos(2026, 2, 4, 19, 15, 0),
                jump_return: 0.010,
                post_shock_activity_lift: 0.95,
            },
        ],
        observed_close_open_boundary_observations: vec![
            ReturnComponentsObservedCloseOpenBoundaryObservation {
                close_timestamp_utc_nanos: utc_nanos(2026, 2, 2, 21, 0, 0),
                close_price: 100.0,
                open_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 14, 30, 0),
                open_price: 98.70858118310978,
                post_shock_activity_lift: 1.40,
            },
            ReturnComponentsObservedCloseOpenBoundaryObservation {
                close_timestamp_utc_nanos: utc_nanos(2026, 2, 3, 21, 0, 0),
                close_price: 100.0,
                open_timestamp_utc_nanos: utc_nanos(2026, 2, 4, 14, 30, 0),
                open_price: 100.80320855069357,
                post_shock_activity_lift: 1.05,
            },
        ],
        close_open_gap_detection_policy: ReturnComponentsCloseOpenGapDetectionPolicy::new(0.005),
        open_session_sell_share_proxy: 0.52,
        shared_mark_law_template: SharedNewsMarkLawConfiguration {
            gamma_severity_marks: GammaSeverityMarkConfiguration::try_new(1.8)
                .expect("gamma severity mark configuration should validate"),
        },
        return_clock_exposure_parameters:
            janiform_model_kernels::return_clock::ReturnClockExposureParameters::neutral(),
        price_discretization_surface: janiform_calibration_artifacts::PriceDiscretizationSurface {
            tick_size: None,
            tick_size_source: "test".to_string(),
            inference_policy: "test".to_string(),
            observation_policy: "test".to_string(),
        },
        timestamp_batching_surface:
            janiform_model_kernels::timestamp_batching_surface::TimestampBatchingSurface::disabled(),
        objective_metric_weights: ReturnComponentsStageObjectiveMetricWeights::new(
            1.0, 1.0, 1.0, 1.0, 1.0, 1.0,
        ),
    }
}

pub(crate) fn calibration_stage_test_registry() -> ExchangeCalendarRegistry {
    ExchangeCalendarRegistryLoader::load_from_json(
        r#"
{
  "schema_version": "calendar.registry.v1",
  "artifact_date": "2026-01-31",
  "exchanges": [
{
  "schema_version": "calendar.v1",
  "exchange_code": "XNYS",
  "exchange_name": "New York Stock Exchange",
  "timezone_name": "America/New_York",
  "rule_metadata": {
    "holiday_calendar_code": "US_EQUITIES",
    "early_close_rule_set": "US_EQUITIES_EARLY_CLOSE"
  },
  "regular_sessions": [
    {
      "weekday": "Monday",
      "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]
    },
    {
      "weekday": "Tuesday",
      "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]
    },
    {
      "weekday": "Wednesday",
      "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]
    }
  ],
  "holiday_dates": [],
  "early_close_sessions": []
}
  ]
}
"#,
    )
    .expect("calendar registry fixture should load")
}

pub(crate) fn calibration_stage_timestamp_fixture() -> Vec<TimestampEventInputRecord> {
    vec![
        timestamp_input_record(0, 2026, 2, 2, 14, 30, 0),
        timestamp_input_record(1, 2026, 2, 2, 15, 30, 0),
        timestamp_input_record(2, 2026, 2, 3, 14, 30, 0),
        timestamp_input_record(3, 2026, 2, 3, 18, 45, 0),
        timestamp_input_record(4, 2026, 2, 4, 14, 30, 0),
        timestamp_input_record(5, 2026, 2, 4, 20, 30, 0),
    ]
}

pub(crate) fn timestamp_input_record(
    source_record_position: u64,
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> TimestampEventInputRecord {
    TimestampEventInputRecord {
        source_record_position,
        timestamp_utc: Utc
            .with_ymd_and_hms(year, month, day, hour, minute, second)
            .single()
            .expect("test timestamp should be valid"),
    }
}
