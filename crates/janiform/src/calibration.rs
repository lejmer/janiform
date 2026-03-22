//! Top-level re-export for calibration artifacts and runtime.

pub use janiform_calibration_artifacts::CalibrationStage;
pub use janiform_calibration_artifacts::ReturnComponentsStageFittedParameters;
pub use janiform_calibration_artifacts::SurpriseNewsDeterministicIntensityProfile;
pub use janiform_calibration_runtime::{
    CalibrationMemoryCheckpointStore, CalibrationReturnComponentsActivityContextPayload,
    CalibrationReturnComponentsStageCandidateGenerationInput, CalibrationStagedExecutionInput,
    CalibrationStagedRunObserverEvent, CalibrationStagedRunReport, CalibrationStagedRunStageReport,
    MarketDataInputFileReference, MarketDataLoadingOutput, MarketDataLoadingRequest,
    MarketDataNormalizationPolicy, MarketDataSessionConfiguration,
    MarketDataSessionResolutionPayload, MarketDataTickDataColumnMapping,
    MarketDataTranslationPreprocessingError, MarketDataTranslationPreprocessingOutput,
    MarketDataTranslationPreprocessingRequest, ReturnComponentsStageExactCandidateEvaluation,
    ReturnComponentsStageObservedSignalSummaryStatistics,
    build_option_surface_calibration_observation_from_pricing_hooks,
    evaluate_return_components_exact_candidate_from_bridge,
    evolve_hawkes_state_for_calibration_step, load_and_preprocess_market_data_translation_request,
    load_hourly_factor_contract_for_calibration, preprocess_market_data_translation_request,
    project_hawkes_intensities_for_calibration_interval,
    resolve_market_data_session_configuration_from_source_file,
    run_staged_calibration_from_execution_input,
    run_staged_calibration_from_execution_input_with_observer,
};
