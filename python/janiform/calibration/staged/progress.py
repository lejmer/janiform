from collections.abc import Iterable
from dataclasses import dataclass, field
from typing import Any, Optional, Protocol

from ._progress_policy import (
    CalibrationLiveProgressSnapshot,
    calibration_total_progress_fraction,
    progress_bar_display_units,
)

_RESTART_PROGRESS_PANEL_PHASE = "restart_optimization"
_OPTION_SEARCH_PROGRESS_PANEL_PHASE = "option_search_tier_refinement"
_OPTION_SELECTION_PROGRESS_PANEL_PHASE = "option_selection_tier_evaluation"
_ACTIVE_CANDIDATE_PROGRESS_PANEL_PHASES = frozenset(
    {
        _RESTART_PROGRESS_PANEL_PHASE,
        _OPTION_SEARCH_PROGRESS_PANEL_PHASE,
        _OPTION_SELECTION_PROGRESS_PANEL_PHASE,
    }
)


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
class ActiveCandidateProgressRow:
    restart_id: str
    progress_phase: Optional[str] = None
    detail: Optional[str] = None
    parameter_block: Optional[str] = None
    current_outer_cycle: Optional[int] = None
    total_outer_cycles: Optional[int] = None
    evaluation_progress_percent: Optional[int] = None
    current_evaluation_count: Optional[int] = None
    total_evaluation_budget: Optional[int] = None
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
    subtask_progress_fraction_override: Optional[float] = None
    active_candidate_rows: dict[str, ActiveCandidateProgressRow] = field(
        default_factory=dict
    )

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
                self.subtask_progress_fraction_override = None

                self.active_candidate_rows.clear()
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

                aggregate_progress_percent = _coerce_optional_integer(
                    raw_event.get("aggregate_progress_percent")
                )

                self.subtask_progress_fraction_override = (
                    None
                    if aggregate_progress_percent is None
                    else min(max(aggregate_progress_percent, 0), 100) / 100.0
                )

                if _is_active_candidate_progress_panel_phase(progress_phase):
                    self._update_active_candidate_rows(raw_event)
                else:
                    self.active_candidate_rows.clear()
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
                self.subtask_progress_fraction_override = None

                self.active_candidate_rows.clear()
            case "run_completed":
                self.status = "completed"
                self.current_stage_is_complete = True
                self.current_subtask = None
                self.subtask_completed_units = None
                self.subtask_total_units = None
                self.subtask_progress_fraction_override = None

                self.active_candidate_rows.clear()
            case "run_failed":
                self.status = "failed"
                self.current_subtask = None
                self.subtask_completed_units = None
                self.subtask_total_units = None
                self.subtask_progress_fraction_override = None

                self.active_candidate_rows.clear()
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
            subtask_progress_fraction_override=self.subtask_progress_fraction_override,
        )

    def active_candidate_progress_panel(self, stage_summary: str) -> str:
        if not self.active_candidate_rows:
            return stage_summary

        restart_id_width = maximum_active_candidate_progress_row_id_width(
            self.active_candidate_rows.values()
        )

        rows = [
            format_active_candidate_progress_panel_row(
                row,
                restart_id_width=restart_id_width,
            )
            for row in self.active_candidate_rows.values()
        ]

        return "\n".join((stage_summary, *rows))

    def _update_active_candidate_rows(self, raw_event: dict[str, Any]) -> None:
        restart_id = _active_candidate_restart_id(raw_event)
        if restart_id is None:
            return

        if _should_clear_active_candidate_row(raw_event):
            self.active_candidate_rows.pop(restart_id, None)
            return

        row = self.active_candidate_rows.get(restart_id)
        if row is None:
            row = ActiveCandidateProgressRow(restart_id=restart_id)
            self.active_candidate_rows[restart_id] = row

        update_active_candidate_progress_row_from_event(row, raw_event)


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
    aggregate_progress_percent = _coerce_optional_integer(
        raw_event.get("aggregate_progress_percent")
    )

    match event_kind:
        case "stage_started":
            progress.update(
                subdetail=_detail_for_stage_started_event(raw_event),
                reset_progress_units=True,
                progress_fraction=progress_fraction,
            )
        case "stage_progress":
            raw_completed_units = _coerce_optional_integer(raw_event.get("completed_units"))
            raw_total_units = _coerce_optional_integer(raw_event.get("total_units"))

            subtask_is_complete = (
                isinstance(raw_completed_units, int)
                and isinstance(raw_total_units, int)
                and 0 < raw_total_units <= raw_completed_units
            )

            display_units = progress_bar_display_units(
                raw_completed_units,
                raw_total_units,
                is_complete=subtask_is_complete,
            )

            compact_detail = format_compact_staged_progress_event_detail(
                raw_event,
                completed_units=None if display_units is None else display_units[0],
                total_units=None if display_units is None else display_units[1],
                progress_percent_override=aggregate_progress_percent,
            )

            progress_phase = _coerce_optional_string(raw_event.get("progress_phase"))

            subdetail = (
                tracker.active_candidate_progress_panel(
                    format_compact_staged_progress_phase_summary(
                        raw_event,
                        completed_units=None if display_units is None else display_units[0],
                        total_units=None if display_units is None else display_units[1],
                        progress_percent_override=aggregate_progress_percent,
                    )
                )
                if _is_active_candidate_progress_panel_phase(progress_phase)
                else compact_detail
            )

            progress.update(
                subdetail=subdetail,
                completed_units=None if display_units is None else display_units[0],
                total_units=None if display_units is None else display_units[1],
                progress_fraction=progress_fraction,
            )
        case "stage_completed":
            progress.update(progress_fraction=progress_fraction)
        case "run_completed":
            progress.update(progress_fraction=progress_fraction)
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
    total_evaluation_budget = raw_event.get("total_evaluation_budget")
    evaluation_progress_percent = raw_event.get("evaluation_progress_percent")
    valid_completed_units = raw_event.get("valid_completed_units")
    cumulative_local_evaluation_count = raw_event.get("cumulative_local_evaluation_count")
    current_weighted_objective = raw_event.get("current_weighted_objective")
    current_restart_best_weighted_objective = raw_event.get("current_restart_best_weighted_objective")
    best_restart_id = _coerce_optional_string(raw_event.get("best_restart_id"))
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

    if current_restart_id is not None:
        detail_parts.append(f"item={current_restart_id}")
    elif current_item_label is not None:
        detail_parts.append(f"item={current_item_label}")

    if current_restart_id is not None and current_item_label is not None:
        detail_parts.append(f"detail={current_item_label}")

    if isinstance(active_restart_count, int):
        detail_parts.append(f"running={active_restart_count}")

    if current_parameter_block is not None:
        detail_parts.append(f"block={current_parameter_block}")

    if isinstance(current_outer_cycle, int) and isinstance(total_outer_cycles, int):
        detail_parts.append(f"cycle={current_outer_cycle}/{total_outer_cycles}")
    elif isinstance(current_outer_cycle, int):
        detail_parts.append(f"cycle={current_outer_cycle}")

    evaluation_label = _format_stage_evaluation_label(
        current_evaluation_count,
        total_evaluation_budget,
        evaluation_progress_percent,
    )
    if evaluation_label is not None:
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

    if best_restart_id is not None:
        detail_parts.append(f"best_candidate={best_restart_id}")

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
    progress_percent_override: Optional[int] = None,
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
    total_evaluation_budget = raw_event.get("total_evaluation_budget")
    evaluation_progress_percent = raw_event.get("evaluation_progress_percent")
    current_weighted_objective = raw_event.get("current_weighted_objective")
    current_restart_best_weighted_objective = raw_event.get("current_restart_best_weighted_objective")
    best_restart_id = _coerce_optional_string(raw_event.get("best_restart_id"))
    best_weighted_objective = raw_event.get("best_weighted_objective")
    current_coarse_consistency_fit = raw_event.get("current_coarse_consistency_fit")
    best_coarse_consistency_fit = raw_event.get("best_coarse_consistency_fit")

    detail = f"{stage} {_label_for_stage_progress_phase(progress_phase)}"

    if (
        isinstance(completed_units, int)
        and isinstance(total_units, int)
        and total_units > 0
    ):
        progress_percent = (
            min(max(progress_percent_override, 0), 100)
            if isinstance(progress_percent_override, int)
            else round(completed_units / total_units * 100.0)
        )
        if progress_phase == _RESTART_PROGRESS_PANEL_PHASE and isinstance(
            progress_percent_override, int
        ):
            detail = f"{detail} [{progress_percent}%]"
        else:
            detail = f"{detail} [{progress_percent}%] ({completed_units}/{total_units})"

    detail_parts = [detail]

    if current_restart_id is not None:
        detail_parts.append(f"item={current_restart_id}")
    elif current_item_label is not None:
        detail_parts.append(f"item={current_item_label}")

    if current_restart_id is not None and current_item_label is not None:
        detail_parts.append(f"detail={current_item_label}")

    if isinstance(active_restart_count, int):
        detail_parts.append(f"running={active_restart_count}")

    if current_parameter_block is not None:
        detail_parts.append(f"block={current_parameter_block}")

    if isinstance(current_outer_cycle, int) and isinstance(total_outer_cycles, int):
        detail_parts.append(f"cycle={current_outer_cycle}/{total_outer_cycles}")
    elif isinstance(current_outer_cycle, int):
        detail_parts.append(f"cycle={current_outer_cycle}")

    evaluation_label = _format_stage_evaluation_label(
        current_evaluation_count,
        total_evaluation_budget,
        evaluation_progress_percent,
    )
    if evaluation_label is not None:
        detail_parts.append(f"eval={evaluation_label}")

    if isinstance(current_weighted_objective, int | float):
        detail_parts.append(f"obj={current_weighted_objective:.6f}")

    if isinstance(current_restart_best_weighted_objective, int | float):
        detail_parts.append(f"candidate_best={current_restart_best_weighted_objective:.6f}")

    if best_restart_id is not None:
        detail_parts.append(f"best={best_restart_id}")

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
    progress_percent_override: Optional[int] = None,
) -> str:
    stage = _coerce_optional_string(raw_event.get("stage")) or "stage"
    progress_phase = _coerce_optional_string(raw_event.get("progress_phase"))
    best_restart_id = _coerce_optional_string(raw_event.get("best_restart_id"))
    best_weighted_objective = raw_event.get("best_weighted_objective")

    detail = f"{stage} {_label_for_stage_progress_phase(progress_phase)}"

    if (
        isinstance(completed_units, int)
        and isinstance(total_units, int)
        and total_units > 0
    ):
        progress_percent = (
            min(max(progress_percent_override, 0), 100)
            if isinstance(progress_percent_override, int)
            else round(completed_units / total_units * 100.0)
        )

        if progress_phase == _RESTART_PROGRESS_PANEL_PHASE and isinstance(progress_percent_override, int):
            detail = f"{detail} [{progress_percent}%]"
        else:
            detail = f"{detail} [{progress_percent}%] ({completed_units}/{total_units})"

    detail_parts = [detail]

    if best_restart_id is not None:
        detail_parts.append(f"best={best_restart_id}")

    if isinstance(best_weighted_objective, int | float):
        detail_parts.append(f"best_obj={best_weighted_objective:.6f}")

    return " | ".join(detail_parts)


def format_active_candidate_progress_panel_row(
    row: ActiveCandidateProgressRow,
    *,
    restart_id_width: int,
) -> str:
    if not _is_restart_like_active_candidate_progress_row(row):
        detail = row.detail or "-"

        evaluation_label = _format_active_candidate_progress_row_evaluation_label(row)

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
            f"  {row.restart_id:<{restart_id_width}} {detail:<28.28}  "
            f"eval {evaluation_label:>5}  obj {objective:>10}  best {candidate_best:>10}"
        )

    block = row.parameter_block or "-"

    cycle = (
        f"{row.current_outer_cycle}/{row.total_outer_cycles}"
        if row.current_outer_cycle is not None and row.total_outer_cycles is not None
        else "-"
    )

    evaluation_label = _format_active_candidate_progress_row_evaluation_label(row)

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
        f"  {row.restart_id:<{restart_id_width}} {block:<8.8} {cycle:>5}  "
        f"eval {evaluation_label:>4}  obj {objective:>10}  best {candidate_best:>10}"
    )


def maximum_active_candidate_progress_row_id_width(
    rows: Iterable[ActiveCandidateProgressRow],
) -> int:
    return max(
        22,
        max(
            (len(row.restart_id) for row in rows),
            default=0,
        ),
    )


def _format_active_candidate_progress_row_evaluation_label(row: ActiveCandidateProgressRow) -> str:
    evaluation_label = _format_stage_evaluation_label(
        row.current_evaluation_count,
        row.total_evaluation_budget,
        row.evaluation_progress_percent,
    )
    if evaluation_label is None:
        return "-"

    return evaluation_label


def _is_restart_like_active_candidate_progress_row(row: ActiveCandidateProgressRow) -> bool:
    return (
        row.progress_phase == _RESTART_PROGRESS_PANEL_PHASE
        or row.parameter_block is not None
        or row.current_outer_cycle is not None
        or row.total_outer_cycles is not None
    )


def _format_stage_evaluation_label(
    current_evaluation_count: Any,
    total_evaluation_budget: Any,
    evaluation_progress_percent: Any,
) -> Optional[str]:
    if isinstance(current_evaluation_count, int) and isinstance(total_evaluation_budget, int):
        return f"{current_evaluation_count}/{total_evaluation_budget}"

    if not isinstance(evaluation_progress_percent, int):
        return None

    if evaluation_progress_percent == 0 and isinstance(current_evaluation_count, int) and current_evaluation_count > 0:
        return "<1%"

    return f"{evaluation_progress_percent}%"


def _is_active_candidate_progress_panel_phase(progress_phase: Optional[str]) -> bool:
    return progress_phase in _ACTIVE_CANDIDATE_PROGRESS_PANEL_PHASES


def _active_candidate_restart_id(raw_event: dict[str, Any]) -> Optional[str]:
    restart_id = _coerce_optional_string(raw_event.get("current_restart_id"))
    if restart_id is not None:
        return restart_id

    return _coerce_optional_string(raw_event.get("current_item_label"))


def _should_clear_active_candidate_row(raw_event: dict[str, Any]) -> bool:
    current_item_label = _coerce_optional_string(raw_event.get("current_item_label"))
    if current_item_label == "resumed":
        return True

    evaluation_progress_percent = _coerce_optional_integer(
        raw_event.get("evaluation_progress_percent")
    )

    return evaluation_progress_percent == 100


def update_active_candidate_progress_row_from_event(
    row: ActiveCandidateProgressRow,
    raw_event: dict[str, Any],
) -> None:
    row.progress_phase = _coerce_optional_string(raw_event.get("progress_phase"))

    current_item_label = _coerce_optional_string(raw_event.get("current_item_label"))
    if current_item_label is not None and current_item_label != row.restart_id:
        row.detail = current_item_label

    parameter_block = _coerce_optional_string(raw_event.get("current_parameter_block"))
    if parameter_block is not None:
        row.parameter_block = parameter_block

    current_outer_cycle = _coerce_optional_integer(raw_event.get("current_outer_cycle"))
    if current_outer_cycle is not None:
        row.current_outer_cycle = current_outer_cycle

    total_outer_cycles = _coerce_optional_integer(raw_event.get("total_outer_cycles"))
    if total_outer_cycles is not None:
        row.total_outer_cycles = total_outer_cycles

    evaluation_progress_percent = _coerce_optional_integer(
        raw_event.get("evaluation_progress_percent")
    )
    if evaluation_progress_percent is not None:
        row.evaluation_progress_percent = evaluation_progress_percent

    current_evaluation_count = _coerce_optional_integer(
        raw_event.get("current_evaluation_count")
    )
    if current_evaluation_count is not None:
        row.current_evaluation_count = current_evaluation_count

    total_evaluation_budget = _coerce_optional_integer(
        raw_event.get("total_evaluation_budget")
    )
    if total_evaluation_budget is not None:
        row.total_evaluation_budget = total_evaluation_budget

    row.current_weighted_objective = _coerce_optional_float(
        raw_event.get("current_weighted_objective")
    )
    row.candidate_best_weighted_objective = _coerce_optional_float(
        raw_event.get("current_restart_best_weighted_objective")
    )


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
    "ActiveCandidateProgressRow",
    "detail_for_failed_native_run",
    "detail_for_stage_completed_event",
    "format_active_candidate_progress_panel_row",
    "format_compact_staged_progress_event_detail",
    "format_staged_progress_event_detail",
    "maximum_active_candidate_progress_row_id_width",
    "NativeStagedCalibrationProgressTracker",
    "subtask_for_stage_progress_event",
    "update_cli_progress_from_staged_native_event",
    "update_progress_from_staged_native_event",
]
