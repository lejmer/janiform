from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from enum import StrEnum
from typing import Any

from .._progress_events import (
    BoundedProgressEvent,
    bounded_progress_counts_from_mapping,
)
from .shared.errors import _simulation_error


class SimulationProgressPhase(StrEnum):
    SEEDED_TICK_INTERVALS = "seeded_tick_intervals"
    COARSE_INTERVALS = "coarse_intervals"


@dataclass(frozen=True, slots=True)
class SimulationProgressEvent(BoundedProgressEvent):
    phase: SimulationProgressPhase
    completed_units: int
    total_units: int

    @property
    def detail(self) -> str:
        return simulation_progress_phase_detail(self.phase)


def simulation_progress_phase_detail(phase: SimulationProgressPhase | str) -> str:
    resolved_phase = (
        phase if isinstance(phase, SimulationProgressPhase) else SimulationProgressPhase(phase)
    )
    match resolved_phase:
        case SimulationProgressPhase.SEEDED_TICK_INTERVALS:
            return "seeded tick path generation"
        case SimulationProgressPhase.COARSE_INTERVALS:
            return "coarse interval path generation"


def simulation_progress_event_from_mapping(
    payload: Mapping[str, Any],
) -> SimulationProgressEvent:
    try:
        phase = SimulationProgressPhase(payload["phase"])
    except (KeyError, TypeError, ValueError) as error:
        raise _simulation_error(
            "progress_event.phase",
            "must be a recognized simulation progress phase",
        ) from error

    counts = bounded_progress_counts_from_mapping(
        payload,
        completed_units_field_path="progress_event.completed_units",
        total_units_field_path="progress_event.total_units",
        error_factory=_simulation_error,
    )

    return SimulationProgressEvent(
        phase=phase,
        completed_units=counts.completed_units,
        total_units=counts.total_units,
    )


__all__ = [
    "SimulationProgressEvent",
    "SimulationProgressPhase",
    "simulation_progress_event_from_mapping",
    "simulation_progress_phase_detail",
]
