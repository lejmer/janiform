from collections.abc import Sequence
from dataclasses import dataclass
from datetime import UTC, datetime
from types import ModuleType
from typing import cast

from .api import native_module
from .calendar import (
    CalendarInvalidTimestampError,
    CalendarInvalidTimezoneError,
    CalendarMissingExchangeError,
    CalendarRegistry,
)
from .seasonality import SeasonalityInputValidationError

HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION: str = "seasonality.hourly_factor.contract.v1"
DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD: str = "explicit_hourly_input.v1"


@dataclass(frozen=True, slots=True)
class HourlyFactorOutputSchema:
    """Describes the required fields and policies of hourly factor outputs."""

    schema_version: str
    required_fields: tuple[str, ...]
    interval_boundary_policy: str
    interval_alignment_policy: str
    required_metadata_fields: tuple[str, ...]


HOURLY_FACTOR_OUTPUT_SCHEMA: HourlyFactorOutputSchema = HourlyFactorOutputSchema(
    schema_version=HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
    required_fields=(
        "schema_version",
        "exchange_code",
        "exchange_timezone_name",
        "request_start_utc_nanos",
        "request_end_utc_nanos",
        "interval_indices",
        "interval_start_utc_nanos",
        "interval_end_utc_nanos",
        "interval_start_exchange_local",
        "interval_end_exchange_local",
        "s_h",
        "nu_h",
        "interval_count",
        "first_interval_start_utc_nanos",
        "last_interval_end_utc_nanos",
        "interval_boundary_policy",
        "interval_alignment_policy",
        "seasonality_materialization_method",
        "news_intensity_materialization_method",
    ),
    interval_boundary_policy="start_inclusive_end_exclusive.v1",
    interval_alignment_policy="utc_hour_aligned.v1",
    required_metadata_fields=(
        "seasonality_materialization_method",
        "news_intensity_materialization_method",
        "interval_boundary_policy",
        "interval_alignment_policy",
    ),
)


class HourlyFactorContract:
    """Hourly `s_h` and `nu_h` values aligned to deterministic `[start, end)` UTC hour buckets."""

    def __init__(self, native_result: object) -> None:
        self._native_result: object = native_result

    @property
    def schema_version(self) -> str:
        return cast(str, self._native_result.schema_version)

    @property
    def exchange_code(self) -> str:
        return cast(str, self._native_result.exchange_code)

    @property
    def exchange_timezone_name(self) -> str:
        return cast(str, self._native_result.exchange_timezone_name)

    @property
    def request_start_utc_nanos(self) -> int:
        return cast(int, self._native_result.request_start_utc_nanos)

    @property
    def request_end_utc_nanos(self) -> int:
        return cast(int, self._native_result.request_end_utc_nanos)

    @property
    def interval_indices(self) -> list[int]:
        return cast(list[int], self._native_result.interval_indices)

    @property
    def interval_start_utc_nanos(self) -> list[int]:
        return cast(list[int], self._native_result.interval_start_utc_nanos)

    @property
    def interval_end_utc_nanos(self) -> list[int]:
        return cast(list[int], self._native_result.interval_end_utc_nanos)

    @property
    def interval_start_exchange_local(self) -> list[str]:
        return cast(list[str], self._native_result.interval_start_exchange_local)

    @property
    def interval_end_exchange_local(self) -> list[str]:
        return cast(list[str], self._native_result.interval_end_exchange_local)

    @property
    def s_h(self) -> list[float]:
        return cast(list[float], self._native_result.s_h)

    @property
    def nu_h(self) -> list[float]:
        return cast(list[float], self._native_result.nu_h)

    @property
    def interval_count(self) -> int:
        return cast(int, self._native_result.interval_count)

    @property
    def first_interval_start_utc_nanos(self) -> int:
        return cast(int, self._native_result.first_interval_start_utc_nanos)

    @property
    def last_interval_end_utc_nanos(self) -> int:
        return cast(int, self._native_result.last_interval_end_utc_nanos)

    @property
    def interval_boundary_policy(self) -> str:
        return cast(str, self._native_result.interval_boundary_policy)

    @property
    def interval_alignment_policy(self) -> str:
        return cast(str, self._native_result.interval_alignment_policy)

    @property
    def seasonality_materialization_method(self) -> str:
        return cast(str, self._native_result.seasonality_materialization_method)

    @property
    def news_intensity_materialization_method(self) -> str:
        return cast(str, self._native_result.news_intensity_materialization_method)


def build_hourly_factor_contract(
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
    seasonality_hourly_factors: Sequence[float],
    news_intensity_hourly_factors: Sequence[float],
    seasonality_materialization_method: str = DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
    news_intensity_materialization_method: str = DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
) -> HourlyFactorContract:
    """
    Build the canonical hourly factor contract for one exchange and UTC date range.

    The request range is interpreted as `[range_start_utc, range_end_utc)`.
    Both boundaries must be exact UTC hour boundaries.
    """

    native: ModuleType = native_module()
    normalized_range_start_utc = _normalize_utc_timestamp(range_start_utc, field_name="range_start_utc")
    normalized_range_end_utc = _normalize_utc_timestamp(range_end_utc, field_name="range_end_utc")

    try:
        native_result: object = native.build_hourly_factor_contract(
            calendar_registry._native_registry,
            exchange_code,
            normalized_range_start_utc.isoformat(),
            normalized_range_end_utc.isoformat(),
            list(seasonality_hourly_factors),
            list(news_intensity_hourly_factors),
            seasonality_materialization_method=seasonality_materialization_method,
            news_intensity_materialization_method=news_intensity_materialization_method,
        )
    except native.SeasonalityInputValidationError as e:
        raise SeasonalityInputValidationError(str(e)) from e
    except native.CalendarMissingExchangeError as e:
        raise CalendarMissingExchangeError(str(e)) from e
    except native.CalendarInvalidTimezoneError as e:
        raise CalendarInvalidTimezoneError(str(e)) from e
    except native.CalendarTimestampParseError as e:
        raise CalendarInvalidTimestampError(str(e)) from e

    return HourlyFactorContract(native_result)


def _normalize_utc_timestamp(timestamp_utc: datetime, *, field_name: str) -> datetime:
    if not isinstance(timestamp_utc, datetime):
        raise CalendarInvalidTimestampError(
            f"{field_name} must be a datetime value with timezone information",
        )

    if timestamp_utc.tzinfo is None:
        raise CalendarInvalidTimestampError(
            f"{field_name} must be timezone-aware",
        )

    return timestamp_utc.astimezone(UTC)


__all__ = [
    "DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD",
    "HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION",
    "HOURLY_FACTOR_OUTPUT_SCHEMA",
    "HourlyFactorContract",
    "HourlyFactorOutputSchema",
    "build_hourly_factor_contract",
]
