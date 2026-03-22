import json
from pathlib import Path

from tools.workbench.artifacts import load_workbench_artifacts
from tools.workbench.candidate_store import (
    candidate_store_from_artifacts,
    candidate_store_from_stage_report_payloads,
)
from tools.workbench.events import WorkbenchEventType, build_workbench_event
from tools.workbench.reducers import apply_workbench_event
from tools.workbench.state import build_initial_workbench_state


def test_candidate_store_enriches_selected_stage_parameters(tmp_path: Path) -> None:
    _write_run_bundle(tmp_path)
    artifacts = load_workbench_artifacts(tmp_path)

    candidate_store = candidate_store_from_artifacts(artifacts)

    assert candidate_store.selected_candidate_id == "candidate-001"
    assert len(candidate_store.candidates) == 2

    selected_candidate = candidate_store.by_id["candidate-001"]
    rejected_candidate = candidate_store.by_id["candidate-002"]

    assert "selected_stage_parameters" in selected_candidate.parameter_snapshot
    assert selected_candidate.parameter_snapshot["selected_stage_parameters"]["stage"] == "joint_refinement"
    assert selected_candidate.candidate_family == "joint_refinement"
    assert selected_candidate.candidate_source == "artifact_frontier"
    assert selected_candidate.admissibility_state == "admissible"
    assert selected_candidate.validity_state == "valid"
    assert selected_candidate.objective_summary["objective_value"] == 0.1
    assert selected_candidate.lineage["originating_stage"] == "joint_refinement"
    assert rejected_candidate.parameter_snapshot == {"theta": 0.6}


def test_workbench_reducer_tracks_live_run_status() -> None:
    state = build_initial_workbench_state(workspace_root=Path.cwd())

    run_started_event = build_workbench_event(
        event_type=WorkbenchEventType.RUN_STARTED,
        run_id="run-001",
        sequence_number=1,
        payload={"detail": "Starting calibration"},
    )

    resource_event = build_workbench_event(
        event_type=WorkbenchEventType.RESOURCE_SNAPSHOT_RECORDED,
        run_id="run-001",
        sequence_number=2,
        payload={"elapsed_seconds": 12.5, "worker_count": 1},
        stage="staged_calibration",
        subtask="running calibration",
    )

    run_completed_event = build_workbench_event(
        event_type=WorkbenchEventType.RUN_COMPLETED,
        run_id="run-001",
        sequence_number=3,
        payload={"detail": "Calibration completed"},
    )

    apply_workbench_event(state, run_started_event)
    apply_workbench_event(state, resource_event)
    apply_workbench_event(state, run_completed_event)

    assert state.session.mode == "inspector"
    assert state.run.status == "completed"
    assert state.run.run_id == "run-001"
    assert state.run.elapsed_seconds == 12.5
    assert state.run.current_stage == "staged_calibration"
    assert state.run.current_subtask == "running calibration"


def test_candidate_store_builds_live_snapshots_from_stage_reports() -> None:
    candidate_store = candidate_store_from_stage_report_payloads(
        (
            {
                "stage_report_kind": "hawkes",
                "fitted_stage_payload": {
                    "selected_candidate_id": "hawkes-001",
                    "fitted_parameters": {"alpha": 0.1},
                    "candidate_diagnostics": [
                        {
                            "candidate_id": "hawkes-001",
                            "penalized_objective_value": 0.4,
                            "constraint_violation_count": 0,
                            "objective_terms": [
                                {"term_id": "branching_ratio", "normalized_error": 0.2}
                            ],
                        },
                        {
                            "candidate_id": "hawkes-002",
                            "penalized_objective_value": 0.8,
                            "constraint_violation_count": 2,
                            "objective_terms": [],
                        },
                    ],
                },
            },
            {
                "stage_report_kind": "joint_refinement",
                "fitted_stage_payload": {
                    "selected_candidate_id": "joint-001",
                    "fitted_parameters": {"theta": 0.25},
                    "candidate_frontier": [
                        {
                            "candidate_id": "joint-001",
                            "valid": True,
                            "admissible": True,
                            "pareto_rank": 1,
                            "lexicographic_rank": 1,
                            "scalar_tie_break": 0.12,
                            "objective_breakdown": {"activity_level_fit": 0.1},
                            "canonical_coordinates": [["theta", 0.25]],
                        }
                    ],
                },
            },
        )
    )

    assert candidate_store.selected_candidate_id == "joint-001"
    assert len(candidate_store.candidates) == 3
    assert candidate_store.by_id["hawkes-001"].family_metrics[0].family_id == "branching_ratio"
    assert candidate_store.by_id["hawkes-002"].rejection_reason == "constraint_violation"
    assert candidate_store.by_id["hawkes-002"].candidate_source == "stage_report"
    assert candidate_store.by_id["joint-001"].objective_summary["objective_value"] == 0.12
    assert candidate_store.by_id["joint-001"].canonical_coordinates == {"theta": 0.25}


def _write_run_bundle(run_directory: Path) -> None:
    (run_directory / "fit_report.json").write_text(
        json.dumps(
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
                            "frontier_entries": [
                                {
                                    "candidate_id": "candidate-001",
                                    "stage": "joint_refinement",
                                    "evaluation_tier": "tick",
                                    "admissible": True,
                                    "valid": True,
                                    "pareto_rank": 0,
                                    "lexicographic_rank": 1,
                                    "scalar_tie_break": 0.1,
                                    "canonical_coordinates": {"theta": 0.5},
                                    "family_metrics": [],
                                },
                                {
                                    "candidate_id": "candidate-002",
                                    "stage": "joint_refinement",
                                    "evaluation_tier": "tick",
                                    "admissible": False,
                                    "valid": True,
                                    "pareto_rank": 1,
                                    "lexicographic_rank": 2,
                                    "scalar_tie_break": 0.2,
                                    "rejection_reason": "constraint_failed",
                                    "canonical_coordinates": {"theta": 0.6},
                                    "family_metrics": [],
                                },
                            ],
                        },
                        "coarse_tick_consistency": {
                            "schema_version": "calibration.diagnostics.coarse_tick_consistency.v1",
                            "metrics": [],
                        },
                        "selection_rationale": {
                            "schema_version": "calibration.diagnostics.selection_rationale.v1",
                            "decision": {
                                "selection_stage": "joint_refinement",
                                "selected_candidate_id": "candidate-001",
                                "selection_policy": "test",
                                "validity_screened_candidate_count": 2,
                                "admissible_candidate_count": 1,
                                "pareto_frontier_count": 1,
                                "tie_break_candidate_count": 1,
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
                            "cpu_count": 8,
                            "entries": [],
                        },
                    },
                },
            },
            indent=2,
            sort_keys=True,
        ),
        encoding="utf-8",
    )

    (run_directory / "model.json").write_text(
        json.dumps(
            {
                "schema_version": "calibration.high_level.model.v1",
                "run_id": "run-001",
                "workflow_scope": "full_without_option_surface",
                "metadata": {},
                "resolved_configuration": {
                    "session": {
                        "source_kind": "automatic",
                        "configuration": {
                            "exchange_code": "FXSPOT",
                            "exchange_name": "FX Spot",
                            "timezone_name": "UTC",
                            "regular_session_windows": [],
                            "recurring_closed_windows": [],
                        },
                        "applied_override_fields": [],
                        "inferred_from_data": True,
                        "selected_preset": None,
                        "inference_resolution": None,
                    },
                    "known_news": {
                        "source_kind": "none",
                        "configuration": None,
                        "selected_preset": None,
                        "recommended_reference": None,
                        "recommendation_applied": False,
                    },
                    "workflow_diagnostics": [],
                },
                "session_resolution": {
                    "exchange_code": "FXSPOT",
                    "exchange_name": "FX Spot",
                    "timezone_name": "UTC",
                    "resolved_session_definition_json": "{\"schema_version\":\"calendar.registry.v1\",\"exchanges\":[]}",
                    "regular_session_windows": [],
                    "recurring_closed_windows": [],
                    "inferred_from_data": True,
                    "applied_override_fields": [],
                },
                "parameters": {
                    "joint_refinement": {
                        "schema_version": "test",
                        "selected_candidate_id": "candidate-001",
                        "fitted_parameters": {"theta": 0.5},
                    }
                },
            },
            indent=2,
            sort_keys=True,
        ),
        encoding="utf-8",
    )
