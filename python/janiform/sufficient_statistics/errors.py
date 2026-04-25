"""Public sufficient-statistics error types."""

from __future__ import annotations

from typing import Optional

from ..contracts import ValidationErrorFactory
from ..errors import ContractValidationFailure, JaniformValidationError

SUFFICIENT_STATISTICS_VALIDATION_ERROR_ID = "sufficient_statistics.validation.contract"


class SufficientStatisticsValidationError(JaniformValidationError):
    pass


def _sufficient_statistics_error(
    field_path: str,
    message: str,
    *,
    entrypoint: str = "contract",
    error_id: str = SUFFICIENT_STATISTICS_VALIDATION_ERROR_ID,
    context: Optional[dict[str, Optional[str | int | float | bool]]] = None,
) -> SufficientStatisticsValidationError:
    return SufficientStatisticsValidationError(
        ContractValidationFailure.from_context(
            error_id=error_id,
            workflow="sufficient_statistics",
            entrypoint=entrypoint,
            field_path=field_path,
            message=message,
            context=context,
        )
    )


SUFFICIENT_STATISTICS_ERROR_FACTORY: ValidationErrorFactory = _sufficient_statistics_error
