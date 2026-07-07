from __future__ import annotations

from dataclasses import dataclass
from math import isfinite
from typing import Any

from .contracts.base import (
    ContractModel,
    ValidationErrorFactory,
    require_finite_number,
    require_mapping,
)


__all__ = [
    "ReturnClockExposureParameters",
    "validate_return_clock_exposure_parameters",
    "validate_return_clock_exposure_scale",
]


@dataclass(frozen=True, slots=True)
class ReturnClockExposureParameters(ContractModel):
    buy_excitation_return_clock_scale: float
    sell_excitation_return_clock_scale: float

    def __post_init__(self) -> None:
        validate_return_clock_exposure_parameters(
            self,
            field_path="return_clock_exposure_parameters",
            error_factory=_return_clock_error,
        )

    @staticmethod
    def neutral() -> ReturnClockExposureParameters:
        return ReturnClockExposureParameters(
            buy_excitation_return_clock_scale=1.0,
            sell_excitation_return_clock_scale=1.0,
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
        error_factory: ValidationErrorFactory,
    ) -> ReturnClockExposureParameters:
        payload = require_mapping(
            {"value": raw_payload},
            "value",
            field_path,
            error_factory,
        )

        return ReturnClockExposureParameters(
            buy_excitation_return_clock_scale=require_finite_number(
                payload,
                "buy_excitation_return_clock_scale",
                f"{field_path}.buy_excitation_return_clock_scale",
                error_factory,
            ),
            sell_excitation_return_clock_scale=require_finite_number(
                payload,
                "sell_excitation_return_clock_scale",
                f"{field_path}.sell_excitation_return_clock_scale",
                error_factory,
            ),
        )


def validate_return_clock_exposure_parameters(
    return_clock_exposure_parameters: ReturnClockExposureParameters,
    *,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> None:
    validate_return_clock_exposure_scale(
        return_clock_exposure_parameters.buy_excitation_return_clock_scale,
        field_path=f"{field_path}.buy_excitation_return_clock_scale",
        error_factory=error_factory,
    )

    validate_return_clock_exposure_scale(
        return_clock_exposure_parameters.sell_excitation_return_clock_scale,
        field_path=f"{field_path}.sell_excitation_return_clock_scale",
        error_factory=error_factory,
    )


def validate_return_clock_exposure_scale(
    value: float,
    *,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> None:
    if not isfinite(value) or value <= 0.0:
        raise error_factory(field_path, "must be finite and strictly positive")


def _return_clock_error(field_path: str, message: str) -> ValueError:
    return ValueError(f"{field_path}: {message}")
