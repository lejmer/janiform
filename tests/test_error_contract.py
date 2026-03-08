from argparse import Namespace
from datetime import UTC, datetime, timedelta
from pathlib import Path

import pytest

import janiform.calibration as calibration_module
import janiform.cli as cli_module
import janiform.pricing as pricing_module
import janiform.simulation as simulation_module
from janiform.calendar import CalendarMissingExchangeError, CalendarRegistry
from janiform.errors import JaniformExecutionError, JaniformIntegrationError
from janiform.hourly_factors import (
    HourlyFactorContract,
    HourlyNewsIntensityProviderExplicitSource,
    HourlySeasonalityProviderExplicitSource,
)
from janiform.pricing import (
    GammaSeverityMarkConfiguration,
    NigSharedPlusShiftConfiguration,
    VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType,
    VanillaOptionPricingExecutionInput,
    VanillaOptionPricingIntervalFactors,
    VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest,
    run_vanilla_option_pricing,
)
from janiform.simulation import (
    HawkesExcitationState,
    HawkesKernelLoadings,
    HawkesKernelParameters,
    SeededTickSimulationRequest,
    SeededTickSimulationValidationError,
    load_hourly_factor_contract_for_simulation,
    run_seeded_tick_event_simulation,
)
from tests.calibration_test_support import build_valid_execution_input_model

START_TIME_UTC = datetime(2026, 2, 2, tzinfo=UTC)


def test_validation_error_string_format_is_stable() -> None:
    with pytest.raises(SeededTickSimulationValidationError) as captured_error:
        SeededTickSimulationRequest(
            seed=17,
            start_time_utc=START_TIME_UTC,
            horizon_interval_count=2,
            kernel_parameters=_valid_kernel_parameters(),
            initial_excitation_state=_valid_excitation_state(),
            hourly_factor_summary=simulation_module.SeededTickSimulationHourlyFactorSummary(
                schema_version="seasonality.hourly_factor.contract.v1",
                exchange_code="XNYS",
                interval_count=2,
                interval_start_utc_nanos=(1, 2),
                interval_end_utc_nanos=(2, 3),
                s_h=(1.0, 1.1),
                nu_h=(0.2, 0.1),
            ),
            sample_every_nth_transition=0,
        )

    assert str(captured_error.value) == (
        '[simulation.validation.contract] '
        'category="validation" '
        'workflow="simulation" '
        'entrypoint="contract" '
        'field_path="sample_every_nth_transition" '
        'message="must be strictly positive"'
    )


def test_simulation_hourly_factor_loader_wraps_lower_level_validation_errors(
    calendar_registry: CalendarRegistry,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def raising_provider(**_: object) -> object:
        raise CalendarMissingExchangeError("exchange_code is unknown")

    monkeypatch.setattr(simulation_module, "provide_hourly_factor_contract", raising_provider)

    with pytest.raises(SeededTickSimulationValidationError) as captured_error:
        load_hourly_factor_contract_for_simulation(
            calendar_registry=calendar_registry,
            exchange_code="XBAD",
            range_start_utc=START_TIME_UTC,
            range_end_utc=START_TIME_UTC + timedelta(hours=2),
            seasonality_source=HourlySeasonalityProviderExplicitSource(hourly_factors=[1.0, 1.0]),
            news_intensity_source=HourlyNewsIntensityProviderExplicitSource(hourly_factors=[0.1, 0.1]),
        )

    assert captured_error.value.failure.error_id == "simulation.validation.contract"
    assert captured_error.value.failure.entrypoint == "load_hourly_factor_contract_for_simulation"
    assert captured_error.value.failure.field_path == "hourly_factor_contract"
    assert captured_error.value.failure.context == {"exchange_code": "XBAD"}


def test_simulation_run_maps_missing_native_module_to_integration_error(
    calendar_registry: CalendarRegistry,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def missing_native_module() -> object:
        raise ModuleNotFoundError("janiform._native")

    monkeypatch.setattr(simulation_module, "native_module", missing_native_module)

    with pytest.raises(JaniformIntegrationError) as captured_error:
        run_seeded_tick_event_simulation(_valid_simulation_request(calendar_registry))

    assert captured_error.value.error_id == "simulation.integration.native_module_unavailable"
    assert captured_error.value.workflow == "simulation"
    assert captured_error.value.entrypoint == "run_seeded_tick_event_simulation"
    assert captured_error.value.context == {"module_name": "janiform._native"}


def test_simulation_run_maps_native_validation_failure_to_execution_error(
    calendar_registry: CalendarRegistry,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class NativeValidationError(Exception):
        pass

    class FakeNative:
        SeasonalityInputValidationError = NativeValidationError

        @staticmethod
        def run_seeded_tick_event_simulation(*args: object, **kwargs: object) -> str:
            raise NativeValidationError("rejected by native layer")

    monkeypatch.setattr(simulation_module, "native_module", lambda: FakeNative())

    with pytest.raises(JaniformExecutionError) as captured_error:
        run_seeded_tick_event_simulation(_valid_simulation_request(calendar_registry))

    assert captured_error.value.error_id == "simulation.execution.run_failed"
    assert captured_error.value.workflow == "simulation"
    assert captured_error.value.entrypoint == "run_seeded_tick_event_simulation"
    assert captured_error.value.context == {"source_category": "native_validation"}


def test_simulation_run_maps_invalid_native_response_to_integration_error(
    calendar_registry: CalendarRegistry,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class NativeValidationError(Exception):
        pass

    class FakeNative:
        SeasonalityInputValidationError = NativeValidationError

        @staticmethod
        def run_seeded_tick_event_simulation(*args: object, **kwargs: object) -> str:
            return "{}"

    monkeypatch.setattr(simulation_module, "native_module", lambda: FakeNative())

    with pytest.raises(JaniformIntegrationError) as captured_error:
        run_seeded_tick_event_simulation(_valid_simulation_request(calendar_registry))

    assert captured_error.value.error_id == "simulation.integration.invalid_native_response"
    assert captured_error.value.workflow == "simulation"
    assert captured_error.value.entrypoint == "run_seeded_tick_event_simulation"
    assert captured_error.value.context == {"source_error_id": "simulation.validation.contract"}


def test_pricing_run_maps_missing_native_module_to_integration_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def missing_native_module() -> object:
        raise ModuleNotFoundError("janiform._native")

    monkeypatch.setattr(pricing_module, "native_module", missing_native_module)

    with pytest.raises(JaniformIntegrationError) as captured_error:
        run_vanilla_option_pricing(_valid_pricing_execution_input())

    assert captured_error.value.error_id == "pricing.integration.native_module_unavailable"
    assert captured_error.value.workflow == "pricing"
    assert captured_error.value.entrypoint == "run_vanilla_option_pricing"
    assert captured_error.value.context == {"module_name": "janiform._native"}


def test_pricing_run_maps_native_execution_failure_to_execution_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class NativeExecutionError(Exception):
        pass

    class FakeNative:
        PricingExecutionError = NativeExecutionError

        @staticmethod
        def run_vanilla_option_pricing(*args: object, **kwargs: object) -> str:
            raise NativeExecutionError("native pricer failed")

    monkeypatch.setattr(pricing_module, "native_module", lambda: FakeNative())

    with pytest.raises(JaniformExecutionError) as captured_error:
        run_vanilla_option_pricing(_valid_pricing_execution_input())

    assert captured_error.value.error_id == "pricing.execution.run_failed"
    assert captured_error.value.workflow == "pricing"
    assert captured_error.value.entrypoint == "run_vanilla_option_pricing"
    assert captured_error.value.context == {"source_exception_type": "NativeExecutionError"}


def test_pricing_run_maps_invalid_native_response_to_integration_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class NativeExecutionError(Exception):
        pass

    class FakeNative:
        PricingExecutionError = NativeExecutionError

        @staticmethod
        def run_vanilla_option_pricing(*args: object, **kwargs: object) -> str:
            return "{}"

    monkeypatch.setattr(pricing_module, "native_module", lambda: FakeNative())

    with pytest.raises(JaniformIntegrationError) as captured_error:
        run_vanilla_option_pricing(_valid_pricing_execution_input())

    assert captured_error.value.error_id == "pricing.integration.invalid_native_response"
    assert captured_error.value.workflow == "pricing"
    assert captured_error.value.entrypoint == "run_vanilla_option_pricing"
    assert captured_error.value.context == {"source_error_id": "pricing.validation.contract"}


def test_calibration_run_maps_missing_native_module_to_integration_error(
    calendar_registry: CalendarRegistry,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def missing_native_module() -> object:
        raise ModuleNotFoundError("janiform._native")

    monkeypatch.setattr(calibration_module, "native_module", missing_native_module)

    with pytest.raises(JaniformIntegrationError) as captured_error:
        calibration_module.run_staged_calibration(
            build_valid_execution_input_model(),
            calendar_registry=calendar_registry,
        )

    assert captured_error.value.error_id == "calibration.integration.native_module_unavailable"
    assert captured_error.value.workflow == "calibration"
    assert captured_error.value.entrypoint == "run_staged_calibration"
    assert captured_error.value.context == {"module_name": "janiform._native"}


def test_calibration_run_maps_native_execution_failure_to_execution_error(
    calendar_registry: CalendarRegistry,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class NativeExecutionError(Exception):
        pass

    class FakeNative:
        CalibrationExecutionError = NativeExecutionError

        @staticmethod
        def run_staged_calibration(*args: object, **kwargs: object) -> str:
            raise NativeExecutionError("native calibration failed")

    monkeypatch.setattr(calibration_module, "native_module", lambda: FakeNative())

    with pytest.raises(JaniformExecutionError) as captured_error:
        calibration_module.run_staged_calibration(
            build_valid_execution_input_model(),
            calendar_registry=calendar_registry,
        )

    assert captured_error.value.error_id == "calibration.execution.run_failed"
    assert captured_error.value.workflow == "calibration"
    assert captured_error.value.entrypoint == "run_staged_calibration"
    assert captured_error.value.context == {"source_exception_type": "NativeExecutionError"}


def test_calibration_run_maps_invalid_native_response_to_integration_error(
    calendar_registry: CalendarRegistry,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class NativeExecutionError(Exception):
        pass

    class FakeNative:
        CalibrationExecutionError = NativeExecutionError

        @staticmethod
        def run_staged_calibration(*args: object, **kwargs: object) -> str:
            return "{}"

    monkeypatch.setattr(calibration_module, "native_module", lambda: FakeNative())

    with pytest.raises(JaniformIntegrationError) as captured_error:
        calibration_module.run_staged_calibration(
            build_valid_execution_input_model(),
            calendar_registry=calendar_registry,
        )

    assert captured_error.value.error_id == "calibration.integration.invalid_native_response"
    assert captured_error.value.workflow == "calibration"
    assert captured_error.value.entrypoint == "run_staged_calibration"
    assert captured_error.value.context == {"source_error_id": "calibration.validation.contract"}


def test_cli_error_output_uses_shared_error_contract(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    def broken_load_json_document(path: object, context: object) -> dict[str, object]:
        raise ValueError("gap behavior fixture must include list field `cases`")

    monkeypatch.setattr(cli_module, "_load_json_document", broken_load_json_document)

    exit_code = cli_module._run_open_closed_gap_leakage_regression_command(
        Namespace(
            calendar_fixture_path=Path("calendar.json"),
            gap_behavior_fixture_path=Path("gap.json"),
            component_absolute_tolerance=1e-12,
        )
    )

    captured = capsys.readouterr()

    assert exit_code == 1
    assert '[cli.validation.open_closed_gap_leakage.invalid_input]' in captured.err
    assert 'workflow="cli"' in captured.err
    assert 'entrypoint="dev.regression.open_closed_gap_leakage"' in captured.err
    assert 'field_path="command_input"' in captured.err


def _valid_hourly_factor_contract(calendar_registry: CalendarRegistry) -> HourlyFactorContract:
    return load_hourly_factor_contract_for_simulation(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=START_TIME_UTC,
        range_end_utc=START_TIME_UTC + timedelta(hours=2),
        seasonality_source=HourlySeasonalityProviderExplicitSource(hourly_factors=[1.0, 1.0]),
        news_intensity_source=HourlyNewsIntensityProviderExplicitSource(hourly_factors=[0.1, 0.1]),
    )


def _valid_simulation_request(calendar_registry: CalendarRegistry) -> SeededTickSimulationRequest:
    return SeededTickSimulationRequest.from_runtime_inputs(
        seed=17,
        start_time_utc=START_TIME_UTC,
        horizon_interval_count=2,
        kernel_parameters=_valid_kernel_parameters(),
        initial_excitation_state=_valid_excitation_state(),
        hourly_factor_contract=_valid_hourly_factor_contract(calendar_registry),
    )


def _valid_kernel_parameters() -> HawkesKernelParameters:
    return HawkesKernelParameters(
        lambda_bar=0.8,
        beta=1.6,
        alpha=HawkesKernelLoadings(
            plus_plus=0.2,
            plus_minus=0.1,
            minus_plus=0.1,
            minus_minus=0.25,
        ),
        rho=HawkesKernelLoadings(
            plus_plus=0.08,
            plus_minus=0.04,
            minus_plus=0.05,
            minus_minus=0.1,
        ),
    )


def _valid_excitation_state() -> HawkesExcitationState:
    return HawkesExcitationState(
        y_plus_plus=0.0,
        y_plus_minus=0.0,
        y_minus_plus=0.0,
        y_minus_minus=0.0,
    )


def _valid_pricing_execution_input() -> VanillaOptionPricingExecutionInput:
    return VanillaOptionPricingExecutionInput(
        request=VanillaOptionPricingRequest(
            market_inputs=VanillaOptionPricingMarketInputs(
                spot=101.0,
                strike=100.0,
                maturity_interval=0.5,
                risk_free_rate=0.02,
                dividend_yield=0.01,
            ),
            payoff=VanillaOptionPayoffConfiguration(payoff_type=VanillaOptionPayoffType.CALL),
            boundary_state=_valid_excitation_state(),
            model_parameters=VanillaOptionPricingModelParameters(
                hawkes_kernel_parameters=_valid_kernel_parameters(),
                gamma_severity_mark_configuration=GammaSeverityMarkConfiguration(
                    shape_k=1.8,
                    scale_theta=0.7,
                ),
                nig_shared_plus_shift_configuration=NigSharedPlusShiftConfiguration(
                    alpha=2.3,
                    beta_0=-0.8,
                    delta=0.7,
                    mu=0.1,
                    delta_beta=-0.2,
                ),
            ),
        ),
        interval_factors=VanillaOptionPricingIntervalFactors(
            s_h=1.1,
            nu_h_known=0.03,
            nu_h_surprise=0.05,
        ),
        optional_output_selection=VanillaOptionPricingOptionalOutputSelection(
            include_transform_diagnostics=True,
            include_integration_quality_diagnostics=True,
            include_calibration_hooks=True,
        ),
    )
