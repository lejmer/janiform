"""Internal workflow entrypoints for hourly-factor materialization and contract loading."""

from __future__ import annotations

import re
from collections.abc import Mapping, Sequence
from datetime import UTC, datetime, timedelta
from decimal import Decimal, InvalidOperation
from types import ModuleType
from typing import Any, Optional

from .._native_api import native_module, remap_native_exception_by_type
from .._native_types import (
    HourlyFactorContractResultProtocol,
    HourlyNewsIntensityMaterializationResultProtocol,
    HourlySeasonalityMaterializationResultProtocol,
)
from ..calendar import (
    CalendarInvalidTimestampError,
    CalendarInvalidTimezoneError,
    CalendarMissingExchangeError,
    CalendarRegistry,
)
from ..contracts.base import (
    require_datetime_instance_utc,
    require_finite_number_sequence,
    require_integer_sequence,
    require_non_empty_string,
)
from ..seasonality import SeasonalityInputValidationError
from .artifacts import (
    HourlyFactorContract,
    HourlyNewsIntensityMaterialization,
    HourlySeasonalityMaterialization,
)
from .contracts import (
    DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
    HourlyNewsIntensityMaterializationSettings,
    HourlyNewsIntensityProviderExplicitSource,
    HourlyNewsIntensityProviderMaterializedSource,
    HourlyNewsIntensityProviderSource,
    HourlySeasonalityMaterializationSettings,
    HourlySeasonalityProviderExplicitSource,
    HourlySeasonalityProviderMaterializedSource,
    HourlySeasonalityProviderSource,
)
from .errors import (
    HourlyFactorProviderMissingInputError,
    HourlyFactorProviderUnsupportedRangeError,
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


def _seasonality_error_factory(field_path: str, message: str) -> SeasonalityInputValidationError:
    return SeasonalityInputValidationError(f"{field_path} {message}")


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
    except InvalidOperation as error:
        raise SeasonalityInputValidationError(
            f"{field_name} contains an invalid numeric interval value `{raw_value}`"
        ) from error

    interval_duration_nanos_decimal = interval_multiplier * Decimal(unit_nanos)
    if interval_duration_nanos_decimal != interval_duration_nanos_decimal.to_integral_value():
        raise SeasonalityInputValidationError(
            f"{field_name} must map exactly to an integer UTC nanosecond duration"
        )

    interval_duration_nanos = int(interval_duration_nanos_decimal)
    if interval_duration_nanos <= 0:
        raise SeasonalityInputValidationError(f"{field_name} must be strictly positive")

    return interval_duration_nanos


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

    normalized_range_start_utc = require_datetime_instance_utc(
        range_start_utc,
        "range_start_utc",
        _calendar_invalid_timestamp_error,
    )

    normalized_range_end_utc = require_datetime_instance_utc(
        range_end_utc,
        "range_end_utc",
        _calendar_invalid_timestamp_error,
    )

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
    except Exception as error:
        mapped_error = remap_native_exception_by_type(
            native,
            error,
            _HOURLY_FACTOR_PROVIDER_NATIVE_EXCEPTION_TYPES,
        )
        if mapped_error is not None:
            raise mapped_error from error

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
    except Exception as error:
        mapped_error = remap_native_exception_by_type(
            native,
            error,
            _HOURLY_FACTOR_NATIVE_EXCEPTION_TYPES,
        )
        if mapped_error is not None:
            raise mapped_error from error

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

    normalized_range_start_utc = require_datetime_instance_utc(
        range_start_utc,
        "range_start_utc",
        _calendar_invalid_timestamp_error,
    )

    normalized_range_end_utc = require_datetime_instance_utc(
        range_end_utc,
        "range_end_utc",
        _calendar_invalid_timestamp_error,
    )

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
    except Exception as error:
        mapped_error = remap_native_exception_by_type(
            native,
            error,
            _HOURLY_FACTOR_NATIVE_EXCEPTION_TYPES,
        )
        if mapped_error is not None:
            raise mapped_error from error

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
                require_datetime_instance_utc(
                    source_timestamp_utc,
                    "source_timestamps_utc",
                    _calendar_invalid_timestamp_error,
                )
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
                require_datetime_instance_utc(
                    known_scheduled_event_timestamp_utc,
                    "known_scheduled_event_timestamps_utc",
                    _calendar_invalid_timestamp_error,
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
                "surprise_open_baseline_intensity": (
                    news_intensity_source.surprise_open_baseline_intensity
                ),
                "surprise_closed_baseline_intensity": (
                    news_intensity_source.surprise_closed_baseline_intensity
                ),
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

    normalized_range_start_utc = require_datetime_instance_utc(
        range_start_utc,
        "range_start_utc",
        _calendar_invalid_timestamp_error,
    )

    normalized_range_end_utc = require_datetime_instance_utc(
        range_end_utc,
        "range_end_utc",
        _calendar_invalid_timestamp_error,
    )

    interval_duration_nanos = _parse_interval_duration_nanos(interval)

    normalized_source_timestamps_utc = [
        require_datetime_instance_utc(
            source_timestamp_utc,
            "source_timestamps_utc",
            _calendar_invalid_timestamp_error,
        )
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
                [
                    source_timestamp_utc.isoformat()
                    for source_timestamp_utc in normalized_source_timestamps_utc
                ],
                list(source_seasonality_factors),
                interpolation_method=settings.interpolation_method,
                aggregation_method=settings.aggregation_method,
                sparse_fallback_policy=settings.sparse_fallback_policy,
                sparse_hour_minimum_observation_count=settings.sparse_hour_minimum_observation_count,
            )
        )
    except Exception as error:
        mapped_error = remap_native_exception_by_type(
            native,
            error,
            _HOURLY_FACTOR_NATIVE_EXCEPTION_TYPES,
        )
        if mapped_error is not None:
            raise mapped_error from error

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

    The helper materializes calibrated event-level business-time rates onto the requested
    fixed-width UTC interval grid. Mean renormalization is enabled by default so `lambda_bar`
    stays responsible for average event-rate level while `s_h` redistributes that level across time.
    """

    exchange_code = require_non_empty_string(
        seasonality_stage_payload,
        "exchange_code",
        "seasonality_stage_payload.exchange_code",
        _seasonality_error_factory,
    )
    timestamp_utc_nanos = require_integer_sequence(
        seasonality_stage_payload,
        "timestamp_utc_nanos",
        "seasonality_stage_payload.timestamp_utc_nanos",
        _seasonality_error_factory,
    )
    business_time_rate_by_event = require_finite_number_sequence(
        seasonality_stage_payload,
        "business_time_rate_by_event",
        "seasonality_stage_payload.business_time_rate_by_event",
        _seasonality_error_factory,
    )

    if len(timestamp_utc_nanos) != len(business_time_rate_by_event):
        raise SeasonalityInputValidationError(
            "seasonality_stage_payload.timestamp_utc_nanos and "
            "seasonality_stage_payload.business_time_rate_by_event must have the same length",
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
        source_seasonality_factors=business_time_rate_by_event,
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
    Materialize known and surprise `nu_news(t)` into interval-aligned `nu_h`.

    Known scheduled-news timestamps are counted per interval and added to the surprise background
    component controlled by baseline and optional interval multipliers.
    UTC interval boundaries remain authoritative through timezone offset transitions.
    """

    native: ModuleType = native_module()

    normalized_range_start_utc = require_datetime_instance_utc(
        range_start_utc,
        "range_start_utc",
        _calendar_invalid_timestamp_error,
    )

    normalized_range_end_utc = require_datetime_instance_utc(
        range_end_utc,
        "range_end_utc",
        _calendar_invalid_timestamp_error,
    )

    interval_duration_nanos = _parse_interval_duration_nanos(interval)

    normalized_known_scheduled_event_timestamps_utc = [
        require_datetime_instance_utc(
            known_scheduled_event_timestamp_utc,
            "known_scheduled_event_timestamps_utc",
            _calendar_invalid_timestamp_error,
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
                settings.surprise_open_baseline_intensity,
                settings.surprise_closed_baseline_intensity,
                known_scheduled_event_timestamps_utc=[
                    known_scheduled_event_timestamp_utc.isoformat()
                    for known_scheduled_event_timestamp_utc in normalized_known_scheduled_event_timestamps_utc
                ],
                surprise_deterministic_time_multiplier_by_interval_index=(
                    surprise_deterministic_time_multiplier_by_interval_index
                ),
            )
        )
    except Exception as error:
        mapped_error = remap_native_exception_by_type(
            native,
            error,
            _HOURLY_FACTOR_NATIVE_EXCEPTION_TYPES,
        )
        if mapped_error is not None:
            raise mapped_error from error

        raise

    return HourlyNewsIntensityMaterialization(native_result)


def _calendar_invalid_timestamp_error(
    field_path: str,
    message: str,
) -> CalendarInvalidTimestampError:
    return CalendarInvalidTimestampError(f"{field_path} {message}")


def _mean_renormalize_hourly_factors(hourly_factors: Sequence[float]) -> tuple[float, ...]:
    if len(hourly_factors) == 0:
        return ()

    mean_hourly_factor = sum(float(value) for value in hourly_factors) / len(hourly_factors)
    if mean_hourly_factor == 0.0:
        return tuple(float(value) for value in hourly_factors)

    return tuple(float(value) / mean_hourly_factor for value in hourly_factors)


def _utc_datetime_from_nanos(timestamp_utc_nanos: int) -> datetime:
    microseconds_since_epoch, _ = divmod(timestamp_utc_nanos, 1_000)
    return _UNIX_EPOCH_UTC + timedelta(microseconds=microseconds_since_epoch)
