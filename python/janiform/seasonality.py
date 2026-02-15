from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from enum import StrEnum
from types import ModuleType
from typing import Optional, cast

from .api import native_module
from .calendar import (
    CalendarInvalidTimezoneError,
    CalendarMissingExchangeError,
    CalendarRegistry,
)

TIMESTAMP_EVENT_INPUT_SCHEMA_VERSION: str = "seasonality.timestamp_event.v1"
PREPROCESSED_TIMESTAMP_EVENT_SCHEMA_VERSION: str = "seasonality.timestamp_event.preprocessed.v1"
INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION: str = "seasonality.intraday.estimate.v1"


class SeasonalityPreprocessingError(Exception):
    """Base class for seasonality preprocessing failures."""


class SeasonalityInputValidationError(SeasonalityPreprocessingError):
    """Raised when event records violate the timestamp input contract."""


class SessionFilter(StrEnum):
    ALL = 'all'
    OPEN_ONLY = 'open_only'
    CLOSED_ONLY = 'closed_only'


class IntradaySmoothingKernel(StrEnum):
    GAUSSIAN = 'gaussian'
    UNIFORM = 'uniform'


@dataclass(frozen=True, slots=True)
class TimestampEventInputSchema:
    schema_version: str
    required_fields: tuple[str, ...]
    timestamp_field_name: str
    accepted_timestamp_types: tuple[str, ...]
    timezone_requirement: str
    ordering_policy: str


@dataclass(frozen=True, slots=True)
class IntradaySeasonalityEstimatorSettings:
    minutes_per_bin: int = 1
    smoothing_kernel: IntradaySmoothingKernel = IntradaySmoothingKernel.GAUSSIAN
    smoothing_bandwidth_bins: float = 3.0
    minimum_bin_events: int = 1
    pseudocount: float = 1e-6


TIMESTAMP_EVENT_INPUT_SCHEMA: TimestampEventInputSchema = TimestampEventInputSchema(
    schema_version=TIMESTAMP_EVENT_INPUT_SCHEMA_VERSION,
    required_fields=("timestamp",),
    timestamp_field_name="timestamp",
    accepted_timestamp_types=("datetime", "RFC3339 / ISO-8601 string"),
    timezone_requirement="timestamp values must include timezone information",
    ordering_policy="output is sorted by normalized UTC timestamp and then input position",
)


class TimestampEventPreprocessingResult:
    """Thin Python wrapper over the Rust preprocessing output object."""

    def __init__(self, native_result: object) -> None:
        self._native_result: object = native_result

    @property
    def schema_version(self) -> str:
        return cast(str, self._native_result.schema_version)

    @property
    def input_schema_version(self) -> str:
        return cast(str, self._native_result.input_schema_version)

    @property
    def exchange_code(self) -> str:
        return cast(str, self._native_result.exchange_code)

    @property
    def exchange_timezone_name(self) -> str:
        return cast(str, self._native_result.exchange_timezone_name)

    @property
    def source_record_positions(self) -> list[int]:
        return cast(list[int], self._native_result.source_record_positions)

    @property
    def timestamp_utc_nanos(self) -> list[int]:
        return cast(list[int], self._native_result.timestamp_utc_nanos)

    @property
    def exchange_local_minute_of_day(self) -> list[int]:
        return cast(list[int], self._native_result.exchange_local_minute_of_day)

    @property
    def exchange_local_day_of_week(self) -> bytes:
        return cast(bytes, self._native_result.exchange_local_day_of_week)

    @property
    def exchange_local_day_of_year(self) -> list[int]:
        return cast(list[int], self._native_result.exchange_local_day_of_year)

    @property
    def market_state_codes(self) -> bytes:
        return cast(bytes, self._native_result.market_state_codes)

    @property
    def missing_hour_bucket_utc_nanos(self) -> list[int]:
        return cast(list[int], self._native_result.missing_hour_bucket_utc_nanos)

    @property
    def sparse_hour_bucket_utc_nanos(self) -> list[int]:
        return cast(list[int], self._native_result.sparse_hour_bucket_utc_nanos)

    @property
    def input_record_count(self) -> int:
        return cast(int, self._native_result.input_record_count)

    @property
    def validated_record_count(self) -> int:
        return cast(int, self._native_result.validated_record_count)

    @property
    def accepted_record_count(self) -> int:
        return cast(int, self._native_result.accepted_record_count)

    @property
    def filtered_record_count(self) -> int:
        return cast(int, self._native_result.filtered_record_count)

    @property
    def first_event_timestamp_utc_nanos(self) -> Optional[int]:
        return cast(Optional[int], self._native_result.first_event_timestamp_utc_nanos)

    @property
    def last_event_timestamp_utc_nanos(self) -> Optional[int]:
        return cast(Optional[int], self._native_result.last_event_timestamp_utc_nanos)

    @property
    def hour_bucket_count(self) -> int:
        return cast(int, self._native_result.hour_bucket_count)

    @property
    def missing_hour_bucket_count(self) -> int:
        return cast(int, self._native_result.missing_hour_bucket_count)

    @property
    def sparse_hour_bucket_count(self) -> int:
        return cast(int, self._native_result.sparse_hour_bucket_count)


class IntradaySeasonalityEstimate:
    """Thin Python wrapper over the Rust intraday seasonality estimator output object."""

    def __init__(self, native_result: object) -> None:
        self._native_result: object = native_result

    @property
    def schema_version(self) -> str:
        return cast(str, self._native_result.schema_version)

    @property
    def minutes_per_bin(self) -> int:
        return cast(int, self._native_result.minutes_per_bin)

    @property
    def smoothing_kernel(self) -> str:
        return cast(str, self._native_result.smoothing_kernel)

    @property
    def smoothing_bandwidth_bins(self) -> float:
        return cast(float, self._native_result.smoothing_bandwidth_bins)

    @property
    def minimum_bin_events(self) -> int:
        return cast(int, self._native_result.minimum_bin_events)

    @property
    def pseudocount(self) -> float:
        return cast(float, self._native_result.pseudocount)

    @property
    def bin_start_minute_of_day(self) -> list[int]:
        return cast(list[int], self._native_result.bin_start_minute_of_day)

    @property
    def event_count_by_bin(self) -> list[int]:
        return cast(list[int], self._native_result.event_count_by_bin)

    @property
    def sparse_bin_flags(self) -> bytes:
        return cast(bytes, self._native_result.sparse_bin_flags)

    @property
    def smoothed_density_by_bin(self) -> list[float]:
        return cast(list[float], self._native_result.smoothed_density_by_bin)

    @property
    def intraday_log_component_by_bin(self) -> list[float]:
        return cast(list[float], self._native_result.intraday_log_component_by_bin)

    @property
    def intraday_factor_by_bin(self) -> list[float]:
        return cast(list[float], self._native_result.intraday_factor_by_bin)

    @property
    def input_event_count(self) -> int:
        return cast(int, self._native_result.input_event_count)

    @property
    def bin_count(self) -> int:
        return cast(int, self._native_result.bin_count)

    @property
    def zero_event_bin_count(self) -> int:
        return cast(int, self._native_result.zero_event_bin_count)

    @property
    def sparse_bin_count(self) -> int:
        return cast(int, self._native_result.sparse_bin_count)


def preprocess_timestamp_events(
    event_records: Sequence[Mapping[str, object]],
    *,
    exchange_code: str,
    calendar_registry: CalendarRegistry,
    timestamp_field_name: str = "timestamp",
    session_filter: SessionFilter = SessionFilter.OPEN_ONLY,
    sparse_hour_event_threshold: int = 1,
) -> TimestampEventPreprocessingResult:
    native: ModuleType = native_module()

    try:
        native_result: object = calendar_registry._preprocess_timestamp_events_native(
            exchange_code=exchange_code,
            event_records=event_records,
            timestamp_field_name=timestamp_field_name,
            session_filter=session_filter.value,
            sparse_hour_event_threshold=sparse_hour_event_threshold,
        )
    except native.SeasonalityInputValidationError as e:
        raise SeasonalityInputValidationError(str(e)) from e
    except native.CalendarMissingExchangeError as e:
        raise CalendarMissingExchangeError(str(e)) from e
    except native.CalendarInvalidTimezoneError as e:
        raise CalendarInvalidTimezoneError(str(e)) from e

    return TimestampEventPreprocessingResult(native_result)


def estimate_intraday_seasonality(
    preprocessed_events: TimestampEventPreprocessingResult,
    *,
    settings: Optional[IntradaySeasonalityEstimatorSettings] = None,
) -> IntradaySeasonalityEstimate:
    native: ModuleType = native_module()
    estimator_settings: IntradaySeasonalityEstimatorSettings = (
        IntradaySeasonalityEstimatorSettings() if settings is None else settings
    )

    try:
        native_result: object = native.estimate_intraday_seasonality(
            preprocessed_events.exchange_local_minute_of_day,
            minutes_per_bin=estimator_settings.minutes_per_bin,
            smoothing_kernel=estimator_settings.smoothing_kernel.value,
            smoothing_bandwidth_bins=estimator_settings.smoothing_bandwidth_bins,
            minimum_bin_events=estimator_settings.minimum_bin_events,
            pseudocount=estimator_settings.pseudocount,
        )
    except native.SeasonalityInputValidationError as e:
        raise SeasonalityInputValidationError(str(e)) from e

    return IntradaySeasonalityEstimate(native_result)


__all__ = [
    "INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION",
    "IntradaySeasonalityEstimate",
    "IntradaySeasonalityEstimatorSettings",
    "IntradaySmoothingKernel",
    "PREPROCESSED_TIMESTAMP_EVENT_SCHEMA_VERSION",
    "SessionFilter",
    "SeasonalityInputValidationError",
    "SeasonalityPreprocessingError",
    "TIMESTAMP_EVENT_INPUT_SCHEMA",
    "TIMESTAMP_EVENT_INPUT_SCHEMA_VERSION",
    "TimestampEventInputSchema",
    "TimestampEventPreprocessingResult",
    "estimate_intraday_seasonality",
    "preprocess_timestamp_events",
]
