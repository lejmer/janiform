//! Top-level re-export for calibration artifacts and runtime.

pub use janiform_calibration_artifacts::CalibrationStage;
pub use janiform_calibration_artifacts::ReturnComponentsStageFittedParameters;
pub use janiform_calibration_artifacts::SurpriseNewsDeterministicIntensityProfile;
pub use janiform_calibration_runtime::{
    CalibrationMemoryCheckpointStore, CalibrationReturnComponentsActivityContextPayload,
    CalibrationReturnComponentsStageCandidateGenerationInput, CalibrationStagedExecutionInput,
    CalibrationStagedRunReport, CalibrationStagedRunStageReport,
    MarketDataTranslationPreprocessingError, MarketDataTranslationPreprocessingOutput,
    MarketDataTranslationPreprocessingRequest, ReturnComponentsStageExactCandidateEvaluation,
    ReturnComponentsStageObservedSignalSummaryStatistics,
    build_option_surface_calibration_observation_from_pricing_hooks,
    evaluate_return_components_exact_candidate_from_bridge,
    evolve_hawkes_state_for_calibration_step, load_hourly_factor_contract_for_calibration,
    preprocess_market_data_translation_request,
    project_hawkes_intensities_for_calibration_interval,
    run_staged_calibration_from_execution_input,
};
