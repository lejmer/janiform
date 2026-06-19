"""Public current-state estimation contracts for fitted Janiform models."""

from __future__ import annotations

from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from datetime import datetime
from typing import Any, Optional

import pandas

from ...._timestamps import utc_nanos_to_datetime
from ....contracts import ContractModel
from ....contracts.base import (
    require_mapping,
    require_non_negative_integer,
    require_positive_finite_scalar,
)
from ....long_variance import LongVarianceState
from ....mean_reversion import MeanReversionBoundaryState
from ....simulation import HawkesExcitationState
from ...errors import calibration_error
from ...market_data.contracts import CalibrationTickDataColumnMapping


@dataclass(frozen=True, slots=True, eq=False)
class CurrentStateEstimationRequest:
    """Observed tick data required to estimate the current fitted-model runtime state."""

    observed_tick_data: pandas.DataFrame | Mapping[str, Sequence[Any]]
    tick_data_column_mapping: Optional[CalibrationTickDataColumnMapping] = None
    observed_price: Optional[float] = None

    def __post_init__(self) -> None:
        if not isinstance(self.observed_tick_data, pandas.DataFrame) and not isinstance(
            self.observed_tick_data,
            Mapping,
        ):
            raise calibration_error(
                "observed_tick_data",
                "must be a pandas.DataFrame or a mapping of column arrays",
            )

        if (
            self.tick_data_column_mapping is not None
            and not isinstance(
                self.tick_data_column_mapping,
                CalibrationTickDataColumnMapping,
            )
        ):
            raise calibration_error(
                "tick_data_column_mapping",
                "must be a CalibrationTickDataColumnMapping when provided",
            )

        if self.observed_price is not None:
            require_positive_finite_scalar(
                self.observed_price,
                "observed_price",
                calibration_error,
            )

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, CurrentStateEstimationRequest):
            return NotImplemented

        return (
            self.tick_data_column_mapping == other.tick_data_column_mapping
            and self.observed_price == other.observed_price
            and _observed_tick_data_equal(
                self.observed_tick_data,
                other.observed_tick_data,
            )
        )


@dataclass(frozen=True, slots=True)
class CurrentStateEstimationResult(ContractModel):
    """Current runtime state derived from a fitted model and observed signed trades."""

    hawkes_excitation_state: HawkesExcitationState
    long_variance_state: LongVarianceState
    mean_reversion_boundary_state: MeanReversionBoundaryState

    def __post_init__(self) -> None:
        if not isinstance(self.hawkes_excitation_state, HawkesExcitationState):
            raise calibration_error(
                "hawkes_excitation_state",
                "must be a HawkesExcitationState",
            )

        if not isinstance(self.long_variance_state, LongVarianceState):
            raise calibration_error(
                "long_variance_state",
                "must be a LongVarianceState",
            )

        if not isinstance(self.mean_reversion_boundary_state, MeanReversionBoundaryState):
            raise calibration_error(
                "mean_reversion_boundary_state",
                "must be a MeanReversionBoundaryState",
            )

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str = "current_state",
    ) -> CurrentStateEstimationResult:
        payload = require_mapping(
            {"value": raw_payload},
            "value",
            field_path,
            calibration_error,
        )

        return CurrentStateEstimationResult(
            hawkes_excitation_state=HawkesExcitationState.from_mapping(
                payload.get("hawkes_excitation_state"),
                field_path=f"{field_path}.hawkes_excitation_state",
            ),
            long_variance_state=LongVarianceState.from_mapping(
                payload.get("long_variance_state"),
                field_path=f"{field_path}.long_variance_state",
                error_factory=calibration_error,
            ),
            mean_reversion_boundary_state=MeanReversionBoundaryState.from_mapping(
                payload.get("mean_reversion_boundary_state"),
                field_path=f"{field_path}.mean_reversion_boundary_state",
            ),
        )


@dataclass(frozen=True, slots=True)
class CurrentStateTrackerSnapshot(ContractModel):
    """Exact fitted-model tracker snapshot returned after a state update."""

    state_time_utc_nanos: int
    observed_price: float
    state: CurrentStateEstimationResult

    def __post_init__(self) -> None:
        require_non_negative_integer(
            {"state_time_utc_nanos": self.state_time_utc_nanos},
            "state_time_utc_nanos",
            "state_time_utc_nanos",
            calibration_error,
        )

        require_positive_finite_scalar(
            self.observed_price,
            "observed_price",
            calibration_error,
        )

        if not isinstance(self.state, CurrentStateEstimationResult):
            raise calibration_error(
                "state",
                "must be a CurrentStateEstimationResult",
            )

    @property
    def state_time_utc(self) -> datetime:
        return utc_nanos_to_datetime(self.state_time_utc_nanos)

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str = "current_state_tracker_snapshot",
    ) -> CurrentStateTrackerSnapshot:
        payload = require_mapping(
            {"value": raw_payload},
            "value",
            field_path,
            calibration_error,
        )

        raw_observed_price = payload.get("observed_price")

        observed_price = require_positive_finite_scalar(
            raw_observed_price,
            f"{field_path}.observed_price",
            calibration_error,
        )

        return CurrentStateTrackerSnapshot(
            state_time_utc_nanos=require_non_negative_integer(
                payload,
                "state_time_utc_nanos",
                f"{field_path}.state_time_utc_nanos",
                calibration_error,
            ),
            observed_price=observed_price,
            state=CurrentStateEstimationResult.from_mapping(
                payload.get("state"),
                field_path=f"{field_path}.state",
            ),
        )


def _observed_tick_data_equal(
    left: pandas.DataFrame | Mapping[str, Sequence[Any]],
    right: pandas.DataFrame | Mapping[str, Sequence[Any]],
) -> bool:
    if isinstance(left, pandas.DataFrame) and isinstance(right, pandas.DataFrame):
        return left.equals(right)

    if isinstance(left, Mapping) and isinstance(right, Mapping):
        if set(left) != set(right):
            return False

        return all(tuple(left[column_name]) == tuple(right[column_name]) for column_name in left)

    return False


__all__ = [
    "CurrentStateEstimationRequest",
    "CurrentStateEstimationResult",
    "CurrentStateTrackerSnapshot",
]
