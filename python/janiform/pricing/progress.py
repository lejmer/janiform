from dataclasses import dataclass
from enum import StrEnum
from typing import Any, Optional
from collections.abc import Mapping

from ..contracts.base import (
    require_non_negative_integer,
    require_optional_non_negative_integer_value,
    require_optional_positive_integer_value,
    require_positive_integer,
)
from .errors import _pricing_error


class PricingProgressPhase(StrEnum):
    GROUP_PROGRESS = "group_progress"
    MATURITY_PROGRESS = "maturity_progress"
    INTERVAL_PROGRESS = "interval_progress"
    TRANSITION_ROW_PROGRESS = "transition_row_progress"
    STRIKE_PRICING_PROGRESS = "strike_pricing_progress"


@dataclass(frozen=True, slots=True)
class PricingProgressEvent:
    phase: PricingProgressPhase
    completed_units: int
    total_units: int
    current_group_index: Optional[int]
    current_maturity_index: Optional[int]
    total_maturities: Optional[int]
    current_interval_index: Optional[int]
    total_intervals: Optional[int]
    current_transition_row_index: Optional[int]
    total_transition_rows: Optional[int]
    current_strike_index: Optional[int]
    total_strikes: Optional[int]

    @property
    def subdetail(self) -> Optional[str]:
        match self.phase:
            case PricingProgressPhase.GROUP_PROGRESS:
                if self.current_group_index is None:
                    return None

                return f"group {self.current_group_index + 1}/{self.total_units}"
            case PricingProgressPhase.MATURITY_PROGRESS:
                if self.current_maturity_index is None or self.total_maturities is None:
                    return None

                return (
                    f"maturity {self.current_maturity_index + 1}/"
                    f"{self.total_maturities}"
                )
            case PricingProgressPhase.INTERVAL_PROGRESS:
                if self.current_interval_index is None or self.total_intervals is None:
                    return None

                return (
                    f"interval {self.current_interval_index + 1}/"
                    f"{self.total_intervals}"
                )
            case PricingProgressPhase.TRANSITION_ROW_PROGRESS:
                interval_detail = None
                if self.current_interval_index is not None and self.total_intervals is not None:
                    interval_detail = (
                        f"interval {self.current_interval_index + 1}/{self.total_intervals}"
                    )

                if (
                    self.current_transition_row_index is None
                    or self.total_transition_rows is None
                ):
                    return interval_detail

                row_detail = (
                    f"transition row "
                    f"{self.current_transition_row_index + 1}/"
                    f"{self.total_transition_rows}"
                )
                if interval_detail is None:
                    return row_detail

                return f"{interval_detail} | {row_detail}"
            case PricingProgressPhase.STRIKE_PRICING_PROGRESS:
                strike_detail = None
                if self.current_strike_index is not None and self.total_strikes is not None:
                    strike_detail = (
                        f"strike {self.current_strike_index + 1}/{self.total_strikes}"
                    )

                if self.current_maturity_index is None or self.total_maturities is None:
                    return strike_detail

                maturity_detail = (
                    f"maturity {self.current_maturity_index + 1}/{self.total_maturities}"
                )
                if strike_detail is None:
                    return maturity_detail

                return f"{maturity_detail} | {strike_detail}"


def pricing_progress_event_from_mapping(payload: Mapping[str, Any]) -> PricingProgressEvent:
    try:
        phase = PricingProgressPhase(payload["phase"])
    except (KeyError, TypeError, ValueError) as error:
        raise _pricing_error(
            "progress_event.phase",
            "must be a recognized pricing progress phase",
        ) from error

    completed_units = require_non_negative_integer(
        payload,
        "completed_units",
        "progress_event.completed_units",
        _pricing_error,
    )
    total_units = require_positive_integer(
        payload,
        "total_units",
        "progress_event.total_units",
        _pricing_error,
    )
    if completed_units > total_units:
        raise _pricing_error(
            "progress_event.completed_units",
            "must be less than or equal to total_units",
        )

    current_group_index = require_optional_non_negative_integer_value(
        payload.get("current_group_index"),
        "progress_event.current_group_index",
        _pricing_error,
    )
    current_maturity_index = require_optional_non_negative_integer_value(
        payload.get("current_maturity_index"),
        "progress_event.current_maturity_index",
        _pricing_error,
    )
    total_maturities = require_optional_positive_integer_value(
        payload.get("total_maturities"),
        "progress_event.total_maturities",
        _pricing_error,
    )
    current_interval_index = require_optional_non_negative_integer_value(
        payload.get("current_interval_index"),
        "progress_event.current_interval_index",
        _pricing_error,
    )
    total_intervals = require_optional_positive_integer_value(
        payload.get("total_intervals"),
        "progress_event.total_intervals",
        _pricing_error,
    )
    current_transition_row_index = require_optional_non_negative_integer_value(
        payload.get("current_transition_row_index"),
        "progress_event.current_transition_row_index",
        _pricing_error,
    )
    total_transition_rows = require_optional_positive_integer_value(
        payload.get("total_transition_rows"),
        "progress_event.total_transition_rows",
        _pricing_error,
    )
    current_strike_index = require_optional_non_negative_integer_value(
        payload.get("current_strike_index"),
        "progress_event.current_strike_index",
        _pricing_error,
    )
    total_strikes = require_optional_positive_integer_value(
        payload.get("total_strikes"),
        "progress_event.total_strikes",
        _pricing_error,
    )

    return PricingProgressEvent(
        phase=phase,
        completed_units=completed_units,
        total_units=total_units,
        current_group_index=current_group_index,
        current_maturity_index=current_maturity_index,
        total_maturities=total_maturities,
        current_interval_index=current_interval_index,
        total_intervals=total_intervals,
        current_transition_row_index=current_transition_row_index,
        total_transition_rows=total_transition_rows,
        current_strike_index=current_strike_index,
        total_strikes=total_strikes,
    )


__all__ = [
    "PricingProgressEvent",
    "PricingProgressPhase",
    "pricing_progress_event_from_mapping",
]
