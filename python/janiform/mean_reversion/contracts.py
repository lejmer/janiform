"""Public mean-reversion domain contracts shared across pricing, calibration, and simulation."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from ..contracts.base import (
    ContractModel,
    require_finite_number,
    require_finite_scalar,
    require_only_supported_fields,
    require_sequence,
    require_string,
)


def _mean_reversion_error(field_path: str, message: str) -> ValueError:
    return ValueError(f"{field_path}: {message}")


@dataclass(frozen=True, slots=True)
class MeanReversionFittedParameters(ContractModel):
    kappa: float

    def __post_init__(self) -> None:
        require_finite_scalar(self.kappa, "kappa", _mean_reversion_error)
        if self.kappa < 0.0:
            raise _mean_reversion_error("kappa", "must be non-negative")

    @property
    def is_enabled(self) -> bool:
        return self.kappa > 0.0

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "mean_reversion_fitted_parameters",
    ) -> MeanReversionFittedParameters:
        if not isinstance(raw_payload, dict):
            raise _mean_reversion_error(field_path, "must be an object")

        require_only_supported_fields(
            raw_payload,
            supported_fields=("kappa",),
            field_path=field_path,
            error_factory=_mean_reversion_error,
        )

        return MeanReversionFittedParameters(
            kappa=require_finite_number(
                raw_payload,
                "kappa",
                f"{field_path}.kappa",
                _mean_reversion_error,
            ),
        )


@dataclass(frozen=True, slots=True)
class MeanReversionRuntimeIntervalAnchors(ContractModel):
    interval_alignment_policy: str
    start_anchor_value: float
    end_anchor_values: tuple[float, ...]
    interval_deltas: tuple[float, ...]

    def __post_init__(self) -> None:
        if not self.interval_alignment_policy.strip():
            raise _mean_reversion_error(
                "interval_alignment_policy",
                "must be a non-empty string",
            )

        require_finite_scalar(
            self.start_anchor_value,
            "start_anchor_value",
            _mean_reversion_error,
        )

        expected_length = len(self.end_anchor_values)
        if len(self.interval_deltas) != expected_length:
            raise _mean_reversion_error(
                "interval_deltas",
                f"must contain {expected_length} values",
            )

        for interval_index, anchor_value in enumerate(self.end_anchor_values):
            require_finite_scalar(
                anchor_value,
                f"end_anchor_values[{interval_index}]",
                _mean_reversion_error,
            )

            interval_delta = self.interval_deltas[interval_index]
            require_finite_scalar(
                interval_delta,
                f"interval_deltas[{interval_index}]",
                _mean_reversion_error,
            )
            if interval_delta <= 0.0:
                raise _mean_reversion_error(
                    f"interval_deltas[{interval_index}]",
                    "must be strictly positive",
                )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "mean_reversion_runtime_interval_anchors",
    ) -> MeanReversionRuntimeIntervalAnchors:
        if not isinstance(raw_payload, dict):
            raise _mean_reversion_error(field_path, "must be an object")

        require_only_supported_fields(
            raw_payload,
            supported_fields=(
                "interval_alignment_policy",
                "start_anchor_value",
                "end_anchor_values",
                "interval_deltas",
            ),
            field_path=field_path,
            error_factory=_mean_reversion_error,
        )

        return MeanReversionRuntimeIntervalAnchors(
            interval_alignment_policy=require_string(
                raw_payload,
                "interval_alignment_policy",
                f"{field_path}.interval_alignment_policy",
                _mean_reversion_error,
            ),
            start_anchor_value=require_finite_number(
                raw_payload,
                "start_anchor_value",
                f"{field_path}.start_anchor_value",
                _mean_reversion_error,
            ),
            end_anchor_values=_numeric_sequence(
                raw_payload,
                "end_anchor_values",
                f"{field_path}.end_anchor_values",
            ),
            interval_deltas=_numeric_sequence(
                raw_payload,
                "interval_deltas",
                f"{field_path}.interval_deltas",
            ),
        )


@dataclass(frozen=True, slots=True)
class MeanReversionBoundaryState(ContractModel):
    observed_log_price: float
    anchor_value: float
    deviation: float

    def __post_init__(self) -> None:
        require_finite_scalar(
            self.observed_log_price,
            "observed_log_price",
            _mean_reversion_error,
        )
        require_finite_scalar(
            self.anchor_value,
            "anchor_value",
            _mean_reversion_error,
        )
        require_finite_scalar(
            self.deviation,
            "deviation",
            _mean_reversion_error,
        )

        if self.anchor_value + self.deviation != self.observed_log_price:
            raise _mean_reversion_error(
                "deviation",
                "anchor_value + deviation must equal observed_log_price",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "mean_reversion_boundary_state",
    ) -> MeanReversionBoundaryState:
        if not isinstance(raw_payload, dict):
            raise _mean_reversion_error(field_path, "must be an object")

        require_only_supported_fields(
            raw_payload,
            supported_fields=("observed_log_price", "anchor_value", "deviation"),
            field_path=field_path,
            error_factory=_mean_reversion_error,
        )

        return MeanReversionBoundaryState(
            observed_log_price=require_finite_number(
                raw_payload,
                "observed_log_price",
                f"{field_path}.observed_log_price",
                _mean_reversion_error,
            ),
            anchor_value=require_finite_number(
                raw_payload,
                "anchor_value",
                f"{field_path}.anchor_value",
                _mean_reversion_error,
            ),
            deviation=require_finite_number(
                raw_payload,
                "deviation",
                f"{field_path}.deviation",
                _mean_reversion_error,
            ),
        )


def _numeric_sequence(
    raw_payload: dict[str, Any],
    field_name: str,
    field_path: str,
) -> tuple[float, ...]:
    raw_values = require_sequence(
        raw_payload,
        field_name,
        field_path,
        _mean_reversion_error,
    )

    values: list[float] = []
    for entry_index, raw_value in enumerate(raw_values):
        values.append(
            require_finite_scalar(
                raw_value,
                f"{field_path}[{entry_index}]",
                _mean_reversion_error,
            )
        )

    return tuple(values)


__all__ = [
    "MeanReversionBoundaryState",
    "MeanReversionFittedParameters",
    "MeanReversionRuntimeIntervalAnchors",
]
