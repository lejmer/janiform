"""Internal pricing-grid validation helpers."""

from __future__ import annotations

from collections.abc import Sequence
from datetime import UTC, datetime
from typing import Any, Optional

import pandas

from ...._pricing_node_validation import validate_option_pricing_node_payload
from janiform.contracts.collections import (
    grid_of_non_negative_floats as shared_grid_of_non_negative_floats,
    grid_of_optional_finite_floats as shared_grid_of_optional_finite_floats,
    grid_of_positive_floats as shared_grid_of_positive_floats,
    grid_of_scalars,
    grid_shape as shared_grid_shape,
    tuple_of_finite_floats as shared_tuple_of_finite_floats,
    tuple_of_non_negative_floats as shared_tuple_of_non_negative_floats,
    tuple_of_positive_floats as shared_tuple_of_positive_floats,
    validate_aligned_count as shared_validate_aligned_count,
    validate_matching_grid_shape as shared_validate_matching_grid_shape,
)
from janiform.contracts.base import (
    require_non_negative_finite_scalar,
    require_positive_finite_scalar,
)
from janiform.contracts.base import require_optional_string_value, require_str_enum_value
from janiform.pricing import (
    VanillaOptionImpliedVolatilityInversionStatus,
    VanillaOptionPricingFailureStage,
    VanillaOptionPricingStatus,
)
from janiform.pricing.contracts import _validate_optional_implied_volatility_inversion_outcome
from janiform.pricing.errors import _pricing_error


def native_datetime(raw_value: Any, *, field_path: str) -> datetime:
    try:
        return pandas.Timestamp(raw_value).to_pydatetime().astimezone(UTC)
    except (TypeError, ValueError) as error:
        raise _pricing_error(field_path, "must be a UTC timestamp") from error


def optional_float(raw_value: Any) -> Optional[float]:
    if raw_value is None:
        return None

    return float(raw_value)


def tuple_of_positive_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[float, ...]:
    return shared_tuple_of_positive_floats(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=_pricing_error,
    )


def tuple_of_non_negative_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[float, ...]:
    return shared_tuple_of_non_negative_floats(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=_pricing_error,
    )


def tuple_of_finite_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[float, ...]:
    return shared_tuple_of_finite_floats(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=_pricing_error,
    )


def grid_of_positive_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[tuple[float, ...], ...]:
    return shared_grid_of_positive_floats(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=_pricing_error,
    )


def grid_of_non_negative_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[tuple[float, ...], ...]:
    return shared_grid_of_non_negative_floats(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=_pricing_error,
    )


def grid_of_optional_finite_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[tuple[Optional[float], ...], ...]:
    return shared_grid_of_optional_finite_floats(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=_pricing_error,
    )


def grid_of_implied_volatility_inversion_statuses(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[tuple[Optional[VanillaOptionImpliedVolatilityInversionStatus], ...], ...]:
    return grid_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=_pricing_error,
        scalar_parser=lambda value, location: (
            require_str_enum_value(
                value,
                location,
                _pricing_error,
                enum_type=VanillaOptionImpliedVolatilityInversionStatus,
                allowed_values_message=(
                    "must be one of: succeeded, "
                    "price_below_no_arbitrage_lower_bound, "
                    "price_equals_finite_upper_bound, "
                    "price_above_finite_upper_bound"
                ),
            )
            if value is not None
            else None
        ),
    )


def grid_of_pricing_statuses(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[tuple[VanillaOptionPricingStatus, ...], ...]:
    return grid_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=_pricing_error,
        scalar_parser=lambda value, location: require_str_enum_value(
            value,
            location,
            _pricing_error,
            enum_type=VanillaOptionPricingStatus,
            allowed_values_message="must be one of: succeeded, failed",
        ),
    )


def grid_of_optional_failure_stages(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[tuple[Optional[VanillaOptionPricingFailureStage], ...], ...]:
    return grid_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=_pricing_error,
        scalar_parser=lambda value, location: (
            require_str_enum_value(
                value,
                location,
                _pricing_error,
                enum_type=VanillaOptionPricingFailureStage,
                allowed_values_message=(
                    "must be one of: node_setup, pricing_projection, "
                    "risk_neutral_characteristic_function, terminal_affine_measure_change, "
                    "spectral_diagnostics, "
                    "response_diagnostics, response_assembly, run_result_assembly, "
                    "worker_execution"
                ),
            )
            if value is not None
            else None
        ),
    )


def grid_of_optional_strings(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[tuple[Optional[str], ...], ...]:
    return grid_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=_pricing_error,
        scalar_parser=lambda value, location: require_optional_string_value(
            value,
            location,
            _pricing_error,
        ),
    )


def validate_group_node_payload(
    *,
    pricing_status: VanillaOptionPricingStatus,
    pricing_failure_stage: Optional[VanillaOptionPricingFailureStage],
    pricing_failure_reason: Optional[str],
    model_price: Optional[float],
    implied_volatility: Optional[float],
    implied_volatility_inversion_status: Optional[VanillaOptionImpliedVolatilityInversionStatus],
    forward_price: Optional[float],
    discount_factor: Optional[float],
    boundary_state_norm: Optional[float],
    field_path: str,
) -> None:
    validate_option_pricing_node_payload(
        pricing_status=pricing_status,
        succeeded_pricing_status=VanillaOptionPricingStatus.SUCCEEDED,
        pricing_failure_stage=pricing_failure_stage,
        pricing_failure_reason=pricing_failure_reason,
        model_price=model_price,
        implied_volatility=implied_volatility,
        implied_volatility_inversion_status=implied_volatility_inversion_status,
        forward_price=forward_price,
        discount_factor=discount_factor,
        boundary_state_norm=boundary_state_norm,
        field_path=field_path,
        error_factory=_pricing_error,
        require_non_negative_finite_numeric_field=_require_non_negative_finite_numeric_field,
        require_positive_finite_numeric_field=_require_positive_finite_numeric_field,
        validate_optional_implied_volatility_inversion_outcome=(
            _validate_optional_implied_volatility_inversion_outcome_for_group_node
        ),
    )


def _require_non_negative_finite_numeric_field(
    value: Optional[float],
    field_path: str,
) -> None:
    require_non_negative_finite_scalar(value, field_path, _pricing_error)


def _require_positive_finite_numeric_field(
    value: Optional[float],
    field_path: str,
) -> None:
    require_positive_finite_scalar(value, field_path, _pricing_error)


def _validate_optional_implied_volatility_inversion_outcome_for_group_node(
    implied_volatility: Optional[float],
    implied_volatility_inversion_status: Optional[VanillaOptionImpliedVolatilityInversionStatus],
    field_path: str,
) -> None:
    _validate_optional_implied_volatility_inversion_outcome(
        implied_volatility=implied_volatility,
        implied_volatility_inversion_status=implied_volatility_inversion_status,
        field_path=field_path,
    )


def validate_optional_tuple_pair(
    left_values: Optional[tuple[Any, ...]],
    right_values: Optional[tuple[Any, ...]],
    *,
    left_field_path: str,
    right_field_path: str,
) -> None:
    if (left_values is None) != (right_values is None):
        raise _pricing_error(
            left_field_path if left_values is None else right_field_path,
            "must be null when the paired terminal distribution field is null",
        )


def grid_shape(
    values: tuple[tuple[Any, ...], ...],
    *,
    field_path: str,
) -> tuple[int, int]:
    return shared_grid_shape(
        values,
        field_path=field_path,
        error_factory=_pricing_error,
    )


def validate_aligned_node_count(
    values: Sequence[Any],
    expected_count: int,
    *,
    field_path: str,
) -> None:
    shared_validate_aligned_count(
        values,
        expected_count,
        field_path=field_path,
        error_factory=_pricing_error,
    )


def validate_matching_grid_shape(
    values: tuple[tuple[Any, ...], ...],
    *,
    grid_shape_value: tuple[int, int],
    field_path: str,
) -> None:
    shared_validate_matching_grid_shape(
        values,
        grid_shape_value=grid_shape_value,
        field_path=field_path,
        error_factory=_pricing_error,
    )
