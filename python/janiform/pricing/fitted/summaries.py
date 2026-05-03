"""Public maturity-summary pricing contracts."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Optional

from janiform.contracts.base import (
    ContractModel,
    require_finite_number,
    require_finite_scalar,
    require_mapping,
    require_non_negative_finite_scalar,
    require_optional_mapping_value,
    require_positive_finite_scalar,
    require_sequence,
)
from janiform.pricing.errors import _pricing_error
from .internal.common import tuple_of_finite_floats, validate_aligned_node_count


@dataclass(frozen=True, slots=True)
class JaniformVanillaOptionMaturityIntervalSummary(ContractModel):
    interval_index: int
    current_anchor_value: float
    next_anchor_value: float
    interval_delta: float
    expected_hawkes_boundary_state_before: dict[str, float]
    expected_hawkes_boundary_state_after: dict[str, float]
    minimum_solved_theta: float
    maximum_solved_theta: float

    def __post_init__(self) -> None:
        if self.interval_index < 0:
            raise _pricing_error("interval_index", "must be non-negative")

        require_finite_scalar(self.current_anchor_value, "current_anchor_value", _pricing_error)
        require_finite_scalar(self.next_anchor_value, "next_anchor_value", _pricing_error)
        require_positive_finite_scalar(self.interval_delta, "interval_delta", _pricing_error)
        require_finite_scalar(self.minimum_solved_theta, "minimum_solved_theta", _pricing_error)
        require_finite_scalar(self.maximum_solved_theta, "maximum_solved_theta", _pricing_error)

        if self.minimum_solved_theta > self.maximum_solved_theta:
            raise _pricing_error(
                "minimum_solved_theta",
                "must be less than or equal to maximum_solved_theta",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> JaniformVanillaOptionMaturityIntervalSummary:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        return JaniformVanillaOptionMaturityIntervalSummary(
            interval_index=int(
                require_non_negative_finite_scalar(
                    raw_payload.get("interval_index"),
                    f"{field_path}.interval_index",
                    _pricing_error,
                )
            ),
            current_anchor_value=require_finite_number(
                raw_payload,
                "current_anchor_value",
                f"{field_path}.current_anchor_value",
                _pricing_error,
            ),
            next_anchor_value=require_finite_number(
                raw_payload,
                "next_anchor_value",
                f"{field_path}.next_anchor_value",
                _pricing_error,
            ),
            interval_delta=require_positive_finite_scalar(
                raw_payload.get("interval_delta"),
                f"{field_path}.interval_delta",
                _pricing_error,
            ),
            expected_hawkes_boundary_state_before=require_mapping(
                raw_payload,
                "expected_hawkes_boundary_state_before",
                f"{field_path}.expected_hawkes_boundary_state_before",
                _pricing_error,
            ),
            expected_hawkes_boundary_state_after=require_mapping(
                raw_payload,
                "expected_hawkes_boundary_state_after",
                f"{field_path}.expected_hawkes_boundary_state_after",
                _pricing_error,
            ),
            minimum_solved_theta=require_finite_number(
                raw_payload,
                "minimum_solved_theta",
                f"{field_path}.minimum_solved_theta",
                _pricing_error,
            ),
            maximum_solved_theta=require_finite_number(
                raw_payload,
                "maximum_solved_theta",
                f"{field_path}.maximum_solved_theta",
                _pricing_error,
            ),
        )


@dataclass(frozen=True, slots=True)
class JaniformVanillaOptionMaturitySummary(ContractModel):
    maturity_interval: float
    terminal_anchor_value: float
    deviation_domain_lower_bound: float
    deviation_domain_upper_bound: float
    collocation_node_count: int
    collocation_nodes: tuple[float, ...]
    interval_summaries: tuple[JaniformVanillaOptionMaturityIntervalSummary, ...]
    representative_transform: Optional[dict[str, Any]]
    integration_quality: Optional[dict[str, Any]]

    def __post_init__(self) -> None:
        require_positive_finite_scalar(
            self.maturity_interval,
            "maturity_interval",
            _pricing_error,
        )
        require_finite_scalar(
            self.terminal_anchor_value,
            "terminal_anchor_value",
            _pricing_error,
        )
        require_finite_scalar(
            self.deviation_domain_lower_bound,
            "deviation_domain_lower_bound",
            _pricing_error,
        )
        require_finite_scalar(
            self.deviation_domain_upper_bound,
            "deviation_domain_upper_bound",
            _pricing_error,
        )

        if self.deviation_domain_lower_bound >= self.deviation_domain_upper_bound:
            raise _pricing_error(
                "deviation_domain_lower_bound",
                "must be strictly less than deviation_domain_upper_bound",
            )

        if self.collocation_node_count <= 0:
            raise _pricing_error("collocation_node_count", "must be strictly positive")

        validate_aligned_node_count(
            self.collocation_nodes,
            self.collocation_node_count,
            field_path="collocation_nodes",
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> JaniformVanillaOptionMaturitySummary:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        raw_interval_summaries = require_sequence(
            raw_payload,
            "interval_summaries",
            f"{field_path}.interval_summaries",
            _pricing_error,
        )

        collocation_node_count = int(
            require_positive_finite_scalar(
                raw_payload.get("collocation_node_count"),
                f"{field_path}.collocation_node_count",
                _pricing_error,
            )
        )

        return JaniformVanillaOptionMaturitySummary(
            maturity_interval=require_positive_finite_scalar(
                raw_payload.get("maturity_interval"),
                f"{field_path}.maturity_interval",
                _pricing_error,
            ),
            terminal_anchor_value=require_finite_number(
                raw_payload,
                "terminal_anchor_value",
                f"{field_path}.terminal_anchor_value",
                _pricing_error,
            ),
            deviation_domain_lower_bound=require_finite_number(
                raw_payload,
                "deviation_domain_lower_bound",
                f"{field_path}.deviation_domain_lower_bound",
                _pricing_error,
            ),
            deviation_domain_upper_bound=require_finite_number(
                raw_payload,
                "deviation_domain_upper_bound",
                f"{field_path}.deviation_domain_upper_bound",
                _pricing_error,
            ),
            collocation_node_count=collocation_node_count,
            collocation_nodes=tuple_of_finite_floats(
                raw_payload,
                field_name="collocation_nodes",
                field_path=f"{field_path}.collocation_nodes",
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
                _pricing_error,
            ),
            integration_quality=require_optional_mapping_value(
                raw_payload.get("integration_quality"),
                f"{field_path}.integration_quality",
                _pricing_error,
            ),
        )


__all__ = [
    "JaniformVanillaOptionMaturityIntervalSummary",
    "JaniformVanillaOptionMaturitySummary",
]
