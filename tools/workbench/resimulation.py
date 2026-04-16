from __future__ import annotations

import multiprocessing
from concurrent.futures import ProcessPoolExecutor
from dataclasses import dataclass
from threading import Event
from typing import Optional

from PyQt6.QtCore import QObject, pyqtSignal

from .artifacts import WorkbenchArtifacts
from .fit_context import build_observed_simulation_comparison
from .loading import workbench_observed_simulation_refresh_task
from .process_pool import await_future_result, terminate_process_pool


@dataclass(frozen=True, slots=True)
class WorkbenchObservedSimulationRefreshResult:
    seed: int
    comparison_available: bool


def _build_observed_simulation_comparison_cache(
    artifacts: WorkbenchArtifacts,
    seed: int,
) -> bool:
    comparison = build_observed_simulation_comparison(artifacts, seed=seed)
    return comparison is not None


class WorkbenchObservedSimulationRefreshWorker(QObject):
    activity_changed = pyqtSignal(str, str)
    task_state_changed = pyqtSignal(str, str, int, int)
    completed = pyqtSignal(object)
    error_raised = pyqtSignal(str)
    finished = pyqtSignal()

    def __init__(
        self,
        artifacts: WorkbenchArtifacts,
        seed: int,
    ) -> None:
        super().__init__()

        self._artifacts: WorkbenchArtifacts = artifacts
        self._seed: int = seed
        self._cancel_requested: Event = Event()
        self._executor: Optional[ProcessPoolExecutor] = None

    def run(self) -> None:
        spawn_context = multiprocessing.get_context("spawn")
        executor: Optional[ProcessPoolExecutor] = None
        refresh_task = workbench_observed_simulation_refresh_task()

        try:
            self.activity_changed.emit(
                refresh_task.title,
                f"Resimulating observed-versus-simulated diagnostics with seed {self._seed}",
            )
            self.task_state_changed.emit(refresh_task.task_id, "running", 0, 1)

            executor = ProcessPoolExecutor(max_workers=1, mp_context=spawn_context)
            self._executor = executor

            comparison_available = await_future_result(
                executor.submit(
                    _build_observed_simulation_comparison_cache,
                    self._artifacts,
                    self._seed,
                ),
                cancel_requested=self._cancel_requested,
            )
            if comparison_available is None or self._cancel_requested.is_set():
                return

            self.task_state_changed.emit(refresh_task.task_id, "completed", 1, 1)
            self.completed.emit(
                WorkbenchObservedSimulationRefreshResult(
                    seed=self._seed,
                    comparison_available=comparison_available,
                )
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


__all__ = [
    "WorkbenchObservedSimulationRefreshResult",
    "WorkbenchObservedSimulationRefreshWorker",
]
