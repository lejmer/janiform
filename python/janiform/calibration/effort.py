from __future__ import annotations

from dataclasses import dataclass
from math import isfinite
from typing import Any, Final

from .errors import calibration_error
from ..contracts import ContractModel
from ..contracts.base import require_positive_finite_number, require_positive_integer


@dataclass(frozen=True, slots=True)
class CalibrationEffortOptions(ContractModel):
    restart_candidate_budget: int
    restart_pool_total_budget_scale: float
    restart_pool_max_candidate_budget_scale: float

    def __post_init__(self) -> None:
        if self.restart_candidate_budget <= 0:
            raise calibration_error(
                "restart_candidate_budget",
                "must be strictly positive",
            )

        if (
            not isfinite(self.restart_pool_total_budget_scale)
            or self.restart_pool_total_budget_scale <= 0.0
        ):
            raise calibration_error(
                "restart_pool_total_budget_scale",
                "must be finite and strictly positive",
            )

        if (
            not isfinite(self.restart_pool_max_candidate_budget_scale)
            or self.restart_pool_max_candidate_budget_scale <= 0.0
        ):
            raise calibration_error(
                "restart_pool_max_candidate_budget_scale",
                "must be finite and strictly positive",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationEffortOptions:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        return CalibrationEffortOptions(
            restart_candidate_budget=require_positive_integer(
                raw_payload,
                "restart_candidate_budget",
                f"{field_path}.restart_candidate_budget",
                calibration_error,
            ),
            restart_pool_total_budget_scale=require_positive_finite_number(
                raw_payload,
                "restart_pool_total_budget_scale",
                f"{field_path}.restart_pool_total_budget_scale",
                calibration_error,
            ),
            restart_pool_max_candidate_budget_scale=require_positive_finite_number(
                raw_payload,
                "restart_pool_max_candidate_budget_scale",
                f"{field_path}.restart_pool_max_candidate_budget_scale",
                calibration_error,
            ),
        )


FAST_CALIBRATION_EFFORT: Final[CalibrationEffortOptions] = CalibrationEffortOptions(
    restart_candidate_budget=10,
    restart_pool_total_budget_scale=0.65,
    restart_pool_max_candidate_budget_scale=1.25,
)
STANDARD_CALIBRATION_EFFORT: Final[CalibrationEffortOptions] = CalibrationEffortOptions(
    restart_candidate_budget=15,
    restart_pool_total_budget_scale=1.0,
    restart_pool_max_candidate_budget_scale=2.0,
)
THOROUGH_CALIBRATION_EFFORT: Final[CalibrationEffortOptions] = CalibrationEffortOptions(
    restart_candidate_budget=24,
    restart_pool_total_budget_scale=1.75,
    restart_pool_max_candidate_budget_scale=3.0,
)
