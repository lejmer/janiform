from __future__ import annotations

from ._workflow_progress import WorkflowProgressMode


def native_terminal_progress_mode(progress_mode: WorkflowProgressMode) -> str:
    match progress_mode:
        case WorkflowProgressMode.TERMINAL:
            return "terminal"
        case WorkflowProgressMode.JSONL | WorkflowProgressMode.NONE:
            return "none"
        case WorkflowProgressMode.AUTO:
            return "none"
