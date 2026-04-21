"""Public option-grid pricing contracts."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import date, datetime
from typing import Any, Optional

from ....contracts.base import (
    ContractModel,
    require_finite_number,
    require_finite_scalar,
    require_mapping,
    require_non_negative_integer,
    require_positive_finite_scalar,
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
from ....pricing._moneyness import calculate_optional_log_forward_moneyness
from ...errors import calibration_error
from ._field_names import (
    BOUNDARY_STATE_NORM_GRID_FIELD_NAME,
    DISCOUNT_FACTOR_GRID_FIELD_NAME,
    DIVIDEND_YIELD_FIELD_NAME,
    EXPIRY_DATES_FIELD_NAME,
    EXPIRY_TIMESTAMPS_UTC_FIELD_NAME,
    FORWARD_PRICE_GRID_FIELD_NAME,
    GRIDS_FIELD_NAME,
    IMPLIED_VOLATILITY_GRID_FIELD_NAME,
    IMPLIED_VOLATILITY_INVERSION_STATUS_GRID_FIELD_NAME,
    MATURITY_INTERVALS_FIELD_NAME,
    MATURITY_SUMMARIES_FIELD_NAME,
    MATURITY_TERMINAL_INTERVAL_INDICES_FIELD_NAME,
    PAYOFF_TYPE_FIELD_NAME,
    PRICE_GRID_FIELD_NAME,
    PRICING_FAILURE_REASON_GRID_FIELD_NAME,
    PRICING_FAILURE_STAGE_GRID_FIELD_NAME,
    PRICING_STATUS_GRID_FIELD_NAME,
    RISK_FREE_RATE_FIELD_NAME,
    RUNTIME_INTERVAL_SCHEDULE_FIELD_NAME,
    SPOT_FIELD_NAME,
    STRIKES_FIELD_NAME,
    TENORS_FIELD_NAME,
    VALUE_FIELD_NAME,
)
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
class JaniformVanillaOptionGrid(ContractModel):
    payoff_type: VanillaOptionPayoffType
    risk_free_rate: float
    dividend_yield: float
    strikes: tuple[float, ...]
    expiry_timestamps_utc: Optional[tuple[Optional[datetime], ...]] = None
    expiry_dates: Optional[tuple[Optional[date], ...]] = None
    tenors: Optional[tuple[Optional[str], ...]] = None

    def __post_init__(self) -> None:
        if len(self.strikes) == 0:
            raise calibration_error(STRIKES_FIELD_NAME, "must contain at least one strike")

        require_finite_scalar(self.risk_free_rate, RISK_FREE_RATE_FIELD_NAME, calibration_error)
        require_finite_scalar(self.dividend_yield, DIVIDEND_YIELD_FIELD_NAME, calibration_error)

        for strike_index, strike in enumerate(self.strikes):
            require_positive_finite_scalar(
                strike,
                f"strikes[{strike_index}]",
                calibration_error,
            )

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
        field_path: str = "group",
    ) -> JaniformVanillaOptionGrid:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        expiry_timestamps_utc = normalize_optional_expiry_timestamps_utc(
            raw_payload.get(EXPIRY_TIMESTAMPS_UTC_FIELD_NAME),
            field_path=f"{field_path}.{EXPIRY_TIMESTAMPS_UTC_FIELD_NAME}",
        )
        expiry_dates = normalize_optional_expiry_dates(
            raw_payload.get(EXPIRY_DATES_FIELD_NAME),
            field_path=f"{field_path}.{EXPIRY_DATES_FIELD_NAME}",
        )
        tenors = normalize_optional_tenors(
            raw_payload.get(TENORS_FIELD_NAME),
            field_path=f"{field_path}.{TENORS_FIELD_NAME}",
        )

        validate_pricing_maturity_axis_fields(
            expiry_timestamps_utc=expiry_timestamps_utc,
            expiry_dates=expiry_dates,
            tenors=tenors,
            field_prefix=field_path,
        )

        return JaniformVanillaOptionGrid(
            payoff_type=require_str_enum_value(
                raw_payload.get(PAYOFF_TYPE_FIELD_NAME),
                f"{field_path}.{PAYOFF_TYPE_FIELD_NAME}",
                calibration_error,
                enum_type=VanillaOptionPayoffType,
                allowed_values_message="must be one of: call, put",
            ),
            risk_free_rate=require_finite_number(
                raw_payload,
                RISK_FREE_RATE_FIELD_NAME,
                f"{field_path}.{RISK_FREE_RATE_FIELD_NAME}",
                calibration_error,
            ),
            dividend_yield=require_finite_number(
                raw_payload,
                DIVIDEND_YIELD_FIELD_NAME,
                f"{field_path}.{DIVIDEND_YIELD_FIELD_NAME}",
                calibration_error,
            ),
            strikes=tuple_of_positive_floats(
                raw_payload,
                field_name=STRIKES_FIELD_NAME,
                field_path=f"{field_path}.{STRIKES_FIELD_NAME}",
            ),
            expiry_timestamps_utc=expiry_timestamps_utc,
            expiry_dates=expiry_dates,
            tenors=tenors,
        )


@dataclass(frozen=True, slots=True)
class JaniformVanillaOptionGridBatch(ContractModel):
    spot: float
    grids: tuple[JaniformVanillaOptionGrid, ...]

    def __post_init__(self) -> None:
        require_positive_finite_scalar(self.spot, SPOT_FIELD_NAME, calibration_error)

        if len(self.grids) == 0:
            raise calibration_error(GRIDS_FIELD_NAME, "must contain at least one pricing grid")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "grid_batch",
    ) -> JaniformVanillaOptionGridBatch:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        raw_grids = require_sequence(
            raw_payload,
            GRIDS_FIELD_NAME,
            f"{field_path}.{GRIDS_FIELD_NAME}",
            calibration_error,
        )

        return JaniformVanillaOptionGridBatch(
            spot=require_positive_finite_scalar(
                raw_payload.get(SPOT_FIELD_NAME),
                f"{field_path}.{SPOT_FIELD_NAME}",
                calibration_error,
            ),
            grids=tuple(
                JaniformVanillaOptionGrid.from_mapping(
                    raw_grid,
                    field_path=f"{field_path}.{GRIDS_FIELD_NAME}[{grid_index}]",
                )
                for grid_index, raw_grid in enumerate(raw_grids)
            ),
        )


@dataclass(frozen=True, slots=True)
class JaniformVanillaOptionGridPricingResult(ContractModel):
    payoff_type: VanillaOptionPayoffType
    risk_free_rate: float
    dividend_yield: float
    strikes: tuple[float, ...]
    maturity_intervals: tuple[float, ...]
    maturity_terminal_interval_indices: tuple[int, ...]
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

    def __post_init__(self) -> None:
        require_finite_scalar(self.risk_free_rate, RISK_FREE_RATE_FIELD_NAME, calibration_error)
        require_finite_scalar(self.dividend_yield, DIVIDEND_YIELD_FIELD_NAME, calibration_error)

        expected_shape = (len(self.maturity_intervals), len(self.strikes))
        if len(self.maturity_terminal_interval_indices) != len(self.maturity_intervals):
            raise calibration_error(
                MATURITY_TERMINAL_INTERVAL_INDICES_FIELD_NAME,
                (
                    "must contain one terminal interval index per maturity interval "
                    f"({len(self.maturity_intervals)}), found "
                    f"{len(self.maturity_terminal_interval_indices)}"
                ),
            )

        actual_shape = grid_shape(self.price_grid, field_path=PRICE_GRID_FIELD_NAME)
        if actual_shape != expected_shape:
            raise calibration_error(
                PRICE_GRID_FIELD_NAME,
                f"must have shape {expected_shape}, found {actual_shape}",
            )

        validate_matching_grid_shape(
            self.implied_volatility_grid,
            grid_shape_value=expected_shape,
            field_path=IMPLIED_VOLATILITY_GRID_FIELD_NAME,
        )
        validate_matching_grid_shape(
            self.implied_volatility_inversion_status_grid,
            grid_shape_value=expected_shape,
            field_path=IMPLIED_VOLATILITY_INVERSION_STATUS_GRID_FIELD_NAME,
        )
        validate_matching_grid_shape(
            self.pricing_status_grid,
            grid_shape_value=expected_shape,
            field_path=PRICING_STATUS_GRID_FIELD_NAME,
        )
        validate_matching_grid_shape(
            self.pricing_failure_stage_grid,
            grid_shape_value=expected_shape,
            field_path=PRICING_FAILURE_STAGE_GRID_FIELD_NAME,
        )
        validate_matching_grid_shape(
            self.pricing_failure_reason_grid,
            grid_shape_value=expected_shape,
            field_path=PRICING_FAILURE_REASON_GRID_FIELD_NAME,
        )
        validate_matching_grid_shape(
            self.forward_price_grid,
            grid_shape_value=expected_shape,
            field_path=FORWARD_PRICE_GRID_FIELD_NAME,
        )
        validate_matching_grid_shape(
            self.discount_factor_grid,
            grid_shape_value=expected_shape,
            field_path=DISCOUNT_FACTOR_GRID_FIELD_NAME,
        )
        validate_matching_grid_shape(
            self.boundary_state_norm_grid,
            grid_shape_value=expected_shape,
            field_path=BOUNDARY_STATE_NORM_GRID_FIELD_NAME,
        )
        validate_aligned_node_count(
            self.maturity_summaries,
            len(self.maturity_intervals),
            field_path=MATURITY_SUMMARIES_FIELD_NAME,
        )

        for maturity_index, implied_volatility_row in enumerate(self.implied_volatility_grid):
            for strike_index, implied_volatility in enumerate(implied_volatility_row):
                validate_group_node_payload(
                    pricing_status=self.pricing_status_grid[maturity_index][strike_index],
                    pricing_failure_stage=self.pricing_failure_stage_grid[maturity_index][strike_index],
                    pricing_failure_reason=self.pricing_failure_reason_grid[maturity_index][strike_index],
                    model_price=self.price_grid[maturity_index][strike_index],
                    implied_volatility=implied_volatility,
                    implied_volatility_inversion_status=self.implied_volatility_inversion_status_grid[
                        maturity_index
                    ][strike_index],
                    forward_price=self.forward_price_grid[maturity_index][strike_index],
                    discount_factor=self.discount_factor_grid[maturity_index][strike_index],
                    boundary_state_norm=self.boundary_state_norm_grid[maturity_index][strike_index],
                    field_path=f"grid_batch_pricing_result.grid_node[{maturity_index}][{strike_index}]",
                )

    @property
    def shape(self) -> tuple[int, int]:
        return len(self.maturity_intervals), len(self.strikes)

    @property
    def log_forward_moneyness_grid(self) -> tuple[tuple[Optional[float], ...], ...]:
        return tuple(
            tuple(
                calculate_optional_log_forward_moneyness(
                    strike=strike,
                    forward_price=forward_price,
                )
                for strike, forward_price in zip(
                    self.strikes,
                    forward_price_row,
                    strict=True,
                )
            )
            for forward_price_row in self.forward_price_grid
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> JaniformVanillaOptionGridPricingResult:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        raw_maturity_summaries = require_sequence(
            raw_payload,
            MATURITY_SUMMARIES_FIELD_NAME,
            f"{field_path}.{MATURITY_SUMMARIES_FIELD_NAME}",
            calibration_error,
        )

        return JaniformVanillaOptionGridPricingResult(
            payoff_type=require_str_enum_value(
                raw_payload.get(PAYOFF_TYPE_FIELD_NAME),
                f"{field_path}.{PAYOFF_TYPE_FIELD_NAME}",
                calibration_error,
                enum_type=VanillaOptionPayoffType,
                allowed_values_message="must be one of: call, put",
            ),
            risk_free_rate=require_finite_number(
                raw_payload,
                RISK_FREE_RATE_FIELD_NAME,
                f"{field_path}.{RISK_FREE_RATE_FIELD_NAME}",
                calibration_error,
            ),
            dividend_yield=require_finite_number(
                raw_payload,
                DIVIDEND_YIELD_FIELD_NAME,
                f"{field_path}.{DIVIDEND_YIELD_FIELD_NAME}",
                calibration_error,
            ),
            strikes=tuple_of_positive_floats(
                raw_payload,
                field_name=STRIKES_FIELD_NAME,
                field_path=f"{field_path}.{STRIKES_FIELD_NAME}",
            ),
            maturity_intervals=tuple_of_positive_floats(
                raw_payload,
                field_name=MATURITY_INTERVALS_FIELD_NAME,
                field_path=f"{field_path}.{MATURITY_INTERVALS_FIELD_NAME}",
            ),
            maturity_terminal_interval_indices=tuple(
                require_non_negative_integer(
                    {VALUE_FIELD_NAME: raw_index},
                    VALUE_FIELD_NAME,
                    f"{field_path}.{MATURITY_TERMINAL_INTERVAL_INDICES_FIELD_NAME}[{index}]",
                    calibration_error,
                )
                for index, raw_index in enumerate(
                    require_sequence(
                        raw_payload,
                        MATURITY_TERMINAL_INTERVAL_INDICES_FIELD_NAME,
                        f"{field_path}.{MATURITY_TERMINAL_INTERVAL_INDICES_FIELD_NAME}",
                        calibration_error,
                    )
                )
            ),
            price_grid=grid_of_optional_finite_floats(
                raw_payload,
                field_name=PRICE_GRID_FIELD_NAME,
                field_path=f"{field_path}.{PRICE_GRID_FIELD_NAME}",
            ),
            implied_volatility_grid=grid_of_optional_finite_floats(
                raw_payload,
                field_name=IMPLIED_VOLATILITY_GRID_FIELD_NAME,
                field_path=f"{field_path}.{IMPLIED_VOLATILITY_GRID_FIELD_NAME}",
            ),
            implied_volatility_inversion_status_grid=grid_of_implied_volatility_inversion_statuses(
                raw_payload,
                field_name=IMPLIED_VOLATILITY_INVERSION_STATUS_GRID_FIELD_NAME,
                field_path=f"{field_path}.{IMPLIED_VOLATILITY_INVERSION_STATUS_GRID_FIELD_NAME}",
            ),
            pricing_status_grid=grid_of_pricing_statuses(
                raw_payload,
                field_name=PRICING_STATUS_GRID_FIELD_NAME,
                field_path=f"{field_path}.{PRICING_STATUS_GRID_FIELD_NAME}",
            ),
            pricing_failure_stage_grid=grid_of_optional_failure_stages(
                raw_payload,
                field_name=PRICING_FAILURE_STAGE_GRID_FIELD_NAME,
                field_path=f"{field_path}.{PRICING_FAILURE_STAGE_GRID_FIELD_NAME}",
            ),
            pricing_failure_reason_grid=grid_of_optional_strings(
                raw_payload,
                field_name=PRICING_FAILURE_REASON_GRID_FIELD_NAME,
                field_path=f"{field_path}.{PRICING_FAILURE_REASON_GRID_FIELD_NAME}",
            ),
            forward_price_grid=grid_of_optional_finite_floats(
                raw_payload,
                field_name=FORWARD_PRICE_GRID_FIELD_NAME,
                field_path=f"{field_path}.{FORWARD_PRICE_GRID_FIELD_NAME}",
            ),
            discount_factor_grid=grid_of_optional_finite_floats(
                raw_payload,
                field_name=DISCOUNT_FACTOR_GRID_FIELD_NAME,
                field_path=f"{field_path}.{DISCOUNT_FACTOR_GRID_FIELD_NAME}",
            ),
            boundary_state_norm_grid=grid_of_optional_finite_floats(
                raw_payload,
                field_name=BOUNDARY_STATE_NORM_GRID_FIELD_NAME,
                field_path=f"{field_path}.{BOUNDARY_STATE_NORM_GRID_FIELD_NAME}",
            ),
            maturity_summaries=tuple(
                (
                    JaniformVanillaOptionMaturitySummary.from_mapping(
                        raw_summary,
                        field_path=f"{field_path}.{MATURITY_SUMMARIES_FIELD_NAME}[{summary_index}]",
                    )
                    if raw_summary is not None
                    else None
                )
                for summary_index, raw_summary in enumerate(raw_maturity_summaries)
            ),
        )


@dataclass(frozen=True, slots=True)
class JaniformVanillaOptionGridBatchPricingResult(ContractModel):
    spot: float
    grids: tuple[JaniformVanillaOptionGridPricingResult, ...]
    runtime_interval_schedule: PricingIntervalSchedule

    def __post_init__(self) -> None:
        require_positive_finite_scalar(self.spot, SPOT_FIELD_NAME, calibration_error)

        if len(self.grids) == 0:
            raise calibration_error(GRIDS_FIELD_NAME, "must contain at least one pricing result grid")

        if not isinstance(self.runtime_interval_schedule, PricingIntervalSchedule):
            raise calibration_error(
                RUNTIME_INTERVAL_SCHEDULE_FIELD_NAME,
                "must be a PricingIntervalSchedule",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "grid_batch_pricing_result",
    ) -> JaniformVanillaOptionGridBatchPricingResult:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        raw_grids = require_sequence(
            raw_payload,
            GRIDS_FIELD_NAME,
            f"{field_path}.{GRIDS_FIELD_NAME}",
            calibration_error,
        )

        return JaniformVanillaOptionGridBatchPricingResult(
            spot=require_positive_finite_scalar(
                raw_payload.get(SPOT_FIELD_NAME),
                f"{field_path}.{SPOT_FIELD_NAME}",
                calibration_error,
            ),
            grids=tuple(
                JaniformVanillaOptionGridPricingResult.from_mapping(
                    raw_grid,
                    field_path=f"{field_path}.{GRIDS_FIELD_NAME}[{grid_index}]",
                )
                for grid_index, raw_grid in enumerate(raw_grids)
            ),
            runtime_interval_schedule=PricingIntervalSchedule.from_mapping(
                require_mapping(
                    raw_payload,
                    RUNTIME_INTERVAL_SCHEDULE_FIELD_NAME,
                    f"{field_path}.{RUNTIME_INTERVAL_SCHEDULE_FIELD_NAME}",
                    calibration_error,
                ),
                field_path=f"{field_path}.{RUNTIME_INTERVAL_SCHEDULE_FIELD_NAME}",
            ),
        )


__all__ = [
    "JaniformVanillaOptionGrid",
    "JaniformVanillaOptionGridBatch",
    "JaniformVanillaOptionGridBatchPricingResult",
    "JaniformVanillaOptionGridPricingResult",
]
