use crate::stages::return_components::constants::{
    RETURN_COMPONENTS_STAGE_DETERMINISTIC_SEED_POLICY,
    RETURN_COMPONENTS_STAGE_SIGNAL_PREPROCESSING_POLICY,
};
use crate::stages::return_components::contracts::{
    ReturnComponentsStageCandidateProvenance, ReturnComponentsStageCandidateSourceWindow,
    ReturnComponentsStageDeterministicSeedMetadata, ReturnComponentsStageDiagnosticPolicy,
    ReturnComponentsStageObjectiveScoringPolicy,
    ReturnComponentsStageObservedSignalSummaryStatistics,
    ReturnComponentsStagePriceRangeSanityPolicy, ReturnComponentsStageQuantileBinningPolicy,
    ReturnComponentsStageSignalSourceWindow, ReturnComponentsStageSurpriseEpisodeSelectionPolicy,
};
use crate::stages::return_components::signal_partition_fit;

pub(super) fn build_candidate_provenance_template(
    input: CandidateProvenanceTemplateBuildInput<'_>,
) -> ReturnComponentsStageCandidateProvenance {
    let CandidateProvenanceTemplateBuildInput {
        source_window,
        stage_seed,
        summary_statistics,
        close_open_gap_detection_filter,
        fitted_signal_partition_layer,
        fitted_surprise_episode_selection_policy,
        fitted_quantile_binning_policy,
        fitted_diagnostic_policy,
        fitted_objective_scoring_policy,
        fitted_price_range_sanity_policy,
    } = input;

    ReturnComponentsStageCandidateProvenance {
        source_windows: build_candidate_source_windows(source_window),
        preprocessing_filter_policy: RETURN_COMPONENTS_STAGE_SIGNAL_PREPROCESSING_POLICY,
        preprocessing_filters: build_preprocessing_filters(
            close_open_gap_detection_filter,
            fitted_signal_partition_layer,
        ),
        surprise_episode_selection_policy: fitted_surprise_episode_selection_policy,
        quantile_binning_policy: fitted_quantile_binning_policy,
        diagnostic_policy: fitted_diagnostic_policy,
        objective_scoring_policy: fitted_objective_scoring_policy,
        price_range_sanity_policy: fitted_price_range_sanity_policy,
        summary_statistics,
        deterministic_seed_metadata: ReturnComponentsStageDeterministicSeedMetadata {
            stage_seed,
            candidate_generation_seed: stage_seed,
            deterministic_seed_policy: RETURN_COMPONENTS_STAGE_DETERMINISTIC_SEED_POLICY,
        },
    }
}

pub(super) struct CandidateProvenanceTemplateBuildInput<'a> {
    pub(super) source_window: ReturnComponentsStageSignalSourceWindow,
    pub(super) stage_seed: u64,
    pub(super) summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    pub(super) close_open_gap_detection_filter: &'a str,
    pub(super) fitted_signal_partition_layer: signal_partition_fit::FittedSignalPartitionLayer,
    pub(super) fitted_surprise_episode_selection_policy:
        ReturnComponentsStageSurpriseEpisodeSelectionPolicy,
    pub(super) fitted_quantile_binning_policy: ReturnComponentsStageQuantileBinningPolicy,
    pub(super) fitted_diagnostic_policy: ReturnComponentsStageDiagnosticPolicy,
    pub(super) fitted_objective_scoring_policy: ReturnComponentsStageObjectiveScoringPolicy,
    pub(super) fitted_price_range_sanity_policy: ReturnComponentsStagePriceRangeSanityPolicy,
}

fn build_candidate_source_windows(
    source_window: ReturnComponentsStageSignalSourceWindow,
) -> Vec<ReturnComponentsStageCandidateSourceWindow> {
    [
        "known_news_calendar",
        "realized_known_news_return_samples",
        "observed_open_session_returns",
        "detected_intraday_jump_candidates",
        "observed_close_open_boundary_observations",
    ]
    .into_iter()
    .map(|source_name| ReturnComponentsStageCandidateSourceWindow {
        source_name: source_name.to_string(),
        window_start_utc_nanos: source_window.window_start_utc_nanos,
        window_end_utc_nanos: source_window.window_end_utc_nanos,
    })
    .collect()
}

fn build_preprocessing_filters(
    close_open_gap_detection_filter: &str,
    fitted_signal_partition_layer: signal_partition_fit::FittedSignalPartitionLayer,
) -> Vec<String> {
    vec![
        "reject_non_finite_known_news_returns".to_string(),
        "reject_non_finite_observed_open_session_returns".to_string(),
        "exclude_known_news_attributed_open_returns_from_continuous_nig_fit".to_string(),
        "exclude_selected_exogenous_surprise_episodes_from_continuous_nig_fit".to_string(),
        "exclude_structural_zero_open_returns_from_continuous_nig_fit".to_string(),
        format!(
            "continuous_return_filter_policy={}",
            fitted_signal_partition_layer.continuous_return_filter_policy
        ),
        format!(
            "continuous_return_filter_threshold={:.12}",
            fitted_signal_partition_layer.continuous_return_filter_threshold
        ),
        "reject_non_finite_intraday_jump_returns".to_string(),
        "reject_non_finite_and_non_positive_close_open_boundary_prices".to_string(),
        "enforce_non_negative_post_shock_activity_lift".to_string(),
        "sort_observed_open_session_returns_by_timestamp".to_string(),
        "sort_intraday_jumps_by_jump_timestamp".to_string(),
        "validate_close_open_session_boundary_transitions".to_string(),
        close_open_gap_detection_filter.to_string(),
        "sort_close_open_gaps_by_close_then_open_timestamp".to_string(),
        "fit_market_pressure_from_observed_side_balance".to_string(),
    ]
}
