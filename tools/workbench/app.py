from __future__ import annotations

import json
import os
import sys
import tempfile

from collections.abc import Callable
from functools import partial
from pathlib import Path
from typing import Any, Optional, override

from PyQt6 import sip
from PyQt6.QtCore import Qt, QThread, QTimer
from PyQt6.QtGui import QCloseEvent, QPixmap, QResizeEvent, QShowEvent
from PyQt6.QtWidgets import (
    QApplication,
    QCheckBox,
    QComboBox,
    QFileDialog,
    QHeaderView,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMainWindow,
    QMessageBox,
    QPushButton,
    QProgressBar,
    QSizePolicy,
    QSplitter,
    QScrollArea,
    QTableWidget,
    QTableWidgetItem,
    QTabWidget,
    QTextEdit,
    QToolButton,
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
    WorkbenchCandidateSnapshot,
    WorkbenchCandidateStore,
    candidate_store_from_artifacts,
    candidate_store_from_stage_report_payloads,
)
from .execution import WorkbenchExecutionResult, WorkbenchExecutionWorker
from .exporting import WorkbenchPlotExportWorker
from .fit_context import load_fit_context
from .lab import (
    WorkbenchReevaluationRequest,
    WorkbenchReevaluationResult,
    WorkbenchReevaluationWorker,
    apply_reevaluation_request,
)
from .loading import WorkbenchLoadWorker, workbench_startup_tasks
from .parameters import flatten_parameter_tree, parse_parameter_value
from .plot_manifest import WorkbenchPlotManifestEntry, workbench_plot_manifest
from .plots import (
    create_observed_simulated_figure,
    create_parameter_objective_figure_with_columns,
    create_parameter_search_figure,
    create_td_sd_horizon_profile_figure,
    create_td_sd_return_distribution_figure,
)
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
        self._plot_manifest_by_task_id: dict[str, WorkbenchPlotManifestEntry] = {
            manifest_entry.task_id: manifest_entry
            for manifest_entry in workbench_plot_manifest()
        }
        self._plot_container_by_task_id: dict[str, QWidget] = {}
        self._plot_group_tabs_by_group_id: dict[str, QTabWidget] = {}
        self._plot_group_id_by_task_id: dict[str, str] = {}
        self._plot_tab_index_by_task_id: dict[str, int] = {}
        self._plot_task_id_by_group_and_index: dict[tuple[str, int], str] = {}
        self._plot_group_id_by_outer_index: dict[int, str] = {}
        self._plot_image_bytes_by_task_id: dict[str, bytes] = {}
        self._plot_figure_by_task_id: dict[str, Figure] = {}
        self._plot_canvas_ready_by_task_id: dict[str, bool] = {}
        self._inspector_tab_key_by_index: dict[int, str] = {}
        self._inspector_tab_loaded_by_key: dict[str, bool] = {}
        self._candidate_ids_by_row: list[str] = []
        self._live_stage_report_payloads: list[dict[str, Any]] = []
        self._pending_candidate_rows: list[WorkbenchCandidateSnapshot] = []
        self._pending_selected_candidate_id: Optional[str] = None
        self._artifact_materialization_steps: list[Callable[[], None]] = []
        self._candidate_population_timer = QTimer(self)
        self._artifact_materialization_timer = QTimer(self)
        self._load_thread: Optional[QThread] = None
        self._load_worker: Optional[WorkbenchLoadWorker] = None
        self._execution_thread: Optional[QThread] = None
        self._execution_worker: Optional[WorkbenchExecutionWorker] = None
        self._export_thread: Optional[QThread] = None
        self._export_worker: Optional[WorkbenchPlotExportWorker] = None

        self._setup_widget = WorkbenchSetupWidget()
        self._root_tabs = QTabWidget()
        self._inspector_tabs = QTabWidget()
        self._summary_container = QWidget()
        self._observed_targets_container = QWidget()
        self._candidate_frontier_container = QWidget()
        self._candidate_inspector_container = QWidget()
        self._coarse_tick_container = QWidget()
        self._compare_container = QWidget()
        self._lab_container = QWidget()
        self._plot_group_tabs = QTabWidget()
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
        self._lab_source_selector = QComboBox()
        self._lab_seed_input = QLineEdit()
        self._lab_exact_checkbox = QCheckBox("Exact rescoring")
        self._lab_seeded_checkbox = QCheckBox("Seeded-tick diagnostics")
        self._lab_coarse_checkbox = QCheckBox("Coarse diagnostics")
        self._lab_consistency_checkbox = QCheckBox("Cross-tier consistency")
        self._lab_parameter_table = QTableWidget()
        self._lab_status_label = QLabel()
        self._lab_result_summary_view = QTextEdit()
        self._lab_plot_tabs = QTabWidget()
        self._lab_run_button = QPushButton("Reevaluate")
        self._lab_reset_button = QPushButton("Reset edits")
        self._lab_result: Optional[WorkbenchReevaluationResult] = None
        self._lab_thread: Optional[QThread] = None
        self._lab_worker: Optional[WorkbenchReevaluationWorker] = None

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
        self._candidate_population_timer.timeout.connect(self._append_candidate_table_chunk)
        self._artifact_materialization_timer.timeout.connect(
            self._run_next_artifact_materialization_step
        )
        self._root_tabs.currentChanged.connect(self._on_root_tab_changed)
        self._plot_group_tabs.currentChanged.connect(self._on_plot_group_changed)

        for plot_manifest_entry in workbench_plot_manifest():
            plot_group_id, plot_group_title = _plot_group_for_manifest_entry(plot_manifest_entry)
            group_tabs = self._plot_group_tabs_by_group_id.get(plot_group_id)
            if group_tabs is None:
                group_tabs = QTabWidget()
                group_tabs.currentChanged.connect(
                    partial(self._on_plot_tab_changed, plot_group_id)
                )
                outer_tab_index = self._plot_group_tabs.addTab(group_tabs, plot_group_title)
                self._plot_group_tabs_by_group_id[plot_group_id] = group_tabs
                self._plot_group_id_by_outer_index[outer_tab_index] = plot_group_id

            plot_container = QWidget()

            self._plot_container_by_task_id[plot_manifest_entry.task_id] = plot_container
            self._plot_group_id_by_task_id[plot_manifest_entry.task_id] = plot_group_id

            self._set_widget_contents(
                plot_container,
                self._build_plot_tab_contents(
                    plot_manifest_entry.task_id,
                    _build_placeholder_panel("Queued", plot_manifest_entry.description),
                ),
            )

            tab_index = group_tabs.addTab(plot_container, plot_manifest_entry.tab_title)

            self._plot_tab_index_by_task_id[plot_manifest_entry.task_id] = tab_index
            self._plot_task_id_by_group_and_index[(plot_group_id, tab_index)] = (
                plot_manifest_entry.task_id
            )

        self._root_tabs.addTab(self._setup_widget, "Setup")
        self._root_tabs.addTab(self._build_run_tab(), "Run")
        self._root_tabs.addTab(self._build_inspector_tab(), "Inspector")
        self._root_tabs.addTab(self._build_compare_tab(), "Compare")
        self._root_tabs.addTab(self._build_lab_tab(), "Lab")
        self._root_tabs.addTab(self._plot_group_tabs, "Plots")

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

        self._inspector_tabs.currentChanged.connect(self._on_inspector_tab_changed)
        self._inspector_tabs.clear()
        self._inspector_tab_key_by_index.clear()
        self._inspector_tab_loaded_by_key.clear()

        self._add_inspector_tab("summary", self._summary_container, "Summary")
        self._add_inspector_tab("candidates", self._candidate_inspector_container, "Candidates")
        self._add_inspector_tab(
            "candidate_frontier",
            self._candidate_frontier_container,
            "Frontier Raw",
        )
        self._add_inspector_tab(
            "observed_targets",
            self._observed_targets_container,
            "Observed Targets",
        )
        self._add_inspector_tab("coarse_tick", self._coarse_tick_container, "Coarse/Tick")

        self._inspector_tab_loaded_by_key["candidates"] = True

        return self._inspector_tabs

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

    def _build_lab_tab(self) -> QWidget:
        self._lab_status_label.setWordWrap(True)
        self._lab_status_label.setText(
            "Load a run to edit fitted model parameters and run simulation-backed reevaluation."
        )
        self._lab_seed_input.setText("1")
        self._lab_seeded_checkbox.setChecked(True)
        self._lab_coarse_checkbox.setChecked(True)
        self._lab_consistency_checkbox.setChecked(True)
        self._lab_result_summary_view.setReadOnly(True)

        self._lab_run_button.clicked.connect(self._start_lab_reevaluation)
        self._lab_reset_button.clicked.connect(self._reset_lab_edits)

        selector_row = QWidget()
        selector_layout = QHBoxLayout(selector_row)
        selector_layout.addWidget(QLabel("Source"))
        selector_layout.addWidget(self._lab_source_selector)
        selector_layout.addWidget(QLabel("Seed"))
        selector_layout.addWidget(self._lab_seed_input)
        selector_layout.addWidget(self._lab_exact_checkbox)
        selector_layout.addWidget(self._lab_seeded_checkbox)
        selector_layout.addWidget(self._lab_coarse_checkbox)
        selector_layout.addWidget(self._lab_consistency_checkbox)
        selector_layout.addWidget(self._lab_run_button)
        selector_layout.addWidget(self._lab_reset_button)

        self._lab_parameter_table.setColumnCount(4)
        self._lab_parameter_table.setHorizontalHeaderLabels(
            ["Path", "Block", "Baseline", "Edited value"]
        )
        self._configure_lab_parameter_table_columns()

        self._lab_plot_tabs.clear()
        self._lab_plot_tabs.addTab(
            _build_placeholder_panel("No Reevaluation", "Run a reevaluation to populate lab plots."),
            "Edited vs Observed",
        )

        layout = QVBoxLayout(self._lab_container)
        layout.addWidget(self._lab_status_label)
        layout.addWidget(selector_row)
        layout.addWidget(self._lab_parameter_table)
        layout.addWidget(self._lab_result_summary_view)
        layout.addWidget(self._lab_plot_tabs)

        return self._lab_container

    def _build_candidate_inspector_panel(self) -> QWidget:
        self._candidate_table.setColumnCount(10)
        self._candidate_table.setHorizontalHeaderLabels(
            [
                "Candidate",
                "Stage",
                "Family",
                "Source",
                "Tier",
                "Admissibility",
                "Validity",
                "Objective",
                "Rejected",
                "Created",
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

    def _add_inspector_tab(self, tab_key: str, container: QWidget, tab_title: str) -> None:
        tab_index = self._inspector_tabs.addTab(container, tab_title)
        self._inspector_tab_key_by_index[tab_index] = tab_key
        self._inspector_tab_loaded_by_key[tab_key] = False

    def _build_actions(self) -> None:
        file_menu = self.menuBar().addMenu("File")

        open_run_action = file_menu.addAction("Open Existing Run")
        open_dataset_action = file_menu.addAction("Open Dataset In Setup")

        open_run_action.triggered.connect(self._open_existing_run_from_dialog)
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
        self._export_plots_action.setEnabled(enabled and self._export_thread is None)

    @override
    def closeEvent(self, event: QCloseEvent) -> None:
        self._shutdown_background_workers()
        super().closeEvent(event)

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
        selected_directory = QFileDialog.getExistingDirectory(
            self,
            "Open run directory",
            str((Path.cwd() / ".janiform").resolve()),
        )
        if not selected_directory:
            return

        self._open_existing_input(Path(selected_directory))

    def _open_existing_input(self, input_path: Path) -> None:
        self._state.session.opened_input_path = input_path
        self._root_tabs.setCurrentIndex(1)
        self.statusBar().showMessage(f"Opening {input_path}")
        self._reset_loader_task_states()
        self._start_loading(input_path)

    def _start_execution(self, run_setup: WorkbenchRunSetup) -> None:
        self._stop_execution_worker()

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
        self._stop_load_worker()

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
                    self._build_plot_tab_contents(
                        task_id,
                        _build_placeholder_panel("Loading", task_detail.text()),
                    ),
                )

    def _on_artifacts_ready(self, artifacts: WorkbenchArtifacts) -> None:
        self._apply_artifacts(artifacts)
        self.statusBar().showMessage(f"Loaded {artifacts.source_path}")

    def _on_plot_ready(self, task_id: str, plot_payload: Any) -> None:
        plot_container = self._plot_container_by_task_id.get(task_id)
        if plot_container is None:
            return

        if isinstance(plot_payload, bytes):
            self._plot_image_bytes_by_task_id[task_id] = plot_payload
        elif isinstance(plot_payload, Figure):
            self._plot_figure_by_task_id[task_id] = plot_payload
        else:
            return

        self._plot_canvas_ready_by_task_id[task_id] = False

        selected_task_id = self._selected_plot_task_id()
        if selected_task_id == task_id:
            self._render_plot_canvas(task_id)
            return

        self._set_widget_contents(
            plot_container,
            self._build_plot_tab_contents(
                task_id,
                _build_placeholder_panel("Ready", "Plot data is ready. Open this tab to render it."),
            ),
        )

    def _on_plot_group_changed(self, group_index: int) -> None:
        group_id = self._plot_group_id_by_outer_index.get(group_index)
        if group_id is None:
            return

        group_tabs = self._plot_group_tabs_by_group_id.get(group_id)
        if group_tabs is None:
            return

        self._on_plot_tab_changed(group_id, group_tabs.currentIndex())

    def _on_plot_tab_changed(self, group_id: str, tab_index: int) -> None:
        task_id = self._plot_task_id_by_group_and_index.get((group_id, tab_index))
        if task_id is None:
            return

        self._render_plot_canvas(task_id)

    def _selected_plot_task_id(self) -> Optional[str]:
        current_group_id = self._plot_group_id_by_outer_index.get(
            self._plot_group_tabs.currentIndex()
        )
        if current_group_id is None:
            return None

        group_tabs = self._plot_group_tabs_by_group_id.get(current_group_id)
        if group_tabs is None:
            return None

        return self._plot_task_id_by_group_and_index.get(
            (current_group_id, group_tabs.currentIndex())
        )

    def _on_root_tab_changed(self, tab_index: int) -> None:
        current_widget = self._root_tabs.widget(tab_index)
        if current_widget is self._inspector_tabs:
            self._ensure_selected_inspector_tab_loaded()
            return

        if current_widget is self._lab_container:
            self._ensure_lab_parameters_loaded()

    def _on_inspector_tab_changed(self, tab_index: int) -> None:
        tab_key = self._inspector_tab_key_by_index.get(tab_index)
        if tab_key is None:
            return

        self._ensure_inspector_tab_loaded(tab_key)

    def _render_plot_canvas(self, task_id: str) -> None:
        if self._plot_canvas_ready_by_task_id.get(task_id):
            return

        plot_container = self._plot_container_by_task_id.get(task_id)
        if plot_container is None:
            return

        if task_id == "plot_parameter_search" and self._artifacts is not None:
            self._set_widget_contents(
                plot_container,
                self._build_plot_tab_contents(
                    task_id,
                    _build_parameter_search_plot_panel(self._artifacts),
                ),
            )

            self._plot_canvas_ready_by_task_id[task_id] = True
            return

        if task_id == "plot_parameter_objective" and self._artifacts is not None:
            self._set_widget_contents(
                plot_container,
                self._build_plot_tab_contents(
                    task_id,
                    _build_parameter_objective_plot_panel(self._artifacts),
                ),
            )

            self._plot_canvas_ready_by_task_id[task_id] = True
            return

        image_bytes = self._plot_image_bytes_by_task_id.get(task_id)
        if image_bytes is not None:
            self._set_widget_contents(
                plot_container,
                self._build_plot_tab_contents(task_id, _image_canvas(image_bytes)),
            )
            self._plot_canvas_ready_by_task_id[task_id] = True

            return

        figure = self._plot_figure_by_task_id.get(task_id)
        if figure is None:
            return

        self._set_widget_contents(
            plot_container,
            self._build_plot_tab_contents(task_id, _figure_canvas(figure)),
        )

        self._plot_canvas_ready_by_task_id[task_id] = True

    def _build_plot_tab_contents(self, task_id: str, body: QWidget) -> QWidget:
        manifest_entry = self._plot_manifest_by_task_id.get(task_id)
        if manifest_entry is None:
            return body

        container = QWidget()
        layout = QVBoxLayout(container)

        header_row = QWidget()
        header_layout = QHBoxLayout(header_row)
        header_layout.setContentsMargins(0, 0, 0, 0)

        title_label = QLabel(manifest_entry.tab_title)
        help_button = QToolButton()
        help_button.setText("?")
        help_button.setToolTip("Explain what this plot shows and how to read it.")
        help_button.clicked.connect(
            lambda _checked=False, task_id=task_id: self._show_plot_help(task_id)
        )

        header_layout.addWidget(title_label)
        header_layout.addStretch(1)
        header_layout.addWidget(help_button)

        description_label = QLabel(manifest_entry.description)
        description_label.setWordWrap(True)

        layout.addWidget(header_row)
        layout.addWidget(description_label)
        layout.addWidget(body)

        return container

    def _show_plot_help(self, task_id: str) -> None:
        manifest_entry = self._plot_manifest_by_task_id.get(task_id)
        if manifest_entry is None:
            return

        message_box = QMessageBox(self)
        message_box.setWindowTitle(f"{manifest_entry.tab_title} Help")
        message_box.setIcon(QMessageBox.Icon.Information)
        message_box.setText(
            "\n\n".join(
                [
                    manifest_entry.description,
                    "How to read it:\n- " + "\n- ".join(manifest_entry.how_to_read),
                ]
            )
        )

        message_box.exec()

    def _schedule_artifact_materialization(self) -> None:
        self._artifact_materialization_steps = [
            self._materialize_summary_panel,
            self._materialize_candidate_store,
        ]

        self._artifact_materialization_timer.start(0)

    def _run_next_artifact_materialization_step(self) -> None:
        if len(self._artifact_materialization_steps) == 0:
            self._artifact_materialization_timer.stop()
            self._ensure_selected_inspector_tab_loaded()

            return

        step = self._artifact_materialization_steps.pop(0)
        step()

    def _materialize_summary_panel(self) -> None:
        artifacts = self._artifacts
        if artifacts is None:
            return

        summary_payload = build_summary_payload(artifacts)

        self._set_widget_contents(
            self._summary_container,
            self._build_summary_panel(summary_payload),
        )

        self._inspector_tab_loaded_by_key["summary"] = True

    def _materialize_candidate_store(self) -> None:
        artifacts = self._artifacts
        if artifacts is None:
            return

        self._candidate_store = candidate_store_from_artifacts(artifacts)

        self._populate_candidate_table()
        self._populate_compare_selectors()
        self._populate_lab_source_selector()

    def _ensure_selected_inspector_tab_loaded(self) -> None:
        current_index = self._inspector_tabs.currentIndex()

        tab_key = self._inspector_tab_key_by_index.get(current_index)
        if tab_key is None:
            return

        self._ensure_inspector_tab_loaded(tab_key)

    def _ensure_inspector_tab_loaded(self, tab_key: str) -> None:
        if self._inspector_tab_loaded_by_key.get(tab_key):
            return

        artifacts = self._artifacts
        if artifacts is None:
            return

        match tab_key:
            case "summary":
                self._materialize_summary_panel()
            case "candidate_frontier":
                self._set_widget_contents(
                    self._candidate_frontier_container,
                    _build_table_tab(_artifact_rows(artifacts.candidate_frontier, "frontier_entries")),
                )
                self._inspector_tab_loaded_by_key[tab_key] = True
            case "observed_targets":
                self._set_widget_contents(
                    self._observed_targets_container,
                    _build_table_tab(_artifact_rows(artifacts.observed_targets, "targets")),
                )
                self._inspector_tab_loaded_by_key[tab_key] = True
            case "coarse_tick":
                self._set_widget_contents(
                    self._coarse_tick_container,
                    _build_table_tab(_artifact_rows(artifacts.coarse_tick_consistency, "metrics")),
                )
                self._inspector_tab_loaded_by_key[tab_key] = True
            case _:
                return

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
        self._candidate_store = None
        self._live_stage_report_payloads = []
        self._set_actions_enabled(True)
        self._state.session.mode = "inspector"

        self._candidate_population_timer.stop()

        self._set_widget_contents(
            self._summary_container,
            _build_placeholder_panel("Preparing Summary", "Summarizing the selected run."),
        )
        self._set_widget_contents(
            self._observed_targets_container,
            _build_placeholder_panel(
                "Deferred",
                "Open this tab to materialize observed-target rows.",
            ),
        )
        self._set_widget_contents(
            self._candidate_frontier_container,
            _build_placeholder_panel(
                "Deferred",
                "Open this tab to materialize frontier rows.",
            ),
        )
        self._set_widget_contents(
            self._coarse_tick_container,
            _build_placeholder_panel(
                "Deferred",
                "Open this tab to materialize coarse-versus-tick rows.",
            ),
        )

        self._inspector_tab_loaded_by_key["summary"] = False
        self._inspector_tab_loaded_by_key["candidate_frontier"] = False
        self._inspector_tab_loaded_by_key["observed_targets"] = False
        self._inspector_tab_loaded_by_key["coarse_tick"] = False
        self._inspector_tab_loaded_by_key["candidates"] = True

        self._candidate_table.setRowCount(0)
        self._candidate_detail_view.clear()

        self._populate_compare_selectors()
        self._populate_lab_controls()

        self._schedule_artifact_materialization()

    def _populate_candidate_table(self) -> None:
        self._candidate_ids_by_row = []
        self._pending_candidate_rows = []
        self._pending_selected_candidate_id = None

        self._candidate_population_timer.stop()

        candidate_store = self._candidate_store
        if candidate_store is None:
            self._candidate_table.setRowCount(0)
            self._candidate_detail_view.clear()

            return

        self._candidate_table.setRowCount(0)

        self._pending_candidate_rows = list(candidate_store.candidates)
        self._pending_selected_candidate_id = candidate_store.selected_candidate_id
        if self._pending_selected_candidate_id is None:
            self._candidate_detail_view.clear()

        self._candidate_population_timer.start(0)

    def _append_candidate_table_chunk(self) -> None:
        if len(self._pending_candidate_rows) == 0:
            self._candidate_population_timer.stop()
            self._candidate_table.resizeColumnsToContents()

            selected_candidate_id = self._pending_selected_candidate_id
            if selected_candidate_id is None:
                return

            try:
                selected_row = self._candidate_ids_by_row.index(selected_candidate_id)
            except ValueError:
                selected_row = 0 if self._candidate_ids_by_row else -1

            if selected_row >= 0:
                self._candidate_table.selectRow(selected_row)
                self._render_candidate_details(self._candidate_ids_by_row[selected_row])

            return

        chunk = self._pending_candidate_rows[:250]
        self._pending_candidate_rows = self._pending_candidate_rows[250:]

        starting_row = self._candidate_table.rowCount()
        self._candidate_table.setRowCount(starting_row + len(chunk))

        for offset, candidate in enumerate(chunk):
            row_index = starting_row + offset
            self._candidate_ids_by_row.append(candidate.candidate_id)

            values = [
                candidate.candidate_id,
                candidate.stage,
                candidate.candidate_family,
                candidate.candidate_source,
                candidate.evaluation_tier,
                candidate.admissibility_state,
                candidate.validity_state,
                "" if candidate.scalar_tie_break is None else f"{candidate.scalar_tie_break:.6g}",
                candidate.rejection_reason or "",
                candidate.created_at_utc or "",
            ]

            for column_index, value in enumerate(values):
                self._candidate_table.setItem(row_index, column_index, QTableWidgetItem(value))

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

        self._candidate_detail_view.setHtml(
            _candidate_detail_html(_candidate_compare_payload(candidate))
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

        self._compare_left_view.setHtml(
            _candidate_overview_html("Left candidate", left_payload)
        )
        self._compare_right_view.setHtml(
            _candidate_overview_html("Right candidate", right_payload)
        )
        self._compare_diff_view.setHtml(_compare_payloads_html(left_payload, right_payload))

    def _populate_lab_controls(self) -> None:
        self._populate_lab_source_selector()

        self._lab_parameter_table.setRowCount(0)
        self._lab_result_summary_view.clear()
        self._lab_plot_tabs.clear()

        self._lab_plot_tabs.addTab(
            _build_placeholder_panel("No Reevaluation", "Run a reevaluation to populate lab plots."),
            "Edited vs Observed",
        )

        if self._artifacts is None:
            self._lab_status_label.setText(
                "Load a run to edit fitted model parameters and run simulation-backed reevaluation."
            )
            return

        self._lab_status_label.setText(
            "Open the Lab tab to prepare editable parameter controls."
        )

    def _populate_lab_source_selector(self) -> None:
        self._lab_source_selector.blockSignals(True)
        self._lab_source_selector.clear()

        if self._candidate_store is not None:
            self._lab_source_selector.addItems(
                [candidate.candidate_id for candidate in self._candidate_store.candidates]
            )

        self._lab_source_selector.blockSignals(False)

    def _ensure_lab_parameters_loaded(self) -> None:
        if self._artifacts is None:
            return

        if self._lab_parameter_table.rowCount() > 0:
            return

        self._lab_status_label.setText("Preparing editable fitted-model parameters.")
        self._populate_lab_parameter_table()

    def _populate_lab_parameter_table(self) -> None:
        artifacts = self._artifacts
        if artifacts is None:
            self._lab_parameter_table.setRowCount(0)
            return

        fit_context = load_fit_context(artifacts)
        if fit_context is None:
            self._lab_parameter_table.setRowCount(0)
            self._lab_status_label.setText(
                "The current run does not contain enough fit context for lab reevaluation."
            )

            return

        parameter_leaves = flatten_parameter_tree(fit_context.model.fitted_parameters)
        self._lab_parameter_table.setRowCount(len(parameter_leaves))
        for row_index, parameter_leaf in enumerate(parameter_leaves):
            path_item = QTableWidgetItem(parameter_leaf.path)
            path_item.setFlags(path_item.flags() & ~Qt.ItemFlag.ItemIsEditable)
            self._lab_parameter_table.setItem(row_index, 0, path_item)

            block_item = QTableWidgetItem(parameter_leaf.block_id)
            block_item.setFlags(block_item.flags() & ~Qt.ItemFlag.ItemIsEditable)
            self._lab_parameter_table.setItem(row_index, 1, block_item)

            baseline_item = QTableWidgetItem(str(parameter_leaf.value))
            baseline_item.setFlags(baseline_item.flags() & ~Qt.ItemFlag.ItemIsEditable)

            self._lab_parameter_table.setItem(row_index, 2, baseline_item)
            self._lab_parameter_table.setItem(row_index, 3, QTableWidgetItem(str(parameter_leaf.value)))

        self._configure_lab_parameter_table_columns()
        self._lab_status_label.setText(
            "Edit scalar fitted-model parameters and run reevaluation against observed data."
        )

    def _configure_lab_parameter_table_columns(self) -> None:
        header = self._lab_parameter_table.horizontalHeader()
        header.setStretchLastSection(False)
        header.setSectionResizeMode(0, QHeaderView.ResizeMode.Stretch)
        header.setSectionResizeMode(1, QHeaderView.ResizeMode.Interactive)
        header.setSectionResizeMode(2, QHeaderView.ResizeMode.Interactive)
        header.setSectionResizeMode(3, QHeaderView.ResizeMode.Interactive)

        self._lab_parameter_table.setColumnWidth(1, 120)
        self._lab_parameter_table.setColumnWidth(2, 140)
        self._lab_parameter_table.setColumnWidth(3, 180)

    def _reset_lab_edits(self) -> None:
        for row_index in range(self._lab_parameter_table.rowCount()):
            baseline_item = self._lab_parameter_table.item(row_index, 2)
            edited_item = self._lab_parameter_table.item(row_index, 3)

            if baseline_item is None or edited_item is None:
                continue

            edited_item.setText(baseline_item.text())

        self._lab_result_summary_view.clear()
        self._lab_status_label.setText("Reset lab edits to fitted-model baseline values.")

    def _start_lab_reevaluation(self) -> None:
        artifacts = self._artifacts
        if artifacts is None or self._candidate_store is None:
            return

        self._stop_lab_worker()
        self._ensure_lab_parameters_loaded()

        fit_context = load_fit_context(artifacts)
        if fit_context is None:
            self._lab_status_label.setText(
                "The current run does not contain enough fit context for lab reevaluation."
            )
            return

        source_candidate_id = self._lab_source_selector.currentText() or (
            self._candidate_store.selected_candidate_id or ""
        )
        if source_candidate_id == "":
            self._lab_status_label.setText("No source candidate is available for reevaluation.")
            return

        parameter_updates = _parameter_updates_from_lab_table(self._lab_parameter_table)
        if len(parameter_updates) == 0:
            self._lab_status_label.setText("No lab edits detected. Running reevaluation with fitted baseline.")

        try:
            simulation_seed = int(self._lab_seed_input.text().strip() or "1")
        except ValueError:
            self._lab_status_label.setText("Lab seed must be an integer.")
            return

        reevaluation_tiers = _selected_lab_tiers(
            exact_selected=self._lab_exact_checkbox.isChecked(),
            seeded_selected=self._lab_seeded_checkbox.isChecked(),
            coarse_selected=self._lab_coarse_checkbox.isChecked(),
            consistency_selected=self._lab_consistency_checkbox.isChecked(),
        )
        edited_parameters = apply_reevaluation_request(
            fit_context.model,
            parameter_updates,
        )

        self._lab_status_label.setText("Starting reevaluation")
        self._lab_thread = QThread(self)
        self._lab_worker = WorkbenchReevaluationWorker(
            artifacts,
            WorkbenchReevaluationRequest(
                run_id=fit_context.model.run_id,
                source_candidate_id=source_candidate_id,
                edited_parameters=edited_parameters,
                edited_parameter_updates=parameter_updates,
                tiers=reevaluation_tiers,
                seed=simulation_seed,
            ),
        )

        self._lab_worker.moveToThread(self._lab_thread)
        self._lab_thread.started.connect(self._lab_worker.run)
        self._lab_worker.status_changed.connect(self._on_lab_status_changed)
        self._lab_worker.completed.connect(self._on_lab_completed)
        self._lab_worker.error_raised.connect(self._on_load_error)
        self._lab_worker.finished.connect(self._lab_thread.quit)
        self._lab_worker.finished.connect(self._lab_worker.deleteLater)
        self._lab_thread.finished.connect(self._lab_thread.deleteLater)

        self._lab_thread.start()

    def _shutdown_background_workers(self) -> None:
        self._artifact_materialization_timer.stop()
        self._candidate_population_timer.stop()
        self._stop_export_worker()
        self._stop_load_worker()
        self._stop_lab_worker()
        self._stop_execution_worker()

    def _stop_load_worker(self) -> None:
        load_worker = self._load_worker
        load_thread = self._load_thread

        if not _qt_object_deleted(load_worker):
            load_worker.cancel()

        _stop_qthread(load_thread)

        self._load_worker = None
        self._load_thread = None

    def _stop_lab_worker(self) -> None:
        lab_worker = self._lab_worker
        lab_thread = self._lab_thread

        if not _qt_object_deleted(lab_worker):
            lab_worker.cancel()

        _stop_qthread(lab_thread)

        self._lab_worker = None
        self._lab_thread = None

    def _stop_execution_worker(self) -> None:
        execution_thread = self._execution_thread

        _stop_qthread(execution_thread)

        self._execution_worker = None
        self._execution_thread = None

    def _stop_export_worker(self) -> None:
        export_worker = self._export_worker
        export_thread = self._export_thread

        if not _qt_object_deleted(export_worker):
            export_worker.cancel()

        _stop_qthread(export_thread)

        self._export_worker = None
        self._export_thread = None
        self._export_plots_action.setEnabled(self._artifacts is not None)

    def _on_lab_status_changed(self, message: str) -> None:
        self._lab_status_label.setText(message)

    def _on_lab_completed(self, result: Any) -> None:
        if not isinstance(result, WorkbenchReevaluationResult):
            return

        self._lab_result = result
        self._lab_status_label.setText(
            f"Reevaluation completed at {result.completed_at_utc}"
        )
        self._lab_result_summary_view.setHtml(_lab_result_html(result))

        self._lab_plot_tabs.clear()
        if self._artifacts is None or result.comparison is None:
            self._lab_plot_tabs.addTab(
                _build_placeholder_panel("No Comparison", "Reevaluation did not produce comparison plots."),
                "Edited vs Observed",
            )

            return

        self._lab_plot_tabs.addTab(
            _figure_canvas(create_observed_simulated_figure(self._artifacts, result.comparison)),
            "Edited Path",
        )
        self._lab_plot_tabs.addTab(
            _figure_canvas(create_td_sd_return_distribution_figure(self._artifacts, result.comparison)),
            "Edited Returns",
        )
        self._lab_plot_tabs.addTab(
            _figure_canvas(create_td_sd_horizon_profile_figure(self._artifacts, result.comparison)),
            "Edited Horizons",
        )

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
        self._artifact_materialization_timer.stop()
        self._artifact_materialization_steps = []
        self._plot_image_bytes_by_task_id.clear()
        self._plot_figure_by_task_id.clear()
        self._plot_canvas_ready_by_task_id.clear()

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

        detail_tabs = QTabWidget()
        detail_tabs.addTab(
            _build_mapping_table_tab(
                {
                    "selected_candidate_id": summary_payload.get("selected_candidate_id"),
                    "target_count": summary_payload.get("target_count"),
                    "candidate_count": summary_payload.get("candidate_count"),
                    "admissible_candidate_count": summary_payload.get("admissible_candidate_count"),
                    "simulation_summary_count": summary_payload.get("simulation_summary_count"),
                    "coarse_tick_metric_count": summary_payload.get("coarse_tick_metric_count"),
                    "runtime_seconds_total": summary_payload.get("runtime_seconds_total"),
                    "maximum_normalized_coarse_tick_delta": summary_payload.get("maximum_normalized_coarse_tick_delta"),
                }
            ),
            "Run",
        )
        detail_tabs.addTab(
            _build_mapping_table_tab(candidate_counts_by_stage),
            "Counts",
        )
        detail_tabs.addTab(
            _build_mapping_table_tab(
                self._artifacts.selection_rationale if self._artifacts else {}
            ),
            "Selection",
        )
        detail_tabs.addTab(
            _build_mapping_table_tab(_model_summary_payload(model_payload)),
            "Parameters",
        )
        detail_tabs.addTab(
            _build_mapping_table_tab(summary_payload.get("metadata", {})),
            "Metadata",
        )

        layout.addWidget(detail_tabs)

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

        detail_tabs = QTabWidget()
        detail_tabs.addTab(
            _build_mapping_table_tab(
                {
                    "run_id": run_state.run_id,
                    "status": run_state.status,
                    "current_stage": run_state.current_stage,
                    "current_subtask": run_state.current_subtask,
                    "current_evaluation_tier": run_state.current_evaluation_tier,
                    "elapsed_seconds": run_state.elapsed_seconds,
                    "observed_stage_reports": len(self._live_stage_report_payloads),
                    "candidate_count": 0 if candidate_store is None else len(candidate_store.candidates),
                }
            ),
            "Run",
        )
        detail_tabs.addTab(
            _build_table_tab(self._live_stage_report_payloads),
            "Stage Reports",
        )

        layout.addWidget(detail_tabs)

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

        self._stop_export_worker()
        self.statusBar().showMessage(f"Exporting plots to {selected_directory}")
        self._export_plots_action.setEnabled(False)

        self._export_thread = QThread(self)
        self._export_worker = WorkbenchPlotExportWorker(
            self._artifacts,
            Path(selected_directory),
        )
        self._export_worker.moveToThread(self._export_thread)

        self._export_thread.started.connect(self._export_worker.run)
        self._export_worker.status_changed.connect(self.statusBar().showMessage)
        self._export_worker.completed.connect(self._on_plot_export_completed)
        self._export_worker.error_raised.connect(self._on_plot_export_failed)
        self._export_worker.finished.connect(self._export_thread.quit)
        self._export_worker.finished.connect(self._export_worker.deleteLater)
        self._export_thread.finished.connect(self._on_plot_export_thread_finished)
        self._export_thread.finished.connect(self._export_thread.deleteLater)

        self._export_thread.start()

    def _on_plot_export_completed(self, written_paths: Any) -> None:
        if not isinstance(written_paths, dict):
            return

        self.statusBar().showMessage(
            f"Wrote {len(written_paths)} plots",
            5000,
        )

    def _on_plot_export_failed(self, message: str) -> None:
        self.statusBar().showMessage("Plot export failed")
        self._state.session.last_error_message = message

        message_box = QMessageBox(self)
        message_box.setWindowTitle("Janiform Calibration Workbench")
        message_box.setText(message)
        message_box.setIcon(QMessageBox.Icon.Critical)
        message_box.exec()

    def _on_plot_export_thread_finished(self) -> None:
        self._export_worker = None
        self._export_thread = None
        self._export_plots_action.setEnabled(self._artifacts is not None)

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


def _build_mapping_table_tab(mapping: dict[str, Any]) -> QWidget:
    rows = [
        {
            "field": key,
            "value": value,
        }
        for key, value in sorted(_flatten_mapping(mapping).items())
    ]

    return _build_table_tab(rows)


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


def _build_parameter_search_plot_panel(artifacts: WorkbenchArtifacts) -> QWidget:
    parameter_names = _parameter_names_from_artifacts(artifacts)
    if len(parameter_names) < 2:
        return _figure_canvas(create_parameter_search_figure(artifacts))

    left_selector = QComboBox()
    right_selector = QComboBox()

    left_selector.addItems(parameter_names)
    right_selector.addItems(parameter_names)

    if len(parameter_names) > 1:
        right_selector.setCurrentIndex(1)

    canvas_container = QWidget()

    def render_selected_pair() -> None:
        left_parameter = left_selector.currentText()
        right_parameter = right_selector.currentText()

        self_contained_canvas = _figure_canvas(
            create_parameter_search_figure(
                artifacts,
                left_parameter=left_parameter,
                right_parameter=right_parameter,
            )
        )
        _replace_widget_contents(canvas_container, self_contained_canvas)

    left_selector.currentIndexChanged.connect(lambda _: render_selected_pair())
    right_selector.currentIndexChanged.connect(lambda _: render_selected_pair())

    render_selected_pair()

    selector_row = QWidget()
    selector_layout = QHBoxLayout(selector_row)
    selector_layout.addWidget(QLabel("X parameter"))
    selector_layout.addWidget(left_selector)
    selector_layout.addWidget(QLabel("Y parameter"))
    selector_layout.addWidget(right_selector)

    container = QWidget()
    layout = QVBoxLayout(container)
    layout.addWidget(selector_row)
    layout.addWidget(canvas_container)

    return container


def _build_parameter_objective_plot_panel(artifacts: WorkbenchArtifacts) -> QWidget:
    parameter_count = len(_parameter_names_from_artifacts(artifacts))
    if parameter_count == 0:
        return _figure_canvas(
            create_parameter_objective_figure_with_columns(
                artifacts,
                column_count=1,
            )
        )

    return _ResponsiveFigurePanel(
        layout_key_for_width=lambda width: _parameter_objective_column_count_for_width(
            width,
            parameter_count,
        ),
        figure_builder=lambda column_count: create_parameter_objective_figure_with_columns(
            artifacts,
            column_count=column_count,
        ),
    )


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
    return _ResponsiveFigureCanvas(figure)


class _WidthScaledImageView(QScrollArea):
    def __init__(self, image_bytes: bytes) -> None:
        super().__init__()

        self._source_pixmap = QPixmap()
        self._source_pixmap.loadFromData(image_bytes)

        self._image_label = QLabel()
        self._image_label.setAlignment(Qt.AlignmentFlag.AlignTop | Qt.AlignmentFlag.AlignHCenter)

        container = QWidget()
        container_layout = QVBoxLayout(container)
        container_layout.addWidget(self._image_label)

        self.setWidget(container)
        self.setWidgetResizable(True)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.setMinimumSize(0, 0)
        self.setSizePolicy(
            QSizePolicy.Policy.Expanding,
            QSizePolicy.Policy.Expanding,
        )

        self._refresh_pixmap()

    @override
    def resizeEvent(self, event: QResizeEvent) -> None:
        super().resizeEvent(event)
        self._refresh_pixmap()

    def _refresh_pixmap(self) -> None:
        if self._source_pixmap.isNull():
            self._image_label.clear()
            return

        viewport_width = self.viewport().width()
        if viewport_width <= 0:
            return

        scaled_pixmap = self._source_pixmap.scaledToWidth(
            viewport_width,
            Qt.TransformationMode.SmoothTransformation,
        )
        self._image_label.setPixmap(scaled_pixmap)
        self._image_label.resize(scaled_pixmap.size())


def _image_canvas(image_bytes: bytes) -> QWidget:
    return _WidthScaledImageView(image_bytes)


class _ResponsiveFigurePanel(QWidget):
    def __init__(
        self,
        *,
        layout_key_for_width: Callable[[int], int],
        figure_builder: Callable[[int], Figure],
    ) -> None:
        super().__init__()

        self._layout_key_for_width: Callable[[int], int] = layout_key_for_width
        self._figure_builder: Callable[[int], Figure] = figure_builder
        self._last_layout_key: Optional[int] = None
        self._rerender_timer = QTimer(self)
        self._rerender_timer.setSingleShot(True)
        self._rerender_timer.timeout.connect(self._render_current_layout)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)

    @override
    def showEvent(self, event: QShowEvent) -> None:
        super().showEvent(event)
        self._schedule_rerender()

    @override
    def resizeEvent(self, event: QResizeEvent) -> None:
        super().resizeEvent(event)
        self._schedule_rerender()

    def _schedule_rerender(self) -> None:
        self._rerender_timer.start(80)

    def _render_current_layout(self) -> None:
        available_width = max(1, self.width())
        layout_key = self._layout_key_for_width(available_width)
        if self._last_layout_key == layout_key:
            return

        self._last_layout_key = layout_key
        figure = self._figure_builder(layout_key)

        _replace_widget_contents(self, _figure_canvas(figure))


class _ResponsiveFigureCanvas(QWidget):
    def __init__(self, figure: Figure) -> None:
        super().__init__()

        self._figure: Figure = figure
        self._canvas = FigureCanvasQTAgg(figure)
        self._last_canvas_size: Optional[tuple[int, int]] = None
        self._rerender_timer = QTimer(self)
        self._rerender_timer.setSingleShot(True)
        self._rerender_timer.timeout.connect(self._resize_figure_to_viewport)

        self._canvas.setSizePolicy(
            QSizePolicy.Policy.Expanding,
            QSizePolicy.Policy.Expanding,
        )

        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.addWidget(self._canvas)

    @override
    def showEvent(self, event: QShowEvent) -> None:
        super().showEvent(event)
        self._schedule_resize()

    @override
    def resizeEvent(self, event: QResizeEvent) -> None:
        super().resizeEvent(event)
        self._schedule_resize()

    def _schedule_resize(self) -> None:
        self._rerender_timer.start(0)

    def _resize_figure_to_viewport(self) -> None:
        canvas_width = max(1, self._canvas.width())
        canvas_height = max(1, self._canvas.height())
        canvas_size = (canvas_width, canvas_height)
        if self._last_canvas_size == canvas_size:
            return

        self._last_canvas_size = canvas_size

        dpi = max(float(self._figure.get_dpi()), 1.0)
        self._figure.set_size_inches(
            canvas_width / dpi,
            canvas_height / dpi,
            forward=False,
        )
        self._canvas.draw_idle()


def _replace_widget_contents(container: QWidget, child: QWidget) -> None:
    layout = container.layout()
    if layout is None:
        layout = QVBoxLayout(container)

    while layout.count() > 0:
        item = layout.takeAt(0)
        existing_widget = item.widget()
        if existing_widget is not None:
            existing_widget.deleteLater()

    layout.addWidget(child)


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


def _parameter_names_from_artifacts(artifacts: WorkbenchArtifacts) -> list[str]:
    frontier_entries = _artifact_rows(artifacts.candidate_frontier, "frontier_entries")
    ordered_parameter_names: dict[str, None] = {}
    for entry in frontier_entries:
        canonical_coordinates = entry.get("canonical_coordinates")

        if isinstance(canonical_coordinates, dict):
            for parameter_name, parameter_value in canonical_coordinates.items():
                if isinstance(parameter_name, str) and isinstance(parameter_value, int | float):
                    ordered_parameter_names.setdefault(parameter_name, None)
        elif isinstance(canonical_coordinates, list):
            for raw_coordinate in canonical_coordinates:
                if (
                    isinstance(raw_coordinate, list | tuple)
                    and len(raw_coordinate) == 2
                    and isinstance(raw_coordinate[0], str)
                    and isinstance(raw_coordinate[1], int | float)
                ):
                    ordered_parameter_names.setdefault(raw_coordinate[0], None)

    return sorted(ordered_parameter_names)


def _parameter_objective_column_count_for_width(
    width: int,
    parameter_count: int,
) -> int:
    target_plot_width = 560
    return max(1, min(parameter_count, width // target_plot_width or 1))


def _plot_group_for_manifest_entry(
    manifest_entry: WorkbenchPlotManifestEntry,
) -> tuple[str, str]:
    plot_id = manifest_entry.plot_id
    family_id = manifest_entry.family_id

    if plot_id == "candidate_compare":
        return "compare", "Compare"

    if family_id in {"candidate_diagnostics", "run_progress"}:
        return "overview", "Overview"

    if family_id in {"search_frontier", "parameter_objective", "parameter_search", "consistency"}:
        return "search", "Search"

    if family_id in {"td_sd_activity", "td_sd_returns", "td_sd_profiles"}:
        return "fit_quality", "Observed vs Simulated"

    return "other", "Other"


def _qt_object_deleted(instance: Optional[Any]) -> bool:
    if instance is None:
        return True

    return sip.isdeleted(instance)


def _stop_qthread(thread: Optional[QThread]) -> None:
    if _qt_object_deleted(thread):
        return

    try:
        thread.quit()
        if not thread.wait(800):
            thread.terminate()
            thread.wait(200)
    except RuntimeError:
        return


def _model_summary_payload(model_payload: dict[str, Any]) -> dict[str, Any]:
    if not isinstance(model_payload, dict):
        return {}

    parameters = model_payload.get("parameters", {})
    if not isinstance(parameters, dict):
        return {}

    parameter_summary: dict[str, Any] = {}
    for stage_name, stage_payload in sorted(parameters.items()):
        if not isinstance(stage_name, str) or not isinstance(stage_payload, dict):
            continue

        fitted_parameters = stage_payload.get("fitted_parameters")
        stage_summary = {
            "selected_candidate_id": stage_payload.get("selected_candidate_id"),
            "field_count": len(_flatten_mapping(stage_payload)),
        }
        if isinstance(fitted_parameters, dict):
            stage_summary["fitted_parameter_count"] = len(_flatten_mapping(fitted_parameters))

        parameter_summary[stage_name] = stage_summary

    return parameter_summary


def _candidate_compare_payload(candidate: Any) -> dict[str, Any]:
    return {
        "candidate_id": candidate.candidate_id,
        "stage": candidate.stage,
        "candidate_family": candidate.candidate_family,
        "candidate_source": candidate.candidate_source,
        "evaluation_tier": candidate.evaluation_tier,
        "admissible": candidate.admissible,
        "valid": candidate.valid,
        "admissibility_state": candidate.admissibility_state,
        "validity_state": candidate.validity_state,
        "pareto_rank": candidate.pareto_rank,
        "lexicographic_rank": candidate.lexicographic_rank,
        "scalar_tie_break": candidate.scalar_tie_break,
        "rejection_reason": candidate.rejection_reason,
        "objective_summary": candidate.objective_summary,
        "canonical_coordinates": candidate.canonical_coordinates,
        "parameter_snapshot": candidate.parameter_snapshot,
        "family_metrics": [metric.raw_payload for metric in candidate.family_metrics],
        "simulation_summary": candidate.simulation_summary,
        "consistency_summary": candidate.consistency_summary,
        "lineage": candidate.lineage,
        "runtime_summary": candidate.runtime_summary,
        "created_at_utc": candidate.created_at_utc,
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


def _candidate_overview_html(title: str, payload: dict[str, Any]) -> str:
    sections = [
        f"<h3>{title}</h3>",
        _html_mapping_table(
            {
                "candidate_id": payload.get("candidate_id"),
                "stage": payload.get("stage"),
                "candidate_family": payload.get("candidate_family"),
                "candidate_source": payload.get("candidate_source"),
                "evaluation_tier": payload.get("evaluation_tier"),
                "admissible": payload.get("admissible"),
                "valid": payload.get("valid"),
                "admissibility_state": payload.get("admissibility_state"),
                "validity_state": payload.get("validity_state"),
                "pareto_rank": payload.get("pareto_rank"),
                "lexicographic_rank": payload.get("lexicographic_rank"),
                "scalar_tie_break": payload.get("scalar_tie_break"),
                "rejection_reason": payload.get("rejection_reason"),
                "created_at_utc": payload.get("created_at_utc"),
            },
            label="Overview",
        ),
        _html_mapping_table(
            payload.get("objective_summary", {}),
            label="Objective summary",
        ),
        _html_mapping_table(
            payload.get("canonical_coordinates", {}),
            label="Canonical coordinates",
        ),
        _html_mapping_table(
            payload.get("parameter_snapshot", {}),
            label="Parameter snapshot",
        ),
        _html_mapping_table(
            payload.get("simulation_summary", {}),
            label="Simulation summary",
        ),
        _html_mapping_table(
            payload.get("consistency_summary", {}),
            label="Consistency summary",
        ),
        _html_mapping_table(
            payload.get("lineage", {}),
            label="Lineage",
        ),
        _html_mapping_table(
            payload.get("runtime_summary", {}),
            label="Runtime summary",
        ),
        _html_rows_table(
            _family_metric_rows(payload.get("family_metrics")),
            label="Family metrics",
        ),
    ]

    return "".join(sections)


def _candidate_detail_html(payload: dict[str, Any]) -> str:
    return "".join(
        [
            _candidate_overview_html("Candidate details", payload),
            _html_mapping_table(payload.get("raw_payload", {}), label="Raw payload fields"),
        ]
    )


def _compare_payloads_html(left_payload: dict[str, Any], right_payload: dict[str, Any]) -> str:
    left_flat = _flatten_mapping(left_payload)
    right_flat = _flatten_mapping(right_payload)

    difference_rows = [
        {
            "field": key,
            "left": left_flat.get(key, "<missing>"),
            "right": right_flat.get(key, "<missing>"),
        }
        for key in sorted(set(left_flat) | set(right_flat))
        if left_flat.get(key) != right_flat.get(key)
    ]

    if not difference_rows:
        return "<h3>Differences</h3><p>No differences in the rendered compare payload.</p>"

    return _html_rows_table(difference_rows, label="Differences")


def _html_mapping_table(mapping: Any, *, label: str) -> str:
    flattened_mapping = _flatten_mapping(mapping if isinstance(mapping, dict) else {})

    rows = "".join(
        f"<tr><td><code>{field_name}</code></td><td>{_html_escape(value)}</td></tr>"
        for field_name, value in sorted(flattened_mapping.items())
    )
    if not rows:
        rows = "<tr><td colspan='2'><em>No data</em></td></tr>"

    return (
        f"<h4>{_html_escape(label)}</h4>"
        "<table border='1' cellspacing='0' cellpadding='4'>"
        "<tr><th>Field</th><th>Value</th></tr>"
        f"{rows}"
        "</table>"
    )


def _html_rows_table(rows: list[dict[str, Any]], *, label: str) -> str:
    if not rows:
        return f"<h4>{_html_escape(label)}</h4><p><em>No data</em></p>"

    flattened_rows = [_flatten_mapping(row) for row in rows]

    headers = sorted({key for row in flattened_rows for key in row})
    header_html = "".join(f"<th>{_html_escape(header)}</th>" for header in headers)

    row_html = "".join(
        "<tr>"
        + "".join(
            f"<td>{_html_escape(row.get(header, ''))}</td>"
            for header in headers
        )
        + "</tr>"
        for row in flattened_rows
    )

    return (
        f"<h4>{_html_escape(label)}</h4>"
        "<table border='1' cellspacing='0' cellpadding='4'>"
        f"<tr>{header_html}</tr>"
        f"{row_html}"
        "</table>"
    )


def _family_metric_rows(raw_metrics: Any) -> list[dict[str, Any]]:
    if not isinstance(raw_metrics, list):
        return []

    return [
        metric
        for metric in raw_metrics
        if isinstance(metric, dict)
    ]


def _parameter_updates_from_lab_table(parameter_table: QTableWidget) -> dict[str, Any]:
    updates: dict[str, Any] = {}
    for row_index in range(parameter_table.rowCount()):
        path_item = parameter_table.item(row_index, 0)
        baseline_item = parameter_table.item(row_index, 2)
        edited_item = parameter_table.item(row_index, 3)
        if path_item is None or baseline_item is None or edited_item is None:
            continue

        if edited_item.text() == baseline_item.text():
            continue

        updates[path_item.text()] = parse_parameter_value(
            edited_item.text(),
            None,
        )

    return updates


def _selected_lab_tiers(
    *,
    exact_selected: bool,
    seeded_selected: bool,
    coarse_selected: bool,
    consistency_selected: bool,
) -> tuple[str, ...]:
    tiers: list[str] = []

    if exact_selected:
        tiers.append("exact_rescoring")
    if seeded_selected:
        tiers.append("seeded_tick")
    if coarse_selected:
        tiers.append("coarse_diagnostics")
    if consistency_selected:
        tiers.append("consistency")

    if not tiers:
        return ("seeded_tick", "coarse_diagnostics", "consistency")

    return tuple(tiers)


def _lab_result_html(result: WorkbenchReevaluationResult) -> str:
    return "".join(
        [
            "<h3>Reevaluation Result</h3>",
            _html_mapping_table(
                {
                    "reevaluation_id": result.reevaluation_id,
                    "source_candidate_id": result.source_candidate_id,
                    "status": result.status,
                    "exact_status": result.exact_status,
                    "completed_at_utc": result.completed_at_utc,
                },
                label="Result",
            ),
            _html_mapping_table(
                result.diagnostics_summary,
                label="Diagnostics summary",
            ),
        ]
    )


def _html_escape(value: Any) -> str:
    text = str(value)
    return (
        text.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )


__all__ = ["launch_workbench"]
