import os
from pathlib import Path

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PyQt6.QtWidgets import QApplication, QTableWidget, QTabWidget
from matplotlib.figure import Figure

from tools.workbench.app import WorkbenchWindow, _ResponsiveFigureCanvas
from tools.workbench.artifacts import WorkbenchArtifacts


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
