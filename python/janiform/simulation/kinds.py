from __future__ import annotations

from enum import StrEnum


class SimulationKind(StrEnum):
    SEEDED_TICK = "seeded_tick"
    COARSE_INTERVAL = "coarse_interval"


__all__ = ["SimulationKind"]
