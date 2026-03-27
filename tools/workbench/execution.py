from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from threading import Event, Thread
from time import monotonic, sleep
from typing import Any, Optional, Protocol

from PyQt6.QtCore import QObject, pyqtSignal

from janiform.calibration.fit.estimator import JaniformEstimator
from janiform.calibration.fit.input_resolution import (
    HighLevelNormalizationPolicyOverrides,
    HighLevelTickDataColumnOverrides,
    build_fit_inference_payload,
    infer_high_level_fit_inputs,
)
from janiform.calibration.fit.model import (
    JaniformModel,
    _build_model_metadata,
    _extract_model_parameters,
    _raw_stage_report_payloads,
)
from janiform.calibration.fit.report import JaniformFitReport
from janiform.calibration.market_data.contracts import (
    CalibrationMarketDataNormalizationPolicy,
    CalibrationTickDataColumnMapping,
)
from janiform.calibration.staged.contracts import CalibrationStageExecutionStatus
from janiform.calibration.staged.runtime import (
    _run_staged_calibration_with_optional_events,
)
from janiform._workflow_outputs import write_fit_artifacts
from janiform.calendar import CalendarRegistry

from .artifacts import load_workbench_artifacts
from .events import WorkbenchEventType, build_workbench_event
from .state import WorkbenchRunSetup


@dataclass(frozen=True, slots=True)
class WorkbenchExecutionResult:
    run_directory: Path
    model: JaniformModel
    fit_report: JaniformFitReport
    artifacts: Any


class _WorkbenchEventEmitter(Protocol):
    def _emit_event(
        self,
        event_type: WorkbenchEventType,
        *,
        payload: dict[str, Any],
        stage: Optional[str] = None,
        subtask: Optional[str] = None,
        evaluation_tier: Optional[str] = None,
    ) -> None: ...


class _WorkbenchProgressAdapter:
    def __init__(self, worker: _WorkbenchEventEmitter) -> None:
        self._worker: _WorkbenchEventEmitter = worker

    def update(self, *, detail: Optional[str] = None) -> None:
        if detail is None:
            return

        self._worker._emit_event(
            WorkbenchEventType.SUBTASK_STARTED,
            payload={"detail": detail},
            subtask=detail,
        )

    def __exit__(self, exc_type, exc_value, traceback) -> None:
        _ = exc_type
        _ = exc_value
        _ = traceback

        return None


class WorkbenchExecutionWorker(QObject):
    event_emitted = pyqtSignal(object)
    execution_completed = pyqtSignal(object)
    error_raised = pyqtSignal(str)
    finished = pyqtSignal()

    def __init__(self, run_setup: WorkbenchRunSetup) -> None:
        super().__init__()

        self._run_setup: WorkbenchRunSetup = run_setup
        self._event_sequence_number: int = 0
        self._run_id: str = run_setup.run_id or "workbench_pending"
        self._heartbeat_stop: Event = Event()

    def run(self) -> None:
        from janiform.calibration.fit.estimator import _PreparedHighLevelFitWorkflow
        prepared_workflow: Optional[_PreparedHighLevelFitWorkflow] = None

        try:
            estimator = JaniformEstimator(
                workflow_scope=self._run_setup.workflow_scope,
                tick_data_column_mapping=_column_mapping_from_setup(self._run_setup),
                normalization_policy=_normalization_policy_from_setup(self._run_setup),
                exchange_code=self._run_setup.exchange_code,
                silent=True,
            )

            initial_inference = infer_high_level_fit_inputs(
                tick_data_path=self._run_setup.dataset_path,
                tick_data_format=self._run_setup.file_format,
                tick_data_provenance_label="tick_data",
                workflow_scope=self._run_setup.workflow_scope,
                run_id=self._run_setup.run_id,
                column_overrides=_column_overrides_from_setup(self._run_setup),
                normalization_overrides=_normalization_overrides_from_setup(self._run_setup),
                market_profile_configuration=estimator._resolve_market_profile_configuration(),
            )

            self._run_id = initial_inference.run_id

            self._emit_event(
                WorkbenchEventType.RUN_STARTED,
                payload={
                    "execution_mode": "memory",
                    "dataset_path": str(self._run_setup.dataset_path),
                    "output_directory": str(self._run_setup.output_directory),
                    "workflow_scope": self._run_setup.workflow_scope.value,
                    "detail": "Starting calibration workflow",
                    "inference_summary": build_fit_inference_payload(
                        inference=initial_inference,
                        workflow_scope=self._run_setup.workflow_scope,
                        market_profile_name=None,
                        known_news_preset_name=None,
                        known_news_recommended_name=None,
                        known_news_recommendation_applied=False,
                        session_source_kind="automatic",
                        known_news_source_kind="none",
                    ),
                },
            )

            progress = _WorkbenchProgressAdapter(self)

            prepared_workflow = estimator._prepare_fit_workflow(
                run_id=self._run_id,
                tick_data_path=self._run_setup.dataset_path,
                tick_data_format=self._run_setup.file_format,
                tick_data_provenance_label="tick_data",
                option_surface_path=None,
                option_surface_format=self._run_setup.file_format or initial_inference.tick_data_source.file_format,
                option_surface_provenance_label="option_surface",
                progress=progress,
            )

            if prepared_workflow.fit_warning is not None:
                self._emit_event(
                    WorkbenchEventType.ALERT_EMITTED,
                    payload={
                        "severity": "warning",
                        "alert_code": "fit_warning",
                        "message": prepared_workflow.fit_warning,
                    },
                )

            self._emit_event(
                WorkbenchEventType.SUBTASK_STARTED,
                payload={"detail": "Running staged calibration"},
                subtask="staged_calibration",
            )

            heartbeat_thread: Optional[Thread] = self._start_heartbeat()

            staged_response, raw_response_payload = (
                _run_staged_calibration_with_optional_events(
                    prepared_workflow.prepared.execution_input,
                    calendar_registry=CalendarRegistry(
                        prepared_workflow.prepared.session_resolution.resolved_session_definition_json
                    ),
                    event_callback=self._forward_native_event,
                )
            )

            self._heartbeat_stop.set()

            if heartbeat_thread.is_alive():
                heartbeat_thread.join(timeout=1.0)

            fit_report = JaniformFitReport(
                run_id=prepared_workflow.run_id,
                workflow_scope=self._run_setup.workflow_scope,
                success=staged_response.status == CalibrationStageExecutionStatus.SUCCESS,
                resolved_configuration=prepared_workflow.resolved_configuration,
                session_resolution=prepared_workflow.prepared.session_resolution,
                preparation_diagnostics=prepared_workflow.prepared.diagnostics,
                preparation_provenance=prepared_workflow.prepared.provenance,
                execution_input=prepared_workflow.prepared.execution_input,
                staged_response=staged_response,
                stage_report_payloads=_raw_stage_report_payloads(raw_response_payload),
            )

            if staged_response.status != CalibrationStageExecutionStatus.SUCCESS:
                failure_kind = (
                    staged_response.failure.failure_kind
                    if staged_response.failure is not None
                    else "fit_failed"
                )

                raise RuntimeError(
                    f"Janiform fit failed for run_id `{prepared_workflow.run_id}` with failure_kind `{failure_kind}`"
                )

            model = JaniformModel(
                run_id=prepared_workflow.run_id,
                workflow_scope=self._run_setup.workflow_scope,
                parameters=_extract_model_parameters(fit_report.stage_report_payloads),
                metadata=_build_model_metadata(
                    resolved_configuration=prepared_workflow.resolved_configuration,
                    prepared=prepared_workflow.prepared,
                    staged_response=staged_response,
                    stage_report_payloads=fit_report.stage_report_payloads,
                    input_inference=build_fit_inference_payload(
                        inference=prepared_workflow.input_inference,
                        workflow_scope=self._run_setup.workflow_scope,
                        market_profile_name=(
                            prepared_workflow.resolved_configuration.market_profile.selected_preset.preset_name
                            if (
                                prepared_workflow.resolved_configuration.market_profile is not None
                                and prepared_workflow.resolved_configuration.market_profile.selected_preset is not None
                            )
                            else None
                        ),
                        known_news_preset_name=(
                            prepared_workflow.resolved_configuration.known_news.selected_preset.preset_name
                            if prepared_workflow.resolved_configuration.known_news.selected_preset is not None
                            else None
                        ),
                        known_news_recommended_name=(
                            prepared_workflow.resolved_configuration.known_news.recommended_reference.preset_name
                            if prepared_workflow.resolved_configuration.known_news.recommended_reference is not None
                            else None
                        ),
                        known_news_recommendation_applied=(
                            prepared_workflow.resolved_configuration.known_news.recommendation_applied
                        ),
                        session_source_kind=prepared_workflow.resolved_configuration.session.source_kind.value,
                        known_news_source_kind=prepared_workflow.resolved_configuration.known_news.source_kind.value,
                    ),
                    reference_price=prepared_workflow.reference_price,
                ),
                resolved_configuration=prepared_workflow.resolved_configuration,
                session_resolution=prepared_workflow.prepared.session_resolution,
                _execution_input=prepared_workflow.prepared.execution_input,
                _stage_report_payloads=fit_report.stage_report_payloads,
            )

            artifact_paths = write_fit_artifacts(
                model=model,
                fit_report=fit_report,
                output_dir=self._run_setup.output_directory,
            )

            self._emit_event(
                WorkbenchEventType.ARTIFACT_RECORDED,
                payload={
                    "artifact_kind": "fit_run_directory",
                    "artifact_path": str(artifact_paths.run_directory),
                    "artifact_label": "Fit artifacts written",
                },
            )

            artifacts = load_workbench_artifacts(artifact_paths.run_directory)

            self.execution_completed.emit(
                WorkbenchExecutionResult(
                    run_directory=artifact_paths.run_directory,
                    model=model,
                    fit_report=fit_report,
                    artifacts=artifacts,
                )
            )

            self._emit_event(
                WorkbenchEventType.RUN_COMPLETED,
                payload={"detail": "Calibration completed"},
            )
        except Exception as error:
            self._heartbeat_stop.set()

            self._emit_event(
                WorkbenchEventType.RUN_FAILED,
                payload={"message": str(error)},
            )

            self.error_raised.emit(str(error))
        finally:
            if prepared_workflow is not None and prepared_workflow.temporary_directory is not None:
                prepared_workflow.temporary_directory.cleanup()

            self.finished.emit()

    def _start_heartbeat(self) -> Thread:
        started_at = monotonic()
        self._heartbeat_stop.clear()

        def emit_heartbeat() -> None:
            while not self._heartbeat_stop.is_set():
                self._emit_event(
                    WorkbenchEventType.RESOURCE_SNAPSHOT_RECORDED,
                    payload={
                        "elapsed_seconds": monotonic() - started_at,
                        "worker_count": 1,
                    },
                )

                sleep(1.0)

        thread = Thread(target=emit_heartbeat, daemon=True)
        thread.start()

        return thread

    def _emit_event(
        self,
        event_type: WorkbenchEventType,
        *,
        payload: dict[str, Any],
        stage: Optional[str] = None,
        subtask: Optional[str] = None,
        evaluation_tier: Optional[str] = None,
    ) -> None:
        self._event_sequence_number += 1

        self.event_emitted.emit(
            build_workbench_event(
                event_type=event_type,
                run_id=self._run_id,
                sequence_number=self._event_sequence_number,
                payload=payload,
                stage=stage,
                subtask=subtask,
                evaluation_tier=evaluation_tier,
            )
        )

    def _forward_native_event(self, raw_event: Any) -> None:
        if not isinstance(raw_event, dict):
            return

        event_kind = raw_event.get("event_kind")
        if not isinstance(event_kind, str):
            return

        match event_kind:
            case "run_started":
                self._emit_event(
                    WorkbenchEventType.RUN_STARTED,
                    payload={
                        "detail": "Running staged calibration",
                        "stage_count": raw_event.get("stage_count"),
                        "execution_mode": raw_event.get("execution_mode"),
                    },
                )
            case "stage_started":
                stage = _coerce_optional_string(raw_event.get("stage"))

                self._emit_event(
                    WorkbenchEventType.STAGE_STARTED,
                    payload={
                        "stage_index": raw_event.get("stage_index"),
                        "stage_count": raw_event.get("stage_count"),
                        "stage_label": stage or "stage",
                        "detail": f"Running {stage or 'stage'}",
                    },
                    stage=stage,
                    evaluation_tier=_evaluation_tier_for_stage(stage),
                )
            case "stage_completed":
                stage = _coerce_optional_string(raw_event.get("stage"))
                stage_report = _coerce_optional_payload_dict(raw_event.get("stage_report"))

                self._emit_event(
                    WorkbenchEventType.STAGE_COMPLETED,
                    payload={
                        "stage_index": raw_event.get("stage_index"),
                        "stage_count": raw_event.get("stage_count"),
                        "stage_label": stage or "stage",
                        "detail": _detail_for_stage_completed_event(stage, stage_report),
                        "runtime_seconds": raw_event.get("runtime_seconds"),
                        "stage_report": stage_report,
                    },
                    stage=stage,
                    evaluation_tier=_evaluation_tier_for_stage(stage),
                )
            case "run_failed":
                failure_payload = raw_event.get("failure")

                self._emit_event(
                    WorkbenchEventType.RUN_FAILED,
                    payload={
                        "message": _detail_for_failed_native_run(failure_payload),
                        "failure": failure_payload,
                    },
                )
            case "run_completed":
                self._emit_event(
                    WorkbenchEventType.SUBTASK_COMPLETED,
                    payload={"detail": "Native calibration completed, writing artifacts"},
                    subtask="artifact_write",
                )
            case _:
                return


def _column_mapping_from_setup(
    run_setup: WorkbenchRunSetup,
) -> Optional[CalibrationTickDataColumnMapping]:
    if (
        run_setup.timestamp_column is None
        and run_setup.price_column is None
        and run_setup.bid_column is None
        and run_setup.ask_column is None
    ):
        return None

    return CalibrationTickDataColumnMapping(
        timestamp_column=run_setup.timestamp_column or "timestamp",
        price_column=run_setup.price_column,
        bid_column=run_setup.bid_column,
        ask_column=run_setup.ask_column,
    )


def _normalization_policy_from_setup(
    run_setup: WorkbenchRunSetup,
) -> Optional[CalibrationMarketDataNormalizationPolicy]:
    if run_setup.tick_price_input_policy is None and run_setup.timestamp_timezone == "UTC":
        return None

    return CalibrationMarketDataNormalizationPolicy(
        timestamp_timezone=run_setup.timestamp_timezone,
        sort_records_by_timestamp=True,
        tick_price_input_policy=run_setup.tick_price_input_policy,
    )


def _column_overrides_from_setup(run_setup: WorkbenchRunSetup) -> HighLevelTickDataColumnOverrides:
    return HighLevelTickDataColumnOverrides(
        timestamp_column=run_setup.timestamp_column,
        price_column=run_setup.price_column,
        bid_column=run_setup.bid_column,
        ask_column=run_setup.ask_column,
    )


def _normalization_overrides_from_setup(
    run_setup: WorkbenchRunSetup,
) -> HighLevelNormalizationPolicyOverrides:
    return HighLevelNormalizationPolicyOverrides(
        timestamp_timezone=run_setup.timestamp_timezone,
        sort_records_by_timestamp=True,
        tick_price_input_policy=run_setup.tick_price_input_policy,
    )


def _coerce_optional_string(value: Any) -> Optional[str]:
    if isinstance(value, str):
        return value

    return None


def _coerce_optional_payload_dict(value: Any) -> Optional[dict[str, Any]]:
    if isinstance(value, dict):
        return value

    return None


def _evaluation_tier_for_stage(stage: Optional[str]) -> Optional[str]:
    match stage:
        case "hawkes" | "return_components":
            return "exact_block"
        case "joint_refinement":
            return "tick_joint"
        case _:
            return None


def _detail_for_stage_completed_event(
    stage: Optional[str],
    stage_report: Optional[dict[str, Any]],
) -> str:
    if stage_report is not None:
        status = _coerce_optional_string(stage_report.get("status"))
        if status is not None and stage is not None:
            return f"{stage} {status}"
        if status is not None:
            return status

    return f"{stage or 'stage'} completed"


def _detail_for_failed_native_run(failure_payload: Any) -> str:
    if isinstance(failure_payload, dict):
        failure_kind = _coerce_optional_string(failure_payload.get("failure_kind"))
        if failure_kind is not None:
            return f"Calibration failed with {failure_kind}"

    return "Calibration failed"


__all__ = [
    "WorkbenchExecutionResult",
    "WorkbenchExecutionWorker",
]
