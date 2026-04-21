import json
import os
import shutil
import sys
from abc import abstractmethod
from contextlib import AbstractContextManager
from dataclasses import dataclass, field
from datetime import UTC, datetime
from enum import StrEnum
from io import UnsupportedOperation
from threading import Event, Lock, Thread
from time import monotonic
from typing import Final, Optional, TextIO, override


_SPINNER_FRAMES: Final[tuple[str, ...]] = ("-", "\\", "|", "/")
_COMPLETED_STAGE_MARKER: Final[str] = "✓"
_ELLIPSIS: Final[str]  = "..."


def format_elapsed_seconds(elapsed_seconds: float) -> str:
    whole_seconds = max(0, int(elapsed_seconds))
    minutes, seconds = divmod(whole_seconds, 60)
    hours, minutes = divmod(minutes, 60)

    if hours > 0:
        return f"{hours:02d}:{minutes:02d}:{seconds:02d}"

    return f"{minutes:02d}:{seconds:02d}"


def _resolve_stream_render_width(stream: TextIO) -> Optional[int]:
    terminal_size = shutil.get_terminal_size(fallback=(0, 0))
    if terminal_size.columns <= 0:
        return None

    return terminal_size.columns


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
        subdetail: Optional[str] = None,
        completed_units: Optional[int] = None,
        total_units: Optional[int] = None,
        reset_progress_units: bool = False,
        progress_fraction: Optional[float] = None,
    ) -> None:
        raise NotImplementedError


class NullProgress(AbstractProgress):
    @override
    def update(
        self,
        *,
        detail: Optional[str] = None,
        subdetail: Optional[str] = None,
        completed_units: Optional[int] = None,
        total_units: Optional[int] = None,
        reset_progress_units: bool = False,
        progress_fraction: Optional[float] = None,
    ) -> None:
        _ = detail
        _ = subdetail
        _ = completed_units
        _ = total_units
        _ = reset_progress_units
        _ = progress_fraction

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
        self._subdetail: Optional[str] = None
        self._completed_units: Optional[int] = None
        self._total_units: Optional[int] = None
        self._progress_fraction: Optional[float] = None
        self._stage_started_at: Optional[float] = None
        self._current_step_index: int = 0
        self._lock = Lock()
        self._last_rendered_width: int = 0
        self._last_rendered_line_count: int = 0
        self._stop_event = Event()
        self._render_requested = Event()
        self._pending_final_lines: list[str] = []
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
        subdetail: Optional[str] = None,
        completed_units: Optional[int] = None,
        total_units: Optional[int] = None,
        reset_progress_units: bool = False,
        progress_fraction: Optional[float] = None,
    ) -> None:
        if not self._enabled:
            return

        if (
            detail is None
            and subdetail is None
            and completed_units is None
            and total_units is None
            and not reset_progress_units
            and progress_fraction is None
        ):
            return

        with self._lock:
            next_detail = self._detail if detail is None else detail
            detail_changed = next_detail != self._detail

            if detail_changed:
                self._queue_current_stage_finalization_locked(status="done")
                self._detail = next_detail
                self._subdetail = subdetail
                self._completed_units = completed_units
                self._total_units = total_units
                self._progress_fraction = progress_fraction
                self._current_step_index += 1
                self._stage_started_at = monotonic()
                self._request_render_locked(frame=_SPINNER_FRAMES[0])
                return

            state_changed = False
            if subdetail is not None:
                if subdetail != self._subdetail:
                    self._subdetail = subdetail
                    state_changed = True

            if reset_progress_units:
                if self._completed_units is not None or self._total_units is not None:
                    self._completed_units = None
                    self._total_units = None
                    state_changed = True

            if progress_fraction is not None:
                if progress_fraction != self._progress_fraction:
                    self._progress_fraction = progress_fraction
                    state_changed = True

            if completed_units is not None:
                if completed_units != self._completed_units:
                    self._completed_units = completed_units
                    state_changed = True

            if total_units is not None:
                if total_units != self._total_units:
                    self._total_units = total_units
                    state_changed = True

            if state_changed and self._detail is not None and self._stage_started_at is not None:
                self._request_render_locked(frame=_SPINNER_FRAMES[0])

    def _render_loop(self) -> None:
        frame_index = 0
        while not self._stop_event.is_set():
            render_requested = self._render_requested.wait(timeout=0.1)

            self._render_requested.clear()

            with self._lock:
                if not render_requested and not self._should_animate_locked():
                    continue

                self._drain_pending_output_locked(
                    frame=_SPINNER_FRAMES[frame_index % len(_SPINNER_FRAMES)]
                )

            frame_index += 1

    def __exit__(self, exc_type, exc, exc_tb) -> None:
        if not self._enabled:
            return None

        self._stop_event.set()
        self._render_requested.set()

        if self._render_thread is not None:
            self._render_thread.join(timeout=1.0)

        total_elapsed = format_elapsed_seconds(monotonic() - self._total_started_at)

        with self._lock:
            self._flush_pending_final_lines_locked()
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
        self._subdetail = None
        self._completed_units = None
        self._total_units = None
        self._progress_fraction = None
        self._stage_started_at = None

    def _queue_current_stage_finalization_locked(self, *, status: str) -> None:
        if self._detail is None or self._stage_started_at is None:
            return

        self._pending_final_lines.append(
            self._build_stage_line(
                indicator=_COMPLETED_STAGE_MARKER if status == "done" else status,
                elapsed_seconds=monotonic() - self._stage_started_at,
                detail=self._detail,
            )
        )

        self._detail = None
        self._subdetail = None
        self._completed_units = None
        self._total_units = None
        self._progress_fraction = None
        self._stage_started_at = None

    def _render_active_stage_locked(self, *, frame: str) -> None:
        if self._detail is None or self._stage_started_at is None:
            return

        lines = self._build_stage_lines(
            indicator=frame,
            elapsed_seconds=monotonic() - self._stage_started_at,
            detail=self._detail,
        )

        self._write_transient_lines_locked(lines)

    def _flush_pending_final_lines_locked(self) -> None:
        while self._pending_final_lines:
            self._write_final_line_locked(self._pending_final_lines.pop(0))

    def _drain_pending_output_locked(self, *, frame: str) -> None:
        self._flush_pending_final_lines_locked()
        if self._detail is not None and self._stage_started_at is not None:
            self._render_active_stage_locked(frame=frame)

    def _request_render_locked(self, *, frame: str) -> None:
        self._render_requested.set()
        if not self._background_render_available():
            self._drain_pending_output_locked(frame=frame)

    def _background_render_available(self) -> bool:
        is_alive = getattr(self._render_thread, "is_alive", None)
        return callable(is_alive) and bool(is_alive())

    def _build_stage_lines(
        self,
        *,
        indicator: str,
        elapsed_seconds: float,
        detail: str,
    ) -> list[str]:
        line = self._build_stage_line(
            indicator=indicator,
            elapsed_seconds=elapsed_seconds,
            detail=detail,
        )
        if self._subdetail is None:
            return [line]

        subdetail_lines = self._subdetail.splitlines()
        if len(subdetail_lines) <= 1:
            return [line]

        return [line, *subdetail_lines[1:]]

    def _build_stage_line(
        self,
        *,
        indicator: str,
        elapsed_seconds: float,
        detail: str,
    ) -> str:
        progress_segment = self._progress_segment()
        timing_segment = self._timing_segment(elapsed_seconds)
        subdetail_summary = self._subdetail_summary()

        if self._progress_fraction is not None:
            detail_segment = (
                f"{detail}{progress_segment}"
                if progress_segment
                else detail
            )
            if subdetail_summary is not None:
                detail_segment = f"{detail_segment} | {subdetail_summary}"

            return (
                f"{indicator} {self._label}{self._step_segment()} "
                f"{timing_segment} {detail_segment}"
            )

        detail_segment = (
            detail
            if subdetail_summary is None
            else f"{detail} | {subdetail_summary}"
        )
        if progress_segment:
            return (
                f"{indicator} {self._label}{self._step_segment()}{progress_segment} "
                f"{detail_segment} {timing_segment}"
            )

        return f"{indicator} {self._label}{self._step_segment()} {timing_segment} {detail_segment}"

    def _subdetail_summary(self) -> Optional[str]:
        if self._subdetail is None:
            return None

        return self._subdetail.splitlines()[0]

    def _progress_segment(self) -> str:
        if self._progress_fraction is not None:
            progress_percent = round(
                min(max(self._progress_fraction, 0.0), 1.0) * 100.0
            )

            return f" [{progress_percent}%]"

        if self._completed_units is None or self._total_units is None or self._total_units <= 0:
            return ""

        progress_fraction = self._completed_units / self._total_units
        progress_percent = round(progress_fraction * 100.0)

        return f" [{progress_percent}% {self._completed_units}/{self._total_units}]"

    def _timing_segment(self, elapsed_seconds: float) -> str:
        elapsed = format_elapsed_seconds(elapsed_seconds)

        if (
            self._progress_fraction is not None
            and 0.0 < self._progress_fraction < 1.0
            and elapsed_seconds > 0.0
        ):
            eta_seconds = elapsed_seconds * (1.0 - self._progress_fraction) / self._progress_fraction
            return f"[{elapsed}, ETA {format_elapsed_seconds(eta_seconds)}]"

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

        return f"[{elapsed}, ETA {format_elapsed_seconds(eta_seconds)}]"

    def _step_segment(self) -> str:
        if self._total_steps is None:
            return ""

        return f" {self._current_step_index}/{self._total_steps}"

    def _write_transient_line_locked(self, line: str) -> None:
        rendered_line = self._fit_line_to_stream_width(line)

        padding = " " * max(0, self._last_rendered_width - len(rendered_line))

        self._stream.write(f"\r{rendered_line}{padding}")
        self._stream.flush()

        self._last_rendered_width = len(rendered_line)
        self._last_rendered_line_count = 1

    def _write_transient_lines_locked(self, lines: list[str]) -> None:
        if not self._supports_cursor_control():
            if len(lines) == 1:
                self._write_transient_line_locked(lines[0])
                return

            self._write_multiline_snapshot_locked(lines)
            return

        rendered_lines = [self._fit_line_to_stream_width(line) for line in lines]
        self._rewrite_cursor_control_lines_locked(rendered_lines)

    def _write_multiline_snapshot_locked(self, lines: list[str]) -> None:
        rendered_lines = [self._fit_line_to_stream_width(line) for line in lines]
        if self._last_rendered_width > 0:
            first_line = rendered_lines[0]
            padding = " " * max(0, self._last_rendered_width - len(first_line))

            self._stream.write(f"\r{first_line}{padding}\n")

            if len(rendered_lines) > 1:
                self._stream.write("\n".join(rendered_lines[1:]))
        else:
            self._stream.write("\n".join(rendered_lines))

        self._stream.write("\n")
        self._stream.flush()

        self._last_rendered_width = 0
        self._last_rendered_line_count = 0

    def _write_final_line_locked(self, line: str) -> None:
        rendered_line = self._fit_line_to_stream_width(line)

        if self._supports_cursor_control():
            self._rewrite_cursor_control_lines_locked([rendered_line])
            self._stream.write("\n")
            self._stream.flush()
            self._last_rendered_width = 0
            self._last_rendered_line_count = 0

            return

        padding = " " * max(0, self._last_rendered_width - len(rendered_line))

        self._stream.write(f"\r{rendered_line}{padding}\n")
        self._stream.flush()

        self._last_rendered_width = 0
        self._last_rendered_line_count = 0

    def _rewrite_cursor_control_lines_locked(self, rendered_lines: list[str]) -> None:
        if self._last_rendered_line_count > 1:
            self._stream.write(f"\x1b[{self._last_rendered_line_count - 1}A\r")
        else:
            self._stream.write("\r")

        for line_number, rendered_line in enumerate(rendered_lines):
            self._stream.write("\x1b[2K")
            self._stream.write(rendered_line)

            if line_number + 1 < len(rendered_lines):
                self._stream.write("\n")

        if self._last_rendered_line_count > len(rendered_lines):
            extra_line_count = self._last_rendered_line_count - len(rendered_lines)
            for _ in range(extra_line_count):
                self._stream.write("\n\x1b[2K")

            self._stream.write(f"\x1b[{extra_line_count}A\r")

        self._stream.flush()

        self._last_rendered_width = len(rendered_lines[-1])
        self._last_rendered_line_count = len(rendered_lines)

    def _fit_line_to_stream_width(self, line: str) -> str:
        render_width = _resolve_stream_render_width(self._stream)
        if render_width is None or render_width <= 1:
            return line

        safe_render_width = render_width - 1
        if len(line) <= safe_render_width:
            return line

        if safe_render_width <= len(_ELLIPSIS):
            return line[:safe_render_width]

        return f"{line[: safe_render_width - len(_ELLIPSIS)]}{_ELLIPSIS}"

    def _supports_cursor_control(self) -> bool:
        is_tty = getattr(self._stream, "isatty", None)
        if callable(is_tty):
            try:
                if is_tty():
                    return True
            except OSError:
                return False

        fileno = getattr(self._stream, "fileno", None)
        if not callable(fileno):
            return False

        try:
            return os.isatty(fileno())
        except (OSError, UnsupportedOperation, ValueError):
            return False

    def _should_animate_locked(self) -> bool:
        if self._detail is None or self._stage_started_at is None:
            return False

        if (
            self._subdetail is not None
            and len(self._subdetail.splitlines()) > 1
            and not self._supports_cursor_control()
        ):
            return False

        return True


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
        self._subdetail: Optional[str] = None
        self._completed_units: Optional[int] = None
        self._total_units: Optional[int] = None
        self._progress_fraction: Optional[float] = None
        self._stage_started_at: Optional[float] = None
        self._current_step_index: int = 0
        self._total_started_at: float = monotonic()

    @override
    def update(
        self,
        *,
        detail: Optional[str] = None,
        subdetail: Optional[str] = None,
        completed_units: Optional[int] = None,
        total_units: Optional[int] = None,
        reset_progress_units: bool = False,
        progress_fraction: Optional[float] = None,
    ) -> None:
        if (
            detail is None
            and subdetail is None
            and completed_units is None
            and total_units is None
            and not reset_progress_units
            and progress_fraction is None
        ):
            return

        next_detail = self._detail if detail is None else detail
        detail_changed = next_detail != self._detail

        if detail_changed:
            self._detail = next_detail
            self._subdetail = subdetail
            self._completed_units = completed_units
            self._total_units = total_units
            self._progress_fraction = progress_fraction
            self._current_step_index += 1
            self._stage_started_at = monotonic()
        else:
            if subdetail is not None:
                self._subdetail = subdetail

            if reset_progress_units:
                self._completed_units = None
                self._total_units = None

            if progress_fraction is not None:
                self._progress_fraction = progress_fraction

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
            "subdetail": self._subdetail,
            "completed_units": self._completed_units,
            "total_units": self._total_units,
            "progress_fraction": self._progress_fraction,
            "elapsed_seconds": elapsed_seconds,
            "eta_seconds": _estimated_remaining_seconds(
                elapsed_seconds,
                self._completed_units,
                self._total_units,
                progress_fraction=self._progress_fraction,
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
    *,
    progress_fraction: Optional[float] = None,
) -> Optional[float]:
    if (
        progress_fraction is not None
        and 0.0 < progress_fraction < 1.0
        and elapsed_seconds > 0.0
    ):
        return elapsed_seconds * (1.0 - progress_fraction) / progress_fraction

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
