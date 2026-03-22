from dataclasses import dataclass
from pathlib import Path
from threading import Event, Thread
from time import monotonic, sleep
from typing import Any

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
from janiform.calibration.staged.runtime import _run_staged_calibration_with_raw_payload
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


class _WorkbenchProgressAdapter:
    def __init__(self, worker: "WorkbenchExecutionWorker") -> None:
        self._worker = worker

    def update(self, *, detail: str | None = None) -> None:
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
        self._run_setup = run_setup
        self._event_sequence_number = 0
        self._run_id = run_setup.run_id or "workbench_pending"
        self._heartbeat_stop = Event()

    def run(self) -> None:
        heartbeat_thread: Thread | None = None
        prepared_workflow: Any = None

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
                WorkbenchEventType.STAGE_STARTED,
                payload={
                    "stage_index": 1,
                    "stage_count": 1,
                    "stage_label": "staged_calibration",
                    "detail": "Running staged calibration",
                },
                stage="staged_calibration",
            )
            heartbeat_thread = self._start_heartbeat()
            staged_response, raw_response_payload = _run_staged_calibration_with_raw_payload(
                prepared_workflow.prepared.execution_input,
                calendar_registry=CalendarRegistry(
                    prepared_workflow.prepared.session_resolution.resolved_session_definition_json
                ),
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
            for stage_index, stage_report in enumerate(staged_response.stage_reports, start=1):
                self._emit_event(
                    WorkbenchEventType.STAGE_COMPLETED,
                    payload={
                        "stage_index": stage_index,
                        "stage_count": len(staged_response.stage_reports),
                        "stage_label": stage_report.stage.value,
                        "detail": f"{stage_report.stage.value} {stage_report.status.value}",
                    },
                    stage=stage_report.stage.value,
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
        stage: str | None = None,
        subtask: str | None = None,
        evaluation_tier: str | None = None,
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


def _column_mapping_from_setup(run_setup: WorkbenchRunSetup) -> CalibrationTickDataColumnMapping | None:
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
) -> CalibrationMarketDataNormalizationPolicy | None:
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


__all__ = [
    "WorkbenchExecutionResult",
    "WorkbenchExecutionWorker",
]
