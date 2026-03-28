from types import SimpleNamespace
from typing import Any

from janiform.calibration.staged.contracts import CalibrationStageExecutionStatus
from pathlib import Path

from janiform.calibration.market_data.contracts import CalibrationMarketDataWorkflowScope
from janiform.contracts import MeanReversionAnchorModelKind
from tools.workbench.execution import WorkbenchExecutionWorker, _mean_reversion_configuration_from_setup
from tools.workbench.state import WorkbenchRunSetup


def test_workbench_execution_enables_constant_anchor_mean_reversion_by_default() -> None:
    run_setup = WorkbenchRunSetup(
        dataset_path=Path("ticks.csv"),
        exchange_code="FXSPOT",
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        output_directory=Path("output"),
    )

    configuration = _mean_reversion_configuration_from_setup(run_setup)

    assert configuration is not None
    assert configuration.anchor_model_kind == MeanReversionAnchorModelKind.CONSTANT
    assert configuration.calibration_grid_interval_hours is None


def test_workbench_execution_honors_selected_mean_reversion_anchor_family() -> None:
    run_setup = WorkbenchRunSetup(
        dataset_path=Path("ticks.csv"),
        exchange_code="FXSPOT",
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        output_directory=Path("output"),
        mean_reversion_anchor_model_kind=MeanReversionAnchorModelKind.ENDOGENOUS_CORE_EXPECTATION_ONE_INTERVAL,
    )

    configuration = _mean_reversion_configuration_from_setup(run_setup)

    assert configuration is not None
    assert (
        configuration.anchor_model_kind
        == MeanReversionAnchorModelKind.ENDOGENOUS_CORE_EXPECTATION_ONE_INTERVAL
    )
    assert configuration.calibration_grid_interval_hours is None


def test_workbench_execution_omits_mean_reversion_when_user_turns_it_off() -> None:
    run_setup = WorkbenchRunSetup(
        dataset_path=Path("ticks.csv"),
        exchange_code="FXSPOT",
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        output_directory=Path("output"),
        mean_reversion_enabled=False,
    )

    assert _mean_reversion_configuration_from_setup(run_setup) is None


def test_workbench_execution_omits_mean_reversion_for_seasonality_only_runs() -> None:
    run_setup = WorkbenchRunSetup(
        dataset_path=Path("ticks.csv"),
        exchange_code="FXSPOT",
        workflow_scope=CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY,
        output_directory=Path("output"),
        mean_reversion_enabled=True,
    )

    assert _mean_reversion_configuration_from_setup(run_setup) is None


def test_workbench_execution_worker_uses_prepared_handle_runner_and_current_fit_report_fields(
    monkeypatch,
) -> None:
    run_setup = WorkbenchRunSetup(
        dataset_path=Path("ticks.csv"),
        exchange_code="FXSPOT",
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        output_directory=Path("output"),
    )
    worker = WorkbenchExecutionWorker(run_setup)
    recorded_runner_call: dict[str, Any] = {}
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
        def __init__(self, **_: Any) -> None:
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
    assert callable(recorded_runner_call["event_callback"])
    assert len(completed_results) == 1
    assert completed_results[0].fit_report.staged_request == "staged-request"
    assert completed_results[0].fit_report.prepared_data_reference == "prepared-data-reference"
