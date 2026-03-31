import json
import sys
from abc import abstractmethod
from contextlib import AbstractContextManager
from dataclasses import dataclass, field
from datetime import UTC, datetime
from enum import StrEnum
from threading import Event, Lock, Thread
from time import monotonic, sleep
from typing import Optional, TextIO, override


_SPINNER_FRAMES: tuple[str, ...] = ("-", "\\", "|", "/")
_COMPLETED_STAGE_MARKER = "✓"


def _format_elapsed_seconds(elapsed_seconds: float) -> str:
    whole_seconds = max(0, int(elapsed_seconds))
    minutes, seconds = divmod(whole_seconds, 60)
    hours, minutes = divmod(minutes, 60)
    if hours > 0:
        return f"{hours:02d}:{minutes:02d}:{seconds:02d}"

    return f"{minutes:02d}:{seconds:02d}"


class WorkflowProgressMode(StrEnum):
    AUTO = "auto"
    HUMAN = "human"
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


class AbstractProgress(AbstractContextManager[None]):
    @abstractmethod
    def update(
        self,
        *,
        detail: Optional[str] = None,
        completed_units: Optional[int] = None,
        total_units: Optional[int] = None,
    ) -> None:
        raise NotImplementedError


class NullProgress(AbstractProgress):
    @override
    def update(
        self,
        *,
        detail: Optional[str] = None,
        completed_units: Optional[int] = None,
        total_units: Optional[int] = None,
    ) -> None:
        _ = detail
        _ = completed_units
        _ = total_units

    def __exit__(self, exc_type, exc, exc_tb) -> None:
        return None


class SpinnerProgress(AbstractProgress):
    def __init__(
        self,
        *,
        label: str,
        stream: TextIO,
        enabled: bool,
        total_steps: Optional[int] = None,
    ) -> None:
        self._label: str = label
        self._stream: TextIO = stream
        self._enabled: bool = enabled
        self._total_steps: Optional[int] = total_steps
        self._detail: Optional[str] = None
        self._completed_units: Optional[int] = None
        self._total_units: Optional[int] = None
        self._stage_started_at: Optional[float] = None
        self._current_step_index: int = 0
        self._lock = Lock()
        self._last_rendered_width: int = 0
        self._stop_event = Event()
        self._render_thread: Optional[Thread] = None
        self._total_started_at: float = monotonic()

        if self._enabled:
            self._render_thread = Thread(target=self._render_loop, daemon=True)
            self._render_thread.start()

    @override
    def update(
        self,
        *,
        detail: Optional[str] = None,
        completed_units: Optional[int] = None,
        total_units: Optional[int] = None,
    ) -> None:
        if not self._enabled:
            return

        if detail is None and completed_units is None and total_units is None:
            return

        with self._lock:
            next_detail = self._detail if detail is None else detail
            detail_changed = next_detail != self._detail

            if detail_changed:
                self._finalize_current_stage_locked(status="done")
                self._detail = next_detail
                self._completed_units = completed_units
                self._total_units = total_units
                self._current_step_index += 1
                self._stage_started_at = monotonic()
                self._render_active_stage_locked(frame=_SPINNER_FRAMES[0])
                return

            if completed_units is not None:
                self._completed_units = completed_units
            if total_units is not None:
                self._total_units = total_units

            if self._detail is not None and self._stage_started_at is not None:
                self._render_active_stage_locked(frame=_SPINNER_FRAMES[0])

    def _render_loop(self) -> None:
        frame_index = 0
        while not self._stop_event.is_set():
            with self._lock:
                if self._detail is not None and self._stage_started_at is not None:
                    self._render_active_stage_locked(
                        frame=_SPINNER_FRAMES[frame_index % len(_SPINNER_FRAMES)]
                    )

            frame_index += 1

            sleep(0.1)

    def __exit__(self, exc_type, exc, exc_tb) -> None:
        if not self._enabled:
            return None

        self._stop_event.set()
        if self._render_thread is not None:
            self._render_thread.join(timeout=1.0)

        total_elapsed = _format_elapsed_seconds(monotonic() - self._total_started_at)

        with self._lock:
            self._finalize_current_stage_locked(
                status="done" if exc_type is None else "failed"
            )
            workflow_status = "done" if exc_type is None else "failed"
            self._stream.write(f"{workflow_status} {self._label} total [{total_elapsed}]\n")
            self._stream.flush()

        return None

    def _finalize_current_stage_locked(self, *, status: str) -> None:
        if self._detail is None or self._stage_started_at is None:
            return

        line = self._build_stage_line(
            indicator=_COMPLETED_STAGE_MARKER if status == "done" else status,
            elapsed_seconds=monotonic() - self._stage_started_at,
            detail=self._detail,
        )
        self._write_final_line_locked(line)
        self._detail = None
        self._completed_units = None
        self._total_units = None
        self._stage_started_at = None

    def _render_active_stage_locked(self, *, frame: str) -> None:
        if self._detail is None or self._stage_started_at is None:
            return

        line = self._build_stage_line(
            indicator=frame,
            elapsed_seconds=monotonic() - self._stage_started_at,
            detail=self._detail,
        )
        self._write_transient_line_locked(line)

    def _build_stage_line(
        self,
        *,
        indicator: str,
        elapsed_seconds: float,
        detail: str,
    ) -> str:
        progress_segment = self._progress_segment()
        timing_segment = self._timing_segment(elapsed_seconds)
        if progress_segment:
            return (
                f"{indicator} {self._label}{self._step_segment()}{progress_segment} "
                f"{detail} {timing_segment}"
            )

        return f"{indicator} {self._label}{self._step_segment()} {timing_segment} {detail}"

    def _progress_segment(self) -> str:
        if self._completed_units is None or self._total_units is None or self._total_units <= 0:
            return ""

        progress_fraction = self._completed_units / self._total_units
        progress_percent = round(progress_fraction * 100.0)
        return f" [{progress_percent}% {self._completed_units}/{self._total_units}]"

    def _timing_segment(self, elapsed_seconds: float) -> str:
        elapsed = _format_elapsed_seconds(elapsed_seconds)
        if (
            self._completed_units is None
            or self._total_units is None
            or self._total_units <= 0
            or self._completed_units <= 0
            or self._completed_units >= self._total_units
            or elapsed_seconds <= 0.0
        ):
            return f"[{elapsed}]"

        rate_units_per_second = self._completed_units / elapsed_seconds
        remaining_units = self._total_units - self._completed_units
        eta_seconds = remaining_units / rate_units_per_second
        return f"[{elapsed}, ETA {_format_elapsed_seconds(eta_seconds)}]"

    def _step_segment(self) -> str:
        if self._total_steps is None:
            return ""

        return f" {self._current_step_index}/{self._total_steps}"

    def _write_transient_line_locked(self, line: str) -> None:
        padding = " " * max(0, self._last_rendered_width - len(line))
        self._stream.write(f"\r{line}{padding}")
        self._stream.flush()
        self._last_rendered_width = len(line)

    def _write_final_line_locked(self, line: str) -> None:
        padding = " " * max(0, self._last_rendered_width - len(line))
        self._stream.write(f"\r{line}{padding}\n")
        self._stream.flush()
        self._last_rendered_width = 0


class JsonLinesProgress(AbstractProgress):
    def __init__(
        self,
        *,
        label: str,
        stream: TextIO,
        total_steps: Optional[int] = None,
    ) -> None:
        self._label: str = label
        self._stream: TextIO = stream
        self._total_steps: Optional[int] = total_steps
        self._detail: Optional[str] = None
        self._completed_units: Optional[int] = None
        self._total_units: Optional[int] = None
        self._stage_started_at: Optional[float] = None
        self._current_step_index: int = 0
        self._total_started_at: float = monotonic()

    @override
    def update(
        self,
        *,
        detail: Optional[str] = None,
        completed_units: Optional[int] = None,
        total_units: Optional[int] = None,
    ) -> None:
        if detail is None and completed_units is None and total_units is None:
            return

        next_detail = self._detail if detail is None else detail
        detail_changed = next_detail != self._detail

        if detail_changed:
            self._detail = next_detail
            self._completed_units = completed_units
            self._total_units = total_units
            self._current_step_index += 1
            self._stage_started_at = monotonic()
        else:
            if completed_units is not None:
                self._completed_units = completed_units
            if total_units is not None:
                self._total_units = total_units

        if self._detail is None or self._stage_started_at is None:
            return

        elapsed_seconds = monotonic() - self._stage_started_at
        payload = {
            "event": "workflow_progress",
            "label": self._label,
            "step_index": self._current_step_index,
            "step_count": self._total_steps,
            "detail": self._detail,
            "completed_units": self._completed_units,
            "total_units": self._total_units,
            "elapsed_seconds": elapsed_seconds,
            "eta_seconds": _estimated_remaining_seconds(
                elapsed_seconds,
                self._completed_units,
                self._total_units,
            ),
        }
        self._stream.write(json.dumps(payload, sort_keys=True) + "\n")
        self._stream.flush()

    def __exit__(self, exc_type, exc, exc_tb) -> None:
        payload = {
            "event": "workflow_completed",
            "label": self._label,
            "status": "done" if exc_type is None else "failed",
            "elapsed_seconds": monotonic() - self._total_started_at,
        }
        self._stream.write(json.dumps(payload, sort_keys=True) + "\n")
        self._stream.flush()
        return None


def _estimated_remaining_seconds(
    elapsed_seconds: float,
    completed_units: Optional[int],
    total_units: Optional[int],
) -> Optional[float]:
    if (
        completed_units is None
        or total_units is None
        or total_units <= 0
        or completed_units <= 0
        or completed_units >= total_units
        or elapsed_seconds <= 0.0
    ):
        return None

    rate_units_per_second = completed_units / elapsed_seconds
    remaining_units = total_units - completed_units
    return remaining_units / rate_units_per_second


@dataclass(frozen=True, slots=True)
class JaniformOutputController:
    suppress_info_output: bool = False
    progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO
    default_progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.HUMAN
    stdout: TextIO = field(default_factory=lambda: sys.stdout)
    stderr: TextIO = field(default_factory=lambda: sys.stderr)

    def info(self, message: str) -> None:
        if self.suppress_info_output:
            return

        self.stdout.write(f"{message}\n")
        self.stdout.flush()

    def warning(self, message: str) -> None:
        timestamp = datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%SZ")

        self.stderr.write(f"[warning {timestamp}] {message}\n")
        self.stderr.flush()

    def progress(self, label: str, *, total_steps: Optional[int] = None) -> AbstractProgress:
        resolved_mode = resolve_workflow_progress_mode(
            self.progress_mode,
            default_mode=self.default_progress_mode,
        )
        match resolved_mode:
            case WorkflowProgressMode.NONE:
                return NullProgress()
            case WorkflowProgressMode.JSONL:
                return JsonLinesProgress(
                    label=label,
                    stream=self.stderr,
                    total_steps=total_steps,
                )
            case WorkflowProgressMode.HUMAN:
                return SpinnerProgress(
                    label=label,
                    stream=self.stderr,
                    enabled=True,
                    total_steps=total_steps,
                )
            case WorkflowProgressMode.AUTO:
                return NullProgress()


__all__ = [
    "JaniformOutputController",
    "WorkflowProgressMode",
    "resolve_workflow_progress_mode",
]
