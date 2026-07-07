from __future__ import annotations

from concurrent.futures import Future, ProcessPoolExecutor, TimeoutError
from threading import Event
from typing import Any, Optional


def await_future_result(
    future: Future[Any],
    *,
    cancel_requested: Event,
) -> Optional[Any]:
    while True:
        try:
            return future.result(timeout=0.2)
        except TimeoutError:
            if cancel_requested.is_set():
                future.cancel()
                return None


def terminate_process_pool(executor: ProcessPoolExecutor) -> None:
    executor.shutdown(wait=False, cancel_futures=True)

    # ProcessPoolExecutor does not expose a public hard-stop API for active children.
    # On workbench shutdown we need to ensure pending subprocess workers do not keep the app alive.
    process_map = executor._processes
    if process_map is None:
        return

    for process in list(process_map.values()):
        if process.is_alive():
            process.terminate()


__all__ = [
    "await_future_result",
    "terminate_process_pool",
]
