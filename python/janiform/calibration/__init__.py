"""
Curated public calibration root surface.

Use this package root for the main high-level fit entrypoints and root-level
validation errors. Fitted model, pricing, simulation, state, preset,
market-data, and staged contracts live in their ownership submodules.
"""

from .errors import CalibrationValidationError
from .fit import (
    JaniformEstimator,
    JaniformFitReport,
    JaniformFitResult,
)

__all__ = [
    "CalibrationValidationError",
    "JaniformEstimator",
    "JaniformFitReport",
    "JaniformFitResult",
]
