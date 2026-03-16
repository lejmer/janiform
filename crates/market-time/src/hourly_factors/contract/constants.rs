pub const HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION: &str = "seasonality.hourly_factor.contract.v1";
pub const HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY: &str = "start_inclusive_end_exclusive.v1";
pub const HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY: &str =
    "request_start_anchored_fixed_width_utc_interval_grid.v1";
pub const HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY: &str =
    "fixed_width_utc_interval_projected_to_exchange_local.v1";
pub const HOURLY_FACTOR_SPRING_FORWARD_POLICY: &str = "skip_nonexistent_local_hour_labels.v1";
pub const HOURLY_FACTOR_FALL_BACK_POLICY: &str =
    "preserve_repeated_local_hour_labels_with_numeric_utc_offset.v1";
pub const DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD: &str = "explicit_hourly_input.v1";
pub const HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION: &str =
    "seasonality.hourly_s_h.materialization.v1";
pub const HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION: &str =
    "seasonality.hourly_nu_h.materialization.v1";
pub const DEFAULT_SEASONALITY_INTERPOLATION_METHOD: &str = "previous_observation_carry_forward.v1";
pub const DEFAULT_SEASONALITY_AGGREGATION_METHOD: &str = "mean_of_observations.v1";
pub const DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY: &str = "global_mean.v1";
pub const DEFAULT_SPARSE_HOUR_MINIMUM_OBSERVATION_COUNT: usize = 1;
pub const DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD: &str =
    "known_calendar_anchor_plus_surprise_background_controls.v1";
pub const DEFAULT_NEWS_INTENSITY_COMPONENT_AGGREGATION_POLICY: &str =
    "known_component_plus_surprise_component.v1";
pub const DEFAULT_KNOWN_NEWS_CALENDAR_ANCHOR_POLICY: &str =
    "known_scheduled_timestamp_utc_nanos_start_inclusive_end_exclusive_count.v1";
pub const DEFAULT_SURPRISE_NEWS_INTENSITY_CONTROL_POLICY: &str =
    "interval_indexed_surprise_multiplier_with_default_one.v1";

pub(super) const ONE_SECOND_NANOS: i64 = 1_000_000_000;
