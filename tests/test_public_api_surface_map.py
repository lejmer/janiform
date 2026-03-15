import ast
from importlib import import_module
from pathlib import Path

import pytest

import janiform
import janiform.cli as cli_module
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


def test_cli_public_module_is_a_package_with_curated_root_exports() -> None:
    assert hasattr(cli_module, "__path__")
    assert "main" in cli_module.__all__
    assert "_build_parser" not in cli_module.__dict__
    assert "_parse_cli_arguments" not in cli_module.__dict__


def test_cli_public_command_submodules_are_supported_domain_imports() -> None:
    cli_commands_module = import_module("janiform.cli.commands")
    cli_calibration_commands_module = import_module("janiform.cli.commands.calibration")
    cli_development_commands_module = import_module("janiform.cli.commands.development")
    cli_model_workflow_commands_module = import_module("janiform.cli.commands.model_workflows")
    cli_simulation_commands_module = import_module("janiform.cli.commands.simulation")

    assert not hasattr(cli_commands_module, "_run_fit_command")
    assert cli_calibration_commands_module.__all__ == []
    assert cli_development_commands_module.__all__ == []
    assert cli_model_workflow_commands_module.__all__ == []
    assert "run_coarse_simulation" in cli_simulation_commands_module.__all__
    assert "run_seeded_tick_event_simulation" in cli_simulation_commands_module.__all__


def test_cli_click_command_tree_helpers_remain_internal() -> None:
    cli_command_tree_module = import_module("janiform.cli.command_tree")

    assert not hasattr(cli_command_tree_module, "__all__")


def test_python_package_all_exports_exclude_private_names() -> None:
    python_root = REPOSITORY_ROOT / "python" / "janiform"

    for module_path in sorted(python_root.rglob("*.py")):
        module_tree = ast.parse(module_path.read_text(encoding="utf-8"))
        module_exports: list[str] | None = None

        for statement in module_tree.body:
            if isinstance(statement, ast.Assign):
                targets = statement.targets
            elif isinstance(statement, ast.AnnAssign):
                targets = [statement.target]
            else:
                continue

            if not any(isinstance(target, ast.Name) and target.id == "__all__" for target in targets):
                continue

            try:
                resolved_exports = ast.literal_eval(statement.value)
            except Exception as error:  # pragma: no cover - test invariant fallback
                raise AssertionError(f"Could not statically evaluate {module_path} __all__") from error

            if not isinstance(resolved_exports, list):
                raise AssertionError(f"{module_path} __all__ must be a list literal")

            module_exports = resolved_exports
            break

        if module_exports is None:
            continue

        private_exports = [
            export_name
            for export_name in module_exports
            if export_name.startswith("_") and not export_name.startswith("__")
        ]
        assert private_exports == [], f"{module_path} exports private names in __all__: {private_exports}"


def test_simulation_public_symbols_live_in_simulation_module() -> None:
    expected_exports = {
        "CalendarRegistry",
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
        "HourlyFactorContract",
        "HourlyNewsIntensityProviderSource",
        "HourlySeasonalityProviderSource",
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


def test_simulation_seeded_public_submodule_is_supported_domain_import() -> None:
    simulation_seeded_module = import_module("janiform.simulation.seeded")

    expected_exports = {
        "CalendarRegistry",
        "HawkesExcitationState",
        "HawkesKernelLoadings",
        "HawkesKernelParameters",
        "HourlyFactorContract",
        "HourlyNewsIntensityProviderSource",
        "HourlySeasonalityProviderSource",
        "SeededTickEventRecord",
        "SeededTickEventSide",
        "SeededTickEventStateSnapshot",
        "SeededTickEventStateSnapshotCapture",
        "SeededTickEventTapeMetadata",
        "SeededTickSimulationDiagnostics",
        "SeededTickSimulationHourlyFactorSummary",
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

    assert expected_exports <= set(simulation_seeded_module.__all__)
    assert expected_exports <= set(dir(simulation_seeded_module))


def test_simulation_coarse_public_submodule_is_supported_domain_import() -> None:
    simulation_coarse_module = import_module("janiform.simulation.coarse")

    expected_exports = {
        "COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION",
        "CoarseSimulationModelInputs",
        "CoarseSimulationPath",
        "CoarseSimulationRunRequest",
        "NewsProcessConfiguration",
        "deserialize_coarse_simulation_path",
        "deserialize_coarse_simulation_run_request",
        "run_coarse_simulation",
        "serialize_coarse_simulation_path",
        "serialize_coarse_simulation_run_request",
        "validate_coarse_simulation_path",
        "validate_coarse_simulation_run_request",
    }

    assert expected_exports <= set(simulation_coarse_module.__all__)
    assert expected_exports <= set(dir(simulation_coarse_module))


def test_simulation_shared_public_submodule_is_supported_domain_import() -> None:
    simulation_shared_module = import_module("janiform.simulation.shared")

    expected_exports = {
        "HawkesExcitationState",
        "HawkesKernelLoadings",
        "HawkesKernelParameters",
        "SeededTickEventSide",
        "SeededTickEventStateSnapshotCapture",
        "SeededTickSimulationValidationError",
        "SeededTickSimulationValidationFailure",
    }

    assert set(simulation_shared_module.__all__) == expected_exports
    assert expected_exports <= set(dir(simulation_shared_module))


def test_simulation_root_and_seeded_submodule_share_supported_symbols() -> None:
    simulation_seeded_module = import_module("janiform.simulation.seeded")

    assert (
        simulation_module.SeededTickSimulationRequest
        is simulation_seeded_module.SeededTickSimulationRequest
    )
    assert (
        simulation_module.SeededTickSimulationResponse
        is simulation_seeded_module.SeededTickSimulationResponse
    )
    assert (
        simulation_module.run_seeded_tick_event_simulation
        is simulation_seeded_module.run_seeded_tick_event_simulation
    )
    assert "GammaSeverityMarkConfiguration" not in simulation_module.__all__

    with pytest.raises(AttributeError):
        simulation_module.GammaSeverityMarkConfiguration


def test_simulation_root_and_coarse_submodule_share_supported_symbols() -> None:
    simulation_coarse_module = import_module("janiform.simulation.coarse")

    assert simulation_module.CoarseSimulationRunRequest is simulation_coarse_module.CoarseSimulationRunRequest
    assert simulation_module.CoarseSimulationPath is simulation_coarse_module.CoarseSimulationPath
    assert simulation_module.run_coarse_simulation is simulation_coarse_module.run_coarse_simulation


def test_simulation_root_and_shared_submodule_share_supported_symbols() -> None:
    simulation_shared_module = import_module("janiform.simulation.shared")

    assert simulation_module.HawkesKernelParameters is simulation_shared_module.HawkesKernelParameters
    assert simulation_module.HawkesExcitationState is simulation_shared_module.HawkesExcitationState
    assert (
        simulation_module.SeededTickSimulationValidationError
        is simulation_shared_module.SeededTickSimulationValidationError
    )


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
    assert import_module("janiform.simulation.coarse").__name__ == "janiform.simulation.coarse"
    assert import_module("janiform.simulation.seeded").__name__ == "janiform.simulation.seeded"
    assert import_module("janiform.simulation.shared").__name__ == "janiform.simulation.shared"

    with pytest.raises(ModuleNotFoundError):
        import_module("janiform.simulation._coarse")

    with pytest.raises(ModuleNotFoundError):
        import_module("janiform.simulation._seeded")

    with pytest.raises(ModuleNotFoundError):
        import_module("janiform.simulation._shared")

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
        "CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION",
        "CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION",
        "CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_SCHEMA_VERSION",
        "CALIBRATION_RUN_SPEC_SCHEMA_VERSION",
        "CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION",
        "CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION",
        "CalibrationDuplicateRecordPolicy",
        "CalibrationExecutionMode",
        "CalibrationInputFileFormat",
        "CalibrationKnownNewsColumnMapping",
        "CalibrationMarketDataNormalizationPolicy",
        "CalibrationMarketDataOutputPreferences",
        "CalibrationMarketDataWorkflowScope",
        "CalibrationOptionQuoteInputPolicy",
        "CalibrationOptionSurfaceColumnMapping",
        "CalibrationTickDataColumnMapping",
        "CalibrationTickPriceInputPolicy",
        "CalibrationValidationError",
        "CalibrationValidationFailure",
        "DEFAULT_HIGH_LEVEL_KNOWN_NEWS_PRESET_NAME",
        "KnownNewsExplicitConfiguration",
        "KnownNewsPresetReference",
        "JaniformEstimator",
        "JaniformFitReport",
        "JaniformModel",
        "JaniformSimulationResult",
        "MarketDataCalibrationRequest",
        "MarketDataSessionRecurringClosedWindow",
        "MarketDataSessionRegularWindow",
        "MarketDataSessionResolution",
        "MarketProfileReference",
        "PresetReferenceUsageMode",
        "PresetCategory",
        "PresetRegistry",
        "PresetRegistryScope",
        "SessionExplicitConfiguration",
        "SessionPresetReference",
        "StagedCalibrationExecutionInput",
        "StagedCalibrationRequest",
        "StagedCalibrationResponse",
        "PreparedMarketDataCalibration",
        "prepare_market_data_calibration",
        "resolve_preset_workflow_configuration",
        "run_market_data_calibration",
        "run_staged_calibration",
        "validate_market_data_calibration_request",
        "validate_staged_calibration_execution_input",
    }

    assert set(calibration_module.__all__) == expected_exports
    assert expected_exports <= set(dir(calibration_module))

    root_exports = set(calibration_module.__all__)

    assert "CalibrationCheckpointMetadata" not in root_exports
    assert "CalibrationInputFileReference" not in root_exports
    assert "serialize_preset_artifact" not in root_exports


def test_calibration_public_module_is_a_package() -> None:
    calibration_module = import_module("janiform.calibration")

    assert hasattr(calibration_module, "__path__")


def test_calibration_fit_public_submodule_is_supported_domain_import() -> None:
    calibration_fit_module = import_module("janiform.calibration.fit")

    assert set(calibration_fit_module.__all__) == {
        "JaniformEstimator",
        "JaniformFitReport",
        "JaniformModel",
        "JaniformSimulationResult",
    }


def test_calibration_fit_public_ownership_submodules_are_supported_domain_imports() -> None:
    calibration_fit_estimator_module = import_module("janiform.calibration.fit.estimator")
    calibration_fit_model_module = import_module("janiform.calibration.fit.model")
    calibration_fit_report_module = import_module("janiform.calibration.fit.report")

    assert set(calibration_fit_estimator_module.__all__) == {"JaniformEstimator"}
    assert set(calibration_fit_model_module.__all__) == {
        "JaniformModel",
        "JaniformSimulationResult",
    }
    assert set(calibration_fit_report_module.__all__) == {"JaniformFitReport"}


def test_calibration_market_data_public_submodule_is_supported_domain_import() -> None:
    calibration_market_data_module = import_module("janiform.calibration.market_data")

    expected_exports = {
        "CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION",
        "CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION",
        "CalibrationDuplicateRecordPolicy",
        "CalibrationInputFileFormat",
        "CalibrationInputFileReference",
        "CalibrationKnownNewsColumnMapping",
        "CalibrationKnownNewsInput",
        "CalibrationMarketDataColumnMapping",
        "CalibrationMarketDataNormalizationPolicy",
        "CalibrationMarketDataOutputPreferences",
        "CalibrationMarketDataWorkflowScope",
        "CalibrationOptionQuoteInputPolicy",
        "CalibrationOptionSurfaceColumnMapping",
        "CalibrationSessionConfigurationInput",
        "CalibrationTickDataColumnMapping",
        "CalibrationTickPriceInputPolicy",
        "MarketDataCalibrationPreparationDiagnostics",
        "MarketDataCalibrationPreparationProvenance",
        "MarketDataCalibrationRequest",
        "MarketDataCalibrationSourceSummary",
        "PreparedMarketDataCalibration",
        "deserialize_market_data_calibration_request",
        "prepare_market_data_calibration",
        "run_market_data_calibration",
        "serialize_market_data_calibration_request",
        "validate_market_data_calibration_request",
    }

    assert set(calibration_market_data_module.__all__) == expected_exports
    assert expected_exports <= set(dir(calibration_market_data_module))


def test_calibration_market_data_public_ownership_submodules_are_supported_domain_imports() -> None:
    calibration_market_data_contracts_module = import_module("janiform.calibration.market_data.contracts")
    calibration_market_data_request_module = import_module("janiform.calibration.market_data.request")
    calibration_market_data_ingestion_module = import_module("janiform.calibration.market_data.ingestion")
    calibration_market_data_translation_module = import_module("janiform.calibration.market_data.translation")
    calibration_market_data_workflow_module = import_module("janiform.calibration.market_data.workflow")

    assert {
        "CalibrationInputFileReference",
        "CalibrationMarketDataWorkflowScope",
        "CalibrationSessionConfigurationInput",
    } <= set(calibration_market_data_contracts_module.__all__)
    assert set(calibration_market_data_request_module.__all__) == {
        "MarketDataCalibrationRequest",
        "deserialize_market_data_calibration_request",
        "serialize_market_data_calibration_request",
        "validate_market_data_calibration_request",
    }
    assert set(calibration_market_data_ingestion_module.__all__) == {
        "LoadedKnownNewsSource",
        "LoadedMarketDataCalibrationInputs",
        "LoadedMarketDataSourceSummary",
        "LoadedOptionSurfaceSource",
        "LoadedTickDataSource",
        "NormalizedKnownNewsRecord",
        "NormalizedOptionSurfaceRecord",
        "NormalizedTickDataRecord",
        "load_market_data_calibration_inputs",
    }
    assert set(calibration_market_data_translation_module.__all__) == {
        "MarketDataTranslationDiagnostics",
        "MarketDataTranslationProvenance",
        "MarketDataTranslationResult",
        "translate_market_data_request_to_staged_execution_input",
    }
    assert set(calibration_market_data_workflow_module.__all__) == {
        "MarketDataCalibrationPreparationDiagnostics",
        "MarketDataCalibrationPreparationProvenance",
        "MarketDataCalibrationSourceSummary",
        "PreparedMarketDataCalibration",
        "prepare_market_data_calibration",
        "run_market_data_calibration",
    }


def test_calibration_staged_public_submodule_is_supported_domain_import() -> None:
    calibration_staged_module = import_module("janiform.calibration.staged")

    expected_exports = {
        "CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY",
        "CALIBRATION_RUN_SPEC_SCHEMA_VERSION",
        "CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION",
        "CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION",
        "CalibrationExecutionMode",
        "CalibrationOptimizerSettings",
        "CalibrationRunDataInputs",
        "CalibrationStage",
        "CalibrationStageExecutionStatus",
        "CalibrationTradeSign",
        "StagedCalibrationExecutionInput",
        "StagedCalibrationRequest",
        "StagedCalibrationResponse",
        "run_staged_calibration",
        "validate_staged_calibration_execution_input",
        "validate_staged_calibration_request",
        "validate_staged_calibration_response",
    }

    assert expected_exports <= set(calibration_staged_module.__all__)
    assert expected_exports <= set(dir(calibration_staged_module))


def test_calibration_staged_public_ownership_submodules_are_supported_domain_imports() -> None:
    calibration_staged_contracts_module = import_module("janiform.calibration.staged.contracts")
    calibration_staged_inputs_module = import_module("janiform.calibration.staged.inputs")
    calibration_staged_request_module = import_module("janiform.calibration.staged.request")
    calibration_staged_results_module = import_module("janiform.calibration.staged.results")
    calibration_staged_runtime_module = import_module("janiform.calibration.staged.runtime")

    assert {
        "CalibrationExecutionMode",
        "CalibrationRunDataInputs",
        "CalibrationStage",
    } <= set(calibration_staged_contracts_module.__all__)
    assert set(calibration_staged_request_module.__all__) == {
        "StagedCalibrationRequest",
        "deserialize_staged_calibration_request",
        "serialize_staged_calibration_request",
        "validate_staged_calibration_request",
    }
    assert {
        "StagedCalibrationExecutionInput",
        "validate_staged_calibration_execution_input",
    } <= set(calibration_staged_inputs_module.__all__)
    assert {
        "CalibrationStageReport",
        "StagedCalibrationResponse",
        "validate_staged_calibration_response",
    } <= set(calibration_staged_results_module.__all__)
    assert {
        "run_staged_calibration",
    } <= set(calibration_staged_runtime_module.__all__)


def test_calibration_presets_public_submodule_is_supported_domain_import() -> None:
    calibration_presets_module = import_module("janiform.calibration.presets")

    expected_exports = {
        "BUNDLED_PRESET_INDEX_SCHEMA_VERSION",
        "DEFAULT_HIGH_LEVEL_KNOWN_NEWS_PRESET_NAME",
        "KnownNewsExplicitConfiguration",
        "KnownNewsPresetReference",
        "MarketProfileReference",
        "PresetCategory",
        "PresetReferenceUsageMode",
        "PresetRegistry",
        "PresetRegistryScope",
        "ResolvedPresetWorkflowConfiguration",
        "SessionExplicitConfiguration",
        "SessionPresetReference",
        "resolve_preset_workflow_configuration",
        "serialize_preset_artifact",
    }

    assert expected_exports <= set(calibration_presets_module.__all__)
    assert expected_exports <= set(dir(calibration_presets_module))


def test_calibration_presets_public_ownership_submodules_are_supported_domain_imports() -> None:
    calibration_presets_references_module = import_module("janiform.calibration.presets.references")
    calibration_presets_registry_module = import_module("janiform.calibration.presets.registry")
    calibration_presets_resolution_module = import_module("janiform.calibration.presets.resolution")

    assert {
        "MarketProfileReference",
        "PresetReferenceUsageMode",
        "SessionPresetReference",
    } <= set(calibration_presets_references_module.__all__)
    assert {
        "PresetCategory",
        "PresetRegistry",
        "serialize_preset_artifact",
    } <= set(calibration_presets_registry_module.__all__)
    assert {
        "ResolvedPresetWorkflowConfiguration",
        "ResolvedWorkflowComponent",
        "resolve_preset_workflow_configuration",
    } <= set(calibration_presets_resolution_module.__all__)


def test_calibration_sessions_public_submodule_is_supported_domain_import() -> None:
    calibration_sessions_module = import_module("janiform.calibration.sessions")

    expected_exports = {
        "CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_ENTRYPOINT",
        "CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_ERROR_ID",
        "CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_SCHEMA_VERSION",
        "MarketDataSessionRecurringClosedWindow",
        "MarketDataSessionRegularWindow",
        "MarketDataSessionResolution",
        "resolve_market_data_session_configuration",
    }

    assert set(calibration_sessions_module.__all__) == expected_exports
    assert expected_exports <= set(dir(calibration_sessions_module))


def test_calibration_sessions_public_ownership_submodules_are_supported_domain_imports() -> None:
    calibration_sessions_contracts_module = import_module("janiform.calibration.sessions.contracts")
    calibration_sessions_resolution_module = import_module("janiform.calibration.sessions.resolution")

    assert set(calibration_sessions_contracts_module.__all__) == {
        "CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_ENTRYPOINT",
        "CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_ERROR_ID",
        "CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_SCHEMA_VERSION",
        "MarketDataSessionRecurringClosedWindow",
        "MarketDataSessionRegularWindow",
        "MarketDataSessionResolution",
    }
    assert set(calibration_sessions_resolution_module.__all__) == {
        "ResolvedMarketDataSession",
        "resolve_market_data_session_configuration",
    }


def test_calibration_root_and_public_submodules_share_supported_symbols() -> None:
    calibration_module = import_module("janiform.calibration")
    calibration_fit_module = import_module("janiform.calibration.fit")
    calibration_market_data_module = import_module("janiform.calibration.market_data")
    calibration_presets_module = import_module("janiform.calibration.presets")
    calibration_sessions_module = import_module("janiform.calibration.sessions")
    calibration_staged_module = import_module("janiform.calibration.staged")

    assert calibration_module.JaniformEstimator is calibration_fit_module.JaniformEstimator
    assert (
        calibration_module.MarketDataCalibrationRequest
        is calibration_market_data_module.MarketDataCalibrationRequest
    )
    assert calibration_module.PresetRegistry is calibration_presets_module.PresetRegistry
    assert (
        calibration_module.MarketDataSessionResolution
        is calibration_sessions_module.MarketDataSessionResolution
    )
    assert (
        calibration_module.StagedCalibrationExecutionInput
        is calibration_staged_module.StagedCalibrationExecutionInput
    )


def test_calibration_root_and_public_ownership_submodules_share_supported_symbols() -> None:
    calibration_module = import_module("janiform.calibration")

    calibration_fit_estimator_module = import_module("janiform.calibration.fit.estimator")
    calibration_fit_model_module = import_module("janiform.calibration.fit.model")
    calibration_fit_report_module = import_module("janiform.calibration.fit.report")
    calibration_market_data_contracts_module = import_module("janiform.calibration.market_data.contracts")
    calibration_market_data_request_module = import_module("janiform.calibration.market_data.request")
    calibration_presets_references_module = import_module("janiform.calibration.presets.references")
    calibration_presets_registry_module = import_module("janiform.calibration.presets.registry")
    calibration_presets_resolution_module = import_module("janiform.calibration.presets.resolution")
    calibration_sessions_contracts_module = import_module("janiform.calibration.sessions.contracts")
    calibration_staged_contracts_module = import_module("janiform.calibration.staged.contracts")
    calibration_staged_inputs_module = import_module("janiform.calibration.staged.inputs")
    calibration_staged_request_module = import_module("janiform.calibration.staged.request")
    calibration_staged_results_module = import_module("janiform.calibration.staged.results")

    assert calibration_module.JaniformEstimator is calibration_fit_estimator_module.JaniformEstimator
    assert calibration_module.JaniformModel is calibration_fit_model_module.JaniformModel
    assert calibration_module.JaniformFitReport is calibration_fit_report_module.JaniformFitReport
    assert (
        calibration_module.CalibrationMarketDataWorkflowScope
        is calibration_market_data_contracts_module.CalibrationMarketDataWorkflowScope
    )
    assert (
        calibration_module.MarketDataCalibrationRequest
        is calibration_market_data_request_module.MarketDataCalibrationRequest
    )
    assert calibration_module.MarketProfileReference is calibration_presets_references_module.MarketProfileReference
    assert calibration_module.PresetRegistry is calibration_presets_registry_module.PresetRegistry
    assert (
        calibration_module.resolve_preset_workflow_configuration
        is calibration_presets_resolution_module.resolve_preset_workflow_configuration
    )
    assert (
        calibration_module.MarketDataSessionResolution
        is calibration_sessions_contracts_module.MarketDataSessionResolution
    )
    assert calibration_module.CalibrationExecutionMode is calibration_staged_contracts_module.CalibrationExecutionMode
    assert calibration_module.StagedCalibrationRequest is calibration_staged_request_module.StagedCalibrationRequest
    assert (
        calibration_module.StagedCalibrationExecutionInput
        is calibration_staged_inputs_module.StagedCalibrationExecutionInput
    )
    assert calibration_module.StagedCalibrationResponse is calibration_staged_results_module.StagedCalibrationResponse


def test_readme_links_public_api_surface_policy() -> None:
    readme_text = README_PATH.read_text(encoding="utf-8")

    assert "python_public_api_surface_map_and_namespace_policy.md" in readme_text
