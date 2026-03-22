import json
import os
import sys
import tempfile

from pathlib import Path
from typing import Any, Optional

from PyQt6.QtCore import QThread
from PyQt6.QtWidgets import (
    QApplication,
    QComboBox,
    QFileDialog,
    QHBoxLayout,
    QLabel,
    QMainWindow,
    QMessageBox,
    QPushButton,
    QProgressBar,
    QSplitter,
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
from matplotlib.figure import Figure

from .artifacts import WorkbenchArtifacts, build_summary_payload
from .candidate_store import (
    WorkbenchCandidateStore,
    candidate_store_from_artifacts,
    candidate_store_from_stage_report_payloads,
)
from .execution import WorkbenchExecutionResult, WorkbenchExecutionWorker
from .loading import WorkbenchLoadWorker, workbench_startup_tasks
from .plot_manifest import workbench_plot_manifest
from .plots import save_all_figures
from .reducers import apply_workbench_event
from .setup import WorkbenchSetupWidget
from .events import WorkbenchEventType
from .state import WorkbenchAppState, WorkbenchRunSetup, build_initial_workbench_state


class WorkbenchWindow(QMainWindow):
    def __init__(self, input_path: Optional[Path] = None) -> None:
        super().__init__()

        self._state: WorkbenchAppState = build_initial_workbench_state(workspace_root=Path.cwd())
        self._artifacts: Optional[WorkbenchArtifacts] = None
        self._candidate_store: Optional[WorkbenchCandidateStore] = None
        self._task_row_by_id: dict[str, int] = {}
        self._plot_container_by_task_id: dict[str, QWidget] = {}
        self._candidate_ids_by_row: list[str] = []
        self._live_stage_report_payloads: list[dict[str, Any]] = []
        self._load_thread: Optional[QThread] = None
        self._load_worker: Optional[WorkbenchLoadWorker] = None
        self._execution_thread: Optional[QThread] = None
        self._execution_worker: Optional[WorkbenchExecutionWorker] = None

        self._setup_widget = WorkbenchSetupWidget()
        self._root_tabs = QTabWidget()
        self._summary_container = QWidget()
        self._observed_targets_container = QWidget()
        self._candidate_frontier_container = QWidget()
        self._candidate_inspector_container = QWidget()
        self._coarse_tick_container = QWidget()
        self._compare_container = QWidget()
        self._plot_tabs = QTabWidget()
        self._activity_title_label = QLabel()
        self._activity_detail_label = QLabel()
        self._overall_progress_bar = QProgressBar()
        self._activity_table = QTableWidget()
        self._run_status_label = QLabel()
        self._run_stage_label = QLabel()
        self._run_elapsed_label = QLabel()
        self._run_progress_bar = QProgressBar()
        self._event_table = QTableWidget()
        self._live_run_plot_container = QWidget()
        self._candidate_table = QTableWidget()
        self._candidate_detail_view = QTextEdit()
        self._compare_left_selector = QComboBox()
        self._compare_right_selector = QComboBox()
        self._compare_left_view = QTextEdit()
        self._compare_right_view = QTextEdit()
        self._compare_diff_view = QTextEdit()

        self.setWindowTitle("Janiform Calibration Workbench")
        self.resize(1600, 1020)
        self._build_ui()
        self._build_actions()
        self._set_actions_enabled(False)

        if input_path is not None:
            self._open_existing_input(input_path)

    def _build_ui(self) -> None:
        self._setup_widget.setup_submitted.connect(self._start_execution)
        self._setup_widget.open_existing_requested.connect(self._open_existing_run_from_dialog)

        for plot_manifest_entry in workbench_plot_manifest():
            plot_container = QWidget()
            self._plot_container_by_task_id[plot_manifest_entry.task_id] = plot_container
            self._set_widget_contents(
                plot_container,
                _build_placeholder_panel("Queued", plot_manifest_entry.description),
            )
            self._plot_tabs.addTab(plot_container, plot_manifest_entry.tab_title)

        self._root_tabs.addTab(self._setup_widget, "Setup")
        self._root_tabs.addTab(self._build_run_tab(), "Run")
        self._root_tabs.addTab(self._build_inspector_tab(), "Inspector")
        self._root_tabs.addTab(self._build_compare_tab(), "Compare")
        self._root_tabs.addTab(self._plot_tabs, "Plots")
        self.setCentralWidget(self._root_tabs)
        self.statusBar().showMessage("Ready")

    def _build_run_tab(self) -> QWidget:
        run_tab = QWidget()
        layout = QVBoxLayout(run_tab)
        layout.addWidget(self._build_live_status_panel())
        layout.addWidget(self._build_activity_panel())

        return run_tab

    def _build_live_status_panel(self) -> QWidget:
        panel = QWidget()
        layout = QVBoxLayout(panel)

        self._run_status_label.setWordWrap(True)
        self._run_stage_label.setWordWrap(True)
        self._run_elapsed_label.setWordWrap(True)
        self._run_status_label.setText("No calibration is running.")
        self._run_stage_label.setText("Stage: idle")
        self._run_elapsed_label.setText("Elapsed: 0.0s")
        self._run_progress_bar.setRange(0, 1)
        self._run_progress_bar.setValue(0)

        layout.addWidget(self._run_status_label)
        layout.addWidget(self._run_stage_label)
        layout.addWidget(self._run_elapsed_label)
        layout.addWidget(self._run_progress_bar)

        self._event_table.setColumnCount(4)
        self._event_table.setHorizontalHeaderLabels(["Time", "Type", "Stage", "Detail"])

        layout.addWidget(self._event_table)

        self._set_widget_contents(
            self._live_run_plot_container,
            _build_placeholder_panel(
                "No Live Search Diagnostics",
                "Live candidate search diagnostics will appear here during calibration.",
            ),
        )

        layout.addWidget(self._live_run_plot_container)

        return panel

    def _build_activity_panel(self) -> QWidget:
        panel = QWidget()
        layout = QVBoxLayout(panel)

        self._activity_title_label.setText("Idle")
        self._activity_detail_label.setWordWrap(True)
        self._activity_detail_label.setText(
            "Open an existing run or launch a calibration to populate diagnostics."
        )
        self._overall_progress_bar.setRange(0, len(workbench_startup_tasks()))
        self._overall_progress_bar.setValue(0)

        layout.addWidget(self._activity_title_label)
        layout.addWidget(self._activity_detail_label)
        layout.addWidget(self._overall_progress_bar)

        self._activity_table.setColumnCount(3)
        self._activity_table.setHorizontalHeaderLabels(["State", "Task", "Detail"])

        task_definitions = workbench_startup_tasks()

        self._activity_table.setRowCount(len(task_definitions))

        for row_index, task_definition in enumerate(task_definitions):
            self._task_row_by_id[task_definition.task_id] = row_index
            self._activity_table.setItem(row_index, 0, QTableWidgetItem("queued"))
            self._activity_table.setItem(row_index, 1, QTableWidgetItem(task_definition.title))
            self._activity_table.setItem(row_index, 2, QTableWidgetItem(task_definition.detail))

        self._activity_table.resizeColumnsToContents()

        layout.addWidget(self._activity_table)

        return panel

    def _build_inspector_tab(self) -> QWidget:
        self._set_widget_contents(
            self._summary_container,
            _build_placeholder_panel(
                "No Run Loaded",
                "Launch a calibration or open an existing run to inspect the result.",
            ),
        )
        self._set_widget_contents(
            self._observed_targets_container,
            _build_placeholder_panel("No Data", "Observed targets will appear here."),
        )
        self._set_widget_contents(
            self._candidate_frontier_container,
            _build_placeholder_panel("No Data", "Candidate frontier rows will appear here."),
        )
        self._set_widget_contents(
            self._candidate_inspector_container,
            self._build_candidate_inspector_panel(),
        )
        self._set_widget_contents(
            self._coarse_tick_container,
            _build_placeholder_panel("No Data", "Coarse-versus-tick diagnostics will appear here."),
        )

        inspector_tabs = QTabWidget()
        inspector_tabs.addTab(self._summary_container, "Summary")
        inspector_tabs.addTab(self._candidate_inspector_container, "Candidates")
        inspector_tabs.addTab(self._candidate_frontier_container, "Frontier Raw")
        inspector_tabs.addTab(self._observed_targets_container, "Observed Targets")
        inspector_tabs.addTab(self._coarse_tick_container, "Coarse/Tick")
        return inspector_tabs

    def _build_compare_tab(self) -> QWidget:
        self._compare_left_view.setReadOnly(True)
        self._compare_right_view.setReadOnly(True)
        self._compare_diff_view.setReadOnly(True)
        self._compare_left_selector.currentIndexChanged.connect(self._on_compare_selection_changed)
        self._compare_right_selector.currentIndexChanged.connect(self._on_compare_selection_changed)

        selector_row = QWidget()
        selector_layout = QHBoxLayout(selector_row)
        selector_layout.addWidget(QLabel("Left candidate"))
        selector_layout.addWidget(self._compare_left_selector)
        selector_layout.addWidget(QLabel("Right candidate"))
        selector_layout.addWidget(self._compare_right_selector)
        swap_button = QPushButton("Swap")
        swap_button.clicked.connect(self._swap_compare_candidates)
        selector_layout.addWidget(swap_button)

        detail_splitter = QSplitter()
        detail_splitter.addWidget(self._compare_left_view)
        detail_splitter.addWidget(self._compare_right_view)
        detail_splitter.setSizes([700, 700])

        container = QWidget()
        layout = QVBoxLayout(container)
        layout.addWidget(selector_row)
        layout.addWidget(detail_splitter)
        layout.addWidget(self._compare_diff_view)
        return container

    def _build_candidate_inspector_panel(self) -> QWidget:
        self._candidate_table.setColumnCount(8)
        self._candidate_table.setHorizontalHeaderLabels(
            [
                "Candidate",
                "Stage",
                "Tier",
                "Admissible",
                "Valid",
                "Pareto",
                "Lexicographic",
                "Rejected",
            ]
        )
        self._candidate_table.itemSelectionChanged.connect(self._on_candidate_selection_changed)
        self._candidate_detail_view.setReadOnly(True)

        splitter = QSplitter()
        splitter.addWidget(self._candidate_table)
        splitter.addWidget(self._candidate_detail_view)
        splitter.setSizes([700, 800])

        panel = QWidget()

        layout = QVBoxLayout(panel)
        layout.addWidget(splitter)

        return panel

    def _build_actions(self) -> None:
        file_menu = self.menuBar().addMenu("File")

        open_run_action = file_menu.addAction("Open Existing Run")
        open_run_action.triggered.connect(self._open_existing_run_from_dialog)

        open_dataset_action = file_menu.addAction("Open Dataset In Setup")
        open_dataset_action.triggered.connect(self._open_dataset_in_setup_from_dialog)

        export_menu = self.menuBar().addMenu("Export")

        self._export_summary_action = export_menu.addAction("Export Summary JSON")
        self._export_summary_action.triggered.connect(self._export_summary_json)
        self._export_frontier_action = export_menu.addAction("Export Frontier JSON")
        self._export_frontier_action.triggered.connect(self._export_frontier_json)
        self._export_plots_action = export_menu.addAction("Export Plots")
        self._export_plots_action.triggered.connect(self._export_plots)

    def _set_actions_enabled(self, enabled: bool) -> None:
        self._export_summary_action.setEnabled(enabled)
        self._export_frontier_action.setEnabled(enabled)
        self._export_plots_action.setEnabled(enabled)

    def _open_dataset_in_setup_from_dialog(self) -> None:
        selected_path, _ = QFileDialog.getOpenFileName(
            self,
            "Open dataset",
            str(Path.cwd()),
            "Market data (*.parquet *.pq *.csv);;All files (*)",
        )
        if not selected_path:
            return

        self._root_tabs.setCurrentWidget(self._setup_widget)
        self._setup_widget.set_dataset_path(Path(selected_path))

    def _open_existing_run_from_dialog(self) -> None:
        selected_path, _ = QFileDialog.getOpenFileName(
            self,
            "Open run artifact",
            str(Path.cwd()),
            "JSON files (*.json);;All files (*)",
        )
        if not selected_path:
            selected_directory = QFileDialog.getExistingDirectory(
                self,
                "Open run directory",
                str(Path.cwd()),
            )
            if not selected_directory:
                return

            self._open_existing_input(Path(selected_directory))
            return

        self._open_existing_input(Path(selected_path))

    def _open_existing_input(self, input_path: Path) -> None:
        self._state.session.opened_input_path = input_path
        self._root_tabs.setCurrentIndex(1)
        self.statusBar().showMessage(f"Opening {input_path}")
        self._reset_loader_task_states()
        self._start_loading(input_path)

    def _start_execution(self, run_setup: WorkbenchRunSetup) -> None:
        self._root_tabs.setCurrentIndex(1)
        self._live_stage_report_payloads = []
        self._run_progress_bar.setRange(0, 0)
        self._run_status_label.setText(f"Starting calibration for {run_setup.dataset_path}")
        self._run_stage_label.setText("Stage: preparing")
        self._run_elapsed_label.setText("Elapsed: 0.0s")
        self._event_table.setRowCount(0)

        self._set_widget_contents(
            self._live_run_plot_container,
            _build_placeholder_panel(
                "Preparing",
                "Waiting for the first completed stage before rendering live search diagnostics.",
            ),
        )

        self._execution_thread = QThread(self)
        self._execution_worker = WorkbenchExecutionWorker(run_setup)
        self._execution_worker.moveToThread(self._execution_thread)

        self._execution_thread.started.connect(self._execution_worker.run)
        self._execution_worker.event_emitted.connect(self._on_execution_event)
        self._execution_worker.execution_completed.connect(self._on_execution_completed)
        self._execution_worker.error_raised.connect(self._on_load_error)
        self._execution_worker.finished.connect(self._execution_thread.quit)
        self._execution_worker.finished.connect(self._execution_worker.deleteLater)
        self._execution_thread.finished.connect(self._execution_thread.deleteLater)

        self._execution_thread.start()

    def _start_loading(self, input_path: Path) -> None:
        self._load_thread = QThread(self)
        self._load_worker = WorkbenchLoadWorker(input_path)
        self._load_worker.moveToThread(self._load_thread)

        self._load_thread.started.connect(self._load_worker.run)
        self._load_worker.activity_changed.connect(self._on_activity_changed)
        self._load_worker.task_state_changed.connect(self._on_task_state_changed)
        self._load_worker.artifacts_ready.connect(self._on_artifacts_ready)
        self._load_worker.plot_ready.connect(self._on_plot_ready)
        self._load_worker.error_raised.connect(self._on_load_error)
        self._load_worker.finished.connect(self._load_thread.quit)
        self._load_worker.finished.connect(self._load_worker.deleteLater)
        self._load_thread.finished.connect(self._load_thread.deleteLater)

        self._load_thread.start()

    def _on_execution_event(self, event: Any) -> None:
        apply_workbench_event(self._state, event)

        self._append_event_row(event)
        self._refresh_run_status_panel()
        self._apply_live_stage_report_from_event(event)

    def _on_execution_completed(self, result: Any) -> None:
        execution_result = result
        if not isinstance(execution_result, WorkbenchExecutionResult):
            return

        self._apply_artifacts(execution_result.artifacts)
        self._root_tabs.setCurrentIndex(2)
        self.statusBar().showMessage(f"Calibration completed at {execution_result.run_directory}")
        self._reset_loader_task_states()

        self._start_loading(execution_result.run_directory)

    def _on_activity_changed(self, task_title: str, detail: str) -> None:
        self._activity_title_label.setText(task_title)
        self._activity_detail_label.setText(detail)

        self.statusBar().showMessage(f"{task_title}: {detail}")

    def _on_task_state_changed(
        self,
        task_id: str,
        state: str,
        completed_count: int,
        total_count: int,
    ) -> None:
        self._overall_progress_bar.setRange(0, total_count)
        self._overall_progress_bar.setValue(completed_count)

        row_index = self._task_row_by_id.get(task_id)
        if row_index is None:
            return

        state_item = self._activity_table.item(row_index, 0)
        if state_item is None:
            state_item = QTableWidgetItem()
            self._activity_table.setItem(row_index, 0, state_item)

        state_item.setText(state)
        self._activity_table.resizeColumnsToContents()

        if state == "running":
            plot_container = self._plot_container_by_task_id.get(task_id)
            task_detail = self._activity_table.item(row_index, 2)
            if plot_container is not None and task_detail is not None:
                self._set_widget_contents(
                    plot_container,
                    _build_placeholder_panel("Loading", task_detail.text()),
                )

    def _on_artifacts_ready(self, artifacts: WorkbenchArtifacts) -> None:
        self._apply_artifacts(artifacts)
        self.statusBar().showMessage(f"Loaded {artifacts.source_path}")

    def _on_plot_ready(self, task_id: str, figure: Any) -> None:
        plot_container = self._plot_container_by_task_id.get(task_id)
        if plot_container is None:
            return

        self._set_widget_contents(plot_container, _figure_canvas(figure))

    def _on_load_error(self, message: str) -> None:
        self.statusBar().showMessage("Workbench loading failed")
        self._state.session.last_error_message = message
        self._run_status_label.setText(f"Failure: {message}")

        message_box = QMessageBox(self)
        message_box.setWindowTitle("Janiform Calibration Workbench")
        message_box.setText(message)
        message_box.setIcon(QMessageBox.Icon.Critical)
        message_box.exec()

    def _apply_artifacts(self, artifacts: WorkbenchArtifacts) -> None:
        self._artifacts = artifacts
        self._candidate_store = candidate_store_from_artifacts(artifacts)
        self._live_stage_report_payloads = []
        self._set_actions_enabled(True)
        self._state.session.mode = "inspector"

        summary_payload = build_summary_payload(artifacts)

        self._set_widget_contents(
            self._summary_container,
            self._build_summary_panel(summary_payload),
        )
        self._set_widget_contents(
            self._observed_targets_container,
            _build_table_tab(_artifact_rows(artifacts.observed_targets, "targets")),
        )
        self._set_widget_contents(
            self._candidate_frontier_container,
            _build_table_tab(_artifact_rows(artifacts.candidate_frontier, "frontier_entries")),
        )
        self._set_widget_contents(
            self._coarse_tick_container,
            _build_table_tab(_artifact_rows(artifacts.coarse_tick_consistency, "metrics")),
        )

        self._populate_candidate_table()
        self._populate_compare_selectors()

    def _populate_candidate_table(self) -> None:
        self._candidate_ids_by_row = []

        candidate_store = self._candidate_store
        if candidate_store is None:
            self._candidate_table.setRowCount(0)
            self._candidate_detail_view.clear()
            return

        self._candidate_table.setRowCount(len(candidate_store.candidates))
        for row_index, candidate in enumerate(candidate_store.candidates):
            self._candidate_ids_by_row.append(candidate.candidate_id)

            values = [
                candidate.candidate_id,
                candidate.stage,
                candidate.evaluation_tier,
                str(candidate.admissible),
                str(candidate.valid),
                "" if candidate.pareto_rank is None else str(candidate.pareto_rank),
                "" if candidate.lexicographic_rank is None else str(candidate.lexicographic_rank),
                candidate.rejection_reason or "",
            ]

            for column_index, value in enumerate(values):
                self._candidate_table.setItem(row_index, column_index, QTableWidgetItem(value))

        self._candidate_table.resizeColumnsToContents()

        selected_candidate_id = candidate_store.selected_candidate_id
        if selected_candidate_id is None:
            self._candidate_detail_view.clear()
            return

        try:
            selected_row = self._candidate_ids_by_row.index(selected_candidate_id)
        except ValueError:
            selected_row = 0 if self._candidate_ids_by_row else -1

        if selected_row >= 0:
            self._candidate_table.selectRow(selected_row)
            self._render_candidate_details(self._candidate_ids_by_row[selected_row])

    def _on_candidate_selection_changed(self) -> None:
        selected_ranges = self._candidate_table.selectedRanges()
        if len(selected_ranges) == 0:
            return

        selected_row = selected_ranges[0].topRow()
        if selected_row < 0 or selected_row >= len(self._candidate_ids_by_row):
            return

        self._render_candidate_details(self._candidate_ids_by_row[selected_row])

    def _render_candidate_details(self, candidate_id: str) -> None:
        candidate_store = self._candidate_store
        if candidate_store is None:
            self._candidate_detail_view.clear()
            return

        candidate = candidate_store.by_id.get(candidate_id)
        if candidate is None:
            self._candidate_detail_view.clear()
            return

        family_metric_payloads = [metric.raw_payload for metric in candidate.family_metrics]
        detail_payload = {
            "candidate_id": candidate.candidate_id,
            "stage": candidate.stage,
            "evaluation_tier": candidate.evaluation_tier,
            "admissible": candidate.admissible,
            "valid": candidate.valid,
            "pareto_rank": candidate.pareto_rank,
            "lexicographic_rank": candidate.lexicographic_rank,
            "scalar_tie_break": candidate.scalar_tie_break,
            "rejection_reason": candidate.rejection_reason,
            "canonical_coordinates": candidate.canonical_coordinates,
            "parameter_snapshot": candidate.parameter_snapshot,
            "family_metrics": family_metric_payloads,
            "raw_payload": candidate.raw_payload,
        }

        self._candidate_detail_view.setPlainText(
            json.dumps(detail_payload, indent=2, sort_keys=True)
        )
        self._render_compare_views()

    def _populate_compare_selectors(self) -> None:
        candidate_store = self._candidate_store
        candidate_ids = (
            []
            if candidate_store is None
            else [candidate.candidate_id for candidate in candidate_store.candidates]
        )

        for selector in (self._compare_left_selector, self._compare_right_selector):
            selector.blockSignals(True)
            selector.clear()
            selector.addItems(candidate_ids)
            selector.blockSignals(False)

        if len(candidate_ids) == 0:
            self._compare_left_view.clear()
            self._compare_right_view.clear()
            self._compare_diff_view.clear()
            return

        self._compare_left_selector.setCurrentIndex(0)
        self._compare_right_selector.setCurrentIndex(1 if len(candidate_ids) > 1 else 0)
        self._render_compare_views()

    def _on_compare_selection_changed(self) -> None:
        self._render_compare_views()

    def _swap_compare_candidates(self) -> None:
        left_index = self._compare_left_selector.currentIndex()
        right_index = self._compare_right_selector.currentIndex()
        self._compare_left_selector.setCurrentIndex(right_index)
        self._compare_right_selector.setCurrentIndex(left_index)

    def _render_compare_views(self) -> None:
        candidate_store = self._candidate_store
        if candidate_store is None or len(candidate_store.candidates) == 0:
            self._compare_left_view.clear()
            self._compare_right_view.clear()
            self._compare_diff_view.clear()
            return

        left_candidate = candidate_store.by_id.get(self._compare_left_selector.currentText())
        right_candidate = candidate_store.by_id.get(self._compare_right_selector.currentText())
        if left_candidate is None or right_candidate is None:
            self._compare_left_view.clear()
            self._compare_right_view.clear()
            self._compare_diff_view.clear()
            return

        left_payload = _candidate_compare_payload(left_candidate)
        right_payload = _candidate_compare_payload(right_candidate)
        self._compare_left_view.setPlainText(json.dumps(left_payload, indent=2, sort_keys=True))
        self._compare_right_view.setPlainText(json.dumps(right_payload, indent=2, sort_keys=True))
        self._compare_diff_view.setPlainText(_compare_payloads_text(left_payload, right_payload))

    def _apply_live_stage_report_from_event(self, event: Any) -> None:
        if getattr(event, "event_type", None) != WorkbenchEventType.STAGE_COMPLETED:
            return

        payload = getattr(event, "payload", None)
        if not isinstance(payload, dict):
            return

        stage_report = payload.get("stage_report")
        if not isinstance(stage_report, dict):
            return

        stage_report_kind = stage_report.get("stage_report_kind")
        if not isinstance(stage_report_kind, str):
            return

        existing_position = next(
            (
                position
                for position, existing_stage_report in enumerate(self._live_stage_report_payloads)
                if existing_stage_report.get("stage_report_kind") == stage_report_kind
            ),
            None,
        )

        if existing_position is None:
            self._live_stage_report_payloads.append(dict(stage_report))
        else:
            self._live_stage_report_payloads[existing_position] = dict(stage_report)

        self._candidate_store = candidate_store_from_stage_report_payloads(
            tuple(self._live_stage_report_payloads)
        )

        self._populate_candidate_table()

        self._set_widget_contents(
            self._candidate_frontier_container,
            _build_table_tab(_live_frontier_rows(self._candidate_store)),
        )
        self._set_widget_contents(
            self._summary_container,
            self._build_live_summary_panel(),
        )
        self._set_widget_contents(
            self._live_run_plot_container,
            _figure_canvas(_build_live_candidate_search_figure(self._candidate_store)),
        )
        self._populate_compare_selectors()

    def _refresh_run_status_panel(self) -> None:
        run_state = self._state.run

        self._run_status_label.setText(
            f"Status: {run_state.status} | Run ID: {run_state.run_id or 'pending'}"
        )
        self._run_stage_label.setText(
            f"Stage: {run_state.current_stage or 'idle'} | Subtask: {run_state.current_subtask or 'idle'}"
        )
        self._run_elapsed_label.setText(f"Elapsed: {run_state.elapsed_seconds:.1f}s")

        if run_state.status == "running":
            self._run_progress_bar.setRange(0, 0)
        else:
            self._run_progress_bar.setRange(0, 1)
            self._run_progress_bar.setValue(1 if run_state.status == "completed" else 0)

    def _append_event_row(self, event: Any) -> None:
        row_index = self._event_table.rowCount()

        self._event_table.insertRow(row_index)

        event_time = str(event.occurred_at_utc).split("T")[-1].replace("+00:00", "Z")

        detail = (
            event.payload.get("detail")
            or event.payload.get("message")
            or event.payload.get("artifact_label")
            or json.dumps(event.payload, sort_keys=True)
        )
        values = [
            event_time,
            event.event_type.value,
            event.stage or "",
            str(detail),
        ]

        for column_index, value in enumerate(values):
            self._event_table.setItem(row_index, column_index, QTableWidgetItem(value))

        self._event_table.resizeColumnsToContents()

    def _reset_loader_task_states(self) -> None:
        for row_index in range(self._activity_table.rowCount()):
            self._activity_table.setItem(row_index, 0, QTableWidgetItem("queued"))

        self._overall_progress_bar.setValue(0)
        self._activity_title_label.setText("Queued")
        self._activity_detail_label.setText("Preparing diagnostics and plots.")

    def _build_summary_panel(self, summary_payload: dict[str, Any]) -> QWidget:
        decision = self._artifacts.selection_rationale.get("decision", {}) if self._artifacts else {}
        decisive_metric_ids = decision.get("decisive_metric_ids", [])
        candidate_counts_by_stage = summary_payload.get("candidate_counts_by_stage", {})
        model_payload = {} if self._artifacts is None or self._artifacts.model_payload is None else self._artifacts.model_payload

        summary_widget = QWidget()
        layout = QVBoxLayout(summary_widget)

        header = QLabel(
            f"Source: {summary_payload['source_path']}\n"
            f"Selected candidate: {summary_payload['selected_candidate_id']}\n"
            f"Selection stage: {decision.get('selection_stage', 'unknown')}\n"
            f"Selection policy: {decision.get('selection_policy', 'unknown')}\n"
            f"Candidates: {summary_payload['candidate_count']}\n"
            f"Admissible candidates: {summary_payload['admissible_candidate_count']}\n"
            f"Maximum coarse/tick delta: {summary_payload['maximum_normalized_coarse_tick_delta']:.3f}\n"
            f"Runtime total: {summary_payload['runtime_seconds_total']:.2f}s\n"
            f"Candidates by stage: {json.dumps(candidate_counts_by_stage, sort_keys=True)}\n"
            f"Decisive metrics: {', '.join(decisive_metric_ids) if decisive_metric_ids else 'none'}"
        )
        header.setWordWrap(True)

        layout.addWidget(header)

        rationale_view = QTextEdit()
        rationale_view.setReadOnly(True)
        rationale_view.setPlainText(
            json.dumps(
                {
                    "summary_payload": summary_payload,
                    "selection_rationale": self._artifacts.selection_rationale if self._artifacts else {},
                    "model": model_payload,
                },
                indent=2,
                sort_keys=True,
            )
        )

        layout.addWidget(rationale_view)

        return summary_widget

    def _build_live_summary_panel(self) -> QWidget:
        candidate_store = self._candidate_store
        run_state = self._state.run

        summary_widget = QWidget()
        layout = QVBoxLayout(summary_widget)

        header = QLabel(
            f"Run ID: {run_state.run_id or 'pending'}\n"
            f"Status: {run_state.status}\n"
            f"Current stage: {run_state.current_stage or 'idle'}\n"
            f"Elapsed: {run_state.elapsed_seconds:.1f}s\n"
            f"Observed stage reports: {len(self._live_stage_report_payloads)}\n"
            f"Candidates available: {0 if candidate_store is None else len(candidate_store.candidates)}"
        )
        header.setWordWrap(True)

        layout.addWidget(header)

        detail_view = QTextEdit()
        detail_view.setReadOnly(True)
        detail_view.setPlainText(
            json.dumps(
                {
                    "run": {
                        "run_id": run_state.run_id,
                        "status": run_state.status,
                        "current_stage": run_state.current_stage,
                        "current_subtask": run_state.current_subtask,
                        "elapsed_seconds": run_state.elapsed_seconds,
                    },
                    "stage_reports": self._live_stage_report_payloads,
                },
                indent=2,
                sort_keys=True,
            )
        )

        layout.addWidget(detail_view)

        return summary_widget

    def _export_summary_json(self) -> None:
        if self._artifacts is None:
            return

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
        if self._artifacts is None:
            return

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
        if self._artifacts is None:
            return

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

    def _set_widget_contents(self, container: QWidget, child: QWidget) -> None:
        layout = container.layout()
        if layout is None:
            layout = QVBoxLayout(container)

        while layout.count() > 0:
            item = layout.takeAt(0)
            existing_widget = item.widget()
            if existing_widget is not None:
                existing_widget.deleteLater()

        layout.addWidget(child)


def launch_workbench(path: Optional[str | Path] = None) -> int:
    application = QApplication(sys.argv)

    try:
        input_path = None if path is None else Path(path).expanduser().resolve()
        window = WorkbenchWindow(input_path)
    except Exception as error:
        message_box = QMessageBox()
        message_box.setWindowTitle("Janiform Calibration Workbench")
        message_box.setText(str(error))
        message_box.setIcon(QMessageBox.Icon.Critical)
        message_box.exec()

        return 1

    window.show()
    return application.exec()


def _artifact_rows(payload: dict[str, Any], field_name: str) -> list[dict[str, Any]]:
    rows = payload.get(field_name, [])
    if isinstance(rows, list):
        return [row for row in rows if isinstance(row, dict)]

    return []


def _live_frontier_rows(
    candidate_store: Optional[WorkbenchCandidateStore],
) -> list[dict[str, Any]]:
    if candidate_store is None:
        return []

    rows: list[dict[str, Any]] = []
    for candidate in candidate_store.candidates:
        rows.append(
            {
                "candidate_id": candidate.candidate_id,
                "stage": candidate.stage,
                "evaluation_tier": candidate.evaluation_tier,
                "admissible": candidate.admissible,
                "valid": candidate.valid,
                "pareto_rank": candidate.pareto_rank,
                "lexicographic_rank": candidate.lexicographic_rank,
                "scalar_tie_break": candidate.scalar_tie_break,
                "rejection_reason": candidate.rejection_reason,
                "canonical_coordinates": candidate.canonical_coordinates,
            }
        )

    return rows


def _build_placeholder_panel(title: str, detail: str) -> QWidget:
    widget = QWidget()

    layout = QVBoxLayout(widget)

    title_label = QLabel(title)
    detail_label = QLabel(detail)
    detail_label.setWordWrap(True)

    layout.addWidget(title_label)
    layout.addWidget(detail_label)

    return widget


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


def _build_live_candidate_search_figure(
    candidate_store: Optional[WorkbenchCandidateStore],
) -> Figure:
    figure = Figure(figsize=(10, 7), tight_layout=True)

    axes = figure.subplots(2, 2)
    if candidate_store is None or len(candidate_store.candidates) == 0:
        for axis in axes.flat:
            axis.text(
                0.5,
                0.5,
                "No live candidates yet.",
                ha="center",
                va="center",
                transform=axis.transAxes,
            )
            axis.set_axis_off()

        return figure

    admissible_candidates = [
        candidate for candidate in candidate_store.candidates if candidate.admissible
    ]

    candidate_sequence = list(range(1, len(candidate_store.candidates) + 1))

    objectives = [
        candidate.scalar_tie_break if candidate.scalar_tie_break is not None else float("nan")
        for candidate in candidate_store.candidates
    ]

    colors = [
        "#1f77b4" if candidate.admissible else "#d62728"
        for candidate in candidate_store.candidates
    ]

    axes[0, 0].scatter(candidate_sequence, objectives, c=colors, alpha=0.8)
    axes[0, 0].set_title("Candidates vs Objective")
    axes[0, 0].set_xlabel("Candidate order")
    axes[0, 0].set_ylabel("Objective")

    stage_counts: dict[str, int] = {}
    for candidate in candidate_store.candidates:
        stage_counts[candidate.stage] = stage_counts.get(candidate.stage, 0) + 1

    axes[0, 1].bar(stage_counts.keys(), stage_counts.values(), color="#2ca02c")
    axes[0, 1].set_title("Candidates by Stage")
    axes[0, 1].set_ylabel("Count")
    axes[0, 1].tick_params(axis="x", rotation=15)

    first_coordinate_name = next(
        (
            coordinate_name
            for candidate in candidate_store.candidates
            for coordinate_name in candidate.canonical_coordinates
        ),
        None,
    )
    if first_coordinate_name is None:
        axes[1, 0].text(
            0.5,
            0.5,
            "No canonical coordinate search surface yet.",
            ha="center",
            va="center",
            transform=axes[1, 0].transAxes,
        )
        axes[1, 0].set_axis_off()
    else:
        coordinate_values = [
            candidate.canonical_coordinates.get(first_coordinate_name, float("nan"))
            for candidate in candidate_store.candidates
        ]

        axes[1, 0].scatter(coordinate_values, objectives, c=colors, alpha=0.8)
        axes[1, 0].set_title(f"{first_coordinate_name} vs Objective")
        axes[1, 0].set_xlabel(first_coordinate_name)
        axes[1, 0].set_ylabel("Objective")

    family_distance_by_name: dict[str, list[float]] = {}
    for candidate in admissible_candidates:
        for family_metric in candidate.family_metrics:
            if family_metric.normalized_distance is None:
                continue

            family_distance_by_name.setdefault(family_metric.family_id, []).append(
                family_metric.normalized_distance
            )

    if len(family_distance_by_name) == 0:
        axes[1, 1].text(
            0.5,
            0.5,
            "No family metrics yet.",
            ha="center",
            va="center",
            transform=axes[1, 1].transAxes,
        )
        axes[1, 1].set_axis_off()
    else:
        family_names = list(family_distance_by_name)

        average_distances = [
            sum(values) / len(values) for values in family_distance_by_name.values()
        ]

        axes[1, 1].barh(family_names, average_distances, color="#9467bd")
        axes[1, 1].set_title("Mean Family Distance")
        axes[1, 1].set_xlabel("Normalized distance")

    return figure


def _choose_output_path(
    parent: QWidget,
    *,
    caption: str,
    suggested_name: str,
) -> Optional[Path]:
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
        elif isinstance(value, list):
            flattened[field_name] = json.dumps(value, sort_keys=True)
        else:
            flattened[field_name] = str(value)

    return flattened


def _candidate_compare_payload(candidate: Any) -> dict[str, Any]:
    return {
        "candidate_id": candidate.candidate_id,
        "stage": candidate.stage,
        "evaluation_tier": candidate.evaluation_tier,
        "admissible": candidate.admissible,
        "valid": candidate.valid,
        "pareto_rank": candidate.pareto_rank,
        "lexicographic_rank": candidate.lexicographic_rank,
        "scalar_tie_break": candidate.scalar_tie_break,
        "rejection_reason": candidate.rejection_reason,
        "canonical_coordinates": candidate.canonical_coordinates,
        "parameter_snapshot": candidate.parameter_snapshot,
        "family_metrics": [metric.raw_payload for metric in candidate.family_metrics],
        "raw_payload": candidate.raw_payload,
    }


def _compare_payloads_text(left_payload: dict[str, Any], right_payload: dict[str, Any]) -> str:
    left_flat = _flatten_mapping(left_payload)
    right_flat = _flatten_mapping(right_payload)
    keys = sorted(set(left_flat) | set(right_flat))
    differences = [
        f"{key}: left={left_flat.get(key, '<missing>')} | right={right_flat.get(key, '<missing>')}"
        for key in keys
        if left_flat.get(key) != right_flat.get(key)
    ]
    if len(differences) == 0:
        return "Selected candidates are identical in the rendered compare payload."

    return "\n".join(differences)


__all__ = ["launch_workbench"]
