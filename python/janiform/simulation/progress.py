from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from enum import StrEnum
from typing import Any

from .shared.errors import _simulation_error


class SimulationProgressPhase(StrEnum):
    SEEDED_TICK_INTERVALS = "seeded_tick_intervals"
    COARSE_INTERVALS = "coarse_intervals"


@dataclass(frozen=True, slots=True)
class SimulationProgressEvent:
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

    completed_units = payload.get("completed_units")
    if not isinstance(completed_units, int) or completed_units < 0:
        raise _simulation_error(
            "progress_event.completed_units",
            "must be a non-negative integer",
        )

    total_units = payload.get("total_units")
    if not isinstance(total_units, int) or total_units <= 0:
        raise _simulation_error(
            "progress_event.total_units",
            "must be a positive integer",
        )

    if completed_units > total_units:
        raise _simulation_error(
            "progress_event.completed_units",
            "must be less than or equal to total_units",
        )

    return SimulationProgressEvent(
        phase=phase,
        completed_units=completed_units,
        total_units=total_units,
    )


__all__ = [
    "SimulationProgressEvent",
    "SimulationProgressPhase",
    "simulation_progress_event_from_mapping",
    "simulation_progress_phase_detail",
]
