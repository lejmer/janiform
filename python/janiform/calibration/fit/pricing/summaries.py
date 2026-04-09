"""Public maturity-summary pricing contracts."""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
from typing import Any, Optional

from ....contracts.base import (
    ContractModel,
    require_finite_number,
    require_finite_scalar,
    require_mapping,
    require_non_negative_finite_scalar,
    require_optional_finite_number,
    require_optional_mapping_value,
    require_positive_finite_scalar,
    require_sequence,
    require_str_enum_value,
)
from ...errors import calibration_error
from .internal.common import (
    tuple_of_finite_floats,
    tuple_of_non_negative_floats,
    validate_aligned_node_count,
    validate_optional_tuple_pair,
)


class JaniformVanillaOptionMaturityPricingEngineKind(StrEnum):
    DIRECT_FOURIER = "direct_fourier"
    FORWARD_LAW = "forward_law"


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
            raise calibration_error("interval_index", "must be non-negative")

        require_finite_scalar(self.current_anchor_value, "current_anchor_value", calibration_error)
        require_finite_scalar(self.next_anchor_value, "next_anchor_value", calibration_error)
        require_positive_finite_scalar(self.interval_delta, "interval_delta", calibration_error)
        require_non_negative_finite_scalar(
            self.maximum_tail_probability_mass,
            "maximum_tail_probability_mass",
            calibration_error,
        )
        require_non_negative_finite_scalar(
            self.maximum_row_probability_mass_error,
            "maximum_row_probability_mass_error",
            calibration_error,
        )
        require_finite_scalar(self.minimum_solved_theta, "minimum_solved_theta", calibration_error)
        require_finite_scalar(self.maximum_solved_theta, "maximum_solved_theta", calibration_error)

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> JaniformVanillaOptionMaturityIntervalSummary:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        return JaniformVanillaOptionMaturityIntervalSummary(
            interval_index=int(
                require_non_negative_finite_scalar(
                    raw_payload.get("interval_index"),
                    f"{field_path}.interval_index",
                    calibration_error,
                )
            ),
            current_anchor_value=require_finite_number(
                raw_payload,
                "current_anchor_value",
                f"{field_path}.current_anchor_value",
                calibration_error,
            ),
            next_anchor_value=require_finite_number(
                raw_payload,
                "next_anchor_value",
                f"{field_path}.next_anchor_value",
                calibration_error,
            ),
            interval_delta=require_positive_finite_scalar(
                raw_payload.get("interval_delta"),
                f"{field_path}.interval_delta",
                calibration_error,
            ),
            expected_hawkes_boundary_state_before=require_mapping(
                raw_payload,
                "expected_hawkes_boundary_state_before",
                f"{field_path}.expected_hawkes_boundary_state_before",
                calibration_error,
            ),
            expected_hawkes_boundary_state_after=require_mapping(
                raw_payload,
                "expected_hawkes_boundary_state_after",
                f"{field_path}.expected_hawkes_boundary_state_after",
                calibration_error,
            ),
            maximum_tail_probability_mass=require_non_negative_finite_scalar(
                raw_payload.get("maximum_tail_probability_mass"),
                f"{field_path}.maximum_tail_probability_mass",
                calibration_error,
            ),
            maximum_row_probability_mass_error=require_non_negative_finite_scalar(
                raw_payload.get("maximum_row_probability_mass_error"),
                f"{field_path}.maximum_row_probability_mass_error",
                calibration_error,
            ),
            minimum_solved_theta=require_finite_number(
                raw_payload,
                "minimum_solved_theta",
                f"{field_path}.minimum_solved_theta",
                calibration_error,
            ),
            maximum_solved_theta=require_finite_number(
                raw_payload,
                "maximum_solved_theta",
                f"{field_path}.maximum_solved_theta",
                calibration_error,
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
        require_positive_finite_scalar(self.maturity_interval, "maturity_interval", calibration_error)

        if self.deviation_grid_node_count is not None and self.deviation_grid_node_count <= 0:
            raise calibration_error(
                "deviation_grid_node_count",
                "must be strictly positive when provided",
            )

        validate_optional_tuple_pair(
            self.terminal_deviation_nodes,
            self.terminal_probability_mass,
            left_field_path="terminal_deviation_nodes",
            right_field_path="terminal_probability_mass",
        )

        if self.terminal_deviation_nodes is not None:
            validate_aligned_node_count(
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
            raise calibration_error(field_path, "must be an object")

        raw_interval_summaries = require_sequence(
            raw_payload,
            "interval_summaries",
            f"{field_path}.interval_summaries",
            calibration_error,
        )

        raw_deviation_grid_node_count = raw_payload.get("deviation_grid_node_count")
        deviation_grid_node_count: Optional[int] = None
        if raw_deviation_grid_node_count is not None:
            deviation_grid_node_count = int(
                require_positive_finite_scalar(
                    raw_deviation_grid_node_count,
                    f"{field_path}.deviation_grid_node_count",
                    calibration_error,
                )
            )

        return JaniformVanillaOptionMaturitySummary(
            maturity_interval=require_positive_finite_scalar(
                raw_payload.get("maturity_interval"),
                f"{field_path}.maturity_interval",
                calibration_error,
            ),
            engine_kind=require_str_enum_value(
                raw_payload.get("engine_kind"),
                f"{field_path}.engine_kind",
                calibration_error,
                enum_type=JaniformVanillaOptionMaturityPricingEngineKind,
                allowed_values_message="must be one of: direct_fourier, forward_law",
            ),
            terminal_anchor_value=require_optional_finite_number(
                raw_payload,
                "terminal_anchor_value",
                f"{field_path}.terminal_anchor_value",
                calibration_error,
            ),
            deviation_grid_lower_bound=require_optional_finite_number(
                raw_payload,
                "deviation_grid_lower_bound",
                f"{field_path}.deviation_grid_lower_bound",
                calibration_error,
            ),
            deviation_grid_upper_bound=require_optional_finite_number(
                raw_payload,
                "deviation_grid_upper_bound",
                f"{field_path}.deviation_grid_upper_bound",
                calibration_error,
            ),
            deviation_grid_node_count=deviation_grid_node_count,
            terminal_deviation_nodes=(
                tuple_of_finite_floats(
                    raw_payload,
                    field_name="terminal_deviation_nodes",
                    field_path=f"{field_path}.terminal_deviation_nodes",
                )
                if raw_payload.get("terminal_deviation_nodes") is not None
                else None
            ),
            terminal_probability_mass=(
                tuple_of_non_negative_floats(
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
                calibration_error,
            ),
            integration_quality=require_optional_mapping_value(
                raw_payload.get("integration_quality"),
                f"{field_path}.integration_quality",
                calibration_error,
            ),
        )


__all__ = [
    "JaniformVanillaOptionMaturityIntervalSummary",
    "JaniformVanillaOptionMaturityPricingEngineKind",
    "JaniformVanillaOptionMaturitySummary",
]
