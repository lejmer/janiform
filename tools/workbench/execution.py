from dataclasses import dataclass
from pathlib import Path
from threading import Event, Thread
from time import monotonic, sleep
from typing import Any, Optional, Protocol

from PyQt6.QtCore import QObject, pyqtSignal

from janiform._workflow_outputs import resolve_fit_artifact_paths
from janiform.calibration.fit._inference import (
    NormalizationPolicyOverrides,
    TickDataColumnOverrides,
    build_fit_inference_payload,
    infer_fit_inputs,
)
from janiform.calibration.fit.estimator import JaniformEstimator
from janiform.calibration.fit.model import JaniformModel
from janiform.calibration.fit.report import JaniformFitReport
from janiform.calibration.market_data.contracts import (
    CalibrationMeanReversionConfiguration,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataWorkflowScope,
    CalibrationTickDataColumnMapping,
)

from .artifacts import load_workbench_artifacts
from .events import WorkbenchEventType, build_workbench_event
from .state import DEFAULT_WORKBENCH_SESSION_PRESET_NAME, WorkbenchRunSetup

RESOURCE_SNAPSHOT_INTERVAL_SECONDS: float = 15.0


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


class WorkbenchExecutionWorker(QObject):
    event_emitted = pyqtSignal(object)
    execution_completed = pyqtSignal(object)
    error_raised = pyqtSignal(str)
    finished = pyqtSignal()

    _run_setup: WorkbenchRunSetup
    _event_sequence_number: int
    _run_id: str
    _heartbeat_stop: Event

    def __init__(self, run_setup: WorkbenchRunSetup) -> None:
        super().__init__()

        self._run_setup: WorkbenchRunSetup = run_setup
        self._event_sequence_number: int = 0
        self._run_id: str = run_setup.run_id or "workbench_pending"
        self._heartbeat_stop: Event = Event()

    def run(self) -> None:
        heartbeat_thread: Optional[Thread] = None

        try:
            session_argument: Optional[str | Path] = None
            exchange_code: Optional[str] = None

            match self._run_setup.session_source.mode:
                case "preset":
                    session_argument = (
                        self._run_setup.session_source.preset_name
                        or DEFAULT_WORKBENCH_SESSION_PRESET_NAME
                    )
                case "definition":
                    session_argument = self._run_setup.session_source.definition_path
                case "probe_fallback":
                    exchange_code = self._run_setup.session_source.exchange_code
                case _:
                    raise RuntimeError(
                        f"unsupported workbench session source mode `{self._run_setup.session_source.mode}`"
                    )

            estimator = JaniformEstimator(
                workflow_scope=self._run_setup.workflow_scope,
                tick_data_column_mapping=_column_mapping_from_setup(self._run_setup),
                normalization_policy=_normalization_policy_from_setup(self._run_setup),
                mean_reversion=_mean_reversion_configuration_from_setup(self._run_setup),
                session=session_argument,
                exchange_code=exchange_code,
                silent=True,
            )

            initial_inference = infer_fit_inputs(
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
                        session_source_kind=_session_source_kind_from_setup(self._run_setup),
                        known_news_source_kind="none",
                    ),
                },
            )

            self._emit_event(
                WorkbenchEventType.SUBTASK_STARTED,
                payload={"detail": "Running fit workflow"},
                subtask="fit_workflow",
            )

            heartbeat_thread = self._start_heartbeat()
            fit_result = estimator.fit_with_report(
                run_id=self._run_id,
                tick_data_path=self._run_setup.dataset_path,
                tick_data_format=self._run_setup.file_format,
                output_dir=self._run_setup.output_directory,
            )

            self._stop_heartbeat(heartbeat_thread)

            model = fit_result.require_model()
            fit_report = fit_result.report
            artifact_paths = resolve_fit_artifact_paths(
                run_id=fit_report.run_id,
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
            self._stop_heartbeat(heartbeat_thread)

            self._emit_event(
                WorkbenchEventType.RUN_FAILED,
                payload={"message": str(error)},
            )

            self.error_raised.emit(str(error))
        finally:
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

                sleep(RESOURCE_SNAPSHOT_INTERVAL_SECONDS)

        thread = Thread(target=emit_heartbeat, daemon=True)
        thread.start()

        return thread

    def _stop_heartbeat(self, heartbeat_thread: Optional[Thread]) -> None:
        self._heartbeat_stop.set()

        if heartbeat_thread is not None and heartbeat_thread.is_alive():
            heartbeat_thread.join(timeout=1.0)

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


def _column_overrides_from_setup(run_setup: WorkbenchRunSetup) -> TickDataColumnOverrides:
    return TickDataColumnOverrides(
        timestamp_column=run_setup.timestamp_column,
        price_column=run_setup.price_column,
        bid_column=run_setup.bid_column,
        ask_column=run_setup.ask_column,
    )


def _normalization_overrides_from_setup(
    run_setup: WorkbenchRunSetup,
) -> NormalizationPolicyOverrides:
    return NormalizationPolicyOverrides(
        timestamp_timezone=run_setup.timestamp_timezone,
        sort_records_by_timestamp=True,
        tick_price_input_policy=run_setup.tick_price_input_policy,
    )


def _mean_reversion_configuration_from_setup(
    run_setup: WorkbenchRunSetup,
) -> Optional[CalibrationMeanReversionConfiguration]:
    if run_setup.workflow_scope == CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY:
        return None

    return CalibrationMeanReversionConfiguration()


def _session_source_kind_from_setup(run_setup: WorkbenchRunSetup) -> str:
    match run_setup.session_source.mode:
        case "preset":
            return "named"
        case "definition":
            return "explicit"
        case "probe_fallback":
            return "automatic"
        case _:
            return "automatic"


__all__ = [
    "WorkbenchExecutionResult",
    "WorkbenchExecutionWorker",
]
