from pathlib import Path
from types import SimpleNamespace
from typing import Any, Optional

from janiform.calibration.market_data.contracts import CalibrationMarketDataWorkflowScope
from janiform.calibration.staged.contracts import CalibrationStageExecutionStatus
from janiform.calibration.staged.progress import format_staged_progress_event_detail
from janiform.contracts import MeanReversionAnchorModelKind
from tools.workbench.execution import (
    RESOURCE_SNAPSHOT_INTERVAL_SECONDS,
    WorkbenchExecutionWorker,
    _live_stage_report_payload,
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
    assert (
        configuration.anchor_model_kind
        == MeanReversionAnchorModelKind.ENDOGENOUS_CORE_PRESSURE_LEVEL
    )
    assert configuration.calibration_grid_interval_hours is None


def test_workbench_execution_honors_selected_mean_reversion_anchor_family() -> None:
    run_setup = WorkbenchRunSetup(
        dataset_path=Path("ticks.csv"),
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        output_directory=Path("output"),
        mean_reversion_anchor_model_kind=MeanReversionAnchorModelKind.ENDOGENOUS_CORE_PRESSURE_LEVEL,
    )

    configuration = _mean_reversion_configuration_from_setup(run_setup)

    assert configuration is not None
    assert (
        configuration.anchor_model_kind
        == MeanReversionAnchorModelKind.ENDOGENOUS_CORE_PRESSURE_LEVEL
    )
    assert configuration.calibration_grid_interval_hours is None


def test_workbench_execution_omits_mean_reversion_for_seasonality_only_runs() -> None:
    run_setup = WorkbenchRunSetup(
        dataset_path=Path("ticks.csv"),
        workflow_scope=CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY,
        output_directory=Path("output"),
    )

    assert _mean_reversion_configuration_from_setup(run_setup) is None


def test_workbench_execution_worker_uses_prepared_handle_runner_and_current_fit_report_fields(
    monkeypatch,
) -> None:
    run_setup = WorkbenchRunSetup(
        dataset_path=Path("ticks.csv"),
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        output_directory=Path("output"),
    )
    worker = WorkbenchExecutionWorker(run_setup)
    recorded_runner_call: dict[str, Any] = {}
    recorded_estimator_kwargs: dict[str, Any] = {}
    emitted_errors: list[str] = []
    completed_results: list[Any] = []

    worker.error_raised.connect(emitted_errors.append)
    worker.execution_completed.connect(completed_results.append)

    prepared = SimpleNamespace(
        prepared_handle="prepared-handle",
        session_resolution=SimpleNamespace(resolved_session_definition_json={}),
        diagnostics=SimpleNamespace(),
        provenance=SimpleNamespace(),
        staged_request="staged-request",
        prepared_data_reference="prepared-data-reference",
        prepared_payload_path="prepared-payload-path",
        staged_execution_input_path="staged-execution-input-path",
    )
    prepared_workflow = SimpleNamespace(
        run_id="workbench-run",
        prepared=prepared,
        resolved_configuration=SimpleNamespace(
            market_profile=None,
            known_news=SimpleNamespace(
                selected_preset=None,
                recommended_reference=None,
                recommendation_applied=False,
                source_kind=SimpleNamespace(value="none"),
            ),
            session=SimpleNamespace(source_kind=SimpleNamespace(value="automatic")),
        ),
        input_inference=SimpleNamespace(),
        fit_warning=None,
        reference_price=None,
        temporary_directory=None,
    )

    class _FakeEstimator:
        def __init__(self, **kwargs: Any) -> None:
            recorded_estimator_kwargs.update(kwargs)
            return None

        def _resolve_market_profile_configuration(self) -> None:
            return None

        def _prepare_fit_workflow(self, **_: Any) -> Any:
            return prepared_workflow

    def _fake_runner(
        prepared_handle: Any,
        *,
        calendar_registry: Any,
        event_callback: Any,
    ) -> tuple[Any, dict[str, Any]]:
        recorded_runner_call["prepared_handle"] = prepared_handle
        recorded_runner_call["calendar_registry"] = calendar_registry
        recorded_runner_call["event_callback"] = event_callback
        return (
            SimpleNamespace(
                status=CalibrationStageExecutionStatus.SUCCESS,
                failure=None,
            ),
            {"staged_response": {"stage_reports": []}},
        )

    monkeypatch.setattr("tools.workbench.execution.JaniformEstimator", _FakeEstimator)
    monkeypatch.setattr(
        "tools.workbench.execution.infer_high_level_fit_inputs",
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
        "tools.workbench.execution._run_native_staged_calibration_with_prepared_handle_and_events",
        _fake_runner,
    )
    monkeypatch.setattr(
        "tools.workbench.execution._raw_stage_report_payloads",
        lambda payload: tuple(payload["staged_response"]["stage_reports"]),
    )
    monkeypatch.setattr(
        "tools.workbench.execution.JaniformFitReport",
        lambda **kwargs: SimpleNamespace(**kwargs),
    )
    monkeypatch.setattr(
        "tools.workbench.execution._extract_model_parameters",
        lambda stage_report_payloads: {"stage_report_count": len(stage_report_payloads)},
    )
    monkeypatch.setattr(
        "tools.workbench.execution._build_model_metadata",
        lambda **_: {"metadata": "ok"},
    )
    monkeypatch.setattr(
        "tools.workbench.execution.JaniformModel",
        lambda **kwargs: SimpleNamespace(**kwargs),
    )
    monkeypatch.setattr(
        "tools.workbench.execution.write_fit_artifacts",
        lambda **_: SimpleNamespace(run_directory=Path("output/workbench-run")),
    )
    monkeypatch.setattr(
        "tools.workbench.execution.load_workbench_artifacts",
        lambda path: SimpleNamespace(source_path=path),
    )
    monkeypatch.setattr(
        "tools.workbench.execution.CalendarRegistry",
        lambda payload: SimpleNamespace(payload=payload),
    )

    worker.run()

    assert emitted_errors == []
    assert recorded_runner_call["prepared_handle"] == "prepared-handle"
    assert recorded_estimator_kwargs["session"] == DEFAULT_WORKBENCH_SESSION_PRESET_NAME
    assert callable(recorded_runner_call["event_callback"])
    assert len(completed_results) == 1
    assert completed_results[0].fit_report.staged_request == "staged-request"
    assert completed_results[0].fit_report.prepared_data_reference == "prepared-data-reference"


def test_live_stage_report_payload_omits_non_live_stage_reports() -> None:
    assert _live_stage_report_payload(
        {
            "stage_report_kind": "seasonality",
            "status": "success",
            "normalized_stage_payload": {
                "timestamp_utc_nanos": [1, 2, 3],
            },
        }
    ) is None


def test_live_stage_report_payload_keeps_candidate_diagnostics_only() -> None:
    payload = _live_stage_report_payload(
        {
            "stage_report_kind": "hawkes",
            "status": "success",
            "fitted_stage_payload": {
                "selected_candidate_id": "hawkes-001",
                "candidate_diagnostics": [
                    {
                        "candidate_id": "hawkes-001",
                        "penalized_objective_value": 1.25,
                    }
                ],
                "observed_interarrival_times": [1.0, 2.0, 3.0],
            },
            "normalized_stage_payload": {
                "tick_event_timestamps": [1, 2, 3],
            },
        }
    )

    assert payload == {
        "stage_report_kind": "hawkes",
        "status": "success",
        "fitted_stage_payload": {
            "selected_candidate_id": "hawkes-001",
            "candidate_diagnostics": [
                {
                    "candidate_id": "hawkes-001",
                    "penalized_objective_value": 1.25,
                }
            ],
        },
    }


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
            "best_weighted_objective": 0.123456789,
        }
    )

    assert detail == (
        "joint_refinement restart optimization 3/8 | latest=halton_03 | valid=2 "
        "| local_evaluations=117 | best_objective=0.123457"
    )


def test_stage_progress_detail_labels_post_restart_joint_refinement_phases() -> None:
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

    assert coarse_detail == (
        "joint_refinement coarse consistency evaluation 7/12 | latest=halton_07 "
        "| objective=0.117000 | best_objective=0.101000 | coarse_fit=1.042000 "
        "| best_coarse_fit=0.998000"
    )
    assert final_detail == "joint_refinement final assembly 2/3 | latest=simulation_summaries"


def test_workbench_execution_worker_forwards_stage_progress_native_events() -> None:
    run_setup = WorkbenchRunSetup(
        dataset_path=Path("ticks.csv"),
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        output_directory=Path("output"),
    )
    worker = WorkbenchExecutionWorker(run_setup)
    emitted_events: list[Any] = []
    worker.event_emitted.connect(emitted_events.append)

    worker._forward_native_event(
        {
            "event_kind": "stage_progress",
            "stage": "joint_refinement",
            "progress_phase": "restart_optimization",
            "completed_units": 2,
            "total_units": 8,
            "current_item_label": "activity_aligned",
        }
    )

    assert len(emitted_events) == 1
    emitted_event = emitted_events[0]
    assert emitted_event.event_type.value == "subtask.started"
    assert emitted_event.subtask == "joint_refinement.restart_optimization"
    assert emitted_event.payload["completed_units"] == 2
    assert emitted_event.payload["total_units"] == 8


def test_workbench_execution_worker_forwards_run_started_stage_sequence() -> None:
    run_setup = WorkbenchRunSetup(
        dataset_path=Path("ticks.csv"),
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        output_directory=Path("output"),
    )
    worker = WorkbenchExecutionWorker(run_setup)
    emitted_events: list[Any] = []
    worker.event_emitted.connect(emitted_events.append)

    worker._forward_native_event(
        {
            "event_kind": "run_started",
            "stage_count": 5,
            "stage_sequence": [
                "seasonality",
                "hawkes",
                "return_components",
                "mean_reversion",
                "joint_refinement",
            ],
            "execution_mode": "memory",
        }
    )

    assert len(emitted_events) == 1
    emitted_event = emitted_events[0]
    assert emitted_event.event_type.value == "run.started"
    assert emitted_event.payload["stage_sequence"] == [
        "seasonality",
        "hawkes",
        "return_components",
        "mean_reversion",
        "joint_refinement",
    ]


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
