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
        "HawkesExcitationState",
        "HawkesKernelLoadings",
        "HawkesKernelParameters",
        "SeededTickEventRecord",
        "SeededTickEventSide",
        "SeededTickEventStateSnapshot",
        "SeededTickEventStateSnapshotCapture",
        "SeededTickEventTapeMetadata",
        "SeededTickSimulationRequest",
        "SeededTickSimulationResponse",
        "SeededTickSimulationValidationError",
        "SeededTickSimulationValidationFailure",
        "deserialize_seeded_tick_simulation_response",
        "load_hourly_factor_contract_for_simulation",
        "run_seeded_tick_event_simulation",
        "serialize_seeded_tick_simulation_response",
        "validate_seeded_tick_simulation_response",
    }

    assert expected_exports <= set(simulation_module.__all__)
    assert expected_exports <= set(dir(simulation_module))


def test_pricing_public_symbols_live_in_pricing_module() -> None:
    expected_exports = {
        "GammaSeverityMarkConfiguration",
        "NigSharedPlusShiftConfiguration",
        "VanillaOptionPayoffConfiguration",
        "VanillaOptionPayoffType",
        "VanillaOptionPricingDiagnostics",
        "VanillaOptionPricingMarketInputs",
        "VanillaOptionPricingModelParameters",
        "VanillaOptionPricingRequest",
        "VanillaOptionPricingResponse",
        "VanillaOptionPricingValidationError",
        "VanillaOptionPricingValidationFailure",
        "load_hourly_factor_contract_for_pricing",
        "validate_vanilla_option_pricing_request",
        "validate_vanilla_option_pricing_response",
        "serialize_vanilla_option_pricing_request",
        "serialize_vanilla_option_pricing_response",
        "deserialize_vanilla_option_pricing_request",
        "deserialize_vanilla_option_pricing_response",
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
        "CALIBRATION_RUN_SPEC_SCHEMA_VERSION",
        "CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION",
        "CalibrationCheckpointMetadata",
        "CalibrationExecutionMode",
        "CalibrationFailureDetails",
        "CalibrationHandoffTransport",
        "CalibrationOptimizerSettings",
        "CalibrationResumeSettings",
        "CalibrationResumeSource",
        "CalibrationRunDataInputs",
        "CalibrationRunSeedSettings",
        "CalibrationStage",
        "CalibrationStageExecutionStatus",
        "CalibrationStageHandoff",
        "CalibrationStageReport",
        "CalibrationStageSeedOverride",
        "CalibrationStageSolverSettings",
        "CalibrationValidationError",
        "CalibrationValidationFailure",
        "DEFAULT_PYTHON_IMPORTABLE_API_EXECUTION_MODE",
        "StagedCalibrationRequest",
        "StagedCalibrationResponse",
        "deserialize_staged_calibration_request",
        "deserialize_staged_calibration_response",
        "serialize_staged_calibration_request",
        "serialize_staged_calibration_response",
        "validate_staged_calibration_request",
        "validate_staged_calibration_response",
    }

    assert expected_exports <= set(calibration_module.__all__)
    assert expected_exports <= set(dir(calibration_module))


def test_readme_links_public_api_surface_policy() -> None:
    readme_text = README_PATH.read_text(encoding="utf-8")

    assert "python_public_api_surface_map_and_namespace_policy.md" in readme_text
