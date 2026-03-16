from typing import Any, cast

import pytest

from benches.python.support import (
    RealDataSeededRuntimeMicrobenchmarkCase,
    load_python_microbenchmark_fixture,
    prepare_real_data_seeded_runtime_microbenchmark_case,
)
from janiform.calibration.fit import JaniformSimulationResult
from janiform.simulation.seeded import (
    deserialize_seeded_tick_simulation_response,
    run_seeded_tick_event_simulation,
    serialize_seeded_tick_simulation_response,
)


@pytest.fixture(scope="session")
def real_data_seeded_runtime_microbenchmark_case(
) -> RealDataSeededRuntimeMicrobenchmarkCase:
    return prepare_real_data_seeded_runtime_microbenchmark_case(
        load_python_microbenchmark_fixture()
    )


def test_seeded_tick_native_wrapper_overhead(
    benchmark: Any,
    real_data_seeded_runtime_microbenchmark_case: RealDataSeededRuntimeMicrobenchmarkCase,
) -> None:
    benchmark.pedantic(
        run_seeded_tick_event_simulation,
        args=(real_data_seeded_runtime_microbenchmark_case.seeded_request,),
        rounds=3,
        iterations=1,
        warmup_rounds=0,
    )


def test_seeded_tick_event_tape_accessor_overhead(
    benchmark: Any,
    real_data_seeded_runtime_microbenchmark_case: RealDataSeededRuntimeMicrobenchmarkCase,
) -> None:
    def materialize_event_tape_head() -> Any:
        response = real_data_seeded_runtime_microbenchmark_case.build_replay_seeded_response()
        return response.event_tape[:256]

    benchmark(materialize_event_tape_head)


def test_seeded_tick_serialization_round_trip_overhead(
    benchmark: Any,
    real_data_seeded_runtime_microbenchmark_case: RealDataSeededRuntimeMicrobenchmarkCase,
) -> None:
    def round_trip_response() -> Any:
        payload_json = serialize_seeded_tick_simulation_response(
            real_data_seeded_runtime_microbenchmark_case.seeded_response
        )
        return deserialize_seeded_tick_simulation_response(payload_json)

    benchmark(round_trip_response)


def test_seeded_tick_to_pandas_materialization_overhead(
    benchmark: Any,
    real_data_seeded_runtime_microbenchmark_case: RealDataSeededRuntimeMicrobenchmarkCase,
) -> None:
    def export_to_pandas() -> Any:
        response = real_data_seeded_runtime_microbenchmark_case.build_replay_seeded_response()
        source_result = real_data_seeded_runtime_microbenchmark_case.seeded_simulation
        replay_result = JaniformSimulationResult(
            simulation_kind=source_result.simulation_kind,
            metadata=dict(source_result.metadata),
            provenance=dict(source_result.provenance),
            diagnostics=response.diagnostics.to_payload(),
            raw_result=response,
            _preferences=cast(Any, source_result._preferences),
        )
        return replay_result.to_pandas()

    benchmark(export_to_pandas)
