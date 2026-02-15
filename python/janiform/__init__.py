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
from .seasonality import (
    INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
    PREPROCESSED_TIMESTAMP_EVENT_SCHEMA_VERSION,
    IntradaySeasonalityEstimate,
    IntradaySeasonalityEstimatorSettings,
    IntradaySmoothingKernel,
    SessionFilter,
    SeasonalityInputValidationError,
    SeasonalityPreprocessingError,
    TIMESTAMP_EVENT_INPUT_SCHEMA,
    TIMESTAMP_EVENT_INPUT_SCHEMA_VERSION,
    TimestampEventInputSchema,
    TimestampEventPreprocessingResult,
    estimate_intraday_seasonality,
    preprocess_timestamp_events,
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
    "INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION",
    "IntradaySeasonalityEstimate",
    "IntradaySeasonalityEstimatorSettings",
    "IntradaySmoothingKernel",
    "MarketState",
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
    "resolve_market_state",
]
