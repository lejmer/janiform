from __future__ import annotations

from copy import deepcopy
from dataclasses import dataclass
from datetime import UTC, datetime
from threading import Event
from typing import Any, Optional
from uuid import uuid4

from PyQt6.QtCore import QObject, pyqtSignal

from janiform.calibration.fit import JaniformModel

from .artifacts import WorkbenchArtifacts
from .fit_context import (
    WorkbenchObservedSimulationComparison,
    build_observed_simulation_comparison_for_fit_context,
    load_fit_context,
)
from .parameters import apply_parameter_updates


@dataclass(frozen=True, slots=True)
class WorkbenchReevaluationRequest:
    run_id: str
    source_candidate_id: str
    edited_parameters: dict[str, Any]
    edited_parameter_updates: dict[str, Any]
    tiers: tuple[str, ...]
    seed: int


@dataclass(frozen=True, slots=True)
class WorkbenchReevaluationResult:
    reevaluation_id: str
    source_candidate_id: str
    edited_model: JaniformModel
    comparison: Optional[WorkbenchObservedSimulationComparison]
    exact_status: str
    status: str
    completed_at_utc: str
    diagnostics_summary: dict[str, Any]


class WorkbenchReevaluationWorker(QObject):
    status_changed = pyqtSignal(str)
    completed = pyqtSignal(object)
    error_raised = pyqtSignal(str)
    finished = pyqtSignal()

    def __init__(
        self,
        artifacts: WorkbenchArtifacts,
        request: WorkbenchReevaluationRequest,
    ) -> None:
        super().__init__()
        self._artifacts: WorkbenchArtifacts = artifacts
        self._request: WorkbenchReevaluationRequest = request
        self._cancel_requested: Event = Event()

    def run(self) -> None:
        try:
            fit_context = load_fit_context(self._artifacts)
            if fit_context is None:
                raise RuntimeError("The current run does not contain enough fit context for reevaluation.")
            if self._cancel_requested.is_set():
                return

            self.status_changed.emit("Cloning fitted model")

            edited_model = _clone_model_with_updated_parameters(
                fit_context.model,
                updated_parameters=self._request.edited_parameters,
            )
            if self._cancel_requested.is_set():
                return

            comparison: Optional[WorkbenchObservedSimulationComparison] = None
            if any(tier in self._request.tiers for tier in ("seeded_tick", "coarse_diagnostics", "consistency")):
                self.status_changed.emit("Running simulation-backed reevaluation")

                comparison = build_observed_simulation_comparison_for_fit_context(
                    fit_context,
                    model=edited_model,
                    seed=self._request.seed,
                )

                if self._cancel_requested.is_set():
                    return

            exact_status = _exact_reevaluation_status(
                artifacts=self._artifacts,
                source_candidate_id=self._request.source_candidate_id,
                tiers=self._request.tiers,
            )

            result = WorkbenchReevaluationResult(
                reevaluation_id=f"reeval_{uuid4().hex}",
                source_candidate_id=self._request.source_candidate_id,
                edited_model=edited_model,
                comparison=comparison,
                exact_status=exact_status,
                status="completed",
                completed_at_utc=datetime.now(tz=UTC).isoformat(),
                diagnostics_summary=_diagnostics_summary(
                    comparison=comparison,
                    exact_status=exact_status,
                    updated_parameter_count=len(self._request.edited_parameter_updates),
                ),
            )

            self.completed.emit(result)
        except Exception as error:
            if not self._cancel_requested.is_set():
                self.error_raised.emit(str(error))
        finally:
            self.finished.emit()

    def cancel(self) -> None:
        self._cancel_requested.set()


def apply_reevaluation_request(
    source_model: JaniformModel,
    parameter_updates: dict[str, Any],
) -> dict[str, Any]:
    return apply_parameter_updates(source_model.fitted_parameters, parameter_updates)


def _clone_model_with_updated_parameters(
    source_model: JaniformModel,
    *,
    updated_parameters: dict[str, Any],
) -> JaniformModel:
    metadata = deepcopy(source_model.metadata)
    metadata["lab_last_edit_summary"] = {
        "updated_at_utc": datetime.now(tz=UTC).isoformat(),
    }

    return JaniformModel(
        run_id=f"{source_model.run_id}_lab",
        workflow_scope=source_model.workflow_scope,
        parameters=updated_parameters,
        metadata=metadata,
        resolved_configuration=source_model.resolved_configuration,
        session_resolution=source_model.session_resolution,
        _stage_report_payloads=source_model._stage_report_payloads,
    )


def _exact_reevaluation_status(
    *,
    artifacts: WorkbenchArtifacts,
    source_candidate_id: str,
    tiers: tuple[str, ...],
) -> str:
    if "exact_rescoring" not in tiers:
        return "not_requested"

    staged_response = {} if artifacts.fit_report_payload is None else artifacts.fit_report_payload.get("staged_response", {})
    if isinstance(staged_response, dict) and isinstance(staged_response.get("stage_reports"), list):
        for stage_report in staged_response["stage_reports"]:
            if not isinstance(stage_report, dict):
                continue

            fitted_stage_payload = stage_report.get("fitted_stage_payload")
            if not isinstance(fitted_stage_payload, dict):
                continue

            if fitted_stage_payload.get("selected_candidate_id") == source_candidate_id:
                return "available_but_not_yet_native_for_this_stage"

    return "unavailable_from_persisted_run"


def _diagnostics_summary(
    *,
    comparison: Optional[WorkbenchObservedSimulationComparison],
    exact_status: str,
    updated_parameter_count: int,
) -> dict[str, Any]:
    diagnostics_summary: dict[str, Any] = {
        "exact_status": exact_status,
        "updated_parameter_count": updated_parameter_count,
    }
    if comparison is None:
        return diagnostics_summary

    diagnostics_summary["observed_row_count"] = int(comparison.observed_series.prices.size)
    diagnostics_summary["simulated_row_count"] = int(comparison.simulated_series.prices.size)
    diagnostics_summary["coarse_row_count"] = int(
        comparison.simulated_coarse_series_15m.prices.size
    )
    diagnostics_summary["simulation_seed"] = comparison.simulation_seed

    return diagnostics_summary


__all__ = [
    "WorkbenchReevaluationRequest",
    "WorkbenchReevaluationResult",
    "WorkbenchReevaluationWorker",
    "apply_reevaluation_request",
]
