import subprocess
import sys
from dataclasses import replace
from functools import lru_cache
from pathlib import Path

from janiform._benchmarking import (
    REAL_DATA_SEEDED_RUNTIME_BASELINE_SCHEMA_VERSION,
    REAL_DATA_SEEDED_RUNTIME_BENCHMARK_FIXTURE_SCHEMA_VERSION,
    REAL_DATA_SEEDED_RUNTIME_BENCHMARK_POLICY_SCHEMA_VERSION,
    REAL_DATA_SEEDED_RUNTIME_BENCHMARK_REPORT_SCHEMA_VERSION,
    RealDataSeededRuntimeBenchmarkBaselineArtifact,
    RealDataSeededRuntimeBenchmarkComparisonReport,
    RealDataSeededRuntimeBenchmarkFixtureDocument,
    RealDataSeededRuntimeBenchmarkPolicy,
    RealDataSeededRuntimeBenchmarkReport,
    SeededRuntimeFailureClassification,
    capture_real_data_seeded_runtime_baseline_artifact,
    compare_real_data_seeded_runtime_benchmark_report_to_baseline,
    load_real_data_seeded_runtime_baseline_from_json,
    load_real_data_seeded_runtime_benchmark_fixture_document,
    load_real_data_seeded_runtime_benchmark_policy_from_json,
    run_real_data_seeded_runtime_benchmark_fixture,
)


def test_real_data_seeded_runtime_fixture_policy_and_baseline_artifacts_load(
    fixtures_root: Path,
) -> None:
    fixture_document = _fixture_document(fixtures_root)
    policy = _policy_artifact(fixtures_root)
    baseline = _baseline_artifact(fixtures_root)

    assert fixture_document.schema_version == REAL_DATA_SEEDED_RUNTIME_BENCHMARK_FIXTURE_SCHEMA_VERSION
    assert policy.schema_version == REAL_DATA_SEEDED_RUNTIME_BENCHMARK_POLICY_SCHEMA_VERSION
    assert baseline.schema_version == REAL_DATA_SEEDED_RUNTIME_BASELINE_SCHEMA_VERSION
    assert len(fixture_document.fixtures) == 1
    assert policy.fixture_thresholds[0].fixture_id == fixture_document.fixtures[0].fixture_id
    assert baseline.records[0].fixture_id == fixture_document.fixtures[0].fixture_id


def test_real_data_seeded_runtime_report_captures_seeded_diagnostics_and_required_metadata(
    fixtures_root: Path,
) -> None:
    report = _cached_fixture_report(fixtures_root)

    assert report.schema_version == REAL_DATA_SEEDED_RUNTIME_BENCHMARK_REPORT_SCHEMA_VERSION
    assert report.output_schema_metadata.required_report_fields
    assert report.output_schema_metadata.required_access_pattern_fields
    assert report.output_schema_metadata.required_provenance_fields
    assert report.observed_source_tick_count == 4096
    assert report.seeded_transition_count >= report.seeded_event_count
    assert report.native_runtime_seconds > 0.0
    assert report.python_materialization_seconds > 0.0
    assert report.boundary_transport_bytes > 0
    assert report.native_estimated_retained_bytes > 0
    assert len(report.access_pattern_reports) == 2
    assert report.access_pattern_reports[0].materialized_event_count == 256
    assert report.access_pattern_reports[1].materialized_transition_count == report.seeded_transition_count
    assert (
        report.provenance.memory_metric_semantics
        == "estimated_native_retained_bytes_proxy_not_peak_rss.v1"
    )


def test_real_data_seeded_runtime_baseline_comparison_passes_for_self_captured_baseline(
    fixtures_root: Path,
) -> None:
    report = _cached_fixture_report(fixtures_root)
    fixture_document = _fixture_document(fixtures_root)
    policy = _policy_artifact(fixtures_root)
    baseline = capture_real_data_seeded_runtime_baseline_artifact(
        fixture_set_id=fixture_document.fixture_set_id,
        reports=(report,),
        baseline_id="self_captured_real_data_seeded_runtime_baseline",
        baseline_version="test.v1",
    )

    comparison = compare_real_data_seeded_runtime_benchmark_report_to_baseline(
        candidate_reports=(report,),
        baseline=baseline,
        policy=policy,
    )

    assert isinstance(comparison, RealDataSeededRuntimeBenchmarkComparisonReport)
    assert comparison.fixture_comparisons[0].status.value == "pass"
    assert comparison.fixture_comparisons[0].failure_classifications == ()


def test_real_data_seeded_runtime_comparison_classifies_model_regime_drift(
    fixtures_root: Path,
) -> None:
    report = _cached_fixture_report(fixtures_root)
    policy = _policy_artifact(fixtures_root)
    baseline = _baseline_artifact(fixtures_root)

    candidate = replace(
        report,
        coarse_generated_event_count=report.coarse_generated_event_count * 5,
        seeded_event_count=report.seeded_event_count * 5,
        seeded_transition_count=report.seeded_transition_count * 5,
    )

    comparison = compare_real_data_seeded_runtime_benchmark_report_to_baseline(
        candidate_reports=(candidate,),
        baseline=baseline,
        policy=policy,
    )
    fixture_comparison = comparison.fixture_comparisons[0]

    assert fixture_comparison.status.value == "fail"
    assert fixture_comparison.primary_failure_classification is SeededRuntimeFailureClassification.MODEL_REGIME_DRIFT
    assert SeededRuntimeFailureClassification.MODEL_REGIME_DRIFT in fixture_comparison.failure_classifications


def test_real_data_seeded_runtime_comparison_classifies_boundary_and_materialization_drift(
    fixtures_root: Path,
) -> None:
    report = _cached_fixture_report(fixtures_root)
    policy = _policy_artifact(fixtures_root)
    baseline = _baseline_artifact(fixtures_root)

    candidate = replace(
        report,
        python_materialization_seconds=report.python_materialization_seconds + 0.5,
        boundary_transport_bytes=report.boundary_transport_bytes + 1_100_000,
    )

    comparison = compare_real_data_seeded_runtime_benchmark_report_to_baseline(
        candidate_reports=(candidate,),
        baseline=baseline,
        policy=policy,
    )
    fixture_comparison = comparison.fixture_comparisons[0]

    assert fixture_comparison.status.value == "fail"
    assert SeededRuntimeFailureClassification.RUNTIME_DRIFT in fixture_comparison.failure_classifications
    assert SeededRuntimeFailureClassification.BOUNDARY_TRANSPORT_DRIFT in fixture_comparison.failure_classifications


def test_real_data_seeded_runtime_benchmark_runner_writes_artifacts(
    fixtures_root: Path,
    tmp_path: Path,
) -> None:
    fixture_path = fixtures_root / "benchmarks" / "real_data_seeded_runtime_benchmark_fixture.json"
    policy_path = fixtures_root / "benchmarks" / "real_data_seeded_runtime_benchmark_policy.json"
    baseline_path = fixtures_root / "benchmarks" / "real_data_seeded_runtime_baseline_v1.json"

    command = [
        sys.executable,
        "scripts/ci/run_real_data_seeded_runtime_benchmark.py",
        "--fixture-path",
        str(fixture_path),
        "--policy-path",
        str(policy_path),
        "--baseline-path",
        str(baseline_path),
        "--output-dir",
        str(tmp_path),
        "--allow-regression",
    ]
    process = subprocess.run(
        command,
        check=False,
        capture_output=True,
        text=True,
        cwd=Path(__file__).resolve().parents[1],
    )

    assert process.returncode == 0, process.stderr
    assert (tmp_path / "real_data_seeded_runtime_benchmark_report.json").exists()
    assert (tmp_path / "real_data_seeded_runtime_benchmark_comparison.json").exists()

    summary_path = tmp_path / "real_data_seeded_runtime_benchmark_summary.md"
    assert summary_path.exists()
    assert "Real-Data Seeded Runtime Benchmark" in summary_path.read_text(encoding="utf-8")


@lru_cache(maxsize=1)
def _cached_fixture_report(fixtures_root: Path) -> RealDataSeededRuntimeBenchmarkReport:
    fixture_document = _fixture_document(fixtures_root)
    return run_real_data_seeded_runtime_benchmark_fixture(fixture_document.fixtures[0])


def _fixture_document(fixtures_root: Path) -> RealDataSeededRuntimeBenchmarkFixtureDocument:
    fixture_path = fixtures_root / "benchmarks" / "real_data_seeded_runtime_benchmark_fixture.json"
    return load_real_data_seeded_runtime_benchmark_fixture_document(fixture_path.read_bytes())


def _policy_artifact(fixtures_root: Path) -> RealDataSeededRuntimeBenchmarkPolicy:
    policy_path = fixtures_root / "benchmarks" / "real_data_seeded_runtime_benchmark_policy.json"
    return load_real_data_seeded_runtime_benchmark_policy_from_json(
        policy_path.read_text(encoding="utf-8")
    )


def _baseline_artifact(fixtures_root: Path) -> RealDataSeededRuntimeBenchmarkBaselineArtifact:
    baseline_path = fixtures_root / "benchmarks" / "real_data_seeded_runtime_baseline_v1.json"
    return load_real_data_seeded_runtime_baseline_from_json(
        baseline_path.read_text(encoding="utf-8")
    )
