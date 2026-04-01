import os
import subprocess
import sys
from functools import lru_cache
from pathlib import Path

from benches.python.support import (
    RealDataSeededRuntimeMicrobenchmarkCase,
    load_python_microbenchmark_fixture,
    prepare_real_data_seeded_runtime_microbenchmark_case,
)


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


def _benchmark_pythonpath() -> str:
    return os.pathsep.join(["python", "."])


def test_real_data_seeded_runtime_microbenchmark_case_reuses_checked_in_fixture() -> None:
    microbenchmark_case = _cached_real_data_seeded_runtime_microbenchmark_case()

    assert (
        microbenchmark_case.fixture.fixture_id
        == "synthetic_eurusd_parquet_slice_240000_512row_smoke_v1"
    )
    assert microbenchmark_case.observed_source_tick_count == 512
    assert microbenchmark_case.seeded_response.diagnostics.transition_count > 0
    assert microbenchmark_case.seeded_response.diagnostics.event_count > 0
    assert (
        microbenchmark_case.implied_metrics["observed_open_event_rate_per_hour"] > 0.0
    )
    assert (
        microbenchmark_case.build_replay_seeded_response().diagnostics.transition_count
        == microbenchmark_case.seeded_response.diagnostics.transition_count
    )


def test_criterion_benchmark_targets_are_discoverable_via_cargo_bench_list() -> None:
    process = subprocess.run(
        [
            "cargo",
            "bench",
            "-p",
            "janiform-governance-bench",
            "--bench",
            "rust_microbenchmarks",
            "--",
            "--list",
        ],
        check=False,
        capture_output=True,
        text=True,
        cwd=REPOSITORY_ROOT,
        env={
            **os.environ,
            "TMPDIR": "/tmp",
        },
    )

    assert process.returncode == 0, process.stderr
    assert "oracle_solver_inner_path" in process.stdout
    assert "pricing_kernel_hot_path" in process.stdout
    assert "risk_sampling_kernel" in process.stdout
    assert "simulation_transition_primitive" in process.stdout


def test_pytest_benchmark_suite_runs_with_disabled_timing() -> None:
    process = subprocess.run(
        [
            sys.executable,
            "-m",
            "pytest",
            "--benchmark-disable",
            "benches/python/python_microbenchmarks.py",
        ],
        check=False,
        capture_output=True,
        text=True,
        cwd=REPOSITORY_ROOT,
        env={
            **os.environ,
            "PYTHONPATH": _benchmark_pythonpath(),
        },
    )

    assert process.returncode == 0, process.stderr
    assert "4 passed" in process.stdout


@lru_cache(maxsize=1)
def _cached_real_data_seeded_runtime_microbenchmark_case(
) -> RealDataSeededRuntimeMicrobenchmarkCase:
    return prepare_real_data_seeded_runtime_microbenchmark_case(
        load_python_microbenchmark_fixture()
    )
