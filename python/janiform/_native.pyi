__doc__: str


class CalendarRegistryLoadError(Exception): ...


class CalendarMissingExchangeError(Exception): ...


class CalendarInvalidTimezoneError(Exception): ...


class CalendarTimestampParseError(ValueError): ...


class SeasonalityInputValidationError(ValueError): ...


class TimestampEventPreprocessingResult:
    @property
    def schema_version(self) -> str: ...

    @property
    def input_schema_version(self) -> str: ...

    @property
    def exchange_code(self) -> str: ...

    @property
    def exchange_timezone_name(self) -> str: ...

    @property
    def source_record_positions(self) -> list[int]: ...

    @property
    def timestamp_utc_nanos(self) -> list[int]: ...

    @property
    def exchange_local_minute_of_day(self) -> list[int]: ...

    @property
    def exchange_local_day_of_week(self) -> bytes: ...

    @property
    def exchange_local_day_of_year(self) -> list[int]: ...

    @property
    def market_state_codes(self) -> bytes: ...

    @property
    def missing_hour_bucket_utc_nanos(self) -> list[int]: ...

    @property
    def sparse_hour_bucket_utc_nanos(self) -> list[int]: ...

    @property
    def input_record_count(self) -> int: ...

    @property
    def validated_record_count(self) -> int: ...

    @property
    def accepted_record_count(self) -> int: ...

    @property
    def filtered_record_count(self) -> int: ...

    @property
    def first_event_timestamp_utc_nanos(self) -> int | None: ...

    @property
    def last_event_timestamp_utc_nanos(self) -> int | None: ...

    @property
    def hour_bucket_count(self) -> int: ...

    @property
    def missing_hour_bucket_count(self) -> int: ...

    @property
    def sparse_hour_bucket_count(self) -> int: ...


class IntradaySeasonalityEstimateResult:
    @property
    def schema_version(self) -> str: ...

    @property
    def minutes_per_bin(self) -> int: ...

    @property
    def smoothing_kernel(self) -> str: ...

    @property
    def smoothing_bandwidth_bins(self) -> float: ...

    @property
    def minimum_bin_events(self) -> int: ...

    @property
    def pseudocount(self) -> float: ...

    @property
    def bin_start_minute_of_day(self) -> list[int]: ...

    @property
    def event_count_by_bin(self) -> list[int]: ...

    @property
    def sparse_bin_flags(self) -> bytes: ...

    @property
    def smoothed_density_by_bin(self) -> list[float]: ...

    @property
    def intraday_log_component_by_bin(self) -> list[float]: ...

    @property
    def intraday_factor_by_bin(self) -> list[float]: ...

    @property
    def input_event_count(self) -> int: ...

    @property
    def bin_count(self) -> int: ...

    @property
    def zero_event_bin_count(self) -> int: ...

    @property
    def sparse_bin_count(self) -> int: ...


class DayOfWeekSeasonalityEstimateResult:
    @property
    def schema_version(self) -> str: ...

    @property
    def minimum_bucket_events(self) -> int: ...

    @property
    def pseudocount(self) -> float: ...

    @property
    def day_of_week_number(self) -> bytes: ...

    @property
    def event_count_by_day_of_week(self) -> list[int]: ...

    @property
    def sparse_bucket_flags(self) -> bytes: ...

    @property
    def fallback_bucket_flags(self) -> bytes: ...

    @property
    def day_of_week_log_component(self) -> list[float]: ...

    @property
    def day_of_week_factor(self) -> list[float]: ...

    @property
    def input_event_count(self) -> int: ...

    @property
    def bucket_count(self) -> int: ...

    @property
    def zero_event_bucket_count(self) -> int: ...

    @property
    def sparse_bucket_count(self) -> int: ...

    @property
    def fallback_bucket_count(self) -> int: ...


class DayOfYearSeasonalityEstimateResult:
    @property
    def schema_version(self) -> str: ...

    @property
    def minimum_bucket_events(self) -> int: ...

    @property
    def pseudocount(self) -> float: ...

    @property
    def day_of_year_number(self) -> list[int]: ...

    @property
    def event_count_by_day_of_year(self) -> list[int]: ...

    @property
    def sparse_bucket_flags(self) -> bytes: ...

    @property
    def fallback_bucket_flags(self) -> bytes: ...

    @property
    def day_of_year_log_component(self) -> list[float]: ...

    @property
    def day_of_year_factor(self) -> list[float]: ...

    @property
    def input_event_count(self) -> int: ...

    @property
    def bucket_count(self) -> int: ...

    @property
    def zero_event_bucket_count(self) -> int: ...

    @property
    def sparse_bucket_count(self) -> int: ...

    @property
    def fallback_bucket_count(self) -> int: ...


class DeterministicSeasonalityCompositionResult:
    @property
    def schema_version(self) -> str: ...

    @property
    def intraday_minutes_per_bin(self) -> int: ...

    @property
    def intraday_bin_index_by_event(self) -> list[int]: ...

    @property
    def intraday_log_component_by_event(self) -> list[float]: ...

    @property
    def day_of_week_log_component_by_event(self) -> list[float]: ...

    @property
    def day_of_year_log_component_by_event(self) -> list[float]: ...

    @property
    def special_log_component_by_event(self) -> list[float]: ...

    @property
    def special_overlay_active_flags_by_event(self) -> bytes: ...

    @property
    def active_overlay_ids_by_event(self) -> list[str]: ...

    @property
    def combined_log_component_by_event(self) -> list[float]: ...

    @property
    def combined_factor_by_event(self) -> list[float]: ...

    @property
    def input_event_count(self) -> int: ...

    @property
    def normalization_policy(self) -> str: ...

    @property
    def fit_window_event_count(self) -> int: ...

    @property
    def target_mean_factor(self) -> float: ...

    @property
    def pre_normalization_mean_factor(self) -> float: ...

    @property
    def normalization_constant(self) -> float: ...

    @property
    def normalization_log_offset(self) -> float: ...

    @property
    def post_normalization_mean_factor(self) -> float: ...

    @property
    def mean_anchor_tolerance(self) -> float: ...

    @property
    def overlays_enabled(self) -> bool: ...

    @property
    def configured_overlay_count(self) -> int: ...

    @property
    def enabled_overlay_count(self) -> int: ...

    @property
    def active_overlay_event_count(self) -> int: ...

    @property
    def overlay_precedence_policy(self) -> str: ...

    @property
    def overlay_merge_policy(self) -> str: ...

    @property
    def overlay_window_boundary_policy(self) -> str: ...

    @property
    def configured_overlay_ids_in_order(self) -> list[str]: ...

    @property
    def applied_overlay_ids_in_order(self) -> list[str]: ...

    @property
    def overlay_source_labels_in_order(self) -> list[str]: ...


class HourlyFactorContractResult:
    @property
    def schema_version(self) -> str: ...

    @property
    def exchange_code(self) -> str: ...

    @property
    def exchange_timezone_name(self) -> str: ...

    @property
    def request_start_utc_nanos(self) -> int: ...

    @property
    def request_end_utc_nanos(self) -> int: ...

    @property
    def interval_indices(self) -> list[int]: ...

    @property
    def interval_start_utc_nanos(self) -> list[int]: ...

    @property
    def interval_end_utc_nanos(self) -> list[int]: ...

    @property
    def interval_start_exchange_local(self) -> list[str]: ...

    @property
    def interval_end_exchange_local(self) -> list[str]: ...

    @property
    def s_h(self) -> list[float]: ...

    @property
    def nu_h(self) -> list[float]: ...

    @property
    def interval_count(self) -> int: ...

    @property
    def first_interval_start_utc_nanos(self) -> int: ...

    @property
    def last_interval_end_utc_nanos(self) -> int: ...

    @property
    def skipped_local_hour_interval_count(self) -> int: ...

    @property
    def repeated_local_hour_interval_count(self) -> int: ...

    @property
    def interval_boundary_policy(self) -> str: ...

    @property
    def interval_alignment_policy(self) -> str: ...

    @property
    def timezone_transition_policy(self) -> str: ...

    @property
    def spring_forward_policy(self) -> str: ...

    @property
    def fall_back_policy(self) -> str: ...

    @property
    def seasonality_materialization_method(self) -> str: ...

    @property
    def news_intensity_materialization_method(self) -> str: ...


class HourlySeasonalityMaterializationResult:
    @property
    def schema_version(self) -> str: ...

    @property
    def exchange_code(self) -> str: ...

    @property
    def exchange_timezone_name(self) -> str: ...

    @property
    def request_start_utc_nanos(self) -> int: ...

    @property
    def request_end_utc_nanos(self) -> int: ...

    @property
    def interval_indices(self) -> list[int]: ...

    @property
    def interval_start_utc_nanos(self) -> list[int]: ...

    @property
    def interval_end_utc_nanos(self) -> list[int]: ...

    @property
    def interval_start_exchange_local(self) -> list[str]: ...

    @property
    def interval_end_exchange_local(self) -> list[str]: ...

    @property
    def s_h(self) -> list[float]: ...

    @property
    def source_point_count(self) -> int: ...

    @property
    def interval_count(self) -> int: ...

    @property
    def sparse_interval_count(self) -> int: ...

    @property
    def interpolated_interval_count(self) -> int: ...

    @property
    def fallback_interval_count(self) -> int: ...

    @property
    def skipped_local_hour_interval_count(self) -> int: ...

    @property
    def repeated_local_hour_interval_count(self) -> int: ...

    @property
    def interpolation_method(self) -> str: ...

    @property
    def aggregation_method(self) -> str: ...

    @property
    def sparse_fallback_policy(self) -> str: ...

    @property
    def sparse_hour_minimum_observation_count(self) -> int: ...

    @property
    def interval_boundary_policy(self) -> str: ...

    @property
    def interval_alignment_policy(self) -> str: ...

    @property
    def timezone_transition_policy(self) -> str: ...

    @property
    def spring_forward_policy(self) -> str: ...

    @property
    def fall_back_policy(self) -> str: ...


class HourlyNewsIntensityMaterializationResult:
    @property
    def schema_version(self) -> str: ...

    @property
    def exchange_code(self) -> str: ...

    @property
    def exchange_timezone_name(self) -> str: ...

    @property
    def request_start_utc_nanos(self) -> int: ...

    @property
    def request_end_utc_nanos(self) -> int: ...

    @property
    def interval_indices(self) -> list[int]: ...

    @property
    def interval_start_utc_nanos(self) -> list[int]: ...

    @property
    def interval_end_utc_nanos(self) -> list[int]: ...

    @property
    def interval_start_exchange_local(self) -> list[str]: ...

    @property
    def interval_end_exchange_local(self) -> list[str]: ...

    @property
    def nu_h(self) -> list[float]: ...

    @property
    def interval_count(self) -> int: ...

    @property
    def fully_closed_interval_count(self) -> int: ...

    @property
    def fully_open_interval_count(self) -> int: ...

    @property
    def mixed_state_interval_count(self) -> int: ...

    @property
    def open_window_override_interval_count(self) -> int: ...

    @property
    def skipped_local_hour_interval_count(self) -> int: ...

    @property
    def repeated_local_hour_interval_count(self) -> int: ...

    @property
    def materialization_method(self) -> str: ...

    @property
    def calendar_market_state_dependency(self) -> str: ...

    @property
    def open_window_override_policy(self) -> str: ...

    @property
    def closed_window_baseline_intensity(self) -> float: ...

    @property
    def interval_boundary_policy(self) -> str: ...

    @property
    def interval_alignment_policy(self) -> str: ...

    @property
    def timezone_transition_policy(self) -> str: ...

    @property
    def spring_forward_policy(self) -> str: ...

    @property
    def fall_back_policy(self) -> str: ...


class CalendarRegistry:
    def __init__(self, registry_json: str) -> None: ...

    def resolve_market_state(self, exchange_code: str, timestamp_utc: str) -> str: ...

    def preprocess_timestamp_events(
        self,
        exchange_code: str,
        event_records: object,
        timestamp_field_name: str = "timestamp",
        session_filter: str = "open_only",
        sparse_hour_event_threshold: int = 1,
    ) -> TimestampEventPreprocessingResult: ...


def estimate_intraday_seasonality(
    exchange_local_minute_of_day: object,
    *,
    minutes_per_bin: int = 1,
    smoothing_kernel: str = "gaussian",
    smoothing_bandwidth_bins: float = 3.0,
    minimum_bin_events: int = 1,
    pseudocount: float = 1e-6,
) -> IntradaySeasonalityEstimateResult: ...


def estimate_day_of_week_seasonality(
    exchange_local_day_of_week: object,
    *,
    minimum_bucket_events: int = 1,
    pseudocount: float = 1e-6,
) -> DayOfWeekSeasonalityEstimateResult: ...


def estimate_day_of_year_seasonality(
    exchange_local_day_of_year: object,
    *,
    minimum_bucket_events: int = 1,
    pseudocount: float = 1e-6,
) -> DayOfYearSeasonalityEstimateResult: ...


def compose_deterministic_seasonality_signal(
    timestamp_utc_nanos: object,
    exchange_local_minute_of_day: object,
    exchange_local_day_of_week: object,
    exchange_local_day_of_year: object,
    intraday_log_component_by_bin: object,
    day_of_week_log_component: object,
    day_of_year_log_component: object,
    *,
    intraday_minutes_per_bin: int = 1,
    scheduled_event_overlays: object | None = None,
) -> DeterministicSeasonalityCompositionResult: ...


def generate_seasonality_diagnostics_report(
    dataset_id: str,
    preprocessed_events: TimestampEventPreprocessingResult,
    intraday_estimate: IntradaySeasonalityEstimateResult,
    day_of_week_estimate: DayOfWeekSeasonalityEstimateResult,
    day_of_year_estimate: DayOfYearSeasonalityEstimateResult,
    composition: DeterministicSeasonalityCompositionResult,
    *,
    estimator_version: str = "seasonality_estimator.v1",
) -> str: ...


def build_hourly_factor_contract(
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: str,
    range_end_utc: str,
    seasonality_hourly_factors: object,
    news_intensity_hourly_factors: object,
    *,
    seasonality_materialization_method: str = "explicit_hourly_input.v1",
    news_intensity_materialization_method: str = "explicit_hourly_input.v1",
) -> HourlyFactorContractResult: ...


def materialize_hourly_seasonality_factors(
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: str,
    range_end_utc: str,
    source_timestamp_utc: object,
    source_seasonality_factors: object,
    *,
    interpolation_method: str = "previous_observation_carry_forward.v1",
    aggregation_method: str = "mean_of_observations.v1",
    sparse_fallback_policy: str = "global_mean.v1",
    sparse_hour_minimum_observation_count: int = 1,
) -> HourlySeasonalityMaterializationResult: ...


def materialize_hourly_news_intensity_factors(
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: str,
    range_end_utc: str,
    closed_window_baseline_intensity: float,
    *,
    open_window_override_by_interval_index: object | None = None,
) -> HourlyNewsIntensityMaterializationResult: ...
