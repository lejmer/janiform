from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from janiform.calibration.market_data.contracts import (
    CalibrationInputFileFormat,
    CalibrationMarketDataWorkflowScope,
    CalibrationTickPriceInputPolicy,
)


@dataclass(frozen=True, slots=True)
class WorkbenchProgressModel:
    label: str
    completed_units: int | None = None
    total_units: int | None = None
    detail: str = ""
    is_indeterminate: bool = True


@dataclass(frozen=True, slots=True)
class WorkbenchRunSetup:
    dataset_path: Path
    exchange_code: str
    workflow_scope: CalibrationMarketDataWorkflowScope
    output_directory: Path
    file_format: CalibrationInputFileFormat | None = None
    timestamp_column: str | None = None
    tick_price_input_policy: CalibrationTickPriceInputPolicy | None = None
    price_column: str | None = None
    bid_column: str | None = None
    ask_column: str | None = None
    timestamp_timezone: str = "UTC"
    run_id: str | None = None


@dataclass(slots=True)
class WorkbenchSessionState:
    workspace_root: Path
    opened_input_path: Path | None = None
    opened_run_id: str | None = None
    mode: str = "setup"
    status_message: str = ""
    last_error_message: str | None = None


@dataclass(slots=True)
class WorkbenchSetupState:
    run_setup: WorkbenchRunSetup | None = None
    available_columns: tuple[str, ...] = ()
    inference_summary: dict[str, Any] = field(default_factory=dict)
    warnings: tuple[str, ...] = ()
    is_valid: bool = False


@dataclass(slots=True)
class WorkbenchRunState:
    run_id: str | None = None
    status: str = "idle"
    started_at_utc: str | None = None
    finished_at_utc: str | None = None
    elapsed_seconds: float = 0.0
    estimated_remaining_seconds: float | None = None
    current_stage: str | None = None
    current_subtask: str | None = None
    current_evaluation_tier: str | None = None
    current_candidate_family: str | None = None
    current_batch_label: str | None = None
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
    best_candidate_id: str | None = None
    selected_candidate_id: str | None = None
    candidate_count: int = 0


@dataclass(slots=True)
class WorkbenchPlotState:
    selected_plot_family: str | None = None
    live_update_enabled: bool = True


@dataclass(slots=True)
class WorkbenchCompareState:
    left_candidate_id: str | None = None
    right_candidate_id: str | None = None
    compare_mode: str = "candidate_vs_candidate"


@dataclass(slots=True)
class WorkbenchLabState:
    source_candidate_id: str | None = None
    reevaluation_status: str = "idle"


@dataclass(slots=True)
class WorkbenchExportState:
    last_export_directory: Path | None = None
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
