"""Public pricing error types."""

from __future__ import annotations

from typing import Optional

from ..contracts import ValidationErrorFactory
from ..errors import (
    ContractValidationFailure,
    JaniformErrorCategory,
    JaniformErrorDetails,
    JaniformExecutionError,
    JaniformIntegrationError,
    JaniformValidationError,
)

PRICING_VALIDATION_ERROR_ID = "pricing.validation.contract"
PRICING_EXECUTION_ERROR_ID = "pricing.execution.run_failed"
PRICING_INTEGRATION_NATIVE_MODULE_ERROR_ID = "pricing.integration.native_module_unavailable"
PRICING_INTEGRATION_RESPONSE_ERROR_ID = "pricing.integration.invalid_native_response"


class VanillaOptionPricingValidationError(JaniformValidationError):
    pass


def _pricing_error(
    field_path: str,
    message: str,
    *,
    entrypoint: str = "contract",
    error_id: str = PRICING_VALIDATION_ERROR_ID,
    context: Optional[dict[str, Optional[str | int | float | bool]]] = None,
) -> VanillaOptionPricingValidationError:
    return VanillaOptionPricingValidationError(
        ContractValidationFailure.from_context(
            error_id=error_id,
            workflow="pricing",
            entrypoint=entrypoint,
            field_path=field_path,
            message=message,
            context=context,
        )
    )


def _pricing_integration_error(
    message: str,
    *,
    entrypoint: str,
    error_id: str,
    field_path: Optional[str] = None,
    context: Optional[dict[str, Optional[str | int | float | bool]]] = None,
) -> JaniformIntegrationError:
    return JaniformIntegrationError(
        JaniformErrorDetails.from_context(
            error_id=error_id,
            category=JaniformErrorCategory.INTEGRATION,
            workflow="pricing",
            entrypoint=entrypoint,
            message=message,
            field_path=field_path,
            context=context,
        )
    )


def _pricing_execution_error(
    message: str,
    *,
    entrypoint: str,
    context: Optional[dict[str, Optional[str | int | float | bool]]] = None,
) -> JaniformExecutionError:
    return JaniformExecutionError(
        JaniformErrorDetails.from_context(
            error_id=PRICING_EXECUTION_ERROR_ID,
            category=JaniformErrorCategory.EXECUTION,
            workflow="pricing",
            entrypoint=entrypoint,
            message=message,
            context=context,
        )
    )


PRICING_ERROR_FACTORY: ValidationErrorFactory = _pricing_error


__all__ = [
    "VanillaOptionPricingValidationError",
]
