import io
import os
from typing import Any, Optional

import pytest

import janiform._workflow_progress as workflow_progress_module
from janiform.calibration.staged._progress_policy import (
    CalibrationLiveProgressSnapshot,
    calibration_total_progress_fraction,
)
from janiform.calibration.staged.progress import (
    ActiveCandidateProgressRow,
    NativeStagedCalibrationProgressTracker,
    format_active_candidate_progress_panel_row,
    format_compact_staged_progress_event_detail,
    format_staged_progress_event_detail,
    maximum_active_candidate_progress_row_id_width,
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
                "stage_progress_percent": 31,
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
            stage_progress_fraction_override=0.31,
        )
    )
    assert progress_fraction is not None
    first_expected_eta_seconds = 20.0 * (1.0 - progress_fraction) / progress_fraction
    expected_eta_seconds = 25.0 * (1.0 - progress_fraction) / progress_fraction

    assert rendered_lines == [
        (
            "- fit 1/8 "
            f"[00:20, ETA {workflow_progress_module.format_elapsed_seconds(first_expected_eta_seconds)}] "
            f"running calibration [{round(progress_fraction * 100.0)}%] | "
            "joint_refinement restart optimization [12%] (1/8)"
        ),
        "  halton_03              -            -  eval    -  obj          -  best          -",
        (
            "✓ fit 1/8 "
            f"[00:25, ETA {workflow_progress_module.format_elapsed_seconds(expected_eta_seconds)}] "
            f"running calibration [{round(progress_fraction * 100.0)}%] | "
            "joint_refinement restart optimization [12%] (1/8)"
        ),
        "done fit total [00:25]",
    ]


def test_staged_total_progress_does_not_decrease_when_return_components_phase_resets() -> None:
    tracker = NativeStagedCalibrationProgressTracker()
    tracker.update(
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
        }
    )
    tracker.update(
        {
            "event_kind": "stage_started",
            "stage": "return_components",
            "stage_index": 3,
            "stage_count": 5,
        }
    )

    completed_phase_snapshot = tracker.update(
        {
            "event_kind": "stage_progress",
            "stage": "return_components",
            "stage_index": 3,
            "stage_count": 5,
            "progress_phase": "signal_partition_fit",
            "completed_units": 1,
            "total_units": 1,
        }
    )
    completed_phase_fraction = calibration_total_progress_fraction(completed_phase_snapshot)

    reset_phase_snapshot = tracker.update(
        {
            "event_kind": "stage_progress",
            "stage": "return_components",
            "stage_index": 3,
            "stage_count": 5,
            "progress_phase": "surprise_episode_selection_fit",
            "completed_units": 0,
            "total_units": 1,
        }
    )
    reset_phase_fraction = calibration_total_progress_fraction(reset_phase_snapshot)

    assert completed_phase_fraction is not None
    assert reset_phase_fraction is not None
    assert reset_phase_fraction == completed_phase_fraction


def test_staged_total_progress_uses_native_stage_progress_percent() -> None:
    tracker = NativeStagedCalibrationProgressTracker()
    tracker.update(
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
        }
    )
    tracker.update(
        {
            "event_kind": "stage_started",
            "stage": "joint_refinement",
            "stage_index": 5,
            "stage_count": 5,
        }
    )

    restart_snapshot = tracker.update(
        {
            "event_kind": "stage_progress",
            "stage": "joint_refinement",
            "stage_index": 5,
            "stage_count": 5,
            "progress_phase": "restart_optimization",
            "completed_units": 18,
            "total_units": 22,
            "aggregate_progress_percent": 2,
            "stage_progress_percent": 31,
            "current_item_label": "baseline",
            "current_restart_id": "baseline",
        }
    )
    restart_fraction = calibration_total_progress_fraction(restart_snapshot)

    assert restart_snapshot.stage_progress_fraction_override == 0.31
    assert restart_fraction is not None
    assert round(restart_fraction * 100.0) == 36


def test_spinner_progress_ignores_native_run_completed_subdetail_to_avoid_duplicate_final_row(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    stream = io.StringIO()
    monotonic_values = iter((0.0, 0.0, 0.0, 25.0, 25.0, 25.0, 25.0, 25.0))

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
                "event_kind": "stage_progress",
                "stage": "joint_refinement",
                "progress_phase": "final_assembly",
                "completed_units": 3,
                "total_units": 3,
                "current_item_label": "simulation_summaries",
            },
        )
        update_cli_progress_from_staged_native_event(
            progress,
            tracker,
            {
                "event_kind": "run_completed",
                "stage_count": 5,
            },
        )

    rendered_lines = _render_terminal_output(stream.getvalue())

    assert rendered_lines == [
        (
            "✓ fit 1/8 [00:25] running calibration [100%] | "
            "joint_refinement final assembly [100%] (3/3) | item=simulation_summaries"
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
            "completed_outer_cycles": 0,
            "total_outer_cycles": 3,
            "evaluation_progress_percent": 0,
            "aggregate_progress_percent": 0,
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
            "completed_outer_cycles": 1,
            "total_outer_cycles": 3,
            "evaluation_progress_percent": 41,
            "aggregate_progress_percent": 20,
            "current_evaluation_count": 48,
            "current_weighted_objective": 12991.02,
            "current_restart_best_weighted_objective": 12991.02,
            "best_restart_id": "baseline",
            "best_weighted_objective": 12991.02,
        }
    )

    assert tracker.active_candidate_progress_panel(
        "joint_refinement restart optimization [20%] | best=baseline | best_obj=12991.020000"
    ) == "\n".join(
        (
            "joint_refinement restart optimization [20%] | best=baseline | best_obj=12991.020000",
            "  halton_04              nig        0/3  eval  <1%  obj  265255.20  best  265255.20",
            "  baseline               hawkes     1/3  eval  41%  obj   12991.02  best   12991.02",
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
            "aggregate_progress_percent": 27,
            "best_restart_id": "baseline",
            "best_weighted_objective": 12991.02,
        }
    )

    assert tracker.active_candidate_progress_panel(
        "joint_refinement restart optimization [27%] | best=baseline | best_obj=12991.020000"
    ) == "\n".join(
        (
            "joint_refinement restart optimization [27%] | best=baseline | best_obj=12991.020000",
            "  baseline               hawkes     1/3  eval  41%  obj   12991.02  best   12991.02",
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
        "completed_outer_cycles": 1,
        "total_outer_cycles": 3,
        "evaluation_progress_percent": 47,
        "aggregate_progress_percent": 63,
        "current_weighted_objective": 265255.2,
        "current_restart_best_weighted_objective": 18422.71,
        "best_restart_id": "baseline",
        "best_weighted_objective": 13001.0,
    }

    assert format_compact_staged_progress_event_detail(
        raw_event,
        completed_units=3,
        total_units=15,
        progress_percent_override=63,
    ) == (
        "joint_refinement restart optimization [63%] | item=halton_09 | "
        "running=12 | block=nig | cycle=1/3 | eval=47% | "
        "obj=265255.200000 | candidate_best=18422.710000 | best=baseline | global_best=13001.000000"
    )
    assert format_staged_progress_event_detail(raw_event) == (
        "joint_refinement restart optimization 3/15 | item=halton_09 | "
        "running=12 | block=nig | cycle=1/3 | eval=47% | "
        "objective=265255.200000 | candidate_best_objective=18422.710000 | "
        "best_candidate=baseline | best_objective=13001.000000"
    )


def test_option_refinement_progress_panel_tracks_active_candidates() -> None:
    tracker = NativeStagedCalibrationProgressTracker()
    tracker.update(
        {
            "event_kind": "stage_progress",
            "stage": "joint_refinement",
            "progress_phase": "option_search_tier_refinement",
            "completed_units": 0,
            "total_units": 5,
            "current_restart_id": "baseline",
            "current_item_label": "interval 99/957",
            "active_restart_count": 2,
            "current_evaluation_count": 7,
            "total_evaluation_budget": 27,
            "current_weighted_objective": 449.91,
            "best_restart_id": "baseline",
            "best_weighted_objective": 449.91,
        }
    )
    tracker.update(
        {
            "event_kind": "stage_progress",
            "stage": "joint_refinement",
            "progress_phase": "option_search_tier_refinement",
            "completed_units": 0,
            "total_units": 5,
            "current_restart_id": "halton_07",
            "current_item_label": "interval 245/957",
            "active_restart_count": 2,
            "current_evaluation_count": 3,
            "total_evaluation_budget": 27,
            "current_weighted_objective": 452.37,
            "best_restart_id": "baseline",
            "best_weighted_objective": 449.91,
        }
    )

    assert tracker.active_candidate_progress_panel(
        "joint_refinement option_search_tier_refinement [0%] | best=baseline | best_obj=449.910000"
    ) == "\n".join(
        (
            "joint_refinement option_search_tier_refinement [0%] | best=baseline | best_obj=449.910000",
            "  baseline               interval 99/957               eval  7/27  obj     449.91  best          -",
            "  halton_07              interval 245/957              eval  3/27  obj     452.37  best          -",
        )
    )


def test_compact_option_refinement_progress_detail_shows_item_before_first_completion() -> None:
    raw_event = {
        "stage": "joint_refinement",
        "progress_phase": "option_search_tier_refinement",
        "completed_units": 0,
        "total_units": 5,
        "current_item_label": "baseline",
    }

    assert format_compact_staged_progress_event_detail(
        raw_event,
        completed_units=0,
        total_units=5,
    ) == (
        "joint_refinement option search tier refinement [0%] (0/5) | item=baseline"
    )


def test_compact_option_refinement_progress_detail_shows_candidate_and_pricing_step() -> None:
    raw_event = {
        "stage": "joint_refinement",
        "progress_phase": "option_search_tier_refinement",
        "completed_units": 0,
        "total_units": 5,
        "current_restart_id": "activity_aligned",
        "current_item_label": "maturity 3/24",
        "current_evaluation_count": 7,
        "total_evaluation_budget": 27,
        "evaluation_progress_percent": 25,
        "best_restart_id": "baseline",
        "best_weighted_objective": 0.1825,
    }

    assert format_compact_staged_progress_event_detail(
        raw_event,
        completed_units=0,
        total_units=5,
    ) == (
        "joint_refinement option search tier refinement [0%] (0/5) | "
        "item=activity_aligned | detail=maturity 3/24 | eval=7/27 | "
        "best=baseline | best_obj=0.182500"
    )
    assert format_staged_progress_event_detail(raw_event) == (
        "joint_refinement option search tier refinement 0/5 | "
        "item=activity_aligned | detail=maturity 3/24 | eval=7/27 | "
        "best_candidate=baseline | best_objective=0.182500"
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
                "joint_refinement restart optimization [68%] | best=baseline | "
                "best_obj=5.315629 | item=baseline | obj=6.843098"
            ),
            progress_fraction=0.64,
        )

    rendered_lines = _render_terminal_output(stream.getvalue())
    expected_eta_seconds = 39.0 * (1.0 - 0.64) / 0.64
    full_stage_line = (
        "✓ fit 1/8 "
        f"[00:39, ETA {workflow_progress_module.format_elapsed_seconds(expected_eta_seconds)}] "
        "running calibration [64%] | joint_refinement restart optimization [68%] "
        "| best=baseline | best_obj=5.315629 | item=baseline | obj=6.843098"
    )
    expected_stage_line = f"{full_stage_line[:76]}..."

    assert rendered_lines == [
        expected_stage_line,
        "done fit total [00:39]",
    ]


def test_spinner_progress_truncates_long_lines_when_terminal_width_is_available_without_tty(
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
        workflow_progress_module.shutil,
        "get_terminal_size",
        lambda fallback=(0, 0): os.terminal_size((80, 24)),
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
                "joint_refinement restart optimization [68%] | best=baseline | "
                "best_obj=5.315629 | item=baseline | obj=6.843098"
            ),
            progress_fraction=0.64,
        )

    rendered_lines = _render_terminal_output(stream.getvalue())
    expected_eta_seconds = 39.0 * (1.0 - 0.64) / 0.64
    full_stage_line = (
        "✓ fit 1/8 "
        f"[00:39, ETA {workflow_progress_module.format_elapsed_seconds(expected_eta_seconds)}] "
        "running calibration [64%] | joint_refinement restart optimization [68%] "
        "| best=baseline | best_obj=5.315629 | item=baseline | obj=6.843098"
    )
    expected_stage_line = f"{full_stage_line[:76]}..."

    assert rendered_lines == [
        expected_stage_line,
        "done fit total [00:39]",
    ]


def test_spinner_progress_truncates_exact_terminal_width_lines_to_avoid_wrap(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    progress = workflow_progress_module.SpinnerProgress(
        label="price",
        stream=io.StringIO(),
        enabled=False,
    )
    monkeypatch.setattr(
        workflow_progress_module,
        "_resolve_stream_render_width",
        lambda stream: 10,
    )

    assert progress._fit_line_to_stream_width("1234567890") == "123456..."


def test_spinner_progress_clears_restart_panel_rows_when_returning_to_single_line_summary(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class _TtyStringIO(io.StringIO):
        def isatty(self) -> bool:
            return True

    stream = _TtyStringIO()
    monotonic_values = iter((0.0, 0.0, 0.0, 39.0, 40.0, 41.0, 41.0))

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
        progress.update(detail="running calibration")
        progress.update(
            detail="running calibration",
            subdetail="\n".join(
                (
                    "joint_refinement restart optimization [99%] | best=baseline | best_obj=1.058281",
                    "  baseline               nig        0/1  eval  99%  obj       1.06  best       1.06",
                    "  halton_01              hawkes     0/1  eval  98%  obj       1.18  best       1.18",
                )
            ),
            progress_fraction=0.82,
        )
        progress.update(
            detail="running calibration",
            subdetail=(
                "joint_refinement coarse consistency evaluation [14%] | "
                "best=baseline | best_obj=1.058281"
            ),
            progress_fraction=0.84,
        )

    rendered_lines = _render_terminal_output(stream.getvalue())
    expected_eta_seconds = 41.0 * (1.0 - 0.84) / 0.84

    assert rendered_lines == [
        (
            "✓ fit 1/8 "
            f"[00:41, ETA {workflow_progress_module.format_elapsed_seconds(expected_eta_seconds)}] "
            "running calibration [84%] | joint_refinement coarse consistency evaluation [14%] | "
            "best=baseline | best_obj=1.058281"
        ),
        "done fit total [00:41]",
    ]


def test_spinner_progress_renders_multiline_panel_without_cursor_control_once_per_state_change(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    stream = io.StringIO()
    monotonic_values = iter((0.0, 0.0, 0.0, 0.0, 25.0, 25.0, 25.0))

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
        label="price",
        stream=stream,
        enabled=True,
    ) as progress:
        progress.update(detail="pricing observed option surface")
        progress.update(
            detail="pricing observed option surface",
            completed_units=0,
            total_units=22,
            subdetail="\n".join(
                (
                    "active 22 | valuation workers 22/24 | pricing cpus per worker 1",
                    "  2026-03-27T18:42:00Z | queued  | - | --:--",
                    "  2026-03-31T20:00:00Z | queued  | - | --:--",
                )
            ),
        )
        progress.update(
            detail="pricing observed option surface",
            completed_units=0,
            total_units=22,
            subdetail="\n".join(
                (
                    "active 22 | valuation workers 22/24 | pricing cpus per worker 1",
                    "  2026-03-27T18:42:00Z | queued  | - | --:--",
                    "  2026-03-31T20:00:00Z | queued  | - | --:--",
                )
            ),
        )

    rendered_lines = _render_terminal_output(stream.getvalue())

    assert rendered_lines == [
        "- price [0% 0/22] pricing observed option surface | active 22 | valuation workers 22/24 | pricing cpus per worker 1 [00:00]",
        "  2026-03-27T18:42:00Z | queued  | - | --:--",
        "  2026-03-31T20:00:00Z | queued  | - | --:--",
        "✓ price [0% 0/22] pricing observed option surface | active 22 | valuation workers 22/24 | pricing cpus per worker 1 [00:25]",
        "done price total [00:25]",
    ]


def test_spinner_progress_rewrites_multiline_panel_in_place_with_portable_cursor_control(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class _TtyStringIO(io.StringIO):
        def isatty(self) -> bool:
            return True

    stream = _TtyStringIO()
    monotonic_values = iter((0.0, 0.0, 0.0, 5.0, 10.0, 10.0, 10.0))

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

    progress = workflow_progress_module.SpinnerProgress(
        label="price",
        stream=stream,
        enabled=True,
    )
    try:
        progress.update(detail="pricing observed option surface")
        progress.update(
            detail="pricing observed option surface",
            completed_units=0,
            total_units=2,
            subdetail="\n".join(
                (
                    "active 2 | valuation workers 2/4 | pricing cpus per worker 2",
                    "  2026-03-27T18:42:00Z | queued  | - | --:--",
                    "  2026-03-31T20:00:00Z | queued  | - | --:--",
                )
            ),
        )
        progress.update(
            detail="pricing observed option surface",
            completed_units=1,
            total_units=2,
            subdetail="\n".join(
                (
                    "active 1 | valuation workers 2/4 | pricing cpus per worker 2",
                    "  2026-03-27T18:42:00Z | done    | strike 41/128 | 03:05",
                    "  2026-03-31T20:00:00Z | running | interval 193/957 | 00:10",
                )
            ),
        )

        raw_output = stream.getvalue()
        rendered_lines = _render_terminal_output(raw_output)
    finally:
        progress.__exit__(None, None, None)

    assert "\x1b[2A\r" in raw_output
    assert rendered_lines == [
        "- price [50% 1/2] pricing observed option surface | active 1 | valuation workers 2/4 | pricing cpus per worker 2 [00:10, ETA 00:10]",
        "  2026-03-27T18:42:00Z | done    | strike 41/128 | 03:05",
        "  2026-03-31T20:00:00Z | running | interval 193/957 | 00:10",
    ]


def test_spinner_progress_animates_multiline_panel_with_cursor_control() -> None:
    class _TtyStringIO(io.StringIO):
        def isatty(self) -> bool:
            return True

    stream = _TtyStringIO()
    progress = workflow_progress_module.SpinnerProgress(
        label="price",
        stream=stream,
        enabled=False,
    )
    progress._detail = "pricing observed option surface"
    progress._subdetail = "\n".join(
        (
            "active 22 | valuation workers 22/24 | pricing cpus per worker 1",
            "  2026-03-27T18:42:00Z | queued  | - | --:--",
        )
    )
    progress._stage_started_at = 0.0

    assert progress._should_animate_locked() is True


def test_spinner_progress_detects_cursor_control_from_stream_fileno(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class _WrappedStream(io.StringIO):
        def isatty(self) -> bool:
            return False

        def fileno(self) -> int:
            return 9

    monkeypatch.setattr(workflow_progress_module.os, "isatty", lambda fileno: fileno == 9)

    progress = workflow_progress_module.SpinnerProgress(
        label="price",
        stream=_WrappedStream(),
        enabled=False,
    )

    assert progress._supports_cursor_control() is True


def test_restart_progress_panel_widens_candidate_column_for_long_restart_ids() -> None:
    long_row = ActiveCandidateProgressRow(
        restart_id="nig_basin_high_alpha_low_delta_activity_aligned",
        parameter_block="hawkes",
        completed_outer_cycles=0,
        total_outer_cycles=1,
        evaluation_progress_percent=60,
        current_evaluation_count=12,
        current_weighted_objective=1.49,
        candidate_best_weighted_objective=1.49,
    )
    short_row = ActiveCandidateProgressRow(
        restart_id="baseline",
        parameter_block="hawkes",
        completed_outer_cycles=0,
        total_outer_cycles=1,
        evaluation_progress_percent=60,
        current_evaluation_count=12,
        current_weighted_objective=1.48,
        candidate_best_weighted_objective=1.48,
    )

    restart_id_width = maximum_active_candidate_progress_row_id_width((long_row, short_row))

    assert restart_id_width == len(long_row.restart_id)
    assert format_active_candidate_progress_panel_row(
        long_row,
        restart_id_width=restart_id_width,
    ) == (
        "  nig_basin_high_alpha_low_delta_activity_aligned hawkes     0/1  "
        "eval  60%  obj       1.49  best       1.49"
    )
    assert format_active_candidate_progress_panel_row(
        short_row,
        restart_id_width=restart_id_width,
    ) == (
        "  baseline                                        hawkes     0/1  "
        "eval  60%  obj       1.48  best       1.48"
    )


def _render_terminal_output(raw_output: str) -> list[str]:
    rendered_lines: list[str] = []
    current_line: list[str] = []
    cursor_position = 0
    line_index = 0
    saved_line_index: Optional[int] = None
    saved_cursor_position: Optional[int] = None
    character_index = 0

    while character_index < len(raw_output):
        character = raw_output[character_index]
        if character == "\r":
            cursor_position = 0
            character_index += 1
            continue

        if character == "\n":
            if line_index == len(rendered_lines):
                rendered_lines.append("".join(current_line).rstrip())
            else:
                rendered_lines[line_index] = "".join(current_line).rstrip()

            line_index += 1
            current_line = (
                list(rendered_lines[line_index])
                if line_index < len(rendered_lines)
                else []
            )
            cursor_position = 0
            character_index += 1
            continue

        if character == "\x1b":
            escape_start = character_index
            character_index += 1
            if character_index < len(raw_output) and raw_output[character_index] == "7":
                saved_line_index = line_index
                saved_cursor_position = cursor_position
                character_index += 1
                continue

            if character_index < len(raw_output) and raw_output[character_index] == "8":
                if line_index == len(rendered_lines):
                    rendered_lines.append("".join(current_line).rstrip())
                else:
                    rendered_lines[line_index] = "".join(current_line).rstrip()

                if saved_line_index is not None and saved_cursor_position is not None:
                    line_index = saved_line_index
                    current_line = (
                        list(rendered_lines[line_index])
                        if line_index < len(rendered_lines)
                        else []
                    )
                    cursor_position = saved_cursor_position

                character_index += 1
                continue

            if character_index >= len(raw_output) or raw_output[character_index] != "[":
                continue

            character_index += 1
            numeric_buffer: list[str] = []
            while (
                character_index < len(raw_output)
                and raw_output[character_index].isdigit()
            ):
                numeric_buffer.append(raw_output[character_index])
                character_index += 1

            if character_index >= len(raw_output):
                break

            command = raw_output[character_index]
            argument = int("".join(numeric_buffer) or "1")
            character_index += 1

            if command == "A":
                if line_index == len(rendered_lines):
                    rendered_lines.append("".join(current_line).rstrip())
                else:
                    rendered_lines[line_index] = "".join(current_line).rstrip()

                line_index = max(0, line_index - argument)
                current_line = list(rendered_lines[line_index])
                cursor_position = 0
                continue

            if command == "K" and argument == 2:
                current_line = []
                cursor_position = 0
                continue

            if command == "s":
                saved_line_index = line_index
                saved_cursor_position = cursor_position
                continue

            if command == "u":
                if line_index == len(rendered_lines):
                    rendered_lines.append("".join(current_line).rstrip())
                else:
                    rendered_lines[line_index] = "".join(current_line).rstrip()

                if saved_line_index is not None and saved_cursor_position is not None:
                    line_index = saved_line_index
                    current_line = (
                        list(rendered_lines[line_index])
                        if line_index < len(rendered_lines)
                        else []
                    )
                    cursor_position = saved_cursor_position
                continue

            character_index = escape_start + 1
            continue

        if cursor_position == len(current_line):
            current_line.append(character)
        else:
            current_line[cursor_position] = character

        cursor_position += 1
        character_index += 1

    if current_line:
        if line_index == len(rendered_lines):
            rendered_lines.append("".join(current_line).rstrip())
        else:
            rendered_lines[line_index] = "".join(current_line).rstrip()

    while rendered_lines and rendered_lines[-1] == "":
        rendered_lines.pop()

    return rendered_lines
