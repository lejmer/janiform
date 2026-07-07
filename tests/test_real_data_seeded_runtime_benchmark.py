import math
import runpy
import sys
from copy import deepcopy
from dataclasses import replace
from functools import lru_cache
from pathlib import Path
from types import SimpleNamespace

import pytest

from benches.python.support import (
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
    load_real_data_seeded_runtime_benchmark_report_from_json,
    run_real_data_seeded_runtime_benchmark_fixture,
)
from janiform.calibration.staged import CalibrationStage, CalibrationStageExecutionStatus


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
    fixture = _fixture_document(fixtures_root).fixtures[0]
    report = _cloned_cached_fixture_report(fixtures_root)

    assert report.schema_version == REAL_DATA_SEEDED_RUNTIME_BENCHMARK_REPORT_SCHEMA_VERSION
    assert report.output_schema_metadata.required_report_fields
    assert report.output_schema_metadata.required_access_pattern_fields
    assert report.output_schema_metadata.required_provenance_fields
    assert report.fixture_id == fixture.fixture_id
    assert report.observed_source_tick_count == fixture.source_tick_data_row_count
    assert report.seeded_transition_count >= report.seeded_event_count
    assert report.native_runtime_seconds > 0.0
    assert report.python_materialization_seconds > 0.0
    assert report.boundary_transport_bytes > 0
    assert report.native_estimated_retained_bytes > 0
    assert len(report.access_pattern_reports) == len(fixture.materialization_access_patterns)
    assert report.access_pattern_reports[0].materialized_event_count > 0
    assert (
        report.access_pattern_reports[-1].materialized_transition_count
        == report.seeded_transition_count
    )
    assert report.provenance.source_tick_data_path == fixture.source_tick_data_path
    assert report.provenance.source_tick_data_format == fixture.source_tick_data_format
    assert report.provenance.source_tick_data_row_offset == fixture.source_tick_data_row_offset
    assert report.provenance.source_tick_data_row_count == fixture.source_tick_data_row_count
    assert report.provenance.source_trade_sign_column is None
    assert (
        report.provenance.memory_metric_semantics
        == "estimated_native_retained_bytes_proxy_not_peak_rss.v1"
    )


def test_real_data_seeded_runtime_baseline_comparison_passes_for_self_captured_baseline(
    fixtures_root: Path,
) -> None:
    report = _cloned_cached_fixture_report(fixtures_root)
    fixture_document = _fixture_document(fixtures_root)
    policy = _self_pass_policy(_policy_artifact(fixtures_root), report)
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
    report = _cloned_cached_fixture_report(fixtures_root)
    policy = _policy_artifact(fixtures_root)
    baseline = _baseline_artifact(fixtures_root)
    candidate = _model_regime_drift_candidate(report, policy)

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
    report = _cloned_cached_fixture_report(fixtures_root)
    policy = _policy_artifact(fixtures_root)
    baseline = _baseline_artifact(fixtures_root)
    thresholds = policy.threshold_for_fixture(report.fixture_id)
    baseline_record = baseline.record_for_fixture(report.fixture_id)

    candidate = replace(
        report,
        python_materialization_seconds=max(
            report.python_materialization_seconds,
            baseline_record.python_materialization_seconds
            + thresholds.maximum_python_materialization_absolute_regression_seconds
            + 1.0,
            baseline_record.python_materialization_seconds
            * (1.0 + thresholds.maximum_python_materialization_relative_regression + 0.25),
        ),
        boundary_transport_bytes=int(
            max(
                float(report.boundary_transport_bytes),
                float(baseline_record.boundary_transport_bytes)
                + float(thresholds.maximum_boundary_transport_absolute_growth_bytes)
                + 1_000_000.0,
                float(baseline_record.boundary_transport_bytes)
                * (1.0 + thresholds.maximum_boundary_transport_relative_growth + 0.05),
            )
        ),
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
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fixture_path = _fixture_path(fixtures_root)
    policy_path = _policy_path(fixtures_root)
    baseline_path = _baseline_path(fixtures_root)
    cached_report = _cloned_cached_fixture_report(fixtures_root)

    monkeypatch.setattr(
        "benches.python.support.run_real_data_seeded_runtime_benchmark_fixture",
        lambda fixture: cached_report,
    )
    monkeypatch.setattr(
        sys,
        "argv",
        [
            "run_real_data_seeded_runtime_benchmark.py",
            "--fixture-path",
            str(fixture_path),
            "--policy-path",
            str(policy_path),
            "--baseline-path",
            str(baseline_path),
            "--output-dir",
            str(tmp_path),
            "--allow-regression",
        ],
    )
    script_globals = runpy.run_path(
        str(
            Path(__file__).resolve().parents[1]
            / "scripts"
            / "ci"
            / "run_real_data_seeded_runtime_benchmark.py"
        ),
        run_name="real_data_seeded_runtime_benchmark_test",
    )
    return_code = script_globals["main"]()

    assert return_code == 0
    assert (tmp_path / "real_data_seeded_runtime_benchmark_report.json").exists()
    comparison_path = tmp_path / "real_data_seeded_runtime_benchmark_comparison.json"
    assert comparison_path.exists()
    assert comparison_path.read_text(encoding="utf-8").startswith("{\n")

    summary_path = tmp_path / "real_data_seeded_runtime_benchmark_summary.md"
    assert summary_path.exists()
    assert "Real-Data Seeded Runtime Benchmark" in summary_path.read_text(encoding="utf-8")


def test_real_data_seeded_runtime_fixture_surfaces_staged_fit_failure_details(
    fixtures_root: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fixture = _fixture_document(fixtures_root).fixtures[0]

    monkeypatch.setattr(
        "benches.python.support.seeded_runtime._write_source_tick_data_slice",
        lambda *, fixture, output_path: output_path.write_bytes(b"fixture"),
    )

    class FailingEstimator:
        def __init__(self, *, market_profile: str, cpu_count: int) -> None:
            self.market_profile = market_profile
            self.cpu_count = cpu_count

        def fit(
            self,
            *,
            tick_data_path: str | Path,
            report: bool,
            silent: bool,
        ) -> tuple[None, SimpleNamespace]:
            assert report is True
            assert silent is True

            failure = SimpleNamespace(
                failure_kind="optimizer_failure",
                error_message="joint refinement diverged",
                checkpoint_reference=None,
                artifact_reference=None,
            )
            stage_report = SimpleNamespace(
                stage=CalibrationStage.JOINT_REFINEMENT,
                failure=failure,
            )
            staged_response = SimpleNamespace(
                run_id="benchmark-fit-run",
                status=CalibrationStageExecutionStatus.FAILURE,
                stage_reports=(stage_report,),
                failure=None,
            )
            fit_report = SimpleNamespace(staged_response=staged_response)
            return None, fit_report

    monkeypatch.setattr(
        "benches.python.support.seeded_runtime.JaniformEstimator",
        FailingEstimator,
    )

    with pytest.raises(RuntimeError) as error_info:
        run_real_data_seeded_runtime_benchmark_fixture(fixture)

    error_message = str(error_info.value)

    assert f"fit failed for fixture `{fixture.fixture_id}`" in error_message
    assert f"source_tick_data_row_offset: {fixture.source_tick_data_row_offset}" in error_message
    assert "staged_failure_summary: Calibration failed at joint_refinement with optimizer_failure: joint refinement diverged" in error_message
    assert '"run_id": "benchmark-fit-run"' in error_message
    assert '"failed_stage": "joint_refinement"' in error_message
    assert '"failure_kind": "optimizer_failure"' in error_message


def test_real_data_seeded_runtime_fixture_uses_explicit_cpu_budget(
    fixtures_root: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fixture = _fixture_document(fixtures_root).fixtures[0]
    constructed_cpu_counts: list[int] = []

    monkeypatch.setattr(
        "benches.python.support.seeded_runtime._write_source_tick_data_slice",
        lambda *, fixture, output_path: output_path.write_bytes(b"fixture"),
    )

    class CapturingEstimator:
        def __init__(self, *, market_profile: str, cpu_count: int) -> None:
            assert market_profile == fixture.market_profile
            constructed_cpu_counts.append(cpu_count)

        def fit(
            self,
            *,
            tick_data_path: str | Path,
            report: bool,
            silent: bool,
        ) -> tuple[None, SimpleNamespace]:
            failure = SimpleNamespace(
                failure_kind="optimizer_failure",
                error_message="joint refinement diverged",
                checkpoint_reference=None,
                artifact_reference=None,
            )
            stage_report = SimpleNamespace(
                stage=CalibrationStage.JOINT_REFINEMENT,
                failure=failure,
            )
            staged_response = SimpleNamespace(
                run_id="benchmark-fit-run",
                status=CalibrationStageExecutionStatus.FAILURE,
                stage_reports=(stage_report,),
                failure=None,
            )
            fit_report = SimpleNamespace(staged_response=staged_response)
            return None, fit_report

    monkeypatch.setattr(
        "benches.python.support.seeded_runtime.JaniformEstimator",
        CapturingEstimator,
    )

    with pytest.raises(RuntimeError):
        run_real_data_seeded_runtime_benchmark_fixture(fixture)

    assert constructed_cpu_counts == [fixture.cpu_count]


@lru_cache(maxsize=1)
def _cached_fixture_report(fixtures_root: Path) -> RealDataSeededRuntimeBenchmarkReport:
    return load_real_data_seeded_runtime_benchmark_report_from_json(
        _report_path(fixtures_root).read_text(encoding="utf-8")
    )


def _cloned_cached_fixture_report(fixtures_root: Path) -> RealDataSeededRuntimeBenchmarkReport:
    return deepcopy(_cached_fixture_report(fixtures_root))


def _self_pass_policy(
    policy: RealDataSeededRuntimeBenchmarkPolicy,
    report: RealDataSeededRuntimeBenchmarkReport,
) -> RealDataSeededRuntimeBenchmarkPolicy:
    threshold = policy.threshold_for_fixture(report.fixture_id)
    seeded_event_rate_ratio_vs_observed = (
        report.seeded_event_count / report.provenance.seeded_horizon_interval_count
    ) / max(report.observed_open_event_rate_per_hour, 1.0e-12)
    coarse_event_rate_ratio_vs_observed = (
        report.coarse_generated_event_count / report.provenance.coarse_horizon_interval_count
    ) / max(report.observed_open_event_rate_per_hour, 1.0e-12)
    seeded_event_rate_ratio_vs_calibrated = (
        (report.seeded_event_count / report.provenance.seeded_horizon_interval_count)
        / max(report.calibrated_open_event_rate_per_hour or 0.0, 1.0e-12)
        if report.calibrated_open_event_rate_per_hour is not None
        else threshold.maximum_seeded_event_rate_ratio_vs_calibrated_open
    )
    adjusted_threshold = replace(
        threshold,
        maximum_seeded_event_rate_ratio_vs_observed_open=(
            seeded_event_rate_ratio_vs_observed + 1.0e-9
        ),
        maximum_seeded_event_rate_ratio_vs_calibrated_open=(
            seeded_event_rate_ratio_vs_calibrated + 1.0e-9
        ),
        maximum_coarse_event_rate_ratio_vs_observed_open=(
            coarse_event_rate_ratio_vs_observed + 1.0e-9
        ),
    )
    return replace(policy, fixture_thresholds=(adjusted_threshold,))


def _model_regime_drift_candidate(
    report: RealDataSeededRuntimeBenchmarkReport,
    policy: RealDataSeededRuntimeBenchmarkPolicy,
) -> RealDataSeededRuntimeBenchmarkReport:
    thresholds = policy.threshold_for_fixture(report.fixture_id)
    seeded_interval_count = report.provenance.seeded_horizon_interval_count
    coarse_interval_count = report.provenance.coarse_horizon_interval_count
    observed_open_event_rate_per_hour = report.observed_open_event_rate_per_hour
    calibrated_open_event_rate_per_hour = report.calibrated_open_event_rate_per_hour

    target_seeded_event_count = max(
        report.seeded_event_count + 1,
        math.ceil(
            thresholds.maximum_seeded_event_rate_ratio_vs_observed_open
            * observed_open_event_rate_per_hour
            * seeded_interval_count
        )
        + 1,
    )
    if calibrated_open_event_rate_per_hour is not None:
        target_seeded_event_count = max(
            target_seeded_event_count,
            math.ceil(
                thresholds.maximum_seeded_event_rate_ratio_vs_calibrated_open
                * calibrated_open_event_rate_per_hour
                * seeded_interval_count
            )
            + 1,
        )

    target_coarse_generated_event_count = max(
        report.coarse_generated_event_count + 1,
        math.ceil(
            thresholds.maximum_coarse_event_rate_ratio_vs_observed_open
            * observed_open_event_rate_per_hour
            * coarse_interval_count
        )
        + 1,
    )
    target_seeded_transition_count = max(
        target_seeded_event_count,
        report.seeded_transition_count
        - report.seeded_event_count
        + target_seeded_event_count,
    )

    return replace(
        report,
        coarse_generated_event_count=target_coarse_generated_event_count,
        seeded_event_count=target_seeded_event_count,
        seeded_transition_count=target_seeded_transition_count,
    )


def _fixture_document(fixtures_root: Path) -> RealDataSeededRuntimeBenchmarkFixtureDocument:
    return load_real_data_seeded_runtime_benchmark_fixture_document(
        _fixture_path(fixtures_root).read_bytes()
    )


def _policy_artifact(fixtures_root: Path) -> RealDataSeededRuntimeBenchmarkPolicy:
    return load_real_data_seeded_runtime_benchmark_policy_from_json(
        _policy_path(fixtures_root).read_text(encoding="utf-8")
    )


def _baseline_artifact(fixtures_root: Path) -> RealDataSeededRuntimeBenchmarkBaselineArtifact:
    return load_real_data_seeded_runtime_baseline_from_json(
        _baseline_path(fixtures_root).read_text(encoding="utf-8")
    )


def _fixture_path(fixtures_root: Path) -> Path:
    return (
        fixtures_root
        / "benchmarks"
        / "real_data_seeded_runtime_smoke_fixture.json"
    )


def _report_path(fixtures_root: Path) -> Path:
    return (
        fixtures_root
        / "benchmarks"
        / "real_data_seeded_runtime_smoke_report.json"
    )


def _policy_path(fixtures_root: Path) -> Path:
    return (
        fixtures_root
        / "benchmarks"
        / "real_data_seeded_runtime_smoke_policy.json"
    )


def _baseline_path(fixtures_root: Path) -> Path:
    return (
        fixtures_root
        / "benchmarks"
        / "real_data_seeded_runtime_smoke_baseline_v1.json"
    )
