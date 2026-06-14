"""Timestamp preprocessing entrypoints for calendar-aligned event data."""

from __future__ import annotations

from collections.abc import Mapping, Sequence
from types import ModuleType
from typing import Any

from .._native_api import native_module, remap_native_exception_by_type
from ..calendar import (
    CalendarInvalidTimezoneError,
    CalendarMissingExchangeError,
    CalendarRegistry,
)
from .artifacts import TimestampEventPreprocessingResult
from .contracts import SessionFilter
from .errors import SeasonalityInputValidationError

_SEASONALITY_PREPROCESS_NATIVE_EXCEPTION_TYPES: dict[str, type[Exception]] = {
    "SeasonalityInputValidationError": SeasonalityInputValidationError,
    "CalendarMissingExchangeError": CalendarMissingExchangeError,
    "CalendarInvalidTimezoneError": CalendarInvalidTimezoneError,
}


def preprocess_timestamp_events(
    event_records: Sequence[Mapping[str, Any]],
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
    except Exception as error:
        mapped_error = remap_native_exception_by_type(
            native,
            error,
            _SEASONALITY_PREPROCESS_NATIVE_EXCEPTION_TYPES,
        )
        if mapped_error is not None:
            raise mapped_error from error

        raise

    return TimestampEventPreprocessingResult(native_result)


__all__ = ["preprocess_timestamp_events"]
