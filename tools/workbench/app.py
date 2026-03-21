import json
import os
import sys
import tempfile

from pathlib import Path
from typing import Any

from PyQt6.QtWidgets import (
    QApplication,
    QFileDialog,
    QLabel,
    QMainWindow,
    QMessageBox,
    QTableWidget,
    QTableWidgetItem,
    QTabWidget,
    QTextEdit,
    QVBoxLayout,
    QWidget,
)

if "MPLCONFIGDIR" not in os.environ:
    matplotlib_cache_directory = Path(tempfile.gettempdir()) / "janiform-mpl"
    matplotlib_cache_directory.mkdir(parents=True, exist_ok=True)
    os.environ["MPLCONFIGDIR"] = str(matplotlib_cache_directory)

from matplotlib.backends.backend_qtagg import FigureCanvasQTAgg

from .artifacts import WorkbenchArtifacts, build_summary_payload, load_workbench_artifacts
from .plots import (
    create_consistency_figure,
    create_frontier_figure,
    create_runtime_figure,
    create_search_surface_figure,
    save_all_figures,
)


class WorkbenchWindow(QMainWindow):
    def __init__(self, artifacts: WorkbenchArtifacts) -> None:
        super().__init__()
        self._artifacts = artifacts
        self.setWindowTitle("Janiform Calibration Workbench")
        self.resize(1400, 900)
        self.statusBar().showMessage("Loaded calibration diagnostics")
        self._build_ui()
        self._build_actions()

    def _build_ui(self) -> None:
        tab_widget = QTabWidget()
        tab_widget.addTab(self._build_summary_tab(), "Summary")
        tab_widget.addTab(
            _build_table_tab(
                _artifact_rows(self._artifacts.observed_targets, "targets")
            ),
            "Observed Targets",
        )
        tab_widget.addTab(
            _build_table_tab(
                _artifact_rows(self._artifacts.candidate_frontier, "frontier_entries")
            ),
            "Candidate Frontier",
        )
        tab_widget.addTab(
            _build_table_tab(
                _artifact_rows(self._artifacts.coarse_tick_consistency, "metrics")
            ),
            "Coarse/Tick",
        )
        tab_widget.addTab(self._build_plot_tab(), "Plots")
        self.setCentralWidget(tab_widget)

    def _build_actions(self) -> None:
        export_menu = self.menuBar().addMenu("Export")
        export_summary_action = export_menu.addAction("Export Summary JSON")
        export_summary_action.triggered.connect(self._export_summary_json)

        export_frontier_action = export_menu.addAction("Export Frontier JSON")
        export_frontier_action.triggered.connect(self._export_frontier_json)

        export_plots_action = export_menu.addAction("Export Plots")
        export_plots_action.triggered.connect(self._export_plots)

    def _build_summary_tab(self) -> QWidget:
        summary_payload = build_summary_payload(self._artifacts)
        summary_widget = QWidget()
        layout = QVBoxLayout(summary_widget)
        header = QLabel(
            f"Source: {self._artifacts.source_path}\n"
            f"Selected candidate: {summary_payload['selected_candidate_id']}\n"
            f"Candidates: {summary_payload['candidate_count']}\n"
            f"Admissible candidates: {summary_payload['admissible_candidate_count']}\n"
            f"Runtime total: {summary_payload['runtime_seconds_total']:.2f}s"
        )
        header.setWordWrap(True)
        layout.addWidget(header)

        rationale_view = QTextEdit()
        rationale_view.setReadOnly(True)
        rationale_view.setPlainText(
            json.dumps(
                self._artifacts.selection_rationale,
                indent=2,
                sort_keys=True,
            )
        )
        layout.addWidget(rationale_view)
        return summary_widget

    def _build_plot_tab(self) -> QWidget:
        plot_tabs = QTabWidget()
        plot_tabs.addTab(
            _figure_canvas(create_frontier_figure(self._artifacts)),
            "Frontier",
        )
        plot_tabs.addTab(
            _figure_canvas(create_runtime_figure(self._artifacts)),
            "Runtime",
        )
        plot_tabs.addTab(
            _figure_canvas(create_consistency_figure(self._artifacts)),
            "Coarse/Tick",
        )
        plot_tabs.addTab(
            _figure_canvas(create_search_surface_figure(self._artifacts)),
            "Search Surfaces",
        )
        return plot_tabs

    def _export_summary_json(self) -> None:
        destination = _choose_output_path(
            self,
            caption="Export summary JSON",
            suggested_name="summary.json",
        )
        if destination is None:
            return

        destination.write_text(
            json.dumps(build_summary_payload(self._artifacts), indent=2, sort_keys=True),
            encoding="utf-8",
        )
        self.statusBar().showMessage(f"Wrote {destination}", 5000)

    def _export_frontier_json(self) -> None:
        destination = _choose_output_path(
            self,
            caption="Export frontier JSON",
            suggested_name="candidate_frontier.json",
        )
        if destination is None:
            return

        destination.write_text(
            json.dumps(self._artifacts.candidate_frontier, indent=2, sort_keys=True),
            encoding="utf-8",
        )
        self.statusBar().showMessage(f"Wrote {destination}", 5000)

    def _export_plots(self) -> None:
        selected_directory = QFileDialog.getExistingDirectory(
            self,
            "Export plots",
            str(self._artifacts.source_path.parent),
        )
        if not selected_directory:
            return

        written_paths = save_all_figures(self._artifacts, Path(selected_directory))
        self.statusBar().showMessage(
            f"Wrote {len(written_paths)} plots to {selected_directory}",
            5000,
        )


def launch_workbench(path: str | Path) -> int:
    application = QApplication(sys.argv)
    try:
        artifacts = load_workbench_artifacts(path)
    except Exception as error:
        message_box = QMessageBox()
        message_box.setWindowTitle("Janiform Calibration Workbench")
        message_box.setText(str(error))
        message_box.setIcon(QMessageBox.Icon.Critical)
        message_box.exec()
        return 1

    window = WorkbenchWindow(artifacts)
    window.show()
    return application.exec()


def _artifact_rows(payload: dict[str, Any], field_name: str) -> list[dict[str, Any]]:
    rows = payload.get(field_name, [])
    if isinstance(rows, list):
        return [row for row in rows if isinstance(row, dict)]

    return []


def _build_table_tab(rows: list[dict[str, Any]]) -> QWidget:
    table_widget = QTableWidget()
    flat_rows = [_flatten_mapping(row) for row in rows]
    headers = sorted({key for row in flat_rows for key in row})
    table_widget.setColumnCount(len(headers))
    table_widget.setHorizontalHeaderLabels(headers)
    table_widget.setRowCount(len(flat_rows))

    for row_index, row in enumerate(flat_rows):
        for column_index, header in enumerate(headers):
            item = QTableWidgetItem(row.get(header, ""))
            table_widget.setItem(row_index, column_index, item)

    table_widget.resizeColumnsToContents()
    tab = QWidget()
    layout = QVBoxLayout(tab)
    layout.addWidget(table_widget)
    return tab


def _choose_output_path(
    parent: QWidget,
    *,
    caption: str,
    suggested_name: str,
) -> Path | None:
    selected_path, _ = QFileDialog.getSaveFileName(
        parent,
        caption,
        suggested_name,
        "JSON files (*.json);;All files (*)",
    )
    if not selected_path:
        return None

    return Path(selected_path)


def _figure_canvas(figure: Any) -> QWidget:
    canvas = FigureCanvasQTAgg(figure)
    widget = QWidget()
    layout = QVBoxLayout(widget)
    layout.addWidget(canvas)
    return widget


def _flatten_mapping(
    payload: dict[str, Any],
    *,
    prefix: str = "",
) -> dict[str, str]:
    flattened: dict[str, str] = {}
    for key, value in payload.items():
        field_name = f"{prefix}.{key}" if prefix else key
        if isinstance(value, dict):
            flattened.update(_flatten_mapping(value, prefix=field_name))
            continue
        if isinstance(value, list):
            flattened[field_name] = json.dumps(value, sort_keys=True)
            continue

        flattened[field_name] = "" if value is None else str(value)

    return flattened
