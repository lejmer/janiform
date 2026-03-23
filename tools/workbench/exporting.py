from __future__ import annotations

from concurrent.futures import ProcessPoolExecutor
from dataclasses import dataclass
import multiprocessing
from pathlib import Path
from threading import Event
from typing import Optional

from PyQt6.QtCore import QObject, pyqtSignal

from .artifacts import WorkbenchArtifacts
from .plot_manifest import workbench_plot_manifest
from .process_pool import await_future_result, terminate_process_pool
from .rendering import render_plot_png_bytes


@dataclass(frozen=True, slots=True)
class WorkbenchPlotExportDefinition:
    task_id: str
    output_stem: str
    title: str


_PLOT_EXPORT_DEFINITIONS: tuple[WorkbenchPlotExportDefinition, ...] = (
    WorkbenchPlotExportDefinition("plot_selected_candidate", "selected_candidate_diagnostics", "Selected Candidate"),
    WorkbenchPlotExportDefinition("plot_observed_targets", "observed_targets", "Observed Targets"),
    WorkbenchPlotExportDefinition("plot_frontier", "candidate_frontier", "Frontier"),
    WorkbenchPlotExportDefinition("plot_run_progress", "run_progress", "Run Progress"),
    WorkbenchPlotExportDefinition("plot_runtime", "runtime_profile", "Runtime"),
    WorkbenchPlotExportDefinition("plot_parameter_objective", "parameter_objective", "Param Objective"),
    WorkbenchPlotExportDefinition("plot_parameter_search", "parameter_search", "Param Search"),
    WorkbenchPlotExportDefinition("plot_consistency", "coarse_tick_consistency", "Coarse/Tick"),
    WorkbenchPlotExportDefinition("plot_simulation_summary", "simulation_summary", "Simulation Summary"),
    WorkbenchPlotExportDefinition("plot_search_surfaces", "search_surface_manifest", "Search Surfaces"),
    WorkbenchPlotExportDefinition("plot_observed_simulated", "observed_simulated_comparison", "Paths & Activity"),
    WorkbenchPlotExportDefinition("plot_td_sd_return_distributions", "observed_simulated_return_distributions", "Return Distributions"),
    WorkbenchPlotExportDefinition("plot_td_sd_return_diagnostics", "observed_simulated_distribution_diagnostics", "Distribution Diagnostics"),
    WorkbenchPlotExportDefinition("plot_td_sd_hourly_profiles", "observed_simulated_hourly_profiles", "Hourly Profiles"),
    WorkbenchPlotExportDefinition("plot_td_sd_horizon_profiles", "observed_simulated_horizon_profiles", "Horizon Profiles"),
    WorkbenchPlotExportDefinition("plot_td_sd_autocorrelation", "observed_simulated_autocorrelation", "Autocorrelation"),
    WorkbenchPlotExportDefinition("plot_td_sd_summary_statistics", "observed_simulated_summary_statistics", "Summary Statistics"),
    WorkbenchPlotExportDefinition("plot_candidate_compare", "candidate_compare", "Compare Plot"),
)


class WorkbenchPlotExportWorker(QObject):
    status_changed = pyqtSignal(str)
    completed = pyqtSignal(object)
    error_raised = pyqtSignal(str)
    finished = pyqtSignal()

    def __init__(
        self,
        artifacts: WorkbenchArtifacts,
        output_directory: Path,
    ) -> None:
        super().__init__()
        self._artifacts: WorkbenchArtifacts = artifacts
        self._output_directory: Path = output_directory
        self._cancel_requested: Event = Event()
        self._executor: Optional[ProcessPoolExecutor] = None

    def run(self) -> None:
        spawn_context = multiprocessing.get_context("spawn")
        executor: Optional[ProcessPoolExecutor] = None

        try:
            self._output_directory.mkdir(parents=True, exist_ok=True)
            executor = ProcessPoolExecutor(max_workers=1, mp_context=spawn_context)
            self._executor = executor

            export_definitions = workbench_plot_export_definitions()
            written_paths: dict[str, str] = {}
            total_count = len(export_definitions)
            for index, export_definition in enumerate(export_definitions, start=1):
                if self._cancel_requested.is_set():
                    return

                self.status_changed.emit(
                    f"Exporting {export_definition.title} ({index}/{total_count})"
                )

                image_bytes = await_future_result(
                    executor.submit(
                        render_plot_png_bytes,
                        self._artifacts,
                        export_definition.task_id,
                    ),
                    cancel_requested=self._cancel_requested,
                )
                if image_bytes is None:
                    return

                output_path = self._output_directory / f"{export_definition.output_stem}.png"
                output_path.write_bytes(image_bytes)
                written_paths[export_definition.output_stem] = str(output_path)

            self.completed.emit(written_paths)
        except Exception as error:
            if not self._cancel_requested.is_set():
                self.error_raised.emit(str(error))
        finally:
            if executor is not None:
                if self._cancel_requested.is_set():
                    terminate_process_pool(executor)
                else:
                    executor.shutdown(wait=True, cancel_futures=True)
            self._executor = None
            self.finished.emit()

    def cancel(self) -> None:
        self._cancel_requested.set()

        executor = self._executor
        if executor is not None:
            terminate_process_pool(executor)


def workbench_plot_export_definitions() -> tuple[WorkbenchPlotExportDefinition, ...]:
    manifest_task_ids = {manifest_entry.task_id for manifest_entry in workbench_plot_manifest()}
    return tuple(
        export_definition
        for export_definition in _PLOT_EXPORT_DEFINITIONS
        if export_definition.task_id in manifest_task_ids
    )


__all__ = [
    "WorkbenchPlotExportDefinition",
    "WorkbenchPlotExportWorker",
    "workbench_plot_export_definitions",
]
