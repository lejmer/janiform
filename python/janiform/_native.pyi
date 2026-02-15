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
