import sys
from abc import abstractmethod
from contextlib import AbstractContextManager
from dataclasses import dataclass, field
from datetime import UTC, datetime
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


class AbstractProgress(AbstractContextManager[None]):
    @abstractmethod
    def update(self, *, detail: Optional[str] = None) -> None:
        raise NotImplementedError


class NullProgress(AbstractProgress):
    @override
    def update(self, *, detail: Optional[str] = None) -> None:
        _ = detail

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
    def update(self, *, detail: Optional[str] = None) -> None:
        if not self._enabled:
            return

        if detail is None:
            return

        with self._lock:
            if detail == self._detail:
                return

            self._finalize_current_stage_locked(status="done")
            self._detail = detail
            self._current_step_index += 1
            self._stage_started_at = monotonic()
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
        elapsed = _format_elapsed_seconds(elapsed_seconds)
        return f"{indicator} {self._label}{self._step_segment()} [{elapsed}] {detail}"

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


@dataclass(frozen=True, slots=True)
class JaniformOutputController:
    silent: bool = False
    stdout: TextIO = field(default_factory=lambda: sys.stdout)
    stderr: TextIO = field(default_factory=lambda: sys.stderr)

    def info(self, message: str) -> None:
        if self.silent:
            return

        self.stdout.write(f"{message}\n")
        self.stdout.flush()

    def warning(self, message: str) -> None:
        timestamp = datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%SZ")

        self.stderr.write(f"[warning {timestamp}] {message}\n")
        self.stderr.flush()

    def progress(self, label: str, *, total_steps: Optional[int] = None) -> AbstractProgress:
        if self.silent:
            return NullProgress()

        return SpinnerProgress(
            label=label,
            stream=self.stdout,
            enabled=True,
            total_steps=total_steps,
        )


__all__ = ["JaniformOutputController"]
