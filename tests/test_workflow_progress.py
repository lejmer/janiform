import io
from typing import Any

import pytest

import janiform._workflow_progress as workflow_progress_module
from janiform.calibration.staged.progress import (
    NativeStagedCalibrationProgressTracker,
    update_cli_progress_from_staged_native_event,
)


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


def test_spinner_progress_keeps_staged_subtasks_within_running_calibration_step(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    stream = io.StringIO()
    monotonic_values = iter((0.0, 0.0, 0.0, 15.0, 20.0, 25.0, 25.0))

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
        total_steps=8,
    ) as progress:
        tracker = NativeStagedCalibrationProgressTracker()
        progress.update(detail="running calibration")
        update_cli_progress_from_staged_native_event(
            progress,
            tracker,
            {
                "event_kind": "run_started",
                "stage_count": 5,
                "stage_sequence": [
                    "seasonality",
                    "hawkes",
                    "return_components",
                    "mean_reversion",
                    "joint_refinement",
                ],
            },
        )
        update_cli_progress_from_staged_native_event(
            progress,
            tracker,
            {
                "event_kind": "stage_started",
                "stage": "joint_refinement",
                "stage_index": 5,
                "stage_count": 5,
            },
        )
        update_cli_progress_from_staged_native_event(
            progress,
            tracker,
            {
                "event_kind": "stage_progress",
                "stage": "joint_refinement",
                "progress_phase": "restart_optimization",
                "completed_units": 2,
                "total_units": 8,
                "current_item_label": "halton_03",
            },
        )

    rendered_lines = _render_terminal_output(stream.getvalue())

    assert rendered_lines == [
        (
            "✓ fit 1/8 [00:25, ETA 01:17] running calibration [24%] | "
            "joint_refinement restart optimization [12%] (1/8) | item=halton_03"
        ),
        "done fit total [00:25]",
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
