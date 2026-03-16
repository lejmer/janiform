import subprocess
import tempfile
from pathlib import Path
from typing import Any

import orjson


def _project_root() -> Path:
    return Path(__file__).resolve().parents[2]


def _load_workflow_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def test_fast_tier_workflow_has_required_triggers_fields_and_steps() -> None:
    workflow_path: Path = _project_root() / ".github" / "workflows" / "ci-fast-regression.yml"
    workflow_text: str = _load_workflow_text(workflow_path)
    runner_script_path: Path = _project_root() / "scripts" / "ci" / "run_fast_tier.py"
    runner_script_text: str = _load_workflow_text(runner_script_path)

    assert "name: ci-fast-regression" in workflow_text
    assert "on:" in workflow_text
    assert "pull_request:" in workflow_text
    assert "workflow_dispatch:" in workflow_text
    assert "fast-deterministic-regression:" in workflow_text
    assert "timeout-minutes: 30" in workflow_text
    assert "scripts/ci/run_fast_tier.py" in workflow_text
    assert "scripts/ci/parse_fast_tier_output.py" in workflow_text
    assert "scripts/ci/write_benchmark_artifact_index.py" in workflow_text
    assert "uses: actions/upload-artifact@v7" in workflow_text
    assert "deterministic_regression_harness" in runner_script_text
    assert "coarse_fine_consistency_regression_harness" in runner_script_text
    assert "pricing_benchmark_validation_regression" in runner_script_text


def test_benchmark_tier_workflow_has_schedule_trigger_and_artifact_hooks() -> None:
    workflow_path: Path = _project_root() / ".github" / "workflows" / "ci-benchmark-tier.yml"
    workflow_text: str = _load_workflow_text(workflow_path)

    assert "name: ci-benchmark-smoke-tier" in workflow_text
    assert "on:" in workflow_text
    assert "schedule:" in workflow_text
    assert "workflow_dispatch:" in workflow_text
    assert "pull_request:" not in workflow_text
    assert "scheduled-benchmark-smoke-workloads:" in workflow_text
    assert "timeout-minutes: 60" in workflow_text
    assert "Run benchmark smoke workloads" in workflow_text
    assert "--test benchmark_scenario_catalog" in workflow_text
    assert "--test oracle_pricer_risk_benchmark_harness" in workflow_text
    assert "--test performance_baseline_capture_and_storage" in workflow_text
    assert "--test regression_drift_attribution_reports" in workflow_text
    assert "scripts/ci/write_benchmark_artifact_index.py" in workflow_text
    assert "real-data-seeded-runtime-benchmark:" in workflow_text
    assert "Run real-data seeded runtime benchmark" in workflow_text
    assert "scripts/ci/run_real_data_seeded_runtime_benchmark.py" in workflow_text
    assert "real_data_seeded_runtime_benchmark_summary.md" in workflow_text
    assert "real-data-seeded-runtime-benchmark-artifacts" in workflow_text
    assert "uses: actions/upload-artifact@v7" in workflow_text


def test_fast_tier_runner_and_parser_smoke() -> None:
    project_root: Path = _project_root()
    with tempfile.TemporaryDirectory(prefix="janiform-ci-fast-tier-") as temporary_dir:
        output_directory: Path = Path(temporary_dir)
        run_log_path: Path = output_directory / "runner-output.log"

        run_command = [
            "python3",
            "scripts/ci/run_fast_tier.py",
            "--output-dir",
            output_directory.as_posix(),
            "--command",
            "smoke_command_one::python3 -c \"print('smoke-one')\"",
            "--command",
            "smoke_command_two::python3 -c \"print('smoke-two')\"",
        ]

        run_process = subprocess.run(
            run_command,
            check=False,
            capture_output=True,
            text=True,
            cwd=project_root,
        )
        run_log_path.write_text(run_process.stdout, encoding="utf-8")
        assert run_process.returncode == 0, run_process.stderr

        parser_output_path: Path = output_directory / "parsed-output.json"
        parse_command = [
            "python3",
            "scripts/ci/parse_fast_tier_output.py",
            "--input-log",
            run_log_path.as_posix(),
            "--output-json",
            parser_output_path.as_posix(),
        ]

        parse_process = subprocess.run(
            parse_command,
            check=False,
            capture_output=True,
            text=True,
            cwd=project_root,
        )
        assert parse_process.returncode == 0, parse_process.stderr

        parsed_output: dict[str, Any] = orjson.loads(
            parser_output_path.read_text(encoding="utf-8"),
        )
        assert parsed_output["schema_version"] == "ci.fast_tier.parsed_output.v1"
        assert parsed_output["result_count"] == 2

        results: list[dict[str, Any]] = list(parsed_output["results"])
        command_names: list[str] = [
            str(result["command_name"]) for result in results
        ]
        assert command_names == ["smoke_command_one", "smoke_command_two"]
        assert all(result["status"] == "pass" for result in results)


def test_benchmark_artifact_index_hook_emits_links() -> None:
    project_root: Path = _project_root()
    with tempfile.TemporaryDirectory(prefix="janiform-ci-benchmark-tier-") as temporary_dir:
        output_directory = Path(temporary_dir)
        command = [
            "python3",
            "scripts/ci/write_benchmark_artifact_index.py",
            "--output-dir",
            output_directory.as_posix(),
            "--repository",
            "acme/janiform",
            "--git-sha",
            "abcdef1234567890",
            "--run-id",
            "123456789",
            "--scenario-path",
            "tests/fixtures/benchmarks/benchmark_scenario_catalog.json",
            "--baseline-path",
            "tests/fixtures/benchmarks/oracle_pricer_risk_performance_baseline_v1_1.json",
        ]

        process = subprocess.run(
            command,
            check=False,
            capture_output=True,
            text=True,
            cwd=project_root,
        )
        assert process.returncode == 0, process.stderr

        index_json_path: Path = output_directory / "benchmark_artifact_index.json"
        index_markdown_path: Path = output_directory / "benchmark_artifact_index.md"
        assert index_json_path.exists()
        assert index_markdown_path.exists()

        index_payload: dict[str, Any] = orjson.loads(index_json_path.read_text(encoding="utf-8"))
        assert index_payload["schema_version"] == "ci.benchmark_tier.artifact_index.v1"

        entries: list[dict[str, Any]] = list(index_payload["entries"])
        assert len(entries) == 2
        assert entries[0]["artifact_type"] == "scenario_catalog"
        assert entries[1]["artifact_type"] == "performance_baseline"

        index_markdown: str = index_markdown_path.read_text(encoding="utf-8")
        assert "Scenario catalog" in index_markdown
        assert "Performance baseline" in index_markdown
        assert "https://github.com/acme/janiform/actions/runs/123456789" in index_markdown
