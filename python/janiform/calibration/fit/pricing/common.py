from __future__ import annotations

from collections.abc import Sequence
from typing import Any, Optional

from ....contracts import require_non_negative_finite_scalar, require_positive_finite_scalar
from ....contracts.base import require_optional_string_value, require_str_enum_value
from ....pricing import (
    VanillaOptionImpliedVolatilityInversionStatus,
    VanillaOptionPricingFailureStage,
    VanillaOptionPricingStatus,
)
from ....pricing.contracts import _validate_optional_implied_volatility_inversion_outcome
from ....pricing.surface.common import (
    grid_of_non_negative_floats as _surface_grid_of_non_negative_floats,
    grid_of_optional_finite_floats as _surface_grid_of_optional_finite_floats,
    grid_of_positive_floats as _surface_grid_of_positive_floats,
    grid_of_scalars,
    grid_shape as _surface_grid_shape,
    tuple_of_finite_floats as _surface_tuple_of_finite_floats,
    tuple_of_non_negative_floats as _surface_tuple_of_non_negative_floats,
    tuple_of_positive_floats as _surface_tuple_of_positive_floats,
    validate_aligned_count as _surface_validate_aligned_node_count,
    validate_matching_grid_shape as _surface_validate_matching_grid_shape,
)
from ...errors import calibration_error


def tuple_of_positive_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[float, ...]:
    return _surface_tuple_of_positive_floats(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=calibration_error,
    )


def tuple_of_non_negative_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[float, ...]:
    return _surface_tuple_of_non_negative_floats(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=calibration_error,
    )


def tuple_of_finite_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[float, ...]:
    return _surface_tuple_of_finite_floats(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=calibration_error,
    )


def grid_of_positive_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[tuple[float, ...], ...]:
    return _surface_grid_of_positive_floats(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=calibration_error,
    )


def grid_of_non_negative_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[tuple[float, ...], ...]:
    return _surface_grid_of_non_negative_floats(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=calibration_error,
    )


def grid_of_optional_finite_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[tuple[Optional[float], ...], ...]:
    return _surface_grid_of_optional_finite_floats(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=calibration_error,
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
        error_factory=calibration_error,
        scalar_parser=lambda value, location: (
            require_str_enum_value(
                value,
                location,
                calibration_error,
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
        error_factory=calibration_error,
        scalar_parser=lambda value, location: require_str_enum_value(
            value,
            location,
            calibration_error,
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
        error_factory=calibration_error,
        scalar_parser=lambda value, location: (
            require_str_enum_value(
                value,
                location,
                calibration_error,
                enum_type=VanillaOptionPricingFailureStage,
                allowed_values_message=(
                    "must be one of: node_setup, transform_input_build, transform_solve, "
                    "transform_output, characteristic_function_assembly, fourier_pricing, "
                    "risk_neutral_characteristic_function, endogenous_anchor_materialization, "
                    "expected_hawkes_state_update, anchored_local_tilt_solve, "
                    "anchored_recursion_observed_boundary_state, "
                    "anchored_recursion_diagnostics, grid_interpolation_shape, "
                    "response_diagnostics, response_assembly, run_result_assembly, "
                    "forward_law, worker_execution"
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
        error_factory=calibration_error,
        scalar_parser=lambda value, location: require_optional_string_value(
            value,
            location,
            calibration_error,
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
    if pricing_status is VanillaOptionPricingStatus.SUCCEEDED:
        if pricing_failure_stage is not None:
            raise calibration_error(
                f"{field_path}.pricing_failure_stage",
                "must be null when pricing_status=succeeded",
            )
        if pricing_failure_reason is not None:
            raise calibration_error(
                f"{field_path}.pricing_failure_reason",
                "must be null when pricing_status=succeeded",
            )
        require_non_negative_finite_scalar(
            model_price,
            f"{field_path}.model_price",
            calibration_error,
        )
        require_positive_finite_scalar(
            forward_price,
            f"{field_path}.forward_price",
            calibration_error,
        )
        require_positive_finite_scalar(
            discount_factor,
            f"{field_path}.discount_factor",
            calibration_error,
        )
        require_non_negative_finite_scalar(
            boundary_state_norm,
            f"{field_path}.boundary_state_norm",
            calibration_error,
        )
        _validate_optional_implied_volatility_inversion_outcome(
            implied_volatility=implied_volatility,
            implied_volatility_inversion_status=implied_volatility_inversion_status,
            field_path=field_path,
        )
        return

    if pricing_failure_stage is None:
        raise calibration_error(
            f"{field_path}.pricing_failure_stage",
            "must be present when pricing_status=failed",
        )
    if pricing_failure_reason is None or pricing_failure_reason.strip() == "":
        raise calibration_error(
            f"{field_path}.pricing_failure_reason",
            "must be a non-empty string when pricing_status=failed",
        )
    if model_price is not None:
        raise calibration_error(
            f"{field_path}.model_price",
            "must be null when pricing_status=failed",
        )
    if implied_volatility is not None:
        raise calibration_error(
            f"{field_path}.implied_volatility",
            "must be null when pricing_status=failed",
        )
    if implied_volatility_inversion_status is not None:
        raise calibration_error(
            f"{field_path}.implied_volatility_inversion_status",
            "must be null when pricing_status=failed",
        )
    if forward_price is not None:
        raise calibration_error(
            f"{field_path}.forward_price",
            "must be null when pricing_status=failed",
        )
    if discount_factor is not None:
        raise calibration_error(
            f"{field_path}.discount_factor",
            "must be null when pricing_status=failed",
        )
    if boundary_state_norm is not None:
        raise calibration_error(
            f"{field_path}.boundary_state_norm",
            "must be null when pricing_status=failed",
        )


def validate_optional_tuple_pair(
    left_values: Optional[tuple[Any, ...]],
    right_values: Optional[tuple[Any, ...]],
    *,
    left_field_path: str,
    right_field_path: str,
) -> None:
    if (left_values is None) != (right_values is None):
        raise calibration_error(
            left_field_path if left_values is None else right_field_path,
            "must be null when the paired terminal distribution field is null",
        )


def grid_shape(
    values: tuple[tuple[Any, ...], ...],
    *,
    field_path: str,
) -> tuple[int, int]:
    return _surface_grid_shape(
        values,
        field_path=field_path,
        error_factory=calibration_error,
    )


def validate_aligned_node_count(
    values: Sequence[Any],
    expected_count: int,
    *,
    field_path: str,
) -> None:
    _surface_validate_aligned_node_count(
        values,
        expected_count,
        field_path=field_path,
        error_factory=calibration_error,
    )


def validate_matching_grid_shape(
    values: tuple[tuple[Any, ...], ...],
    *,
    grid_shape_value: tuple[int, int],
    field_path: str,
) -> None:
    _surface_validate_matching_grid_shape(
        values,
        grid_shape_value=grid_shape_value,
        field_path=field_path,
        error_factory=calibration_error,
    )
