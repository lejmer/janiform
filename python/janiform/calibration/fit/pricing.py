from __future__ import annotations

from collections.abc import Callable, Sequence
from dataclasses import dataclass
from enum import StrEnum
from typing import Any, Optional

from ...contracts import (
    ContractModel,
    require_finite_number,
    require_finite_scalar,
    require_mapping,
    require_non_negative_finite_scalar,
    require_positive_finite_scalar,
    require_sequence,
)
from ...contracts.base import (
    require_optional_finite_number,
    require_optional_mapping_value,
    require_str_enum_value,
)
from ...pricing import (
    VanillaOptionImpliedVolatilityInversionStatus,
    VanillaOptionPayoffType,
)
from ...pricing.contracts import _validate_implied_volatility_inversion_outcome
from .._errors import _calibration_error


class JaniformVanillaOptionMaturityPricingEngineKind(StrEnum):
    DIRECT_FOURIER = "direct_fourier"
    FORWARD_LAW = "forward_law"


@dataclass(frozen=True, slots=True)
class JaniformGroupedVanillaOptionGroup(ContractModel):
    payoff_type: VanillaOptionPayoffType
    risk_free_rate: float
    dividend_yield: float
    strikes: tuple[float, ...]
    maturity_intervals: tuple[float, ...]

    def __post_init__(self) -> None:
        if len(self.strikes) == 0:
            raise _calibration_error("strikes", "must contain at least one strike")

        if len(self.maturity_intervals) == 0:
            raise _calibration_error(
                "maturity_intervals",
                "must contain at least one maturity interval",
            )

        require_finite_scalar(self.risk_free_rate, "risk_free_rate", _calibration_error)
        require_finite_scalar(self.dividend_yield, "dividend_yield", _calibration_error)

        for strike_index, strike in enumerate(self.strikes):
            require_positive_finite_scalar(
                strike,
                f"strikes[{strike_index}]",
                _calibration_error,
            )

        for maturity_index, maturity_interval in enumerate(self.maturity_intervals):
            require_positive_finite_scalar(
                maturity_interval,
                f"maturity_intervals[{maturity_index}]",
                _calibration_error,
            )

    @property
    def shape(self) -> tuple[int, int]:
        return len(self.maturity_intervals), len(self.strikes)

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "group",
    ) -> JaniformGroupedVanillaOptionGroup:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return JaniformGroupedVanillaOptionGroup(
            payoff_type=require_str_enum_value(
                raw_payload.get("payoff_type"),
                f"{field_path}.payoff_type",
                _calibration_error,
                enum_type=VanillaOptionPayoffType,
                allowed_values_message="must be one of: call, put",
            ),
            risk_free_rate=require_finite_number(
                raw_payload,
                "risk_free_rate",
                f"{field_path}.risk_free_rate",
                _calibration_error,
            ),
            dividend_yield=require_finite_number(
                raw_payload,
                "dividend_yield",
                f"{field_path}.dividend_yield",
                _calibration_error,
            ),
            strikes=_tuple_of_positive_floats(
                raw_payload,
                field_name="strikes",
                field_path=f"{field_path}.strikes",
            ),
            maturity_intervals=_tuple_of_positive_floats(
                raw_payload,
                field_name="maturity_intervals",
                field_path=f"{field_path}.maturity_intervals",
            ),
        )


@dataclass(frozen=True, slots=True)
class JaniformGroupedVanillaOptionBatch(ContractModel):
    spot: float
    groups: tuple[JaniformGroupedVanillaOptionGroup, ...]

    def __post_init__(self) -> None:
        require_positive_finite_scalar(self.spot, "spot", _calibration_error)
        if len(self.groups) == 0:
            raise _calibration_error("groups", "must contain at least one pricing group")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "grouped_batch",
    ) -> JaniformGroupedVanillaOptionBatch:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_groups = require_sequence(
            raw_payload,
            "groups",
            f"{field_path}.groups",
            _calibration_error,
        )

        return JaniformGroupedVanillaOptionBatch(
            spot=require_positive_finite_scalar(
                raw_payload.get("spot"),
                f"{field_path}.spot",
                _calibration_error,
            ),
            groups=tuple(
                JaniformGroupedVanillaOptionGroup.from_mapping(
                    raw_group,
                    field_path=f"{field_path}.groups[{group_index}]",
                )
                for group_index, raw_group in enumerate(raw_groups)
            ),
        )


@dataclass(frozen=True, slots=True)
class JaniformVanillaOptionMaturityIntervalSummary(ContractModel):
    interval_index: int
    current_anchor_value: float
    next_anchor_value: float
    interval_delta: float
    expected_hawkes_boundary_state_before: dict[str, float]
    expected_hawkes_boundary_state_after: dict[str, float]
    maximum_tail_probability_mass: float
    maximum_row_probability_mass_error: float
    minimum_solved_theta: float
    maximum_solved_theta: float

    def __post_init__(self) -> None:
        if self.interval_index < 0:
            raise _calibration_error("interval_index", "must be non-negative")

        require_finite_scalar(
            self.current_anchor_value,
            "current_anchor_value",
            _calibration_error,
        )
        require_finite_scalar(
            self.next_anchor_value,
            "next_anchor_value",
            _calibration_error,
        )
        require_positive_finite_scalar(
            self.interval_delta,
            "interval_delta",
            _calibration_error,
        )
        require_non_negative_finite_scalar(
            self.maximum_tail_probability_mass,
            "maximum_tail_probability_mass",
            _calibration_error,
        )
        require_non_negative_finite_scalar(
            self.maximum_row_probability_mass_error,
            "maximum_row_probability_mass_error",
            _calibration_error,
        )
        require_finite_scalar(
            self.minimum_solved_theta,
            "minimum_solved_theta",
            _calibration_error,
        )
        require_finite_scalar(
            self.maximum_solved_theta,
            "maximum_solved_theta",
            _calibration_error,
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> JaniformVanillaOptionMaturityIntervalSummary:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return JaniformVanillaOptionMaturityIntervalSummary(
            interval_index=int(
                require_non_negative_finite_scalar(
                    raw_payload.get("interval_index"),
                    f"{field_path}.interval_index",
                    _calibration_error,
                )
            ),
            current_anchor_value=require_finite_number(
                raw_payload,
                "current_anchor_value",
                f"{field_path}.current_anchor_value",
                _calibration_error,
            ),
            next_anchor_value=require_finite_number(
                raw_payload,
                "next_anchor_value",
                f"{field_path}.next_anchor_value",
                _calibration_error,
            ),
            interval_delta=require_positive_finite_scalar(
                raw_payload.get("interval_delta"),
                f"{field_path}.interval_delta",
                _calibration_error,
            ),
            expected_hawkes_boundary_state_before=require_mapping(
                raw_payload,
                "expected_hawkes_boundary_state_before",
                f"{field_path}.expected_hawkes_boundary_state_before",
                _calibration_error,
            ),
            expected_hawkes_boundary_state_after=require_mapping(
                raw_payload,
                "expected_hawkes_boundary_state_after",
                f"{field_path}.expected_hawkes_boundary_state_after",
                _calibration_error,
            ),
            maximum_tail_probability_mass=require_non_negative_finite_scalar(
                raw_payload.get("maximum_tail_probability_mass"),
                f"{field_path}.maximum_tail_probability_mass",
                _calibration_error,
            ),
            maximum_row_probability_mass_error=require_non_negative_finite_scalar(
                raw_payload.get("maximum_row_probability_mass_error"),
                f"{field_path}.maximum_row_probability_mass_error",
                _calibration_error,
            ),
            minimum_solved_theta=require_finite_number(
                raw_payload,
                "minimum_solved_theta",
                f"{field_path}.minimum_solved_theta",
                _calibration_error,
            ),
            maximum_solved_theta=require_finite_number(
                raw_payload,
                "maximum_solved_theta",
                f"{field_path}.maximum_solved_theta",
                _calibration_error,
            ),
        )


@dataclass(frozen=True, slots=True)
class JaniformVanillaOptionMaturitySummary(ContractModel):
    maturity_interval: float
    engine_kind: JaniformVanillaOptionMaturityPricingEngineKind
    terminal_anchor_value: Optional[float]
    deviation_grid_lower_bound: Optional[float]
    deviation_grid_upper_bound: Optional[float]
    deviation_grid_node_count: Optional[int]
    terminal_deviation_nodes: Optional[tuple[float, ...]]
    terminal_probability_mass: Optional[tuple[float, ...]]
    interval_summaries: tuple[JaniformVanillaOptionMaturityIntervalSummary, ...]
    representative_transform: Optional[dict[str, Any]]
    integration_quality: Optional[dict[str, Any]]

    def __post_init__(self) -> None:
        require_positive_finite_scalar(
            self.maturity_interval,
            "maturity_interval",
            _calibration_error,
        )

        if self.deviation_grid_node_count is not None and self.deviation_grid_node_count <= 0:
            raise _calibration_error(
                "deviation_grid_node_count",
                "must be strictly positive when provided",
            )

        _validate_optional_tuple_pair(
            self.terminal_deviation_nodes,
            self.terminal_probability_mass,
            left_field_path="terminal_deviation_nodes",
            right_field_path="terminal_probability_mass",
        )

        if self.terminal_deviation_nodes is not None:
            _validate_aligned_node_count(
                self.terminal_deviation_nodes,
                len(self.terminal_probability_mass or ()),
                field_path="terminal_deviation_nodes",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> JaniformVanillaOptionMaturitySummary:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_interval_summaries = require_sequence(
            raw_payload,
            "interval_summaries",
            f"{field_path}.interval_summaries",
            _calibration_error,
        )

        raw_deviation_grid_node_count = raw_payload.get("deviation_grid_node_count")
        deviation_grid_node_count = None
        if raw_deviation_grid_node_count is not None:
            deviation_grid_node_count = int(
                require_positive_finite_scalar(
                    raw_deviation_grid_node_count,
                    f"{field_path}.deviation_grid_node_count",
                    _calibration_error,
                )
            )

        return JaniformVanillaOptionMaturitySummary(
            maturity_interval=require_positive_finite_scalar(
                raw_payload.get("maturity_interval"),
                f"{field_path}.maturity_interval",
                _calibration_error,
            ),
            engine_kind=require_str_enum_value(
                raw_payload.get("engine_kind"),
                f"{field_path}.engine_kind",
                _calibration_error,
                enum_type=JaniformVanillaOptionMaturityPricingEngineKind,
                allowed_values_message="must be one of: direct_fourier, forward_law",
            ),
            terminal_anchor_value=require_optional_finite_number(
                raw_payload,
                "terminal_anchor_value",
                f"{field_path}.terminal_anchor_value",
                _calibration_error,
            ),
            deviation_grid_lower_bound=require_optional_finite_number(
                raw_payload,
                "deviation_grid_lower_bound",
                f"{field_path}.deviation_grid_lower_bound",
                _calibration_error,
            ),
            deviation_grid_upper_bound=require_optional_finite_number(
                raw_payload,
                "deviation_grid_upper_bound",
                f"{field_path}.deviation_grid_upper_bound",
                _calibration_error,
            ),
            deviation_grid_node_count=deviation_grid_node_count,
            terminal_deviation_nodes=(
                _tuple_of_finite_floats(
                    raw_payload,
                    field_name="terminal_deviation_nodes",
                    field_path=f"{field_path}.terminal_deviation_nodes",
                )
                if raw_payload.get("terminal_deviation_nodes") is not None
                else None
            ),
            terminal_probability_mass=(
                _tuple_of_non_negative_floats(
                    raw_payload,
                    field_name="terminal_probability_mass",
                    field_path=f"{field_path}.terminal_probability_mass",
                )
                if raw_payload.get("terminal_probability_mass") is not None
                else None
            ),
            interval_summaries=tuple(
                JaniformVanillaOptionMaturityIntervalSummary.from_mapping(
                    raw_interval_summary,
                    field_path=f"{field_path}.interval_summaries[{interval_index}]",
                )
                for interval_index, raw_interval_summary in enumerate(raw_interval_summaries)
            ),
            representative_transform=require_optional_mapping_value(
                raw_payload.get("representative_transform"),
                f"{field_path}.representative_transform",
                _calibration_error,
            ),
            integration_quality=require_optional_mapping_value(
                raw_payload.get("integration_quality"),
                f"{field_path}.integration_quality",
                _calibration_error,
            ),
        )


@dataclass(frozen=True, slots=True)
class JaniformVanillaOptionGroupedBatchPricingGroupResult(ContractModel):
    payoff_type: VanillaOptionPayoffType
    risk_free_rate: float
    dividend_yield: float
    strikes: tuple[float, ...]
    maturity_intervals: tuple[float, ...]
    price_grid: tuple[tuple[float, ...], ...]
    implied_volatility_grid: tuple[tuple[Optional[float], ...], ...]
    implied_volatility_inversion_status_grid: tuple[
        tuple[VanillaOptionImpliedVolatilityInversionStatus, ...],
        ...,
    ]
    forward_price_grid: tuple[tuple[float, ...], ...]
    discount_factor_grid: tuple[tuple[float, ...], ...]
    boundary_state_norm_grid: tuple[tuple[float, ...], ...]
    maturity_summaries: tuple[JaniformVanillaOptionMaturitySummary, ...]

    def __post_init__(self) -> None:
        require_finite_scalar(self.risk_free_rate, "risk_free_rate", _calibration_error)
        require_finite_scalar(self.dividend_yield, "dividend_yield", _calibration_error)

        expected_shape = (len(self.maturity_intervals), len(self.strikes))
        grid_shape = _grid_shape(self.price_grid, field_path="price_grid")
        if grid_shape != expected_shape:
            raise _calibration_error(
                "price_grid",
                f"must have shape {expected_shape}, found {grid_shape}",
            )

        _validate_matching_grid_shape(
            self.implied_volatility_grid,
            grid_shape=expected_shape,
            field_path="implied_volatility_grid",
        )
        _validate_matching_grid_shape(
            self.implied_volatility_inversion_status_grid,
            grid_shape=expected_shape,
            field_path="implied_volatility_inversion_status_grid",
        )
        _validate_matching_grid_shape(
            self.forward_price_grid,
            grid_shape=expected_shape,
            field_path="forward_price_grid",
        )
        _validate_matching_grid_shape(
            self.discount_factor_grid,
            grid_shape=expected_shape,
            field_path="discount_factor_grid",
        )
        _validate_matching_grid_shape(
            self.boundary_state_norm_grid,
            grid_shape=expected_shape,
            field_path="boundary_state_norm_grid",
        )
        _validate_aligned_node_count(
            self.maturity_summaries,
            len(self.maturity_intervals),
            field_path="maturity_summaries",
        )

        for maturity_index, implied_volatility_row in enumerate(self.implied_volatility_grid):
            for strike_index, implied_volatility in enumerate(implied_volatility_row):
                _validate_implied_volatility_inversion_outcome(
                    implied_volatility=implied_volatility,
                    implied_volatility_inversion_status=self.implied_volatility_inversion_status_grid[
                        maturity_index
                    ][strike_index],
                    field_path=(
                        "grouped_batch_pricing_result"
                        f".group_node[{maturity_index}][{strike_index}]"
                    ),
                )

    @property
    def shape(self) -> tuple[int, int]:
        return len(self.maturity_intervals), len(self.strikes)

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> JaniformVanillaOptionGroupedBatchPricingGroupResult:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_maturity_summaries = require_sequence(
            raw_payload,
            "maturity_summaries",
            f"{field_path}.maturity_summaries",
            _calibration_error,
        )

        return JaniformVanillaOptionGroupedBatchPricingGroupResult(
            payoff_type=require_str_enum_value(
                raw_payload.get("payoff_type"),
                f"{field_path}.payoff_type",
                _calibration_error,
                enum_type=VanillaOptionPayoffType,
                allowed_values_message="must be one of: call, put",
            ),
            risk_free_rate=require_finite_number(
                raw_payload,
                "risk_free_rate",
                f"{field_path}.risk_free_rate",
                _calibration_error,
            ),
            dividend_yield=require_finite_number(
                raw_payload,
                "dividend_yield",
                f"{field_path}.dividend_yield",
                _calibration_error,
            ),
            strikes=_tuple_of_positive_floats(
                raw_payload,
                field_name="strikes",
                field_path=f"{field_path}.strikes",
            ),
            maturity_intervals=_tuple_of_positive_floats(
                raw_payload,
                field_name="maturity_intervals",
                field_path=f"{field_path}.maturity_intervals",
            ),
            price_grid=_grid_of_non_negative_floats(
                raw_payload,
                field_name="price_grid",
                field_path=f"{field_path}.price_grid",
            ),
            implied_volatility_grid=_grid_of_optional_finite_floats(
                raw_payload,
                field_name="implied_volatility_grid",
                field_path=f"{field_path}.implied_volatility_grid",
            ),
            implied_volatility_inversion_status_grid=_grid_of_implied_volatility_inversion_statuses(
                raw_payload,
                field_name="implied_volatility_inversion_status_grid",
                field_path=f"{field_path}.implied_volatility_inversion_status_grid",
            ),
            forward_price_grid=_grid_of_positive_floats(
                raw_payload,
                field_name="forward_price_grid",
                field_path=f"{field_path}.forward_price_grid",
            ),
            discount_factor_grid=_grid_of_positive_floats(
                raw_payload,
                field_name="discount_factor_grid",
                field_path=f"{field_path}.discount_factor_grid",
            ),
            boundary_state_norm_grid=_grid_of_non_negative_floats(
                raw_payload,
                field_name="boundary_state_norm_grid",
                field_path=f"{field_path}.boundary_state_norm_grid",
            ),
            maturity_summaries=tuple(
                JaniformVanillaOptionMaturitySummary.from_mapping(
                    raw_summary,
                    field_path=f"{field_path}.maturity_summaries[{summary_index}]",
                )
                for summary_index, raw_summary in enumerate(raw_maturity_summaries)
            ),
        )


@dataclass(frozen=True, slots=True)
class JaniformVanillaOptionGroupedBatchPricingResult(ContractModel):
    spot: float
    groups: tuple[JaniformVanillaOptionGroupedBatchPricingGroupResult, ...]

    def __post_init__(self) -> None:
        require_positive_finite_scalar(self.spot, "spot", _calibration_error)
        if len(self.groups) == 0:
            raise _calibration_error("groups", "must contain at least one pricing result group")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "grouped_batch_pricing_result",
    ) -> JaniformVanillaOptionGroupedBatchPricingResult:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_groups = require_sequence(
            raw_payload,
            "groups",
            f"{field_path}.groups",
            _calibration_error,
        )

        return JaniformVanillaOptionGroupedBatchPricingResult(
            spot=require_positive_finite_scalar(
                raw_payload.get("spot"),
                f"{field_path}.spot",
                _calibration_error,
            ),
            groups=tuple(
                JaniformVanillaOptionGroupedBatchPricingGroupResult.from_mapping(
                    raw_group,
                    field_path=f"{field_path}.groups[{group_index}]",
                )
                for group_index, raw_group in enumerate(raw_groups)
            ),
        )


@dataclass(frozen=True, slots=True)
class JaniformVanillaOptionSurface(ContractModel):
    spot: float
    strikes: tuple[float, ...]
    maturity_intervals: tuple[float, ...]
    payoff_type: VanillaOptionPayoffType = VanillaOptionPayoffType.CALL
    risk_free_rate: float = 0.0
    dividend_yield: float = 0.0

    def __post_init__(self) -> None:
        require_positive_finite_scalar(self.spot, "spot", _calibration_error)
        if len(self.strikes) == 0:
            raise _calibration_error("strikes", "must contain at least one strike")
        if len(self.maturity_intervals) == 0:
            raise _calibration_error(
                "maturity_intervals",
                "must contain at least one maturity interval",
            )

        require_finite_scalar(self.risk_free_rate, "risk_free_rate", _calibration_error)
        require_finite_scalar(self.dividend_yield, "dividend_yield", _calibration_error)

        for strike_index, strike in enumerate(self.strikes):
            require_positive_finite_scalar(
                strike,
                f"strikes[{strike_index}]",
                _calibration_error,
            )

        for maturity_index, maturity_interval in enumerate(self.maturity_intervals):
            require_positive_finite_scalar(
                maturity_interval,
                f"maturity_intervals[{maturity_index}]",
                _calibration_error,
            )

    @property
    def shape(self) -> tuple[int, int]:
        return len(self.maturity_intervals), len(self.strikes)

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "surface",
    ) -> JaniformVanillaOptionSurface:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return JaniformVanillaOptionSurface(
            spot=require_positive_finite_scalar(
                raw_payload.get("spot"),
                f"{field_path}.spot",
                _calibration_error,
            ),
            strikes=_tuple_of_positive_floats(
                raw_payload,
                field_name="strikes",
                field_path=f"{field_path}.strikes",
            ),
            maturity_intervals=_tuple_of_positive_floats(
                raw_payload,
                field_name="maturity_intervals",
                field_path=f"{field_path}.maturity_intervals",
            ),
            payoff_type=require_str_enum_value(
                raw_payload.get("payoff_type", VanillaOptionPayoffType.CALL.value),
                f"{field_path}.payoff_type",
                _calibration_error,
                enum_type=VanillaOptionPayoffType,
                allowed_values_message="must be one of: call, put",
            ),
            risk_free_rate=require_optional_finite_number(
                raw_payload,
                "risk_free_rate",
                f"{field_path}.risk_free_rate",
                _calibration_error,
                default_value=0.0,
            ),
            dividend_yield=require_optional_finite_number(
                raw_payload,
                "dividend_yield",
                f"{field_path}.dividend_yield",
                _calibration_error,
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
    price_grid: tuple[tuple[float, ...], ...]
    implied_volatility_grid: tuple[tuple[Optional[float], ...], ...]
    implied_volatility_inversion_status_grid: tuple[
        tuple[VanillaOptionImpliedVolatilityInversionStatus, ...],
        ...,
    ]
    forward_price_grid: tuple[tuple[float, ...], ...]
    discount_factor_grid: tuple[tuple[float, ...], ...]
    boundary_state_norm_grid: tuple[tuple[float, ...], ...]
    maturity_summaries: tuple[JaniformVanillaOptionMaturitySummary, ...]

    def __post_init__(self) -> None:
        require_positive_finite_scalar(self.spot, "spot", _calibration_error)
        require_finite_scalar(self.risk_free_rate, "risk_free_rate", _calibration_error)
        require_finite_scalar(self.dividend_yield, "dividend_yield", _calibration_error)

        expected_shape = (len(self.maturity_intervals), len(self.strikes))
        grid_shape = _grid_shape(self.price_grid, field_path="price_grid")
        if grid_shape != expected_shape:
            raise _calibration_error(
                "price_grid",
                f"must have shape {expected_shape}, found {grid_shape}",
            )

        _validate_matching_grid_shape(
            self.implied_volatility_grid,
            grid_shape=expected_shape,
            field_path="implied_volatility_grid",
        )
        _validate_matching_grid_shape(
            self.implied_volatility_inversion_status_grid,
            grid_shape=expected_shape,
            field_path="implied_volatility_inversion_status_grid",
        )
        _validate_matching_grid_shape(
            self.forward_price_grid,
            grid_shape=expected_shape,
            field_path="forward_price_grid",
        )
        _validate_matching_grid_shape(
            self.discount_factor_grid,
            grid_shape=expected_shape,
            field_path="discount_factor_grid",
        )
        _validate_matching_grid_shape(
            self.boundary_state_norm_grid,
            grid_shape=expected_shape,
            field_path="boundary_state_norm_grid",
        )
        _validate_aligned_node_count(
            self.maturity_summaries,
            len(self.maturity_intervals),
            field_path="maturity_summaries",
        )

        for maturity_index, implied_volatility_row in enumerate(self.implied_volatility_grid):
            for strike_index, implied_volatility in enumerate(implied_volatility_row):
                _validate_implied_volatility_inversion_outcome(
                    implied_volatility=implied_volatility,
                    implied_volatility_inversion_status=self.implied_volatility_inversion_status_grid[
                        maturity_index
                    ][strike_index],
                    field_path=(
                        "surface_pricing_result"
                        f".node[{maturity_index}][{strike_index}]"
                    ),
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
            raise _calibration_error(field_path, "must be an object")

        response = require_mapping(
            raw_payload,
            "response",
            f"{field_path}.response",
            _calibration_error,
        )
        raw_maturity_summaries = require_sequence(
            raw_payload,
            "maturity_summaries",
            f"{field_path}.maturity_summaries",
            _calibration_error,
        )

        return JaniformVanillaOptionSurfacePricingResult(
            spot=require_positive_finite_scalar(
                response.get("spot"),
                f"{field_path}.response.spot",
                _calibration_error,
            ),
            strikes=_tuple_of_positive_floats(
                response,
                field_name="strikes",
                field_path=f"{field_path}.response.strikes",
            ),
            maturity_intervals=_tuple_of_positive_floats(
                response,
                field_name="maturity_intervals",
                field_path=f"{field_path}.response.maturity_intervals",
            ),
            payoff_type=require_str_enum_value(
                response.get("payoff_type"),
                f"{field_path}.response.payoff_type",
                _calibration_error,
                enum_type=VanillaOptionPayoffType,
                allowed_values_message="must be one of: call, put",
            ),
            risk_free_rate=require_finite_number(
                response,
                "risk_free_rate",
                f"{field_path}.response.risk_free_rate",
                _calibration_error,
            ),
            dividend_yield=require_finite_number(
                response,
                "dividend_yield",
                f"{field_path}.response.dividend_yield",
                _calibration_error,
            ),
            price_grid=_grid_of_non_negative_floats(
                response,
                field_name="price_grid",
                field_path=f"{field_path}.response.price_grid",
            ),
            implied_volatility_grid=_grid_of_optional_finite_floats(
                response,
                field_name="implied_volatility_grid",
                field_path=f"{field_path}.response.implied_volatility_grid",
            ),
            implied_volatility_inversion_status_grid=_grid_of_implied_volatility_inversion_statuses(
                response,
                field_name="implied_volatility_inversion_status_grid",
                field_path=f"{field_path}.response.implied_volatility_inversion_status_grid",
            ),
            forward_price_grid=_grid_of_positive_floats(
                response,
                field_name="forward_price_grid",
                field_path=f"{field_path}.response.forward_price_grid",
            ),
            discount_factor_grid=_grid_of_positive_floats(
                response,
                field_name="discount_factor_grid",
                field_path=f"{field_path}.response.discount_factor_grid",
            ),
            boundary_state_norm_grid=_grid_of_non_negative_floats(
                response,
                field_name="boundary_state_norm_grid",
                field_path=f"{field_path}.response.boundary_state_norm_grid",
            ),
            maturity_summaries=tuple(
                JaniformVanillaOptionMaturitySummary.from_mapping(
                    raw_summary,
                    field_path=f"{field_path}.maturity_summaries[{summary_index}]",
                )
                for summary_index, raw_summary in enumerate(raw_maturity_summaries)
            ),
        )


def _tuple_of_positive_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[float, ...]:
    values = require_sequence(raw_payload, field_name, field_path, _calibration_error)
    normalized: list[float] = []
    for value_index, value in enumerate(values):
        normalized.append(
            require_positive_finite_scalar(
                value,
                f"{field_path}[{value_index}]",
                _calibration_error,
            )
        )

    return tuple(normalized)


def _tuple_of_non_negative_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[float, ...]:
    values = require_sequence(raw_payload, field_name, field_path, _calibration_error)
    normalized: list[float] = []
    for value_index, value in enumerate(values):
        normalized.append(
            require_non_negative_finite_scalar(
                value,
                f"{field_path}[{value_index}]",
                _calibration_error,
            )
        )

    return tuple(normalized)


def _tuple_of_finite_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[float, ...]:
    values = require_sequence(raw_payload, field_name, field_path, _calibration_error)
    normalized: list[float] = []
    for value_index, value in enumerate(values):
        normalized.append(
            require_finite_scalar(
                value,
                f"{field_path}[{value_index}]",
                _calibration_error,
            )
        )

    return tuple(normalized)


def _grid_of_positive_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[tuple[float, ...], ...]:
    return _grid_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        scalar_parser=lambda value, location: require_positive_finite_scalar(
            value,
            location,
            _calibration_error,
        ),
    )


def _grid_of_non_negative_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[tuple[float, ...], ...]:
    return _grid_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        scalar_parser=lambda value, location: require_non_negative_finite_scalar(
            value,
            location,
            _calibration_error,
        ),
    )


def _grid_of_optional_finite_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[tuple[Optional[float], ...], ...]:
    return _grid_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        scalar_parser=lambda value, location: require_optional_finite_number(
            {"value": value},
            "value",
            location,
            _calibration_error,
        ),
    )


def _grid_of_implied_volatility_inversion_statuses(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[tuple[VanillaOptionImpliedVolatilityInversionStatus, ...], ...]:
    return _grid_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        scalar_parser=lambda value, location: require_str_enum_value(
            value,
            location,
            _calibration_error,
            enum_type=VanillaOptionImpliedVolatilityInversionStatus,
            allowed_values_message=(
                "must be one of: succeeded, "
                "price_below_no_arbitrage_lower_bound, "
                "price_equals_finite_upper_bound, "
                "price_above_finite_upper_bound"
            ),
        ),
    )


def _grid_of_scalars[TScalar](
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
    scalar_parser: Callable[[Any, str], TScalar],
) -> tuple[tuple[TScalar, ...], ...]:
    rows = require_sequence(raw_payload, field_name, field_path, _calibration_error)

    normalized_rows: list[tuple[TScalar, ...]] = []
    expected_column_count: Optional[int] = None

    for row_index, raw_row in enumerate(rows):
        if isinstance(raw_row, str) or not isinstance(raw_row, Sequence):
            raise _calibration_error(f"{field_path}[{row_index}]", "must be a sequence")

        normalized_row = tuple(
            scalar_parser(value, f"{field_path}[{row_index}][{column_index}]")
            for column_index, value in enumerate(raw_row)
        )

        if expected_column_count is None:
            expected_column_count = len(normalized_row)
        elif len(normalized_row) != expected_column_count:
            raise _calibration_error(
                f"{field_path}[{row_index}]",
                f"must contain {expected_column_count} aligned values",
            )

        normalized_rows.append(normalized_row)

    return tuple(normalized_rows)


def _validate_aligned_node_count(
    values: Sequence[Any],
    expected_count: int,
    *,
    field_path: str,
) -> None:
    if len(values) != expected_count:
        raise _calibration_error(
            field_path,
            f"must contain {expected_count} aligned values",
        )


def _validate_optional_tuple_pair(
    left_values: Optional[tuple[Any, ...]],
    right_values: Optional[tuple[Any, ...]],
    *,
    left_field_path: str,
    right_field_path: str,
) -> None:
    if (left_values is None) != (right_values is None):
        raise _calibration_error(
            left_field_path if left_values is None else right_field_path,
            "must be null when the paired terminal distribution field is null",
        )


def _grid_shape(
    values: tuple[tuple[Any, ...], ...],
    *,
    field_path: str,
) -> tuple[int, int]:
    row_count = len(values)
    if row_count == 0:
        return 0, 0

    column_count = len(values[0])
    for row_index, row in enumerate(values):
        if len(row) != column_count:
            raise _calibration_error(
                f"{field_path}[{row_index}]",
                f"must contain {column_count} aligned values",
            )

    return row_count, column_count


def _validate_matching_grid_shape(
    values: tuple[tuple[Any, ...], ...],
    *,
    grid_shape: tuple[int, int],
    field_path: str,
) -> None:
    observed_shape = _grid_shape(values, field_path=field_path)
    if observed_shape != grid_shape:
        raise _calibration_error(
            field_path,
            f"must have shape {grid_shape}, found {observed_shape}",
        )


__all__ = [
    "JaniformGroupedVanillaOptionBatch",
    "JaniformGroupedVanillaOptionGroup",
    "JaniformVanillaOptionGroupedBatchPricingGroupResult",
    "JaniformVanillaOptionGroupedBatchPricingResult",
    "JaniformVanillaOptionMaturityIntervalSummary",
    "JaniformVanillaOptionMaturityPricingEngineKind",
    "JaniformVanillaOptionMaturitySummary",
    "JaniformVanillaOptionSurface",
    "JaniformVanillaOptionSurfacePricingResult",
]
