mod activity_coupling_fit;
mod constants;
mod continuous_fit;
mod continuous_layer_fit;
mod contracts;
mod derived_contracts;
mod diagnostic_policy_fit;
mod execution;
mod known_arrival_fit;
mod mark_layer_fit;
mod objective_scoring_policy_fit;
mod price_range_policy_fit;
mod progress;
mod quantile_binning_fit;
mod signal_partition_fit;
mod surprise_episode_selection_fit;
mod surprise_intensity_fit;
#[cfg(test)]
mod test_support;

pub(crate) use continuous_fit::{
    ContinuousLikelihoodEvaluatorCounts, ContinuousLikelihoodFidelityTier,
    ContinuousReturnOptimizerContext, NigShapeSearchPoint,
    NigSharedPlusShiftConfigurationSearchPointMapping,
    ReturnComponentsStageContinuousReturnCalibrationSample,
    build_continuous_return_calibration_sample_from_components,
    build_continuous_return_optimizer_context, build_nig_moment_seed_configurations,
    build_weighted_continuous_return_optimizer_context, distribution_profile_fit_score,
    evaluate_configuration_likelihood_objective_with_context_and_counts,
    nig_configuration_from_search_point, nig_configuration_search_point_mapping,
    nig_log_alpha_bounds_for_calibration_sample,
    nig_search_bounds_for_calibration_sample_with_solver_settings,
    nig_search_point_from_configuration,
    rescale_continuous_return_calibration_sample_business_time_scales,
    rescale_continuous_return_optimizer_context_business_time_scales,
};
#[cfg(test)]
pub(crate) use continuous_fit::{
    continuous_return_calibration_sample_business_time_scales,
    continuous_return_calibration_sample_observed_returns,
};
pub use contracts::{
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
    ReturnComponentsStageProgressPhase, ReturnComponentsStageProgressUpdate,
    ReturnComponentsStageQuantileBinningPolicy, ReturnComponentsStageReturnDistributionDiagnostic,
    ReturnComponentsStageSignalSourceWindow, ReturnComponentsStageSurpriseArrivalDiagnostic,
    ReturnComponentsStageSurpriseEpisodeSelectionPolicy,
    ReturnComponentsSurpriseNewsArrivalCandidateFamily,
};
pub(crate) use execution::run_return_components_calibration_stage_with_progress_observer;
pub(crate) use execution::{
    ReturnComponentsStageJointRefinementObservedSignalPreparation,
    prepare_observed_signal_inputs_for_joint_refinement, resolve_activity_calibration_context,
};
pub use execution::{
    evaluate_return_components_exact_candidate, run_return_components_calibration_stage,
};
