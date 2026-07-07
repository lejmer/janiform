"""Shared option-pricing node payload validation helpers."""

from __future__ import annotations

from typing import Any, Callable, Optional

from ._pricing_field_names import (
    BOUNDARY_STATE_NORM_FIELD_NAME,
    DISCOUNT_FACTOR_FIELD_NAME,
    FORWARD_PRICE_FIELD_NAME,
    IMPLIED_VOLATILITY_FIELD_NAME,
    IMPLIED_VOLATILITY_INVERSION_STATUS_FIELD_NAME,
    MODEL_PRICE_FIELD_NAME,
    PRICING_FAILURE_REASON_FIELD_NAME,
    PRICING_FAILURE_STAGE_FIELD_NAME,
    PRICING_STATUS_FIELD_NAME,
)


type NumericFieldValidator = Callable[[Optional[float], str], None]
type OptionalImpliedVolatilityOutcomeValidator = Callable[[Optional[float], Optional[Any], str], None]
type ValidationErrorFactory = Callable[[str, str], Exception]


def validate_option_pricing_node_payload(
    *,
    pricing_status: Any,
    succeeded_pricing_status: Any,
    pricing_failure_stage: Optional[Any],
    pricing_failure_reason: Optional[str],
    model_price: Optional[float],
    implied_volatility: Optional[float],
    implied_volatility_inversion_status: Optional[Any],
    forward_price: Optional[float],
    discount_factor: Optional[float],
    boundary_state_norm: Optional[float],
    field_path: str,
    error_factory: ValidationErrorFactory,
    require_non_negative_finite_numeric_field: NumericFieldValidator,
    require_positive_finite_numeric_field: NumericFieldValidator,
    validate_optional_implied_volatility_inversion_outcome: OptionalImpliedVolatilityOutcomeValidator,
) -> None:
    if pricing_status is succeeded_pricing_status:
        if pricing_failure_stage is not None:
            raise error_factory(
                f"{field_path}.{PRICING_FAILURE_STAGE_FIELD_NAME}",
                f"must be null when {PRICING_STATUS_FIELD_NAME}=succeeded",
            )

        if pricing_failure_reason is not None:
            raise error_factory(
                f"{field_path}.{PRICING_FAILURE_REASON_FIELD_NAME}",
                f"must be null when {PRICING_STATUS_FIELD_NAME}=succeeded",
            )

        require_non_negative_finite_numeric_field(
            model_price,
            f"{field_path}.{MODEL_PRICE_FIELD_NAME}",
        )
        require_positive_finite_numeric_field(
            forward_price,
            f"{field_path}.{FORWARD_PRICE_FIELD_NAME}",
        )
        require_positive_finite_numeric_field(
            discount_factor,
            f"{field_path}.{DISCOUNT_FACTOR_FIELD_NAME}",
        )
        require_non_negative_finite_numeric_field(
            boundary_state_norm,
            f"{field_path}.{BOUNDARY_STATE_NORM_FIELD_NAME}",
        )
        validate_optional_implied_volatility_inversion_outcome(
            implied_volatility,
            implied_volatility_inversion_status,
            field_path,
        )

        return

    if pricing_failure_stage is None:
        raise error_factory(
            f"{field_path}.{PRICING_FAILURE_STAGE_FIELD_NAME}",
            f"must be present when {PRICING_STATUS_FIELD_NAME}=failed",
        )

    if pricing_failure_reason is None or pricing_failure_reason.strip() == "":
        raise error_factory(
            f"{field_path}.{PRICING_FAILURE_REASON_FIELD_NAME}",
            f"must be a non-empty string when {PRICING_STATUS_FIELD_NAME}=failed",
        )

    if model_price is not None:
        raise error_factory(
            f"{field_path}.{MODEL_PRICE_FIELD_NAME}",
            f"must be null when {PRICING_STATUS_FIELD_NAME}=failed",
        )

    if implied_volatility is not None:
        raise error_factory(
            f"{field_path}.{IMPLIED_VOLATILITY_FIELD_NAME}",
            f"must be null when {PRICING_STATUS_FIELD_NAME}=failed",
        )

    if implied_volatility_inversion_status is not None:
        raise error_factory(
            f"{field_path}.{IMPLIED_VOLATILITY_INVERSION_STATUS_FIELD_NAME}",
            f"must be null when {PRICING_STATUS_FIELD_NAME}=failed",
        )

    if forward_price is not None:
        raise error_factory(
            f"{field_path}.{FORWARD_PRICE_FIELD_NAME}",
            f"must be null when {PRICING_STATUS_FIELD_NAME}=failed",
        )

    if discount_factor is not None:
        raise error_factory(
            f"{field_path}.{DISCOUNT_FACTOR_FIELD_NAME}",
            f"must be null when {PRICING_STATUS_FIELD_NAME}=failed",
        )

    if boundary_state_norm is not None:
        raise error_factory(
            f"{field_path}.{BOUNDARY_STATE_NORM_FIELD_NAME}",
            f"must be null when {PRICING_STATUS_FIELD_NAME}=failed",
        )
