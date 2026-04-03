"""Public risk error types."""

from __future__ import annotations

from typing import Optional

from ..contracts import ValidationErrorFactory
from ..errors import ContractValidationFailure, JaniformValidationError

RISK_VALIDATION_ERROR_ID = "risk.validation.contract"


class SufficientStatisticsRiskValidationError(JaniformValidationError):
    pass


def _risk_error(
    field_path: str,
    message: str,
    *,
    entrypoint: str = "contract",
    error_id: str = RISK_VALIDATION_ERROR_ID,
    context: Optional[dict[str, Optional[str | int | float | bool]]] = None,
) -> SufficientStatisticsRiskValidationError:
    return SufficientStatisticsRiskValidationError(
        ContractValidationFailure.from_context(
            error_id=error_id,
            workflow="risk",
            entrypoint=entrypoint,
            field_path=field_path,
            message=message,
            context=context,
        )
    )


RISK_ERROR_FACTORY: ValidationErrorFactory = _risk_error
