from typing import Any, Optional

__doc__: str


class CalendarRegistryLoadError(Exception): ...


class CalendarMissingExchangeError(Exception): ...


class CalendarInvalidTimezoneError(Exception): ...


class CalendarTimestampParseError(ValueError): ...


class PricingExecutionError(Exception): ...


class CalibrationExecutionError(Exception): ...


class CalibrationValidationError(ValueError): ...


class SimulationExecutionError(Exception): ...


class SeasonalityInputValidationError(ValueError): ...


class IntervalFactorGridProviderMissingInputError(ValueError): ...


class IntervalFactorGridProviderUnsupportedRangeError(ValueError): ...


def compute_fnv1a_64_hex_lowercase(payload: bytes) -> str: ...


class PreparedCalibrationExecutionHandle:
    @property
    def schema_version(self) -> str: ...

    @property
    def run_id(self) -> str: ...

    def serialize_execution_input_json(self) -> str: ...


class NormalizedMarketDataHandle:
    def serialize_summary_json(self) -> str: ...


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


class IntervalFactorGridContractResult:
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
    def interval_duration_nanos(self) -> int: ...

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
    def seasonality_factor(self) -> list[float]: ...

    @property
    def known_news_intensity(self) -> list[float]: ...

    @property
    def surprise_news_intensity(self) -> list[float]: ...

    @property
    def news_intensity(self) -> list[float]: ...

    @property
    def interval_count(self) -> int: ...

    @property
    def first_interval_start_utc_nanos(self) -> int: ...

    @property
    def last_interval_end_utc_nanos(self) -> int: ...

    @property
    def skipped_exchange_local_interval_count(self) -> int: ...

    @property
    def repeated_exchange_local_interval_count(self) -> int: ...

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


class IntervalSeasonalityMaterializationResult:
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
    def interval_duration_nanos(self) -> int: ...

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
    def seasonality_factor(self) -> list[float]: ...

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
    def skipped_exchange_local_interval_count(self) -> int: ...

    @property
    def repeated_exchange_local_interval_count(self) -> int: ...

    @property
    def interpolation_method(self) -> str: ...

    @property
    def aggregation_method(self) -> str: ...

    @property
    def sparse_fallback_policy(self) -> str: ...

    @property
    def sparse_interval_minimum_observation_count(self) -> int: ...

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


class IntervalNewsIntensityMaterializationResult:
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
    def interval_duration_nanos(self) -> int: ...

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
    def known_news_intensity(self) -> list[float]: ...

    @property
    def surprise_news_intensity(self) -> list[float]: ...

    @property
    def news_intensity(self) -> list[float]: ...

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
    def skipped_exchange_local_interval_count(self) -> int: ...

    @property
    def repeated_exchange_local_interval_count(self) -> int: ...

    @property
    def materialization_method(self) -> str: ...

    @property
    def component_aggregation_policy(self) -> str: ...

    @property
    def arrival_intensity_policy(self) -> str: ...

    @property
    def known_news_calendar_anchor_policy(self) -> str: ...

    @property
    def surprise_news_intensity_control_policy(self) -> str: ...

    @property
    def known_news_calendar_anchor_count(self) -> int: ...

    @property
    def surprise_open_baseline_intensity(self) -> float: ...

    @property
    def surprise_closed_baseline_intensity(self) -> float: ...


from typing import Optional


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
    def side(self) -> list[Optional[str]]: ...

    @property
    def news_arrival_component(self) -> list[Optional[str]]: ...

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
    def pressure_state(self) -> list[float]: ...

    @property
    def deviation_from_anchor(self) -> list[float]: ...

    @property
    def cumulative_core_log_return(self) -> list[float]: ...

    @property
    def state_snapshot_y_plus_plus(self) -> list[Optional[float]]: ...

    @property
    def state_snapshot_y_plus_minus(self) -> list[Optional[float]]: ...

    @property
    def state_snapshot_y_minus_plus(self) -> list[Optional[float]]: ...

    @property
    def state_snapshot_y_minus_minus(self) -> list[Optional[float]]: ...

    @property
    def state_snapshot_deseasonalized_lambda_plus(self) -> list[Optional[float]]: ...

    @property
    def state_snapshot_deseasonalized_lambda_minus(self) -> list[Optional[float]]: ...


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
    def pressure_state(self) -> list[float]: ...

    @property
    def deviation_from_anchor(self) -> list[float]: ...

    @property
    def state_snapshot_y_plus_plus(self) -> list[Optional[float]]: ...

    @property
    def state_snapshot_y_plus_minus(self) -> list[Optional[float]]: ...

    @property
    def state_snapshot_y_minus_plus(self) -> list[Optional[float]]: ...

    @property
    def state_snapshot_y_minus_minus(self) -> list[Optional[float]]: ...

    @property
    def state_snapshot_deseasonalized_lambda_plus(self) -> list[Optional[float]]: ...

    @property
    def state_snapshot_deseasonalized_lambda_minus(self) -> list[Optional[float]]: ...


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
    def surprise_open_baseline_intensity(self) -> float: ...

    @property
    def surprise_closed_baseline_intensity(self) -> float: ...

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


def build_interval_factor_grid(
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: str,
    range_end_utc: str,
    interval_duration_nanos: int,
    seasonality_factors: Any,
    news_intensity_factors: Any,
    *,
    seasonality_materialization_method: str = "explicit_interval_grid_input.v1",
    news_intensity_materialization_method: str = "explicit_interval_grid_input.v1",
) -> IntervalFactorGridContractResult: ...


def load_interval_factor_grid_payload(
    interval_factor_grid_payload: Any,
) -> IntervalFactorGridContractResult: ...


def materialize_interval_seasonality_factors(
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: str,
    range_end_utc: str,
    interval_duration_nanos: int,
    source_timestamps_utc: Any,
    source_seasonality_factors: Any,
    *,
    interpolation_method: str = "previous_observation_carry_forward.v1",
    aggregation_method: str = "mean_of_observations.v1",
    sparse_fallback_policy: str = "global_mean.v1",
    sparse_interval_minimum_observation_count: int = 1,
) -> IntervalSeasonalityMaterializationResult: ...


def materialize_interval_news_intensity_factors(
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: str,
    range_end_utc: str,
    interval_duration_nanos: int,
    surprise_open_baseline_intensity: float,
    surprise_closed_baseline_intensity: float,
    *,
    known_scheduled_event_timestamps_utc: Optional[Any] = None,
    surprise_deterministic_time_multiplier_by_interval_index: Optional[Any] = None,
) -> IntervalNewsIntensityMaterializationResult: ...


def provide_interval_factor_grid(
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: str,
    range_end_utc: str,
    interval_duration_nanos: int,
    *,
    seasonality_source: Optional[Any] = None,
    news_intensity_source: Optional[Any] = None,
) -> IntervalFactorGridContractResult: ...


def run_deterministic_regression_harness(
    calendar_registry: CalendarRegistry,
    calendar_cases: object,
    seasonality_cases: object,
    *,
    seasonality_absolute_tolerance: float,
    news_intensity_absolute_tolerance: float,
) -> str: ...


def run_open_closed_gap_leakage_regression_harness(
    calendar_registry: CalendarRegistry,
    gap_behavior_cases: object,
    *,
    component_absolute_tolerance: float,
) -> str: ...


def run_stylized_fact_horizon_comparison_report_generator(
    generator_request_json: str,
) -> str: ...


def resolve_high_level_pricing_maturity_inputs(
    request: Any,
) -> Any: ...


def derive_activity_compressed_pricing_interval_schedule(
    request: Any,
) -> Any: ...


def run_vanilla_option_pricing(
    execution_input: Any,
) -> Any: ...


def run_vanilla_option_grid_batch_pricing(
    execution_input: Any,
) -> Any: ...


def run_vanilla_option_grid_batch_pricing_with_terminal_progress(
    execution_input: Any,
    progress_mode: str,
) -> Any: ...


def run_vanilla_option_grid_batch_pricing_with_events(
    execution_input: Any,
    event_callback: Any,
) -> Any: ...


def run_vanilla_option_surface_pricing(
    execution_input: Any,
) -> Any: ...


def run_vanilla_option_surface_pricing_with_terminal_progress(
    execution_input: Any,
    progress_mode: str,
) -> Any: ...


def run_vanilla_option_surface_pricing_with_events(
    execution_input: Any,
    event_callback: Any,
) -> Any: ...


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


def estimate_current_state_from_observed_data(
    kernel_parameters: object,
    mean_reversion_fitted_parameters: object,
    state_time_utc_nanos: int,
    observed_log_return: float,
    observation_timestamps_utc: object,
    observation_trade_prices: object,
    observation_trade_sides: object,
) -> object: ...


def materialize_runtime_structural_seasonality(
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: str,
    range_end_utc: str,
    interval_duration_nanos: int,
    clock_model: object,
) -> object: ...


def materialize_surprise_news_weekly_profile_multipliers_for_interval_range(
    range_start_utc: str,
    range_end_utc: str,
    interval_duration_nanos: int,
    surprise_profile: object,
) -> object: ...


def preprocess_market_data_translation_request(
    preprocessing_request: object,
    calendar_registry: CalendarRegistry,
) -> object: ...


def load_and_preprocess_market_data_translation_request(
    loading_request: object,
) -> object: ...


def probe_market_data_source(
    tick_data_source: object,
) -> object: ...


def normalize_market_data_source(
    tick_data_source: object,
    tick_data_column_mapping: object,
    normalization_policy: object,
    require_hawkes: bool = False,
) -> object: ...


def prepare_market_data_calibration_plan(
    request: object,
    cpu_count: Optional[int] = None,
) -> object: ...


def prepare_market_data_calibration_plan_from_phases(
    request: object,
    normalized_market_data_handle: NormalizedMarketDataHandle,
    session_resolution: object,
    cpu_count: Optional[int] = None,
) -> object: ...


def resolve_market_data_session(
    session_configuration: object,
    normalized_market_data_handle: Optional[NormalizedMarketDataHandle] = None,
) -> object: ...


def prepare_fit_workflow(request: object) -> object: ...


def run_prepared_fit_workflow(
    prepared_handle: PreparedCalibrationExecutionHandle,
    request: object,
    event_callback: Optional[object] = None,
) -> object: ...


def run_fit_workflow(
    request: object,
    event_callback: Optional[object] = None,
) -> object: ...


def save_model_artifact(request: object) -> object: ...


def run_coarse_simulation(
    request: object,
    event_callback: Optional[object] = None,
    progress_mode: str = "none",
) -> Any: ...


def export_coarse_simulation_artifact(
    path_payload: object,
    output_path: str,
    reference_price: float,
    timestamp_column: str,
    price_column: str,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    batch_size: int = 256,
) -> int: ...


def validate_coarse_simulation_path(path_payload: object) -> Any: ...


def run_prepared_staged_calibration(
    prepared_handle: PreparedCalibrationExecutionHandle,
    calendar_registry: CalendarRegistry,
    memory_checkpoint_store: Optional[CalibrationMemoryCheckpointStore] = None,
) -> object: ...


def run_prepared_staged_calibration_with_events(
    prepared_handle: PreparedCalibrationExecutionHandle,
    calendar_registry: CalendarRegistry,
    event_callback: object,
    memory_checkpoint_store: Optional[CalibrationMemoryCheckpointStore] = None,
) -> object: ...


def materialize_endogenous_mean_reversion_runtime_interval_anchors(
    kernel_parameters: object,
    nig_shared_plus_shift_configuration: object,
    news_process_configuration: object,
    initial_excitation_state: object,
    mean_reversion_fitted_parameters: object,
    interval_alignment_policy: str,
    start_anchor_value: float,
    seasonality_factor: object,
    known_news_intensity: object,
    surprise_news_intensity: object,
    interval_deltas: object,
) -> object: ...


def run_fitted_model_single_option_pricing_workflow(
    request: Any,
) -> Any: ...


def run_simulation_from_model_artifact(
    request: object,
    event_callback: Optional[object] = None,
    progress_mode: str = "none",
) -> object: ...


def run_seeded_tick_event_simulation(
    request: object,
    event_callback: Optional[object] = None,
    progress_mode: str = "none",
) -> SeededTickSimulationNativeResult: ...


def export_seeded_tick_simulation_artifact(
    native_result: SeededTickSimulationNativeResult,
    output_path: str,
    reference_price: float,
    timestamp_column: str,
    price_column: str,
    side_column: Optional[str] = None,
    batch_size: int = 256,
) -> int: ...
