from importlib import import_module
from pathlib import Path

import pytest

import janiform
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
        "SeededTickEventTape",
        "load_hourly_factor_contract_for_simulation",
        "run_seeded_tick_event_simulation",
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


def test_calibration_module_is_not_public_in_current_milestone() -> None:
    with pytest.raises(ModuleNotFoundError):
        import_module("janiform.calibration")


def test_readme_links_public_api_surface_policy() -> None:
    readme_text = README_PATH.read_text(encoding="utf-8")

    assert "python_public_api_surface_map_and_namespace_policy.md" in readme_text
