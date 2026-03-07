import json
from datetime import UTC, datetime

import pytest

from janiform.simulation import (
    HawkesExcitationState,
    HawkesKernelLoadings,
    HawkesKernelParameters,
    SeededTickEventSide,
    SeededTickEventStateSnapshotCapture,
    SeededTickSimulationRequest,
    SeededTickSimulationResponse,
    SeededTickSimulationValidationError,
    deserialize_seeded_tick_simulation_response,
    serialize_seeded_tick_simulation_response,
    validate_seeded_tick_simulation_response,
)


def test_simulation_request_model_validates_construction() -> None:
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
        initial_excitation_state=HawkesExcitationState(
            y_plus_plus=0.0,
            y_plus_minus=0.0,
            y_minus_plus=0.0,
            y_minus_minus=0.0,
        ),
        hourly_factor_summary=_valid_hourly_factor_summary(),
        state_snapshot_capture=SeededTickEventStateSnapshotCapture.POST_EVENT_TRANSITION,
    )

    assert request.state_snapshot_capture == SeededTickEventStateSnapshotCapture.POST_EVENT_TRANSITION


def test_simulation_request_rejects_non_positive_sampling_stride() -> None:
    with pytest.raises(SeededTickSimulationValidationError) as captured_error:
        SeededTickSimulationRequest(
            seed=17,
            start_time_utc=datetime(2026, 2, 2, tzinfo=UTC),
            horizon_interval_count=2,
            kernel_parameters=_valid_kernel_parameters(),
            initial_excitation_state=_valid_excitation_state(),
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


def test_simulation_response_rejects_invalid_side_with_structured_error() -> None:
    response_payload = _valid_response_payload()
    response_payload["event_tape"][0]["side"] = "hold"

    with pytest.raises(SeededTickSimulationValidationError) as captured_error:
        validate_seeded_tick_simulation_response(response_payload)

    assert captured_error.value.failure.error_id == "simulation.validation.contract"
    assert captured_error.value.failure.field_path == "seeded_tick_simulation_response.event_tape[0].side"
    assert "must be one of: buy, sell" in captured_error.value.failure.message


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
    from janiform.simulation import SeededTickSimulationHourlyFactorSummary

    return SeededTickSimulationHourlyFactorSummary(
        schema_version="seasonality.hourly_factor.contract.v1",
        exchange_code="XNYS",
        interval_count=2,
        interval_start_utc_nanos=(1, 2),
        interval_end_utc_nanos=(2, 3),
        s_h=(1.0, 1.1),
        nu_h=(0.2, 0.1),
    )


def _valid_response_payload() -> dict[str, object]:
    return {
        "schema_version": "simulation.seeded_tick_event.path.v1",
        "metadata": {
            "schema_version": "simulation.seeded_tick_event.tape_metadata.v1",
            "event_record_schema_version": "simulation.seeded_tick_event.record.v1",
            "state_snapshot_schema_version": "simulation.seeded_tick_event.state_snapshot.v1",
            "event_ordering_policy": "deterministic_tie_break.v1",
            "timestamp_representation_policy": "utc_nanos.v1",
            "side_encoding_policy": "buy_sell.v1",
            "state_snapshot_capture": "disabled",
            "state_snapshot_capture_point_policy": None,
        },
        "event_tape": [
            {
                "event_index": 0,
                "timestamp_utc_nanos": 100,
                "side": SeededTickEventSide.BUY.value,
            }
        ],
    }
