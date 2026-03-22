from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

from PyQt6.QtCore import QObject, pyqtSignal

from matplotlib.figure import Figure

from .artifacts import WorkbenchArtifacts, load_workbench_artifacts
from .plot_manifest import workbench_plot_manifest
from .plots import create_message_figure


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
    builder: Callable[[WorkbenchArtifacts], Figure]


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


class WorkbenchLoadWorker(QObject):
    activity_changed = pyqtSignal(str, str)
    task_state_changed = pyqtSignal(str, str, int, int)
    artifacts_ready = pyqtSignal(object)
    plot_ready = pyqtSignal(str, object)
    error_raised = pyqtSignal(str)
    finished = pyqtSignal()

    def __init__(self, input_path: Path) -> None:
        super().__init__()
        self._input_path = input_path

    def run(self) -> None:
        total_task_count = len(workbench_startup_tasks())

        try:
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
            artifacts = load_workbench_artifacts(self._input_path)
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
                try:
                    figure = plot_definition.builder(artifacts)
                    task_state = "completed"
                except Exception as error:
                    figure = create_message_figure(
                        f"{plot_definition.tab_title} Failed",
                        str(error),
                    )
                    task_state = "failed"
                self.plot_ready.emit(plot_definition.task_id, figure)
                completed_task_count += 1
                self.task_state_changed.emit(
                    plot_definition.task_id,
                    task_state,
                    completed_task_count,
                    total_task_count,
                )

            self._emit_activity(
                task_title="Ready",
                detail="The workbench finished loading diagnostics and plots.",
            )
        except Exception as error:
            self.error_raised.emit(str(error))
        finally:
            self.finished.emit()

    def _emit_activity(self, *, task_title: str, detail: str) -> None:
        self.activity_changed.emit(task_title, detail)


__all__ = [
    "WorkbenchLoadWorker",
    "WorkbenchPlotDefinition",
    "WorkbenchTaskDefinition",
    "workbench_plot_definitions",
    "workbench_startup_tasks",
]
