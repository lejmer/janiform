from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
from typing import Any

from ..contracts import ContractModel, require_sequence, require_str_enum_value
from .errors import calibration_error


class CalibrationExtendedDiagnosticKind(StrEnum):
    OPTION_SURFACE_SHAPE = "option_surface_shape"


@dataclass(frozen=True, slots=True)
class CalibrationDiagnosticsRequest(ContractModel):
    extended: tuple[CalibrationExtendedDiagnosticKind, ...] = ()

    def __post_init__(self) -> None:
        for diagnostic_position, diagnostic_kind in enumerate(self.extended):
            if not isinstance(diagnostic_kind, CalibrationExtendedDiagnosticKind):
                raise calibration_error(
                    f"diagnostics.extended[{diagnostic_position}]",
                    "must be a CalibrationExtendedDiagnosticKind",
                )

        if len(set(self.extended)) != len(self.extended):
            raise calibration_error(
                "diagnostics.extended",
                "must not contain duplicate diagnostics",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationDiagnosticsRequest:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        raw_extended = require_sequence(
            raw_payload,
            "extended",
            f"{field_path}.extended",
            calibration_error,
        )

        return CalibrationDiagnosticsRequest(
            extended=tuple(
                require_str_enum_value(
                    raw_diagnostic,
                    f"{field_path}.extended[{diagnostic_position}]",
                    calibration_error,
                    enum_type=CalibrationExtendedDiagnosticKind,
                    allowed_values_message="must be one of: option_surface_shape",
                    include_received_value=True,
                )
                for diagnostic_position, raw_diagnostic in enumerate(raw_extended)
            ),
        )

    def option_surface_shape_requested(self) -> bool:
        return CalibrationExtendedDiagnosticKind.OPTION_SURFACE_SHAPE in self.extended
