from dataclasses import dataclass
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any, cast

from .seeded_runtime import (
    RealDataSeededRuntimeBenchmarkFixture,
    RealDataSeededRuntimeBenchmarkFixtureDocument,
    _selected_hawkes_candidate_diagnostics,
    _write_source_tick_data_slice,
    load_real_data_seeded_runtime_benchmark_fixture_document,
)
from ..calibration import JaniformEstimator
from ..calibration.fit import JaniformFitReport, JaniformModel, JaniformSimulationResult
from ..calibration.fit.model import _prepare_model_simulation_inputs
from ..simulation import SeededTickEventStateSnapshotCapture
from ..simulation.seeded import (
    SeededTickSimulationRequest,
    SeededTickSimulationResponse,
    deserialize_seeded_tick_simulation_response,
    serialize_seeded_tick_simulation_response,
)


_CANONICAL_REAL_DATA_SEEDED_RUNTIME_FIXTURE_PATH = (
    Path(__file__).resolve().parents[3]
    / "tests"
    / "fixtures"
    / "benchmarks"
    / "real_data_seeded_runtime_benchmark_fixture.json"
)


@dataclass(frozen=True, slots=True)
class RealDataSeededRuntimeMicrobenchmarkCase:
    fixture: RealDataSeededRuntimeBenchmarkFixture
    model: JaniformModel
    fit_report: JaniformFitReport
    seeded_request: SeededTickSimulationRequest
    seeded_simulation: JaniformSimulationResult
    seeded_response: SeededTickSimulationResponse
    observed_source_tick_count: int
    source_regime: dict[str, Any]
    implied_metrics: dict[str, Any]
    serialized_seeded_response_json: str

    def build_replay_seeded_response(self) -> SeededTickSimulationResponse:
        return deserialize_seeded_tick_simulation_response(
            self.serialized_seeded_response_json
        )


def load_python_microbenchmark_fixture_document(
) -> RealDataSeededRuntimeBenchmarkFixtureDocument:
    return load_real_data_seeded_runtime_benchmark_fixture_document(
        _CANONICAL_REAL_DATA_SEEDED_RUNTIME_FIXTURE_PATH.read_bytes()
    )


def load_python_microbenchmark_fixture(
) -> RealDataSeededRuntimeBenchmarkFixture:
    fixture_document = load_python_microbenchmark_fixture_document()
    return fixture_document.fixtures[0]


def prepare_real_data_seeded_runtime_microbenchmark_case(
    fixture: RealDataSeededRuntimeBenchmarkFixture,
) -> RealDataSeededRuntimeMicrobenchmarkCase:
    with TemporaryDirectory(
        prefix="janiform-real-data-seeded-runtime-microbenchmark-"
    ) as temp_dir:
        temp_path = Path(temp_dir)
        tick_data_path = temp_path / "source_tick_slice.parquet"
        _write_source_tick_data_slice(
            fixture=fixture,
            output_path=tick_data_path,
        )

        model, fit_report = JaniformEstimator(
            market_profile=fixture.market_profile
        ).fit(
            tick_data_path=tick_data_path,
            report=True,
            silent=True,
        )
        if model is None:
            raise RuntimeError(
                f"fit failed while preparing microbenchmark fixture `{fixture.fixture_id}`"
            )

        prepared_inputs = _prepare_model_simulation_inputs(
            model,
            seed=fixture.simulation_seed,
            start_time_utc=fixture.simulation_start_time_utc,
            horizon_interval_count=fixture.seeded_horizon_interval_count,
            interval="1h",
            simulation_kind="seeded_tick",
            initial_excitation_state=None,
            debug_trace_enabled=False,
            sample_every_nth_transition=1,
            max_records_per_chunk=1024,
            state_snapshot_capture=SeededTickEventStateSnapshotCapture.DISABLED,
        )
        if prepared_inputs.seeded_request is None:
            raise RuntimeError(
                "expected seeded microbenchmark preparation to build a seeded request"
            )

        seeded_simulation = model.simulate(
            seed=fixture.simulation_seed,
            start_time_utc=fixture.simulation_start_time_utc,
            horizon_interval_count=fixture.seeded_horizon_interval_count,
            silent=True,
        )
        seeded_response = cast(
            SeededTickSimulationResponse,
            seeded_simulation.raw_result,
        )
        hawkes_candidate = _selected_hawkes_candidate_diagnostics(fit_report)

        return RealDataSeededRuntimeMicrobenchmarkCase(
            fixture=fixture,
            model=model,
            fit_report=fit_report,
            seeded_request=prepared_inputs.seeded_request,
            seeded_simulation=seeded_simulation,
            seeded_response=seeded_response,
            observed_source_tick_count=fit_report.preparation_diagnostics.tick_record_count,
            source_regime=cast(dict[str, Any], hawkes_candidate["source_regime"]),
            implied_metrics=cast(dict[str, Any], hawkes_candidate["implied_metrics"]),
            serialized_seeded_response_json=serialize_seeded_tick_simulation_response(
                seeded_response
            ),
        )
