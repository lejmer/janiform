from __future__ import annotations

from concurrent.futures import ProcessPoolExecutor
from collections.abc import Callable
from dataclasses import dataclass
import multiprocessing
from pathlib import Path
from threading import Event
from typing import Any, Optional

from PyQt6.QtCore import QObject, pyqtSignal

from matplotlib.figure import Figure

from .artifacts import WorkbenchArtifacts, load_workbench_artifacts
from .plot_manifest import workbench_plot_manifest
from .process_pool import await_future_result, terminate_process_pool
from .rendering import render_plot_png_bytes


@dataclass(frozen=True, slots=True)
class WorkbenchTaskDefinition:
    task_id: str
    title: str
    detail: str


@dataclass(frozen=True, slots=True)
class WorkbenchPlotDefinition:
    task_id: str
    tab_title: str
    detail: str
    builder: Callable[..., Figure]
    uses_observed_simulation_comparison: bool = False


_LOAD_ARTIFACTS_TASK = WorkbenchTaskDefinition(
    task_id="load_artifacts",
    title="Load diagnostics bundle",
    detail="Reading the calibration run and resolving its artifact bundle.",
)

_BUILD_SUMMARY_TASK = WorkbenchTaskDefinition(
    task_id="build_summary",
    title="Build dashboard",
    detail="Preparing summary and inspection tabs.",
)

_PLOT_DEFINITIONS: tuple[WorkbenchPlotDefinition, ...] = tuple(
    WorkbenchPlotDefinition(
        task_id=manifest_entry.task_id,
        tab_title=manifest_entry.tab_title,
        detail=manifest_entry.description,
        builder=manifest_entry.builder,
        uses_observed_simulation_comparison=manifest_entry.uses_observed_simulation_comparison,
    )
    for manifest_entry in workbench_plot_manifest()
)


def workbench_startup_tasks() -> tuple[WorkbenchTaskDefinition, ...]:
    return (
        _LOAD_ARTIFACTS_TASK,
        _BUILD_SUMMARY_TASK,
        *(
            WorkbenchTaskDefinition(
                task_id=plot_definition.task_id,
                title=plot_definition.tab_title,
                detail=plot_definition.detail,
            )
            for plot_definition in _PLOT_DEFINITIONS
        ),
    )


def workbench_plot_definitions() -> tuple[WorkbenchPlotDefinition, ...]:
    return _PLOT_DEFINITIONS


def _load_artifacts_in_subprocess(input_path: Path) -> WorkbenchArtifacts:
    return load_workbench_artifacts(input_path)


class WorkbenchLoadWorker(QObject):
    activity_changed = pyqtSignal(str, str)
    task_state_changed = pyqtSignal(str, str, int, int)
    artifacts_ready = pyqtSignal(object)
    plot_ready = pyqtSignal(str, object)
    error_raised = pyqtSignal(str)
    finished = pyqtSignal()

    def __init__(self, input_path: Path) -> None:
        super().__init__()

        self._input_path: Path = input_path
        self._cancel_requested: Event = Event()
        self._executor: Optional[ProcessPoolExecutor] = None

    def run(self) -> None:
        total_task_count = len(workbench_startup_tasks())
        spawn_context = multiprocessing.get_context("spawn")
        executor: Optional[ProcessPoolExecutor] = None

        try:
            executor = ProcessPoolExecutor(
                max_workers=1,
                mp_context=spawn_context,
            )
            self._executor = executor

            self._emit_activity(
                task_title=_LOAD_ARTIFACTS_TASK.title,
                detail=_LOAD_ARTIFACTS_TASK.detail,
            )

            self.task_state_changed.emit(
                _LOAD_ARTIFACTS_TASK.task_id,
                "running",
                0,
                total_task_count,
            )

            artifacts = self._await_future_result(
                executor.submit(
                    _load_artifacts_in_subprocess,
                    self._input_path,
                )
            )
            if artifacts is None:
                return

            self.task_state_changed.emit(
                _LOAD_ARTIFACTS_TASK.task_id,
                "completed",
                1,
                total_task_count,
            )

            self.artifacts_ready.emit(artifacts)

            self._emit_activity(
                task_title=_BUILD_SUMMARY_TASK.title,
                detail=_BUILD_SUMMARY_TASK.detail,
            )

            self.task_state_changed.emit(
                _BUILD_SUMMARY_TASK.task_id,
                "running",
                1,
                total_task_count,
            )

            self.task_state_changed.emit(
                _BUILD_SUMMARY_TASK.task_id,
                "completed",
                2,
                total_task_count,
            )

            completed_task_count = 2
            for plot_definition in _PLOT_DEFINITIONS:
                if self._cancel_requested.is_set():
                    return

                self._emit_activity(
                    task_title=f"Generate {plot_definition.tab_title}",
                    detail=plot_definition.detail,
                )

                self.task_state_changed.emit(
                    plot_definition.task_id,
                    "running",
                    completed_task_count,
                    total_task_count,
                )

                image_bytes = self._await_future_result(
                    executor.submit(
                        render_plot_png_bytes,
                        artifacts,
                        plot_definition.task_id,
                    )
                )
                if image_bytes is None:
                    return

                self.plot_ready.emit(plot_definition.task_id, image_bytes)

                completed_task_count += 1

                self.task_state_changed.emit(
                    plot_definition.task_id,
                    "completed",
                    completed_task_count,
                    total_task_count,
                )

            self._emit_activity(
                task_title="Ready",
                detail="The workbench finished loading diagnostics and plots.",
            )
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

    def _await_future_result(
        self,
        future: Any,
    ) -> Optional[Any]:
        return await_future_result(
            future,
            cancel_requested=self._cancel_requested,
        )

    def _emit_activity(self, *, task_title: str, detail: str) -> None:
        self.activity_changed.emit(task_title, detail)


__all__ = [
    "WorkbenchLoadWorker",
    "WorkbenchPlotDefinition",
    "WorkbenchTaskDefinition",
    "workbench_plot_definitions",
    "workbench_startup_tasks",
]
