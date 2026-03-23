from dataclasses import dataclass
from datetime import UTC, datetime
from enum import StrEnum
from typing import Any, Optional
from uuid import uuid4


class WorkbenchEventType(StrEnum):
    RUN_STARTED = "run.started"
    RUN_COMPLETED = "run.completed"
    RUN_FAILED = "run.failed"
    STAGE_STARTED = "stage.started"
    STAGE_COMPLETED = "stage.completed"
    SUBTASK_STARTED = "subtask.started"
    SUBTASK_COMPLETED = "subtask.completed"
    RESOURCE_SNAPSHOT_RECORDED = "resource.snapshot_recorded"
    ALERT_EMITTED = "alert.emitted"
    ARTIFACT_RECORDED = "artifact.recorded"


@dataclass(frozen=True, slots=True)
class WorkbenchEvent:
    event_id: str
    event_type: WorkbenchEventType
    occurred_at_utc: str
    run_id: str
    sequence_number: int
    stage: Optional[str]
    subtask: Optional[str]
    evaluation_tier: Optional[str]
    payload: dict[str, Any]


def build_workbench_event(
    *,
    event_type: WorkbenchEventType,
    run_id: str,
    sequence_number: int,
    payload: dict[str, Any],
    stage: Optional[str] = None,
    subtask: Optional[str] = None,
    evaluation_tier: Optional[str] = None,
) -> WorkbenchEvent:
    return WorkbenchEvent(
        event_id=f"workbench_event_{uuid4().hex}",
        event_type=event_type,
        occurred_at_utc=datetime.now(tz=UTC).isoformat(),
        run_id=run_id,
        sequence_number=sequence_number,
        stage=stage,
        subtask=subtask,
        evaluation_tier=evaluation_tier,
        payload=payload,
    )


__all__ = [
    "WorkbenchEvent",
    "WorkbenchEventType",
    "build_workbench_event",
]
