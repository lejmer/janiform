import os
from pathlib import Path

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PyQt6.QtCore import Qt
from PyQt6.QtWidgets import (
    QApplication,
    QLineEdit,
    QPushButton,
    QSplitter,
    QTableWidget,
    QTabWidget,
)
from matplotlib.figure import Figure

from janiform.calibration.staged._progress_policy import (
    CalibrationLiveProgressSnapshot,
    calibration_total_progress_fraction,
)
from tools.workbench import app as workbench_app
from tools.workbench.app import WorkbenchWindow, _ResponsiveFigureCanvas
from tools.workbench.artifacts import WorkbenchArtifacts
from tools.workbench.events import WorkbenchEventType, build_workbench_event
from tools.workbench.loading import workbench_observed_simulation_refresh_task
from tools.workbench.resimulation import WorkbenchObservedSimulationRefreshResult


def test_responsive_figure_canvas_resizes_figure_to_widget_viewport() -> None:
    application = QApplication.instance() or QApplication([])
    figure = Figure(figsize=(15, 9), constrained_layout=True)
    widget = _ResponsiveFigureCanvas(figure)

    try:
        widget.resize(900, 420)
        widget.show()
        application.processEvents()

        figure_width_pixels = figure.get_size_inches()[0] * figure.get_dpi()
        figure_height_pixels = figure.get_size_inches()[1] * figure.get_dpi()

        assert abs(figure_width_pixels - widget.width()) <= 2.0
        assert abs(figure_height_pixels - widget.height()) <= 2.0
    finally:
        widget.close()
        application.processEvents()


def test_selected_candidate_plot_uses_native_summary_table_panel() -> None:
    application = QApplication.instance() or QApplication([])
    window = WorkbenchWindow()
    window._artifacts = WorkbenchArtifacts(
        source_path=Path("/tmp/workbench-native-summary"),
        source_kind="run_directory",
        metadata={},
        fit_report_payload=None,
        model_payload=None,
        observed_targets={},
        candidate_frontier={
            "frontier_entries": [
                {
                    "candidate_id": "candidate-001",
                    "stage": "joint_refinement",
                    "evaluation_tier": "tick_joint",
                    "admissible": False,
                    "valid": True,
                    "scalar_tie_break": 0.3,
                    "canonical_coordinates": {"theta": 0.0},
                    "family_metrics": [],
                }
            ]
        },
        coarse_tick_consistency={},
        selection_rationale={
            "decision": {
                "selected_candidate_id": "candidate-001",
                "selection_stage": "joint_refinement",
                "selection_policy": "validity_then_admissibility_then_pareto_then_lexicographic_tie_break.v1",
                "pareto_frontier_count": 1,
                "admissible_candidate_count": 0,
            }
        },
        simulation_summary={},
        search_surface_manifest={},
        runtime_profile={},
    )

    try:
        panel = window._build_native_plot_panel("plot_selected_candidate")
        assert isinstance(panel, QTabWidget)
        assert panel.tabText(0) == "Chart"
        assert panel.tabText(1) == "Summary Table"

        summary_table = panel.widget(1).findChild(QTableWidget)
        assert summary_table is not None
        assert summary_table.rowCount() >= 1
        assert summary_table.columnCount() == 2
    finally:
        window.close()
        application.processEvents()


def test_observed_simulation_comparison_cache_keys_on_selected_seed(
    monkeypatch,
) -> None:
    application = QApplication.instance() or QApplication([])
    window = WorkbenchWindow()
    window._artifacts = WorkbenchArtifacts(
        source_path=Path("/tmp/workbench-seed-cache"),
        source_kind="run_directory",
        metadata={},
        fit_report_payload=None,
        model_payload=None,
        observed_targets={},
        candidate_frontier={},
        coarse_tick_consistency={},
        selection_rationale={},
        simulation_summary={},
        search_surface_manifest={},
        runtime_profile={},
    )
    captured_seeds: list[int] = []

    monkeypatch.setattr(
        workbench_app,
        "build_observed_simulation_comparison",
        lambda artifacts, *, seed=1: (
            captured_seeds.append(seed),
            {"seed": seed},
        )[1],
    )

    try:
        first = window._observed_simulation_comparison_for_current_artifacts()
        second = window._observed_simulation_comparison_for_current_artifacts()

        window._observed_simulation_seed = 7
        third = window._observed_simulation_comparison_for_current_artifacts()

        assert first == {"seed": 1}
        assert second == {"seed": 1}
        assert third == {"seed": 7}
        assert captured_seeds == [1, 7]
    finally:
        window.close()
        application.processEvents()


def test_resimulate_updates_seed_controls_for_all_observed_simulation_plots(
    monkeypatch,
) -> None:
    application = QApplication.instance() or QApplication([])
    window = WorkbenchWindow()

    observed_task_ids = [
        manifest_entry.task_id
        for manifest_entry in window._plot_manifest_by_task_id.values()
        if manifest_entry.uses_observed_simulation_comparison
    ]

    assert len(observed_task_ids) >= 2

    first_task_id = observed_task_ids[0]
    second_task_id = observed_task_ids[1]

    first_seed_input = window._plot_container_by_task_id[first_task_id].findChildren(QLineEdit)[0]
    second_seed_input = window._plot_container_by_task_id[second_task_id].findChildren(QLineEdit)[0]

    try:
        assert first_seed_input.text() == "1"
        assert second_seed_input.text() == "1"

        monkeypatch.setattr(
            window,
            "_start_observed_simulation_refresh",
            lambda seed: None,
        )
        window._resimulate_observed_simulation_plots(first_task_id, "7")

        refreshed_first_seed_input = window._plot_container_by_task_id[first_task_id].findChildren(
            QLineEdit
        )[-1]
        refreshed_second_seed_input = window._plot_container_by_task_id[
            second_task_id
        ].findChildren(QLineEdit)[-1]
        refreshed_first_button = next(
            button
            for button in window._plot_container_by_task_id[first_task_id].findChildren(
                QPushButton
            )
            if button.text() == "Resimulating..."
        )
        refreshed_second_button = next(
            button
            for button in window._plot_container_by_task_id[second_task_id].findChildren(
                QPushButton
            )
            if button.text() == "Resimulating..."
        )

        assert refreshed_first_seed_input.text() == "7"
        assert refreshed_second_seed_input.text() == "7"
        assert not refreshed_first_seed_input.isEnabled()
        assert not refreshed_second_seed_input.isEnabled()
        assert not refreshed_first_button.isEnabled()
        assert not refreshed_second_button.isEnabled()
    finally:
        window.close()
        application.processEvents()


def test_resimulate_starts_background_refresh_instead_of_rendering_immediately(
    monkeypatch,
) -> None:
    application = QApplication.instance() or QApplication([])
    window = WorkbenchWindow()

    observed_task_id = next(
        manifest_entry.task_id
        for manifest_entry in window._plot_manifest_by_task_id.values()
        if manifest_entry.uses_observed_simulation_comparison
    )
    started_seeds: list[int] = []

    monkeypatch.setattr(
        window,
        "_start_observed_simulation_refresh",
        lambda seed: started_seeds.append(seed),
    )
    monkeypatch.setattr(
        window,
        "_selected_plot_task_id",
        lambda: observed_task_id,
    )

    try:
        window._resimulate_observed_simulation_plots(observed_task_id, "9")

        assert window._observed_simulation_seed == 9
        assert window._observed_simulation_refresh_in_progress
        assert started_seeds == [9]
    finally:
        window.close()
        application.processEvents()


def test_run_tab_tracks_observed_simulation_refresh_activity() -> None:
    application = QApplication.instance() or QApplication([])
    window = WorkbenchWindow()
    refresh_task = workbench_observed_simulation_refresh_task()
    refresh_row = window._task_row_by_id[refresh_task.task_id]

    try:
        assert window._activity_table.item(refresh_row, 1).text() == refresh_task.title
        assert window._activity_table.item(refresh_row, 2).text() == refresh_task.detail

        window._on_activity_changed(refresh_task.title, "Resimulating observed range")
        window._on_task_state_changed(refresh_task.task_id, "running", 0, 1)

        assert window._activity_title_label.text() == refresh_task.title
        assert window._activity_detail_label.text() == "Resimulating observed range"
        assert window._activity_table.item(refresh_row, 0).text() == "running"

        window._on_observed_simulation_refresh_completed(
            WorkbenchObservedSimulationRefreshResult(
                seed=11,
                comparison_available=True,
            )
        )

        assert window._activity_title_label.text() == refresh_task.title
        assert window._activity_detail_label.text() == (
            "Observed-versus-simulated plots refreshed to seed 11."
        )
    finally:
        window.close()
        application.processEvents()


def test_render_plot_canvas_skips_observed_simulation_build_while_refresh_runs(
    monkeypatch,
) -> None:
    application = QApplication.instance() or QApplication([])
    window = WorkbenchWindow()
    observed_task_id = next(
        manifest_entry.task_id
        for manifest_entry in window._plot_manifest_by_task_id.values()
        if manifest_entry.uses_observed_simulation_comparison
    )

    try:
        window._observed_simulation_refresh_in_progress = True
        monkeypatch.setattr(
            window,
            "_build_native_plot_panel",
            lambda task_id: (_ for _ in ()).throw(AssertionError(task_id)),
        )

        window._render_plot_canvas(observed_task_id)
    finally:
        window.close()
        application.processEvents()


def test_workbench_window_hides_resource_snapshot_events_from_event_table() -> None:
    application = QApplication.instance() or QApplication([])
    window = WorkbenchWindow()

    run_started_event = build_workbench_event(
        event_type=WorkbenchEventType.RUN_STARTED,
        run_id="run-001",
        sequence_number=1,
        payload={"detail": "Starting calibration"},
    )
    resource_event = build_workbench_event(
        event_type=WorkbenchEventType.RESOURCE_SNAPSHOT_RECORDED,
        run_id="run-001",
        sequence_number=2,
        payload={"elapsed_seconds": 12.5, "worker_count": 1},
    )

    try:
        window._on_execution_event(run_started_event)
        window._on_execution_event(resource_event)

        assert window._event_table.rowCount() == 1
        assert window._state.run.elapsed_seconds == 12.5
        assert window._run_stage_elapsed_label.text() == "Current stage elapsed: 0.0s"
        assert window._run_total_elapsed_label.text() == "Total elapsed: 12.5s"
    finally:
        window.close()
        application.processEvents()


def test_workbench_window_run_tab_keeps_event_and_activity_tables_side_by_side() -> None:
    application = QApplication.instance() or QApplication([])
    window = WorkbenchWindow()

    try:
        run_tab = window._root_tabs.widget(1)
        assert run_tab is not None

        run_splitters = run_tab.findChildren(QSplitter)
        horizontal_splitter = next(
            splitter
            for splitter in run_splitters
            if splitter.orientation() == Qt.Orientation.Horizontal
        )
        vertical_splitter = next(
            splitter
            for splitter in run_splitters
            if splitter.orientation() == Qt.Orientation.Vertical
        )

        event_panel = window._event_table.parentWidget()
        activity_panel = window._activity_table.parentWidget()
        live_plot_panel = window._live_run_plot_container.parentWidget()

        assert event_panel is not None
        assert activity_panel is not None
        assert live_plot_panel is not None
        assert horizontal_splitter.indexOf(event_panel) >= 0
        assert horizontal_splitter.indexOf(activity_panel) >= 0
        assert vertical_splitter.indexOf(horizontal_splitter) >= 0
        assert vertical_splitter.indexOf(live_plot_panel) >= 0
    finally:
        window.close()
        application.processEvents()


def test_workbench_window_scrolls_event_table_to_latest_row() -> None:
    application = QApplication.instance() or QApplication([])
    window = WorkbenchWindow()
    window.resize(900, 520)
    window.show()
    application.processEvents()

    try:
        for sequence_number in range(1, 41):
            window._on_execution_event(
                build_workbench_event(
                    event_type=WorkbenchEventType.RUN_STARTED,
                    run_id="run-001",
                    sequence_number=sequence_number,
                    payload={"detail": f"Event {sequence_number}"},
                )
            )

        application.processEvents()
        vertical_scroll_bar = window._event_table.verticalScrollBar()

        assert vertical_scroll_bar.maximum() > 0
        assert vertical_scroll_bar.value() == vertical_scroll_bar.maximum()
    finally:
        window.close()
        application.processEvents()


def test_workbench_window_tracks_stage_and_total_progress_separately() -> None:
    application = QApplication.instance() or QApplication([])
    window = WorkbenchWindow()

    try:
        window._on_execution_event(
            build_workbench_event(
                event_type=WorkbenchEventType.RUN_STARTED,
                run_id="run-001",
                sequence_number=1,
                payload={
                    "detail": "Starting calibration",
                    "stage_sequence": [
                        "seasonality",
                        "hawkes",
                        "return_components",
                        "mean_reversion",
                        "joint_refinement",
                    ],
                },
            )
        )
        window._on_execution_event(
            build_workbench_event(
                event_type=WorkbenchEventType.STAGE_STARTED,
                run_id="run-001",
                sequence_number=2,
                stage="joint_refinement",
                payload={
                    "stage_index": 5,
                    "stage_count": 5,
                    "stage_label": "joint_refinement",
                    "detail": "Running joint_refinement",
                },
            )
        )
        window._on_execution_event(
            build_workbench_event(
                event_type=WorkbenchEventType.SUBTASK_STARTED,
                run_id="run-001",
                sequence_number=3,
                stage="joint_refinement",
                subtask="joint_refinement.restart_optimization",
                payload={
                    "detail": "joint_refinement restart optimization 2/8",
                    "completed_units": 2,
                    "total_units": 8,
                },
            )
        )
        window._on_execution_event(
            build_workbench_event(
                event_type=WorkbenchEventType.RESOURCE_SNAPSHOT_RECORDED,
                run_id="run-001",
                sequence_number=4,
                stage="joint_refinement",
                payload={"elapsed_seconds": 42.5, "worker_count": 1},
            )
        )

        assert window._run_stage_elapsed_label.text() == "Current stage elapsed: 42.5s"
        assert window._run_total_elapsed_label.text() == "Total elapsed: 42.5s"
        assert window._run_stage_progress_bar.maximum() == 8
        assert window._run_stage_progress_bar.value() == 1
        assert window._run_total_progress_bar.maximum() == 1000
        expected_progress_fraction = calibration_total_progress_fraction(
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
        assert expected_progress_fraction is not None
        assert window._run_total_progress_bar.value() == round(expected_progress_fraction * 1000)
    finally:
        window.close()
        application.processEvents()


def test_workbench_window_total_progress_does_not_regress_across_joint_refinement_phases() -> None:
    application = QApplication.instance() or QApplication([])
    window = WorkbenchWindow()

    try:
        window._on_execution_event(
            build_workbench_event(
                event_type=WorkbenchEventType.RUN_STARTED,
                run_id="run-001",
                sequence_number=1,
                payload={
                    "detail": "Starting calibration",
                    "stage_sequence": [
                        "seasonality",
                        "hawkes",
                        "return_components",
                        "mean_reversion",
                        "joint_refinement",
                    ],
                },
            )
        )
        window._on_execution_event(
            build_workbench_event(
                event_type=WorkbenchEventType.STAGE_STARTED,
                run_id="run-001",
                sequence_number=2,
                stage="joint_refinement",
                payload={
                    "stage_index": 5,
                    "stage_count": 5,
                    "stage_label": "joint_refinement",
                    "detail": "Running joint_refinement",
                },
            )
        )
        window._on_execution_event(
            build_workbench_event(
                event_type=WorkbenchEventType.SUBTASK_STARTED,
                run_id="run-001",
                sequence_number=3,
                stage="joint_refinement",
                subtask="joint_refinement.restart_optimization",
                payload={
                    "detail": "joint_refinement restart optimization 8/8",
                    "completed_units": 8,
                    "total_units": 8,
                },
            )
        )
        restart_completion_value = window._run_total_progress_bar.value()

        window._on_execution_event(
            build_workbench_event(
                event_type=WorkbenchEventType.SUBTASK_STARTED,
                run_id="run-001",
                sequence_number=4,
                stage="joint_refinement",
                subtask="joint_refinement.coarse_consistency_evaluation",
                payload={
                    "detail": "joint_refinement coarse consistency evaluation 0/12",
                    "completed_units": 0,
                    "total_units": 12,
                },
            )
        )

        expected_restart_completion_fraction = calibration_total_progress_fraction(
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
                subtask_completed_units=8,
                subtask_total_units=8,
            )
        )
        expected_coarse_phase_fraction = calibration_total_progress_fraction(
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
                current_subtask="joint_refinement.coarse_consistency_evaluation",
                current_stage_is_complete=False,
                subtask_completed_units=0,
                subtask_total_units=12,
            )
        )
        assert expected_restart_completion_fraction is not None
        assert expected_coarse_phase_fraction is not None
        assert restart_completion_value == round(expected_restart_completion_fraction * 1000)
        assert window._run_total_progress_bar.value() == round(expected_coarse_phase_fraction * 1000)
    finally:
        window.close()
        application.processEvents()


def test_workbench_window_total_progress_stays_below_full_until_run_completed() -> None:
    application = QApplication.instance() or QApplication([])
    window = WorkbenchWindow()

    try:
        window._on_execution_event(
            build_workbench_event(
                event_type=WorkbenchEventType.RUN_STARTED,
                run_id="run-001",
                sequence_number=1,
                payload={
                    "detail": "Starting calibration",
                    "stage_sequence": [
                        "seasonality",
                        "hawkes",
                        "return_components",
                        "mean_reversion",
                        "joint_refinement",
                    ],
                },
            )
        )
        window._on_execution_event(
            build_workbench_event(
                event_type=WorkbenchEventType.STAGE_COMPLETED,
                run_id="run-001",
                sequence_number=2,
                stage="joint_refinement",
                payload={
                    "stage_index": 5,
                    "stage_count": 5,
                    "stage_label": "joint_refinement",
                    "detail": "joint_refinement completed",
                    "runtime_seconds": 120.0,
                },
            )
        )

        assert window._run_total_progress_bar.value() == 990

        window._on_execution_event(
            build_workbench_event(
                event_type=WorkbenchEventType.RUN_COMPLETED,
                run_id="run-001",
                sequence_number=3,
                payload={"detail": "Calibration completed"},
            )
        )

        assert window._run_total_progress_bar.value() == 1000
    finally:
        window.close()
        application.processEvents()


def test_workbench_window_total_elapsed_updates_before_first_backend_heartbeat(
    monkeypatch,
) -> None:
    application = QApplication.instance() or QApplication([])
    window = WorkbenchWindow()
    monotonic_values = iter([10.0, 12.3])

    monkeypatch.setattr(workbench_app, "monotonic", lambda: next(monotonic_values))

    try:
        window._state.run.status = "running"
        window._start_live_run_elapsed_timer()
        window._refresh_run_status_panel()

        assert window._run_total_elapsed_label.text() == "Total elapsed: 2.3s"
    finally:
        window._stop_live_run_elapsed_timer()
        window.close()
        application.processEvents()


def test_workbench_window_copies_visible_event_table_to_clipboard() -> None:
    application = QApplication.instance() or QApplication([])
    window = WorkbenchWindow()

    run_started_event = build_workbench_event(
        event_type=WorkbenchEventType.RUN_STARTED,
        run_id="run-001",
        sequence_number=1,
        payload={"detail": "Starting calibration"},
    )

    try:
        window._on_execution_event(run_started_event)
        window._copy_event_table_to_clipboard()

        clipboard_text = application.clipboard().text()

        assert clipboard_text.startswith("Time\tType\tStage\tDetail\n")
        assert "run.started" in clipboard_text
        assert "Starting calibration" in clipboard_text
    finally:
        window.close()
        application.processEvents()


def test_workbench_window_exports_visible_event_table_to_tsv(
    tmp_path: Path,
    monkeypatch,
) -> None:
    application = QApplication.instance() or QApplication([])
    window = WorkbenchWindow()

    run_started_event = build_workbench_event(
        event_type=WorkbenchEventType.RUN_STARTED,
        run_id="run-001",
        sequence_number=1,
        payload={"detail": "Starting calibration"},
    )
    export_path = tmp_path / "event_table.tsv"

    monkeypatch.setattr(
        workbench_app.QFileDialog,
        "getSaveFileName",
        lambda *args, **kwargs: (str(export_path), "Tab-separated values (*.tsv)"),
    )

    try:
        window._on_execution_event(run_started_event)
        window._export_event_table_tsv()

        exported_text = export_path.read_text(encoding="utf-8")

        assert exported_text.startswith("Time\tType\tStage\tDetail\n")
        assert "run.started" in exported_text
        assert "Starting calibration" in exported_text
    finally:
        window.close()
        application.processEvents()
