from importlib.metadata import PackageNotFoundError, version

from .calendar import (
    CalendarError,
    CalendarInvalidTimestampError,
    CalendarInvalidTimezoneError,
    CalendarMissingExchangeError,
    CalendarRegistry,
    CalendarRegistryLoadError,
    CalendarResolveError,
    MarketState,
    resolve_market_state,
)


try:
    __version__: str = version("janiform")
except PackageNotFoundError:
    __version__ = "0.0.0"


__all__ = [
    "__version__",
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
