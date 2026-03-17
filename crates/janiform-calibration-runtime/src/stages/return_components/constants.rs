pub(super) const RETURN_COMPONENTS_STAGE_CANDIDATE_GENERATION_POLICY: &str =
    "observed_open_session_returns_and_shock_evidence_to_nig_search_and_news_candidate_families.v1";
pub(super) const RETURN_COMPONENTS_STAGE_SIGNAL_PREPROCESSING_POLICY: &str = "timestamp_sorted_finite_signal_filtering_with_material_shock_and_continuous_return_partitioning.v1";
pub(super) const RETURN_COMPONENTS_STAGE_OBJECTIVE_METRIC_BUCKET_POLICY: &str = "continuous_distribution_surprise_arrival_jump_gap_activity_and_price_sanity_bucketed_weighted_terms.v1";
pub(super) const RETURN_COMPONENTS_STAGE_DETERMINISTIC_SEED_POLICY: &str =
    "stage_seed_reused_for_candidate_generation_enumeration.v1";
pub(super) const RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_CONTINUOUS_DISTRIBUTION_FIT: &str =
    "continuous_distribution_fit";
pub(super) const RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_SURPRISE_ARRIVAL_FIT: &str =
    "surprise_arrival_fit";
pub(super) const RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_OPEN_JUMP_FIT: &str = "open_jump_fit";
pub(super) const RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_CLOSED_GAP_FIT: &str = "closed_gap_fit";
pub(super) const RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_ACTIVITY_COUPLING_FIT: &str =
    "activity_coupling_fit";
pub(super) const RETURN_COMPONENTS_STAGE_OBJECTIVE_TERM_PRICE_RANGE_SANITY_FIT: &str =
    "price_range_sanity_fit";
pub(super) const RETURN_COMPONENTS_STAGE_JUMP_BIN_QUANTILES: [f64; 3] = [0.25, 0.5, 0.9];
pub(super) const RETURN_COMPONENTS_STAGE_GAP_BIN_QUANTILES: [f64; 3] = [0.25, 0.5, 0.9];
pub(super) const RETURN_COMPONENTS_STAGE_KNOWN_NEWS_EXCLUSION_WINDOW_MINUTES: i64 = 15;
pub(super) const RETURN_COMPONENTS_STAGE_SHOCK_EVIDENCE_MIN_COUNT: usize = 2;
pub(super) const RETURN_COMPONENTS_STAGE_CONTINUOUS_RETURN_FILTER_MULTIPLIER: f64 = 4.0;
pub(super) const RETURN_COMPONENTS_STAGE_PRICE_RANGE_SANITY_THRESHOLD_RATIO: f64 = 4.0;
pub(super) const RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER: f64 = 1.0e-12;
pub(super) const RETURN_COMPONENTS_STAGE_MAX_NIG_CANDIDATES: usize = 16;
pub(super) const RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_WEEKLY_PROFILE_BUCKET_COUNT: usize =
    7 * 24;
pub(super) const RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_RESOLUTION_HOURS: usize = 1;
pub(super) const RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_FIT_POLICY: &str =
    "selected_exogenous_surprise_episode_rate_per_utc_hour_of_week_with_baseline_shrinkage.v1";
pub(super) const RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_NORMALIZATION_POLICY: &str =
    "exposure_weighted_mean_one_over_source_window.v1";
