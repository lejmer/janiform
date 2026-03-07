from datetime import UTC, datetime
from enum import StrEnum
from types import ModuleType

from ._native_api import native_module


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
            native_registry: object = native.CalendarRegistry(registry_json)
        except native.CalendarRegistryLoadError as e:
            raise CalendarRegistryLoadError(str(e)) from e

        self._native_registry: object = native_registry
        self._native: ModuleType = native

    def resolve_market_state(self, exchange_code: str, timestamp_utc: datetime) -> MarketState:
        normalized_timestamp: datetime = _normalize_utc_timestamp(timestamp_utc)
        native_timestamp: str = normalized_timestamp.isoformat()

        try:
            state_value: str = self._native_registry.resolve_market_state(exchange_code, native_timestamp)
        except self._native.CalendarMissingExchangeError as e:
            raise CalendarMissingExchangeError(str(e)) from e
        except self._native.CalendarInvalidTimezoneError as e:
            raise CalendarInvalidTimezoneError(str(e)) from e
        except self._native.CalendarTimestampParseError as e:
            raise CalendarInvalidTimestampError(str(e)) from e

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
        event_records: object,
        timestamp_field_name: str,
        session_filter: str,
        sparse_hour_event_threshold: int,
    ) -> object:
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


def _normalize_utc_timestamp(timestamp_utc: datetime) -> datetime:
    if not isinstance(timestamp_utc, datetime):
        raise CalendarInvalidTimestampError(
            "timestamp_utc must be a datetime value with timezone information",
        )

    if timestamp_utc.tzinfo is None:
        raise CalendarInvalidTimestampError(
            "timestamp_utc must be timezone-aware",
        )

    return timestamp_utc.astimezone(UTC)


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
