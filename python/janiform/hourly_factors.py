from __future__ import annotations

import re
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from decimal import Decimal, InvalidOperation
from types import ModuleType
from typing import Any, Optional, cast

from ._native_api import native_module, remap_native_exception_by_type
from ._native_types import (
    HourlyFactorContractResultProtocol,
    HourlyNewsIntensityMaterializationResultProtocol,
    HourlySeasonalityMaterializationResultProtocol,
)
from .calendar import (
    CalendarInvalidTimestampError,
    CalendarInvalidTimezoneError,
    CalendarMissingExchangeError,
    CalendarRegistry,
)
from .seasonality import SeasonalityInputValidationError

HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION: str = "seasonality.hourly_factor.contract.v1"
HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION: str = "seasonality.hourly_s_h.materialization.v1"
HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION: str = "seasonality.hourly_nu_h.materialization.v1"
DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD: str = "explicit_hourly_input.v1"
DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD: str = (
    "known_calendar_anchor_plus_surprise_background_controls.v1"
)
DEFAULT_SEASONALITY_INTERPOLATION_METHOD: str = "previous_observation_carry_forward.v1"
DEFAULT_SEASONALITY_AGGREGATION_METHOD: str = "mean_of_observations.v1"
DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY: str = "global_mean.v1"
DEFAULT_SPARSE_HOUR_MINIMUM_OBSERVATION_COUNT: int = 1
HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY: str = (
    "fixed_width_utc_interval_projected_to_exchange_local.v1"
)
HOURLY_FACTOR_SPRING_FORWARD_POLICY: str = "skip_nonexistent_local_hour_labels.v1"
HOURLY_FACTOR_FALL_BACK_POLICY: str = (
    "preserve_repeated_local_hour_labels_with_numeric_utc_offset.v1"
)
_RUNTIME_CALIBRATION_SEASONALITY_MATERIALIZATION_METHOD: str = (
    "calibration.normalized_event_signal_hourly.v1"
)
_RUNTIME_CALIBRATION_SEASONALITY_MEAN_RENORMALIZED_MATERIALIZATION_METHOD: str = (
    "calibration.normalized_event_signal_hourly_mean_renormalized.v1"
)
_UNIX_EPOCH_UTC = datetime(1970, 1, 1, tzinfo=UTC)
_INTERVAL_SPEC_PATTERN = re.compile(
    r"^\s*(?P<value>[0-9]+(?:\.[0-9]+)?)\s*(?P<unit>[A-Za-z]+)\s*$"
)
_INTERVAL_SPEC_NANOS_BY_UNIT: dict[str, int] = {
    "ns": 1,
    "nanosecond": 1,
    "nanoseconds": 1,
    "us": 1_000,
    "microsecond": 1_000,
    "microseconds": 1_000,
    "ms": 1_000_000,
    "millisecond": 1_000_000,
    "milliseconds": 1_000_000,
    "s": 1_000_000_000,
    "sec": 1_000_000_000,
    "secs": 1_000_000_000,
    "second": 1_000_000_000,
    "seconds": 1_000_000_000,
    "m": 60 * 1_000_000_000,
    "min": 60 * 1_000_000_000,
    "mins": 60 * 1_000_000_000,
    "minute": 60 * 1_000_000_000,
    "minutes": 60 * 1_000_000_000,
    "h": 3_600 * 1_000_000_000,
    "hr": 3_600 * 1_000_000_000,
    "hrs": 3_600 * 1_000_000_000,
    "hour": 3_600 * 1_000_000_000,
    "hours": 3_600 * 1_000_000_000,
    "d": 86_400 * 1_000_000_000,
    "day": 86_400 * 1_000_000_000,
    "days": 86_400 * 1_000_000_000,
}


def _parse_interval_duration_nanos(
    interval: str | timedelta,
    *,
    field_name: str = "interval",
) -> int:
    if isinstance(interval, timedelta):
        interval_duration_nanos = round(interval.total_seconds() * 1_000_000_000)
        if interval_duration_nanos <= 0:
            raise SeasonalityInputValidationError(f"{field_name} must be strictly positive")
        return interval_duration_nanos

    if not isinstance(interval, str):
        raise SeasonalityInputValidationError(
            f"{field_name} must be a pandas-style interval string or timedelta"
        )

    parsed_match = _INTERVAL_SPEC_PATTERN.match(interval)
    if parsed_match is None:
        raise SeasonalityInputValidationError(
            f"{field_name} must be a pandas-style interval string like `1h` or `15min`"
        )

    raw_value = parsed_match.group("value")
    unit = parsed_match.group("unit").lower()
    unit_nanos = _INTERVAL_SPEC_NANOS_BY_UNIT.get(unit)
    if unit_nanos is None:
        raise SeasonalityInputValidationError(
            f"{field_name} uses unsupported interval unit `{unit}`"
        )

    try:
        interval_multiplier = Decimal(raw_value)
    except InvalidOperation as e:
        raise SeasonalityInputValidationError(
            f"{field_name} contains an invalid numeric interval value `{raw_value}`"
        ) from e

    interval_duration_nanos_decimal = interval_multiplier * Decimal(unit_nanos)
    if interval_duration_nanos_decimal != interval_duration_nanos_decimal.to_integral_value():
        raise SeasonalityInputValidationError(
            f"{field_name} must map exactly to an integer UTC nanosecond duration"
        )

    interval_duration_nanos = int(interval_duration_nanos_decimal)
    if interval_duration_nanos <= 0:
        raise SeasonalityInputValidationError(f"{field_name} must be strictly positive")

    return interval_duration_nanos


class HourlyFactorProviderError(Exception):
    """Base class for shared hourly factor provider failures."""


class HourlyFactorProviderMissingInputError(HourlyFactorProviderError):
    """Raised when required seasonality or news-intensity source input is missing."""


class HourlyFactorProviderUnsupportedRangeError(HourlyFactorProviderError):
    """Raised when the requested range is unsupported by hourly interval rules."""


_HOURLY_FACTOR_NATIVE_EXCEPTION_TYPES: dict[str, type[Exception]] = {
    "SeasonalityInputValidationError": SeasonalityInputValidationError,
    "CalendarMissingExchangeError": CalendarMissingExchangeError,
    "CalendarInvalidTimezoneError": CalendarInvalidTimezoneError,
    "CalendarTimestampParseError": CalendarInvalidTimestampError,
}
_HOURLY_FACTOR_PROVIDER_NATIVE_EXCEPTION_TYPES: dict[str, type[Exception]] = {
    **_HOURLY_FACTOR_NATIVE_EXCEPTION_TYPES,
    "HourlyFactorProviderMissingInputError": HourlyFactorProviderMissingInputError,
    "HourlyFactorProviderUnsupportedRangeError": HourlyFactorProviderUnsupportedRangeError,
}


@dataclass(frozen=True, slots=True)
class HourlySeasonalityProviderExplicitSource:
    hourly_factors: Sequence[float]
    materialization_method: str = DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD


@dataclass(frozen=True, slots=True)
class HourlySeasonalityProviderMaterializedSource:
    source_timestamps_utc: Sequence[datetime]
    source_seasonality_factors: Sequence[float]
    interpolation_method: str = DEFAULT_SEASONALITY_INTERPOLATION_METHOD
    aggregation_method: str = DEFAULT_SEASONALITY_AGGREGATION_METHOD
    sparse_fallback_policy: str = DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY
    sparse_hour_minimum_observation_count: int = DEFAULT_SPARSE_HOUR_MINIMUM_OBSERVATION_COUNT


@dataclass(frozen=True, slots=True)
class HourlyNewsIntensityProviderExplicitSource:
    hourly_factors: Sequence[float]
    materialization_method: str = DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD


@dataclass(frozen=True, slots=True)
class HourlyNewsIntensityProviderMaterializedSource:
    known_scheduled_event_timestamps_utc: Sequence[datetime]
    surprise_baseline_intensity: float
    surprise_deterministic_time_multiplier_by_interval_index: Optional[Mapping[int, float]] = None


type HourlySeasonalityProviderSource = (
    HourlySeasonalityProviderExplicitSource | HourlySeasonalityProviderMaterializedSource
)
type HourlyNewsIntensityProviderSource = (
    HourlyNewsIntensityProviderExplicitSource | HourlyNewsIntensityProviderMaterializedSource
)


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
        "interval_duration_nanos",
        "interval_indices",
        "interval_start_utc_nanos",
        "interval_end_utc_nanos",
        "interval_start_exchange_local",
        "interval_end_exchange_local",
        "s_h",
        "known_component_nu_h",
        "surprise_component_nu_h",
        "nu_h",
        "interval_count",
        "first_interval_start_utc_nanos",
        "last_interval_end_utc_nanos",
        "skipped_local_hour_interval_count",
        "repeated_local_hour_interval_count",
        "interval_boundary_policy",
        "interval_alignment_policy",
        "timezone_transition_policy",
        "spring_forward_policy",
        "fall_back_policy",
        "seasonality_materialization_method",
        "news_intensity_materialization_method",
    ),
    interval_boundary_policy="start_inclusive_end_exclusive.v1",
    interval_alignment_policy="request_start_anchored_fixed_width_utc_interval_grid.v1",
    required_metadata_fields=(
        "seasonality_materialization_method",
        "news_intensity_materialization_method",
        "interval_boundary_policy",
        "interval_alignment_policy",
        "timezone_transition_policy",
        "spring_forward_policy",
        "fall_back_policy",
    ),
)


class HourlyFactorContract:
    """Hourly `s_h` and `nu_h` values aligned to deterministic `[start, end)` UTC hour buckets."""

    def __init__(self, native_result: HourlyFactorContractResultProtocol) -> None:
        _validate_hourly_factor_contract_result(native_result)
        self._native_result = native_result

    def __eq__(self, other: object) -> bool:
        return isinstance(other, HourlyFactorContract) and self.to_payload() == other.to_payload()

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
    def interval_duration_nanos(self) -> int:
        return cast(int, self._native_result.interval_duration_nanos)

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
    def known_component_nu_h(self) -> list[float]:
        return cast(list[float], self._native_result.known_component_nu_h)

    @property
    def surprise_component_nu_h(self) -> list[float]:
        return cast(list[float], self._native_result.surprise_component_nu_h)

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
    def skipped_local_hour_interval_count(self) -> int:
        return cast(int, self._native_result.skipped_local_hour_interval_count)

    @property
    def repeated_local_hour_interval_count(self) -> int:
        return cast(int, self._native_result.repeated_local_hour_interval_count)

    @property
    def interval_boundary_policy(self) -> str:
        return cast(str, self._native_result.interval_boundary_policy)

    @property
    def interval_alignment_policy(self) -> str:
        return cast(str, self._native_result.interval_alignment_policy)

    @property
    def timezone_transition_policy(self) -> str:
        return cast(str, self._native_result.timezone_transition_policy)

    @property
    def spring_forward_policy(self) -> str:
        return cast(str, self._native_result.spring_forward_policy)

    @property
    def fall_back_policy(self) -> str:
        return cast(str, self._native_result.fall_back_policy)

    @property
    def seasonality_materialization_method(self) -> str:
        return cast(str, self._native_result.seasonality_materialization_method)

    @property
    def news_intensity_materialization_method(self) -> str:
        return cast(str, self._native_result.news_intensity_materialization_method)

    def to_payload(self) -> dict[str, Any]:
        return {
            "schema_version": self.schema_version,
            "exchange_code": self.exchange_code,
            "exchange_timezone_name": self.exchange_timezone_name,
            "request_start_utc_nanos": self.request_start_utc_nanos,
            "request_end_utc_nanos": self.request_end_utc_nanos,
            "interval_duration_nanos": self.interval_duration_nanos,
            "interval_indices": list(self.interval_indices),
            "interval_start_utc_nanos": list(self.interval_start_utc_nanos),
            "interval_end_utc_nanos": list(self.interval_end_utc_nanos),
            "interval_start_exchange_local": list(self.interval_start_exchange_local),
            "interval_end_exchange_local": list(self.interval_end_exchange_local),
            "s_h": list(self.s_h),
            "known_component_nu_h": list(self.known_component_nu_h),
            "surprise_component_nu_h": list(self.surprise_component_nu_h),
            "nu_h": list(self.nu_h),
            "interval_count": self.interval_count,
            "first_interval_start_utc_nanos": self.first_interval_start_utc_nanos,
            "last_interval_end_utc_nanos": self.last_interval_end_utc_nanos,
            "skipped_local_hour_interval_count": self.skipped_local_hour_interval_count,
            "repeated_local_hour_interval_count": self.repeated_local_hour_interval_count,
            "interval_boundary_policy": self.interval_boundary_policy,
            "interval_alignment_policy": self.interval_alignment_policy,
            "timezone_transition_policy": self.timezone_transition_policy,
            "spring_forward_policy": self.spring_forward_policy,
            "fall_back_policy": self.fall_back_policy,
            "seasonality_materialization_method": self.seasonality_materialization_method,
            "news_intensity_materialization_method": self.news_intensity_materialization_method,
        }


def _validate_hourly_factor_contract_result(native_result: HourlyFactorContractResultProtocol) -> None:
    interval_count = int(native_result.interval_count)
    sequence_lengths = {
        "interval_indices": len(native_result.interval_indices),
        "interval_start_utc_nanos": len(native_result.interval_start_utc_nanos),
        "interval_end_utc_nanos": len(native_result.interval_end_utc_nanos),
        "interval_start_exchange_local": len(native_result.interval_start_exchange_local),
        "interval_end_exchange_local": len(native_result.interval_end_exchange_local),
        "s_h": len(native_result.s_h),
        "known_component_nu_h": len(native_result.known_component_nu_h),
        "surprise_component_nu_h": len(native_result.surprise_component_nu_h),
        "nu_h": len(native_result.nu_h),
    }

    for field_name, observed_length in sequence_lengths.items():
        if observed_length != interval_count:
            raise SeasonalityInputValidationError(
                (
                    f"hourly_factor_contract_payload.{field_name} must have length {interval_count} "
                    f"to match interval_count, found {observed_length}"
                ),
            )

    if interval_count > 0:
        if native_result.first_interval_start_utc_nanos != native_result.interval_start_utc_nanos[0]:
            raise SeasonalityInputValidationError(
                "hourly_factor_contract_payload.first_interval_start_utc_nanos must match the first interval start",
            )
        if native_result.last_interval_end_utc_nanos != native_result.interval_end_utc_nanos[-1]:
            raise SeasonalityInputValidationError(
                "hourly_factor_contract_payload.last_interval_end_utc_nanos must match the last interval end",
            )


@dataclass(frozen=True, slots=True)
class HourlySeasonalityMaterializationOutputSchema:
    """Describes required fields and policies of hourly seasonality materialization output."""

    schema_version: str
    required_fields: tuple[str, ...]
    interval_boundary_policy: str
    interval_alignment_policy: str
    required_metadata_fields: tuple[str, ...]


HOURLY_SEASONALITY_MATERIALIZATION_OUTPUT_SCHEMA: HourlySeasonalityMaterializationOutputSchema = (
    HourlySeasonalityMaterializationOutputSchema(
        schema_version=HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION,
        required_fields=(
            "schema_version",
            "exchange_code",
            "exchange_timezone_name",
            "request_start_utc_nanos",
            "request_end_utc_nanos",
            "interval_duration_nanos",
            "interval_indices",
            "interval_start_utc_nanos",
            "interval_end_utc_nanos",
            "interval_start_exchange_local",
            "interval_end_exchange_local",
            "s_h",
            "source_point_count",
            "interval_count",
            "sparse_interval_count",
            "interpolated_interval_count",
            "fallback_interval_count",
            "skipped_local_hour_interval_count",
            "repeated_local_hour_interval_count",
            "interpolation_method",
            "aggregation_method",
            "sparse_fallback_policy",
            "sparse_hour_minimum_observation_count",
            "interval_boundary_policy",
            "interval_alignment_policy",
            "timezone_transition_policy",
            "spring_forward_policy",
            "fall_back_policy",
        ),
        interval_boundary_policy="start_inclusive_end_exclusive.v1",
        interval_alignment_policy="request_start_anchored_fixed_width_utc_interval_grid.v1",
        required_metadata_fields=(
            "interpolation_method",
            "aggregation_method",
            "sparse_fallback_policy",
            "sparse_hour_minimum_observation_count",
            "interval_boundary_policy",
            "interval_alignment_policy",
            "timezone_transition_policy",
            "spring_forward_policy",
            "fall_back_policy",
        ),
    )
)


@dataclass(frozen=True, slots=True)
class HourlyNewsIntensityMaterializationOutputSchema:
    """Describes required fields and policies of hourly news-intensity materialization output."""

    schema_version: str
    required_fields: tuple[str, ...]
    interval_boundary_policy: str
    interval_alignment_policy: str
    required_metadata_fields: tuple[str, ...]


HOURLY_NEWS_INTENSITY_MATERIALIZATION_OUTPUT_SCHEMA: HourlyNewsIntensityMaterializationOutputSchema = (
    HourlyNewsIntensityMaterializationOutputSchema(
        schema_version=HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
        required_fields=(
            "schema_version",
            "exchange_code",
            "exchange_timezone_name",
            "request_start_utc_nanos",
            "request_end_utc_nanos",
            "interval_duration_nanos",
            "interval_indices",
            "interval_start_utc_nanos",
            "interval_end_utc_nanos",
            "interval_start_exchange_local",
            "interval_end_exchange_local",
            "known_component_nu_h",
            "surprise_component_nu_h",
            "nu_h",
            "interval_count",
            "known_component_non_zero_interval_count",
            "surprise_component_non_zero_interval_count",
            "combined_component_non_zero_interval_count",
            "known_scheduled_arrival_count",
            "surprise_multiplier_override_interval_count",
            "skipped_local_hour_interval_count",
            "repeated_local_hour_interval_count",
            "materialization_method",
            "component_aggregation_policy",
            "arrival_intensity_policy",
            "known_news_calendar_anchor_policy",
            "surprise_news_intensity_control_policy",
            "known_news_calendar_anchor_count",
            "surprise_baseline_intensity",
            "interval_boundary_policy",
            "interval_alignment_policy",
            "timezone_transition_policy",
            "spring_forward_policy",
            "fall_back_policy",
        ),
        interval_boundary_policy="start_inclusive_end_exclusive.v1",
        interval_alignment_policy="request_start_anchored_fixed_width_utc_interval_grid.v1",
        required_metadata_fields=(
            "materialization_method",
            "component_aggregation_policy",
            "arrival_intensity_policy",
            "known_news_calendar_anchor_policy",
            "surprise_news_intensity_control_policy",
            "known_news_calendar_anchor_count",
            "surprise_baseline_intensity",
            "interval_boundary_policy",
            "interval_alignment_policy",
            "timezone_transition_policy",
            "spring_forward_policy",
            "fall_back_policy",
        ),
    )
)


@dataclass(frozen=True, slots=True)
class HourlySeasonalityMaterializationSettings:
    """Configuration for deterministic materialization from source seasonality samples to hourly `s_h`."""

    interpolation_method: str = DEFAULT_SEASONALITY_INTERPOLATION_METHOD
    aggregation_method: str = DEFAULT_SEASONALITY_AGGREGATION_METHOD
    sparse_fallback_policy: str = DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY
    sparse_hour_minimum_observation_count: int = DEFAULT_SPARSE_HOUR_MINIMUM_OBSERVATION_COUNT


class HourlySeasonalityMaterialization:
    """Hourly `s_h` values aligned to deterministic `[start, end)` UTC hour buckets."""

    def __init__(self, native_result: HourlySeasonalityMaterializationResultProtocol) -> None:
        self._native_result = native_result

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
    def interval_duration_nanos(self) -> int:
        return cast(int, self._native_result.interval_duration_nanos)

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
    def source_point_count(self) -> int:
        return cast(int, self._native_result.source_point_count)

    @property
    def interval_count(self) -> int:
        return cast(int, self._native_result.interval_count)

    @property
    def sparse_interval_count(self) -> int:
        return cast(int, self._native_result.sparse_interval_count)

    @property
    def interpolated_interval_count(self) -> int:
        return cast(int, self._native_result.interpolated_interval_count)

    @property
    def fallback_interval_count(self) -> int:
        return cast(int, self._native_result.fallback_interval_count)

    @property
    def skipped_local_hour_interval_count(self) -> int:
        return cast(int, self._native_result.skipped_local_hour_interval_count)

    @property
    def repeated_local_hour_interval_count(self) -> int:
        return cast(int, self._native_result.repeated_local_hour_interval_count)

    @property
    def interpolation_method(self) -> str:
        return cast(str, self._native_result.interpolation_method)

    @property
    def aggregation_method(self) -> str:
        return cast(str, self._native_result.aggregation_method)

    @property
    def sparse_fallback_policy(self) -> str:
        return cast(str, self._native_result.sparse_fallback_policy)

    @property
    def sparse_hour_minimum_observation_count(self) -> int:
        return cast(int, self._native_result.sparse_hour_minimum_observation_count)

    @property
    def interval_boundary_policy(self) -> str:
        return cast(str, self._native_result.interval_boundary_policy)

    @property
    def interval_alignment_policy(self) -> str:
        return cast(str, self._native_result.interval_alignment_policy)

    @property
    def timezone_transition_policy(self) -> str:
        return cast(str, self._native_result.timezone_transition_policy)

    @property
    def spring_forward_policy(self) -> str:
        return cast(str, self._native_result.spring_forward_policy)

    @property
    def fall_back_policy(self) -> str:
        return cast(str, self._native_result.fall_back_policy)


@dataclass(frozen=True, slots=True)
class HourlyNewsIntensityMaterializationSettings:
    """Configuration for deterministic known/surprise materialization from `nu_news(t)` to hourly `nu_h`."""

    known_scheduled_event_timestamps_utc: Sequence[datetime]
    surprise_baseline_intensity: float
    surprise_deterministic_time_multiplier_by_interval_index: Optional[Mapping[int, float]] = None


class HourlyNewsIntensityMaterialization:
    """Hourly `nu_h` values aligned to deterministic `[start, end)` UTC hour buckets."""

    def __init__(self, native_result: HourlyNewsIntensityMaterializationResultProtocol) -> None:
        self._native_result = native_result

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
    def interval_duration_nanos(self) -> int:
        return cast(int, self._native_result.interval_duration_nanos)

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
    def known_component_nu_h(self) -> list[float]:
        return cast(list[float], self._native_result.known_component_nu_h)

    @property
    def surprise_component_nu_h(self) -> list[float]:
        return cast(list[float], self._native_result.surprise_component_nu_h)

    @property
    def nu_h(self) -> list[float]:
        return cast(list[float], self._native_result.nu_h)

    @property
    def interval_count(self) -> int:
        return cast(int, self._native_result.interval_count)

    @property
    def known_component_non_zero_interval_count(self) -> int:
        return cast(int, self._native_result.known_component_non_zero_interval_count)

    @property
    def surprise_component_non_zero_interval_count(self) -> int:
        return cast(int, self._native_result.surprise_component_non_zero_interval_count)

    @property
    def combined_component_non_zero_interval_count(self) -> int:
        return cast(int, self._native_result.combined_component_non_zero_interval_count)

    @property
    def known_scheduled_arrival_count(self) -> int:
        return cast(int, self._native_result.known_scheduled_arrival_count)

    @property
    def surprise_multiplier_override_interval_count(self) -> int:
        return cast(int, self._native_result.surprise_multiplier_override_interval_count)

    @property
    def skipped_local_hour_interval_count(self) -> int:
        return cast(int, self._native_result.skipped_local_hour_interval_count)

    @property
    def repeated_local_hour_interval_count(self) -> int:
        return cast(int, self._native_result.repeated_local_hour_interval_count)

    @property
    def materialization_method(self) -> str:
        return cast(str, self._native_result.materialization_method)

    @property
    def component_aggregation_policy(self) -> str:
        return cast(str, self._native_result.component_aggregation_policy)

    @property
    def arrival_intensity_policy(self) -> str:
        return cast(str, self._native_result.arrival_intensity_policy)

    @property
    def known_news_calendar_anchor_policy(self) -> str:
        return cast(str, self._native_result.known_news_calendar_anchor_policy)

    @property
    def surprise_news_intensity_control_policy(self) -> str:
        return cast(str, self._native_result.surprise_news_intensity_control_policy)

    @property
    def known_news_calendar_anchor_count(self) -> int:
        return cast(int, self._native_result.known_news_calendar_anchor_count)

    @property
    def surprise_baseline_intensity(self) -> float:
        return cast(float, self._native_result.surprise_baseline_intensity)

    @property
    def interval_boundary_policy(self) -> str:
        return cast(str, self._native_result.interval_boundary_policy)

    @property
    def interval_alignment_policy(self) -> str:
        return cast(str, self._native_result.interval_alignment_policy)

    @property
    def timezone_transition_policy(self) -> str:
        return cast(str, self._native_result.timezone_transition_policy)

    @property
    def spring_forward_policy(self) -> str:
        return cast(str, self._native_result.spring_forward_policy)

    @property
    def fall_back_policy(self) -> str:
        return cast(str, self._native_result.fall_back_policy)


def provide_hourly_factor_contract(
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
    interval: str | timedelta = "1h",
    seasonality_source: Optional[HourlySeasonalityProviderSource] = None,
    news_intensity_source: Optional[HourlyNewsIntensityProviderSource] = None,
) -> HourlyFactorContract:
    """
    Resolve one canonical hourly factor contract from shared seasonality and news-intensity sources.

    The shared provider enforces one deterministic interval assignment path for pricing and simulation.
    """

    native: ModuleType = native_module()
    normalized_range_start_utc = _normalize_utc_timestamp(range_start_utc, field_name="range_start_utc")
    normalized_range_end_utc = _normalize_utc_timestamp(range_end_utc, field_name="range_end_utc")
    interval_duration_nanos = _parse_interval_duration_nanos(interval)
    serialized_seasonality_source = (
        _serialize_seasonality_provider_source(seasonality_source)
        if seasonality_source is not None
        else None
    )
    serialized_news_intensity_source = (
        _serialize_news_intensity_provider_source(news_intensity_source)
        if news_intensity_source is not None
        else None
    )

    try:
        native_result: HourlyFactorContractResultProtocol = native.provide_hourly_factor_contract(
            calendar_registry._native_registry,
            exchange_code,
            normalized_range_start_utc.isoformat(),
            normalized_range_end_utc.isoformat(),
            interval_duration_nanos,
            seasonality_source=serialized_seasonality_source,
            news_intensity_source=serialized_news_intensity_source,
        )
    except Exception as e:
        mapped_error = remap_native_exception_by_type(
            native,
            e,
            _HOURLY_FACTOR_PROVIDER_NATIVE_EXCEPTION_TYPES,
        )
        if mapped_error is not None:
            raise mapped_error from e
        raise

    return HourlyFactorContract(native_result)


def load_hourly_factor_contract_payload(
    hourly_factor_contract_payload: dict[str, Any] | Any,
) -> HourlyFactorContract:
    if not isinstance(hourly_factor_contract_payload, dict):
        raise SeasonalityInputValidationError("hourly_factor_contract_payload must be an object")

    native: ModuleType = native_module()
    try:
        native_result: HourlyFactorContractResultProtocol = native.load_hourly_factor_contract_payload(
            hourly_factor_contract_payload
        )
    except Exception as e:
        mapped_error = remap_native_exception_by_type(
            native,
            e,
            _HOURLY_FACTOR_NATIVE_EXCEPTION_TYPES,
        )
        if mapped_error is not None:
            raise mapped_error from e
        raise

    return HourlyFactorContract(native_result)


def build_hourly_factor_contract(
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
    interval: str | timedelta = "1h",
    seasonality_hourly_factors: Sequence[float],
    news_intensity_hourly_factors: Sequence[float],
    seasonality_materialization_method: str = DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
    news_intensity_materialization_method: str = DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
) -> HourlyFactorContract:
    """
    Build the canonical hourly factor contract for one exchange and UTC date range.

    The request range is interpreted as `[range_start_utc, range_end_utc)`.
    The `interval` argument defines a fixed-width UTC grid anchored at `range_start_utc`.
    Exchange-local labels are projected from UTC boundaries and include offsets so spring-forward
    missing hours are skipped and fall-back repeated hours stay unambiguous.
    """

    native: ModuleType = native_module()
    normalized_range_start_utc = _normalize_utc_timestamp(range_start_utc, field_name="range_start_utc")
    normalized_range_end_utc = _normalize_utc_timestamp(range_end_utc, field_name="range_end_utc")
    interval_duration_nanos = _parse_interval_duration_nanos(interval)

    try:
        native_result: HourlyFactorContractResultProtocol = native.build_hourly_factor_contract(
            calendar_registry._native_registry,
            exchange_code,
            normalized_range_start_utc.isoformat(),
            normalized_range_end_utc.isoformat(),
            interval_duration_nanos,
            list(seasonality_hourly_factors),
            list(news_intensity_hourly_factors),
            seasonality_materialization_method=seasonality_materialization_method,
            news_intensity_materialization_method=news_intensity_materialization_method,
        )
    except Exception as e:
        mapped_error = remap_native_exception_by_type(
            native,
            e,
            _HOURLY_FACTOR_NATIVE_EXCEPTION_TYPES,
        )
        if mapped_error is not None:
            raise mapped_error from e
        raise

    return HourlyFactorContract(native_result)


def _serialize_seasonality_provider_source(
    seasonality_source: HourlySeasonalityProviderSource,
) -> dict[str, Any]:
    match seasonality_source:
        case HourlySeasonalityProviderExplicitSource():
            return {
                "source_kind": 'explicit',
                "hourly_factors": list(seasonality_source.hourly_factors),
                "materialization_method": seasonality_source.materialization_method,
            }
        case HourlySeasonalityProviderMaterializedSource():
            normalized_source_timestamps_utc = [
                _normalize_utc_timestamp(source_timestamp_utc, field_name="source_timestamps_utc")
                for source_timestamp_utc in seasonality_source.source_timestamps_utc
            ]
            return {
                "source_kind": 'materialized',
                "source_timestamps_utc": [
                    source_timestamp_utc.isoformat()
                    for source_timestamp_utc in normalized_source_timestamps_utc
                ],
                "source_seasonality_factors": list(seasonality_source.source_seasonality_factors),
                "interpolation_method": seasonality_source.interpolation_method,
                "aggregation_method": seasonality_source.aggregation_method,
                "sparse_fallback_policy": seasonality_source.sparse_fallback_policy,
                "sparse_hour_minimum_observation_count": (
                    seasonality_source.sparse_hour_minimum_observation_count
                ),
            }
        case _:
            raise SeasonalityInputValidationError(
                "seasonality_source must be HourlySeasonalityProviderExplicitSource or "
                "HourlySeasonalityProviderMaterializedSource",
            )


def _serialize_news_intensity_provider_source(
    news_intensity_source: HourlyNewsIntensityProviderSource,
) -> dict[str, Any]:
    match news_intensity_source:
        case HourlyNewsIntensityProviderExplicitSource():
            return {
                "source_kind": 'explicit',
                "hourly_factors": list(news_intensity_source.hourly_factors),
                "materialization_method": news_intensity_source.materialization_method,
            }
        case HourlyNewsIntensityProviderMaterializedSource():
            normalized_known_scheduled_event_timestamps_utc = [
                _normalize_utc_timestamp(
                    known_scheduled_event_timestamp_utc,
                    field_name="known_scheduled_event_timestamps_utc",
                )
                for known_scheduled_event_timestamp_utc in news_intensity_source.known_scheduled_event_timestamps_utc
            ]
            surprise_deterministic_time_multiplier_by_interval_index = (
                dict(news_intensity_source.surprise_deterministic_time_multiplier_by_interval_index)
                if news_intensity_source.surprise_deterministic_time_multiplier_by_interval_index is not None
                else {}
            )

            return {
                "source_kind": 'materialized',
                "known_scheduled_event_timestamps_utc": [
                    known_scheduled_event_timestamp_utc.isoformat()
                    for known_scheduled_event_timestamp_utc in normalized_known_scheduled_event_timestamps_utc
                ],
                "surprise_baseline_intensity": news_intensity_source.surprise_baseline_intensity,
                "surprise_deterministic_time_multiplier_by_interval_index": (
                    surprise_deterministic_time_multiplier_by_interval_index
                ),
            }
        case _:
            raise SeasonalityInputValidationError(
                "news_intensity_source must be HourlyNewsIntensityProviderExplicitSource or "
                "HourlyNewsIntensityProviderMaterializedSource",
            )


def materialize_hourly_seasonality_factors(
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
    interval: str | timedelta = "1h",
    source_timestamps_utc: Sequence[datetime],
    source_seasonality_factors: Sequence[float],
    settings: HourlySeasonalityMaterializationSettings = HourlySeasonalityMaterializationSettings(),
) -> HourlySeasonalityMaterialization:
    """
    Materialize source seasonality samples into hourly piecewise-constant `s_h`.

    Source timestamps are interpreted in UTC and target buckets follow `[start, end)` semantics.
    UTC interval boundaries remain authoritative through timezone offset transitions.
    """

    native: ModuleType = native_module()
    normalized_range_start_utc = _normalize_utc_timestamp(range_start_utc, field_name="range_start_utc")
    normalized_range_end_utc = _normalize_utc_timestamp(range_end_utc, field_name="range_end_utc")
    interval_duration_nanos = _parse_interval_duration_nanos(interval)
    normalized_source_timestamps_utc = [
        _normalize_utc_timestamp(source_timestamp_utc, field_name="source_timestamps_utc")
        for source_timestamp_utc in source_timestamps_utc
    ]

    try:
        native_result: HourlySeasonalityMaterializationResultProtocol = (
            native.materialize_hourly_seasonality_factors(
            calendar_registry._native_registry,
            exchange_code,
            normalized_range_start_utc.isoformat(),
            normalized_range_end_utc.isoformat(),
            interval_duration_nanos,
            [source_timestamp_utc.isoformat() for source_timestamp_utc in normalized_source_timestamps_utc],
            list(source_seasonality_factors),
            interpolation_method=settings.interpolation_method,
            aggregation_method=settings.aggregation_method,
            sparse_fallback_policy=settings.sparse_fallback_policy,
            sparse_hour_minimum_observation_count=settings.sparse_hour_minimum_observation_count,
            )
        )
    except Exception as e:
        mapped_error = remap_native_exception_by_type(
            native,
            e,
            _HOURLY_FACTOR_NATIVE_EXCEPTION_TYPES,
        )
        if mapped_error is not None:
            raise mapped_error from e
        raise

    return HourlySeasonalityMaterialization(native_result)


def build_runtime_hourly_seasonality_source_from_calibration_payload(
    *,
    calendar_registry: CalendarRegistry,
    range_start_utc: datetime,
    range_end_utc: datetime,
    interval: str | timedelta = "1h",
    seasonality_stage_payload: Mapping[str, Any],
    mean_renormalize: bool = True,
    settings: HourlySeasonalityMaterializationSettings = HourlySeasonalityMaterializationSettings(),
) -> HourlySeasonalityProviderExplicitSource:
    """
    Build runtime-ready hourly seasonality from the normalized seasonality stage payload.

    The helper materializes the calibrated event-level normalized seasonality signal onto the
    requested fixed-width UTC interval grid. Mean renormalization is enabled by default so `lambda_bar` stays
    responsible for average event-rate level while `s_h` redistributes that level across time.
    """

    exchange_code = _require_calibration_stage_payload_string(
        seasonality_stage_payload,
        "exchange_code",
    )
    timestamp_utc_nanos = _require_calibration_stage_payload_integer_sequence(
        seasonality_stage_payload,
        "timestamp_utc_nanos",
    )
    normalized_combined_factor_by_event = _require_calibration_stage_payload_numeric_sequence(
        seasonality_stage_payload,
        "normalized_combined_factor_by_event",
    )

    if len(timestamp_utc_nanos) != len(normalized_combined_factor_by_event):
        raise SeasonalityInputValidationError(
            "seasonality_stage_payload.timestamp_utc_nanos and "
            "seasonality_stage_payload.normalized_combined_factor_by_event must have the same length",
        )

    materialization = materialize_hourly_seasonality_factors(
        calendar_registry=calendar_registry,
        exchange_code=exchange_code,
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        interval=interval,
        source_timestamps_utc=[
            _utc_datetime_from_nanos(timestamp_utc_nanos_value)
            for timestamp_utc_nanos_value in timestamp_utc_nanos
        ],
        source_seasonality_factors=normalized_combined_factor_by_event,
        settings=settings,
    )

    hourly_factors = tuple(float(value) for value in materialization.s_h)
    materialization_method = _RUNTIME_CALIBRATION_SEASONALITY_MATERIALIZATION_METHOD
    if mean_renormalize:
        hourly_factors = _mean_renormalize_hourly_factors(hourly_factors)
        materialization_method = (
            _RUNTIME_CALIBRATION_SEASONALITY_MEAN_RENORMALIZED_MATERIALIZATION_METHOD
        )

    return HourlySeasonalityProviderExplicitSource(
        hourly_factors=hourly_factors,
        materialization_method=materialization_method,
    )


def materialize_hourly_news_intensity_factors(
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
    interval: str | timedelta = "1h",
    settings: HourlyNewsIntensityMaterializationSettings,
) -> HourlyNewsIntensityMaterialization:
    """
    Materialize known/surprise `nu_news(t)` into interval-aligned `nu_h`.

    Known scheduled-news timestamps are counted per interval and added to the surprise background
    component controlled by baseline and optional interval multipliers.
    UTC interval boundaries remain authoritative through timezone offset transitions.
    """

    native: ModuleType = native_module()

    normalized_range_start_utc = _normalize_utc_timestamp(range_start_utc, field_name="range_start_utc")
    normalized_range_end_utc = _normalize_utc_timestamp(range_end_utc, field_name="range_end_utc")
    interval_duration_nanos = _parse_interval_duration_nanos(interval)
    normalized_known_scheduled_event_timestamps_utc = [
        _normalize_utc_timestamp(
            known_scheduled_event_timestamp_utc,
            field_name="known_scheduled_event_timestamps_utc",
        )
        for known_scheduled_event_timestamp_utc in settings.known_scheduled_event_timestamps_utc
    ]

    surprise_deterministic_time_multiplier_by_interval_index: dict[int, float] = (
        dict(settings.surprise_deterministic_time_multiplier_by_interval_index)
        if settings.surprise_deterministic_time_multiplier_by_interval_index is not None
        else {}
    )

    try:
        native_result: HourlyNewsIntensityMaterializationResultProtocol = (
            native.materialize_hourly_news_intensity_factors(
            calendar_registry._native_registry,
            exchange_code,
            normalized_range_start_utc.isoformat(),
            normalized_range_end_utc.isoformat(),
            interval_duration_nanos,
            settings.surprise_baseline_intensity,
            known_scheduled_event_timestamps_utc=[
                known_scheduled_event_timestamp_utc.isoformat()
                for known_scheduled_event_timestamp_utc in normalized_known_scheduled_event_timestamps_utc
            ],
            surprise_deterministic_time_multiplier_by_interval_index=(
                surprise_deterministic_time_multiplier_by_interval_index
            ),
            )
        )
    except Exception as e:
        mapped_error = remap_native_exception_by_type(
            native,
            e,
            _HOURLY_FACTOR_NATIVE_EXCEPTION_TYPES,
        )
        if mapped_error is not None:
            raise mapped_error from e
        raise

    return HourlyNewsIntensityMaterialization(native_result)


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


def _mean_renormalize_hourly_factors(hourly_factors: Sequence[float]) -> tuple[float, ...]:
    if len(hourly_factors) == 0:
        return ()

    mean_hourly_factor = sum(float(value) for value in hourly_factors) / len(hourly_factors)
    if mean_hourly_factor == 0.0:
        return tuple(float(value) for value in hourly_factors)

    return tuple(float(value) / mean_hourly_factor for value in hourly_factors)


def _require_calibration_stage_payload_string(
    seasonality_stage_payload: Mapping[str, Any],
    field_name: str,
) -> str:
    if not isinstance(seasonality_stage_payload, Mapping):
        raise SeasonalityInputValidationError("seasonality_stage_payload must be a mapping")

    raw_value = seasonality_stage_payload.get(field_name)
    if not isinstance(raw_value, str) or not raw_value.strip():
        raise SeasonalityInputValidationError(
            f"seasonality_stage_payload.{field_name} must be a non-empty string",
        )

    return raw_value


def _require_calibration_stage_payload_integer_sequence(
    seasonality_stage_payload: Mapping[str, Any],
    field_name: str,
) -> list[int]:
    if not isinstance(seasonality_stage_payload, Mapping):
        raise SeasonalityInputValidationError("seasonality_stage_payload must be a mapping")

    raw_value = seasonality_stage_payload.get(field_name)
    if not isinstance(raw_value, Sequence) or isinstance(raw_value, str | bytes):
        raise SeasonalityInputValidationError(
            f"seasonality_stage_payload.{field_name} must be a sequence",
        )

    values: list[int] = []
    for entry_index, entry in enumerate(raw_value):
        if not isinstance(entry, int):
            raise SeasonalityInputValidationError(
                f"seasonality_stage_payload.{field_name}[{entry_index}] must be an integer",
            )
        values.append(entry)

    return values


def _require_calibration_stage_payload_numeric_sequence(
    seasonality_stage_payload: Mapping[str, Any],
    field_name: str,
) -> list[float]:
    if not isinstance(seasonality_stage_payload, Mapping):
        raise SeasonalityInputValidationError("seasonality_stage_payload must be a mapping")

    raw_value = seasonality_stage_payload.get(field_name)
    if not isinstance(raw_value, Sequence) or isinstance(raw_value, str | bytes):
        raise SeasonalityInputValidationError(
            f"seasonality_stage_payload.{field_name} must be a sequence",
        )

    values: list[float] = []
    for entry_index, entry in enumerate(raw_value):
        if not isinstance(entry, int | float):
            raise SeasonalityInputValidationError(
                f"seasonality_stage_payload.{field_name}[{entry_index}] must be numeric",
            )
        values.append(float(entry))

    return values


def _utc_datetime_from_nanos(timestamp_utc_nanos: int) -> datetime:
    microseconds_since_epoch, _ = divmod(timestamp_utc_nanos, 1_000)
    return _UNIX_EPOCH_UTC + timedelta(microseconds=microseconds_since_epoch)


__all__ = [
    "DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD",
    "DEFAULT_SEASONALITY_AGGREGATION_METHOD",
    "DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD",
    "DEFAULT_SEASONALITY_INTERPOLATION_METHOD",
    "DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY",
    "DEFAULT_SPARSE_HOUR_MINIMUM_OBSERVATION_COUNT",
    "HOURLY_FACTOR_FALL_BACK_POLICY",
    "HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION",
    "HOURLY_FACTOR_OUTPUT_SCHEMA",
    "HOURLY_FACTOR_SPRING_FORWARD_POLICY",
    "HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY",
    "HOURLY_NEWS_INTENSITY_MATERIALIZATION_OUTPUT_SCHEMA",
    "HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION",
    "HOURLY_SEASONALITY_MATERIALIZATION_OUTPUT_SCHEMA",
    "HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION",
    "HourlyFactorContract",
    "HourlyFactorProviderError",
    "HourlyFactorProviderMissingInputError",
    "HourlyFactorProviderUnsupportedRangeError",
    "HourlyFactorOutputSchema",
    "HourlyNewsIntensityProviderExplicitSource",
    "HourlyNewsIntensityProviderMaterializedSource",
    "HourlyNewsIntensityProviderSource",
    "HourlyNewsIntensityMaterialization",
    "HourlyNewsIntensityMaterializationOutputSchema",
    "HourlyNewsIntensityMaterializationSettings",
    "HourlySeasonalityProviderExplicitSource",
    "HourlySeasonalityProviderMaterializedSource",
    "HourlySeasonalityProviderSource",
    "HourlySeasonalityMaterialization",
    "HourlySeasonalityMaterializationOutputSchema",
    "HourlySeasonalityMaterializationSettings",
    "build_hourly_factor_contract",
    "build_runtime_hourly_seasonality_source_from_calibration_payload",
    "load_hourly_factor_contract_payload",
    "materialize_hourly_news_intensity_factors",
    "materialize_hourly_seasonality_factors",
    "provide_hourly_factor_contract",
]
