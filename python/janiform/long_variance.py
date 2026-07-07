from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from .contracts.base import (
    ContractModel,
    ValidationErrorFactory,
    require_finite_number,
    require_mapping,
    require_non_negative_finite_scalar,
    require_positive_finite_scalar,
)

__all__ = [
    "LongVarianceParameters",
    "LongVarianceState",
    "validate_long_variance_parameters",
    "validate_long_variance_state",
]


@dataclass(frozen=True, slots=True)
class LongVarianceParameters(ContractModel):
    mean_reversion_rate: float
    vol_of_vol: float
    stationary_return_energy_rate_per_hour: float
    return_energy_loading: float
    buy_event_loading: float
    sell_event_loading: float

    def __post_init__(self) -> None:
        validate_long_variance_parameters(
            self,
            field_path="long_variance_parameters",
            error_factory=_long_variance_error,
        )

    @staticmethod
    def neutral() -> LongVarianceParameters:
        return LongVarianceParameters(
            mean_reversion_rate=1.0,
            vol_of_vol=0.0,
            stationary_return_energy_rate_per_hour=0.0,
            return_energy_loading=0.0,
            buy_event_loading=0.0,
            sell_event_loading=0.0,
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
        error_factory: ValidationErrorFactory,
    ) -> LongVarianceParameters:
        payload = require_mapping(
            {"value": raw_payload},
            "value",
            field_path,
            error_factory,
        )

        return LongVarianceParameters(
            mean_reversion_rate=require_finite_number(
                payload,
                "mean_reversion_rate",
                f"{field_path}.mean_reversion_rate",
                error_factory,
            ),
            vol_of_vol=require_finite_number(
                payload,
                "vol_of_vol",
                f"{field_path}.vol_of_vol",
                error_factory,
            ),
            stationary_return_energy_rate_per_hour=require_finite_number(
                payload,
                "stationary_return_energy_rate_per_hour",
                f"{field_path}.stationary_return_energy_rate_per_hour",
                error_factory,
            ),
            return_energy_loading=require_finite_number(
                payload,
                "return_energy_loading",
                f"{field_path}.return_energy_loading",
                error_factory,
            ),
            buy_event_loading=require_finite_number(
                payload,
                "buy_event_loading",
                f"{field_path}.buy_event_loading",
                error_factory,
            ),
            sell_event_loading=require_finite_number(
                payload,
                "sell_event_loading",
                f"{field_path}.sell_event_loading",
                error_factory,
            ),
        )


@dataclass(frozen=True, slots=True)
class LongVarianceState(ContractModel):
    level: float

    def __post_init__(self) -> None:
        validate_long_variance_state(
            self,
            field_path="long_variance_state",
            error_factory=_long_variance_error,
        )

    @staticmethod
    def unit() -> LongVarianceState:
        return LongVarianceState(level=1.0)

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
        error_factory: ValidationErrorFactory,
    ) -> LongVarianceState:
        payload = require_mapping(
            {"value": raw_payload},
            "value",
            field_path,
            error_factory,
        )

        return LongVarianceState(
            level=require_finite_number(
                payload,
                "level",
                f"{field_path}.level",
                error_factory,
            ),
        )


def validate_long_variance_parameters(
    parameters: LongVarianceParameters,
    *,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> None:
    require_positive_finite_scalar(
        parameters.mean_reversion_rate,
        field_path=f"{field_path}.mean_reversion_rate",
        error_factory=error_factory,
    )

    require_non_negative_finite_scalar(
        parameters.vol_of_vol,
        field_path=f"{field_path}.vol_of_vol",
        error_factory=error_factory,
    )

    require_non_negative_finite_scalar(
        parameters.stationary_return_energy_rate_per_hour,
        field_path=f"{field_path}.stationary_return_energy_rate_per_hour",
        error_factory=error_factory,
    )

    require_non_negative_finite_scalar(
        parameters.return_energy_loading,
        field_path=f"{field_path}.return_energy_loading",
        error_factory=error_factory,
    )

    require_non_negative_finite_scalar(
        parameters.buy_event_loading,
        field_path=f"{field_path}.buy_event_loading",
        error_factory=error_factory,
    )

    require_non_negative_finite_scalar(
        parameters.sell_event_loading,
        field_path=f"{field_path}.sell_event_loading",
        error_factory=error_factory,
    )


def validate_long_variance_state(
    state: LongVarianceState,
    *,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> None:
    require_positive_finite_scalar(
        state.level,
        field_path=f"{field_path}.level",
        error_factory=error_factory,
    )


def _long_variance_error(field_path: str, message: str) -> ValueError:
    return ValueError(f"{field_path}: {message}")
