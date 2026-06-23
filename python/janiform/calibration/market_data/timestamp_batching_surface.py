from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
from typing import Any

from ...contracts import ContractModel
from ...contracts.base import require_boolean, require_mapping, require_string
from ...simulation.timestamp_batching_surface import TimestampBatchingSurface
from ..errors import calibration_error


CALIBRATION_TIMESTAMP_BATCHING_SURFACE_DIAGNOSTIC_SCHEMA_VERSION = (
    "calibration.market_data.timestamp_batching_surface_diagnostic.v1"
)
CALIBRATION_TIMESTAMP_BATCHING_SURFACE_DIAGNOSTIC_POLICY = (
    "return_components_timestamp_batching_surface_visibility.v1"
)


class MarketDataTimestampBatchingSurfaceDiagnosticState(StrEnum):
    INFERRED_FIXED_BUCKET = "inferred_fixed_bucket"
    DISABLED_NO_DETECTABLE_BATCHING = "disabled_no_detectable_batching"
    NOT_EVALUATED_RETURN_COMPONENTS_NOT_SELECTED = (
        "not_evaluated_return_components_not_selected"
    )
    NOT_EVALUATED_RETURN_COMPONENTS_INPUT_UNAVAILABLE = (
        "not_evaluated_return_components_input_unavailable"
    )


@dataclass(frozen=True, slots=True)
class MarketDataTimestampBatchingSurfaceDiagnostic(ContractModel):
    schema_version: str
    diagnostic_policy: str
    state: MarketDataTimestampBatchingSurfaceDiagnosticState
    return_components_stage_selected: bool
    timestamp_batching_surface: TimestampBatchingSurface
    message: str

    def __post_init__(self) -> None:
        if (
            self.schema_version
            != CALIBRATION_TIMESTAMP_BATCHING_SURFACE_DIAGNOSTIC_SCHEMA_VERSION
        ):
            raise calibration_error(
                "timestamp_batching_surface.schema_version",
                f"must be `{CALIBRATION_TIMESTAMP_BATCHING_SURFACE_DIAGNOSTIC_SCHEMA_VERSION}`",
            )

        if (
            self.diagnostic_policy
            != CALIBRATION_TIMESTAMP_BATCHING_SURFACE_DIAGNOSTIC_POLICY
        ):
            raise calibration_error(
                "timestamp_batching_surface.diagnostic_policy",
                f"must be `{CALIBRATION_TIMESTAMP_BATCHING_SURFACE_DIAGNOSTIC_POLICY}`",
            )

        if not self.message.strip():
            raise calibration_error(
                "timestamp_batching_surface.message",
                "must be a non-empty string",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> MarketDataTimestampBatchingSurfaceDiagnostic:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        raw_state = require_string(
            raw_payload,
            "state",
            f"{field_path}.state",
            calibration_error,
        )

        try:
            state = MarketDataTimestampBatchingSurfaceDiagnosticState(raw_state)
        except ValueError as e:
            raise calibration_error(
                f"{field_path}.state",
                "must be a supported timestamp batching surface diagnostic state",
            ) from e

        raw_timestamp_batching_surface = require_mapping(
            raw_payload,
            "timestamp_batching_surface",
            f"{field_path}.timestamp_batching_surface",
            calibration_error,
        )

        return MarketDataTimestampBatchingSurfaceDiagnostic(
            schema_version=require_string(
                raw_payload,
                "schema_version",
                f"{field_path}.schema_version",
                calibration_error,
            ),
            diagnostic_policy=require_string(
                raw_payload,
                "diagnostic_policy",
                f"{field_path}.diagnostic_policy",
                calibration_error,
            ),
            state=state,
            return_components_stage_selected=require_boolean(
                raw_payload,
                "return_components_stage_selected",
                f"{field_path}.return_components_stage_selected",
                calibration_error,
            ),
            timestamp_batching_surface=TimestampBatchingSurface.from_mapping(
                dict(raw_timestamp_batching_surface),
                field_path=f"{field_path}.timestamp_batching_surface",
            ),
            message=require_string(
                raw_payload,
                "message",
                f"{field_path}.message",
                calibration_error,
            ),
        )


__all__ = [
    "CALIBRATION_TIMESTAMP_BATCHING_SURFACE_DIAGNOSTIC_POLICY",
    "CALIBRATION_TIMESTAMP_BATCHING_SURFACE_DIAGNOSTIC_SCHEMA_VERSION",
    "MarketDataTimestampBatchingSurfaceDiagnostic",
    "MarketDataTimestampBatchingSurfaceDiagnosticState",
]
