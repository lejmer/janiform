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


def test_workbench_loader_reads_fit_artifact_directory(tmp_path: Path) -> None:
    report_path = tmp_path / "fit_report.json"
    _write_json(
        report_path,
        {
            "run_id": "run-003",
            "staged_response": {
                "run_id": "run-003",
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
                            "selected_candidate_id": "candidate-003",
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
    (tmp_path / "model.json").write_text("{}", encoding="utf-8")

    artifacts = load_workbench_artifacts(tmp_path)

    assert artifacts.source_path == tmp_path
    assert artifacts.source_kind == "run_directory"
    assert artifacts.fit_report_payload is not None
    assert artifacts.model_payload == {}
    assert artifacts.metadata["run_id"] == "run-003"
    assert artifacts.metadata["run_directory"] == str(tmp_path)
    assert artifacts.selected_candidate_id == "candidate-003"


def test_workbench_loader_reads_disk_fit_report_from_run_directory(tmp_path: Path) -> None:
    _write_disk_artifact_store(
        tmp_path,
        run_id="run-005",
        selected_candidate_id="candidate-005",
    )
    _write_json(
        tmp_path / "fit_report.json",
        {
            "run_id": "run-005",
            "staged_response": {
                "run_id": "run-005",
                "status": "success",
                "execution_mode": "disk",
                "diagnostic_artifact_references": [
                    {
                        "artifact_family": "selection_rationale",
                        "relative_path": "diagnostics/selection_rationale.json",
                    }
                ],
            },
        },
    )

    artifacts = load_workbench_artifacts(tmp_path)

    assert artifacts.source_path == tmp_path
    assert artifacts.fit_report_payload is not None
    assert artifacts.metadata["run_id"] == "run-005"
    assert artifacts.metadata["execution_mode"] == "disk"
    assert artifacts.selected_candidate_id == "candidate-005"


def test_workbench_loader_reads_disk_fit_report_document(tmp_path: Path) -> None:
    _write_disk_artifact_store(
        tmp_path,
        run_id="run-006",
        selected_candidate_id="candidate-006",
    )
    report_path = tmp_path / "fit_report.json"
    _write_json(
        report_path,
        {
            "run_id": "run-006",
            "staged_response": {
                "run_id": "run-006",
                "status": "success",
                "execution_mode": "disk",
                "diagnostic_artifact_references": [
                    {
                        "artifact_family": "selection_rationale",
                        "relative_path": "diagnostics/selection_rationale.json",
                    }
                ],
            },
        },
    )

    artifacts = load_workbench_artifacts(report_path)

    assert artifacts.source_path == report_path
    assert artifacts.source_kind == "json_document"
    assert artifacts.fit_report_payload is not None
    assert artifacts.metadata["run_id"] == "run-006"
    assert artifacts.metadata["execution_mode"] == "disk"
    assert artifacts.selected_candidate_id == "candidate-006"


def test_workbench_loader_reads_disk_artifact_store_run_directory(tmp_path: Path) -> None:
    run_directory = tmp_path / "run-004"
    _write_disk_artifact_store(
        run_directory,
        run_id="run-004",
        selected_candidate_id="candidate-004",
    )

    artifacts = load_workbench_artifacts(run_directory)

    assert artifacts.source_path == run_directory
    assert artifacts.source_kind == "run_directory"
    assert artifacts.fit_report_payload is None
    assert artifacts.metadata["run_directory"] == str(run_directory)
    assert artifacts.metadata["run_id"] == "run-004"
    assert artifacts.metadata["execution_mode"] == "disk"
    assert artifacts.selected_candidate_id == "candidate-004"


def _write_json(path: Path, payload: dict[str, object]) -> None:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True), encoding="utf-8")


def _write_disk_artifact_store(
    run_directory: Path,
    *,
    run_id: str,
    selected_candidate_id: str,
) -> None:
    diagnostics_directory = run_directory / "artifacts" / "calibration" / "diagnostics"
    diagnostics_directory.mkdir(parents=True)

    _write_json(
        run_directory / "artifacts" / "calibration" / "artifact_manifest.json",
        {
            "schema_version": "calibration.artifact_registry.manifest.v1",
            "run_id": run_id,
            "execution_mode": "disk",
            "index_metadata": {
                "exchange_code": "XCME",
            },
        },
    )
    _write_json(
        diagnostics_directory / "observed_targets.json",
        {
            "schema_version": "calibration.diagnostics.observed_targets.v1",
            "target_builder_version": "test",
            "targets": [],
        },
    )
    _write_json(
        diagnostics_directory / "candidate_frontier.json",
        {
            "schema_version": "calibration.diagnostics.candidate_frontier.v1",
            "frontier_entries": [],
        },
    )
    _write_json(
        diagnostics_directory / "coarse_tick_consistency.json",
        {
            "schema_version": "calibration.diagnostics.coarse_tick_consistency.v1",
            "metrics": [],
        },
    )
    _write_json(
        diagnostics_directory / "selection_rationale.json",
        {
            "schema_version": "calibration.diagnostics.selection_rationale.v1",
            "decision": {
                "selection_stage": "joint_refinement",
                "selected_candidate_id": selected_candidate_id,
                "selection_policy": "test",
                "validity_screened_candidate_count": 0,
                "admissible_candidate_count": 0,
                "pareto_frontier_count": 0,
                "tie_break_candidate_count": 0,
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
            "entries": [],
        },
    )
