"""Public market-pressure state contracts."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Optional

from ..contracts.base import (
    ContractModel,
    require_finite_number,
    require_finite_number_sequence,
    require_finite_scalar,
    require_mapping,
    require_only_supported_fields,
)


def _market_pressure_error(field_path: str, message: str) -> ValueError:
    return ValueError(f"{field_path}: {message}")


@dataclass(frozen=True, slots=True)
class MarketPressureTransitionRates(ContractModel):
    upward_transition_rates: list[float]
    downward_transition_rates: list[float]

    def __post_init__(self) -> None:
        _validate_non_negative_finite_values(
            self.upward_transition_rates,
            "upward_transition_rates",
        )
        _validate_non_negative_finite_values(
            self.downward_transition_rates,
            "downward_transition_rates",
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "market_pressure_transition_rates",
    ) -> MarketPressureTransitionRates:
        if not isinstance(raw_payload, dict):
            raise _market_pressure_error(field_path, "must be an object")

        require_only_supported_fields(
            raw_payload,
            supported_fields=("upward_transition_rates", "downward_transition_rates"),
            field_path=field_path,
            error_factory=_market_pressure_error,
        )

        return MarketPressureTransitionRates(
            upward_transition_rates=require_finite_number_sequence(
                raw_payload,
                "upward_transition_rates",
                f"{field_path}.upward_transition_rates",
                _market_pressure_error,
            ),
            downward_transition_rates=require_finite_number_sequence(
                raw_payload,
                "downward_transition_rates",
                f"{field_path}.downward_transition_rates",
                _market_pressure_error,
            ),
        )


@dataclass(frozen=True, slots=True)
class MarketPressureMeasureParameters(ContractModel):
    transition_rates: MarketPressureTransitionRates
    anchor_drift_per_signed_balance: float

    def __post_init__(self) -> None:
        if not isinstance(self.transition_rates, MarketPressureTransitionRates):
            raise _market_pressure_error(
                "transition_rates",
                "must be a MarketPressureTransitionRates",
            )

        require_finite_scalar(
            self.anchor_drift_per_signed_balance,
            "anchor_drift_per_signed_balance",
            _market_pressure_error,
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "market_pressure_measure_parameters",
    ) -> MarketPressureMeasureParameters:
        if not isinstance(raw_payload, dict):
            raise _market_pressure_error(field_path, "must be an object")

        require_only_supported_fields(
            raw_payload,
            supported_fields=("transition_rates", "anchor_drift_per_signed_balance"),
            field_path=field_path,
            error_factory=_market_pressure_error,
        )

        return MarketPressureMeasureParameters(
            transition_rates=MarketPressureTransitionRates.from_mapping(
                require_mapping(
                    raw_payload,
                    "transition_rates",
                    f"{field_path}.transition_rates",
                    _market_pressure_error,
                ),
                field_path=f"{field_path}.transition_rates",
            ),
            anchor_drift_per_signed_balance=require_finite_number(
                raw_payload,
                "anchor_drift_per_signed_balance",
                f"{field_path}.anchor_drift_per_signed_balance",
                _market_pressure_error,
            ),
        )


@dataclass(frozen=True, slots=True)
class MarketPressureParameters(ContractModel):
    side_imbalance_levels: list[float]
    physical_measure: MarketPressureMeasureParameters

    def __post_init__(self) -> None:
        _validate_side_imbalance_support(self.side_imbalance_levels)

        if not isinstance(self.physical_measure, MarketPressureMeasureParameters):
            raise _market_pressure_error(
                "physical_measure",
                "must be a MarketPressureMeasureParameters",
            )

        _validate_transition_rate_count(
            self.physical_measure.transition_rates,
            len(self.side_imbalance_levels),
            "physical_measure.transition_rates",
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "market_pressure_parameters",
    ) -> MarketPressureParameters:
        if not isinstance(raw_payload, dict):
            raise _market_pressure_error(field_path, "must be an object")

        require_only_supported_fields(
            raw_payload,
            supported_fields=(
                "side_imbalance_levels",
                "physical_measure",
            ),
            field_path=field_path,
            error_factory=_market_pressure_error,
        )

        return MarketPressureParameters(
            side_imbalance_levels=require_finite_number_sequence(
                raw_payload,
                "side_imbalance_levels",
                f"{field_path}.side_imbalance_levels",
                _market_pressure_error,
                require_non_empty=True,
            ),
            physical_measure=MarketPressureMeasureParameters.from_mapping(
                require_mapping(
                    raw_payload,
                    "physical_measure",
                    f"{field_path}.physical_measure",
                    _market_pressure_error,
                ),
                field_path=f"{field_path}.physical_measure",
            ),
        )


@dataclass(frozen=True, slots=True)
class MarketPressureState(ContractModel):
    state_weights: list[float]

    def __post_init__(self) -> None:
        if not self.state_weights:
            raise _market_pressure_error(
                "state_weights",
                "must include at least one weight",
            )

        weight_sum = 0.0
        for state_position, state_weight in enumerate(self.state_weights):
            normalized_state_weight = require_finite_scalar(
                state_weight,
                f"state_weights[{state_position}]",
                _market_pressure_error,
            )
            if normalized_state_weight < 0.0:
                raise _market_pressure_error(
                    f"state_weights[{state_position}]",
                    "must be non-negative",
                )

            weight_sum += normalized_state_weight

        if weight_sum <= 0.0:
            raise _market_pressure_error(
                "state_weights",
                "must have strictly positive total weight",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "market_pressure_state",
    ) -> MarketPressureState:
        if not isinstance(raw_payload, dict):
            raise _market_pressure_error(field_path, "must be an object")

        require_only_supported_fields(
            raw_payload,
            supported_fields=("state_weights",),
            field_path=field_path,
            error_factory=_market_pressure_error,
        )

        return MarketPressureState(
            state_weights=require_finite_number_sequence(
                raw_payload,
                "state_weights",
                f"{field_path}.state_weights",
                _market_pressure_error,
                require_non_empty=True,
            ),
        )


def _validate_non_negative_finite_values(
    values: list[float],
    field_path: str,
) -> None:
    for value_position, value in enumerate(values):
        normalized_value = require_finite_scalar(
            value,
            f"{field_path}[{value_position}]",
            _market_pressure_error,
        )
        if normalized_value < 0.0:
            raise _market_pressure_error(
                f"{field_path}[{value_position}]",
                "must be non-negative",
            )


def _validate_side_imbalance_support(side_imbalance_levels: list[float]) -> None:
    if not side_imbalance_levels:
        raise _market_pressure_error(
            "side_imbalance_levels",
            "must include at least one support point",
        )

    previous_level: Optional[float] = None
    for state_position, side_imbalance_level in enumerate(side_imbalance_levels):
        normalized_level = require_finite_scalar(
            side_imbalance_level,
            f"side_imbalance_levels[{state_position}]",
            _market_pressure_error,
        )
        if normalized_level < -1.0 or normalized_level > 1.0:
            raise _market_pressure_error(
                f"side_imbalance_levels[{state_position}]",
                "must be between -1 and 1",
            )

        if previous_level is not None and normalized_level <= previous_level:
            raise _market_pressure_error(
                "side_imbalance_levels",
                "must be strictly increasing",
            )

        previous_level = normalized_level


def _validate_transition_rate_count(
    transition_rates: MarketPressureTransitionRates,
    state_count: int,
    field_path: str,
) -> None:
    expected_rate_count = state_count - 1
    if len(transition_rates.upward_transition_rates) != expected_rate_count:
        raise _market_pressure_error(
            f"{field_path}.upward_transition_rates",
            f"must contain {expected_rate_count} rates",
        )
    if len(transition_rates.downward_transition_rates) != expected_rate_count:
        raise _market_pressure_error(
            f"{field_path}.downward_transition_rates",
            f"must contain {expected_rate_count} rates",
        )


__all__ = [
    "MarketPressureMeasureParameters",
    "MarketPressureParameters",
    "MarketPressureState",
    "MarketPressureTransitionRates",
]
