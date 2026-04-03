"""Public hourly-factor error types."""


class HourlyFactorProviderError(Exception):
    """Base class for shared hourly factor provider failures."""


class HourlyFactorProviderMissingInputError(HourlyFactorProviderError):
    """Raised when required seasonality or news-intensity source input is missing."""


class HourlyFactorProviderUnsupportedRangeError(HourlyFactorProviderError):
    """Raised when the requested range is unsupported by hourly interval rules."""
