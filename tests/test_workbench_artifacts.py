import json

from pathlib import Path

from tools.workbench import build_summary_payload, load_workbench_artifacts


def test_workbench_loader_reads_run_directory(tmp_path: Path) -> None:
    diagnostics_directory = tmp_path / "diagnostics"
    diagnostics_directory.mkdir()

    _write_json(
        diagnostics_directory / "observed_targets.json",
        {
            "schema_version": "calibration.diagnostics.observed_targets.v1",
            "target_builder_version": "test",
            "targets": [{"target_id": "target-001"}],
        },
    )
    _write_json(
        diagnostics_directory / "candidate_frontier.json",
        {
            "schema_version": "calibration.diagnostics.candidate_frontier.v1",
            "frontier_entries": [
                {
                    "candidate_id": "candidate-001",
                    "stage": "joint_refinement",
                    "admissible": True,
                    "lexicographic_rank": 1,
                    "scalar_tie_break": 0.25,
                }
            ],
        },
    )
    _write_json(
        diagnostics_directory / "coarse_tick_consistency.json",
        {
            "schema_version": "calibration.diagnostics.coarse_tick_consistency.v1",
            "metrics": [
                {
                    "metric_id": "metric-001",
                    "normalized_delta": 0.15,
                    "coarse_value": 1.0,
                    "tick_value": 1.1,
                }
            ],
        },
    )
    _write_json(
        diagnostics_directory / "selection_rationale.json",
        {
            "schema_version": "calibration.diagnostics.selection_rationale.v1",
            "decision": {
                "selection_stage": "joint_refinement",
                "selected_candidate_id": "candidate-001",
                "selection_policy": "test",
                "validity_screened_candidate_count": 1,
                "admissible_candidate_count": 1,
                "pareto_frontier_count": 1,
                "tie_break_candidate_count": 1,
                "decisive_metric_ids": [],
                "rationale": "test",
            },
        },
    )
    _write_json(
        diagnostics_directory / "simulation_summary.json",
        {
            "schema_version": "calibration.diagnostics.simulation_summary.v1",
            "summaries": [],
        },
    )
    _write_json(
        diagnostics_directory / "search_surface_manifest.json",
        {
            "schema_version": "calibration.diagnostics.search_surface_manifest.v1",
            "surfaces": [],
        },
    )
    _write_json(
        diagnostics_directory / "runtime_profile.json",
        {
            "schema_version": "calibration.diagnostics.runtime_profile.v1",
            "cpu_count": 24,
            "entries": [
                {
                    "component_id": "joint_refinement",
                    "elapsed_seconds": 12.5,
                    "evaluation_count": 32,
                }
            ],
        },
    )

    artifacts = load_workbench_artifacts(tmp_path)
    summary = build_summary_payload(artifacts)

    assert artifacts.selected_candidate_id == "candidate-001"
    assert summary["candidate_count"] == 1
    assert summary["admissible_candidate_count"] == 1
    assert summary["runtime_seconds_total"] == 12.5


def test_workbench_loader_reads_fit_report_bundle(tmp_path: Path) -> None:
    report_path = tmp_path / "fit_report.json"
    _write_json(
        report_path,
        {
            "run_id": "run-001",
            "staged_response": {
                "run_id": "run-001",
                "status": "success",
                "execution_mode": "memory",
                "diagnostic_artifact_bundle": {
                    "observed_targets": {
                        "schema_version": "calibration.diagnostics.observed_targets.v1",
                        "target_builder_version": "test",
                        "targets": [],
                    },
                    "candidate_frontier": {
                        "schema_version": "calibration.diagnostics.candidate_frontier.v1",
                        "frontier_entries": [],
                    },
                    "coarse_tick_consistency": {
                        "schema_version": "calibration.diagnostics.coarse_tick_consistency.v1",
                        "metrics": [],
                    },
                    "selection_rationale": {
                        "schema_version": "calibration.diagnostics.selection_rationale.v1",
                        "decision": {
                            "selection_stage": "joint_refinement",
                            "selected_candidate_id": "candidate-002",
                            "selection_policy": "test",
                            "validity_screened_candidate_count": 0,
                            "admissible_candidate_count": 0,
                            "pareto_frontier_count": 0,
                            "tie_break_candidate_count": 0,
                            "decisive_metric_ids": [],
                            "rationale": "test",
                        },
                    },
                    "simulation_summary": {
                        "schema_version": "calibration.diagnostics.simulation_summary.v1",
                        "summaries": [],
                    },
                    "search_surface_manifest": {
                        "schema_version": "calibration.diagnostics.search_surface_manifest.v1",
                        "surfaces": [],
                    },
                    "runtime_profile": {
                        "schema_version": "calibration.diagnostics.runtime_profile.v1",
                        "cpu_count": 24,
                        "entries": [],
                    },
                },
            },
        },
    )

    artifacts = load_workbench_artifacts(report_path)

    assert artifacts.metadata["run_id"] == "run-001"
    assert artifacts.selected_candidate_id == "candidate-002"


def _write_json(path: Path, payload: dict[str, object]) -> None:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True), encoding="utf-8")
