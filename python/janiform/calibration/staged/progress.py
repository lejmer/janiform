from dataclasses import dataclass, field
from typing import Any, Optional, Protocol

from ._progress_policy import (
    CalibrationLiveProgressSnapshot,
    calibration_total_progress_fraction,
    progress_bar_display_units,
)

_RESTART_PROGRESS_PANEL_PHASE = "restart_optimization"


class StagedCalibrationProgressSink(Protocol):
    def update(
        self,
        *,
        detail: Optional[str] = None,
        subdetail: Optional[str] = None,
        completed_units: Optional[int] = None,
        total_units: Optional[int] = None,
        reset_progress_units: bool = False,
        progress_fraction: Optional[float] = None,
    ) -> None: ...


def update_progress_from_staged_native_event(
    progress: StagedCalibrationProgressSink,
    raw_event: Any,
) -> None:
    if not isinstance(raw_event, dict):
        return

    event_kind = _coerce_optional_string(raw_event.get("event_kind"))

    match event_kind:
        case "stage_started":
            progress.update(
                subdetail=_detail_for_stage_started_event(raw_event),
                reset_progress_units=True,
            )
        case "stage_progress":
            progress.update(
                subdetail=format_staged_progress_event_detail(
                    raw_event,
                    include_progress_counts=False,
                ),
                completed_units=_coerce_optional_integer(raw_event.get("completed_units")),
                total_units=_coerce_optional_integer(raw_event.get("total_units")),
            )
        case "stage_completed":
            progress.update(
                subdetail=_detail_for_stage_completed_event(
                    _coerce_optional_string(raw_event.get("stage")),
                    _coerce_optional_payload_dict(raw_event.get("stage_report")),
                ),
                reset_progress_units=True,
            )
        case "run_completed":
            progress.update(
                subdetail="native calibration completed, writing artifacts",
                reset_progress_units=True,
            )
        case "run_failed":
            progress.update(
                subdetail=_detail_for_failed_native_run(raw_event.get("failure")),
                reset_progress_units=True,
            )
        case _:
            return


@dataclass(slots=True)
class ActiveRestartProgressRow:
    restart_id: str
    parameter_block: Optional[str] = None
    current_outer_cycle: Optional[int] = None
    total_outer_cycles: Optional[int] = None
    evaluation_progress_percent: Optional[int] = None
    current_evaluation_count: Optional[int] = None
    current_weighted_objective: Optional[float] = None
    candidate_best_weighted_objective: Optional[float] = None


@dataclass(slots=True)
class NativeStagedCalibrationProgressTracker:
    status: str = "running"
    stage_sequence: tuple[str, ...] = ()
    stage_index: Optional[int] = None
    stage_count: Optional[int] = None
    current_stage: Optional[str] = None
    current_subtask: Optional[str] = None
    current_stage_is_complete: bool = False
    subtask_completed_units: Optional[int] = None
    subtask_total_units: Optional[int] = None
    active_restart_rows: dict[str, ActiveRestartProgressRow] = field(default_factory=dict)

    def update(self, raw_event: dict[str, Any]) -> CalibrationLiveProgressSnapshot:
        event_kind = _coerce_optional_string(raw_event.get("event_kind"))

        match event_kind:
            case "run_started":
                raw_stage_sequence = raw_event.get("stage_sequence")

                self.stage_sequence = (
                    tuple(stage_name for stage_name in raw_stage_sequence if isinstance(stage_name, str))
                    if isinstance(raw_stage_sequence, list)
                    else ()
                )

                self.stage_count = _coerce_optional_integer(raw_event.get("stage_count"))
                self.status = "running"
            case "stage_started":
                stage = _coerce_optional_string(raw_event.get("stage"))
                stage_index = _coerce_optional_integer(raw_event.get("stage_index"))
                stage_count = _coerce_optional_integer(raw_event.get("stage_count"))

                if stage is not None:
                    self.current_stage = stage

                if stage_index is not None:
                    self.stage_index = stage_index

                if stage_count is not None:
                    self.stage_count = stage_count

                self.current_subtask = None
                self.current_stage_is_complete = False
                self.subtask_completed_units = None
                self.subtask_total_units = None
                self.active_restart_rows.clear()
            case "stage_progress":
                stage = _coerce_optional_string(raw_event.get("stage"))
                progress_phase = _coerce_optional_string(raw_event.get("progress_phase"))
                stage_index = _coerce_optional_integer(raw_event.get("stage_index"))
                stage_count = _coerce_optional_integer(raw_event.get("stage_count"))

                if stage is not None:
                    self.current_stage = stage

                if stage_index is not None:
                    self.stage_index = stage_index

                if stage_count is not None:
                    self.stage_count = stage_count

                self.current_subtask = subtask_for_stage_progress_event(
                    self.current_stage,
                    progress_phase,
                )

                self.current_stage_is_complete = False

                self.subtask_completed_units = _coerce_optional_integer(
                    raw_event.get("completed_units")
                )
                self.subtask_total_units = _coerce_optional_integer(raw_event.get("total_units"))
                if progress_phase == _RESTART_PROGRESS_PANEL_PHASE:
                    self._update_active_restart_rows(raw_event)
                else:
                    self.active_restart_rows.clear()
            case "stage_completed":
                stage = _coerce_optional_string(raw_event.get("stage"))
                stage_index = _coerce_optional_integer(raw_event.get("stage_index"))
                stage_count = _coerce_optional_integer(raw_event.get("stage_count"))

                if stage is not None:
                    self.current_stage = stage

                if stage_index is not None:
                    self.stage_index = stage_index

                if stage_count is not None:
                    self.stage_count = stage_count

                self.current_stage_is_complete = True
                self.current_subtask = None
                self.subtask_completed_units = None
                self.subtask_total_units = None
                self.active_restart_rows.clear()
            case "run_completed":
                self.status = "completed"
                self.current_stage_is_complete = True
                self.current_subtask = None
                self.subtask_completed_units = None
                self.subtask_total_units = None
                self.active_restart_rows.clear()
            case "run_failed":
                self.status = "failed"
                self.current_subtask = None
                self.subtask_completed_units = None
                self.subtask_total_units = None
                self.active_restart_rows.clear()
            case _:
                return self.snapshot()

        return self.snapshot()

    def snapshot(self) -> CalibrationLiveProgressSnapshot:
        return CalibrationLiveProgressSnapshot(
            status=self.status,
            stage_sequence=self.stage_sequence,
            stage_index=self.stage_index,
            stage_count=self.stage_count,
            current_stage=self.current_stage,
            current_subtask=self.current_subtask,
            current_stage_is_complete=self.current_stage_is_complete,
            subtask_completed_units=self.subtask_completed_units,
            subtask_total_units=self.subtask_total_units,
        )

    def restart_progress_panel(self, stage_summary: str) -> str:
        if not self.active_restart_rows:
            return stage_summary

        rows = [
            format_restart_progress_panel_row(row)
            for row in self.active_restart_rows.values()
        ]

        return "\n".join((stage_summary, *rows))

    def _update_active_restart_rows(self, raw_event: dict[str, Any]) -> None:
        restart_id = _coerce_optional_string(raw_event.get("current_restart_id"))
        if restart_id is None:
            restart_id = _coerce_optional_string(raw_event.get("current_item_label"))

        if restart_id is None:
            return

        evaluation_progress_percent = _coerce_optional_integer(
            raw_event.get("evaluation_progress_percent")
        )
        if evaluation_progress_percent == 100:
            self.active_restart_rows.pop(restart_id, None)
            return

        row = self.active_restart_rows.get(restart_id)
        if row is None:
            row = ActiveRestartProgressRow(restart_id=restart_id)
            self.active_restart_rows[restart_id] = row

        parameter_block = _coerce_optional_string(raw_event.get("current_parameter_block"))
        current_outer_cycle = _coerce_optional_integer(raw_event.get("current_outer_cycle"))
        total_outer_cycles = _coerce_optional_integer(raw_event.get("total_outer_cycles"))
        current_evaluation_count = _coerce_optional_integer(
            raw_event.get("current_evaluation_count")
        )

        if parameter_block is not None:
            row.parameter_block = parameter_block

        if current_outer_cycle is not None:
            row.current_outer_cycle = current_outer_cycle

        if total_outer_cycles is not None:
            row.total_outer_cycles = total_outer_cycles

        if evaluation_progress_percent is not None:
            row.evaluation_progress_percent = evaluation_progress_percent

        if current_evaluation_count is not None:
            row.current_evaluation_count = current_evaluation_count

        row.current_weighted_objective = _coerce_optional_float(
            raw_event.get("current_weighted_objective")
        )
        row.candidate_best_weighted_objective = _coerce_optional_float(
            raw_event.get("current_restart_best_weighted_objective")
        )


def update_cli_progress_from_staged_native_event(
    progress: StagedCalibrationProgressSink,
    tracker: NativeStagedCalibrationProgressTracker,
    raw_event: Any,
) -> None:
    if not isinstance(raw_event, dict):
        return

    snapshot = tracker.update(raw_event)

    progress_fraction = calibration_total_progress_fraction(snapshot)

    event_kind = _coerce_optional_string(raw_event.get("event_kind"))

    match event_kind:
        case "stage_started":
            progress.update(
                subdetail=_detail_for_stage_started_event(raw_event),
                reset_progress_units=True,
                progress_fraction=progress_fraction,
            )
        case "stage_progress":
            display_units = progress_bar_display_units(
                _coerce_optional_integer(raw_event.get("completed_units")),
                _coerce_optional_integer(raw_event.get("total_units")),
            )
            compact_detail = format_compact_staged_progress_event_detail(
                raw_event,
                completed_units=None if display_units is None else display_units[0],
                total_units=None if display_units is None else display_units[1],
            )
            progress_phase = _coerce_optional_string(raw_event.get("progress_phase"))
            subdetail = (
                tracker.restart_progress_panel(
                    format_compact_staged_progress_phase_summary(
                        raw_event,
                        completed_units=None if display_units is None else display_units[0],
                        total_units=None if display_units is None else display_units[1],
                    )
                )
                if progress_phase == _RESTART_PROGRESS_PANEL_PHASE
                else compact_detail
            )
            progress.update(
                subdetail=subdetail,
                completed_units=None if display_units is None else display_units[0],
                total_units=None if display_units is None else display_units[1],
                progress_fraction=progress_fraction,
            )
        case "stage_completed":
            progress.update(
                subdetail=_detail_for_stage_completed_event(
                    _coerce_optional_string(raw_event.get("stage")),
                    _coerce_optional_payload_dict(raw_event.get("stage_report")),
                ),
                reset_progress_units=True,
                progress_fraction=progress_fraction,
            )
        case "run_completed":
            progress.update(
                subdetail="native calibration completed, writing artifacts",
                reset_progress_units=True,
                progress_fraction=progress_fraction,
            )
        case "run_failed":
            progress.update(
                subdetail=_detail_for_failed_native_run(raw_event.get("failure")),
                reset_progress_units=True,
                progress_fraction=progress_fraction,
            )
        case _:
            return


def format_staged_progress_event_detail(
    raw_event: dict[str, Any],
    *,
    include_progress_counts: bool = True,
) -> str:
    stage = _coerce_optional_string(raw_event.get("stage")) or "stage"
    progress_phase = _coerce_optional_string(raw_event.get("progress_phase"))
    completed_units = raw_event.get("completed_units")
    total_units = raw_event.get("total_units")
    current_item_label = _coerce_optional_string(raw_event.get("current_item_label"))
    current_restart_id = _coerce_optional_string(raw_event.get("current_restart_id"))
    active_restart_count = raw_event.get("active_restart_count")
    current_parameter_block = _coerce_optional_string(raw_event.get("current_parameter_block"))
    current_outer_cycle = raw_event.get("current_outer_cycle")
    total_outer_cycles = raw_event.get("total_outer_cycles")
    current_evaluation_count = raw_event.get("current_evaluation_count")
    evaluation_progress_percent = raw_event.get("evaluation_progress_percent")
    valid_completed_units = raw_event.get("valid_completed_units")
    cumulative_local_evaluation_count = raw_event.get("cumulative_local_evaluation_count")
    current_weighted_objective = raw_event.get("current_weighted_objective")
    current_restart_best_weighted_objective = raw_event.get(
        "current_restart_best_weighted_objective"
    )
    best_weighted_objective = raw_event.get("best_weighted_objective")
    current_coarse_consistency_fit = raw_event.get("current_coarse_consistency_fit")
    best_coarse_consistency_fit = raw_event.get("best_coarse_consistency_fit")

    phase_label = _label_for_stage_progress_phase(progress_phase)
    detail = f"{stage} {phase_label}"

    if (
        include_progress_counts
        and isinstance(completed_units, int)
        and isinstance(total_units, int)
    ):
        detail = f"{detail} {completed_units}/{total_units}"

    detail_parts = [detail]

    item_label = current_restart_id or current_item_label
    if item_label is not None:
        detail_parts.append(f"item={item_label}")

    if isinstance(active_restart_count, int):
        detail_parts.append(f"running={active_restart_count}")

    if current_parameter_block is not None:
        detail_parts.append(f"block={current_parameter_block}")

    if isinstance(current_outer_cycle, int) and isinstance(total_outer_cycles, int):
        detail_parts.append(f"cycle={current_outer_cycle}/{total_outer_cycles}")
    elif isinstance(current_outer_cycle, int):
        detail_parts.append(f"cycle={current_outer_cycle}")

    if isinstance(evaluation_progress_percent, int):
        evaluation_label = (
            "<1%"
            if evaluation_progress_percent == 0
            and isinstance(current_evaluation_count, int)
            and current_evaluation_count > 0
            else f"{evaluation_progress_percent}%"
        )
        detail_parts.append(f"eval={evaluation_label}")

    if isinstance(valid_completed_units, int):
        detail_parts.append(f"valid={valid_completed_units}")

    if isinstance(cumulative_local_evaluation_count, int):
        detail_parts.append(f"local_evaluations={cumulative_local_evaluation_count}")

    if isinstance(current_weighted_objective, int | float):
        detail_parts.append(f"objective={current_weighted_objective:.6f}")

    if isinstance(current_restart_best_weighted_objective, int | float):
        detail_parts.append(
            f"candidate_best_objective={current_restart_best_weighted_objective:.6f}"
        )

    if isinstance(best_weighted_objective, int | float):
        detail_parts.append(f"best_objective={best_weighted_objective:.6f}")

    if isinstance(current_coarse_consistency_fit, int | float):
        detail_parts.append(f"coarse_fit={current_coarse_consistency_fit:.6f}")

    if isinstance(best_coarse_consistency_fit, int | float):
        detail_parts.append(f"best_coarse_fit={best_coarse_consistency_fit:.6f}")

    return " | ".join(detail_parts)


def format_compact_staged_progress_event_detail(
    raw_event: dict[str, Any],
    *,
    completed_units: Optional[int] = None,
    total_units: Optional[int] = None,
) -> str:
    stage = _coerce_optional_string(raw_event.get("stage")) or "stage"
    progress_phase = _coerce_optional_string(raw_event.get("progress_phase"))
    current_item_label = _coerce_optional_string(raw_event.get("current_item_label"))
    current_restart_id = _coerce_optional_string(raw_event.get("current_restart_id"))
    active_restart_count = raw_event.get("active_restart_count")
    current_parameter_block = _coerce_optional_string(raw_event.get("current_parameter_block"))
    current_outer_cycle = raw_event.get("current_outer_cycle")
    total_outer_cycles = raw_event.get("total_outer_cycles")
    current_evaluation_count = raw_event.get("current_evaluation_count")
    evaluation_progress_percent = raw_event.get("evaluation_progress_percent")
    current_weighted_objective = raw_event.get("current_weighted_objective")
    current_restart_best_weighted_objective = raw_event.get(
        "current_restart_best_weighted_objective"
    )
    best_weighted_objective = raw_event.get("best_weighted_objective")
    current_coarse_consistency_fit = raw_event.get("current_coarse_consistency_fit")
    best_coarse_consistency_fit = raw_event.get("best_coarse_consistency_fit")

    detail = f"{stage} {_label_for_stage_progress_phase(progress_phase)}"
    if (
        isinstance(completed_units, int)
        and isinstance(total_units, int)
        and total_units > 0
    ):
        detail = (
            f"{detail} [{round(completed_units / total_units * 100.0)}%] "
            f"({completed_units}/{total_units})"
        )

    detail_parts = [detail]

    item_label = current_restart_id or current_item_label
    if item_label is not None:
        detail_parts.append(f"item={item_label}")

    if isinstance(active_restart_count, int):
        detail_parts.append(f"running={active_restart_count}")

    if current_parameter_block is not None:
        detail_parts.append(f"block={current_parameter_block}")

    if isinstance(current_outer_cycle, int) and isinstance(total_outer_cycles, int):
        detail_parts.append(f"cycle={current_outer_cycle}/{total_outer_cycles}")
    elif isinstance(current_outer_cycle, int):
        detail_parts.append(f"cycle={current_outer_cycle}")

    if isinstance(evaluation_progress_percent, int):
        evaluation_label = (
            "<1%"
            if evaluation_progress_percent == 0
            and isinstance(current_evaluation_count, int)
            and current_evaluation_count > 0
            else f"{evaluation_progress_percent}%"
        )
        detail_parts.append(f"eval={evaluation_label}")

    if isinstance(current_weighted_objective, int | float):
        detail_parts.append(f"obj={current_weighted_objective:.6f}")

    if isinstance(current_restart_best_weighted_objective, int | float):
        detail_parts.append(f"candidate_best={current_restart_best_weighted_objective:.6f}")

    if isinstance(best_weighted_objective, int | float):
        label = (
            "global_best"
            if isinstance(current_restart_best_weighted_objective, int | float)
            else "best_obj"
        )
        detail_parts.append(f"{label}={best_weighted_objective:.6f}")

    if isinstance(current_coarse_consistency_fit, int | float):
        detail_parts.append(f"coarse={current_coarse_consistency_fit:.6f}")

    if isinstance(best_coarse_consistency_fit, int | float):
        detail_parts.append(f"best_coarse={best_coarse_consistency_fit:.6f}")

    return " | ".join(detail_parts)


def format_compact_staged_progress_phase_summary(
    raw_event: dict[str, Any],
    *,
    completed_units: Optional[int] = None,
    total_units: Optional[int] = None,
) -> str:
    stage = _coerce_optional_string(raw_event.get("stage")) or "stage"
    progress_phase = _coerce_optional_string(raw_event.get("progress_phase"))
    detail = f"{stage} {_label_for_stage_progress_phase(progress_phase)}"
    if (
        isinstance(completed_units, int)
        and isinstance(total_units, int)
        and total_units > 0
    ):
        return (
            f"{detail} [{round(completed_units / total_units * 100.0)}%] "
            f"({completed_units}/{total_units})"
        )

    return detail


def format_restart_progress_panel_row(row: ActiveRestartProgressRow) -> str:
    block = row.parameter_block or "-"
    cycle = (
        f"{row.current_outer_cycle}/{row.total_outer_cycles}"
        if row.current_outer_cycle is not None and row.total_outer_cycles is not None
        else "-"
    )
    evaluation_label = _format_restart_row_evaluation_label(row)
    objective = (
        f"{row.current_weighted_objective:.2f}"
        if row.current_weighted_objective is not None
        else "-"
    )
    candidate_best = (
        f"{row.candidate_best_weighted_objective:.2f}"
        if row.candidate_best_weighted_objective is not None
        else "-"
    )

    return (
        f"  {row.restart_id:<22} {block:<8.8} {cycle:>5}  "
        f"eval {evaluation_label:>4}  obj {objective:>10}  best {candidate_best:>10}"
    )


def _format_restart_row_evaluation_label(row: ActiveRestartProgressRow) -> str:
    if row.evaluation_progress_percent is None:
        return "-"

    if (
        row.evaluation_progress_percent == 0
        and row.current_evaluation_count is not None
        and row.current_evaluation_count > 0
    ):
        return "<1%"

    return f"{row.evaluation_progress_percent}%"


def subtask_for_stage_progress_event(
    stage: Optional[str],
    progress_phase: Optional[str],
) -> str:
    if stage is None and progress_phase is None:
        return "stage_progress"

    if stage is None:
        return progress_phase or "stage_progress"

    if progress_phase is None:
        return stage

    return f"{stage}.{progress_phase}"


def detail_for_stage_completed_event(
    stage: Optional[str],
    stage_report: Optional[dict[str, Any]],
) -> str:
    return _detail_for_stage_completed_event(stage, stage_report)


def detail_for_failed_native_run(failure_payload: Any) -> str:
    return _detail_for_failed_native_run(failure_payload)


def _detail_for_stage_started_event(raw_event: dict[str, Any]) -> str:
    stage = _coerce_optional_string(raw_event.get("stage")) or "stage"
    stage_index = _coerce_optional_integer(raw_event.get("stage_index"))
    stage_count = _coerce_optional_integer(raw_event.get("stage_count"))

    if (
        stage_index is not None
        and stage_count is not None
        and stage_index > 0
        and stage_count > 0
    ):
        return f"stage {stage_index}/{stage_count}: {stage}"

    return stage


def _coerce_optional_integer(value: Any) -> Optional[int]:
    if isinstance(value, int):
        return value

    return None


def _coerce_optional_float(value: Any) -> Optional[float]:
    if isinstance(value, int | float):
        return float(value)

    return None


def _coerce_optional_string(value: Any) -> Optional[str]:
    if isinstance(value, str):
        return value

    return None


def _coerce_optional_payload_dict(value: Any) -> Optional[dict[str, Any]]:
    if isinstance(value, dict):
        return value

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
        failed_stage = _coerce_optional_string(failure_payload.get("stage"))
        failure_kind = _coerce_optional_string(failure_payload.get("failure_kind"))
        error_message = _coerce_optional_string(failure_payload.get("error_message"))

        if failed_stage is not None and error_message is not None and failure_kind is not None:
            return f"Calibration failed at {failed_stage} with {failure_kind}: {error_message}"

        if failed_stage is not None and error_message is not None:
            return f"Calibration failed at {failed_stage}: {error_message}"

        if error_message is not None and failure_kind is not None:
            return f"Calibration failed with {failure_kind}: {error_message}"

        if failed_stage is not None and failure_kind is not None:
            return f"Calibration failed at {failed_stage} with {failure_kind}"

        if error_message is not None:
            return f"Calibration failed: {error_message}"

        if failure_kind is not None:
            return f"Calibration failed with {failure_kind}"

    return "Calibration failed"


def _label_for_stage_progress_phase(progress_phase: Optional[str]) -> str:
    match progress_phase:
        case "baseline_local_evaluation":
            return "baseline local evaluation"
        case "baseline_final_evaluation":
            return "baseline final evaluation"
        case "restart_optimization":
            return "restart optimization"
        case "coarse_consistency_evaluation":
            return "coarse consistency evaluation"
        case "final_seed_evaluation":
            return "final seed evaluation"
        case "final_assembly":
            return "final assembly"
        case _:
            return progress_phase or "progress"


__all__ = [
    "detail_for_failed_native_run",
    "detail_for_stage_completed_event",
    "format_compact_staged_progress_event_detail",
    "format_staged_progress_event_detail",
    "NativeStagedCalibrationProgressTracker",
    "subtask_for_stage_progress_event",
    "update_cli_progress_from_staged_native_event",
    "update_progress_from_staged_native_event",
]
