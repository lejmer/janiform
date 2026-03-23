from .events import WorkbenchEvent, WorkbenchEventType
from .state import WorkbenchAppState, WorkbenchProgressModel


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
        case WorkbenchEventType.STAGE_STARTED:
            state.run.current_stage = event.stage
            state.run.stage_progress = WorkbenchProgressModel(
                label=str(event.payload.get("stage_label", event.stage or "Stage")),
                completed_units=event.payload.get("stage_index"),
                total_units=event.payload.get("stage_count"),
                detail=str(event.payload.get("detail", "Stage started")),
                is_indeterminate=False,
            )
        case WorkbenchEventType.STAGE_COMPLETED:
            state.run.current_stage = event.stage
            state.run.stage_progress = WorkbenchProgressModel(
                label=str(event.payload.get("stage_label", event.stage or "Stage")),
                completed_units=event.payload.get("stage_index"),
                total_units=event.payload.get("stage_count"),
                detail=str(event.payload.get("detail", "Stage completed")),
                is_indeterminate=False,
            )
        case WorkbenchEventType.SUBTASK_STARTED | WorkbenchEventType.SUBTASK_COMPLETED:
            detail = str(event.payload.get("detail", event.subtask or "Working"))
            state.run.subprogress = WorkbenchProgressModel(
                label=event.subtask or "Subtask",
                detail=detail,
                is_indeterminate=True,
            )
            state.session.status_message = detail
        case WorkbenchEventType.RESOURCE_SNAPSHOT_RECORDED:
            state.run.resource_snapshot = dict(event.payload)
            elapsed_seconds = event.payload.get("elapsed_seconds")
            if isinstance(elapsed_seconds, int | float):
                state.run.elapsed_seconds = float(elapsed_seconds)
        case WorkbenchEventType.ALERT_EMITTED:
            state.run.recent_alerts.append(str(event.payload.get("message", "Workbench alert")))
        case WorkbenchEventType.ARTIFACT_RECORDED:
            state.session.status_message = str(event.payload.get("artifact_label", "Artifact recorded"))


__all__ = ["apply_workbench_event"]
