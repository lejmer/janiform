import os
from pathlib import Path

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PyQt6.QtWidgets import QApplication, QTableWidget, QTabWidget
from matplotlib.figure import Figure

from tools.workbench import app as workbench_app
from tools.workbench.app import WorkbenchWindow, _ResponsiveFigureCanvas
from tools.workbench.artifacts import WorkbenchArtifacts
from tools.workbench.events import WorkbenchEventType, build_workbench_event


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
        assert window._run_elapsed_label.text() == "Elapsed: 12.5s"
    finally:
        window.close()
        application.processEvents()
