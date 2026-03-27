from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Optional

from janiform.calibration.market_data.contracts import (
    CalibrationInputFileFormat,
    CalibrationMarketDataWorkflowScope,
    CalibrationTickPriceInputPolicy,
)


@dataclass(frozen=True, slots=True)
class WorkbenchProgressModel:
    label: str
    completed_units: Optional[int] = None
    total_units: Optional[int] = None
    detail: str = ""
    is_indeterminate: bool = True


@dataclass(frozen=True, slots=True)
class WorkbenchRunSetup:
    dataset_path: Path
    exchange_code: str
    workflow_scope: CalibrationMarketDataWorkflowScope
    output_directory: Path
    mean_reversion_enabled: bool = True
    file_format: Optional[CalibrationInputFileFormat] = None
    timestamp_column: Optional[str] = None
    tick_price_input_policy: Optional[CalibrationTickPriceInputPolicy] = None
    price_column: Optional[str] = None
    bid_column: Optional[str] = None
    ask_column: Optional[str] = None
    timestamp_timezone: str = "UTC"
    run_id: Optional[str] = None


@dataclass(slots=True)
class WorkbenchSessionState:
    workspace_root: Path
    opened_input_path: Optional[Path] = None
    opened_run_id: Optional[str] = None
    mode: str = "setup"
    status_message: str = ""
    last_error_message: Optional[str] = None


@dataclass(slots=True)
class WorkbenchSetupState:
    run_setup: Optional[WorkbenchRunSetup] = None
    available_columns: tuple[str, ...] = ()
    inference_summary: dict[str, Any] = field(default_factory=dict)
    warnings: tuple[str, ...] = ()
    is_valid: bool = False


@dataclass(slots=True)
class WorkbenchRunState:
    run_id: Optional[str] = None
    status: str = "idle"
    started_at_utc: Optional[str] = None
    finished_at_utc: Optional[str] = None
    elapsed_seconds: float = 0.0
    estimated_remaining_seconds: Optional[float] = None
    current_stage: Optional[str] = None
    current_subtask: Optional[str] = None
    current_evaluation_tier: Optional[str] = None
    current_candidate_family: Optional[str] = None
    current_batch_label: Optional[str] = None
    overall_progress: WorkbenchProgressModel = field(
        default_factory=lambda: WorkbenchProgressModel(label="Run")
    )
    stage_progress: WorkbenchProgressModel = field(
        default_factory=lambda: WorkbenchProgressModel(label="Stage")
    )
    subprogress: WorkbenchProgressModel = field(
        default_factory=lambda: WorkbenchProgressModel(label="Subtask")
    )
    recent_alerts: list[str] = field(default_factory=list)
    resource_snapshot: dict[str, Any] = field(default_factory=dict)


@dataclass(slots=True)
class WorkbenchCandidateState:
    best_candidate_id: Optional[str] = None
    selected_candidate_id: Optional[str] = None
    candidate_count: int = 0


@dataclass(slots=True)
class WorkbenchPlotState:
    selected_plot_family: Optional[str] = None
    live_update_enabled: bool = True


@dataclass(slots=True)
class WorkbenchCompareState:
    left_candidate_id: Optional[str] = None
    right_candidate_id: Optional[str] = None
    compare_mode: str = "candidate_vs_candidate"


@dataclass(slots=True)
class WorkbenchLabState:
    source_candidate_id: Optional[str] = None
    reevaluation_status: str = "idle"


@dataclass(slots=True)
class WorkbenchExportState:
    last_export_directory: Optional[Path] = None
    last_export_manifest: dict[str, Any] = field(default_factory=dict)


@dataclass(slots=True)
class WorkbenchAppState:
    session: WorkbenchSessionState
    setup: WorkbenchSetupState
    run: WorkbenchRunState = field(default_factory=WorkbenchRunState)
    candidates: WorkbenchCandidateState = field(default_factory=WorkbenchCandidateState)
    plots: WorkbenchPlotState = field(default_factory=WorkbenchPlotState)
    compare: WorkbenchCompareState = field(default_factory=WorkbenchCompareState)
    lab: WorkbenchLabState = field(default_factory=WorkbenchLabState)
    exports: WorkbenchExportState = field(default_factory=WorkbenchExportState)


def build_initial_workbench_state(*, workspace_root: Path) -> WorkbenchAppState:
    return WorkbenchAppState(
        session=WorkbenchSessionState(workspace_root=workspace_root),
        setup=WorkbenchSetupState(),
    )


__all__ = [
    "WorkbenchAppState",
    "WorkbenchCandidateState",
    "WorkbenchCompareState",
    "WorkbenchExportState",
    "WorkbenchLabState",
    "WorkbenchPlotState",
    "WorkbenchProgressModel",
    "WorkbenchRunSetup",
    "WorkbenchRunState",
    "WorkbenchSessionState",
    "WorkbenchSetupState",
    "build_initial_workbench_state",
]
