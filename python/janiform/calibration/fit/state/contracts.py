"""Public current-state estimation contracts for fitted Janiform models."""

from __future__ import annotations

from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from typing import Any, Optional

import pandas

from ....mean_reversion import MeanReversionBoundaryState
from ....contracts import require_positive_finite_scalar
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
class CurrentStateEstimationResult:
    """Current runtime state derived from a fitted model and observed signed trades."""

    hawkes_excitation_state: HawkesExcitationState
    mean_reversion_boundary_state: MeanReversionBoundaryState

    def __post_init__(self) -> None:
        if not isinstance(self.hawkes_excitation_state, HawkesExcitationState):
            raise calibration_error(
                "hawkes_excitation_state",
                "must be a HawkesExcitationState",
            )
        if not isinstance(self.mean_reversion_boundary_state, MeanReversionBoundaryState):
            raise calibration_error(
                "mean_reversion_boundary_state",
                "must be a MeanReversionBoundaryState",
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
]
