"""Public interval-factor-grid error types."""


class IntervalFactorGridProviderError(Exception):
    """Base class for shared interval factor grid provider failures."""


class IntervalFactorGridProviderMissingInputError(IntervalFactorGridProviderError):
    """Raised when required seasonality or news-intensity source input is missing."""


class IntervalFactorGridProviderUnsupportedRangeError(IntervalFactorGridProviderError):
    """Raised when the requested range is unsupported by interval grid rules."""
