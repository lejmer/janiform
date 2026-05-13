from datetime import UTC, datetime, timedelta
from typing import Any, NoReturn

import pytest

import janiform.calendar as calendar_module
import janiform.calibration.staged.runtime as calibration_staged_runtime_module
import janiform.pricing.workflow as pricing_workflow_module
import janiform.simulation as simulation_module
import janiform.simulation.coarse.runtime as simulation_coarse_runtime_module
import janiform.simulation.seeded.runtime as simulation_seeded_runtime_module
from janiform._timestamps import NANOS_PER_HOUR
from janiform.calendar import CalendarRegistry
from janiform.calibration.staged import run_staged_calibration
from janiform.mean_reversion import (
    MeanReversionAnchorModel,
    MeanReversionBoundaryState,
    MeanReversionFittedParameters,
)
from janiform.errors import JaniformExecutionError, JaniformIntegrationError
from janiform.hourly_factors import (
    HourlyFactorContract,
    HourlyNewsIntensityProviderExplicitSource,
    HourlySeasonalityProviderExplicitSource,
    provide_hourly_factor_contract,
)
from janiform.pricing import (
    AnchoredVanillaOptionPricingBoundaryInput,
    GammaSeverityMarkConfiguration,
    NigSharedPlusShiftConfiguration,
    VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType,
    VanillaOptionPricingExecutionInput,
    VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest,
    run_vanilla_option_pricing,
    run_vanilla_option_surface_pricing,
)
from janiform.sufficient_statistics import (
    SufficientStatisticsEquationSpecification,
    SufficientStatisticsModelInputs,
)
from janiform.simulation import (
    SeededTickSimulationValidationError,
    run_seeded_tick_event_simulation,
)
from janiform.simulation.coarse import (
    CoarseSimulationRunRequest,
    CoupledGapMarkConfiguration,
    DoubleExponentialBaseShockConfiguration,
    KnownNewsArrivalConfiguration,
    NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration,
    NewsProcessConfiguration,
    SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
    run_coarse_simulation,
)
from janiform.simulation.seeded import SeededTickSimulationRequest
from janiform.simulation.shared import (
    HawkesExcitationState,
    HawkesKernelLoadings,
    HawkesKernelParameters,
)
from janiform.simulation.seeded.contracts import (
    build_endogenous_seeded_mean_reversion_runtime_inputs,
)
from tests.support.calibration import build_valid_execution_input_model
from tests.support.pricing import (
    build_anchored_runtime_execution_input_model,
    build_anchored_surface_execution_input_model,
)
from tests.support.simulation import build_endogenous_mean_reversion_runtime_contracts


START_TIME_UTC = datetime(2026, 2, 2, tzinfo=UTC)


def test_validation_error_string_format_is_stable() -> None:
    (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = build_endogenous_mean_reversion_runtime_contracts(interval_count=2)

    with pytest.raises(SeededTickSimulationValidationError) as captured_error:
        SeededTickSimulationRequest(
            seed=17,
            start_time_utc=START_TIME_UTC,
            horizon_interval_count=2,
            kernel_parameters=_valid_kernel_parameters(),
            nig_shared_plus_shift_configuration=NigSharedPlusShiftConfiguration(
                alpha=2.3,
                beta_0=-0.8,
                delta=0.7,
                mu=0.1,
                delta_beta=-0.2,
            ),
            news_process_configuration=NewsProcessConfiguration(
                schema_version="news.process.configuration.v1",
                arrivals=NewsArrivalConfiguration(
                    known=KnownNewsArrivalConfiguration(
                        scheduled_event_timestamps_utc_nanos=(),
                    ),
                    surprise=SurpriseNewsArrivalConfiguration(
                        baseline_intensity=0.0,
                    ),
                ),
                shared_mark_law=SharedNewsMarkLawConfiguration(
                    gamma_severity_marks=GammaSeverityMarkConfiguration(
                        shape_k=1.5,
                        scale_theta=0.9,
                    ),
                    double_exponential_base_shock=DoubleExponentialBaseShockConfiguration(
                        probability_positive=0.4,
                        eta_plus=1.3,
                        eta_minus=1.1,
                    ),
                    coupled_gap_marks=CoupledGapMarkConfiguration(sigma_g=0.75),
                ),
                known_component_mark_scales=NewsComponentMarkScaleConfiguration(
                    activity_coupling_scale=0.6,
                    return_jump_scale=1.2,
                ),
                surprise_component_mark_scales=NewsComponentMarkScaleConfiguration(
                    activity_coupling_scale=0.6,
                    return_jump_scale=1.2,
                ),
            ),
            initial_excitation_state=_valid_excitation_state(),
            mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
            mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
            initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
            hourly_factor_summary=simulation_module.SeededTickSimulationHourlyFactorSummary(
                schema_version="seasonality.hourly_factor.contract.v1",
                exchange_code="XNYS",
                interval_count=2,
                interval_duration_nanos=NANOS_PER_HOUR,
                interval_start_utc_nanos=(1, 2),
                interval_end_utc_nanos=(2, 3),
                s_h=(1.0, 1.1),
                nu_h=(0.2, 0.1),
            ),
            return_mark_hour_of_week_factors=(1.0,) * 168,
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


def test_simulation_run_maps_missing_native_module_to_integration_error(
    calendar_registry: CalendarRegistry,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def missing_native_module() -> NoReturn:
        raise ModuleNotFoundError("janiform._native")

    monkeypatch.setattr(simulation_seeded_runtime_module, "native_module", missing_native_module)

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
        def run_seeded_tick_event_simulation(*args, **kwargs) -> str:
            raise NativeValidationError("rejected by native layer")

    monkeypatch.setattr(simulation_seeded_runtime_module, "native_module", lambda: FakeNative())

    with pytest.raises(JaniformExecutionError) as captured_error:
        run_seeded_tick_event_simulation(_valid_simulation_request(calendar_registry))

    assert captured_error.value.error_id == "simulation.execution.run_failed"
    assert captured_error.value.workflow == "simulation"
    assert captured_error.value.entrypoint == "run_seeded_tick_event_simulation"
    assert captured_error.value.context == {"source_category": "native_validation"}


def test_coarse_simulation_run_includes_native_failure_message_in_context(
    calendar_registry: CalendarRegistry,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class FakeSimulationExecutionError(Exception):
        pass

    class FakeNative:
        SimulationExecutionError = FakeSimulationExecutionError

        @staticmethod
        def run_coarse_simulation(*args, **kwargs) -> NoReturn:
            raise FakeSimulationExecutionError(
                "interval 0 solver did not produce completed output"
            )

    monkeypatch.setattr(simulation_coarse_runtime_module, "native_module", lambda: FakeNative())

    with pytest.raises(JaniformExecutionError) as captured_error:
        run_coarse_simulation(_valid_coarse_simulation_request(calendar_registry))

    assert captured_error.value.error_id == "simulation.execution.run_failed"
    assert captured_error.value.workflow == "simulation"
    assert captured_error.value.entrypoint == "run_coarse_simulation"
    assert captured_error.value.context == {
        "source_exception_type": "FakeSimulationExecutionError",
        "source_exception_message": "interval 0 solver did not produce completed output",
    }


def test_simulation_run_maps_native_chunk_projection_failure_to_integration_error(
    calendar_registry: CalendarRegistry,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class FakeNativeChunkResult:
        schema_version = "simulation.seeded_tick_event.path.v1"
        transition_count = 1
        event_count = 0
        estimated_native_retained_bytes = 0

        @staticmethod
        def fine_path_metadata_json() -> str:
            return (
                '{"schema_version":"simulation.seeded_tick_event.fine_path_metadata.v1",'
                '"path_record_schema_version":"simulation.seeded_tick_event.path_record.v1",'
                '"event_projection_record_schema_version":"simulation.seeded_tick_event.event_record.v1",'
                '"transition_ordering_policy":"canonical",'
                '"timestamp_representation_policy":"utc_nanos",'
                '"side_encoding_policy":"snake_case",'
                '"state_snapshot_capture":"disabled"}'
            )

        @staticmethod
        def event_tape_metadata_json() -> str:
            return (
                '{"schema_version":"simulation.seeded_tick_event.event_tape_metadata.v1",'
                '"event_record_schema_version":"simulation.seeded_tick_event.event_record.v1",'
                '"state_snapshot_schema_version":"simulation.seeded_tick_event.state_snapshot.v1",'
                '"event_ordering_policy":"canonical",'
                '"timestamp_representation_policy":"utc_nanos",'
                '"side_encoding_policy":"snake_case",'
                '"state_snapshot_capture":"disabled"}'
            )

        @staticmethod
        def get_fine_path_chunk_columns(start_index: int, max_records: int) -> NoReturn:
            del start_index
            del max_records
            raise RuntimeError("native chunk projection failed")

        @staticmethod
        def get_event_tape_chunk_columns(start_index: int, max_records: int) -> NoReturn:
            del start_index
            del max_records
            raise RuntimeError("native event-tape chunk projection failed")

    class FakeNative:
        @staticmethod
        def run_seeded_tick_event_simulation(*args, **kwargs) -> FakeNativeChunkResult:
            del args
            del kwargs
            return FakeNativeChunkResult()

    monkeypatch.setattr(simulation_seeded_runtime_module, "native_module", lambda: FakeNative())

    response = run_seeded_tick_event_simulation(_valid_simulation_request(calendar_registry))

    with pytest.raises(JaniformIntegrationError) as captured_error:
        response.to_payload()

    assert captured_error.value.error_id == "simulation.integration.invalid_native_response"
    assert captured_error.value.workflow == "simulation"
    assert captured_error.value.entrypoint == "run_seeded_tick_event_simulation"
    assert captured_error.value.field_path == "fine_path"
    assert captured_error.value.context == {"chunk_start_index": 0}


def test_simulation_run_maps_native_chunk_field_failure_to_integration_error(
    calendar_registry: CalendarRegistry,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class FakeBrokenChunkColumns:
        @property
        def transition_index(self) -> NoReturn:
            raise RuntimeError("native chunk field access failed")

    class FakeNativeChunkResult:
        schema_version = "simulation.seeded_tick_event.path.v1"
        transition_count = 1
        event_count = 0
        estimated_native_retained_bytes = 0

        @staticmethod
        def fine_path_metadata_json() -> str:
            return (
                '{"schema_version":"simulation.seeded_tick_event.fine_path_metadata.v1",'
                '"path_record_schema_version":"simulation.seeded_tick_event.path_record.v1",'
                '"event_projection_record_schema_version":"simulation.seeded_tick_event.event_record.v1",'
                '"transition_ordering_policy":"canonical",'
                '"timestamp_representation_policy":"utc_nanos",'
                '"side_encoding_policy":"snake_case",'
                '"state_snapshot_capture":"disabled"}'
            )

        @staticmethod
        def event_tape_metadata_json() -> str:
            return (
                '{"schema_version":"simulation.seeded_tick_event.event_tape_metadata.v1",'
                '"event_record_schema_version":"simulation.seeded_tick_event.event_record.v1",'
                '"state_snapshot_schema_version":"simulation.seeded_tick_event.state_snapshot.v1",'
                '"event_ordering_policy":"canonical",'
                '"timestamp_representation_policy":"utc_nanos",'
                '"side_encoding_policy":"snake_case",'
                '"state_snapshot_capture":"disabled"}'
            )

        @staticmethod
        def get_fine_path_chunk_columns(start_index: int, max_records: int) -> FakeBrokenChunkColumns:
            del start_index
            del max_records
            return FakeBrokenChunkColumns()

        @staticmethod
        def get_event_tape_chunk_columns(start_index: int, max_records: int) -> NoReturn:
            del start_index
            del max_records
            raise RuntimeError("native event-tape chunk projection failed")

    class FakeNative:
        @staticmethod
        def run_seeded_tick_event_simulation(*args, **kwargs) -> FakeNativeChunkResult:
            del args
            del kwargs
            return FakeNativeChunkResult()

    monkeypatch.setattr(simulation_seeded_runtime_module, "native_module", lambda: FakeNative())

    response = run_seeded_tick_event_simulation(_valid_simulation_request(calendar_registry))

    with pytest.raises(JaniformIntegrationError) as captured_error:
        response.to_payload()

    assert captured_error.value.error_id == "simulation.integration.invalid_native_response"
    assert captured_error.value.workflow == "simulation"
    assert captured_error.value.entrypoint == "run_seeded_tick_event_simulation"
    assert captured_error.value.field_path == "fine_path"
    assert captured_error.value.context == {"chunk_start_index": 0}


def test_pricing_run_maps_missing_native_module_to_integration_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def missing_native_module() -> NoReturn:
        raise ModuleNotFoundError("janiform._native")

    monkeypatch.setattr(pricing_workflow_module, "native_module", missing_native_module)

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
        def run_vanilla_option_pricing(*args, **kwargs) -> str:
            raise NativeExecutionError("native pricer failed")

    monkeypatch.setattr(pricing_workflow_module, "native_module", lambda: FakeNative())

    with pytest.raises(JaniformExecutionError) as captured_error:
        run_vanilla_option_pricing(_valid_pricing_execution_input())

    assert captured_error.value.error_id == "pricing.execution.run_failed"
    assert captured_error.value.workflow == "pricing"
    assert captured_error.value.entrypoint == "run_vanilla_option_pricing"
    assert captured_error.value.context == {"source_exception_type": "NativeExecutionError"}


def test_pricing_run_does_not_raise_attribute_error_when_native_exception_type_is_missing(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class FakeNative:
        @staticmethod
        def run_vanilla_option_pricing(*args, **kwargs) -> str:
            raise RuntimeError("native pricer failed")

    monkeypatch.setattr(pricing_workflow_module, "native_module", lambda: FakeNative())

    with pytest.raises(JaniformExecutionError) as captured_error:
        run_vanilla_option_pricing(_valid_pricing_execution_input())

    assert captured_error.value.error_id == "pricing.execution.run_failed"
    assert captured_error.value.context == {"source_exception_type": "RuntimeError"}


def test_pricing_run_maps_invalid_native_response_to_integration_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class NativeExecutionError(Exception):
        pass

    class FakeNative:
        PricingExecutionError = NativeExecutionError

        @staticmethod
        def run_vanilla_option_pricing(*args, **kwargs) -> dict[str, Any]:
            return {}

    monkeypatch.setattr(pricing_workflow_module, "native_module", lambda: FakeNative())

    with pytest.raises(JaniformIntegrationError) as captured_error:
        run_vanilla_option_pricing(_valid_pricing_execution_input())

    assert captured_error.value.error_id == "pricing.integration.invalid_native_response"
    assert captured_error.value.workflow == "pricing"
    assert captured_error.value.entrypoint == "run_vanilla_option_pricing"
    assert captured_error.value.context == {"source_error_id": "pricing.validation.contract"}


def test_surface_pricing_run_maps_missing_native_module_to_integration_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def missing_native_module() -> NoReturn:
        raise ModuleNotFoundError("janiform._native")

    monkeypatch.setattr(pricing_workflow_module, "native_module", missing_native_module)

    with pytest.raises(JaniformIntegrationError) as captured_error:
        run_vanilla_option_surface_pricing(build_anchored_surface_execution_input_model())

    assert captured_error.value.error_id == "pricing.integration.native_module_unavailable"
    assert captured_error.value.workflow == "pricing"
    assert captured_error.value.entrypoint == "run_vanilla_option_surface_pricing"
    assert captured_error.value.context == {"module_name": "janiform._native"}


def test_surface_pricing_run_maps_native_execution_failure_to_execution_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class NativeExecutionError(Exception):
        pass

    class FakeNative:
        PricingExecutionError = NativeExecutionError

        @staticmethod
        def run_vanilla_option_surface_pricing(*args, **kwargs) -> str:
            raise NativeExecutionError("native surface pricer failed")

    monkeypatch.setattr(pricing_workflow_module, "native_module", lambda: FakeNative())

    with pytest.raises(JaniformExecutionError) as captured_error:
        run_vanilla_option_surface_pricing(build_anchored_surface_execution_input_model())

    assert captured_error.value.error_id == "pricing.execution.run_failed"
    assert captured_error.value.workflow == "pricing"
    assert captured_error.value.entrypoint == "run_vanilla_option_surface_pricing"
    assert captured_error.value.context == {"source_exception_type": "NativeExecutionError"}


def test_surface_pricing_run_maps_invalid_native_response_to_integration_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class NativeExecutionError(Exception):
        pass

    class FakeNative:
        PricingExecutionError = NativeExecutionError

        @staticmethod
        def run_vanilla_option_surface_pricing(*args, **kwargs) -> dict[str, Any]:
            return {}

    monkeypatch.setattr(pricing_workflow_module, "native_module", lambda: FakeNative())

    with pytest.raises(JaniformIntegrationError) as captured_error:
        run_vanilla_option_surface_pricing(build_anchored_surface_execution_input_model())

    assert captured_error.value.error_id == "pricing.integration.invalid_native_response"
    assert captured_error.value.workflow == "pricing"
    assert captured_error.value.entrypoint == "run_vanilla_option_surface_pricing"
    assert captured_error.value.context == {"source_error_id": "pricing.validation.contract"}


def test_calibration_run_maps_missing_native_module_to_integration_error(
    calendar_registry: CalendarRegistry,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def missing_native_module() -> NoReturn:
        raise ModuleNotFoundError("janiform._native")

    monkeypatch.setattr(calibration_staged_runtime_module, "native_module", missing_native_module)

    with pytest.raises(JaniformIntegrationError) as captured_error:
        run_staged_calibration(
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
        def run_staged_calibration(*args, **kwargs) -> str:
            raise NativeExecutionError("native calibration failed")

    monkeypatch.setattr(calibration_staged_runtime_module, "native_module", lambda: FakeNative())

    with pytest.raises(JaniformExecutionError) as captured_error:
        run_staged_calibration(
            build_valid_execution_input_model(),
            calendar_registry=calendar_registry,
        )

    assert captured_error.value.error_id == "calibration.execution.run_failed"
    assert captured_error.value.workflow == "calibration"
    assert captured_error.value.entrypoint == "run_staged_calibration"
    assert captured_error.value.context == {
        "source_exception_type": "NativeExecutionError",
        "source_error_message": "native calibration failed",
    }


def test_calibration_run_maps_invalid_native_response_to_integration_error(
    calendar_registry: CalendarRegistry,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class NativeExecutionError(Exception):
        pass

    class FakeNative:
        CalibrationExecutionError = NativeExecutionError

        @staticmethod
        def run_staged_calibration(*args, **kwargs) -> dict[str, Any]:
            return {}

    monkeypatch.setattr(calibration_staged_runtime_module, "native_module", lambda: FakeNative())

    with pytest.raises(JaniformIntegrationError) as captured_error:
        run_staged_calibration(
            build_valid_execution_input_model(),
            calendar_registry=calendar_registry,
        )

    assert captured_error.value.error_id == "calibration.integration.invalid_native_response"
    assert captured_error.value.workflow == "calibration"
    assert captured_error.value.entrypoint == "run_staged_calibration"
    assert captured_error.value.context == {"source_error_id": "calibration.validation.contract"}


def test_calendar_registry_resolve_does_not_mask_runtime_errors_when_native_exception_types_are_missing(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class FakeNativeRegistry:
        @staticmethod
        def resolve_market_state(exchange_code: str, timestamp_utc: str) -> str:
            raise RuntimeError("native calendar failure")

    class FakeNative:
        @staticmethod
        def CalendarRegistry(registry_json: str) -> FakeNativeRegistry:
            return FakeNativeRegistry()

    monkeypatch.setattr(calendar_module, "native_module", lambda: FakeNative())

    registry = calendar_module.CalendarRegistry('{"schema_version":"calendar.registry.v1","exchanges":[]}')

    with pytest.raises(RuntimeError, match="native calendar failure"):
        registry.resolve_market_state("XNYS", START_TIME_UTC)


def _valid_hourly_factor_contract(calendar_registry: CalendarRegistry) -> HourlyFactorContract:
    return provide_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=START_TIME_UTC,
        range_end_utc=START_TIME_UTC + timedelta(hours=2),
        seasonality_source=HourlySeasonalityProviderExplicitSource(hourly_factors=[1.0, 1.0]),
        news_intensity_source=HourlyNewsIntensityProviderExplicitSource(hourly_factors=[0.1, 0.1]),
    )


def _valid_simulation_request(calendar_registry: CalendarRegistry) -> SeededTickSimulationRequest:
    hourly_factor_contract = _valid_hourly_factor_contract(calendar_registry)
    (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = build_endogenous_seeded_mean_reversion_runtime_inputs(
        hourly_factor_contract=hourly_factor_contract
    )

    return SeededTickSimulationRequest.from_runtime_inputs(
        seed=17,
        start_time_utc=START_TIME_UTC,
        horizon_interval_count=2,
        kernel_parameters=_valid_kernel_parameters(),
        nig_shared_plus_shift_configuration=NigSharedPlusShiftConfiguration(
            alpha=2.3,
            beta_0=-0.8,
            delta=0.7,
            mu=0.1,
            delta_beta=-0.2,
        ),
        news_process_configuration=NewsProcessConfiguration(
            schema_version="news.process.configuration.v1",
            arrivals=NewsArrivalConfiguration(
                known=KnownNewsArrivalConfiguration(
                    scheduled_event_timestamps_utc_nanos=(),
                ),
                surprise=SurpriseNewsArrivalConfiguration(
                    baseline_intensity=0.0,
                ),
            ),
            shared_mark_law=SharedNewsMarkLawConfiguration(
                gamma_severity_marks=GammaSeverityMarkConfiguration(
                    shape_k=1.5,
                    scale_theta=0.9,
                ),
                double_exponential_base_shock=DoubleExponentialBaseShockConfiguration(
                    probability_positive=0.4,
                    eta_plus=1.3,
                    eta_minus=1.1,
                ),
                coupled_gap_marks=CoupledGapMarkConfiguration(sigma_g=0.75),
            ),
            known_component_mark_scales=NewsComponentMarkScaleConfiguration(
                activity_coupling_scale=0.6,
                return_jump_scale=1.2,
            ),
            surprise_component_mark_scales=NewsComponentMarkScaleConfiguration(
                activity_coupling_scale=0.6,
                return_jump_scale=1.2,
            ),
        ),
        initial_excitation_state=_valid_excitation_state(),
        hourly_factor_contract=hourly_factor_contract,
        return_mark_hour_of_week_factors=(1.0,) * 168,
        mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
    )


def _valid_coarse_simulation_request(
    calendar_registry: CalendarRegistry,
) -> CoarseSimulationRunRequest:
    hourly_factor_contract = _valid_hourly_factor_contract(calendar_registry)
    (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = build_endogenous_mean_reversion_runtime_contracts(interval_count=2)

    return CoarseSimulationRunRequest.from_runtime_inputs(
        seed=17,
        start_time_utc=START_TIME_UTC,
        horizon_interval_count=2,
        interval_delta=1.0,
        sufficient_statistics_model_inputs=SufficientStatisticsModelInputs(
            equation_specification=SufficientStatisticsEquationSpecification(
                hawkes_kernel_parameters=_valid_kernel_parameters(),
                gamma_severity_mark_configuration=GammaSeverityMarkConfiguration(
                    shape_k=1.5,
                    scale_theta=0.9,
                ),
            ),
            nig_shared_plus_shift_configuration=NigSharedPlusShiftConfiguration(
                alpha=2.3,
                beta_0=-0.8,
                delta=0.7,
                mu=0.1,
                delta_beta=-0.2,
            ),
            initial_excitation_state=_valid_excitation_state(),
            mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
            mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
            initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
        ),
        hourly_factor_contract=hourly_factor_contract,
        news_process_configuration=_valid_news_process_configuration(),
        return_mark_hour_of_week_factors=(1.0,) * 168,
        observed_price_tick_size=None,
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
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
    return build_anchored_runtime_execution_input_model()


def _valid_news_process_configuration() -> NewsProcessConfiguration:
    return NewsProcessConfiguration(
        schema_version="news.process.configuration.v1",
        arrivals=NewsArrivalConfiguration(
            known=KnownNewsArrivalConfiguration(
                scheduled_event_timestamps_utc_nanos=(),
            ),
            surprise=SurpriseNewsArrivalConfiguration(
                baseline_intensity=0.75,
            ),
        ),
        shared_mark_law=SharedNewsMarkLawConfiguration(
            gamma_severity_marks=GammaSeverityMarkConfiguration(
                shape_k=1.8,
                scale_theta=0.7,
            ),
            double_exponential_base_shock=DoubleExponentialBaseShockConfiguration(
                probability_positive=0.35,
                eta_plus=2.1,
                eta_minus=3.0,
            ),
            coupled_gap_marks=CoupledGapMarkConfiguration(sigma_g=0.6),
        ),
        known_component_mark_scales=NewsComponentMarkScaleConfiguration(
            activity_coupling_scale=1.0,
            return_jump_scale=1.0,
        ),
        surprise_component_mark_scales=NewsComponentMarkScaleConfiguration(
            activity_coupling_scale=1.0,
            return_jump_scale=1.0,
        ),
    )
