"""
Curated public calibration surface.

Use this package root for the high-level fit workflow and the primary
calibration entrypoints. Detailed request, preset, session, and staged
contracts live in their ownership submodules.
"""

from ._errors import CalibrationValidationError, CalibrationValidationFailure
from .fit import (
    CurrentStateEstimationRequest,
    CurrentStateEstimationResult,
    JaniformEstimator,
    JaniformFitReport,
    JaniformModel,
    JaniformSimulationResult,
    JaniformVanillaOptionBatch,
    JaniformVanillaOptionBatchPricingCalibrationHooks,
    JaniformVanillaOptionBatchPricingResult,
    JaniformVanillaOptionBatchPricingRunDiagnostics,
    JaniformVanillaOptionSurface,
    JaniformVanillaOptionSurfacePricingCalibrationHooks,
    JaniformVanillaOptionSurfacePricingResult,
    JaniformVanillaOptionSurfacePricingRunDiagnostics,
)
from .market_data import (
    CalibrationMeanReversionConfiguration,
    MarketDataCalibrationRequest,
    PreparedMarketDataCalibration,
    prepare_market_data_calibration,
    run_market_data_calibration,
)
from .presets import (
    DEFAULT_HIGH_LEVEL_KNOWN_NEWS_PRESET_NAME,
    KnownNewsExplicitConfiguration,
    KnownNewsPresetReference,
    MarketProfileReference,
    PresetRegistry,
    SessionExplicitConfiguration,
    SessionPresetReference,
    resolve_preset_workflow_configuration,
)
from .staged import (
    StagedCalibrationExecutionInput,
    StagedCalibrationRequest,
    StagedCalibrationResponse,
    run_staged_calibration,
)

__all__ = [
    "CalibrationMeanReversionConfiguration",
    "CalibrationValidationError",
    "CalibrationValidationFailure",
    "CurrentStateEstimationRequest",
    "CurrentStateEstimationResult",
    "DEFAULT_HIGH_LEVEL_KNOWN_NEWS_PRESET_NAME",
    "JaniformEstimator",
    "JaniformFitReport",
    "JaniformModel",
    "JaniformSimulationResult",
    "JaniformVanillaOptionBatch",
    "JaniformVanillaOptionBatchPricingCalibrationHooks",
    "JaniformVanillaOptionBatchPricingResult",
    "JaniformVanillaOptionBatchPricingRunDiagnostics",
    "JaniformVanillaOptionSurface",
    "JaniformVanillaOptionSurfacePricingCalibrationHooks",
    "JaniformVanillaOptionSurfacePricingResult",
    "JaniformVanillaOptionSurfacePricingRunDiagnostics",
    "KnownNewsExplicitConfiguration",
    "KnownNewsPresetReference",
    "MarketDataCalibrationRequest",
    "MarketProfileReference",
    "PresetRegistry",
    "PreparedMarketDataCalibration",
    "SessionExplicitConfiguration",
    "SessionPresetReference",
    "StagedCalibrationExecutionInput",
    "StagedCalibrationRequest",
    "StagedCalibrationResponse",
    "prepare_market_data_calibration",
    "resolve_preset_workflow_configuration",
    "run_market_data_calibration",
    "run_staged_calibration",
]
