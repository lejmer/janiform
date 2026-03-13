from dataclasses import dataclass
from enum import StrEnum
from typing import Any, cast

from .errors import SIMULATION_ERROR_FACTORY, _simulation_error
from ..._contracts import (
    ContractModel,
    compute_hawkes_alpha_over_beta_spectral_radius,
    require_mapping,
    require_non_negative_finite_number,
    require_non_negative_finite_scalar,
    require_positive_finite_number,
)


class SeededTickEventSide(StrEnum):
    BUY = 'buy'
    SELL = 'sell'


class SeededTickEventStateSnapshotCapture(StrEnum):
    DISABLED = 'disabled'
    POST_EVENT_TRANSITION = 'post_event_transition'


def _parse_seeded_tick_state_snapshot_capture(
    raw_value: object,
    *,
    field_path: str,
) -> SeededTickEventStateSnapshotCapture:
    if not isinstance(raw_value, str):
        raise _simulation_error(field_path, "must be a string")

    try:
        return SeededTickEventStateSnapshotCapture(raw_value)
    except ValueError as e:
        raise _simulation_error(
            field_path,
            f"must be one of: disabled, post_event_transition, received '{raw_value}'",
        ) from e


@dataclass(frozen=True, slots=True)
class HawkesKernelLoadings(ContractModel):
    """Directional Hawkes loadings for one 2x2 excitation block."""

    plus_plus: float
    plus_minus: float
    minus_plus: float
    minus_minus: float

    def __post_init__(self) -> None:
        for field_name in ("plus_plus", "plus_minus", "minus_plus", "minus_minus"):
            value = cast(float, getattr(self, field_name))
            require_non_negative_finite_scalar(value, field_name, SIMULATION_ERROR_FACTORY)

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str = "kernel_loadings") -> HawkesKernelLoadings:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        return HawkesKernelLoadings(
            plus_plus=require_non_negative_finite_number(
                raw_payload,
                "plus_plus",
                f"{field_path}.plus_plus",
                SIMULATION_ERROR_FACTORY,
            ),
            plus_minus=require_non_negative_finite_number(
                raw_payload,
                "plus_minus",
                f"{field_path}.plus_minus",
                SIMULATION_ERROR_FACTORY,
            ),
            minus_plus=require_non_negative_finite_number(
                raw_payload,
                "minus_plus",
                f"{field_path}.minus_plus",
                SIMULATION_ERROR_FACTORY,
            ),
            minus_minus=require_non_negative_finite_number(
                raw_payload,
                "minus_minus",
                f"{field_path}.minus_minus",
                SIMULATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class HawkesKernelParameters(ContractModel):
    """Canonical Hawkes kernel parameters with stability validation."""

    lambda_bar: float
    beta: float
    alpha: HawkesKernelLoadings
    rho: HawkesKernelLoadings

    def __post_init__(self) -> None:
        require_positive_finite_number(
            {"lambda_bar": self.lambda_bar},
            "lambda_bar",
            "lambda_bar",
            SIMULATION_ERROR_FACTORY,
        )
        require_positive_finite_number(
            {"beta": self.beta},
            "beta",
            "beta",
            SIMULATION_ERROR_FACTORY,
        )

        spectral_radius = compute_hawkes_alpha_over_beta_spectral_radius(
            self.alpha.plus_plus,
            self.alpha.plus_minus,
            self.alpha.minus_plus,
            self.alpha.minus_minus,
            self.beta,
        )
        if spectral_radius >= 1.0:
            raise _simulation_error(
                "alpha",
                f"spectral radius(alpha / beta) must be < 1.0, found {spectral_radius}",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "kernel_parameters",
    ) -> HawkesKernelParameters:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        return HawkesKernelParameters(
            lambda_bar=require_positive_finite_number(
                raw_payload,
                "lambda_bar",
                f"{field_path}.lambda_bar",
                SIMULATION_ERROR_FACTORY,
            ),
            beta=require_positive_finite_number(
                raw_payload,
                "beta",
                f"{field_path}.beta",
                SIMULATION_ERROR_FACTORY,
            ),
            alpha=HawkesKernelLoadings.from_mapping(
                require_mapping(raw_payload, "alpha", f"{field_path}.alpha", SIMULATION_ERROR_FACTORY),
                field_path=f"{field_path}.alpha",
            ),
            rho=HawkesKernelLoadings.from_mapping(
                require_mapping(raw_payload, "rho", f"{field_path}.rho", SIMULATION_ERROR_FACTORY),
                field_path=f"{field_path}.rho",
            ),
        )


@dataclass(frozen=True, slots=True)
class HawkesExcitationState(ContractModel):
    """4D Hawkes excitation state at one simulation boundary."""

    y_plus_plus: float
    y_plus_minus: float
    y_minus_plus: float
    y_minus_minus: float

    def __post_init__(self) -> None:
        for field_name in ("y_plus_plus", "y_plus_minus", "y_minus_plus", "y_minus_minus"):
            value = cast(float, getattr(self, field_name))
            require_non_negative_finite_scalar(value, field_name, SIMULATION_ERROR_FACTORY)

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "initial_excitation_state",
    ) -> HawkesExcitationState:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        return HawkesExcitationState(
            y_plus_plus=require_non_negative_finite_number(
                raw_payload,
                "y_plus_plus",
                f"{field_path}.y_plus_plus",
                SIMULATION_ERROR_FACTORY,
            ),
            y_plus_minus=require_non_negative_finite_number(
                raw_payload,
                "y_plus_minus",
                f"{field_path}.y_plus_minus",
                SIMULATION_ERROR_FACTORY,
            ),
            y_minus_plus=require_non_negative_finite_number(
                raw_payload,
                "y_minus_plus",
                f"{field_path}.y_minus_plus",
                SIMULATION_ERROR_FACTORY,
            ),
            y_minus_minus=require_non_negative_finite_number(
                raw_payload,
                "y_minus_minus",
                f"{field_path}.y_minus_minus",
                SIMULATION_ERROR_FACTORY,
            ),
        )


__all__ = [
    "HawkesExcitationState",
    "HawkesKernelLoadings",
    "HawkesKernelParameters",
    "SeededTickEventSide",
    "SeededTickEventStateSnapshotCapture",
]
