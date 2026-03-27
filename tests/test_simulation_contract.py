import json
from datetime import UTC, datetime
from typing import Any

import pytest

from janiform.pricing import GammaSeverityMarkConfiguration, NigSharedPlusShiftConfiguration
from janiform.simulation import (
    HawkesExcitationState,
    HawkesKernelLoadings,
    HawkesKernelParameters,
    MeanReversionBoundaryState,
    MeanReversionRuntimeIntervalAnchors,
    SeededTickEventSide,
    SeededTickEventStateSnapshotCapture,
    SeededTickSimulationValidationError,
)
from janiform.simulation.coarse import (
    CoupledGapMarkConfiguration,
    DoubleExponentialBaseShockConfiguration,
    KnownNewsArrivalConfiguration,
    NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration,
    NewsProcessConfiguration,
    SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
)
from janiform.simulation.seeded import (
    SeededTickSimulationRequest,
    deserialize_seeded_tick_simulation_response,
    run_seeded_tick_event_simulation,
    serialize_seeded_tick_simulation_response,
    validate_seeded_tick_simulation_response,
)
from tests.simulation_test_support import build_disabled_mean_reversion_runtime_contracts


def test_simulation_request_model_validates_construction() -> None:
    (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = build_disabled_mean_reversion_runtime_contracts(interval_count=2)

    request = SeededTickSimulationRequest(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, tzinfo=UTC),
        horizon_interval_count=2,
        kernel_parameters=HawkesKernelParameters(
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
        nig_shared_plus_shift_configuration=_valid_nig_shared_plus_shift_configuration(),
        news_process_configuration=_valid_news_process_configuration(),
        initial_excitation_state=HawkesExcitationState(
            y_plus_plus=0.0,
            y_plus_minus=0.0,
            y_minus_plus=0.0,
            y_minus_minus=0.0,
        ),
        mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
        hourly_factor_summary=_valid_hourly_factor_summary(),
        state_snapshot_capture=SeededTickEventStateSnapshotCapture.POST_EVENT_TRANSITION,
    )

    assert request.state_snapshot_capture == SeededTickEventStateSnapshotCapture.POST_EVENT_TRANSITION


def test_simulation_request_rejects_non_positive_sampling_stride() -> None:
    (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = build_disabled_mean_reversion_runtime_contracts(interval_count=2)

    with pytest.raises(SeededTickSimulationValidationError) as captured_error:
        SeededTickSimulationRequest(
            seed=17,
            start_time_utc=datetime(2026, 2, 2, tzinfo=UTC),
            horizon_interval_count=2,
            kernel_parameters=_valid_kernel_parameters(),
            nig_shared_plus_shift_configuration=_valid_nig_shared_plus_shift_configuration(),
            news_process_configuration=_valid_news_process_configuration(),
            initial_excitation_state=_valid_excitation_state(),
            mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
            mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
            initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
            hourly_factor_summary=_valid_hourly_factor_summary(),
            sample_every_nth_transition=0,
        )

    assert captured_error.value.failure.error_id == "simulation.validation.contract"
    assert captured_error.value.failure.workflow == "simulation"
    assert captured_error.value.failure.entrypoint == "contract"
    assert captured_error.value.failure.field_path == "sample_every_nth_transition"
    assert captured_error.value.failure.message == "must be strictly positive"


def test_simulation_response_roundtrip_serialization_is_deterministic() -> None:
    response_payload = _valid_response_payload()

    validated_response = validate_seeded_tick_simulation_response(response_payload)
    serialized_response = serialize_seeded_tick_simulation_response(validated_response)
    deserialized_response = deserialize_seeded_tick_simulation_response(serialized_response)

    assert deserialized_response == validated_response
    assert json.loads(serialized_response)["event_tape"][0]["side"] == "buy"


def test_mean_reversion_runtime_interval_anchors_roundtrip_serialization_is_deterministic() -> None:
    interval_anchors = MeanReversionRuntimeIntervalAnchors(
        interval_alignment_policy="explicit_runtime_interval_grid_alignment.v1",
        start_anchor_value=4.2,
        end_anchor_values=(4.1, 4.0),
        interval_deltas=(1.0, 1.0),
    )

    serialized = interval_anchors.to_json()
    deserialized = MeanReversionRuntimeIntervalAnchors.from_mapping(json.loads(serialized))

    assert deserialized == interval_anchors


def test_mean_reversion_boundary_state_rejects_observed_anchor_invariant_drift() -> None:
    with pytest.raises(
        ValueError,
        match="deviation: must equal observed_log_price - anchor_value",
    ):
        MeanReversionBoundaryState(
            observed_log_price=5.0,
            anchor_value=1.5,
            deviation=3.7,
        )


def test_simulation_response_rejects_invalid_side_with_structured_error() -> None:
    response_payload = _valid_response_payload()
    response_payload["fine_path"][0]["side"] = "hold"

    with pytest.raises(SeededTickSimulationValidationError) as captured_error:
        validate_seeded_tick_simulation_response(response_payload)

    assert captured_error.value.failure.error_id == "simulation.validation.contract"
    assert captured_error.value.failure.field_path == "seeded_tick_simulation_response.fine_path[0].side"
    assert "must be one of: buy, sell" in captured_error.value.failure.message


def test_simulation_entrypoint_rejects_request_without_runtime_hourly_factor_handle() -> None:
    (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = build_disabled_mean_reversion_runtime_contracts(interval_count=2)

    request = SeededTickSimulationRequest(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, tzinfo=UTC),
        horizon_interval_count=2,
        kernel_parameters=_valid_kernel_parameters(),
        nig_shared_plus_shift_configuration=_valid_nig_shared_plus_shift_configuration(),
        news_process_configuration=_valid_news_process_configuration(),
        initial_excitation_state=_valid_excitation_state(),
        mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
        hourly_factor_summary=_valid_hourly_factor_summary(),
    )

    with pytest.raises(SeededTickSimulationValidationError) as captured_error:
        run_seeded_tick_event_simulation(request)

    assert captured_error.value.failure.field_path == "hourly_factor_summary"
    assert "runtime hourly factor contract handle is unavailable" in captured_error.value.failure.message


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


def _valid_hourly_factor_summary():
    from janiform.simulation.seeded import SeededTickSimulationHourlyFactorSummary

    return SeededTickSimulationHourlyFactorSummary(
        schema_version="seasonality.hourly_factor.contract.v1",
        exchange_code="XNYS",
        interval_count=2,
        interval_duration_nanos=1,
        interval_start_utc_nanos=(1, 2),
        interval_end_utc_nanos=(2, 3),
        s_h=(1.0, 1.1),
        nu_h=(0.2, 0.1),
    )


def _valid_nig_shared_plus_shift_configuration() -> NigSharedPlusShiftConfiguration:
    return NigSharedPlusShiftConfiguration(
        alpha=2.3,
        beta_0=-0.8,
        delta=0.7,
        mu=0.1,
        delta_beta=-0.2,
    )


def _valid_news_process_configuration() -> NewsProcessConfiguration:
    return NewsProcessConfiguration(
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
    )


def _valid_response_payload() -> dict[str, Any]:
    return {
        "schema_version": "simulation.seeded_tick_event.path.v1",
        "metadata": {
            "schema_version": "simulation.seeded_tick_event.tape_metadata.v1",
            "path_record_schema_version": "simulation.seeded_tick_event.record.v1",
            "event_projection_record_schema_version": "simulation.seeded_tick_event.record.v1",
            "transition_ordering_policy": "deterministic_tie_break.v1",
            "timestamp_representation_policy": "utc_nanos.v1",
            "side_encoding_policy": "buy_sell.v1",
            "state_snapshot_capture": "disabled",
            "state_snapshot_capture_point_policy": None,
        },
        "event_tape_metadata": {
            "schema_version": "simulation.seeded_tick_event.tape_metadata.v1",
            "event_record_schema_version": "simulation.seeded_tick_event.record.v1",
            "state_snapshot_schema_version": "simulation.seeded_tick_event.state_snapshot.v1",
            "event_ordering_policy": "deterministic_tie_break.v1",
            "timestamp_representation_policy": "utc_nanos.v1",
            "side_encoding_policy": "buy_sell.v1",
            "state_snapshot_capture": "disabled",
            "state_snapshot_capture_point_policy": None,
        },
        "diagnostics": {
            "schema_version": "simulation.seeded_tick.path_diagnostics.v1",
            "transition_count": 1,
            "event_count": 1,
            "native_runtime_seconds": 0.0,
            "python_response_construction_seconds": 0.0,
            "python_materialization_seconds": 0.0,
            "boundary_transport_bytes": 0,
            "native_estimated_retained_bytes": 0,
            "materialized_transition_count": 1,
            "materialized_event_count": 1,
            "boundary_transport_policy": "materialized_payload_projection.v1",
            "boundary_chunk_record_count": 1,
        },
        "fine_path": [
            {
                "transition_index": 0,
                "timestamp_utc_nanos": 100,
                "transition_kind": "buy_trade",
                "side": SeededTickEventSide.BUY.value,
                "news_arrival_component": None,
                "market_state": "open",
                "return_components": {
                    "buy_side_return": 0.1,
                    "sell_side_return": 0.0,
                    "trading_time_return": 0.1,
                    "open_jump_return": 0.0,
                    "closed_gap_return": 0.0,
                    "total_return": 0.1,
                },
                "core_total_return_increment": 0.1,
                "observed_log_price": 0.1,
                "anchor_value": 0.0,
                "deviation_from_anchor": 0.1,
                "cumulative_core_log_return": 0.1,
            }
        ],
        "event_tape": [
            {
                "event_index": 0,
                "timestamp_utc_nanos": 100,
                "side": SeededTickEventSide.BUY.value,
                "core_total_return_increment": 0.1,
                "observed_log_price": 0.1,
                "anchor_value": 0.0,
                "deviation_from_anchor": 0.1,
            }
        ],
    }
