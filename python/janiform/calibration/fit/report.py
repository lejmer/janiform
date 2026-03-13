from dataclasses import dataclass
from typing import Any

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
