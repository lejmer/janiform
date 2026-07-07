from ..contracts.base import ValidationErrorFactory
from ..seasonality import SeasonalityInputValidationError


def validation_error(field_path: str, message: str) -> SeasonalityInputValidationError:
    return SeasonalityInputValidationError(f"{field_path} {message}")


VALIDATION_ERROR_FACTORY: ValidationErrorFactory = validation_error
