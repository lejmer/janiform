"""Shared progress helpers for observed option surface pricing workflows."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
from enum import StrEnum
from queue import Empty
from time import monotonic
from typing import Any, Optional

from ......_workflow_progress import (
    AbstractProgress,
    format_elapsed_seconds,
)


class SingleExpirySurfaceSliceProgressEventKind(StrEnum):
    STARTED = "started"
    PROGRESS = "progress"


class SingleExpirySurfaceSliceProgressStatus(StrEnum):
    QUEUED = "queued"
    RUNNING = "running"
    FAILED = "failed"
    DONE = "done"


@dataclass(frozen=True, slots=True)
class SingleExpirySurfaceSliceProgressMessage:
    valuation_time_utc: datetime
    event_kind: SingleExpirySurfaceSliceProgressEventKind
    detail: Optional[str] = None


@dataclass(slots=True)
class SingleExpirySurfaceSliceProgressRow:
    valuation_time_utc: datetime
    status: SingleExpirySurfaceSliceProgressStatus = SingleExpirySurfaceSliceProgressStatus.QUEUED
    detail: str = "-"
    started_at_monotonic: Optional[float] = None
    last_observed_elapsed_seconds: Optional[float] = None
    completed_elapsed_seconds: Optional[float] = None


class SingleExpirySurfaceSliceWorkerProgress(AbstractProgress):
    def __init__(
        self,
        *,
        valuation_time_utc: datetime,
        progress_queue: Any,
    ) -> None:
        self._valuation_time_utc = valuation_time_utc
        self._progress_queue = progress_queue
        self._started = False
        self._last_detail: Optional[str] = None

    def notify_started(self) -> None:
        if self._started:
            return

        self._progress_queue.put(
            SingleExpirySurfaceSliceProgressMessage(
                valuation_time_utc=self._valuation_time_utc,
                event_kind=SingleExpirySurfaceSliceProgressEventKind.STARTED,
            )
        )

        self._started = True

    def update(
        self,
        *,
        detail: Optional[str] = None,
        subdetail: Optional[str] = None,
        completed_units: Optional[int] = None,
        total_units: Optional[int] = None,
        reset_progress_units: bool = False,
        progress_fraction: Optional[float] = None,
    ) -> None:
        _ = completed_units
        _ = total_units
        _ = reset_progress_units
        _ = progress_fraction

        if detail is None and subdetail is None:
            return

        self.notify_started()

        normalized_detail = normalize_single_expiry_surface_slice_progress_detail(
            subdetail
        )
        if normalized_detail is None or normalized_detail == self._last_detail:
            return

        self._last_detail = normalized_detail

        self._progress_queue.put(
            SingleExpirySurfaceSliceProgressMessage(
                valuation_time_utc=self._valuation_time_utc,
                event_kind=SingleExpirySurfaceSliceProgressEventKind.PROGRESS,
                detail=normalized_detail,
            )
        )

    def __exit__(self, exc_type, exc, exc_tb) -> None:
        return None


def build_single_expiry_surface_progress_panel(
    *,
    rows_by_time: dict[datetime, SingleExpirySurfaceSliceProgressRow],
    active_count: int,
    surface_parallelism: int,
    total_cpu_count: int,
    pricing_cpu_count: int,
) -> str:
    summary = (
        f"active {active_count} | valuation workers {surface_parallelism}/{total_cpu_count} | "
        f"pricing cpus per worker {pricing_cpu_count}"
    )

    row_lines = [
        format_single_expiry_surface_slice_progress_row(row)
        for _, row in sorted(rows_by_time.items())
    ]

    return "\n".join((summary, *row_lines))


def drain_single_expiry_surface_progress_queue(
    *,
    progress_queue: Any,
    rows_by_time: dict[datetime, SingleExpirySurfaceSliceProgressRow],
) -> None:
    while True:
        try:
            message = progress_queue.get_nowait()
        except Empty:
            return

        row = rows_by_time.get(message.valuation_time_utc)
        if row is None:
            continue

        now = monotonic()
        if message.event_kind == SingleExpirySurfaceSliceProgressEventKind.STARTED:
            start_single_expiry_surface_slice_progress_row(row, now=now)
            continue

        if message.event_kind == SingleExpirySurfaceSliceProgressEventKind.PROGRESS:
            start_single_expiry_surface_slice_progress_row(row, now=now)
            if row.started_at_monotonic is not None:
                row.last_observed_elapsed_seconds = now - row.started_at_monotonic
            if message.detail is not None:
                row.detail = message.detail


def fail_single_expiry_surface_slice_progress_row(
    row: SingleExpirySurfaceSliceProgressRow,
) -> None:
    complete_single_expiry_surface_slice_progress_row(
        row,
        status=SingleExpirySurfaceSliceProgressStatus.FAILED,
    )


def complete_single_expiry_surface_slice_progress_row(
    row: SingleExpirySurfaceSliceProgressRow,
    *,
    status: SingleExpirySurfaceSliceProgressStatus = SingleExpirySurfaceSliceProgressStatus.DONE,
) -> None:
    now = monotonic()
    if row.started_at_monotonic is None:
        row.started_at_monotonic = now

    row.status = status
    row.last_observed_elapsed_seconds = now - row.started_at_monotonic
    row.completed_elapsed_seconds = now - row.started_at_monotonic


def format_single_expiry_surface_slice_progress_row(
    row: SingleExpirySurfaceSliceProgressRow,
) -> str:
    valuation_label = format_single_expiry_surface_slice_valuation_time(
        row.valuation_time_utc
    )

    return (
        f"  {valuation_label} | {row.status:<7} | "
        f"{row.detail} | {single_expiry_surface_slice_row_elapsed_label(row)}"
    )


def format_single_expiry_surface_slice_valuation_time(
    valuation_time_utc: datetime,
) -> str:
    return valuation_time_utc.isoformat().replace("+00:00", "Z")


def normalize_single_expiry_surface_slice_progress_detail(
    subdetail: Optional[str],
) -> Optional[str]:
    if subdetail is None:
        return None

    if subdetail.startswith("maturity 1/1 | "):
        return subdetail.split(" | ", maxsplit=1)[1]

    if subdetail == "maturity 1/1" or subdetail.startswith("group "):
        return None

    return subdetail


def single_expiry_surface_slice_row_elapsed_label(
    row: SingleExpirySurfaceSliceProgressRow,
) -> str:
    if row.completed_elapsed_seconds is not None:
        return format_elapsed_seconds(row.completed_elapsed_seconds)

    if row.started_at_monotonic is None:
        return "--:--"

    if row.status == SingleExpirySurfaceSliceProgressStatus.RUNNING:
        return format_elapsed_seconds(monotonic() - row.started_at_monotonic)

    if row.last_observed_elapsed_seconds is not None:
        return format_elapsed_seconds(row.last_observed_elapsed_seconds)

    return "--:--"


def start_single_expiry_surface_slice_progress_row(
    row: SingleExpirySurfaceSliceProgressRow,
    *,
    now: Optional[float] = None,
) -> None:
    if row.started_at_monotonic is None:
        row.started_at_monotonic = monotonic() if now is None else now

    if row.status == SingleExpirySurfaceSliceProgressStatus.QUEUED:
        row.status = SingleExpirySurfaceSliceProgressStatus.RUNNING


__all__ = [
    "SingleExpirySurfaceSliceProgressRow",
    "SingleExpirySurfaceSliceWorkerProgress",
    "build_single_expiry_surface_progress_panel",
    "complete_single_expiry_surface_slice_progress_row",
    "drain_single_expiry_surface_progress_queue",
    "fail_single_expiry_surface_slice_progress_row",
    "start_single_expiry_surface_slice_progress_row",
]
