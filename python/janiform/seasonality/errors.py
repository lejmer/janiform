"""Public seasonality error types."""


class SeasonalityPreprocessingError(Exception):
    """Base class for seasonality preprocessing failures."""


class SeasonalityInputValidationError(SeasonalityPreprocessingError):
    """Raised when event records violate the timestamp input contract."""
