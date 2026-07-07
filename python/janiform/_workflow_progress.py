from enum import StrEnum


class WorkflowProgressMode(StrEnum):
    AUTO = "auto"
    TERMINAL = "terminal"
    JSONL = "jsonl"
    NONE = "none"


def resolve_workflow_progress_mode(
    requested_mode: WorkflowProgressMode | str,
    *,
    default_mode: WorkflowProgressMode | str,
) -> WorkflowProgressMode:
    normalized_requested_mode = WorkflowProgressMode(requested_mode)
    if normalized_requested_mode is not WorkflowProgressMode.AUTO:
        return normalized_requested_mode

    return WorkflowProgressMode(default_mode)


__all__ = [
    "WorkflowProgressMode",
    "resolve_workflow_progress_mode",
]
