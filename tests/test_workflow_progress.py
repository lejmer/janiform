import io
from typing import Any

import pytest

import janiform._workflow_progress as workflow_progress_module
from janiform.calibration.staged._progress_policy import (
    CalibrationLiveProgressSnapshot,
    calibration_total_progress_fraction,
)
from janiform.calibration.staged.progress import (
    NativeStagedCalibrationProgressTracker,
    format_compact_staged_progress_event_detail,
    format_staged_progress_event_detail,
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
    progress_fraction = calibration_total_progress_fraction(
        CalibrationLiveProgressSnapshot(
            status="running",
            stage_sequence=(
                "seasonality",
                "hawkes",
                "return_components",
                "mean_reversion",
                "joint_refinement",
            ),
            stage_index=5,
            stage_count=5,
            current_stage="joint_refinement",
            current_subtask="joint_refinement.restart_optimization",
            current_stage_is_complete=False,
            subtask_completed_units=2,
            subtask_total_units=8,
        )
    )
    assert progress_fraction is not None
    expected_eta_seconds = 25.0 * (1.0 - progress_fraction) / progress_fraction

    assert rendered_lines == [
        (
            "✓ fit 1/8 "
            f"[00:25, ETA {workflow_progress_module._format_elapsed_seconds(expected_eta_seconds)}] "
            f"running calibration [{round(progress_fraction * 100.0)}%] | "
            "joint_refinement restart optimization [12%] (1/8)"
        ),
        "done fit total [00:25]",
    ]


def test_restart_optimization_progress_panel_tracks_active_candidates() -> None:
    tracker = NativeStagedCalibrationProgressTracker()
    tracker.update(
        {
            "event_kind": "stage_progress",
            "stage": "joint_refinement",
            "progress_phase": "restart_optimization",
            "completed_units": 0,
            "total_units": 15,
            "current_restart_id": "halton_04",
            "current_parameter_block": "nig",
            "current_outer_cycle": 1,
            "total_outer_cycles": 3,
            "evaluation_progress_percent": 0,
            "current_evaluation_count": 1,
            "current_weighted_objective": 265255.2,
            "current_restart_best_weighted_objective": 265255.2,
        }
    )
    tracker.update(
        {
            "event_kind": "stage_progress",
            "stage": "joint_refinement",
            "progress_phase": "restart_optimization",
            "completed_units": 0,
            "total_units": 15,
            "current_restart_id": "baseline",
            "current_parameter_block": "hawkes",
            "current_outer_cycle": 2,
            "total_outer_cycles": 3,
            "evaluation_progress_percent": 41,
            "current_evaluation_count": 48,
            "current_weighted_objective": 12991.02,
            "current_restart_best_weighted_objective": 12991.02,
        }
    )

    assert tracker.restart_progress_panel(
        "joint_refinement restart optimization [0%] (0/15)"
    ) == "\n".join(
        (
            "joint_refinement restart optimization [0%] (0/15)",
            "  halton_04              nig        1/3  eval  <1%  obj  265255.20  best  265255.20",
            "  baseline               hawkes     2/3  eval  41%  obj   12991.02  best   12991.02",
        )
    )

    tracker.update(
        {
            "event_kind": "stage_progress",
            "stage": "joint_refinement",
            "progress_phase": "restart_optimization",
            "completed_units": 1,
            "total_units": 15,
            "current_restart_id": "halton_04",
            "evaluation_progress_percent": 100,
        }
    )

    assert tracker.restart_progress_panel(
        "joint_refinement restart optimization [7%] (1/15)"
    ) == "\n".join(
        (
            "joint_refinement restart optimization [7%] (1/15)",
            "  baseline               hawkes     2/3  eval  41%  obj   12991.02  best   12991.02",
        )
    )


def test_restart_optimization_progress_renders_live_candidate_details() -> None:
    raw_event = {
        "event_kind": "stage_progress",
        "stage": "joint_refinement",
        "progress_phase": "restart_optimization",
        "completed_units": 3,
        "total_units": 15,
        "current_restart_id": "halton_09",
        "active_restart_count": 12,
        "current_parameter_block": "nig",
        "current_outer_cycle": 2,
        "total_outer_cycles": 3,
        "evaluation_progress_percent": 47,
        "current_weighted_objective": 265255.2,
        "current_restart_best_weighted_objective": 18422.71,
        "best_weighted_objective": 13001.0,
    }

    assert format_compact_staged_progress_event_detail(
        raw_event,
        completed_units=3,
        total_units=15,
    ) == (
        "joint_refinement restart optimization [20%] (3/15) | item=halton_09 | "
        "running=12 | block=nig | cycle=2/3 | eval=47% | "
        "obj=265255.200000 | candidate_best=18422.710000 | global_best=13001.000000"
    )
    assert format_staged_progress_event_detail(raw_event) == (
        "joint_refinement restart optimization 3/15 | item=halton_09 | "
        "running=12 | block=nig | cycle=2/3 | eval=47% | "
        "objective=265255.200000 | candidate_best_objective=18422.710000 | "
        "best_objective=13001.000000"
    )


def test_spinner_progress_truncates_long_lines_to_terminal_width(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    stream = io.StringIO()
    monotonic_values = iter((0.0, 0.0, 0.0, 39.0, 39.0, 39.0, 39.0))

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
    monkeypatch.setattr(
        workflow_progress_module,
        "_resolve_stream_render_width",
        lambda stream: 80,
    )

    with workflow_progress_module.SpinnerProgress(
        label="fit",
        stream=stream,
        enabled=True,
        total_steps=8,
    ) as progress:
        progress.update(detail="running calibration")
        progress.update(
            detail="running calibration",
            subdetail=(
                "joint_refinement restart optimization [68%] (34/50) | item=baseline | "
                "obj=6.843098 | best_obj=5.315629"
            ),
            progress_fraction=0.64,
        )

    rendered_lines = _render_terminal_output(stream.getvalue())
    expected_eta_seconds = 39.0 * (1.0 - 0.64) / 0.64
    full_stage_line = (
        "✓ fit 1/8 "
        f"[00:39, ETA {workflow_progress_module._format_elapsed_seconds(expected_eta_seconds)}] "
        "running calibration [64%] | joint_refinement restart optimization [68%] "
        "(34/50) | item=baseline | obj=6.843098 | best_obj=5.315629"
    )
    expected_stage_line = f"{full_stage_line[:77]}..."

    assert rendered_lines == [
        expected_stage_line,
        "done fit total [00:39]",
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
