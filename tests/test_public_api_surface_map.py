from importlib import import_module
from pathlib import Path

import pytest

import janiform
import janiform.errors as errors_module
import janiform.hourly_factors as hourly_factors_module
import janiform.pricing as pricing_module
import janiform.risk as risk_module
import janiform.simulation as simulation_module
import janiform.validation as validation_module

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
README_PATH = REPOSITORY_ROOT / "README.md"


def test_package_root_surface_is_intentionally_minimal() -> None:
    assert janiform.__all__ == ["__version__"]
    assert "__version__" in janiform.__dict__

    root_exports = set(janiform.__all__)

    assert "run_seeded_tick_event_simulation" not in root_exports
    assert "load_hourly_factor_contract_for_pricing" not in root_exports
    assert "validate_sufficient_statistics_risk_run_request" not in root_exports
    assert "native_module" not in root_exports


def test_simulation_public_symbols_live_in_simulation_module() -> None:
    expected_exports = {
        "COARSE_SIMULATION_ARTIFACT_WORKFLOW_POLICY",
        "COARSE_SIMULATION_COMPATIBLE_INTERVAL_GRID_POLICY",
        "COARSE_SIMULATION_ENVIRONMENT_METADATA_POLICY",
        "COARSE_SIMULATION_INTERVAL_FACTOR_ALIGNMENT_POLICY",
        "COARSE_SIMULATION_INTERVAL_IDENTIFIER_POLICY",
        "COARSE_SIMULATION_INTERVAL_TIMING_POLICY",
        "COARSE_SIMULATION_PATH_DIAGNOSTICS_SCHEMA_VERSION",
        "COARSE_SIMULATION_PATH_OUTPUT_SCHEMA_METADATA_VERSION",
        "COARSE_SIMULATION_PATH_PROVENANCE_SCHEMA_VERSION",
        "COARSE_SIMULATION_PATH_RECORD_SCHEMA_VERSION",
        "COARSE_SIMULATION_PATH_SCHEMA_VERSION",
        "COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION",
        "CoarseSimulationModelInputs",
        "CoarseSimulationPath",
        "CoarseSimulationPathDiagnostics",
        "CoarseSimulationPathOutputSchemaMetadata",
        "CoarseSimulationPathProvenance",
        "CoarseSimulationPathRecord",
        "CoarseSimulationRunRequest",
        "CoupledGapMarkConfiguration",
        "DoubleExponentialBaseShockConfiguration",
        "HawkesExcitationState",
        "HawkesKernelLoadings",
        "HawkesKernelParameters",
        "KnownNewsArrivalConfiguration",
        "NewsArrivalConfiguration",
        "NewsComponentMarkScaleConfiguration",
        "NewsProcessConfiguration",
        "SeededTickEventRecord",
        "SeededTickEventSide",
        "SeededTickEventStateSnapshot",
        "SeededTickEventStateSnapshotCapture",
        "SeededTickEventTapeMetadata",
        "SeededTickSimulationHourlyFactorSummary",
        "SeededTickSimulationRequest",
        "SeededTickSimulationResponse",
        "SeededTickSimulationValidationError",
        "SeededTickSimulationValidationFailure",
        "SharedNewsMarkLawConfiguration",
        "SurpriseNewsArrivalConfiguration",
        "deserialize_coarse_simulation_path",
        "deserialize_coarse_simulation_run_request",
        "deserialize_seeded_tick_simulation_response",
        "load_hourly_factor_contract_for_simulation",
        "run_coarse_simulation",
        "run_seeded_tick_event_simulation",
        "serialize_coarse_simulation_path",
        "serialize_coarse_simulation_run_request",
        "serialize_seeded_tick_simulation_response",
        "validate_coarse_simulation_path",
        "validate_coarse_simulation_run_request",
        "validate_seeded_tick_simulation_response",
    }

    assert expected_exports <= set(simulation_module.__all__)
    assert expected_exports <= set(dir(simulation_module))


def test_simulation_public_module_is_a_package() -> None:
    assert hasattr(simulation_module, "__path__")


def test_pricing_public_symbols_live_in_pricing_module() -> None:
    expected_exports = {
        "GammaSeverityMarkConfiguration",
        "NigSharedPlusShiftConfiguration",
        "VanillaOptionPricingCalibrationHooks",
        "VanillaOptionPricingExecutionInput",
        "VanillaOptionPricingIntervalFactors",
        "VanillaOptionPricingOptionalOutputSelection",
        "VanillaOptionPayoffConfiguration",
        "VanillaOptionPayoffType",
        "VanillaOptionPricingDiagnostics",
        "VanillaOptionPricingMarketInputs",
        "VanillaOptionPricingModelParameters",
        "VanillaOptionPricingRequest",
        "VanillaOptionPricingResponse",
        "VanillaOptionPricingRunDiagnostics",
        "VanillaOptionPricingRunOutputSchemaMetadata",
        "VanillaOptionPricingRunResult",
        "VanillaOptionPricingValidationError",
        "VanillaOptionPricingValidationFailure",
        "deserialize_vanilla_option_pricing_execution_input",
        "load_hourly_factor_contract_for_pricing",
        "run_vanilla_option_pricing",
        "serialize_vanilla_option_pricing_execution_input",
        "validate_vanilla_option_pricing_request",
        "validate_vanilla_option_pricing_response",
        "validate_vanilla_option_pricing_execution_input",
        "validate_vanilla_option_pricing_run_result",
        "serialize_vanilla_option_pricing_request",
        "serialize_vanilla_option_pricing_response",
        "serialize_vanilla_option_pricing_run_result",
        "deserialize_vanilla_option_pricing_request",
        "deserialize_vanilla_option_pricing_response",
        "deserialize_vanilla_option_pricing_run_result",
    }

    assert expected_exports <= set(pricing_module.__all__)
    assert expected_exports <= set(dir(pricing_module))


def test_risk_public_symbols_live_in_risk_module() -> None:
    expected_exports = {
        "SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY",
        "SufficientStatisticsRiskEquationSpecification",
        "SufficientStatisticsRiskIntervalFactors",
        "SufficientStatisticsRiskModelInputs",
        "SufficientStatisticsRiskRunRequest",
        "SufficientStatisticsIntervalStatistics",
        "SufficientStatisticsRiskReturnComponents",
        "SufficientStatisticsRiskPathRecord",
        "SufficientStatisticsRiskPath",
        "SufficientStatisticsRiskValidationError",
        "SufficientStatisticsRiskValidationFailure",
        "validate_sufficient_statistics_risk_run_request",
        "validate_sufficient_statistics_risk_path",
        "serialize_sufficient_statistics_risk_run_request",
        "serialize_sufficient_statistics_risk_path",
        "deserialize_sufficient_statistics_risk_run_request",
        "deserialize_sufficient_statistics_risk_path",
    }

    assert expected_exports <= set(risk_module.__all__)
    assert expected_exports <= set(dir(risk_module))


def test_hourly_factor_support_symbols_live_in_hourly_factor_module() -> None:
    expected_exports = {
        "HourlyFactorContract",
        "HourlyNewsIntensityProviderExplicitSource",
        "HourlyNewsIntensityProviderMaterializedSource",
        "HourlySeasonalityProviderExplicitSource",
        "HourlySeasonalityProviderMaterializedSource",
        "build_hourly_factor_contract",
        "build_runtime_hourly_seasonality_source_from_calibration_payload",
        "materialize_hourly_news_intensity_factors",
        "materialize_hourly_seasonality_factors",
        "provide_hourly_factor_contract",
    }

    assert expected_exports <= set(hourly_factors_module.__all__)
    assert expected_exports <= set(dir(hourly_factors_module))


def test_private_modules_are_not_exposed_through_public_exports() -> None:
    assert "native_module" not in validation_module.__all__
    assert "harness" not in validation_module.__all__

    with pytest.raises(ModuleNotFoundError):
        import_module("janiform.api")


def test_errors_public_symbols_live_in_errors_module() -> None:
    expected_exports = {
        "ContractValidationFailure",
        "JaniformError",
        "JaniformErrorCategory",
        "JaniformErrorDetails",
        "JaniformExecutionError",
        "JaniformIntegrationError",
        "JaniformValidationError",
        "format_janiform_error",
    }

    assert expected_exports <= set(errors_module.__all__)
    assert expected_exports <= set(dir(errors_module))


def test_calibration_public_symbols_live_in_calibration_module() -> None:
    calibration_module = import_module("janiform.calibration")

    expected_exports = {
        "CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY",
        "CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION",
        "CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_SCHEMA_VERSION",
        "CALIBRATION_RUN_SPEC_SCHEMA_VERSION",
        "CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION",
        "CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION",
        "CalibrationCoupledGapMarkConfiguration",
        "CalibrationCheckpointMetadata",
        "CalibrationDuplicateRecordPolicy",
        "CalibrationDayOfWeekSeasonalityEstimatorSettings",
        "CalibrationDayOfYearSeasonalityEstimatorSettings",
        "CalibrationDoubleExponentialBaseShockConfiguration",
        "CalibrationExecutionMode",
        "CalibrationFailureDetails",
        "CalibrationInputFileFormat",
        "CalibrationInputFileReference",
        "CalibrationHawkesStageCandidateInput",
        "CalibrationHawkesStructuredAlphaParameterization",
        "CalibrationHawkesStructuredRhoParameterization",
        "CalibrationHawkesStructuredRhoSelfDominancePolicy",
        "CalibrationHandoffTransport",
        "CalibrationIntradaySeasonalityEstimatorSettings",
        "CalibrationIntradaySmoothingKernel",
        "CalibrationKnownNewsColumnMapping",
        "CalibrationKnownNewsInput",
        "CalibrationMarketDataColumnMapping",
        "CalibrationMarketDataNormalizationPolicy",
        "CalibrationMarketDataOutputPreferences",
        "CalibrationMarketDataWorkflowScope",
        "MarketDataSessionRecurringClosedWindow",
        "MarketDataSessionRegularWindow",
        "MarketDataSessionResolution",
        "CalibrationOptimizerSettings",
        "CalibrationOptionQuoteInputPolicy",
        "CalibrationOptionSurfaceColumnMapping",
        "CalibrationObjectiveTermInput",
        "CalibrationReturnComponentsCloseOpenGapDetectionPolicy",
        "CalibrationReturnComponentsDetectedIntradayJumpCandidate",
        "CalibrationReturnComponentsKnownNewsCalendarInput",
        "CalibrationReturnComponentsKnownNewsReturnSample",
        "CalibrationReturnComponentsObservedCloseOpenBoundaryObservation",
        "CalibrationReturnComponentsObservedOpenSessionReturn",
        "CalibrationReturnComponentsObjectiveMetricWeights",
        "CalibrationReturnComponentsSignalSourceWindow",
        "CalibrationReturnComponentsStageCandidateGenerationInput",
        "CalibrationResumeSettings",
        "CalibrationResumeSource",
        "CalibrationRunDataInputs",
        "CalibrationRunSeedSettings",
        "CalibrationScheduledEventOverlay",
        "CalibrationSeasonalityStageExecutionInput",
        "CalibrationSeasonalityStageExecutionSettings",
        "CalibrationSessionConfigurationInput",
        "CalibrationSharedNewsMarkLawConfiguration",
        "CalibrationStage",
        "CalibrationStageExecutionStatus",
        "CalibrationStageHandoff",
        "CalibrationStageReport",
        "CalibrationStageSeedOverride",
        "CalibrationStageSolverSettings",
        "CalibrationStagedExecutionRuntimeResources",
        "CalibrationTimestampEventRecord",
        "CalibrationTimestampEventSessionFilter",
        "CalibrationTickDataColumnMapping",
        "CalibrationTickPriceInputPolicy",
        "CalibrationValidationError",
        "CalibrationValidationFailure",
        "DEFAULT_HIGH_LEVEL_KNOWN_NEWS_PRESET_NAME",
        "DEFAULT_PYTHON_IMPORTABLE_API_EXECUTION_MODE",
        "KnownNewsConfigurationOverridePatch",
        "KnownNewsEvent",
        "KnownNewsExplicitConfiguration",
        "KnownNewsPresetReference",
        "MarketDataCalibrationRequest",
        "MarketProfileColumnAliasHint",
        "MarketProfileExplicitConfiguration",
        "MarketProfileKnownNewsPolicy",
        "MarketProfileOverridePatch",
        "MarketProfileReference",
        "PresetReferenceUsageMode",
        "PresetRegistryScope",
        "RESOLVED_PRESET_WORKFLOW_CONFIGURATION_SCHEMA_VERSION",
        "ResolvedKnownNewsComponent",
        "ResolvedMarketProfileComponent",
        "ResolvedPresetSelection",
        "ResolvedPresetWorkflowConfiguration",
        "ResolvedSessionComponent",
        "ResolvedWorkflowComponent",
        "ResolvedWorkflowComponentSource",
        "ResolvedWorkflowDiagnostic",
        "ResolvedWorkflowDiagnosticSeverity",
        "SessionConfigurationOverridePatch",
        "SessionExplicitConfiguration",
        "SessionPresetReference",
        "deserialize_known_news_preset_reference",
        "deserialize_market_data_calibration_request",
        "deserialize_market_profile_reference",
        "deserialize_session_preset_reference",
        "serialize_known_news_preset_reference",
        "serialize_market_data_calibration_request",
        "serialize_market_profile_reference",
        "serialize_session_preset_reference",
        "StagedCalibrationExecutionInput",
        "StagedCalibrationRequest",
        "StagedCalibrationResponse",
        "validate_known_news_preset_reference",
        "validate_market_data_calibration_request",
        "validate_market_profile_reference",
        "validate_session_preset_reference",
        "deserialize_staged_calibration_execution_input",
        "deserialize_staged_calibration_request",
        "deserialize_staged_calibration_response",
        "resolve_preset_workflow_configuration",
        "run_staged_calibration",
        "serialize_staged_calibration_execution_input",
        "serialize_staged_calibration_request",
        "serialize_staged_calibration_response",
        "validate_staged_calibration_execution_input",
        "validate_staged_calibration_request",
        "validate_staged_calibration_response",
    }

    assert expected_exports <= set(calibration_module.__all__)
    assert expected_exports <= set(dir(calibration_module))


def test_readme_links_public_api_surface_policy() -> None:
    readme_text = README_PATH.read_text(encoding="utf-8")

    assert "python_public_api_surface_map_and_namespace_policy.md" in readme_text
