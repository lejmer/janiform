mod activity_coupling_fit;
mod constants;
mod continuous_fit;
mod continuous_layer_fit;
mod contracts;
mod diagnostic_policy_fit;
mod execution;
mod known_arrival_fit;
mod mark_layer_fit;
mod objective_scoring_policy_fit;
mod price_range_policy_fit;
mod quantile_binning_fit;
mod signal_partition_fit;
mod surprise_episode_selection_fit;
mod surprise_intensity_fit;
#[cfg(test)]
mod test_support;

pub(crate) use continuous_fit::{
    ContinuousLikelihoodFidelityTier, NigShapeSearchPoint,
    ReturnComponentsStageContinuousReturnCalibrationSample,
    build_continuous_return_calibration_sample_from_components,
    continuous_return_calibration_sample_observed_returns, distribution_profile_fit_score,
    evaluate_configuration_likelihood_objective, nig_configuration_from_search_point,
    nig_search_bounds_for_calibration_sample, nig_search_point_from_configuration,
    rescale_continuous_return_calibration_sample_business_time_scales,
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
    ReturnComponentsStageQuantileBinningPolicy, ReturnComponentsStageReturnDistributionDiagnostic,
    ReturnComponentsStageSignalSourceWindow, ReturnComponentsStageSurpriseArrivalDiagnostic,
    ReturnComponentsStageSurpriseEpisodeSelectionPolicy,
    ReturnComponentsSurpriseNewsArrivalCandidateFamily,
};
pub(crate) use execution::{
    ReturnComponentsStageJointRefinementObservedSignalPreparation,
    prepare_observed_signal_inputs_for_joint_refinement, resolve_activity_calibration_context,
};
pub use execution::{
    evaluate_return_components_exact_candidate, run_return_components_calibration_stage,
};
