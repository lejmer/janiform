use janiform_foundations::randomness::SimulationRandomnessSubstreamDomain;
use janiform_model_kernels::news::SHARED_NEWS_EVENT_DISPATCH_RETURN_ROUTING_POLICY;

pub const SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY: &str =
    "explicit_interval_aligned_s_h_and_news_intensity_inputs_without_implicit_resampling.v1";
pub const SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SETTINGS_POLICY: &str =
    "positive_u_probes_and_positive_non_unit_z_probes_for_oracle_slice_sampling.v1";
pub const SUFFICIENT_STATISTICS_TRANSFORM_SLICE_EXPECTATION_ESTIMATION_POLICY: &str =
    "first_order_finite_difference_of_laplace_and_pgf_slices.v1";
pub const SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SAMPLING_POLICY: &str = "counts_via_fractional_rounding_and_integrated_intensity_via_exponential_with_slice_implied_mean.v1";
pub const SUFFICIENT_STATISTICS_TRANSFORM_SLICE_RANDOMNESS_SUBSTREAM_POLICY: &str =
    "reuse_seeded_tick_substream_domains_with_interval_index_for_slice_sampler.v1";
pub const SUFFICIENT_STATISTICS_STREAMING_CHUNKING_POLICY: &str =
    "fixed_max_intervals_per_chunk_with_ordered_interval_emission.v1";
pub const SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY: &str =
    "path_seed_partitioned_from_root_seed_and_path_index_with_shared_substream_mixer.v1";
pub const SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY: &str =
    "interval_boundary_drift_then_count_mapped_endogenous_jump_composition.v1";
pub const SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY: &str =
    "sample_trading_time_increment_from_sampled_statistics_via_shared_return_decomposition.v1";
pub const SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_BREAKDOWN_POLICY: &str =
    "optional_component_breakdown_toggle_for_risk_interval_return_generation.v1";
pub const SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY: &str =
    SHARED_NEWS_EVENT_DISPATCH_RETURN_ROUTING_POLICY;
/// Public default risk interval in hours, overrideable on the risk request contract.
pub const DEFAULT_INTRADAY_RISK_INTERVAL_DELTA: f64 = 1.0;

// Public default transform probe for positive `a + u`, overrideable via slice settings.
pub(crate) const DEFAULT_TRANSFORM_SLICE_A_PLUS_U: f64 = 1.0e-3;
// Public default transform probe for positive `a - u`, overrideable via slice settings.
pub(crate) const DEFAULT_TRANSFORM_SLICE_A_MINUS_U: f64 = 1.0e-3;
// Public default transform probe for `n + z`, overrideable via slice settings.
pub(crate) const DEFAULT_TRANSFORM_SLICE_N_PLUS_Z: f64 = 0.99;
// Public default transform probe for `n - z`, overrideable via slice settings.
pub(crate) const DEFAULT_TRANSFORM_SLICE_N_MINUS_Z: f64 = 0.99;
// Public default streaming chunk size, overrideable through chunking settings.
pub(crate) const DEFAULT_STREAMING_MAX_INTERVALS_PER_CHUNK: usize = 256;
pub(crate) const TRANSFORM_SLICE_NEGATIVE_EXPECTATION_TOLERANCE: f64 = 1.0e-10;
pub(crate) const MAX_INTERVAL_STATISTICS_COUNT: u64 = 1_000_000;
pub(crate) const NEWS_INTENSITY_COMPONENT_SUM_TOLERANCE: f64 = 1.0e-12;
pub(crate) const BATCH_SEED_PARTITION_SUBSTREAM_DOMAIN: SimulationRandomnessSubstreamDomain =
    SimulationRandomnessSubstreamDomain::ExogenousNewsCount;
