from dataclasses import dataclass
from enum import StrEnum
from typing import Any, Final

from .base import (
    ContractModel,
    require_finite_number,
    require_finite_scalar,
    require_mapping,
    require_only_supported_fields,
    require_sequence,
    require_string,
    serialize_deterministic_json,
)


_BOUNDARY_STATE_INVARIANT_TOLERANCE: Final[float] = 1e-12


def _mean_reversion_error(field_path: str, message: str) -> ValueError:
    return ValueError(f"{field_path}: {message}")


class MeanReversionAnchorModelKind(StrEnum):
    ENDOGENOUS_CORE_EXPECTATION_ONE_INTERVAL = "endogenous_core_expectation_one_interval"


@dataclass(frozen=True, slots=True)
class MeanReversionAnchorModel(ContractModel):
    anchor_model_kind: MeanReversionAnchorModelKind

    def __post_init__(self) -> None:
        match self.anchor_model_kind:
            case MeanReversionAnchorModelKind.ENDOGENOUS_CORE_EXPECTATION_ONE_INTERVAL:
                return

    def to_payload(self) -> dict[str, Any]:
        return {
            "anchor_model_kind": self.anchor_model_kind.value,
        }

    def to_json(self) -> str:
        return serialize_deterministic_json(self.to_payload())

    @staticmethod
    def endogenous_core_expectation_one_interval() -> MeanReversionAnchorModel:
        return MeanReversionAnchorModel(
            anchor_model_kind=MeanReversionAnchorModelKind.ENDOGENOUS_CORE_EXPECTATION_ONE_INTERVAL
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "anchor_model",
    ) -> MeanReversionAnchorModel:
        if not isinstance(raw_payload, dict):
            raise _mean_reversion_error(field_path, "must be an object")

        require_only_supported_fields(
            raw_payload,
            supported_fields=("anchor_model_kind",),
            field_path=field_path,
            error_factory=_mean_reversion_error,
        )

        anchor_model_kind = MeanReversionAnchorModelKind(
            require_string(
                raw_payload,
                "anchor_model_kind",
                f"{field_path}.anchor_model_kind",
                _mean_reversion_error,
            )
        )

        return MeanReversionAnchorModel(
            anchor_model_kind=anchor_model_kind,
        )


@dataclass(frozen=True, slots=True)
class MeanReversionFittedParameters(ContractModel):
    kappa: float
    anchor_model: MeanReversionAnchorModel

    def __post_init__(self) -> None:
        require_finite_scalar(self.kappa, "kappa", _mean_reversion_error)
        if self.kappa < 0.0:
            raise _mean_reversion_error("kappa", "must be non-negative")

    def to_payload(self) -> dict[str, Any]:
        return {
            "kappa": self.kappa,
            "anchor_model": self.anchor_model.to_payload(),
        }

    def to_json(self) -> str:
        return serialize_deterministic_json(self.to_payload())

    @property
    def is_enabled(self) -> bool:
        return True

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
            supported_fields=("kappa", "anchor_model"),
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
            anchor_model=MeanReversionAnchorModel.from_mapping(
                require_mapping(
                    raw_payload,
                    "anchor_model",
                    f"{field_path}.anchor_model",
                    _mean_reversion_error,
                ),
                field_path=f"{field_path}.anchor_model",
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

        if len(self.end_anchor_values) != len(self.interval_deltas):
            raise _mean_reversion_error(
                "interval_deltas",
                f"must contain {len(self.end_anchor_values)} values",
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

        expected_deviation = self.observed_log_price - self.anchor_value
        if abs(self.deviation - expected_deviation) > _BOUNDARY_STATE_INVARIANT_TOLERANCE:
            raise _mean_reversion_error(
                "deviation",
                "must equal observed_log_price - anchor_value",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "mean_reversion_boundary_state",
    ) -> MeanReversionBoundaryState:
        if not isinstance(raw_payload, dict):
            raise _mean_reversion_error(field_path, "must be an object")

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
    if field_name not in raw_payload:
        return ()

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
