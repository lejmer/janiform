from typing import Optional

__doc__: str


class CalendarRegistryLoadError(Exception): ...


class CalendarMissingExchangeError(Exception): ...


class CalendarInvalidTimezoneError(Exception): ...


class CalendarTimestampParseError(ValueError): ...


class PricingExecutionError(Exception): ...


class CalibrationExecutionError(Exception): ...


class SimulationExecutionError(Exception): ...


class SeasonalityInputValidationError(ValueError): ...


class HourlyFactorProviderMissingInputError(ValueError): ...


class HourlyFactorProviderUnsupportedRangeError(ValueError): ...


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
    def first_event_timestamp_utc_nanos(self) -> Optional[int]: ...

    @property
    def last_event_timestamp_utc_nanos(self) -> Optional[int]: ...

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
    def known_component_nu_h(self) -> list[float]: ...

    @property
    def surprise_component_nu_h(self) -> list[float]: ...

    @property
    def nu_h(self) -> list[float]: ...

    @property
    def interval_count(self) -> int: ...

    @property
    def known_component_non_zero_interval_count(self) -> int: ...

    @property
    def surprise_component_non_zero_interval_count(self) -> int: ...

    @property
    def combined_component_non_zero_interval_count(self) -> int: ...

    @property
    def known_scheduled_arrival_count(self) -> int: ...

    @property
    def surprise_multiplier_override_interval_count(self) -> int: ...

    @property
    def skipped_local_hour_interval_count(self) -> int: ...

    @property
    def repeated_local_hour_interval_count(self) -> int: ...

    @property
    def materialization_method(self) -> str: ...

    @property
    def component_aggregation_policy(self) -> str: ...

    @property
    def arrival_intensity_policy(self) -> str: ...


class SeededFinePathChunkColumns:
    @property
    def estimated_transport_bytes(self) -> int: ...

    @property
    def transition_index(self) -> list[int]: ...

    @property
    def timestamp_utc_nanos(self) -> list[int]: ...

    @property
    def transition_kind(self) -> list[str]: ...

    @property
    def side(self) -> list[str | None]: ...

    @property
    def news_arrival_component(self) -> list[str | None]: ...

    @property
    def market_state(self) -> list[str]: ...

    @property
    def buy_side_return(self) -> list[float]: ...

    @property
    def sell_side_return(self) -> list[float]: ...

    @property
    def trading_time_return(self) -> list[float]: ...

    @property
    def open_jump_return(self) -> list[float]: ...

    @property
    def closed_gap_return(self) -> list[float]: ...

    @property
    def total_return(self) -> list[float]: ...

    @property
    def core_total_return_increment(self) -> list[float]: ...

    @property
    def observed_log_price(self) -> list[float]: ...

    @property
    def anchor_value(self) -> list[float]: ...

    @property
    def deviation_from_anchor(self) -> list[float]: ...

    @property
    def cumulative_core_log_return(self) -> list[float]: ...

    @property
    def state_snapshot_y_plus_plus(self) -> list[float | None]: ...

    @property
    def state_snapshot_y_plus_minus(self) -> list[float | None]: ...

    @property
    def state_snapshot_y_minus_plus(self) -> list[float | None]: ...

    @property
    def state_snapshot_y_minus_minus(self) -> list[float | None]: ...

    @property
    def state_snapshot_deseasonalized_lambda_plus(self) -> list[float | None]: ...

    @property
    def state_snapshot_deseasonalized_lambda_minus(self) -> list[float | None]: ...


class SeededTickEventTapeChunkColumns:
    @property
    def estimated_transport_bytes(self) -> int: ...

    @property
    def event_index(self) -> list[int]: ...

    @property
    def timestamp_utc_nanos(self) -> list[int]: ...

    @property
    def side(self) -> list[str]: ...

    @property
    def core_total_return_increment(self) -> list[float]: ...

    @property
    def observed_log_price(self) -> list[float]: ...

    @property
    def anchor_value(self) -> list[float]: ...

    @property
    def deviation_from_anchor(self) -> list[float]: ...

    @property
    def state_snapshot_y_plus_plus(self) -> list[float | None]: ...

    @property
    def state_snapshot_y_plus_minus(self) -> list[float | None]: ...

    @property
    def state_snapshot_y_minus_plus(self) -> list[float | None]: ...

    @property
    def state_snapshot_y_minus_minus(self) -> list[float | None]: ...

    @property
    def state_snapshot_deseasonalized_lambda_plus(self) -> list[float | None]: ...

    @property
    def state_snapshot_deseasonalized_lambda_minus(self) -> list[float | None]: ...


class SeededTickSimulationNativeResult:
    @property
    def schema_version(self) -> str: ...

    @property
    def transition_count(self) -> int: ...

    @property
    def event_count(self) -> int: ...

    @property
    def estimated_native_retained_bytes(self) -> int: ...

    def fine_path_metadata_json(self) -> str: ...

    def event_tape_metadata_json(self) -> str: ...

    def get_fine_path_chunk_columns(
        self,
        start_index: int,
        max_records: int,
    ) -> SeededFinePathChunkColumns: ...

    def get_event_tape_chunk_columns(
        self,
        start_index: int,
        max_records: int,
    ) -> SeededTickEventTapeChunkColumns: ...

    @property
    def known_news_calendar_anchor_policy(self) -> str: ...

    @property
    def surprise_news_intensity_control_policy(self) -> str: ...

    @property
    def known_news_calendar_anchor_count(self) -> int: ...

    @property
    def surprise_baseline_intensity(self) -> float: ...

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


class CalibrationMemoryCheckpointStore:
    def __init__(self) -> None: ...

    def checkpoint_count_for_run(self, run_id: str) -> int: ...

    def checkpoint_reference_for_stage(self, run_id: str, stage: str) -> Optional[str]: ...

    def last_checkpoint_reference_for_run(self, run_id: str) -> Optional[str]: ...


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
    scheduled_event_overlays: Optional[object] = None,
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
    interval_duration_nanos: int,
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
    interval_duration_nanos: int,
    source_timestamps_utc: object,
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
    interval_duration_nanos: int,
    surprise_baseline_intensity: float,
    *,
    known_scheduled_event_timestamps_utc: Optional[object] = None,
    surprise_deterministic_time_multiplier_by_interval_index: Optional[object] = None,
) -> HourlyNewsIntensityMaterializationResult: ...


def provide_hourly_factor_contract(
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: str,
    range_end_utc: str,
    interval_duration_nanos: int,
    *,
    seasonality_source: Optional[object] = None,
    news_intensity_source: Optional[object] = None,
) -> HourlyFactorContractResult: ...


def run_deterministic_regression_harness(
    calendar_registry: CalendarRegistry,
    calendar_cases: object,
    seasonality_cases: object,
    *,
    seasonality_absolute_tolerance: float = 1e-12,
    news_intensity_absolute_tolerance: float = 1e-12,
) -> str: ...


def run_open_closed_gap_leakage_regression_harness(
    calendar_registry: CalendarRegistry,
    gap_behavior_cases: object,
    *,
    direct_gap_absolute_tolerance: float = 1e-12,
) -> str: ...


def run_stylized_fact_horizon_comparison_report_generator(
    generator_request_json: str,
) -> str: ...


def run_vanilla_option_pricing(
    execution_input: object,
) -> object: ...


def run_staged_calibration(
    execution_input: object,
    calendar_registry: CalendarRegistry,
    memory_checkpoint_store: Optional[CalibrationMemoryCheckpointStore] = None,
) -> object: ...


def run_staged_calibration_with_events(
    execution_input: object,
    calendar_registry: CalendarRegistry,
    event_callback: object,
    memory_checkpoint_store: Optional[CalibrationMemoryCheckpointStore] = None,
) -> object: ...


def evaluate_return_components_candidate(
    candidate_generation_input: object,
    observed_summary_statistics: object,
    fitted_parameters: object,
    stage_seed: int,
    hawkes_payload: Optional[object] = None,
) -> object: ...


def preprocess_market_data_translation_request(
    preprocessing_request: object,
    calendar_registry: CalendarRegistry,
) -> object: ...


def load_and_preprocess_market_data_translation_request(
    loading_request: object,
) -> object: ...


def resolve_market_data_session_configuration_from_source_file(
    session_configuration: object,
    tick_data_source: object,
    tick_data_column_mapping: object,
    normalization_policy: object,
) -> object: ...


def run_coarse_simulation(
    run_request: object,
) -> object: ...


def run_seeded_tick_event_simulation(
    seed: int,
    start_time_utc: str,
    horizon_interval_count: int,
    kernel_parameters: object,
    nig_shared_plus_shift_configuration: object,
    initial_excitation_state: object,
    hourly_factor_contract: HourlyFactorContractResult,
    news_process_configuration: object,
    mean_reversion_fitted_parameters: object,
    mean_reversion_runtime_interval_anchors: object,
    initial_mean_reversion_boundary_state: object,
    *,
    debug_trace_enabled: bool = False,
    sample_every_nth_transition: int = 1,
    max_records_per_chunk: int = 1024,
    state_snapshot_capture: str = "disabled",
) -> SeededTickSimulationNativeResult: ...
