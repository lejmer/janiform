import io
from typing import Any

import pytest

import janiform._workflow_progress as workflow_progress_module


def test_spinner_progress_writes_distinct_stage_rows_and_total_summary(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    stream = io.StringIO()
    monotonic_values = iter((0.0, 0.0, 0.0, 65.0, 65.0, 65.0, 70.0, 70.0))

    class _DummyThread:
        def __init__(self, *, target: Any, daemon: bool) -> None:
            self._target = target
            self._daemon = daemon

        def start(self) -> None:
            return None

        def join(self, timeout: float) -> None:
            _ = timeout

    monkeypatch.setattr(workflow_progress_module, "Thread", _DummyThread)
    monkeypatch.setattr(workflow_progress_module, "monotonic", lambda: next(monotonic_values))

    with workflow_progress_module.SpinnerProgress(
        label="fit",
        stream=stream,
        enabled=True,
        total_steps=7,
    ) as progress:
        progress.update(detail="running calibrationinputs")
        progress.update(detail="running calibration")

    rendered_lines = _render_terminal_output(stream.getvalue())

    assert rendered_lines == [
        "✓ fit 1/7 [01:05] running calibrationinputs",
        "✓ fit 2/7 [00:05] running calibration",
        "done fit total [01:10]",
    ]


def test_spinner_progress_renders_determinate_counts_with_spinner_friendly_layout(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    stream = io.StringIO()
    monotonic_values = iter((0.0, 0.0, 0.0, 761.0, 761.0, 761.0, 761.0))

    class _DummyThread:
        def __init__(self, *, target: Any, daemon: bool) -> None:
            self._target = target
            self._daemon = daemon

        def start(self) -> None:
            return None

        def join(self, timeout: float) -> None:
            _ = timeout

    monkeypatch.setattr(workflow_progress_module, "Thread", _DummyThread)
    monkeypatch.setattr(workflow_progress_module, "monotonic", lambda: next(monotonic_values))

    with workflow_progress_module.SpinnerProgress(
        label="simulate",
        stream=stream,
        enabled=True,
    ) as progress:
        progress.update(
            detail="seeded tick path generation",
            completed_units=0,
            total_units=240,
        )
        progress.update(
            detail="seeded tick path generation",
            completed_units=89,
            total_units=240,
        )

    rendered_lines = _render_terminal_output(stream.getvalue())

    assert rendered_lines == [
        "✓ simulate [37% 89/240] seeded tick path generation [12:41, ETA 21:31]",
        "done simulate total [12:41]",
    ]


def _render_terminal_output(raw_output: str) -> list[str]:
    rendered_lines: list[str] = []
    current_line: list[str] = []
    cursor_position = 0

    for character in raw_output:
        if character == "\r":
            cursor_position = 0
            continue

        if character == "\n":
            rendered_lines.append("".join(current_line))
            current_line = []
            cursor_position = 0
            continue

        if cursor_position == len(current_line):
            current_line.append(character)
        else:
            current_line[cursor_position] = character

        cursor_position += 1

    if current_line:
        rendered_lines.append("".join(current_line))

    return rendered_lines
