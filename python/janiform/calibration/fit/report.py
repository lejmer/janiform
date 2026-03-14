from dataclasses import dataclass
from pathlib import Path
from typing import Any

from ..._contracts import serialize_pretty_json
from ..market_data.contracts import CalibrationMarketDataWorkflowScope
from ..market_data.workflow import (
    MarketDataCalibrationPreparationDiagnostics,
    MarketDataCalibrationPreparationProvenance,
)
from ..presets.resolution import ResolvedPresetWorkflowConfiguration
from ..sessions.contracts import MarketDataSessionResolution
from ..staged.inputs import StagedCalibrationExecutionInput
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
    execution_input: StagedCalibrationExecutionInput
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
            "staged_response": self.staged_response.to_payload(),
        }

    def save(self, path: str | Path) -> None:
        Path(path).write_text(serialize_pretty_json(self.to_payload()), encoding="utf-8")

    def summary(self) -> dict[str, Any]:
        return {
            "run_id": self.run_id,
            "workflow_scope": self.workflow_scope.value,
            "success": self.success,
            "selected_stages": [
                stage.value for stage in self.execution_input.request.selected_stages
            ],
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
