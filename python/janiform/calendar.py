from collections.abc import Mapping, Sequence
from datetime import datetime
from enum import StrEnum
from types import ModuleType
from typing import Any

import orjson

from ._native_api import native_module, remap_native_exception_by_type
from .contracts.base import require_datetime_instance_utc


class CalendarError(Exception):
    """Base class for calendar query failures."""


class CalendarRegistryLoadError(CalendarError):
    """Raised when a calendar registry artifact cannot be loaded."""


class CalendarResolveError(CalendarError):
    """Base class for market-state resolver failures."""


class CalendarMissingExchangeError(CalendarResolveError):
    """Raised when the requested exchange identifier does not exist."""


class CalendarInvalidTimezoneError(CalendarResolveError):
    """Raised when calendar configuration contains an invalid timezone name."""


class CalendarInvalidTimestampError(CalendarResolveError):
    """Raised when the timestamp input is invalid for calendar resolution."""


class MarketState(StrEnum):
    OPEN = 'open'
    CLOSED = 'closed'


class CalendarRegistry:
    def __init__(self, registry_json: str) -> None:
        native: ModuleType = native_module()
        try:
            registry_payload: Any = orjson.loads(registry_json)
        except orjson.JSONDecodeError as e:
            raise CalendarRegistryLoadError(f"calendar registry payload must be valid JSON: {e.msg}") from e

        if not isinstance(registry_payload, dict):
            raise CalendarRegistryLoadError("calendar registry payload must decode to an object")

        try:
            native_registry: object = native.CalendarRegistry(registry_json)
        except Exception as e:
            mapped_error = remap_native_exception_by_type(
                native,
                e,
                {"CalendarRegistryLoadError": CalendarRegistryLoadError},
            )
            if mapped_error is not None:
                raise mapped_error from e

            raise

        self._native_registry: object = native_registry
        self._native: ModuleType = native
        self._registry_json: str = registry_json
        self._registry_payload: dict[str, Any] = registry_payload

    def resolve_market_state(self, exchange_code: str, timestamp_utc: datetime) -> MarketState:
        normalized_timestamp = require_datetime_instance_utc(
            timestamp_utc,
            "timestamp_utc",
            _calendar_invalid_timestamp_error,
        )
        native_timestamp: str = normalized_timestamp.isoformat()

        try:
            state_value: str = self._native_registry.resolve_market_state(exchange_code, native_timestamp)
        except Exception as e:
            mapped_error = remap_native_exception_by_type(
                self._native,
                e,
                {
                    "CalendarMissingExchangeError": CalendarMissingExchangeError,
                    "CalendarInvalidTimezoneError": CalendarInvalidTimezoneError,
                    "CalendarTimestampParseError": CalendarInvalidTimestampError,
                },
            )
            if mapped_error is not None:
                raise mapped_error from e

            raise

        match state_value:
            case MarketState.OPEN.value:
                return MarketState.OPEN
            case MarketState.CLOSED.value:
                return MarketState.CLOSED
            case _:
                raise CalendarResolveError(f"unexpected market state value returned by native resolver: {state_value}")

    def _preprocess_timestamp_events_native(
        self,
        exchange_code: str,
        event_records: Sequence[Mapping[str, Any]],
        timestamp_field_name: str,
        session_filter: str,
        sparse_hour_event_threshold: int,
    ) -> Any:
        return self._native_registry.preprocess_timestamp_events(
            exchange_code,
            event_records,
            timestamp_field_name,
            session_filter,
            sparse_hour_event_threshold,
        )


def resolve_market_state(registry_json: str, exchange_code: str, timestamp_utc: datetime) -> MarketState:
    calendar_registry: CalendarRegistry = CalendarRegistry(registry_json)
    return calendar_registry.resolve_market_state(exchange_code, timestamp_utc)


def _calendar_invalid_timestamp_error(
    field_path: str,
    message: str,
) -> CalendarInvalidTimestampError:
    return CalendarInvalidTimestampError(f"{field_path} {message}")


__all__ = [
    "CalendarError",
    "CalendarInvalidTimestampError",
    "CalendarInvalidTimezoneError",
    "CalendarMissingExchangeError",
    "CalendarRegistry",
    "CalendarRegistryLoadError",
    "CalendarResolveError",
    "MarketState",
    "resolve_market_state",
]
