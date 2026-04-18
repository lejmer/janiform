"""Internal maturity-axis resolution helpers for fitted-model pricing."""

from __future__ import annotations

from collections.abc import Callable, Sequence
from dataclasses import dataclass
from datetime import UTC, date, datetime
from types import ModuleType
from typing import Any, Optional

from ....._native_api import native_exception_matches, native_module
from ....._timestamps import datetime_to_utc_nanos
from .....contracts.base import (
    require_date,
    require_datetime_utc,
    require_non_empty_string_value,
)
from .....pricing import (
    AutoActivityCompressedPricingIntervalSchedulePolicy,
    AutoExactSparsePricingIntervalSchedulePolicy,
    PricingIntervalSchedule,
    PricingIntervalSchedulePolicy,
    default_pricing_interval_schedule_policy,
)
from ....errors import (
    CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
    calibration_error,
    calibration_integration_error,
)


type ExpiryTimestampAxis = Optional[tuple[Optional[datetime], ...]]
type ExpiryDateAxis = Optional[tuple[Optional[date], ...]]
type TenorAxis = Optional[tuple[Optional[str], ...]]


@dataclass(frozen=True, slots=True)
class ResolvedPricingMaturityInputs:
    maturity_intervals: tuple[float, ...]
    resolved_expiry_timestamps_utc: tuple[datetime, ...]
    pricing_interval_schedule: PricingIntervalSchedule
    maturity_terminal_interval_indices: tuple[int, ...]


def finalize_runtime_pricing_schedule(
    *,
    resolved_maturity_inputs: ResolvedPricingMaturityInputs,
    hourly_factor_contract: Any,
    pricing_interval_schedule_policy: Optional[PricingIntervalSchedulePolicy],
    field_path: str,
) -> ResolvedPricingMaturityInputs:
    effective_policy = (
        default_pricing_interval_schedule_policy()
        if pricing_interval_schedule_policy is None
        else pricing_interval_schedule_policy
    )

    if not isinstance(
        effective_policy,
        AutoActivityCompressedPricingIntervalSchedulePolicy,
    ):
        return resolved_maturity_inputs

    try:
        native: ModuleType = native_module()
    except ModuleNotFoundError as error:
        raise calibration_integration_error(
            "native pricing module is unavailable",
            entrypoint="pricing_maturity_resolution",
            error_id=CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from error

    request_payload = {
        "valuation_time_utc_nanos": resolved_maturity_inputs.pricing_interval_schedule.valuation_time_utc_nanos,
        "expiry_timestamps_utc_nanos": [
            datetime_to_utc_nanos(expiry_timestamp)
            for expiry_timestamp in resolved_maturity_inputs.resolved_expiry_timestamps_utc
        ],
        "hourly_factor_contract": hourly_factor_contract.to_payload(),
    }

    try:
        raw_resolution = native.derive_activity_compressed_pricing_interval_schedule(
            request_payload
        )
    except Exception as error:
        if native_exception_matches(native, error, "PricingExecutionError"):
            raise calibration_error(field_path, str(error)) from error

        raise calibration_error(field_path, str(error)) from error

    if not isinstance(raw_resolution, dict):
        raise calibration_integration_error(
            "native pricing maturity resolution returned an invalid response payload",
            entrypoint="pricing_maturity_resolution",
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_resolution).__name__},
        )

    pricing_interval_schedule = raw_resolution.get("schedule")
    maturity_terminal_interval_indices = raw_resolution.get(
        "maturity_terminal_interval_indices"
    )
    if not isinstance(pricing_interval_schedule, dict) or not isinstance(
        maturity_terminal_interval_indices,
        list,
    ):
        raise calibration_integration_error(
            "native pricing maturity resolution returned an invalid response payload",
            entrypoint="pricing_maturity_resolution",
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_resolution).__name__},
        )

    return ResolvedPricingMaturityInputs(
        maturity_intervals=resolved_maturity_inputs.maturity_intervals,
        resolved_expiry_timestamps_utc=resolved_maturity_inputs.resolved_expiry_timestamps_utc,
        pricing_interval_schedule=PricingIntervalSchedule.from_mapping(
            pricing_interval_schedule,
            field_path=f"{field_path}.pricing_interval_schedule",
        ),
        maturity_terminal_interval_indices=tuple(
            int(raw_index) for raw_index in maturity_terminal_interval_indices
        ),
    )


def normalize_optional_expiry_timestamps_utc(
    raw_values: Any,
    *,
    field_path: str,
) -> ExpiryTimestampAxis:
    return _normalize_optional_axis(
        raw_values,
        field_path=field_path,
        item_parser=lambda value, location: require_datetime_utc(
            value,
            location,
            calibration_error,
        ),
    )


def normalize_optional_expiry_dates(
    raw_values: Any,
    *,
    field_path: str,
) -> ExpiryDateAxis:
    return _normalize_optional_axis(
        raw_values,
        field_path=field_path,
        item_parser=lambda value, location: require_date(
            value,
            location,
            calibration_error,
        ),
    )


def normalize_optional_tenors(
    raw_values: Any,
    *,
    field_path: str,
) -> TenorAxis:
    return _normalize_optional_axis(
        raw_values,
        field_path=field_path,
        item_parser=lambda value, location: require_non_empty_string_value(
            value,
            location,
            calibration_error,
        ),
    )


def validate_pricing_maturity_axis_fields(
    *,
    expiry_timestamps_utc: ExpiryTimestampAxis,
    expiry_dates: ExpiryDateAxis,
    tenors: TenorAxis,
    field_prefix: Optional[str] = None,
) -> int:
    expiry_timestamp_field_path = _prefixed_field_path(field_prefix, "expiry_timestamps_utc")
    expiry_date_field_path = _prefixed_field_path(field_prefix, "expiry_dates")
    tenor_field_path = _prefixed_field_path(field_prefix, "tenors")

    entry_count: Optional[int] = None
    for field_path, axis in (
        (expiry_timestamp_field_path, expiry_timestamps_utc),
        (expiry_date_field_path, expiry_dates),
        (tenor_field_path, tenors),
    ):
        if axis is None:
            continue

        if len(axis) == 0:
            raise calibration_error(
                field_path,
                "must contain at least one maturity input when provided",
            )

        if entry_count is None:
            entry_count = len(axis)
            continue

        if len(axis) != entry_count:
            raise calibration_error(
                field_path,
                f"must contain {entry_count} entries to match the other maturity input axes, found {len(axis)}",
            )

    if entry_count is None:
        raise calibration_error(
            field_prefix or "maturity_inputs",
            "at least one of expiry_timestamps_utc, expiry_dates, or tenors must be provided",
        )

    for index in range(entry_count):
        selected_count = sum(
            axis is not None and axis[index] is not None
            for axis in (expiry_timestamps_utc, expiry_dates, tenors)
        )
        if selected_count != 1:
            raise calibration_error(
                field_prefix or "maturity_inputs",
                (
                    f"exactly one of {expiry_timestamp_field_path}[{index}], "
                    f"{expiry_date_field_path}[{index}], or {tenor_field_path}[{index}] must be provided"
                ),
            )

    return entry_count


def resolve_single_pricing_maturity_input(
    *,
    valuation_time_utc: datetime,
    expiry_timestamp_utc: Optional[datetime],
    expiry_date: Optional[date],
    tenor: Optional[str],
    pricing_interval_schedule_policy: Optional[PricingIntervalSchedulePolicy] = None,
) -> ResolvedPricingMaturityInputs:
    selected_count = sum(
        value is not None for value in (expiry_timestamp_utc, expiry_date, tenor)
    )
    if selected_count != 1:
        raise calibration_error(
            "maturity_input",
            "exactly one of expiry_timestamp_utc, expiry_date, or tenor must be provided",
        )

    normalized_expiry_timestamp_utc = (
        None
        if expiry_timestamp_utc is None
        else require_datetime_utc(
            expiry_timestamp_utc,
            "expiry_timestamp_utc",
            calibration_error,
        )
    )

    normalized_expiry_date = (
        None if expiry_date is None else require_date(expiry_date, "expiry_date", calibration_error)
    )

    normalized_tenor = (
        None
        if tenor is None
        else require_non_empty_string_value(tenor, "tenor", calibration_error)
    )

    return _resolve_pricing_maturity_inputs(
        valuation_time_utc=valuation_time_utc,
        expiry_timestamps_utc=(
            None
            if normalized_expiry_timestamp_utc is None
            else (normalized_expiry_timestamp_utc,)
        ),
        expiry_dates=None if normalized_expiry_date is None else (normalized_expiry_date,),
        tenors=None if normalized_tenor is None else (normalized_tenor,),
        pricing_interval_schedule_policy=pricing_interval_schedule_policy,
        field_path="maturity_input",
    )


def resolve_pricing_maturity_axis(
    *,
    valuation_time_utc: datetime,
    expiry_timestamps_utc: ExpiryTimestampAxis,
    expiry_dates: ExpiryDateAxis,
    tenors: TenorAxis,
    field_path: str,
    pricing_interval_schedule_policy: Optional[PricingIntervalSchedulePolicy] = None,
) -> ResolvedPricingMaturityInputs:
    validate_pricing_maturity_axis_fields(
        expiry_timestamps_utc=expiry_timestamps_utc,
        expiry_dates=expiry_dates,
        tenors=tenors,
        field_prefix=field_path,
    )

    return _resolve_pricing_maturity_inputs(
        valuation_time_utc=valuation_time_utc,
        expiry_timestamps_utc=expiry_timestamps_utc,
        expiry_dates=expiry_dates,
        tenors=tenors,
        pricing_interval_schedule_policy=pricing_interval_schedule_policy,
        field_path=field_path,
    )


def _normalize_optional_axis[T](
    raw_values: Any,
    *,
    field_path: str,
    item_parser: Callable[[Any, str], T],
) -> Optional[tuple[Optional[T], ...]]:
    if raw_values is None:
        return None

    if isinstance(raw_values, str) or not isinstance(raw_values, Sequence):
        raise calibration_error(field_path, "must be a sequence or null")

    return tuple(
        None
        if raw_value is None
        else item_parser(raw_value, f"{field_path}[{value_index}]")
        for value_index, raw_value in enumerate(raw_values)
    )


def _resolve_pricing_maturity_inputs(
    *,
    valuation_time_utc: datetime,
    expiry_timestamps_utc: ExpiryTimestampAxis,
    expiry_dates: ExpiryDateAxis,
    tenors: TenorAxis,
    pricing_interval_schedule_policy: Optional[PricingIntervalSchedulePolicy],
    field_path: str,
) -> ResolvedPricingMaturityInputs:
    normalized_valuation_time_utc = require_datetime_utc(
        valuation_time_utc,
        "valuation_time_utc",
        calibration_error,
    )

    try:
        native: ModuleType = native_module()
    except ModuleNotFoundError as error:
        raise calibration_integration_error(
            "native pricing module is unavailable",
            entrypoint="pricing_maturity_resolution",
            error_id=CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from error

    request_payload = {
        "valuation_time_utc": normalized_valuation_time_utc.astimezone(UTC).isoformat(),
        "expiry_timestamps_utc": _datetime_axis_payload(expiry_timestamps_utc),
        "expiry_dates": _date_axis_payload(expiry_dates),
        "tenors": _string_axis_payload(tenors),
        "pricing_interval_schedule_policy": (
            _maturity_resolution_policy_payload(
                pricing_interval_schedule_policy
            ).to_payload()
        ),
    }

    try:
        raw_resolution = native.resolve_high_level_pricing_maturity_inputs(request_payload)
    except Exception as error:
        if native_exception_matches(native, error, "PricingExecutionError"):
            raise calibration_error(field_path, str(error)) from error

        raise calibration_error(field_path, str(error)) from error

    if not isinstance(raw_resolution, dict):
        raise calibration_integration_error(
            "native pricing maturity resolution returned an invalid response payload",
            entrypoint="pricing_maturity_resolution",
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_resolution).__name__},
        )

    maturity_intervals = raw_resolution.get("maturity_intervals")
    resolved_expiry_timestamps_utc = raw_resolution.get("resolved_expiry_timestamps_utc")
    pricing_interval_schedule = raw_resolution.get("pricing_interval_schedule")
    maturity_terminal_interval_indices = raw_resolution.get("maturity_terminal_interval_indices")

    if not isinstance(maturity_intervals, list) or not isinstance(
        resolved_expiry_timestamps_utc,
        list,
    ) or not isinstance(pricing_interval_schedule, dict) or not isinstance(
        maturity_terminal_interval_indices,
        list,
    ):
        raise calibration_integration_error(
            "native pricing maturity resolution returned an invalid response payload",
            entrypoint="pricing_maturity_resolution",
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_resolution).__name__},
        )

    return ResolvedPricingMaturityInputs(
        maturity_intervals=tuple(
            float(interval) for interval in maturity_intervals
        ),
        resolved_expiry_timestamps_utc=tuple(
            require_datetime_utc(
                raw_timestamp,
                f"{field_path}.resolved_expiry_timestamps_utc[{index}]",
                calibration_error,
            )
            for index, raw_timestamp in enumerate(resolved_expiry_timestamps_utc)
        ),
        pricing_interval_schedule=PricingIntervalSchedule.from_mapping(
            pricing_interval_schedule,
            field_path=f"{field_path}.pricing_interval_schedule",
        ),
        maturity_terminal_interval_indices=tuple(
            int(raw_index) for raw_index in maturity_terminal_interval_indices
        ),
    )


def _maturity_resolution_policy_payload(
    pricing_interval_schedule_policy: Optional[PricingIntervalSchedulePolicy],
) -> PricingIntervalSchedulePolicy:
    effective_policy = (
        default_pricing_interval_schedule_policy()
        if pricing_interval_schedule_policy is None
        else pricing_interval_schedule_policy
    )

    if isinstance(
        effective_policy,
        AutoActivityCompressedPricingIntervalSchedulePolicy,
    ):
        return AutoExactSparsePricingIntervalSchedulePolicy()

    return effective_policy


def _prefixed_field_path(field_prefix: Optional[str], field_name: str) -> str:
    if field_prefix is None:
        return field_name

    return f"{field_prefix}.{field_name}"


def _datetime_axis_payload(values: ExpiryTimestampAxis) -> Optional[list[Optional[str]]]:
    if values is None:
        return None

    return [
        None if value is None else value.astimezone(UTC).isoformat()
        for value in values
    ]


def _date_axis_payload(values: ExpiryDateAxis) -> Optional[list[Optional[str]]]:
    if values is None:
        return None

    return [None if value is None else value.isoformat() for value in values]


def _string_axis_payload(values: TenorAxis) -> Optional[list[Optional[str]]]:
    if values is None:
        return None

    return list(values)
