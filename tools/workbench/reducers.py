from .events import WorkbenchEvent, WorkbenchEventType
from .state import WorkbenchAppState, WorkbenchProgressModel


def _update_total_progress_from_stage_position(
    state: WorkbenchAppState,
    *,
    stage_index: int,
    stage_count: int,
    current_stage_fraction: float,
    detail: str,
) -> None:
    completed_units = stage_index if current_stage_fraction >= 1.0 else max(stage_index - 1, 0)
    state.run.overall_progress = WorkbenchProgressModel(
        label="Calibration",
        completed_units=completed_units,
        total_units=stage_count,
        detail=detail,
        is_indeterminate=False,
    )


def apply_workbench_event(
    state: WorkbenchAppState,
    event: WorkbenchEvent,
) -> None:
    state.session.opened_run_id = event.run_id
    state.run.run_id = event.run_id
    if event.stage is not None:
        state.run.current_stage = event.stage
    if event.subtask is not None:
        state.run.current_subtask = event.subtask
    if event.evaluation_tier is not None:
        state.run.current_evaluation_tier = event.evaluation_tier

    match event.event_type:
        case WorkbenchEventType.RUN_STARTED:
            state.session.mode = "live_run"
            state.session.status_message = "Calibration running"
            state.run.status = "running"
            state.run.started_at_utc = event.occurred_at_utc
            state.run.finished_at_utc = None
            raw_stage_sequence = event.payload.get("stage_sequence")
            if isinstance(raw_stage_sequence, list):
                state.run.stage_sequence = tuple(
                    stage_name
                    for stage_name in raw_stage_sequence
                    if isinstance(stage_name, str)
                )
            else:
                state.run.stage_sequence = ()
            state.run.elapsed_seconds = 0.0
            state.run.current_stage_elapsed_seconds = 0.0
            state.run.current_stage_started_elapsed_seconds = None
            state.run.current_stage_is_complete = False
            state.run.overall_progress = WorkbenchProgressModel(
                label="Calibration",
                detail=str(event.payload.get("detail", "Starting calibration")),
                is_indeterminate=True,
            )
        case WorkbenchEventType.RUN_COMPLETED:
            state.session.mode = "inspector"
            state.session.status_message = "Calibration completed"
            state.run.status = "completed"
            state.run.finished_at_utc = event.occurred_at_utc
            state.run.current_stage_is_complete = True
            state.run.current_stage_started_elapsed_seconds = None
            state.run.overall_progress = WorkbenchProgressModel(
                label="Calibration",
                completed_units=1,
                total_units=1,
                detail=str(event.payload.get("detail", "Calibration completed")),
                is_indeterminate=False,
            )
        case WorkbenchEventType.RUN_FAILED:
            state.session.status_message = "Calibration failed"
            state.session.last_error_message = str(event.payload.get("message", "Calibration failed"))
            state.run.status = "failed"
            state.run.finished_at_utc = event.occurred_at_utc
            state.run.current_stage_started_elapsed_seconds = None
        case WorkbenchEventType.STAGE_STARTED:
            state.run.current_stage = event.stage
            state.run.current_stage_elapsed_seconds = 0.0
            state.run.current_stage_started_elapsed_seconds = state.run.elapsed_seconds
            state.run.current_stage_is_complete = False
            state.run.stage_progress = WorkbenchProgressModel(
                label=str(event.payload.get("stage_label", event.stage or "Stage")),
                completed_units=event.payload.get("stage_index"),
                total_units=event.payload.get("stage_count"),
                detail=str(event.payload.get("detail", "Stage started")),
                is_indeterminate=False,
            )
            stage_index = event.payload.get("stage_index")
            stage_count = event.payload.get("stage_count")
            if isinstance(stage_index, int) and isinstance(stage_count, int) and stage_count > 0:
                _update_total_progress_from_stage_position(
                    state,
                    stage_index=stage_index,
                    stage_count=stage_count,
                    current_stage_fraction=0.0,
                    detail=str(event.payload.get("detail", "Stage started")),
                )
        case WorkbenchEventType.STAGE_COMPLETED:
            state.run.current_stage = event.stage
            state.run.current_stage_is_complete = True
            runtime_seconds = event.payload.get("runtime_seconds")
            if isinstance(runtime_seconds, int | float):
                state.run.current_stage_elapsed_seconds = float(runtime_seconds)
            elif state.run.current_stage_started_elapsed_seconds is not None:
                state.run.current_stage_elapsed_seconds = max(
                    0.0,
                    state.run.elapsed_seconds - state.run.current_stage_started_elapsed_seconds,
                )
            state.run.current_stage_started_elapsed_seconds = None
            state.run.stage_progress = WorkbenchProgressModel(
                label=str(event.payload.get("stage_label", event.stage or "Stage")),
                completed_units=event.payload.get("stage_index"),
                total_units=event.payload.get("stage_count"),
                detail=str(event.payload.get("detail", "Stage completed")),
                is_indeterminate=False,
            )
            stage_index = event.payload.get("stage_index")
            stage_count = event.payload.get("stage_count")
            if isinstance(stage_index, int) and isinstance(stage_count, int) and stage_count > 0:
                _update_total_progress_from_stage_position(
                    state,
                    stage_index=stage_index,
                    stage_count=stage_count,
                    current_stage_fraction=1.0,
                    detail=str(event.payload.get("detail", "Stage completed")),
                )
        case WorkbenchEventType.SUBTASK_STARTED | WorkbenchEventType.SUBTASK_COMPLETED:
            detail = str(event.payload.get("detail", event.subtask or "Working"))
            completed_units = event.payload.get("completed_units")
            total_units = event.payload.get("total_units")
            has_determinate_progress = isinstance(completed_units, int) and isinstance(
                total_units, int
            )
            state.run.subprogress = WorkbenchProgressModel(
                label=event.subtask or "Subtask",
                completed_units=completed_units if has_determinate_progress else None,
                total_units=total_units if has_determinate_progress else None,
                detail=detail,
                is_indeterminate=not has_determinate_progress,
            )
            state.session.status_message = detail
            stage_index = state.run.stage_progress.completed_units
            stage_count = state.run.stage_progress.total_units
            if (
                state.run.status == "running"
                and isinstance(stage_index, int)
                and isinstance(stage_count, int)
                and stage_count > 0
            ):
                current_stage_fraction = 0.0
                if (
                    has_determinate_progress
                    and isinstance(total_units, int)
                    and total_units > 0
                    and isinstance(completed_units, int)
                ):
                    current_stage_fraction = min(max(completed_units / total_units, 0.0), 1.0)
                elif state.run.current_stage_is_complete:
                    current_stage_fraction = 1.0

                _update_total_progress_from_stage_position(
                    state,
                    stage_index=stage_index,
                    stage_count=stage_count,
                    current_stage_fraction=current_stage_fraction,
                    detail=detail,
                )
        case WorkbenchEventType.RESOURCE_SNAPSHOT_RECORDED:
            state.run.resource_snapshot = dict(event.payload)
            elapsed_seconds = event.payload.get("elapsed_seconds")
            if isinstance(elapsed_seconds, int | float):
                state.run.elapsed_seconds = float(elapsed_seconds)
                if (
                    state.run.current_stage is not None
                    and state.run.status == "running"
                    and not state.run.current_stage_is_complete
                    and state.run.current_stage_started_elapsed_seconds is None
                ):
                    state.run.current_stage_started_elapsed_seconds = 0.0
                if state.run.current_stage_started_elapsed_seconds is not None:
                    state.run.current_stage_elapsed_seconds = max(
                        0.0,
                        state.run.elapsed_seconds - state.run.current_stage_started_elapsed_seconds,
                    )
        case WorkbenchEventType.ALERT_EMITTED:
            state.run.recent_alerts.append(str(event.payload.get("message", "Workbench alert")))
        case WorkbenchEventType.ARTIFACT_RECORDED:
            state.session.status_message = str(event.payload.get("artifact_label", "Artifact recorded"))


__all__ = ["apply_workbench_event"]
