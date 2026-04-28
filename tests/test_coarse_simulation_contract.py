import json
from datetime import UTC, datetime

import pytest

from janiform._native_api import native_module
from janiform.pricing import GammaSeverityMarkConfiguration, NigSharedPlusShiftConfiguration
from janiform.sufficient_statistics import (
    SufficientStatisticsEquationSpecification,
    SufficientStatisticsModelInputs,
)
from janiform.simulation import (
    HawkesExcitationState,
    HawkesKernelLoadings,
    HawkesKernelParameters,
    SeededTickSimulationValidationError,
)
from janiform.simulation.coarse import (
    COARSE_SIMULATION_PATH_SCHEMA_VERSION,
    COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION,
    CoarseSimulationRunRequest,
    CoupledGapMarkConfiguration,
    DoubleExponentialBaseShockConfiguration,
    KnownNewsArrivalConfiguration,
    NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration,
    NewsProcessConfiguration,
    SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
    deserialize_coarse_simulation_path,
    deserialize_coarse_simulation_run_request,
    run_coarse_simulation,
    serialize_coarse_simulation_path,
    serialize_coarse_simulation_run_request,
    validate_coarse_simulation_path,
    validate_coarse_simulation_run_request,
)
from tests.support.hourly_factor_contracts import build_hourly_factor_contract
from tests.support.simulation import build_endogenous_mean_reversion_runtime_contracts


def test_coarse_simulation_run_request_roundtrip_serialization(
    calendar_registry,
) -> None:
    request = _build_coarse_run_request(calendar_registry)

    serialized_request = serialize_coarse_simulation_run_request(request)
    deserialized_request = deserialize_coarse_simulation_run_request(serialized_request)

    assert deserialized_request == request
    assert json.loads(serialized_request)["schema_version"] == COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION
    assert json.loads(serialized_request)["hourly_factor_contract"]["known_component_nu_h"] == [2.0]


def test_coarse_simulation_run_request_rejects_interval_factor_delta_mismatch(
    calendar_registry,
) -> None:
    request_payload = _build_coarse_run_request(calendar_registry).to_payload()
    request_payload["interval_delta"] = 1.0

    with pytest.raises(SeededTickSimulationValidationError) as captured_error:
        validate_coarse_simulation_run_request(request_payload)

    failure = captured_error.value.failure
    assert failure.error_id == "simulation.validation.contract"
    assert failure.field_path == "hourly_factor_contract.interval_duration_nanos"
    assert "must match interval_delta exactly" in failure.message


def test_coarse_simulation_path_roundtrip_serialization(
    calendar_registry,
) -> None:
    path = run_coarse_simulation(_build_coarse_run_request(calendar_registry))

    serialized_path = serialize_coarse_simulation_path(path)
    deserialized_path = deserialize_coarse_simulation_path(serialized_path)

    assert deserialized_path == path
    assert path.schema_version == COARSE_SIMULATION_PATH_SCHEMA_VERSION
    assert path.records[0].interval_start_utc_nanos == path.start_time_utc_nanos
    assert path.records[-1].interval_end_utc_nanos == path.end_time_utc_nanos
    assert json.loads(serialized_path)["diagnostics"]["simulated_interval_count"] == 1


def test_coarse_simulation_python_wrapper_matches_native_json_bridge(
    calendar_registry,
) -> None:
    request = _build_coarse_run_request(calendar_registry)

    wrapped_path = run_coarse_simulation(request)
    native_path = validate_coarse_simulation_path(
        native_module().run_coarse_simulation(
            {
                "seed": request.seed,
                "start_time_utc_nanos": request.start_time_utc_nanos,
                "horizon_interval_count": request.horizon_interval_count,
                "interval_delta": request.interval_delta,
                "model_inputs": request.model_inputs.to_payload(),
                "hourly_factor_contract": request.hourly_factor_contract._native_result,
            }
        )
    )

    assert wrapped_path == native_path


def _build_coarse_run_request(calendar_registry) -> CoarseSimulationRunRequest:
    (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = build_endogenous_mean_reversion_runtime_contracts(interval_count=1, interval_delta_hours=0.5)

    return CoarseSimulationRunRequest.from_runtime_inputs(
        seed=3_010,
        start_time_utc=datetime.fromtimestamp(1704205800, tz=UTC),
        horizon_interval_count=1,
        interval_delta=0.5,
        sufficient_statistics_model_inputs=SufficientStatisticsModelInputs(
            equation_specification=SufficientStatisticsEquationSpecification(
                hawkes_kernel_parameters=HawkesKernelParameters(
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
                ),
                gamma_severity_mark_configuration=GammaSeverityMarkConfiguration(
                    shape_k=1.8,
                    scale_theta=0.7,
                ),
            ),
            nig_shared_plus_shift_configuration=NigSharedPlusShiftConfiguration(
                alpha=2.3,
                beta_0=-0.8,
                delta=0.7,
                mu=0.1,
                delta_beta=-0.2,
            ),
            initial_excitation_state=HawkesExcitationState(
                y_plus_plus=0.2,
                y_plus_minus=0.1,
                y_minus_plus=0.15,
                y_minus_minus=0.25,
            ),
            mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
            mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
            initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
        ),
        hourly_factor_contract=build_hourly_factor_contract(
            request_start_utc_nanos=1704205800000000000,
            interval_duration_nanos=1_800_000_000_000,
            s_h=(2.0,),
            known_component_nu_h=(2.0,),
            surprise_component_nu_h=(0.0,),
        ),
        news_process_configuration=NewsProcessConfiguration(
            schema_version="news.process.configuration.v1",
            arrivals=NewsArrivalConfiguration(
                known=KnownNewsArrivalConfiguration(
                    scheduled_event_timestamps_utc_nanos=(1704206700000000000,),
                ),
                surprise=SurpriseNewsArrivalConfiguration(baseline_intensity=0.0),
            ),
            shared_mark_law=SharedNewsMarkLawConfiguration(
                gamma_severity_marks=GammaSeverityMarkConfiguration(
                    shape_k=1.4,
                    scale_theta=0.6,
                ),
                double_exponential_base_shock=DoubleExponentialBaseShockConfiguration(
                    probability_positive=0.35,
                    eta_plus=2.1,
                    eta_minus=3.0,
                ),
                coupled_gap_marks=CoupledGapMarkConfiguration(sigma_g=0.8),
            ),
            known_component_mark_scales=NewsComponentMarkScaleConfiguration(
                activity_coupling_scale=1.0,
                return_jump_scale=1.0,
            ),
            surprise_component_mark_scales=NewsComponentMarkScaleConfiguration(
                activity_coupling_scale=1.0,
                return_jump_scale=1.0,
            ),
        ),
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
    )
