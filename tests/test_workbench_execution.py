from pathlib import Path
from types import SimpleNamespace
from typing import Any, Optional

from janiform.calibration.market_data.contracts import CalibrationMarketDataWorkflowScope
from janiform.calibration.staged.progress import format_staged_progress_event_detail
from tools.workbench.execution import (
    RESOURCE_SNAPSHOT_INTERVAL_SECONDS,
    WorkbenchExecutionWorker,
    _mean_reversion_configuration_from_setup,
)
from tools.workbench.state import DEFAULT_WORKBENCH_SESSION_PRESET_NAME, WorkbenchRunSetup


def test_workbench_execution_uses_endogenous_mean_reversion_by_default() -> None:
    run_setup = WorkbenchRunSetup(
        dataset_path=Path("ticks.csv"),
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        output_directory=Path("output"),
    )

    configuration = _mean_reversion_configuration_from_setup(run_setup)

    assert configuration is not None
    assert configuration.calibration_grid_interval_hours is None


def test_workbench_execution_builds_mean_reversion_configuration_without_selector() -> None:
    run_setup = WorkbenchRunSetup(
        dataset_path=Path("ticks.csv"),
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        output_directory=Path("output"),
    )

    configuration = _mean_reversion_configuration_from_setup(run_setup)

    assert configuration is not None
    assert configuration.calibration_grid_interval_hours is None


def test_workbench_execution_omits_mean_reversion_for_seasonality_only_runs() -> None:
    run_setup = WorkbenchRunSetup(
        dataset_path=Path("ticks.csv"),
        workflow_scope=CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY,
        output_directory=Path("output"),
    )

    assert _mean_reversion_configuration_from_setup(run_setup) is None


def test_workbench_execution_worker_uses_shared_fit_workflow_entrypoint(
    monkeypatch,
) -> None:
    run_setup = WorkbenchRunSetup(
        dataset_path=Path("ticks.csv"),
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        output_directory=Path("output"),
    )
    worker = WorkbenchExecutionWorker(run_setup)
    recorded_fit_call: dict[str, Any] = {}
    recorded_estimator_kwargs: dict[str, Any] = {}
    emitted_errors: list[str] = []
    completed_results: list[Any] = []

    worker.error_raised.connect(emitted_errors.append)
    worker.execution_completed.connect(completed_results.append)

    model = SimpleNamespace(run_id="workbench-run")
    fit_report = SimpleNamespace(run_id="workbench-run")
    fit_result = SimpleNamespace(
        report=fit_report,
        require_model=lambda: model,
    )

    class _FakeEstimator:
        def __init__(self, **kwargs: Any) -> None:
            recorded_estimator_kwargs.update(kwargs)
            return None

        def _resolve_market_profile_configuration(self) -> None:
            return None

        def fit_with_report(self, **kwargs: Any) -> Any:
            recorded_fit_call.update(kwargs)
            return fit_result

    monkeypatch.setattr("tools.workbench.execution.JaniformEstimator", _FakeEstimator)
    monkeypatch.setattr(
        "tools.workbench.execution.infer_fit_inputs",
        lambda **_: SimpleNamespace(
            run_id="workbench-run",
            tick_data_source=SimpleNamespace(file_format="csv"),
        ),
    )
    monkeypatch.setattr(
        "tools.workbench.execution.build_fit_inference_payload",
        lambda **_: {"run_id": "workbench-run"},
    )
    monkeypatch.setattr(
        "tools.workbench.execution.resolve_fit_artifact_paths",
        lambda **_: SimpleNamespace(run_directory=Path("output/workbench-run")),
    )
    monkeypatch.setattr(
        "tools.workbench.execution.load_workbench_artifacts",
        lambda path: SimpleNamespace(source_path=path),
    )
    worker.run()

    assert emitted_errors == []
    assert recorded_fit_call["run_id"] == "workbench-run"
    assert recorded_fit_call["output_dir"] == Path("output")
    assert recorded_estimator_kwargs["session"] == DEFAULT_WORKBENCH_SESSION_PRESET_NAME
    assert len(completed_results) == 1
    assert completed_results[0].model is model
    assert completed_results[0].fit_report is fit_report


def test_stage_progress_detail_summarizes_joint_refinement_runtime_state() -> None:
    detail = format_staged_progress_event_detail(
        {
            "stage": "joint_refinement",
            "progress_phase": "restart_optimization",
            "completed_units": 3,
            "total_units": 8,
            "current_item_label": "halton_03",
            "valid_completed_units": 2,
            "cumulative_local_evaluation_count": 117,
            "best_restart_id": "baseline",
            "best_weighted_objective": 0.123456789,
        }
    )

    assert detail == (
        "joint_refinement restart optimization 3/8 | item=halton_03 | valid=2 "
        "| local_evaluations=117 | best_candidate=baseline | best_objective=0.123457"
    )


def test_stage_progress_detail_labels_post_restart_joint_refinement_phases() -> None:
    seed_generation_detail = format_staged_progress_event_detail(
        {
            "stage": "joint_refinement",
            "progress_phase": "restart_seed_generation",
            "completed_units": 1,
            "total_units": 2,
            "current_item_label": "tick_moment_seed_generation",
        }
    )
    screening_detail = format_staged_progress_event_detail(
        {
            "stage": "joint_refinement",
            "progress_phase": "restart_candidate_screening",
            "completed_units": 4,
            "total_units": 18,
            "current_item_label": "halton_01",
        }
    )
    coarse_detail = format_staged_progress_event_detail(
        {
            "stage": "joint_refinement",
            "progress_phase": "coarse_consistency_evaluation",
            "completed_units": 7,
            "total_units": 12,
            "current_item_label": "halton_07",
            "current_weighted_objective": 0.117,
            "best_weighted_objective": 0.101,
            "current_coarse_consistency_fit": 1.042,
            "best_coarse_consistency_fit": 0.998,
        }
    )
    final_detail = format_staged_progress_event_detail(
        {
            "stage": "joint_refinement",
            "progress_phase": "final_assembly",
            "completed_units": 2,
            "total_units": 3,
            "current_item_label": "simulation_summaries",
        }
    )

    assert seed_generation_detail == (
        "joint_refinement restart seed generation 1/2 | item=tick_moment_seed_generation"
    )
    assert screening_detail == (
        "joint_refinement restart candidate screening 4/18 | item=halton_01"
    )
    assert coarse_detail == (
        "joint_refinement coarse consistency evaluation 7/12 | item=halton_07 "
        "| objective=0.117000 | best_objective=0.101000 | coarse_fit=1.042000 "
        "| best_coarse_fit=0.998000"
    )
    assert final_detail == "joint_refinement final assembly 2/3 | item=simulation_summaries"


def test_workbench_execution_heartbeat_uses_resource_snapshot_interval(
    monkeypatch,
) -> None:
    run_setup = WorkbenchRunSetup(
        dataset_path=Path("ticks.csv"),
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        output_directory=Path("output"),
    )
    worker = WorkbenchExecutionWorker(run_setup)
    emitted_events: list[tuple[Any, dict[str, Any]]] = []
    recorded_sleep_intervals: list[float] = []

    def fake_emit_event(
        event_type: Any,
        *,
        payload: dict[str, Any],
        stage: Optional[str] = None,
        subtask: Optional[str] = None,
        evaluation_tier: Optional[str] = None,
    ) -> None:
        _ = stage
        _ = subtask
        _ = evaluation_tier
        emitted_events.append((event_type, payload))

    def fake_sleep(interval_seconds: float) -> None:
        recorded_sleep_intervals.append(interval_seconds)
        worker._heartbeat_stop.set()

    monkeypatch.setattr(worker, "_emit_event", fake_emit_event)
    monkeypatch.setattr("tools.workbench.execution.sleep", fake_sleep)

    heartbeat_thread = worker._start_heartbeat()
    heartbeat_thread.join(timeout=1.0)

    assert recorded_sleep_intervals == [RESOURCE_SNAPSHOT_INTERVAL_SECONDS]
    assert len(emitted_events) == 1
    assert emitted_events[0][0] == "resource.snapshot_recorded" or (
        emitted_events[0][0].value == "resource.snapshot_recorded"
    )
    assert emitted_events[0][1]["worker_count"] == 1
