from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Any, Optional

from ...contracts import serialize_pretty_json
from ..market_data.contracts import CalibrationMarketDataWorkflowScope
from ..market_data.workflow import (
    MarketDataCalibrationPreparationDiagnostics,
    MarketDataCalibrationPreparationProvenance,
)
from ..presets.resolution import ResolvedPresetWorkflowConfiguration
from ..sessions.contracts import MarketDataSessionResolution
from ..staged.request import StagedCalibrationRequest
from ..staged.results import StagedCalibrationResponse


@dataclass(frozen=True, slots=True)
class JaniformFitReport:
    run_id: str
    workflow_scope: CalibrationMarketDataWorkflowScope
    success: bool
    resolved_configuration: ResolvedPresetWorkflowConfiguration
    session_resolution: MarketDataSessionResolution
    preparation_diagnostics: MarketDataCalibrationPreparationDiagnostics
    preparation_provenance: MarketDataCalibrationPreparationProvenance
    staged_request: StagedCalibrationRequest
    prepared_data_reference: Optional[str]
    prepared_payload_path: Optional[str]
    staged_execution_input_path: Optional[str]
    staged_response: StagedCalibrationResponse
    stage_report_payloads: tuple[dict[str, Any], ...]

    def to_payload(self) -> dict[str, Any]:
        return {
            "schema_version": "janiform.fit_report.v1",
            "run_id": self.run_id,
            "workflow_scope": self.workflow_scope.value,
            "success": self.success,
            "summary": self.summary(),
            "resolved_configuration": self.resolved_configuration.to_payload(),
            "session_resolution": self.session_resolution.to_payload(),
            "preparation_diagnostics": self.preparation_diagnostics.to_payload(),
            "preparation_provenance": self.preparation_provenance.to_payload(),
            "prepared_data_reference": self.prepared_data_reference,
            "prepared_payload_path": self.prepared_payload_path,
            "staged_execution_input_path": self.staged_execution_input_path,
            "staged_response": self.staged_response.to_payload(),
        }

    def save(self, path: str | Path) -> None:
        Path(path).write_text(serialize_pretty_json(self.to_payload()), encoding="utf-8")

    def summary(self) -> dict[str, Any]:
        return {
            "run_id": self.run_id,
            "workflow_scope": self.workflow_scope.value,
            "success": self.success,
            "selected_stages": [stage.value for stage in self.staged_request.selected_stages],
            "stage_statuses": [
                {
                    "stage": report.stage.value,
                    "status": report.status.value,
                }
                for report in self.staged_response.stage_reports
            ],
            "diagnostic_count": len(self.resolved_configuration.diagnostics),
        }


__all__ = ["JaniformFitReport"]
