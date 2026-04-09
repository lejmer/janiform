"""Public option-surface pricing contracts."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import date, datetime
from typing import Any, Optional

from ....contracts.base import (
    ContractModel,
    require_finite_number,
    require_finite_scalar,
    require_non_negative_integer,
    require_positive_finite_scalar,
    require_optional_finite_number,
    require_sequence,
    require_str_enum_value,
)
from ....pricing import (
    PricingIntervalSchedule,
    VanillaOptionImpliedVolatilityInversionStatus,
    VanillaOptionPayoffType,
    VanillaOptionPricingFailureStage,
    VanillaOptionPricingStatus,
)
from ...errors import calibration_error
from .internal.common import (
    grid_of_implied_volatility_inversion_statuses,
    grid_of_optional_failure_stages,
    grid_of_optional_finite_floats,
    grid_of_optional_strings,
    grid_of_pricing_statuses,
    grid_shape,
    tuple_of_positive_floats,
    validate_aligned_node_count,
    validate_group_node_payload,
    validate_matching_grid_shape,
)
from .internal.maturity import (
    normalize_optional_expiry_dates,
    normalize_optional_expiry_timestamps_utc,
    normalize_optional_tenors,
    validate_pricing_maturity_axis_fields,
)
from .summaries import JaniformVanillaOptionMaturitySummary


@dataclass(frozen=True, slots=True)
class JaniformVanillaOptionSurface(ContractModel):
    spot: float
    strikes: tuple[float, ...]
    expiry_timestamps_utc: Optional[tuple[Optional[datetime], ...]] = None
    expiry_dates: Optional[tuple[Optional[date], ...]] = None
    tenors: Optional[tuple[Optional[str], ...]] = None
    payoff_type: VanillaOptionPayoffType = VanillaOptionPayoffType.CALL
    risk_free_rate: float = 0.0
    dividend_yield: float = 0.0

    def __post_init__(self) -> None:
        require_positive_finite_scalar(self.spot, "spot", calibration_error)

        if len(self.strikes) == 0:
            raise calibration_error("strikes", "must contain at least one strike")

        require_finite_scalar(self.risk_free_rate, "risk_free_rate", calibration_error)
        require_finite_scalar(self.dividend_yield, "dividend_yield", calibration_error)

        for strike_index, strike in enumerate(self.strikes):
            require_positive_finite_scalar(strike, f"strikes[{strike_index}]", calibration_error)

        validate_pricing_maturity_axis_fields(
            expiry_timestamps_utc=self.expiry_timestamps_utc,
            expiry_dates=self.expiry_dates,
            tenors=self.tenors,
        )

    @property
    def shape(self) -> tuple[int, int]:
        return (
            validate_pricing_maturity_axis_fields(
                expiry_timestamps_utc=self.expiry_timestamps_utc,
                expiry_dates=self.expiry_dates,
                tenors=self.tenors,
            ),
            len(self.strikes),
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "surface",
    ) -> JaniformVanillaOptionSurface:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        expiry_timestamps_utc = normalize_optional_expiry_timestamps_utc(
            raw_payload.get("expiry_timestamps_utc"),
            field_path=f"{field_path}.expiry_timestamps_utc",
        )
        expiry_dates = normalize_optional_expiry_dates(
            raw_payload.get("expiry_dates"),
            field_path=f"{field_path}.expiry_dates",
        )
        tenors = normalize_optional_tenors(
            raw_payload.get("tenors"),
            field_path=f"{field_path}.tenors",
        )

        validate_pricing_maturity_axis_fields(
            expiry_timestamps_utc=expiry_timestamps_utc,
            expiry_dates=expiry_dates,
            tenors=tenors,
            field_prefix=field_path,
        )

        return JaniformVanillaOptionSurface(
            spot=require_positive_finite_scalar(
                raw_payload.get("spot"),
                f"{field_path}.spot",
                calibration_error,
            ),
            strikes=tuple_of_positive_floats(
                raw_payload,
                field_name="strikes",
                field_path=f"{field_path}.strikes",
            ),
            expiry_timestamps_utc=expiry_timestamps_utc,
            expiry_dates=expiry_dates,
            tenors=tenors,
            payoff_type=require_str_enum_value(
                raw_payload.get("payoff_type", VanillaOptionPayoffType.CALL.value),
                f"{field_path}.payoff_type",
                calibration_error,
                enum_type=VanillaOptionPayoffType,
                allowed_values_message="must be one of: call, put",
            ),
            risk_free_rate=require_optional_finite_number(
                raw_payload,
                "risk_free_rate",
                f"{field_path}.risk_free_rate",
                calibration_error,
                default_value=0.0,
            ),
            dividend_yield=require_optional_finite_number(
                raw_payload,
                "dividend_yield",
                f"{field_path}.dividend_yield",
                calibration_error,
                default_value=0.0,
            ),
        )


@dataclass(frozen=True, slots=True)
class JaniformVanillaOptionSurfacePricingResult(ContractModel):
    spot: float
    strikes: tuple[float, ...]
    maturity_intervals: tuple[float, ...]
    payoff_type: VanillaOptionPayoffType
    risk_free_rate: float
    dividend_yield: float
    price_grid: tuple[tuple[Optional[float], ...], ...]
    implied_volatility_grid: tuple[tuple[Optional[float], ...], ...]
    implied_volatility_inversion_status_grid: tuple[
        tuple[Optional[VanillaOptionImpliedVolatilityInversionStatus], ...],
        ...,
    ]
    pricing_status_grid: tuple[tuple[VanillaOptionPricingStatus, ...], ...]
    pricing_failure_stage_grid: tuple[
        tuple[Optional[VanillaOptionPricingFailureStage], ...],
        ...,
    ]
    pricing_failure_reason_grid: tuple[tuple[Optional[str], ...], ...]
    forward_price_grid: tuple[tuple[Optional[float], ...], ...]
    discount_factor_grid: tuple[tuple[Optional[float], ...], ...]
    boundary_state_norm_grid: tuple[tuple[Optional[float], ...], ...]
    maturity_summaries: tuple[Optional[JaniformVanillaOptionMaturitySummary], ...]
    runtime_interval_schedule: PricingIntervalSchedule
    maturity_terminal_interval_indices: tuple[int, ...]

    def __post_init__(self) -> None:
        require_positive_finite_scalar(self.spot, "spot", calibration_error)
        require_finite_scalar(self.risk_free_rate, "risk_free_rate", calibration_error)
        require_finite_scalar(self.dividend_yield, "dividend_yield", calibration_error)

        expected_shape = (len(self.maturity_intervals), len(self.strikes))
        actual_shape = grid_shape(self.price_grid, field_path="price_grid")
        if actual_shape != expected_shape:
            raise calibration_error(
                "price_grid",
                f"must have shape {expected_shape}, found {actual_shape}",
            )

        validate_matching_grid_shape(self.implied_volatility_grid, grid_shape_value=expected_shape, field_path="implied_volatility_grid")
        validate_matching_grid_shape(self.implied_volatility_inversion_status_grid, grid_shape_value=expected_shape, field_path="implied_volatility_inversion_status_grid")
        validate_matching_grid_shape(self.pricing_status_grid, grid_shape_value=expected_shape, field_path="pricing_status_grid")
        validate_matching_grid_shape(self.pricing_failure_stage_grid, grid_shape_value=expected_shape, field_path="pricing_failure_stage_grid")
        validate_matching_grid_shape(self.pricing_failure_reason_grid, grid_shape_value=expected_shape, field_path="pricing_failure_reason_grid")
        validate_matching_grid_shape(self.forward_price_grid, grid_shape_value=expected_shape, field_path="forward_price_grid")
        validate_matching_grid_shape(self.discount_factor_grid, grid_shape_value=expected_shape, field_path="discount_factor_grid")
        validate_matching_grid_shape(self.boundary_state_norm_grid, grid_shape_value=expected_shape, field_path="boundary_state_norm_grid")
        validate_aligned_node_count(self.maturity_summaries, len(self.maturity_intervals), field_path="maturity_summaries")
        validate_aligned_node_count(
            self.maturity_terminal_interval_indices,
            len(self.maturity_intervals),
            field_path="maturity_terminal_interval_indices",
        )

        if not isinstance(self.runtime_interval_schedule, PricingIntervalSchedule):
            raise calibration_error(
                "runtime_interval_schedule",
                "must be a PricingIntervalSchedule",
            )

        for maturity_index, implied_volatility_row in enumerate(self.implied_volatility_grid):
            for strike_index, implied_volatility in enumerate(implied_volatility_row):
                validate_group_node_payload(
                    pricing_status=self.pricing_status_grid[maturity_index][strike_index],
                    pricing_failure_stage=self.pricing_failure_stage_grid[maturity_index][strike_index],
                    pricing_failure_reason=self.pricing_failure_reason_grid[maturity_index][strike_index],
                    model_price=self.price_grid[maturity_index][strike_index],
                    implied_volatility=implied_volatility,
                    implied_volatility_inversion_status=self.implied_volatility_inversion_status_grid[maturity_index][strike_index],
                    forward_price=self.forward_price_grid[maturity_index][strike_index],
                    discount_factor=self.discount_factor_grid[maturity_index][strike_index],
                    boundary_state_norm=self.boundary_state_norm_grid[maturity_index][strike_index],
                    field_path=f"surface_pricing_result.node[{maturity_index}][{strike_index}]",
                )

    @property
    def shape(self) -> tuple[int, int]:
        return len(self.maturity_intervals), len(self.strikes)

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "surface_pricing_result",
    ) -> JaniformVanillaOptionSurfacePricingResult:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        from ....contracts.base import require_mapping

        response = require_mapping(
            raw_payload,
            "response",
            f"{field_path}.response",
            calibration_error,
        )
        raw_maturity_summaries = require_sequence(
            raw_payload,
            "maturity_summaries",
            f"{field_path}.maturity_summaries",
            calibration_error,
        )
        raw_maturity_terminal_interval_indices = require_sequence(
            raw_payload,
            "maturity_terminal_interval_indices",
            f"{field_path}.maturity_terminal_interval_indices",
            calibration_error,
        )

        return JaniformVanillaOptionSurfacePricingResult(
            spot=require_positive_finite_scalar(response.get("spot"), f"{field_path}.response.spot", calibration_error),
            strikes=tuple_of_positive_floats(response, field_name="strikes", field_path=f"{field_path}.response.strikes"),
            maturity_intervals=tuple_of_positive_floats(response, field_name="maturity_intervals", field_path=f"{field_path}.response.maturity_intervals"),
            payoff_type=require_str_enum_value(
                response.get("payoff_type"),
                f"{field_path}.response.payoff_type",
                calibration_error,
                enum_type=VanillaOptionPayoffType,
                allowed_values_message="must be one of: call, put",
            ),
            risk_free_rate=require_finite_number(response, "risk_free_rate", f"{field_path}.response.risk_free_rate", calibration_error),
            dividend_yield=require_finite_number(response, "dividend_yield", f"{field_path}.response.dividend_yield", calibration_error),
            price_grid=grid_of_optional_finite_floats(response, field_name="price_grid", field_path=f"{field_path}.response.price_grid"),
            implied_volatility_grid=grid_of_optional_finite_floats(response, field_name="implied_volatility_grid", field_path=f"{field_path}.response.implied_volatility_grid"),
            implied_volatility_inversion_status_grid=grid_of_implied_volatility_inversion_statuses(response, field_name="implied_volatility_inversion_status_grid", field_path=f"{field_path}.response.implied_volatility_inversion_status_grid"),
            pricing_status_grid=grid_of_pricing_statuses(response, field_name="pricing_status_grid", field_path=f"{field_path}.response.pricing_status_grid"),
            pricing_failure_stage_grid=grid_of_optional_failure_stages(response, field_name="pricing_failure_stage_grid", field_path=f"{field_path}.response.pricing_failure_stage_grid"),
            pricing_failure_reason_grid=grid_of_optional_strings(response, field_name="pricing_failure_reason_grid", field_path=f"{field_path}.response.pricing_failure_reason_grid"),
            forward_price_grid=grid_of_optional_finite_floats(response, field_name="forward_price_grid", field_path=f"{field_path}.response.forward_price_grid"),
            discount_factor_grid=grid_of_optional_finite_floats(response, field_name="discount_factor_grid", field_path=f"{field_path}.response.discount_factor_grid"),
            boundary_state_norm_grid=grid_of_optional_finite_floats(response, field_name="boundary_state_norm_grid", field_path=f"{field_path}.response.boundary_state_norm_grid"),
            maturity_summaries=tuple(
                (
                    JaniformVanillaOptionMaturitySummary.from_mapping(
                        raw_summary,
                        field_path=f"{field_path}.maturity_summaries[{summary_index}]",
                    )
                    if raw_summary is not None
                    else None
                )
                for summary_index, raw_summary in enumerate(raw_maturity_summaries)
            ),
            runtime_interval_schedule=PricingIntervalSchedule.from_mapping(
                require_mapping(
                    raw_payload,
                    "runtime_interval_schedule",
                    f"{field_path}.runtime_interval_schedule",
                    calibration_error,
                ),
                field_path=f"{field_path}.runtime_interval_schedule",
            ),
            maturity_terminal_interval_indices=tuple(
                require_non_negative_integer(
                    {"value": raw_index},
                    "value",
                    f"{field_path}.maturity_terminal_interval_indices[{index}]",
                    calibration_error,
                )
                for index, raw_index in enumerate(raw_maturity_terminal_interval_indices)
            ),
        )


__all__ = [
    "JaniformVanillaOptionSurface",
    "JaniformVanillaOptionSurfacePricingResult",
]
